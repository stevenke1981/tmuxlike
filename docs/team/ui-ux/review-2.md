# REV-2 independent review (Sonnet 5.5, read-only) — range 0c0ae71..44082c2

Verdict: APPROVE WITH FIXES. No CRITICAL. Reproduced with a wire-protocol client against the release build.
Fixed by coordinator: M3 (KEYS pane-titles text, e4ec443). Contracts added for fix wave R2 (this commit):
`Prompt.hist`, `Prompt.expect_pane`, theme glyphs `info`/`dot`/`dash`, shared `render::cut(s, max, ell)`.

## Open findings → owners (fix wave R2)
| ID | Finding | Owner |
|---|---|---|
| H1 | mouse.rs:236 wheel_overlay steps the PACKED chooser value: filter lost, selection jumps to last row. Unpack → step within `visible_entries(filter)` → `pack_chooser`. | R2a |
| H2 | mouse.rs:61,88 click_sidebar returns true on the status row inside the sidebar columns, so counters/labels there never reach click_status. Check the status row first. | R2a |
| H3 | sidebar.rs:199 overflow `truncate(keep)` keeps the first sessions; the attached (open) group can vanish behind "+N more". Keep the open group, drop closed groups first. | R2b |
| M1 | confirm-before is not bound to its target: `pending` = `kill-pane`/`kill-window` resolved at `y` time; focus can move in between (reproduced: wrong pane killed). Store target: `kill-window -t S:IDX`; for kill-pane set `Prompt.expect_pane` and refuse with "target changed" if the active pane differs (keys side R2a, prompt construction in server.rs exec / commands/ui.rs R2d). | R2a + R2d |
| M2 | keys.rs:371 history position inferred from text; duplicates loop. Use `Prompt.hist`. | R2a |
| M4 | ASCII mode leaks Unicode: `render::truncate` hardcodes `…` (panes.rs:142, 8 sidebar sites); `·` panes.rs:165; `▲ ◆ ·` commands/ui.rs:104,106,146,173; `—` server.rs:759, sidebar.rs:29. Use `render::cut(.., theme.g.ellipsis)` and `theme.g.{dot,dash,blocked,done}`; delete `truncate` when unused. | R2b (render) + R2c (status/overlay cut dedupe) + R2d (commands/server strings) |
| M5 | input/mod.rs:35 paste into a Confirm prompt appends to the question. Ignore paste in Confirm. | R2a |
| L1 | status.rs:189 global HITS keyed by session name (multi-client wrong, never pruned, cleared during prefix). Derive hits on demand from `status_layout` like `status_regions`. | R2c |
| L2 | Mono: muted chrome lacks DIM outside pane titles (status, sidebar, overlay). | R2b (sidebar, panes) + R2c (status, overlay) |
| L3 | muted (ANSI 8) on bar_bg (ANSI 0) for hints can vanish on Solarized; use bar_fg + DIM on the bar. | R2c |
| L4 | info notices show green `✓` ("✓ cancelled", blocked toast). Use `theme.g.info` + neutral color; `✓` only for successes (copied, sourced). | R2c (render) + R2a/R2d (callers mark kind) |
| L5 | next-attention / chooser jump call switch_session for the same session → full redraw on every Tab. Skip when same session. | R2d (commands/ui.rs) + R2a (chooser jump in keys.rs) |
| L6 | title row appearing on first report resizes → contained vt100 panic resets CJK screens. Known; documented, no change. | — |
| pre | copy/drag `clamp(r.y, r.y+r.h-1)` panics if pane h=0 (read, not run). Guard h==0. | R2a |
