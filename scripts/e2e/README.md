# End-to-end tests

`gui_smoke.sh` runs the real Malgel window and drives it with the keyboard,
the way a person would. It is the counterpart to `cargo test`: the unit tests
check the pieces in isolation, this checks that the pieces add up to an
application that starts, paints and responds.

## What it does

1. Starts a headless X server (Xvfb) and a throwaway workspace of sample
   files (Markdown, Rust, JSON, a shell script, CSV).
2. Launches the built binary against that workspace.
3. Waits for the window to appear and screenshots it.
4. Presses the shortcuts the VS Code alignment introduced —
   `Ctrl+Shift+P` (command palette), `Ctrl+P` (Quick Open), `Ctrl+B`
   (toggle sidebar) — screenshots each, and asserts the frame actually
   changed, so a shortcut that silently does nothing fails the run.
5. Focuses the Explorer, renames the picked file with `F2`, and checks the
   file on disk was renamed and still holds its contents.
6. Closes an editor with `Ctrl+W` and brings it back with `Ctrl+Shift+T`.
7. Pins the tab in front from the palette, then runs Close All Editors
   (`Ctrl+K Ctrl+W`) and checks the editor pane is untouched — the pinned
   tab stayed.
8. Searches the folder's files with `Ctrl+Shift+F`, takes the first hit, and
   types into the file the hit opened — so the check is on the file the hit
   came from, not on a changed frame.
9. Checks the process is still alive at the end (no crash from any step).

Screenshots land in `target/e2e-artifacts/` for eyeballing after a failure.

## Running it

Needs a release binary, plus `xvfb`, `xdotool`, ImageMagick (`import` /
`compare` / `identify`) and Mesa's software Vulkan (the `mesa-vulkan-drivers`
package). Those are installed for the Linux build in `.cnb.yml`-style images
and in any desktop Linux; the software renderer means no GPU is required.

```sh
cargo build --release
scripts/e2e/gui_smoke.sh
```

Overridable with environment variables: `MALGEL_BIN` (binary to run),
`E2E_ARTIFACTS` (where shots go), `E2E_DISPLAY`, `E2E_SETTLE` (seconds to let
a frame land before each screenshot — raise it on a slow machine).

A passing run prints `PASS` and the list of screenshots; any failed check
exits non-zero with the reason.
