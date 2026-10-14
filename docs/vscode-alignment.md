# Aligning Malgel with VS Code

Malgel is a Markdown editor, not an IDE. The goal is to feel familiar to
someone who lives in VS Code — the same keys, the same places to find
commands, the same visual language for a file's type — not to grow every IDE
feature. The scope is documents: writing, navigating and reading Markdown.

The reference is VS Code's **default** behaviour (its own `keybindings.json`
is not considered).

## What is done

- **Command palette** (Ctrl+Shift+P). Every command in one searchable list,
  grouped File / Edit / Go / View / Format / Settings / Help. Each row shows
  the command's current key binding, and confirming a row dispatches its
  Action. `src/commands.rs` holds the list; the palette reuses GPUI Kit's
  `Command` component.
- **File-type icons.** `src/file_icon.rs` maps an extension to a bundled
  Lucide icon, and the file tree, the tab strip and Quick Open all draw it.
  VS Code uses coloured Seti icons; Malgel uses the single-colour GPUI Kit
  set, so files differ by shape, not colour.
- **Go to Line** (Ctrl+G) and **Toggle Word Wrap** (Alt+Z).
- **Key bindings follow VS Code.** Where Malgel had taken a VS Code key for
  something else, the other command moved or lost its key:

  | Key | VS Code | Malgel before | Now |
  | --- | --- | --- | --- |
  | Ctrl+B | Toggle Side Bar | Ctrl+Shift+B | Ctrl+B from the workspace; Bold still takes Ctrl+B inside the editor |
  | Ctrl+Shift+F | Find in Files | Focus mode | freed; Focus mode moved to Ctrl+Alt+F |
  | Ctrl+Shift+E | Explorer | Export as HTML | freed; Export is in the File menu |
  | Ctrl+Shift+L | Select All Occurrences | Switch light and dark | freed; the switch is in the View menu |

  Ctrl+1/2/3 (layout, not editor groups) and Ctrl+K (Link, and a chord
  prefix) stay as Malgel has them: there are no editor groups and no other
  chord to prefix.

- **Menus.** A **Go** menu (Show all commands, Go to file / heading / line)
  groups navigation the way VS Code does; **Settings** moved under File on
  Windows and Linux, matching File ▸ Preferences.

## How it is checked

`cargo test` covers the pieces (the key-binding table, the icon map, the
formatting transforms). On top of that, `scripts/e2e/gui_smoke.sh` runs the
real window headlessly and drives it the way a person would: it types into the
open document and saves it, opens and filters the command palette, opens Quick
Open, and presses Ctrl+B inside the editor to check Bold still wins there. It
asserts on the results — the file on disk, the changed frame — so a shortcut
that stops working fails the run. See `scripts/e2e/README.md`.

## What is left

- **File tree editing**: new file and folder, rename (F2), delete, cut /
  copy / paste, keyboard navigation, and the matching view-header actions.
- **Cross-file search** (Ctrl+Shift+F), now that the key is free.
- **Pinned tabs**, reopening a closed editor (Ctrl+Shift+T), and editor
  back / forward (Alt+← / →) — each needs state Malgel does not keep yet.
- Editor niceties that depend on GPUI Kit's editor: current-line highlight,
  bracket matching, indent guides, Ctrl+D multi-select, Alt+↑/↓ line moves.

## Non-goals

Activity bar, minimap, breadcrumbs, an embedded terminal panel, **coloured
(Seti) file icons**, and Debug / Source Control / Extensions / multi-root
workspaces. Malgel edits documents; these belong to an IDE.
