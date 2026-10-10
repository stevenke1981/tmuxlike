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
- 複製模式：vi 風格游標移動與選取（`v` 字元 / `V` 整行），`y` 複製；滑鼠拖曳選取自動複製；`prefix ]` 貼上；同時透過 OSC 52 寫入系統剪貼簿
  Copy mode with vi-style movement and selection (`v` / `V`, `y` to yank), mouse-drag selection, `prefix ]` to paste, system clipboard via OSC 52
- `:` 指令列，支援 tmux 風格指令 · `:` command prompt with tmux-style commands
- 自訂按鍵（config.toml 的 `[bind]` / `unbind`，或執行期 `bind-key` / `unbind-key`）· Custom key bindings
- Session／視窗選單（`prefix s`）· Interactive session/window tree chooser
- 重新命名視窗（`prefix ,`）與 session（`prefix $`）；視窗名稱自動依前景程式命名（手動命名後停止）
  Rename window / session; windows auto-named after the foreground process (Linux/macOS) until renamed manually
- 不需重啟即可重新載入設定（`prefix r` 或 `source-file`）· Reload config without restarting (`prefix r` / `source-file`)
- TOML 設定檔；側邊欄與狀態列支援中日韓全形字寬 · TOML config; CJK-width-aware sidebar/status bar
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
tmuxlike source-file            # 重新載入設定 / reload config in the running server
tmuxlike set mouse off          # 任何伺服器指令皆可從命令列執行 / run any server command from the shell
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
| `[` / `PgUp` | 複製模式 Copy mode（見下 see below）|
| `]` | 貼上緩衝區 Paste buffer |
| `:` | 指令列 Command prompt |
| `,` | 重新命名視窗 Rename window |
| `$` | 重新命名 session Rename session |
| `s` / `w` | Session／視窗選單 Session/window chooser (`↑↓` `Enter`, `x` 刪除 kill, `Esc`) |
| `(` `)` | 上一個／下一個 session Previous / next session |
| `a` or `b` | 切換側邊欄 Toggle status sidebar |
| `r` | 重新載入設定 Reload config |
| `?` | 列出按鍵 List key bindings (`j/k/Space` 捲動 scroll) |
| `d` | 離開（detach）Detach |
| `Ctrl-b` | 傳送前綴鍵給程式 Send prefix to the program |

### 複製模式 Copy mode

| 按鍵 Key | 動作 Action |
|---|---|
| `h j k l` / 方向鍵 arrows | 移動游標 Move cursor（到頂/底自動捲動 scrolls at edges）|
| `w` / `b` | 下一個／上一個字 Next / previous word |
| `0` / `$` | 行首／行尾 Line start / end |
| `H` / `L` | 畫面頂／底 Top / bottom of screen |
| `PgUp` `PgDn` / `C-b` `f` / `u` `d` | 翻頁／半頁 Page / half page |
| `g` / `G` | 最舊／最新 Oldest / newest |
| `v` or `Space` | 開始字元選取 Start character selection |
| `V` | 整行選取 Line selection |
| `y` / `Enter` | 複製並離開 Copy and exit |
| `Esc` | 取消選取（再按離開）Clear selection (again to exit) |
| `q` | 離開 Exit |

滑鼠：在窗格內拖曳即選取，放開時自動複製（含 OSC 52 系統剪貼簿）。
Mouse: drag inside a pane to select; releasing copies (also to the system clipboard via OSC 52).

## 指令 Commands

可在 `prefix :`、`bind`、`commands = [...]`、文字設定檔或命令列（`tmuxlike <command>`）使用。
Usable from `prefix :`, key bindings, `commands = [...]`, plain command files, or the shell (`tmuxlike <command>`).

```
split-window [-h|-v]          new-window [-n NAME]        rename-window [NAME]
rename-session [NAME]         kill-pane   kill-window     kill-session   kill-server
select-pane -L|-R|-U|-D|-t :.+                            resize-pane -L|-R|-U|-D [N] | -Z
next-window  previous-window  select-window -t N          copy-mode [-u]   paste-buffer
choose-tree  switch-client -n|-p  detach-client           toggle-sidebar   command-prompt
source-file [PATH]            set OPTION VALUE            display-message TEXT   list-keys
bind-key KEY COMMAND...       unbind-key KEY | -a         send-prefix
```

按鍵名稱 Key names: `c`, `%`, `C-a`, `M-x`, `C-M-Left`, `Up`, `PageUp`, `F5`, `Space`, `Enter`, `Escape`, `BSpace`, `^A`…

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
automatic_rename = true # 依前景程式自動命名視窗 / auto-name windows
osc52 = true            # 複製時寫入系統剪貼簿 / copy to system clipboard via OSC 52

unbind = ["x"]          # 移除預設按鍵；"all" 移除全部 / remove defaults ("all" = every default)
commands = ["set busy-secs 2"]   # 載入後執行的指令 / commands run after loading

[bind]                  # prefix 後的自訂按鍵 / custom keys after the prefix
"C-k" = "kill-pane"
v = "split-window -h"
"M-1" = "select-window -t 1"
```

`set` 可用選項 options: `prefix`, `shell`, `mouse`, `sidebar`, `sidebar-width`, `scrollback`/`history-limit`（僅影響新窗格 new panes only）, `busy-secs`, `automatic-rename`, `osc52`/`set-clipboard`.

重新載入 Reload: `prefix r`、`:source-file [PATH]` 或 `tmuxlike source-file`。`.toml` 檔會整個取代設定；其他副檔名視為每行一條指令（類似 `.tmux.conf`）。
A `.toml` file replaces the whole config; any other file is run line by line as commands (like `.tmux.conf`).

環境變數 Env: `TMUXLIKE_SOCKET` 可指定 socket 名稱以執行多個獨立伺服器 / set to run independent servers.

## 限制 Limitations

- 視窗自動命名僅支援 Linux / macOS；Windows 使用 shell 名稱 / Automatic window naming works on Linux/macOS only; Windows uses the shell name
- OSC 52 需要外層終端支援（Windows Terminal、iTerm2、kitty、WezTerm 等）；不支援時仍可用內部緩衝區 `prefix ]`
  OSC 52 requires a supporting outer terminal; the internal buffer (`prefix ]`) always works
- 只有一個按鍵表（prefix 表），尚無 `-n` 免前綴綁定、複製模式內搜尋、多個貼上緩衝區
  Single key table (prefix only): no `-n` root bindings, no search in copy mode, single paste buffer
- 窗格內程式的滑鼠事件不會轉送（滾輪在全螢幕程式中轉為方向鍵）/ Mouse events are not forwarded to apps (wheel becomes arrow keys in full-screen apps)
- 終端模擬由 `vt100` crate 提供，少數進階序列可能不支援 / Terminal emulation via the `vt100` crate; some advanced sequences may be unsupported

## 測試 Testing

`cargo test`（單元測試 unit tests）與 `python3 scripts/e2e_linux.py`（需 `pip install pyte`，在 Linux 上以虛擬終端驅動實際操作 / drives a real session through a pty on Linux）。

## License

MIT
