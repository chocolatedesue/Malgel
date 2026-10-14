# Aligning Malgel with VS Code

Malgel is a Markdown editor, not an IDE. The goal is to feel familiar to
someone who lives in VS Code — the same keys, the same places to find
commands, the same visual language for a file's type — not to grow every IDE
feature. The scope is documents: writing, navigating and reading Markdown.

The reference is VS Code's **default** behaviour (its own `keybindings.json`
is not considered).

## What is done

- **Command palette** (Ctrl+Shift+P). Every command in one searchable list,
  grouped File / Explorer / Edit / Go / View / Format / Settings / Help. Each
  row shows the command's current key binding, and confirming a row runs its
  Action. `src/commands.rs` holds the list; the palette reuses GPUI Kit's
  `Command` component.

  The palette lives in a dialog, and an action confirmed there is dispatched
  along the dialog's own focus path — which the window's workspace is not part
  of. Its rows therefore carry no action at all; the workspace keeps the Action
  beside each label and dispatches the confirmed row itself, from the window's
  own focus path. Without that, every row looked right and did nothing.

- **File-type icons.** `src/file_icon.rs` maps an extension to a bundled
  Lucide icon, and the file tree, the tab strip and Quick Open all draw it.
  VS Code uses coloured Seti icons; Malgel uses the single-colour GPUI Kit
  set, so files differ by shape, not colour.

- **The Explorer's commands** (Ctrl+Shift+E shows the sidebar, focuses the
  tree and picks the open file). While the tree has the keyboard its keys are
  VS Code's, and they mean nothing outside it — in the editor Del still
  deletes text and Ctrl+X still cuts the selection. `src/file_tree.rs` holds
  them, under its own key context:

  | Command | Key | Also |
  | --- | --- | --- |
  | Move the pick, open and close folders | ↑ ↓ ← → | |
  | First / last entry | Home / End | |
  | Open the picked file | Enter | double click |
  | Rename | F2 | right-click menu |
  | Delete | Del / Backspace | right-click menu |
  | Cut / copy / paste | Ctrl+X / Ctrl+C / Ctrl+V | right-click menu |
  | New file / new folder | — | the view header buttons, the right-click menu, the palette |

  A new name may be nested (`notes/plan.md`) and builds the folders on the way;
  a rename stays in its folder. Open editors follow both, and deleting an entry
  closes the editors that were showing it.

- **Pinned tabs.** Pin or unpin from a tab's own menu or the palette; a pinned
  tab shows a pin where its close box was, survives Close Other Tabs and Close
  All Editors, and is unpinned when its editor closes.

- **Close All Editors** (Ctrl+K Ctrl+W) and **Reopen Closed Editor**
  (Ctrl+Shift+T, which skips files that have since gone).

- **Back / forward** through the editors shown (Alt+← / Alt+→). The history is
  the sequence of files the window has shown, so it walks tabs and files alike.

- **Go to Line** (Ctrl+G) and **Toggle Word Wrap** (Alt+Z).

- **Key bindings follow VS Code.** Where Malgel had taken a VS Code key for
  something else, the other command moved or lost its key:

  | Key | VS Code | Malgel before | Now |
  | --- | --- | --- | --- |
  | Ctrl+B | Toggle Side Bar | Ctrl+Shift+B | Ctrl+B from the workspace; Bold still takes Ctrl+B inside the editor |
  | Ctrl+Shift+F | Find in Files | Focus mode | freed; Focus mode moved to Ctrl+Alt+F |
  | Ctrl+Shift+E | Explorer | Export as HTML | Explorer; Export is in the File menu |
  | Ctrl+Shift+L | Select All Occurrences | Switch light and dark | freed; the switch is in the View menu |

  Ctrl+1/2/3 (layout, not editor groups) and Ctrl+K (Link, and a chord
  prefix) stay as Malgel has them: there are no editor groups and no other
  chord to prefix.

- **Menus.** A **Go** menu (Show all commands, Go to file / heading / line,
  Back, Forward) groups navigation the way VS Code does; **Settings** moved
  under File on Windows and Linux, matching File ▸ Preferences; the File menu
  carries Close all editors and Reopen closed editor.

## How it is checked

`cargo test` covers the pieces (the key-binding table, the icon map, the
entry-name rules, the formatting transforms). On top of that,
`scripts/e2e/gui_smoke.sh` runs the real window headlessly and drives it the
way a person would. It types into the open document and saves it; opens,
filters and confirms the command palette; opens Quick Open and picks a file;
presses Ctrl+B inside the editor to check Bold still wins there; focuses the
Explorer and renames the picked file with F2; closes an editor and brings it
back with Ctrl+Shift+T; and pins a tab. It asserts on the results — the file
on disk, the changed frame, the editor pane a pinned tab keeps — so a shortcut
or a command that stops working fails the run. See `scripts/e2e/README.md`.

## What is left

- **Cross-file search** (Ctrl+Shift+F), now that the key is free.
- Editor niceties that depend on GPUI Kit's editor: current-line highlight,
  bracket matching, indent guides, Ctrl+D multi-select, Alt+↑/↓ line moves.

## Non-goals

Activity bar, minimap, breadcrumbs, an embedded terminal panel, **coloured
(Seti) file icons**, and Debug / Source Control / Extensions / multi-root
workspaces. Malgel edits documents; these belong to an IDE.
