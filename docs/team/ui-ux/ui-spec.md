# tmuxlike UI spec (UI-1): visual system + accessibility

Task UI-1 · actor ui-designer-01 · spec only, no code changed · base `2fbdeec` · inputs: `src/render.rs` (761 lines), `src/config.rs`, README, herdr screenshot/docs/`src/ui/*` (ideas only), real frames from `target/release/tmuxlike.exe`.
Direction: **herdr-like but terminal-native**: follow the user's ANSI theme (herdr `theme.name = "terminal"`), accent-colored active edge, dim secondary text, right-aligned state words, "agents" sidebar, toast chip. Kept from tmux: tiled panes without an outer frame, bottom bar.
**Invariants:** 1. Default colors are ANSI-16 indices, never truecolor. 2. State = SHAPE + WORD, color only reinforces. 3. Focus = shape (heavy edge, chip, pointer) + color. 4. All text uses display width; every cut ends in `…` (ASCII `~`). 5. No blink or animation. 6. `NO_COLOR` and `TERM=dumb` are honored by the chrome.

## 1. Visual audit
Sev: H = breaks a stated goal, M = degrades, L = polish. Contrast = WCAG ratio with Windows Terminal Campbell ANSI values (theme dependent, see section 5).

| Surface | Issue | Sev | Evidence |
|---|---|---|---|
| Light terminals | Chrome uses bright fg: White 97 (1.1:1 on white), Yellow 93 (1.2), Cyan 96 header (1.7), Green 92 border (2.3), Blue 94 done (3.9) | H | render.rs:301,337,352,366-370; frame `\e[1m\e[96m`tmuxlike · panes`, `\e[92m│` |
| Active border | Focus = color only (92 vs 90). Test is a +-1 bounding box of the active rect, so a whole shared column lights up and you cannot tell which side is active. No tee glyphs at joins (`┼`/plain `│`) | H | render.rs:290-301; frame: col 46 is `\e[92m│` on all 29 rows though only p1 (left) is active; row 14 border run is grey |
| NO_COLOR | `sgr_color` is hand-written to bypass NO_COLOR; no non-color fallback, no DIM attribute | H | render.rs:638-639, attrs 27-30 |
| Attention | Window needing attention = yellow bg only (color-only), no flag char | H | render.rs:439-440 |
| CJK / ambiguous width | Width helpers are CJK-correct (test render.rs:759) but `● ▲ ◆ ○ ▸ │ ─ …` are East-Asian *Ambiguous*: 2 cols on CJK-wide terminals while `width()` says 1, so rows shear (likely for zh-TW) | H | no opt-out in render.rs/config.rs |
| Status bar | Loud full-width bright-green fill; session, windows, hint same weight; active window only bold 92 on 40 | M | frame `\e[1m\e[30m\e[102m[demo]`, `\e[1m\e[92m\e[40m0:cmd*`; render.rs:413-445 |
| Status density | No agent summary; fixed `C-b \| tmuxlike` uses 16 cols; labels drawn to `w-a`, then overpainted by the right text (no `…`) | M | render.rs:444,446-459 |
| CJK hit-test | `status_labels` starts at `s.name.len()+3` (bytes), so CJK session names shift click targets | M | render.rs:156 |
| Sidebar legend | 28-col legend in a 25-col slot, clipped; `idle`+unseen shows `done` (`◆`) undocumented | M | frame row 28 `● work ▲ block ◆ done ○ i`; agent.rs `derive_status` |
| Sidebar rows | Hard cut without `…`; row loop stops at `h-1` but legend is at `h-2` (collision); `▸` on an expanded group; `0.2` index jargon; focused row = Green+Bold only | M | render.rs:343,359,381-390,396; frame row 3 `◆ 0.0 pi done` |
| Pane identity | No title/label in the pane; agent name only in the sidebar (herdr draws it inset in the border) | M | no title code in render.rs |
| Chooser / help | Instructions in the title (cut, no `…`), `├─` on last child, "1 windows", uncolored markers; help has no close hint, flat | M | render.rs:546-554,586-613,623,629-632 |
| Prompt/message | Replaces the whole status bar (window list lost); left-clip without `…`; no severity | M | render.rs:461-501,481-485 |
| Copy chip | `[copy 0/10000 sel]` over content row 1; contrast fine (black on 103, 12:1) but noisy | L | render.rs:252-260 |

**Blocker found while capturing (outside UI scope, hand to engineering):** `set sidebar on` on a 3-pane session with a CJK shell banner panics the server in `vt100-0.16.2/src/screen.rs:870` (`drawing_cell_mut(col+1).unwrap()` for a wide char at the last column after the pane narrows). The state mutex is then poisoned and every API call returns `{"code":"internal","message":"state lock poisoned"}` (seen in the stderr of a foreground `tmuxlike server`). Frames below were captured with `sidebar = true` in config before splitting. Suggest `catch_unwind` around vt100 calls and `lock().unwrap_or_else(PoisonError::into_inner)`.

## 2. Design tokens
SGR for index n: fg `30+n` (0-7) / `90+n-8` (8-15), bg `40+n` / `100+n-8`; 16-255 -> `38;5;n`; `#rrggbb` -> `38;2;r;g;b`. Contrast = on black `#0C0C0C` / on white for ANSI; on the preset bg for truecolor.

| Token | Use | ANSI default | 256 | truecolor dark / light | Contrast dark/light |
|---|---|---|---|---|---|
| `accent` | headers, current session, active title chip | 6 (`36`) | 74 | `#56b6c2` / `#0e7490` | ANSI 6.1/3.2; tc 8.9/5.4 |
| `muted` | hints, idle, unknown, state words | 8 (`90`) | 244 | `#7f8796` / `#6b7280` | ANSI 4.3/4.5; tc 5.8/4.8 |
| `border` | inactive pane edge, sidebar rule | 8 | 243 | `#6b7280` both | ANSI 4.3/4.5 |
| `border_active` | focused pane edge | 6 | 74 | = accent | = accent |
| `status_working` | `●` | 3 (`33`) | 179 | `#e5c07b` / `#a16207` | 7.5/2.6; tc 12.2/4.9 |
| `status_blocked` | `▲` (+bold) | 1 (`31`) | 167 | `#e06c75` / `#b91c1c` | 3.2/6.1; tc 6.6/6.5 |
| `status_done` | `◆` | 2 (`32`) | 114 | `#98c379` / `#15803d` | 5.7/3.4; tc 10.4/5.0 |
| `status_idle`, `status_unknown` | `○`, `?` | = muted | | = muted | |
| `bg_bar` / `fg_bar` | status bar (explicit pair, theme-proof) | 0 / 7 (`40`/`37`) | 235/252 | `#1f2430`/`#c8ccd4`; light `#e5e7eb`/`#1f2937` | 12.2 |
| `selection` | chooser row bg, fg = `bg_bar` | 6 (`46`) | 74 | = accent | 6.1 |
| `warn` | prefix/prompt/copy chip bg, fg black | 3 (`43`) | 178 | = status_working | 7.5 |
| `ok`, `error` | toast glyph | = done, = blocked | | | |

Stated weak spots: on a white terminal ANSI `33` is 2.6:1 and `32`/`36` ~3.3:1 (under 4.5 for text); mitigations are redundant shape+word and the `light` preset. The current bright codes are worse (1.1-2.3). Never use `97 93 96 92` as fg; never let `90` alone carry meaning (Solarized Dark maps it to the background).

`[theme]` proposal (`Config.theme: ThemeCfg`, `#[serde(default)]`):
```toml
[theme]
preset = "terminal"     # terminal (ANSI-16, default) | dark | light (truecolor) | mono (no color)
ascii_glyphs = "auto"   # auto | true | false (auto: TERM=dumb, TMUXLIKE_ASCII=1, non-UTF-8 locale)
[theme.colors]          # per-token override: 0-255 | "#rrggbb" | "reset"
border_active = "#e5c07b"
[ui]
clock = false           # right of status bar, HH:MM
pane_titles = "auto"    # off | auto (>1 pane and an agent/label exists) | always
```
Precedence: explicit `preset` > (`NO_COLOR` non-empty or `TERM=dumb` -> `mono`) > default. `mono`: no chrome fg/bg; focus -> INVERSE/BOLD, muted -> DIM (new attr bit, SGR 2), error -> BOLD; pane program output is passed through.

Glyph set (distinct silhouettes; `ascii_glyphs` swaps the whole table):

| Role | Unicode | ASCII | Role | Unicode | ASCII |
|---|---|---|---|---|---|
| working | `●` | `@` | border thin / tees | `│ ─ ├ ┤ ┬ ┴` | `\| - + + + +` |
| blocked | `▲` | `!` | border active | `┃ ━` | `# =` |
| done | `◆` | `+` | group open / closed | `▾ ▸` | `v >` |
| idle / unknown | `○` / `?` | `-` / `?` | pointer / ellipsis | `›` / `…` | `>` / `~` |
| ok / error | `✓ ✗` | `+ x` | tree mid / last | `├ └` | `\| \` |

## 3. Component specs
Exact cell grids (CJK = 2 cols, ambiguous glyphs = 1 col). Heavy edge = `border_active`, thin = `border`.

**3.1 Frame 80x24** (sidebar 28, panes 52; focus on p2 claude `blocked`). The 120x30 frame is the same chrome with 40 more pane columns and 6 more rows. Sidebar auto-hides when `cols <= sidebar_width+20` (server.rs:151).
```
$ cargo test              ┃Allow edit to render.rs? │ agents                    
   Compiling tmuxlike     ┃                         │                           
running 42 tests          ┃> 1. Yes                 │ ▾ demo                    
test result: ok.          ┃  2. No                  │   ◆ pi               done 
                          ┃  3. Always              │ › ▲ claude        blocked 
$                         ┃                         │   ● codex         working 
                          ┃(needs your answer)      │                           
                          ┃                         │ ▸ 測試代理人        ▲1 ●2 
                          ┃                         │ ▸ build                ○3 
                          ┃                         │                           
                          ┃                         │                           
                          ├━━━━━━━━━━━━━━━━━━━━━━━━━│                           
                          │cargo build --release    │                           
                          │   Compiling vt100       │                           
                          │   Compiling tmuxlike    │                           
                          │▮                        │                           
                          │                         │                           
                          │                         │                           
                          │                         │                           
                          │                         │                           
                          │                         │                           
                          │                         │ ●work ▲block ◆done ○idle  
                          │                         │ C-b a hide  C-b s switch  
 demo  0:cmd*  1:文件!  2:logs Z                                  ▲1 ●1 ◆1  C-b 
```
Active edge = only segments touching the focused rect (heavy + `border_active`); a join uses the thin tee, in `border_active` if any arm is active; the sidebar rule is always `border`. Other sessions collapse to one line with counts. `›` + bold marks the focused pane. State word is right-aligned `muted` and dropped when the name budget is < 4 cols (the shape stays). The 24-col legend fits the 25-col slot; widths >= 31 may expand it to `● work ▲ block ◆ done ○ idle`.

**3.2 Status bar.** Left: ` demo ` chip (`accent` bg, black bold), then `i:name` + flag (`*` active, `Z` zoom, `!` attention). Active window = `accent` chip; attention = bold `warn` fg + `!` (never color only). Right: agent counts (`▲` first, zero hidden, idle never shown), `C-b` hint or `PREFIX` chip (`warn`), optional clock. Narrow cut order: clock, hint text, counts, window names (`…`, active never cut), overflow windows into `+N`, session name (`…`). The right block is never overpainted. 120 cols (normal, prefix active), then 80 cols (three narrowing steps):
```
 demo  0:cmd*  1:文件!  2:logs Z  3:build                                                        ▲1 ●2 ◆1   C-b   14:05 
 demo  0:cmd*  1:文件!  2:logs Z  3:build                                                             ▲1 ●2 ◆1   PREFIX 

 demo  0:cmd*  1:文件!  2:logs Z                                  ▲1 ●1 ◆1  C-b 
 demo  0:cmd*  1:文件!  +2                                             ▲1 ●1 ◆1 
 開發會…  0:cmd*  +3                                                   ▲1 ●1 ◆1 
```

**3.3 Sidebar** 28 cols (left) and the 4-col collapsed rail (right; `[ui] sidebar_mode = "full"|"rail"|"off"`, `prefix a` cycles; rail = shape per pane, group glyph per session).
```
│ agents                        │   
│                               │   
│ ▾ demo                        │ ▾ 
│   ◆ pi               done     │ ◆ 
│ › ▲ claude        blocked     │ ▲ 
│   ● codex         working     │ ● 
│   ○ shell            idle     │ ○ 
│   ? tool          unknown     │ ? 
│                               │   
│ ▸ 測試代理人        ▲1 ●2     │ ▲ 
│ ▸ build                ○3     │ ○ 
│                               │   
│ ●work ▲block ◆done ○idle      │   
│ C-b a hide  C-b s switch      │   
```
Row = `ptr sp glyph sp name ... state`. Current session header `accent` bold; other sessions `fg`, collapsed with counts. Footer = rule, legend, key hint (prefix from config). List height = `h - 1 - footer_rows`; overflow ends with `… +N more`.

**3.4 Pane titles** (UI-11, P2: reserves each pane's first row; touches `visible_rects`/PTY size in server.rs). `glyph name · state` left, pane id right in `muted`; focused title = chip (`accent` bg, black; INVERSE in mono), unfocused = `muted`; CJK-safe `…`. 52 cols, focus right:
```
 ○ 測試代理人 · 很長…  p5 │ ▲ claude · blocked   p2 
$ pytest -q               ┃Allow edit to render.rs? 
..........  [100%]        ┃> 1. Yes                 
$ ▮                       ┃  2. No                  
──────────────────────────┤━━━━━━━━━━━━━━━━━━━━━━━━━
```
**3.5 Chooser** (`prefix s`): title `Sessions`, hints in the bottom border, `└` for the last child, pluralization, selected row = `selection` bg full width (INVERSE in mono), per-token colored counts, `▲ more`/`▼ more` on the border when scrolling. **3.6 Help** (`prefix ?`): grouped, labels `muted`, close hint always visible, long lines cut with `…` (see `Copy`).
```
         ┌─ Sessions ─────────────────────────────────────────────────┐         
         │ ▾ demo            2 windows            (attached)          │         
         │     0 cmd*       3 panes      ▲1 ●1                        │         
         │     1 文件!      1 pane       ◆1                           │         
         │ ▸ 測試代理人      1 window     ○1                          │         
         │ ▸ build           1 window     ○3                          │         
         │                                                            │         
         │                                                            │         
         └─ ↑↓ move  ⏎ open  x kill  esc close ───────────────────────┘         

        ┌─ Keys · prefix C-b ──────────────────────────────────────────┐        
        │ Windows  c new  n/p next/prev  0-9 select  , rename  & close │        
        │ Panes    % split L/R  " split T/B  o next  z zoom  x close   │        
        │ Resize   C-arrows / H J K L                                  │        
        │ Copy     [ enter  hjkl move  v/V select  y yank  q quit  ] … │        
        │ Session  s chooser  ( ) prev/next  $ rename  d detach        │        
        │ Misc     a sidebar  : prompt  r reload  ? this help          │        
        │                                                              │        
        └─ j/k scroll   esc close ─────────────────────────────────────┘        
```
**3.7 Prompt and toast** (80 cols): the prompt owns the bar (tail kept visible, left cut marked `…`); a message never erases the window list, it is a right-aligned chip over the counts block (`ok`/`error` glyph + token fg) that auto-clears as today (server.rs:193,1634).
```
 : split-window -h█                                                             
 (rename-window) 開發主機█                                                      
…very-long-command --with --many --args and-the-tail-stays-visible█             
 demo  0:cmd*  1:文件!  2:logs Z                              ✓ copied 42 chars 
 demo  0:cmd*  1:文件!                 ✗ unknown command: a-very-long-error-me… 
```

## 4. Implementable changes (P0+P1 about 540 changed lines incl. ~100 test lines; no new dependencies)
Acceptance uses `tmuxlike debug-frame -t S` in the isolated env of section 6.

| ID | Pri | What / where | Acceptance |
|---|---|---|---|
| UI-01 | P0 | `Theme` (tokens, glyph table, mono/ascii flags) from `Config.theme` + env: new `src/theme.rs` (~110 lines), `ThemeCfg` in config.rs, `&Theme` into `render_client`; replaces every `Color::` literal (render.rs:259,301,337,352,366-370,387,417-458,472,494,540-559) | default frame: no chrome fg `\e[97m \e[93m \e[96m \e[92m \e[94m`; active-edge SGR == `border_active` (`\e[36m`) |
| UI-02 | P0 | NO_COLOR / TERM=dumb per attach (`Client.mono`); `sgr_color` returns `""` for chrome in mono; DIM bit (render.rs:27-30,638,694-711) | frame for a `NO_COLOR=1` client: no chrome `\e[3Xm/4Xm/9Xm/10Xm`; focus uses `\e[7m`, muted `\e[2m` |
| UI-03 | P0 | Per-segment active edge, heavy glyphs + tees: rewrite render.rs:273-312 (`near` -> adjacency to the active rect's edge) | p2 active: `┃`/`━` only on cells touching p2, each preceded by `\e[36m`; all other edges `│`/`─` with `\e[90m`; no `┼` at T joins |
| UI-04 | P0 | Status glyph/color from tokens, ASCII table, legend fits `tw` (render.rs:365-371,396-406) | `● ▲ ◆ ○ ?` map to `33 31 32 90 90`; `ascii_glyphs=true`: no non-ASCII chrome; legend width <= 25 |
| UI-05 | P0 | Status bar chips, `!` flag, counts, hint, cut order, no overpaint; `status_labels` on display width, hit-test in sync (render.rs:154-179,409-459) | last row shows `▲1 ●1 ◆1` for the demo state; every label start col == `status_hit` col with a CJK session name; no `\e[102m` fill |
| UI-06 | P0 | Sidebar: group collapse, pointer, right-aligned state, `truncate` with `…`, list excludes footer (render.rs:314-407) | no row wider than 25; focused row has `›` and `\e[1m`; 20 panes at h=30 keep footer rows |
| UI-07 | P1 | Overlay: border hint, short titles, `└`, plural, scroll markers, theme selection (render.rs:503-633) | chooser last border row contains `esc close`; "1 window" singular |
| UI-08 | P1 | Prompt left-cut `…`; message = right chip, window list kept (render.rs:461-501) | with a message active the last row still contains `0:`; over-long prompt starts with `…` |
| UI-09 | P1 | Copy chip `copy 12/10000` + `v`/`V` word, `warn` token (render.rs:251-260) | chip <= 20 cols, black on `\e[43m` |
| UI-10 | P2 | `ambiguous_wide` option: ASCII chrome + width override for CJK-wide terminals | no ambiguous-width glyph in a frame with the option on |
| UI-11 | P2 | Pane title rows + layout reserve (3.4) | frame shows `▲ claude · blocked`; PTY rows = rect.h-1 |
| UI-12 | P2 | `sidebar_mode = rail`, `prefix a` cycle | rail frame = 4 cols |
| UI-13 | P2 | `[ui] clock`, `COLORFGBG` -> light, toast severity level on `Client.message`; README `[theme]`/`[ui]` docs | `HH:MM` right of hint; error toast shows `✗` |

## 5. Accessibility checklist
- [ ] State = shape + word + color: `● @`, `▲ !`, `◆ +`, `○ -`, `?` (disc, triangle, diamond, ring, mark: red/green and blue/yellow color blindness cannot merge them; `●` vs `○` differs in fill and always has the word beside it).
- [ ] Focus never color-only (heavy edge, chip/INVERSE, `›`, bold); attention never color-only (`!` flag).
- [ ] Contrast intent: text >= 4.5:1, non-text >= 3:1 on the theme bg. ANSI-16 values belong to the user's terminal theme, so this is intent, not a guarantee (`33` 2.6:1 and `32`/`36` ~3.3:1 on white; use `preset = "light"` or overrides). Bars use explicit fg/bg pairs (12:1) so they hold on any theme.
- [ ] Reduced motion: SGR 5/6 never emitted, no spinners, no timed color change; the only timer is toast auto-clear and it does not move layout. Test: no `\e[5m`/`\e[6m` in any frame.
- [ ] NO_COLOR (no-color.org): a non-empty value drops chrome color, keeps bold/inverse/dim and the Unicode shapes (deliberate: shapes carry the meaning; ASCII is a separate switch, so NO_COLOR alone does not force it). Explicit `[theme] preset` overrides it. `TERM=dumb` or no UTF-8 locale: ascii + mono automatically.
- [ ] CJK and size: `unicode-width` widths, ambiguous glyphs 1 col unless `ambiguous_wide` (UI-10), cuts never split a wide char; sidebar auto-hides below 49 cols; status bar degrades per 3.2.
- [ ] Screen readers, honestly: a multiplexer repaints a character grid with no roles or live regions and the diff order is not reading order, so NVDA/VoiceOver/Orca cannot follow it reliably. Mitigation is textual: state is always also a word, and `capture-pane -p`, `pane read`, `agent get`, `api snapshot` give linear text/JSON; a `--plain` line-oriented status view is recommended (not specified here).
- [ ] Mouse is optional: every status/sidebar action has a key path (`prefix s/w/a/0-9`).

## 6. Evidence and limits
Env: `TMUXLIKE_SOCKET=uispec-$RANDOM`, `TMUXLIKE_STATE=<tmp>/state.json`, `TMUXLIKE_CONFIG=<tmp>/cfg.toml` (`sidebar = true`, `shell = "cmd.exe"`). `new -d -s demo`, two `pane split`, three `pane report-agent` (claude blocked, codex working, pi idle), `debug-frame -t demo`, `kill-server`. Observed: p1 cols 0-45, border col 46, p2/p3 cols 47-91, sidebar rule col 92, text from col 94, status row 29. Not verified: real attach rendering (chooser/prompt/help/toast mockups derive from render.rs, not captured), 80x24 frames (detached sessions are 120x30), appearance on a light terminal (contrast computed, not viewed), truecolor presets. After each UI-nn regenerate the frame in the same state and diff its SGR runs against section 2; section 3 grids are the cell reference.
