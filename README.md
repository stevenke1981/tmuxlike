# tmuxlike

跨平台終端多工器（terminal multiplexer），以 Rust 撰寫，功能類似 tmux / herdr。主要支援 **Windows** 與 **Linux**，相容 **macOS**。

A cross-platform terminal multiplexer written in Rust, inspired by tmux and herdr. Primary targets: **Windows** and **Linux**; also works on **macOS**.

## 功能 Features

- 伺服器／客戶端架構：工作階段（session）在背景持續執行，可 detach / attach
  Client/server architecture: sessions keep running in the background; detach and re-attach anytime
- 多個視窗（分頁）＋狀態列 · Multiple windows (tabs) with a status bar
- 水平／垂直分割窗格、切換焦點、調整大小、縮放、關閉 · Horizontal/vertical splits, focus movement, resize, zoom, close
- tmux 風格前綴鍵（預設 `Ctrl-b`）· tmux-style prefix key (default `Ctrl-b`)
- 捲動回溯（scrollback）與捲動模式 · Scrollback with scroll mode
- 滑鼠：點選聚焦窗格、點狀態列切換視窗、滾輪捲動 · Mouse: click to focus, click status bar to switch windows, wheel to scroll
- herdr 風格側邊欄：顯示所有 session / pane 的狀態（● 執行中 working、◆ 完成待查看 done、○ 閒置 idle），適合同時監看多個 AI agent
  herdr-like sidebar showing every pane's status (● working, ◆ done/needs attention, ○ idle) — handy for watching several AI coding agents
- TOML 設定檔 · TOML config file
- 平台後端：Windows 使用 ConPTY + Named Pipe；Linux/macOS 使用 PTY + Unix socket
  Backends: ConPTY + named pipes on Windows; PTY + unix sockets on Linux/macOS

## 安裝 Install

從 [Releases](../../releases) 下載執行檔，或自行編譯 / Download a binary from Releases, or build:

```sh
cargo install --git https://github.com/stevenke1981/tmuxlike
# or
git clone https://github.com/stevenke1981/tmuxlike && cd tmuxlike && cargo build --release
```

## 使用 Usage

```sh
tmuxlike                        # 連到最近的 session，沒有則建立 / attach last session or create one
tmuxlike new -s work            # 建立名為 work 的 session / new session "work"
tmuxlike attach -t work         # 重新連線 / re-attach (alias: a)
tmuxlike ls                     # 列出 sessions / list sessions
tmuxlike kill-session -t work   # 關閉 session
tmuxlike kill-server            # 關閉伺服器與所有 session
tmuxlike keys                   # 顯示快捷鍵 / show key bindings
```

## 快捷鍵 Key bindings

先按前綴鍵 `Ctrl-b`，再按 / Press the prefix `Ctrl-b`, then:

| 按鍵 Key | 動作 Action |
|---|---|
| `c` | 新視窗 New window |
| `n` / `p` | 下一個／上一個視窗 Next / previous window |
| `0`–`9` | 選擇視窗 Select window |
| `%` or `\|` | 左右分割 Split left/right |
| `"` or `-` | 上下分割 Split top/bottom |
| 方向鍵 Arrows / `h j k l` | 移動焦點 Move focus |
| `Ctrl`/`Alt`+方向鍵, `H J K L` | 調整窗格大小 Resize pane |
| `o` | 下一個窗格 Next pane |
| `z` | 縮放（最大化）窗格 Zoom pane |
| `x` | 關閉窗格 Kill pane |
| `&` | 關閉視窗 Kill window |
| `[` / `PgUp` | 捲動模式 Scroll mode (`↑ ↓ PgUp PgDn k j u d g G`, `q`/`Esc` 離開 exit) |
| `a` or `b` | 切換側邊欄 Toggle status sidebar |
| `s` / `(` `)` | 切換 session Switch session |
| `d` | 離開（detach）Detach |
| `Ctrl-b` | 傳送前綴鍵給程式 Send prefix to the program |

## 設定 Configuration

路徑 Path: `$TMUXLIKE_CONFIG`，或 or
- Linux: `~/.config/tmuxlike/config.toml`
- macOS: `~/Library/Application Support/tmuxlike/config.toml`
- Windows: `%APPDATA%\tmuxlike\config.toml`

```toml
prefix = "C-a"          # 前綴鍵 prefix key
shell = "pwsh -NoLogo"  # 預設 shell（Windows 預設 powershell.exe，Unix 為 $SHELL）
scrollback = 10000      # 回溯行數 scrollback lines
mouse = true            # 滑鼠支援 mouse support
sidebar = false         # 啟動時顯示側邊欄 show sidebar on start
sidebar_width = 28
busy_secs = 1.5         # 最後輸出後幾秒內視為 working / seconds after output counted as "working"
```

設定變更需重啟伺服器（`tmuxlike kill-server`）。Config is read when the server starts.

環境變數 Env: `TMUXLIKE_SOCKET` 可指定 socket 名稱以執行多個獨立伺服器 / set to run independent servers.

## 限制 Limitations

- 尚無複製/貼上選取、指令模式（`:`）、自訂按鍵綁定、session 選單 / No copy selection, command prompt, custom keybindings or session chooser yet
- 窗格內程式的滑鼠事件不會轉送（滾輪在全螢幕程式中轉為方向鍵）/ Mouse events are not forwarded to apps (wheel becomes arrow keys in full-screen apps)
- 終端模擬由 `vt100` crate 提供，少數進階序列可能不支援 / Terminal emulation via the `vt100` crate; some advanced sequences may be unsupported

## License

MIT
