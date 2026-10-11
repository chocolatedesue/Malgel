//! The file sidebar: a folder's Markdown and text files as a tree.
//!
//! Folders are listed when first expanded and re-listed in the background
//! when the workspace refreshes, so the tree follows files created, renamed
//! and deleted by other programs.

use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui_kit::{
    AppContext as _, Context, DragMoveEvent, EventEmitter, ExternalPaths, FontWeight,
    InteractiveElement as _, IntoElement, Modifiers, MouseButton, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, UniformListScrollHandle,
    Window,
    assets::IconName,
    component::{
        ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
        button::{Button, ButtonVariants as _},
        checkbox::Checkbox,
        dialog::DialogFooter,
        h_flex,
        scroll::ScrollableElement as _,
        v_flex,
    },
    div,
    prelude::FluentBuilder as _,
    px, uniform_list,
};

use crate::workspace::DragGhost;

use crate::actions::OpenFolder;

/// Entries hidden by default, matching VS Code's `files.exclude`.
const DEFAULT_EXCLUDED: &[&str] = &[".git", ".svn", ".hg", "CVS", ".DS_Store", "Thumbs.db"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

struct Row {
    entry: Entry,
    depth: usize,
}

pub enum FileTreeEvent {
    Open(PathBuf),
    /// Files dropped onto the tree were copied into a folder.
    Copied {
        copied: usize,
        failed: usize,
    },
    /// An entry (and everything below it) was moved within the tree.
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
}

/// A file or folder being dragged inside the tree, to move it the way
/// VS Code moves entries within the explorer.
#[derive(Clone)]
pub struct TreeDrag {
    pub path: PathBuf,
}

/// Whether the drag-and-drop invert key is held while a tree entry drags:
/// Ctrl on Windows and Linux, Option on macOS. Held, an in-app drag copies
/// instead of moving, as in VS Code.
fn invert_key_held(modifiers: &Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.alt
    } else {
        modifiers.control
    }
}

/// Whether `source` may be dropped into `target_dir`. VS Code rejects
/// invalid targets while dragging, not at drop time: a folder can't go
/// into itself or a descendant, and a drop in the folder an entry already
/// sits in would be a no-op.
fn can_drop_into(source: &Path, target_dir: &Path) -> bool {
    source.parent() != Some(target_dir) && !(source.is_dir() && target_dir.starts_with(source))
}

pub struct FileTree {
    root: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    listings: HashMap<PathBuf, Vec<Entry>>,
    rows: Vec<Row>,
    /// The file the workspace is showing.
    active: Option<PathBuf>,
    /// Whether dotfiles and the default-excluded entries are listed.
    show_hidden: bool,
    /// Shared with the drag-over styles, which run without access to the
    /// tree's state: whether the invert key is held right now.
    drag_copy: Rc<Cell<bool>>,
    scroll: UniformListScrollHandle,
    refresh_task: Option<Task<()>>,
}

impl EventEmitter<FileTreeEvent> for FileTree {}

impl FileTree {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let this = Self {
            root: None,
            expanded: HashSet::new(),
            listings: HashMap::new(),
            rows: Vec::new(),
            active: None,
            show_hidden: crate::themes::AppSettings::get(cx).show_hidden_entries,
            drag_copy: Rc::new(Cell::new(false)),
            scroll: UniformListScrollHandle::new(),
            refresh_task: None,
        };
        cx.observe_global::<crate::themes::AppSettings>(|this, cx| {
            this.set_show_hidden(crate::themes::AppSettings::get(cx).show_hidden_entries, cx);
        })
        .detach();
        this
    }

    /// Apply the "show hidden entries" setting and re-list.
    pub fn set_show_hidden(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_hidden == show {
            return;
        }
        self.show_hidden = show;
        self.refresh(cx);
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Show `root`, or nothing.
    pub fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.root == root {
            return;
        }
        self.root = root;
        self.expanded.clear();
        self.listings.clear();
        self.rows.clear();
        if let Some(root) = self.root.clone() {
            self.expanded.insert(root);
        }
        self.refresh(cx);
        cx.notify();
    }

    /// Highlight `path`, expanding the folders above it.
    pub fn set_active(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.active == path {
            return;
        }
        self.active = path;
        let mut revealed = false;
        if let (Some(root), Some(active)) = (&self.root, &self.active) {
            let mut dir = active.parent();
            while let Some(folder) = dir {
                if !folder.starts_with(root) {
                    break;
                }
                revealed |= self.expanded.insert(folder.to_path_buf());
                dir = folder.parent();
            }
        }
        if revealed {
            self.refresh(cx);
        } else {
            self.scroll_to_active();
        }
        cx.notify();
    }

    /// Re-list the root and every expanded folder in the background.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_task.is_some() {
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let folders: Vec<PathBuf> = self
            .expanded
            .iter()
            .filter(|folder| folder.starts_with(&root))
            .cloned()
            .collect();
        let show_hidden = self.show_hidden;
        let listing = cx.background_executor().spawn(async move {
            folders
                .into_iter()
                .filter_map(|folder| list(&folder, show_hidden).map(|entries| (folder, entries)))
                .collect::<HashMap<_, _>>()
        });
        self.refresh_task = Some(cx.spawn(async move |this, cx| {
            let listings = listing.await;
            _ = this.update(cx, |this, cx| {
                this.refresh_task = None;
                // Folders that disappeared collapse.
                this.expanded.retain(|folder| listings.contains_key(folder));
                if listings != this.listings {
                    this.listings = listings;
                    this.rebuild_rows();
                    this.scroll_to_active();
                    cx.notify();
                }
            });
        }));
    }

    fn rebuild_rows(&mut self) {
        fn add(tree: &FileTree, folder: &Path, depth: usize, rows: &mut Vec<Row>) {
            for entry in tree.listings.get(folder).into_iter().flatten() {
                rows.push(Row {
                    entry: entry.clone(),
                    depth,
                });
                if entry.is_dir && tree.expanded.contains(&entry.path) {
                    add(tree, &entry.path, depth + 1, rows);
                }
            }
        }
        let mut rows = Vec::new();
        if let Some(root) = &self.root {
            add(self, root, 0, &mut rows);
        }
        self.rows = rows;
    }

    fn scroll_to_active(&self) {
        if let Some(ix) = self
            .rows
            .iter()
            .position(|row| Some(&row.entry.path) == self.active.as_ref())
        {
            self.scroll
                .scroll_to_item(ix, gpui_kit::ScrollStrategy::Nearest);
        }
    }

    fn toggle(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.expanded.remove(&path) {
            self.expanded.insert(path.clone());
            if !self.listings.contains_key(&path) {
                self.refresh(cx);
                return;
            }
        }
        self.rebuild_rows();
        cx.notify();
    }

    /// Copy the dropped paths into `target` (VS Code copies an OS drop into
    /// the folder it lands on), then re-list and report the outcome.
    fn copy_paths_into(&mut self, target: &Path, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let (mut copied, mut failed) = (0usize, 0usize);
        for src in paths.paths() {
            match copy_entry(src, target) {
                Ok(()) => copied += 1,
                Err(_) => failed += 1,
            }
        }
        if copied > 0 {
            self.refresh(cx);
        }
        cx.notify();
        cx.emit(FileTreeEvent::Copied { copied, failed });
    }

    /// A drop on the tree's empty space lands in the root folder.
    fn drop_into_root(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        match self.root.clone() {
            Some(root) => self.copy_paths_into(&root, paths, cx),
            None => cx.emit(FileTreeEvent::Copied {
                copied: 0,
                failed: paths.paths().len(),
            }),
        }
    }

    /// A tree entry dropped on empty space lands in the root folder
    /// (copied with the invert key held, otherwise moved).
    fn drop_entry_into_root(&mut self, source: &Path, window: &mut Window, cx: &mut Context<Self>) {
        match self.root.clone() {
            Some(root) => self.drop_tree_entry(source, &root, window, cx),
            None => cx.emit(FileTreeEvent::Copied {
                copied: 0,
                failed: 1,
            }),
        }
    }

    /// A tree entry dropped on a file row: VS Code rejects the drop; with
    /// the invert key held it copies into the file's folder.
    fn drop_onto_file(&mut self, source: &Path, file_dir: &Path, cx: &mut Context<Self>) {
        if !self.drag_copy.get() || !can_drop_into(source, file_dir) {
            return;
        }
        match copy_entry(source, file_dir) {
            Ok(()) => {
                self.refresh(cx);
                cx.emit(FileTreeEvent::Copied {
                    copied: 1,
                    failed: 0,
                });
            }
            Err(_) => cx.emit(FileTreeEvent::Copied {
                copied: 0,
                failed: 1,
            }),
        }
    }

    /// Record whether the invert key is held right now, so the drag-over
    /// styles (which run without tree state) can reflect copy intent, and
    /// re-render when it flips.
    fn note_drag_intent(&self, modifiers: &Modifiers, cx: &mut Context<Self>) {
        let copy = invert_key_held(modifiers);
        if self.drag_copy.get() != copy {
            self.drag_copy.set(copy);
            cx.notify();
        }
    }

    /// Land a dragged tree entry in `target_dir`: a copy with the invert
    /// key held, otherwise a move (confirmed first, as in VS Code).
    fn drop_tree_entry(
        &mut self,
        source: &Path,
        target_dir: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !can_drop_into(source, target_dir) {
            // An invalid target stays rejected, exactly as VS Code.
            return;
        }
        if self.drag_copy.get() {
            match copy_entry(source, target_dir) {
                Ok(()) => {
                    self.refresh(cx);
                    cx.emit(FileTreeEvent::Copied {
                        copied: 1,
                        failed: 0,
                    });
                }
                Err(_) => cx.emit(FileTreeEvent::Copied {
                    copied: 0,
                    failed: 1,
                }),
            }
            return;
        }
        if crate::themes::AppSettings::get(cx).confirm_drag_and_drop {
            self.confirm_move(source.to_path_buf(), target_dir.to_path_buf(), window, cx);
        } else {
            self.apply_move(source, target_dir, cx);
        }
    }

    /// Move a tree entry into `target_dir` (VS Code moves entries dragged
    /// within the explorer), then re-list and report the outcome.
    fn apply_move(&mut self, source: &Path, target_dir: &Path, cx: &mut Context<Self>) {
        let Some(name) = source.file_name() else {
            return;
        };
        let dest = target_dir.join(name);
        if source == dest || source.parent() == Some(target_dir) {
            // Already there: nothing to do, as in VS Code.
            return;
        }
        // A folder can't go inside itself or one of its descendants.
        if source.is_dir() && target_dir.starts_with(source) {
            cx.emit(FileTreeEvent::Copied {
                copied: 0,
                failed: 1,
            });
            return;
        }
        match move_path(source, &dest) {
            Ok(()) => {
                self.refresh(cx);
                cx.emit(FileTreeEvent::Moved {
                    from: source.to_path_buf(),
                    to: dest,
                });
            }
            Err(_) => cx.emit(FileTreeEvent::Copied {
                copied: 0,
                failed: 1,
            }),
        }
    }

    /// Ask before moving a dragged entry, with a "don't ask again" box
    /// that mirrors VS Code's `explorer.confirmDragAndDrop`.
    fn confirm_move(
        &mut self,
        source: PathBuf,
        target_dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let source_name = display_name(&source);
        let target_name = display_name(&target_dir);
        let dont_ask = Rc::new(Cell::new(false));
        let tree = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, _cx| {
            let (checkbox, remember) = (dont_ask.clone(), dont_ask.clone());
            let (tree_move, source_move, target_move) =
                (tree.clone(), source.clone(), target_dir.clone());
            alert
                .title(format!("Move “{source_name}” into “{target_name}”?"))
                .description(
                    v_flex().gap_3().child(
                        Checkbox::new("dont-ask-move")
                            .label("Don’t ask again when moving files")
                            .checked(dont_ask.get())
                            .on_click(move |checked, window, _| {
                                checkbox.set(*checked);
                                window.refresh();
                            }),
                    ),
                )
                .footer(
                    DialogFooter::new()
                        .justify_end()
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("move").label("Move").primary().on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                if remember.get() {
                                    crate::themes::AppSettings::update(cx, |settings| {
                                        settings.confirm_drag_and_drop = false
                                    });
                                }
                                _ = tree_move.update(cx, |tree, cx| {
                                    tree.apply_move(&source_move, &target_move, cx)
                                });
                            },
                        )),
                )
        });
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui_kit::AnyElement> {
        let theme = cx.theme();
        let (hover, active_bg, muted) =
            (theme.list_hover, theme.list_active, theme.muted_foreground);
        range
            .filter_map(|ix| {
                let row = self.rows.get(ix)?;
                let entry = row.entry.clone();
                let is_active = Some(&entry.path) == self.active.as_ref();
                let expanded = entry.is_dir && self.expanded.contains(&entry.path);
                let icon = match (entry.is_dir, expanded) {
                    (true, true) => IconName::FolderOpen,
                    (true, false) => IconName::Folder,
                    (false, _) => IconName::FileText,
                };
                let path = entry.path.clone();
                let drop_path = entry.path.clone();
                let drag_name = entry.name.clone();
                Some(
                    h_flex()
                        .id(ix)
                        .h(px(26.))
                        .pl(px(8. + 14. * row.depth as f32))
                        .pr_2()
                        .gap_1p5()
                        .rounded_md()
                        .text_sm()
                        .cursor_pointer()
                        .when(is_active, |this| {
                            this.bg(active_bg).font_weight(FontWeight::MEDIUM)
                        })
                        .when(!is_active, |this| this.hover(move |style| style.bg(hover)))
                        // Entries drag within the tree to move, the way
                        // VS Code's explorer does.
                        .on_drag(TreeDrag { path: path.clone() }, move |_, _, _, cx| {
                            cx.new(|_| DragGhost {
                                label: drag_name.clone().into(),
                            })
                        })
                        .child(div().w(px(12.)).flex_none().when(entry.is_dir, |this| {
                            this.child(
                                Icon::new(if expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .xsmall()
                                .text_color(muted),
                            )
                        }))
                        .child(Icon::new(icon).small().text_color(muted).flex_none())
                        .child(
                            div()
                                .truncate()
                                .child(SharedString::from(entry.name.clone())),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                if entry.is_dir {
                                    this.toggle(path.clone(), cx);
                                } else {
                                    cx.emit(FileTreeEvent::Open(path.clone()));
                                }
                            }),
                        )
                        // A drop on a folder copies an OS drop into it, and
                        // moves a tree entry into it (a copy with the
                        // invert key held). Illegal targets stay
                        // unhighlighted and reject the drop, as in VS Code.
                        .when(entry.is_dir, |this| {
                            let folder = drop_path.clone();
                            let folder_for_drop = folder.clone();
                            let folder_for_style = folder.clone();
                            this.on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                                this.copy_paths_into(&folder, paths, cx)
                            }))
                            .drag_over::<ExternalPaths>(|style, _, _, cx| {
                                style.bg(cx.theme().drop_target)
                            })
                            .on_drop(cx.listener(move |this, drag: &TreeDrag, window, cx| {
                                this.drop_tree_entry(&drag.path, &folder_for_drop, window, cx)
                            }))
                            .drag_over::<TreeDrag>(
                                move |style, drag: &TreeDrag, _, cx| {
                                    if can_drop_into(&drag.path, &folder_for_style) {
                                        style.bg(cx.theme().drop_target)
                                    } else {
                                        style
                                    }
                                },
                            )
                        })
                        // A drop on a file row is rejected, as in VS Code;
                        // with the invert key it copies into the file's
                        // folder.
                        .when(!entry.is_dir, |this| {
                            let parent = entry.path.parent().map(Path::to_path_buf);
                            let parent_for_style = parent.clone();
                            let intent = self.drag_copy.clone();
                            this.on_drop(cx.listener(move |this, drag: &TreeDrag, _, cx| {
                                if let Some(parent) = parent.clone() {
                                    this.drop_onto_file(&drag.path, &parent, cx);
                                }
                            }))
                            .drag_over::<TreeDrag>(
                                move |style, drag: &TreeDrag, _, cx| {
                                    let copy = intent.get();
                                    if copy
                                        && parent_for_style
                                            .as_deref()
                                            .is_some_and(|parent| can_drop_into(&drag.path, parent))
                                    {
                                        style.bg(cx.theme().drop_target)
                                    } else {
                                        style
                                    }
                                },
                            )
                        })
                        .into_any_element(),
                )
            })
            .collect()
    }
}

impl Render for FileTree {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let name = self
            .root
            .as_ref()
            .and_then(|root| root.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "Files".to_string());

        let header = h_flex()
            .h(px(32.))
            .pl_3()
            .pr_1()
            .flex_none()
            .justify_between()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.muted_foreground)
            .child(div().truncate().child(name.to_uppercase()))
            .child(
                Button::new("open-folder")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(IconName::FolderOpen))
                    .tooltip("Open folder…")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenFolder), cx)),
            );

        let body = if self.root.is_none() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .p_4()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("No folder open")
                .child(
                    Button::new("open-folder-empty")
                        .small()
                        .label("Open folder…")
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenFolder), cx)),
                )
                .into_any_element()
        } else if self.rows.is_empty() && self.refresh_task.is_none() {
            div()
                .p_4()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("No Markdown files here")
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .px_1()
                .child(
                    uniform_list(
                        "file-tree-rows",
                        self.rows.len(),
                        cx.processor(Self::render_rows),
                    )
                    .size_full()
                    .track_scroll(&self.scroll),
                )
                .vertical_scrollbar(&self.scroll)
                .into_any_element()
        };

        v_flex()
            .id("file-tree")
            .size_full()
            .bg(theme.sidebar)
            .on_drop(
                cx.listener(|this, paths: &ExternalPaths, _, cx| this.drop_into_root(paths, cx)),
            )
            .drag_over::<ExternalPaths>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            // Empty space accepts tree entries into the root folder; VS
            // Code shows no highlight for it, so none is drawn here.
            .on_drop(cx.listener(|this, drag: &TreeDrag, window, cx| {
                this.drop_entry_into_root(&drag.path, window, cx)
            }))
            .on_drag_move::<TreeDrag>(cx.listener(
                |this, event: &DragMoveEvent<TreeDrag>, _, cx| {
                    this.note_drag_intent(&event.event.modifiers, cx);
                },
            ))
            .child(header)
            .child(body)
    }
}

/// A path's file name for dialog text, falling back to the whole path.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Move `source` to `dest`, falling back to copy + delete when the rename
/// crosses filesystems.
fn move_path(source: &Path, dest: &Path) -> std::io::Result<()> {
    if std::fs::rename(source, dest).is_ok() {
        return Ok(());
    }
    let target = dest.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination has no parent",
        )
    })?;
    copy_entry(source, target)?;
    if source.is_dir() {
        std::fs::remove_dir_all(source)
    } else {
        std::fs::remove_file(source)
    }
}

/// Copy `src` into the `target` folder, recursing into directories.
fn copy_entry(src: &Path, target: &Path) -> std::io::Result<()> {
    let name = src.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name")
    })?;
    let dst = target.join(name);
    if src == dst {
        return Ok(());
    }
    if src.is_dir() {
        // Copying a folder into itself (or a folder below it) would recurse.
        if target.starts_with(src) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot copy a folder into itself",
            ));
        }
        copy_dir_all(src, &dst)
    } else {
        std::fs::copy(src, &dst).map(|_| ())
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dst = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_all(&path, &dst)?;
        } else {
            std::fs::copy(&path, &dst)?;
        }
    }
    Ok(())
}

/// A folder's subfolders and Markdown files, folders first, by name.
fn list(folder: &Path, show_hidden: bool) -> Option<Vec<Entry>> {
    let mut entries: Vec<Entry> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter_map(|item| {
            let name = item.file_name().to_string_lossy().to_string();
            let path = item.path();
            // Follow links, so linked folders and files show as what they point to.
            let is_dir = path.is_dir();
            // VS Code's default excluded entries stay hidden until
            // "show hidden entries" is turned on; everything else shows.
            let excluded = !show_hidden && DEFAULT_EXCLUDED.contains(&name.as_str());
            (!excluded).then_some(Entry { name, path, is_dir })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_first_and_everything_else() {
        let dir = std::env::temp_dir().join(format!("malgel-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for folder in ["b-notes", "node_modules", ".git"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        for file in [
            "Zed.md",
            "alpha.markdown",
            "image.png",
            ".hidden.md",
            "todo.txt",
        ] {
            std::fs::write(dir.join(file), "").unwrap();
        }
        let names: Vec<String> = list(&dir, false)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(
            names,
            [
                "b-notes",
                "node_modules",
                ".hidden.md",
                "alpha.markdown",
                "image.png",
                "todo.txt",
                "Zed.md"
            ]
        );
        let all: Vec<String> = list(&dir, true)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(all.contains(&".git".to_string()));
        let _ = std::fs::remove_dir_all(dir);
    }
}
