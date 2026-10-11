# Upstream feedback: functional changes on `upstream-feedback`

Status: **draft for review — no PR submitted yet.**

- Base: `rcawston/Malgel@845864b` ("Bump the version to 0.1.1", upstream `main`)
- Branch: `upstream-feedback` (on the private mirror `chocolatedesue/Malgel`)
- Purpose: collect the changes we made while using Malgel on a Windows 10
  x86_64 machine, stripped of everything personal (CI pipelines, build-host
  workarounds, machine-specific fixes), so each functional change can be
  reviewed on its own merits and, where it holds up, submitted upstream
  one PR at a time.

## Change inventory

| # | Commit | Change | Category | Upstream fit |
|---|--------|--------|----------|--------------|
| 1 | `bdcdbeb` | Markdown tabs (fenced + container syntax), workspace multi-document tabs, native PDF viewer (hayro) | feature | large; split before any PR |
| 2 | `c0bff41` | PDFs unreachable from CLI args and session restore | bug fix | yes, with one caveat (see 2.2) |
| 3 | `091ca7e` | Terminal summon hotkey, CLI file/folder args, single-instance wake | feature | yes, self-contained |
| 4 | `c1bbd97` | `:::tabs` containers spanning multiple blocks leaked raw text | bug fix | yes, small |
| 5 | `0ac93c0` | `cargo fmt` | style | yes |
| 6 | `d828d4c` | clippy: `question_mark` in `parse_tab_header`/`parse_tabs_container` | lint | yes, trivial |

## 1. Markdown tabs, workspace tabs, PDF viewer (`bdcdbeb`, 317+ lines)

Adds three related capabilities in one commit (it was developed as one
task; a PR would likely split it):

- **Markdown preview tabs**: fenced ```` ```tabs ```` blocks (MkDocs
  Material style, `=== "Title"` headers, `@tab`, `:::tab`, `---`, `!`
  default-tab marker), rendered with GPUI-Kit's `TabBar`, per-block
  interactive state, copy button, nested markdown/math rendering inside
  tab bodies.
- **Workspace multi-document tabs**: open several documents as tabs,
  close others/right, dirty indicators, `Ctrl+Tab` / `Ctrl+Shift+Tab`
  navigation, `tabs` setting (default on).
- **Native PDF viewing**: `DocumentView` routes `*.pdf` to a `PdfView`
  (hayro + hayro-svg rendering to SVG, zoom controls, page numbering),
  wired into the file tree, links, CLI and session restore.

Verified on Windows 10 x86_64 (real machine): tab rendering and
switching, workspace tabs, PDF rendering of generated and hand-written
PDFs. 102 unit tests pass (host + `x86_64-pc-windows-msvc` builds).

## 2. PDFs unreachable from CLI args and session restore (`c0bff41`)

**Symptom**: `malgel doc.pdf` (CLI argument) and reopening a PDF from the
saved session left an empty "Untitled" document; only opening a PDF from
the sidebar tree worked.

**Root causes, three stacked:**

1. `open_paths`/`load_path` spawn an async load whose continuation calls
   `update_in` on the workspace — during startup this runs while the
   window is still inside its build callback and the update is silently
   dropped (`_ =` swallows the error). Deferred via `Context::defer_in`
   until the window is returned.
2. The `spawn_in` continuation after the background render never resumed
   under `gpui-pre 0.3.7` when spawned from the startup path (traced:
   the render completes, the continuation is never polled). The PDF
   branch now renders **inline** in `load_path` — PDFs are small and
   this keeps startup/restore working; worth revisiting upstream with a
   proper fix in the executor.
3. Session restore routed *every* document through `document::load`
   (text only); PDF entries now defer through the PDF loader.

Also included: a stderr logger (`RUST_LOG`-aware, `Warn` by default) and
the full `anyhow` chain when a window fails to open — without those, the
errors above were invisible.

**Caveat for upstream**: (2) was reproduced on a cross-compiled
(`cargo-xwin`) build; we could not reproduce the continuation stall on a
native build (not tested). The deferral in (1) and the restore routing in
(3) are correct regardless.

## 3. Terminal summon, CLI file/folder args, single-instance wake (`091ca7e`)

- **`Ctrl+\`` (Cmd+\` on macOS)** opens a system terminal in the current
  sidebar folder: `wt.exe` on Windows with a `cmd` fallback, Terminal on
  macOS, `x-terminal-emulator`/`xterm` on Linux. This is the
  "summon an external terminal" flavor; an embedded terminal panel is a
  larger follow-up and intentionally out of scope here.
- **CLI arguments may be folders** (become the sidebar root, like
  VS Code's folder open) as well as files — previously non-file paths
  were silently dropped by `open_external`.
- **Second launch forwards to the running instance** over a local TCP
  handshake (ports 47615–47624, magic-prefixed payload) and exits; an
  empty payload just activates the window. Verified: three consecutive
  launches leave a single window; the forwarded file opens in it and is
  selected in the sidebar.

## 4. `:::tabs` containers spanning multiple blocks (`c1bbd97`)

**Symptom**: a `:::tabs … :::` container whose body contains a fenced
code block rendered only its first tab; every later `@tab` section
leaked as raw markdown.

**Root cause**: the markdown pipeline parses block by block and plugins
match a single node (`MarkdownPlugin::parse(node)`); the container's
later sections are separate blocks (paragraph → code fence → paragraph)
that no node claims.

**Fix**: normalize the *source* before parsing — rewrite `:::tabs … :::`
regions into ```` ```tabs ```` fences (tracking open code fences so
documented examples inside fences are untouched), reusing the already
tested fenced-tabs path. Two new unit tests cover the multi-tab
container and the inner-fence case.

## 5. Environment-specific work **not** included here

For completeness, since the icon bug may be reported by anyone using the
same setup:

- `1437b58` upstream-side (private main) enables `debug-assertions` for
  the Windows target because runtime shader compilation is required when
  cross-compiling with `cargo-xwin`. **Interaction**: with
  `debug-assertions` on, `rust-embed` 8.x loads embedded assets from the
  *build machine's* filesystem at runtime instead of embedding them, so
  every gpui-kit icon lookup fails on the target machine
  (`could not find asset at path "icons/…"`). The one-line companion is
  `rust-embed = { version = "8", features = ["debug-embed"] }`.
  Native builds don't hit this; that's why it is not part of the branch.
- CI pipelines (CNB, GitHub Actions matrix) — personal infrastructure.

## Known issue not fixed yet

- **PDF page offset**: the PDF view renders correctly but its document
  pane can be wider than the window (toolbar right edge and page edge
  extend past the visible area). Diagnosis: pane width overflow in the
  workspace layout for PDF-backed views; fix pending.

## Planned follow-ups

- **Open Recent** (LRU list of recent files/folders, reopen action,
  `Ctrl+R`).
- Integrated terminal panel (embedded PTY) as the in-app counterpart of
  the terminal-summon hotkey.

## Review guide

Suggested reading order: 4 (smallest bug fix) → 2 (bug fix with root
cause chain) → 3 (self-contained feature) → 1 (the large feature) → 5/6
(style). Each commit is independently buildable; the whole branch passes
`cargo test` (102 tests) on the host and builds for
`x86_64-pc-windows-msvc` and the host target.

Maintenance: this branch rebases onto `upstream/main` as it moves;
environment-specific commits stay out of it and are documented here
instead.
