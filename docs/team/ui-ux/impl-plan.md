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
