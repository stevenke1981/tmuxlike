# tmuxlike TUI - interaction spec (UX-1)

Task UX-1, actor ux-designer-01. Spec only; no code changed. Base `2fbdeec` (branch feat/herdr-alignment).
Evidence: read of `src/render.rs`, `src/server.rs`, `src/commands.rs`, `src/main.rs`, README; real frames from `target/release/tmuxlike.exe` (`debug-frame`, `capture-pane`; isolated socket/state/config; 120x30; sessions killed and `kill-server` run afterwards). herdr (Apache-2.0) is used for behavior ideas only: Goto picker with b/w/d/i filters, mode bar replacing the bottom row, focus-colored pane frame with title, `confirm_close`, toasts, mouse parity.
Constraints held: terminal-only, Windows Terminal/conhost + Linux + macOS, CJK-width-aware, no new deps, every current tmux binding keeps working.

## 1. Current-state audit

Frame A (demo, 2 panes, sidebar on, p1 blocked, p2 working; `debug-frame`, plain text, trimmed):
```
Windows PowerShell                            │Windows PowerShell                           │ tmuxlike · panes
...                                           │                                             │ ▸ demo
                                              │                                             │  ▲ 0.0 claude blocked
                                              │                                             │  ● 0.1 codex working
                                              │                                             │ ● work ▲ block ◆ done ○ i   <- legend clipped
[demo] 0:powershell*                                                                                     C-b | tmuxlike
```
Frame B (session named `測試`): status row is `[測試]   0:powershell*` (3 spaces, normally 1). Frame C (`pane zoom p1 --on`): row stays `0:powershell*`, no zoom flag. CLI: `tmuxlike rename-session X` with no client prints `RenameSession(Some("X")): needs an attached client`.

| Surface | Problem | Sev | Evidence |
|---|---|---|---|
| Status bar: agent state | A blocked/done agent is invisible in the bar. Only unseen output turns a non-active window yellow: no glyph, no count, nothing about other sessions. With default `sidebar=false` the user cannot see agent state at all. | H | render.rs:432-445; config.rs:42; server.rs:300 (`attention` = any output) |
| Status bar: geometry | Label offset uses `name.len()` (bytes), not display width. CJK session names shift every label and the click hit-test (`status_hit`) by the same amount. | H | render.rs:156; Frame B |
| Status bar: zoom, overflow | Active zoomed window shows `*` only (the `Z` branch is reached only for inactive windows). Many windows overdraw the right segment; the active label can fall off-screen while its hit area stays live. | M | render.rs:159-165, 444, 458; Frame C |
| Status bar: right segment | The only free space carries a brand string, not help or alert info. | M | render.rs:446-450 |
| Prefix feedback | Only the right corner flips to yellow `PREFIX`. No list of next keys; an unbound key is dropped silently; the prefix never expires. | H | render.rs:446-447; server.rs:1226-1238 (`None => {}` at 1236) |
| Pane borders / focus | A divider cell is accent-colored if within 1 cell of the active rect. In a 2-pane window the shared divider is always "near", so focus cannot be told apart (the hardware cursor is the only cue, and claude/codex hide it). Single pane: no cue. Panes have no title, label, index or state. | H | render.rs:274-312 (295-301) |
| Sidebar | Legend (28 cols) is wider than the text area (25): `idle` is cut. Focus = green/bold only (invisible without color). Blocked `message` is never shown. No session rollup. Not clickable. Silently absent when `cols <= sidebar_width+20`. `done` uses ANSI Blue (low contrast on conhost/xterm default palettes). | M | render.rs:315-407 (367-371, 401); server.rs:150-155; no sidebar branch in handle_mouse |
| Chooser | Window rows show only ●/▲, no done glyph, no pane rows, no filter. Any unknown key (Left/Right/Tab/typo) closes it. `x` kills a window or a WHOLE session at once. A mouse click anywhere closes it, so rows are not clickable. Key hints are crammed into the border title and truncated when narrow. | H | render.rs:579-628 (623); server.rs:1159-1194 (1177-1189, 1190), 1028-1033 |
| Destructive actions | `prefix x`, `prefix &` and chooser `x` act immediately (tmux itself confirms x and &). Agent work in the pane is lost without warning. | H | commands.rs:483-484; server.rs:804-814 |
| Prompt (`:`, rename) | Append/backspace only: no cursor keys, Home/End, Delete, word delete, history. Rename pre-fills the old name with the cursor stuck at the end. The prompt hides the window list. | M | server.rs:1195-1225; render.rs:461-487 |
| Messages | Info and error share one yellow bar and vanish in about 3 s. CLI errors leak Rust Debug. Killing the attached session silently moves the client to another session. | M | render.rs:488-501; server.rs:765, 1630-1635, 551-557 |
| Help popup (`prefix ?`) | Raw tmux command strings, alphabetical by key (`0..9`, `C-Down`...), about 65 lines; the scroll hint is on line 1 and scrolls away; no position indicator or filter; any other key closes it. | M | server.rs:959-973; render.rs:629-632 |
| Copy mode | Only a `[copy n/N]` label; no key hints; unmapped keys are swallowed silently. No search (known limit). | M | render.rs:251-260; server.rs:1376 |
| Mouse | Click focuses pane, status labels switch window, wheel scrolls, drag copies. Missing: sidebar rows, session name, overlay rows, divider drag, right-click menu. A click does not cancel a pending prefix. Mouse is not forwarded to apps (documented). | M | server.rs:1016-1126; README:272; main.rs:65 |
| Detach / empty / exit | `[detached (from session X)]` is clear. `[exited]` names no session or reason. A sidebar with no agent reports shows only idle rows and no hint how to report. First attach gives no hint that `C-b ?` exists. | L | client.rs:173-210; render.rs:315-395 |
| Glyph width | `● ▲ ◆ ○ ▸ │ ─ ┼` are East-Asian *Ambiguous* width. Width is computed narrow (right for Windows Terminal), but a CJK-codepage conhost (this machine is cp950) draws them wide and corrupts the diff-rendered frame. | M | render.rs:72-81 and glyph literals |

## 2. Target interaction model

**Hierarchy.** session (herdr workspace) > window (herdr tab) > pane. State rolls up pane -> window -> session -> status counters. One glyph+color language everywhere (glyph always present, color additive, NO_COLOR-safe): `▲` blocked (red, bold), `◆` done (cyan or 256-color 75, not ANSI Blue), `●` working (yellow), `○` idle (dim), `?` unknown (dim). Rollup precedence: blocked > done > working > unknown > idle.

**Modes.** Each mode has one status-bar signature and one exit; Esc always leaves, `q` also in overlays.
| Mode | Bar signature | Leave |
|---|---|---|
| terminal | `[sess] windows...   ▲n ◆n ●n \| C-b ? help` | - |
| prefix | which-key bar (UX-04), yellow | next key, Esc, click |
| overlay (chooser, help) | boxed list with key-hint footer | Esc, q, click outside |
| prompt / confirm | `:` or label; confirm bar is red | Enter, Esc; any non-`y` cancels confirm |
| copy | cyan `COPY` hint (UX-11) | q, Esc, y |

**Navigation.** Direct: `prefix 0-9 n p` (windows), `h j k l`/arrows (panes), `( )` (sessions), `o`. Search-and-jump: `prefix s|w|g` opens the tree chooser (`g` is the herdr Goto alias). Attention jump: `prefix Tab` / `BTab` = next/previous pane that needs me, across sessions.

**Agent states, "never hunt for the stuck one".** needs-me = blocked or done, computed from `agent::status` (so inferred done counts too). Surfaces: status counters (all sessions), window-tab glyph, pane title, sidebar row, chooser row, toast, jump key. Jump order: all blocked first (cyclic, starting after the current pane in session/window/pane order), then done. Clearing: done clears when the pane is viewed (existing, server.rs:1647-1658; landing by jump counts as viewing); blocked is truthful state and clears only when the agent reports another state. A transition into blocked on an unviewed pane raises a toast `▲ claude blocked in demo 0.1: need approval (C-b Tab)`. Plain output in a background window keeps today's yellow tab (tmux monitor-activity), separate from the glyphs.

**Discoverability.** (1) permanent `C-b ? help` in the right segment; (2) which-key bar on prefix; (3) one-time first-attach toast; (4) sidebar empty-state line; all gated by `hints` (default on). An unbound key after prefix says so.

**Prefix feedback.** Immediate which-key bar of live bindings (derived from the bindings table, so unbind/rebind is honored). `prefix-timeout` is P2 (default off = today).

**Confirmations (herdr `confirm_close`, tmux `confirm-before`).** Interactive destructive keys ask y/n; typed commands, CLI and API stay immediate (same split as herdr). New tmux-compatible `confirm-before [-p TEXT] CMD`; default bindings for `x`/`&` and chooser `x` use it. `confirm-close = on|off`, default `on` (matches tmux's own x/&; `off` restores today). The prompt names the target and its agent state.

**Mouse parity.** Everything navigational is clickable: status labels (exists), `[session]` -> chooser, counters -> next-attention, sidebar rows, pane/title row, chooser rows, wheel in overlays. Clicks cancel a pending prefix. Divider drag and right-click menu are P2.

**Preserve (asserts in `scripts/e2e_linux.py`):** `(rename-window)`, `unknown command`, `sourced`, `choose session`, `work: 2 windows`, `detached`, sidebar `idle|working`, `new-window` in help, every `DEFAULT_BINDINGS` key.

## 3. Changes (estimates = non-test changed lines; P0 345 + P1 265 = about 610)

**UX-00 (P0, ~35) debug-frame test hooks.** `debug-frame -t S [--text] [--prefix] [--prompt KIND:TEXT] [--chooser[=N]] [--popup] [--message TEXT] [--error] [--copy]`; `--text` dumps the grid as plain rows (exactly `rows` lines, no ESC, wide cells folded). Files: commands.rs:44,253; server.rs:1456-1470 (`Client` literal). Accept: `debug-frame -t demo --text` exits 0, prints 30 lines, none contains `\x1b`; flags compose; with no flags output is byte-identical to today.

**UX-01 (P0, ~55) Status-bar windows.** Per window `idx:name` + rollup glyph + flag (`*`, `Z`, `*Z`); width-based offsets; labels clipped to `w - right_len - 1`; on overflow drop leftmost labels so the active one stays visible and draw `<`; `status_hit` uses the same layout (pure fn `status_layout(s, w, right_len)`). Files: render.rs:155-179, 423-459. Accept: demo frame last row starts `[demo] 0:powershell*▲`; after `pane zoom p1 --on` it starts `[demo] 0:powershell*Z▲`; session `測試` row starts `[測試] 0:powershell*` (one space); unit test: 12 windows in 60 cols keeps the active label inside columns 0..50 and `status_hit` matches drawn columns.

**UX-02 (P0, ~25, same code area) Right segment.** `▲n ◆n ●n | C-b ? help ` with counters over all sessions, only n>0, never dropped; help text dropped first when narrow; `hints=off` gives ` C-b `. Files: render.rs:446-459. Accept: demo frame last row ends `▲1 ●1 | C-b ? help`; with no agents it ends ` C-b ? help`; after `set hints off` it ends ` C-b`.

**UX-03 (P0, ~60) `next-attention`.** Command `next-attention [-p]`; bindings `Tab` (next) and `BTab` (previous), both free today. Pick blocked panes first, else done, cyclic from the current pane over all sessions; switch session, then `focus_pane`; message `▲ claude 0.1 blocked: need approval`; none -> `no pane needs attention`. Headless (no client): focuses in the most recent session, prints `p1 blocked`, exit 1 if none. Toast on transition to blocked for an unviewed pane in `report_agent` (5 s, suppressed during a prompt). Files: commands.rs (Cmd, parse, DEFAULT_BINDINGS, test at :566), server.rs `exec` (:759), api/agent.rs `report_agent`. Accept (CLI, no client): report p1 done and p2 blocked, `pane focus p3`; `tmuxlike next-attention` prints `p2 blocked`, `pane get p2` shows focused; a second call exits 1 with `no other pane needs attention`; with only p1 done it focuses p1 and the next render turns it idle. Toast (pty e2e): `report-agent p2 --state blocked` while p1 is focused -> screen shows `▲ codex` within 1 s.

**UX-04 (P0, ~50) Which-key bar + unbound notice.** While `c.prefix` the whole bar becomes ` PREFIX ` + `key label` pairs in priority order (`c new`, `n/p win`, split keys, `h/j/k/l move`, `z zoom`, `x kill`, `Tab alert`, `s tree`, `[ copy`, `d detach`) with a reserved right end ` ? all  Esc cancel`; pairs that do not fit are dropped, never cut. Keys come from reverse lookup in `st.bindings` (command-prefix match), so rebinding shows the real key. Unbound key after prefix -> message `C-b Q is not bound (C-b ? lists keys)`. Files: render.rs:446-458 (+ pure fn `which_key(bindings, width)`), server.rs:1233-1237. Accept: `debug-frame -t demo --prefix --text` last row contains `c new`, `x kill`, `Tab alert` and ends `? all  Esc cancel`; after `unbind-key x` there is no `x kill`; unit test: width 40 still ends `? all  Esc cancel`; pty e2e: `C-b Q` shows `not bound`.

**UX-05 (P0, ~80) Confirmations.** `confirm-before [-p TEXT] CMD...`; `confirm-close on|off` in `Config::set` (config.rs:131) and TOML; `PromptKind::Confirm` with a pending action (command line, or (session, window) from the chooser); red bar `TEXT (y/n) `; `y` runs, anything else cancels; stale target -> `target gone`. Default bindings `x` -> `confirm-before kill-pane`, `&` -> `confirm-before kill-window`; chooser `x` confirms for windows and sessions (`kill session work (2 windows, 1 blocked)? (y/n)`). Typed `kill-*`, CLI and API unchanged. Files: commands.rs:443-500, config.rs, server.rs:1177-1189 and 1195-1225, render.rs:461-487. Accept: `debug-frame -t demo --prompt confirm:"kill pane p2 (codex, working)?" --text` last row = `kill pane p2 (codex, working)? (y/n) ` and the ANSI has a red background; pty e2e: `C-b x` leaves the pane count unchanged, `n` keeps it, `C-b x` `y` removes it; after `set confirm-close off` `C-b x` kills at once; `pane close p2` stays immediate; `list-keys` shows `confirm-before kill-pane`.

**UX-06 (P0, ~40) Sidebar polish.** Legend wraps to two rows when wider than the text area; focused row gets a `>` marker in the blank column; blocked `message` as a dim second line; session header gets counters (`▸ demo ▲1 ●1`); `done` color -> Cyan/AnsiValue(75); empty state `no agent reports: tmuxlike pane report-agent --help` (dim, only when no pane has `agent`); `prefix a` on a too-narrow terminal sets `sidebar needs more than 48 columns`. Files: render.rs:315-407, server.rs:920-927. Accept: `debug-frame -t demo --text` has a row ending `idle` and a row containing `need approval`, and the focused row contains `│ >▲ 0.0 claude blocked`; ANSI for the `◆` row uses SGR 36 or `38;5;75`, never `34`.

**UX-07 (P1, ~45) Mouse parity.** Shared `sidebar_rows(st, s) -> Vec<(u16, pane_id)>` used by render and mouse; click sidebar row -> switch session if needed + `focus_pane`; click `[session]` -> chooser; click counters -> next-attention; a click cancels pending prefix; wheel in chooser/popup moves selection/scroll. Files: render.rs, server.rs:1016-1126. Accept (pty e2e, SGR mouse): click on row `0.1 codex` focuses p2; click on `[demo]` opens the chooser; click on `▲1` focuses the blocked pane; the prefix bar disappears after a click.

**UX-08 (P1, ~80) Chooser v2.** Tree session > window > pane, each row with a rollup glyph (keep the `name: N windows` text); initial selection stays the current window; keys `j k ↑ ↓`, `Enter` jumps (pane row focuses that pane), `b w d i` filter blocked/working/done/idle, `a` all (title shows `[blocked]`), `x` confirm-kill (UX-05), `Esc`/`q` close, unknown keys ignored; footer row carries the hints and the title shortens to `sessions`; mouse: click row = jump, click outside = close, wheel = move. `g` -> `choose-tree`. Files: render.rs:579-628 (+ shared overlay-rect fn for hit-testing), server.rs:1159-1194 and 1028-1033, commands.rs bindings. Accept: `debug-frame -t demo --chooser --text` shows `demo: 1 windows`, one row per pane with its glyph, and a footer containing `b blocked`; filter `b` leaves only blocked panes; pty e2e: a stray `Left` or `z` no longer closes the overlay.

**UX-09 (P1, ~50) Prompt editing + message kinds.** `Prompt.cur` (char index): Left/Right/Home/End/Delete, Ctrl-a/e/k/u/w, Up/Down history for `:` (last 50, kept in `State`); render scrolls around the cursor using `unicode_width` and puts the terminal cursor at the cursor column; messages get a kind: error = red bg/white, 5 s; info = yellow, 3 s. Files: server.rs:112-125, 1195-1225; render.rs:461-501. Accept: pty e2e: `C-b ,` Left Left `X` Enter inserts `X` two chars before the end; `C-b :` `bogus` Enter shows `unknown command` on a red-bg row; Up recalls the previous command. `debug-frame --message x --error --text` last row = `x`, ANSI has red bg.

**UX-10 (P1, ~60) Pane titles (focus fix).** `pane-titles = auto|on|off`, default `auto` (clearly better: removes the 2-pane focus ambiguity and shows agent state with the sidebar off). Title = first row of each pane rect: `▲ claude · p1 · need approval`, width-truncated; active = bold black on accent, others dim. `auto` = window has >=2 visible panes, not zoomed, pane height >= 6. `visible_rects` reserves the row so `p.rect`, PTY size and mouse hit-tests agree; render marks it covered so no divider is drawn over it; clicking a title focuses the pane. Also fix divider accent: only the segment touching the active rect, heavy `┃ ━`. Files: server.rs:150-176, render.rs:195-312, config.rs. Accept: two-pane window: `pane list` rows 28 each (was 29), single-pane window still 29, `set pane-titles off` -> 29; `debug-frame --text` row 0 contains `▲ claude` in the left pane and `codex` in the right; ANSI shows the accent bg only on the focused title, and after `pane focus p2` it moves. Note: `scripts/api_smoke.py` has no rows/cols asserts (grep); README/API docs must mention the change.

**UX-11 (P1, ~30) Copy hint, first-run, session-ended.** Copy mode: right segment shows cyan `COPY  hjkl move  v/V select  y copy  q quit` (pane keeps `[copy n/N]`). First client attach per server run: 8 s message `C-b ? keys · C-b a agent sidebar · C-b Tab next alert` (when `hints`). `drop_clients_of` moving a client sets `session X ended - now in Y`. Files: render.rs:446-459, server.rs:538-562 and 1518-1531. Accept: `debug-frame -t demo --copy --text` last row contains `y copy`; pty e2e: two sessions, kill the attached one -> screen has `ended`; a fresh attach shows `C-b ? keys`.

**P2 (not counted).** UX-12 (~40) `glyphs = unicode|ascii` (`* ! + - ?`, borders `| - +`, `>` for `▸`; `TMUXLIKE_ASCII=1`) for CJK-codepage conhost. UX-13 (~25) outer title `OSC 2 tmuxlike ▲1 ◆2` and optional `bell-on-blocked` (BEL flashes the Windows Terminal tab/taskbar). UX-14 (~60) help popup grouped by task with one-line descriptions, `/` filter, `n/m` indicator, Esc/q only. UX-15 (~90) divider drag-resize and right-click menu (split, close, rename, zoom). UX-16 (~15) `prefix-timeout` ms (default 0 = never) and readable CLI errors (server.rs:765, drop the Debug dump).

## 4. Out of scope / deferred
- Root key table (`bind -n`), copy-mode search, multiple paste buffers (README limitations).
- Forwarding mouse to pane apps; herdr's right-click passthrough modifier.
- Renaming session/window to workspace/tab vocabulary; worktree UI; remote machines; plugins; sound; toast stacking.
- Theme system or user palettes (only the single `done` color fix is in scope); light-theme audit.
- Title row sharing a divider row for stacked panes (v1 reserves its own row; optimize later).
- Any GUI front-end.

## 5. Verification notes
- `debug-frame` prints a cursor-addressed diff; UX-00 `--text` removes the need for a parser. Color checks grep the ANSI form (`\x1b[41m`, `38;5;75`).
- Behaviors that need a client (prefix, chooser keys, mouse, toast, confirm) are verified with `scripts/e2e_linux.py` (pty); Windows/macOS rely on `debug-frame` plus unit tests of pure fns (`status_layout`, `which_key`, next-attention picker).
- Risks: UX-10 changes `rows` for split windows; UX-05 default-on changes `prefix x/&` (tmux parity, `confirm-close off` restores). Both need README and `main.rs` KEYS text updates (main.rs:51-65).
