//! Quick Open: the files under the open folder, so Ctrl+P (⌘P) can jump to
//! one by typing part of its path.
//!
//! The walk honours `.gitignore` (plus `.ignore`, the global git ignore and
//! `.git/info/exclude`), stops descending at [`MAX_DEPTH`], and goes deeper
//! only inside folders the sidebar has already opened, so a folder opened by
//! accident cannot drag the palette through a whole vendored tree. The call
//! is cheap enough to run beside the UI rather than in front of it.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// How deep the listing walks on its own.
pub const MAX_DEPTH: usize = 6;
/// The deepest a folder the user opened in the sidebar may pull the walk.
const MAX_OPENED_DEPTH: usize = 24;
/// The most files one listing hands to the palette. A writing folder runs to
/// the hundreds; the cap keeps a folder opened by accident from filling the
/// picker with tens of thousands of rows.
pub const MAX_FILES: usize = 4000;

/// Every file under `root` the palette should offer, by path.
///
/// `show_hidden` follows the sidebar's setting for dot files. `opened` is
/// the set of folders the sidebar currently has open: below [`MAX_DEPTH`]
/// only those are searched further, and everything else stops at the cap.
pub fn files_under(
    root: &Path,
    show_hidden: bool,
    opened: &[PathBuf],
    limit: usize,
) -> Vec<PathBuf> {
    let opened = opened.to_vec();
    let walker = WalkBuilder::new(root)
        .hidden(!show_hidden)
        // Apply ignore rules outside a git repository too, so a folder of
        // notes with a .gitignore is respected as well.
        .require_git(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .follow_links(false)
        // One thread: this runs beside the UI, not in place of it.
        .threads(1)
        .filter_entry(move |entry| {
            let depth = entry.depth();
            if depth < MAX_DEPTH {
                return true;
            }
            // Below the cap, keep the folders the user opened, the folders
            // on the way down to one, and whatever is inside them.
            depth < MAX_OPENED_DEPTH
                && opened.iter().any(|folder| {
                    entry.path().starts_with(folder) || folder.starts_with(entry.path())
                })
        })
        .build();

    let mut files = Vec::new();
    for entry in walker.flatten() {
        if files.len() >= limit {
            break;
        }
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            files.push(entry.into_path());
        }
    }
    files.sort_by(|a, b| {
        a.to_string_lossy()
            .to_lowercase()
            .cmp(&b.to_string_lossy().to_lowercase())
    });
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, rel: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
    }

    fn relative(root: &Path, files: Vec<PathBuf>) -> Vec<String> {
        files
            .into_iter()
            .map(|path| {
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("malgel-quick-open-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn files_under_respects_gitignore() {
        let dir = scratch("gitignore");
        std::fs::write(dir.join(".gitignore"), "build/\nsecret.md\n").unwrap();
        touch(&dir, "top.md");
        touch(&dir, "notes/a.md");
        touch(&dir, "secret.md");
        touch(&dir, "build/out.md");

        assert_eq!(
            relative(&dir, files_under(&dir, false, &[], 100)),
            ["notes/a.md", "top.md"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_under_stops_at_the_depth_cap_unless_the_folder_was_opened() {
        let dir = scratch("depth");
        touch(&dir, "top.md");
        touch(&dir, "a/b/c/d/e/f/g/h/deep.md");

        // Eight levels down is past the cap, so it is not listed…
        assert_eq!(
            relative(&dir, files_under(&dir, false, &[], 100)),
            ["top.md"]
        );
        // …until the sidebar has opened the folder holding it.
        let opened = [dir.join("a/b/c/d/e/f/g")];
        assert_eq!(
            relative(&dir, files_under(&dir, false, &opened, 100)),
            ["a/b/c/d/e/f/g/h/deep.md", "top.md"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_under_follows_the_show_hidden_setting_and_the_limit() {
        let dir = scratch("hidden");
        touch(&dir, "top.md");
        touch(&dir, ".hidden.md");

        assert_eq!(
            relative(&dir, files_under(&dir, false, &[], 100)),
            ["top.md"]
        );
        assert_eq!(
            relative(&dir, files_under(&dir, true, &[], 100)),
            [".hidden.md", "top.md"]
        );
        assert_eq!(files_under(&dir, true, &[], 1).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
