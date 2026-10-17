# i18n wave — worker brief (coordinator: Opus 5.5; workers: Haiku 5.5 high)

Goal: every UI string a person reads in the TUI and on the command line can be shown in Traditional
Chinese (zh-TW); English stays the source and the default when the locale is unknown.

## Contract (commit on branch `feat/i18n`, read these first)
- `src/i18n/mod.rs`: `Lang::{En, ZhTw}`, `Lang::detect()` (client side), `Lang::resolve(cfg_lang, client)`,
  `tr(lang, "key") -> &'static str`, `trf(lang, "key", &[("name", &value)]) -> String` (`{name}` placeholders).
- Catalogs: `src/i18n/catalog/{render,messages,cli}.rs` — `pub const ENTRIES: &[Entry] = &[(key, en, zh_tw), ...]`.
  Keys are `area.thing` (e.g. `render.status.help`, `messages.cancelled`, `cli.help.usage`). Tests require every key
  unique, both texts non-empty, and identical `{placeholders}` in both.
- Where the language comes from: render code uses `cx.lang`; server/input/commands code uses `st.client_lang(cid)`
  (or `Lang::resolve(&st.cfg.lang, None)` when there is no client); client-side CLI code uses
  `Lang::resolve("auto", Lang::detect())` (config is not read by the CLI).
- Config: `lang = "auto" | "en" | "zh-TW"` (`set lang zh-TW`).

## Never translate
Key names (`C-b`, `Esc`, `Tab`, `Enter`), command names and their arguments (`kill-pane`, `split-window -h`),
option names/values, JSON API output (`{"result"}`, `{"error":{"code","message"}}`, status values `working` etc.),
pane/session ids, file paths, log lines (`crate::log!`), `debug-frame` without `--lang`.
Strings asserted by `scripts/e2e_linux.py`/`smoke.py`/`api_smoke.py` must still appear in **English** output.

## Glossary (zh-TW, use exactly)
| en | zh-TW | en | zh-TW |
|---|---|---|---|
| session | 工作階段 | window | 視窗 |
| pane | 窗格 | prefix (key) | 前綴鍵 |
| sidebar | 側邊欄 | chooser / tree | 選單 |
| agent | 代理 | agents (header) | 代理 |
| working | 執行中 | blocked | 待回應 |
| done | 已完成 | idle | 閒置 |
| unknown | 未知 | attached | 已連線 |
| detach | 離開 | kill / close | 關閉 |
| split | 分割 | zoom | 放大 |
| rename | 重新命名 | reload config | 重新載入設定 |
| help | 說明 | keys | 按鍵 |
| scroll | 捲動 | filter | 篩選 |
| move | 移動 | open | 開啟 |
| cancel / cancelled | 取消 / 已取消 | select | 選取 |
| copy / copied | 複製 / 已複製 | paste | 貼上 |
| copy mode | 複製模式 | command prompt | 指令列 |
| next alert | 下一個提醒 | more (overflow) | 更多 |
| not bound | 未綁定 | target changed | 目標已變更 |
| windows (count) | 個視窗 | panes (count) | 個窗格 |
Use full-width punctuation in prose（，。：？），keep `(y/n)` and key names half-width. Counts: `3 個視窗`.
Keep translations short: the status bar, which-key bar and sidebar are width-limited (CJK = 2 columns).

## Rules
Hard rules of `docs/team/herdr-alignment/brief.md` apply (own files only, no git writes, no new deps, isolated
servers only). Each worker owns ONE catalog file plus its listed source files. Add unit tests that render/format a
string in both languages (zh-TW output contains the glossary term; English output unchanged vs before).
Verify: cargo fmt, build (no warnings in your files), test, clippy, release build, `scripts/smoke.py` and
`scripts/api_smoke.py` (absolute exe path) — on this host the locale is zh-TW, so the scripts exercise zh-TW clients
where they attach; JSON/API output must stay English.
Report (≤ 40 lines): files+lines, number of keys added, commands+exit codes, zh-TW evidence excerpts, gaps.
