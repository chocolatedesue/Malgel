#!/usr/bin/env bash
#
# End-to-end smoke test for Malgel's GUI.
#
# `cargo test` covers the pieces in isolation; nothing so far proves the whole
# application boots, paints a window and answers the keyboard. This script
# does: it starts a headless X server (Xvfb), runs the real binary against a
# throwaway workspace, drives it with xdotool, and checks the results. Where a
# step has an observable result (a saved file, a changed frame) it asserts on
# that rather than on pixels alone.
#
#     scripts/e2e/gui_smoke.sh
#
# Requirements (Debian/Ubuntu names): xvfb, xdotool, imagemagick, and Mesa's
# software renderers (libgl1-mesa-dri + libegl1 + libegl-mesa0). A binary must
# already be built (`cargo build` or `--release`); point MALGEL_BIN at anything
# else to override. scripts/e2e/Dockerfile carries all of this.
#
# The software GL renderer is used by default so no GPU is needed: Xvfb has no
# DRI3, and the Vulkan present path needs it, so a Vulkan render shows a
# mapped but unpainted window, while the GL path draws fine. Set E2E_GPU=1 to
# keep Vulkan (a machine with a real GPU).
#
# Exit status is 0 only when every check passes.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

BIN="${MALGEL_BIN:-$ROOT/target/release/malgel}"
[[ -x "$BIN" ]] || BIN="$ROOT/target/debug/malgel"
ART_DIR="${E2E_ARTIFACTS:-$ROOT/target/e2e-artifacts}"
DISPLAY_NUM="${E2E_DISPLAY:-:99}"
SETTLE="${E2E_SETTLE:-1.2}" # seconds to let a frame land before each shot

mkdir -p "$ART_DIR"
rm -f "$ART_DIR"/*.png "$ART_DIR"/*.log 2>/dev/null || true

fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "  ok: $*"; }

command -v Xvfb >/dev/null || fail "Xvfb not installed"
command -v xdotool >/dev/null || fail "xdotool not installed"
command -v import >/dev/null || fail "ImageMagick 'import' not installed"
[[ -x "$BIN" ]] || fail "no binary at $BIN — build it first (cargo build)"

cleanup() {
  [[ -n "${APP_PID:-}" ]] && kill "$APP_PID" 2>/dev/null || true
  [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null || true
}
trap cleanup EXIT

# A previous run that was killed can leave a server, a lock and a socket
# behind; a second Xvfb on the same display then refuses to start and the app
# fails to connect. Clear ours, then start and wait for it, retrying once.
rm -rf /tmp/malgel-e2e-xdg && mkdir -p /tmp/malgel-e2e-xdg && chmod 700 /tmp/malgel-e2e-xdg
# A fresh HOME keeps the run reproducible: no session to restore, no settings
# or recently-opened list carried over from an earlier run or a real profile.
export HOME=/tmp/malgel-e2e-home
rm -rf "$HOME" && mkdir -p "$HOME"
export XDG_CONFIG_HOME="$HOME/.config" XDG_DATA_HOME="$HOME/.local/share"
export DISPLAY="$DISPLAY_NUM"
num="${DISPLAY_NUM#:}"
start_xvfb() {
  pkill -f "Xvfb $DISPLAY_NUM" 2>/dev/null || true
  sleep 0.3
  rm -f "/tmp/.X${num}-lock" "/tmp/.X11-unix/X${num}" 2>/dev/null || true
  Xvfb "$DISPLAY_NUM" -screen 0 1600x1000x24 +extension GLX +render -nolisten tcp \
    >"$ART_DIR/xvfb.log" 2>&1 &
  XVFB_PID=$!
  for _ in $(seq 1 50); do
    xdotool getdisplaygeometry >/dev/null 2>&1 && return 0
    sleep 0.1
  done
  return 1
}
start_xvfb || start_xvfb || { cat "$ART_DIR/xvfb.log" >&2; fail "Xvfb did not come up on $DISPLAY_NUM"; }

export XDG_RUNTIME_DIR=/tmp/malgel-e2e-xdg
if [[ "${E2E_GPU:-0}" != "1" ]]; then
  export LIBGL_ALWAYS_SOFTWARE=1
  export GALLIUM_DRIVER=llvmpipe
  # A path with no ICD makes wgpu skip Vulkan and pick the GL renderer, whose
  # present path Xvfb actually supports (see the header note).
  export VK_ICD_FILENAMES="$ART_DIR/no-such-icd.json"
fi

# A small workspace whose file types exercise the icon map: Markdown, Rust,
# JSON, a script and CSV.
WS="$(mktemp -d)"
mkdir -p "$WS/notes" "$WS/src"
printf '# Malgel end-to-end\n\nHello from the smoke test.\n' >"$WS/README.md"
printf '# Second document\n\nA paragraph.\n' >"$WS/notes/todo.md"
printf 'fn main() { println!("hi"); }\n' >"$WS/src/main.rs"
printf '{"a": 1}\n' >"$WS/config.json"
printf '#!/bin/sh\necho hi\n' >"$WS/run.sh"
printf 'name,qty\nwidget,3\n' >"$WS/data.csv"

shot() { # shot <name>
  sleep "$SETTLE"
  import -display "$DISPLAY" -window root "$ART_DIR/$1.png" 2>/dev/null
}
colours() { identify -format '%k' "$ART_DIR/$1.png" 2>/dev/null || echo 0; }
# `compare -metric AE` writes the count to stderr and exits non-zero when the
# images differ; capture both and keep the number.
pixels_differ() { compare -metric AE "$ART_DIR/$1.png" "$ART_DIR/$2.png" null: 2>&1 || true; }
# Dismiss an overlay (palette, Quick Open). The first Escape clears the search
# box, the second closes; sending both is harmless when one would do.
dismiss() {
  xdotool key --clearmodifiers Escape
  sleep 0.2
  xdotool key --clearmodifiers Escape
}

echo "== malgel GUI smoke =="
echo "binary:   $BIN"
echo "display:  $DISPLAY ($(xdotool getdisplaygeometry))"

# Open the folder so the file tree is exercised, and a file so the editor is.
"$BIN" "$WS" "$WS/README.md" >"$ART_DIR/app.log" 2>&1 &
APP_PID=$!

# 1. The window comes up and the process survives.
WIN=""
for _ in $(seq 1 100); do
  WIN="$(xdotool search --onlyvisible --class malgel 2>/dev/null | head -1 || true)"
  [[ -z "$WIN" ]] && WIN="$(xdotool search --onlyvisible --name -i malgel 2>/dev/null | head -1 || true)"
  [[ -n "$WIN" ]] && break
  kill -0 "$APP_PID" 2>/dev/null || fail "process exited during startup; see $ART_DIR/app.log"
  sleep 0.1
done
[[ -n "$WIN" ]] || fail "no window appeared"
pass "window $WIN mapped ($(xdotool getwindowgeometry "$WIN" | tail -1 | tr -s ' '))"
# No window manager under Xvfb, so focus the window directly for key events.
xdotool windowfocus "$WIN" 2>/dev/null || true
xdotool windowraise "$WIN" 2>/dev/null || true
shot 01-startup
[[ "$(colours 01-startup)" -gt 8 ]] || fail "startup frame is blank ($(colours 01-startup) colours)"
pass "startup painted ($(colours 01-startup) colours)"

# 2. Typing reaches the editor and Ctrl+S writes the file the app opened.
#    The deepest check: input path, editor, save and disk, end to end.
xdotool type --delay 30 'SMOKE'
sleep 0.3
xdotool key --clearmodifiers ctrl+s
shot 02-edited-and-saved
grep -q 'SMOKE' "$WS/README.md" || fail "typed text was not saved to README.md"
pass "typed text reached the file (edit + Ctrl+S round-trip)"

# 3. Ctrl+Shift+P opens the command palette; typing filters it; Escape closes.
xdotool key --clearmodifiers ctrl+shift+p
shot 03-command-palette
[[ "$(pixels_differ 02-edited-and-saved 03-command-palette)" -gt 1000 ]] \
  || fail "Ctrl+Shift+P did not change the frame"
xdotool type --delay 40 'save'
shot 04-command-palette-filtered
[[ "$(pixels_differ 03-command-palette 04-command-palette-filtered)" -gt 100 ]] \
  || fail "typing did not filter the command palette"
dismiss
shot 05-after-palette
[[ "$(pixels_differ 03-command-palette 05-after-palette)" -gt 1000 ]] \
  || fail "the command palette did not close"
pass "command palette opens, filters and closes (Ctrl+Shift+P)"

# 4. Ctrl+P opens Quick Open over the workspace files.
xdotool key --clearmodifiers ctrl+p
shot 06-quick-open
[[ "$(pixels_differ 05-after-palette 06-quick-open)" -gt 1000 ]] \
  || fail "Ctrl+P did not change the frame"
dismiss
shot 07-after-quick-open
[[ "$(pixels_differ 06-quick-open 07-after-quick-open)" -gt 1000 ]] \
  || fail "Quick Open did not close"
pass "Quick Open opens and closes (Ctrl+P)"

# 5. Ctrl+B in the editor is Bold, as in VS Code (Toggle Side Bar is the same
#    key outside the editor). Assert on the file, not the pixels.
xdotool key --clearmodifiers ctrl+a
xdotool key --clearmodifiers ctrl+b
sleep 0.3
xdotool key --clearmodifiers ctrl+s
shot 08-bold
grep -q '\*\*' "$WS/README.md" || fail "Ctrl+B in the editor did not apply bold"
pass "Ctrl+B in the editor applies bold (Ctrl+S round-trip)"

# 6. Still alive at the end — no crash from any of the above.
kill -0 "$APP_PID" 2>/dev/null || fail "process died during the run"
pass "process alive at exit"

echo
echo "PASS — artifacts in $ART_DIR"
ls -1 "$ART_DIR"/*.png
