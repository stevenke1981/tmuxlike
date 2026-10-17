# UI/UX implementation plan (coordinator merge of ux-spec.md + ui-spec.md)

Implementers: Claude Haiku 5.5 (high). Coordinator: Opus 5.5. Base: UI-0 mechanical splits
(render/ and input/ submodules, golden frames byte-identical) + UI contracts commit 2d9e5e9.

## Decisions where the specs differ
- Colors/glyphs: ui-spec §2 tokens via `crate::theme::Theme` (ANSI-16 defaults; done = green 2, accent = cyan 6).
  ux-spec's "done = cyan" is superseded. Never emit bright fg 97/93/96/92/94 for chrome.
- Pane titles: `pane-titles = off|auto|always`, default `auto` = window has ≥2 visible panes, not zoomed,
  pane height ≥ 6, AND the pane has an agent report or a label (ui-spec definition; plain panes keep their rows).
- `confirm-close` default on (tmux parity); typed commands, CLI and API stay immediate.
- `hints` default on. NO_COLOR keeps Unicode shapes; TERM=dumb / TMUXLIKE_ASCII=1 → ASCII glyphs.
- Out of this wave (P2): help popup regrouping/filter (UX-14) except the close hint, divider drag + right-click
  menu (UX-15), sidebar rail mode (UI-12), clock/COLORFGBG (UI-13), prefix-timeout (UX-16), OSC title/bell (UX-13).

## Ownership (wave UI-1)
| Worker | Items | Owns |
|---|---|---|
| U1 panes+theme | UI-01 presets/overrides, UI-02 mono+DIM in emit, UI-03 per-segment active edge + tees, UX-10/UI-11 pane titles | theme.rs, render/mod.rs (Grid/emit/sgr only), render/panes.rs, server.rs `visible_rects` only |
| U2 status bar | UX-01/02 + UI-05 chips/flags/counts/cut order/CJK width + hit-test, UX-04 which-key render, UI-08/UX-09 prompt cursor+left-cut render and message chip with kind, UI-09/UX-11 copy chip+hint | render/status.rs |
| U3 sidebar+mouse | UI-06/UX-06/UI-04 sidebar redesign + legend, UX-07 mouse parity (sidebar rows, `[session]`, counters, overlay rows/wheel, click cancels prefix) | render/sidebar.rs, input/mouse.rs |
| U4 overlays+keys | UX-08/UI-07 chooser v2 (tree, filters b/w/d/i/a, footer hints, `└`, plurals, unknown keys ignored), help close hint, UX-09 prompt editing (cursor, Ctrl-a/e/k/u/w, history), UX-05 confirm key handling, UX-04 unbound-key notice | render/overlay.rs, input/keys.rs |
| U5 commands | UX-00 debug-frame flags, UX-03 next-attention (+ Tab/BTab, `g` alias) + blocked toast, UX-05 `confirm-before` command + default bindings, KEYS help text, readable no-client errors (UX-16 part) | commands.rs, server.rs `exec`/`exec_global`/`handle_conn` Run arm, api/agent.rs (toast hook only), main.rs KEYS |

Coordinator-owned shared state (added before dispatch): `PromptKind::Confirm`, `Prompt { cur, pending }`,
`State.history`, `Client.message` kind, `Ctx` passed to every render section with the resolved `Theme`.

## Acceptance
Each item's acceptance criteria are in the source specs (`debug-frame --text` flags from UX-00 are the test hooks).
Gate for the wave: cargo fmt/clippy clean for new code, cargo test, scripts/smoke.py, scripts/api_smoke.py, golden
frame review (frames are expected to change; coordinator reviews the 5 scenarios + NO_COLOR + ASCII variants).

## Worker rules (UI-1)
- Base commit `dc79cbf`. Worktree `D:/tmuxlike-wt/uN`, branch `ui/uN`. Hard rules of `docs/team/herdr-alignment/brief.md` apply
  (own files only, no git writes, no new deps, no server start without a unique TMUXLIKE_SOCKET/TMUXLIKE_STATE/TMUXLIKE_CONFIG; always kill-server).
- Shared state (read-only for you): `crate::theme::{Theme, Glyphs}` (`cx.theme` in every render section; use `cx.theme.c(color)` for
  every chrome color so mono works; `cx.theme.status(Status)` for glyph/color/word); `server::{Prompt, PromptKind::Confirm, Message}`,
  `State.history`, config `hints`, `confirm_close`, `pane_titles`, `theme`.
- Use `unicode-width` for every width; cut with `…` (`cx.theme.g.ellipsis`); never split a wide char.
- Pure layout logic goes in pure functions with unit tests (e.g. `status_layout`, `which_key`, `sidebar_rows`, `edge_segments`).
- Verify: `cargo fmt`, `cargo build` (no warnings from your files), `cargo test`, `cargo clippy --all-targets` (clean in your files),
  `cargo build --release` + `python -I <scratchpad>/frame_golden.py <your exe> <scratchpad>/golden-uN` and READ the frames you changed
  (strip ANSI) to check them against the spec mockups; `scripts/smoke.py` and `scripts/api_smoke.py` must still pass.
  Scratchpad: C:/Users/eda/AppData/Local/Temp/claude/D--agent-team-workflow/5fe1fd1b-0a35-48d8-bbad-ffd9097c1ead/scratchpad
- e2e strings that must survive (scripts/e2e_linux.py): `(rename-window)`, `unknown command`, `sourced`, `choose session`,
  `work: 2 windows`, `detached`, sidebar words `idle`/`working`, `new-window` in help.
- Report (≤ 40 lines): STATUS, files+line counts, item IDs done/partial/skipped, commands+exit codes, frame excerpts as evidence, gaps.
