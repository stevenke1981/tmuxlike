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
- JSON 自動化 API：`tmuxlike pane|agent|session|api ...` 以 JSON 列出／分割／輸入／讀取窗格，回報與等待 agent 狀態（見下節）
  JSON automation API: `tmuxlike pane|agent|session|api ...` lists, splits, types into and reads panes, and reports and waits on agent state (see below)
- 版面保存：伺服器重啟後還原 session、視窗、分割與窗格目錄（Linux 為即時目錄，其他平台為啟動目錄）（`persist`）
  Layout persistence: sessions, windows, splits and pane directories are restored after a server restart (live directories on Linux, start directories elsewhere) (`persist`)

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
tmuxlike new -d -s bg           # 在背景建立 session / create a session without attaching
tmuxlike send-keys -t bg 'echo hi' Enter   # 輸入文字或按鍵 / type text or keys
tmuxlike capture-pane -p -t bg  # 印出窗格內容 / print the pane's visible text
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

## 自動化 API Automation API (herdr-style)

以 JSON 指令透過同一個本機 socket 操作伺服器，讓 script 或 coding agent 可以列出、分割、輸入與讀取窗格，並回報與等待 agent 狀態。不需要另外啟動程式。
A JSON command interface over the same local socket, so scripts and coding agents can list, split, type into and read panes, and report and wait on agent state. No extra process is needed.

### 慣例 Conventions

- 成功：stdout 印出 `{"result": ...}`，exit 0；`pane read` 與 `agent read` 直接印出純文字。
  Success: `{"result": ...}` on stdout with exit 0; `pane read` and `agent read` print raw text.
- 失敗：stderr 印出 `{"error": {"code": "...", "message": "..."}}`，exit 1。命令語法錯誤（例如未知選項）exit 2。
  Failure: `{"error": {"code", "message"}}` on stderr with exit 1. Syntax errors (for example an unknown option) exit 2.
- 常見錯誤代碼 Common codes: `pane_not_found`, `agent_not_found`, `session_not_found`, `server_not_running`, `invalid_argument`, `timeout`, `agent_blocked`, `name_taken`, `no_neighbor`, `spawn_failed`, `internal`.
- 窗格 ID 為 `p<N>`，同一次伺服器執行期間不變；伺服器重啟後請重新以 `pane list` 查詢。也接受 tmux 風格的 `%N`。
  Pane IDs are `p<N>` (also accepts tmux-style `%N`). They are stable while one server runs; query `pane list` again after a restart.
- 每個窗格的環境變數 `TMUXLIKE_PANE_ID` 即是該窗格的 ID；`--current` 指向呼叫者所在窗格。省略 ID 時，目標為最近使用之 session 的焦點窗格。
  Each pane has `TMUXLIKE_PANE_ID` set to its own ID; `--current` targets the calling pane. An omitted ID means the focused pane of the most recently used session.
- 伺服器未執行時，API 指令在 stderr 印出 `{"error":{"code":"server_not_running","message":"..."}}` 並 exit 1。
  With no server running, API commands print `{"error":{"code":"server_not_running","message":"..."}}` on stderr and exit 1.
- `tmuxlike <group> --help`（例如 `tmuxlike pane --help`）把說明印到 stdout 並以 exit 0 結束，不需要伺服器；`tmuxlike pane` 不帶子指令時亦同。
  `tmuxlike <group> --help` (for example `tmuxlike pane --help`) prints the help to stdout and exits 0 without a server; `tmuxlike pane` with no subcommand behaves the same.
- `pane wait-output` 以邏輯行比對：自動換行（soft wrap）的列會先接合再比對，因此超過窗格寬度的字串仍可符合。`--match` 為字面子字串，`--regex` 為正規表示式。
  `pane wait-output` matches logical lines: soft-wrapped rows are joined before matching, so a string wider than the pane still matches. `--match` is a literal substring; `--regex` is a regular expression.

### 指令 Commands

| 群組 Group | 指令 Command | 說明 Description |
|---|---|---|
| pane | `pane list [--session S]` | 列出窗格 List panes |
| pane | `pane get <id>` | 單一窗格資料 One pane |
| pane | `pane current [--pane ID \| --current]` | 查詢目前窗格 Current pane |
| pane | `pane split [ID] --direction right\|down [--ratio F] [--cwd P] [--focus\|--no-focus]` | 分割窗格，預設不改變焦點 Split; focus unchanged by default |
| pane | `pane focus <id>` / `pane focus --direction D` | 移動焦點 Move focus |
| pane | `pane zoom [ID] [--toggle\|--on\|--off]` | 縮放 Zoom |
| pane | `pane rename <id> <label>` / `--clear` | 設定窗格標籤 Pane label |
| pane | `pane close <id>` | 關閉窗格 Close pane |
| pane | `pane read <id> [--source visible\|recent] [--lines N]` | 讀取文字 Read text |
| pane | `pane send-text <id> <text>` | 輸入文字，不送出 Enter Type text, no Enter |
| pane | `pane send-keys <id> <key>...` | 送出按鍵，例如 `enter`、`ctrl+c`、`esc`、`up`、`f1` Send keys |
| pane | `pane run <id> <cmd>` | 輸入指令並按 Enter Type a command and press Enter |
| pane | `pane wait-output <id> --match T \| --regex R [--lines N] [--timeout MS]` | 等待輸出出現 Wait for output |
| pane | `pane report-agent <id> --source S --agent L --state S [--message T] [--seq N] [-- RESUME...]` | 回報 agent 狀態 Report agent state |
| pane | `pane release-agent <id> --source S --agent L` | 釋放回報的 agent 狀態 Release reported state |
| agent | `agent list` | 列出 agent List agents |
| agent | `agent get\|read\|send-keys\|focus <target>` | 查詢、讀取、送鍵、聚焦 Inspect, read, send keys, focus |
| agent | `agent rename <target> <name>` / `--clear` | 設定唯一名稱；窗格須已有回報，名稱符合 `[a-z][a-z0-9_-]{0,31}` 且不可像窗格 ID（如 `p12`）Set a unique name; the pane must already have a report, and the name must match `[a-z][a-z0-9_-]{0,31}` and not look like a pane ID (such as `p12`) |
| agent | `agent wait <target> [--until S]... [--timeout MS]` | 等待狀態 Wait for a state |
| agent | `agent prompt <target> <text> [--wait] [--until S] [--timeout MS]` | 送出提示；被 blocked 時回傳 `agent_blocked` Submit a prompt |
| session / server | `session list` | 列出 session List sessions |
| session / server | `api snapshot` | 完整快照 Full snapshot |
| session / server | `server stop` | 保存版面後停止伺服器 Save layout, then stop the server |
| session / server | `kill-server` | 關閉所有 session 並刪除保存的版面 Close everything and delete the saved layout |

`<target>` 可以是窗格 ID 或唯一的 agent 名稱。`<target>` is a pane ID or a unique agent name.

### Agent 狀態 Agent status

| 狀態 State | 符號 | 意義 Meaning |
|---|---|---|
| working | ● | 執行中 Busy |
| blocked | ▲ | 等待核准或提問 Waiting for approval or a question |
| done | ◆ | 閒置，且有尚未查看的輸出 Idle, with output not yet viewed |
| idle | ○ | 閒置 Idle |
| unknown | ? | 無法可靠判斷，不代表已完成 Cannot classify; does not mean finished |

- 有回報時以回報為準：`pane report-agent` 的 `--seq` 必須較新才會套用，過期的回報回傳 `applied: false`。
  Reported state wins: `pane report-agent` applies only a newer `--seq`; a stale report returns `applied: false`.
- 沒有回報時以輸出活動推定：`busy-secs` 內有輸出為 working，否則有未查看的輸出為 done，其餘為 idle。tmuxlike 不會自動偵測 agent 種類。
  Without a report, status is inferred from output activity: output within `busy-secs` means working, unseen output means done, otherwise idle. tmuxlike does not detect agent kinds automatically.

### 範例 Examples

1. 分割、執行並等待輸出 Split, run, and wait for output：

```sh
pane=$(tmuxlike pane split --current --direction right --cwd "$PWD" --no-focus | jq -r .result.pane.pane_id)
tmuxlike pane run "$pane" "cargo test"
tmuxlike pane wait-output "$pane" --regex "test result" --timeout 120000
tmuxlike pane read "$pane" --source recent --lines 60
```

   Windows PowerShell：`$pane = (tmuxlike pane split --current --direction right --no-focus | ConvertFrom-Json).result.pane.pane_id`

2. Agent 回報 hook Agent report hook（在 agent 窗格內執行 run inside the agent's pane）：

```sh
tmuxlike pane report-agent "$TMUXLIKE_PANE_ID" --source my-hook --agent claude --state working --seq "$SEQ"
# 另一個 script 等待它閒置 another script waits for idle
tmuxlike agent wait "$TMUXLIKE_PANE_ID" --until idle --timeout 600000
```

3. 送出提示並等待 Prompt and wait：

```sh
tmuxlike agent rename p4 reviewer
tmuxlike agent prompt reviewer "review the diff" --wait --until idle --timeout 300000
tmuxlike agent read reviewer --source recent --lines 80
```

### 版面保存 Session persistence

- 保存：session 與視窗結構、分割方向與比例、窗格目錄（cwd）、agent 名稱與 resume 指令。版面改變時自動寫入。
  Saved: session and window structure, split directions and ratios, pane directories (cwd), agent names and resume commands. Written automatically when the layout changes.
- 工作目錄 Working directory：Linux 會即時追蹤窗格前景程序的目錄，保存與回報的 `cwd` 為目前目錄。Windows 與 macOS 不追蹤：保存與回報的 `cwd` 是窗格的啟動目錄，之後的 `cd` 不會反映。
  On Linux the live directory of the pane's foreground process is tracked, and the saved and reported `cwd` is the current directory. On Windows and macOS it is not tracked: the saved and reported `cwd` is the pane's start directory, and later `cd` commands are not reflected.
- 不保存：執行中的程式與行程、螢幕內容與捲動回溯、shell 變數與歷程。還原時每個窗格會在保存的 cwd 重新啟動 shell。
  Not saved: running processes, screen contents and scrollback, shell variables and history. On restore each pane starts a new shell in its saved cwd.
- Resume 指令：`pane report-agent ... -- RESUME...` 給的指令，還原後會在該窗格的 shell 中輸入並送出（Enter）。tmuxlike 依 POSIX shell 與 PowerShell 的引號規則組成指令，不支援 cmd.exe。第一個參數必須是程式名稱，不可為路徑（如 `./claude`、`C:\...`）；參數不可含單引號（含 PowerShell 的全形引號）或控制字元。不符合時不送出，並記入除錯紀錄。
  Resume commands: the command given after `--` in `pane report-agent` is typed and submitted (Enter) in the pane's shell after a restore. tmuxlike quotes it by POSIX shell and PowerShell rules; cmd.exe is not supported. The first argument must be a program name, not a path (such as `./claude` or `C:\...`), and arguments may not contain apostrophes (including PowerShell's typographic ones) or control characters. Otherwise nothing is sent and the rejection is logged.
- 位置：`$TMUXLIKE_STATE`；未設定時使用資料目錄。停用：設定檔 `persist = false`，或執行期 `tmuxlike set persist off`（會立即刪除保存檔）。
  Location: `$TMUXLIKE_STATE`, or the data directory if unset. Disable with `persist = false` in the config, or `tmuxlike set persist off` at runtime, which deletes the state file immediately.
- 何時刪除保存檔：關閉最後一個 session（例如 `kill-session`，或 `pane close` 關掉最後一個窗格）時，伺服器會刪除保存檔並結束，因此不會還原。只有 `tmuxlike server stop`，或伺服器異常結束／重新開機，會留下可還原的版面。
  When the file is deleted: closing the last session (for example with `kill-session`, or `pane close` on the last pane) deletes the state file and the server exits, so nothing is restored. Only `tmuxlike server stop`, or a crash or reboot, leaves a layout to restore.
- 損毀的保存檔：無法讀取或解析的檔案會移到 `<file>.bad`，伺服器以空白版面啟動；檔案不會被覆寫，供手動檢查。若只還原了部分 session，原始檔會複製為 `<file>.partial.bad`。
  Damaged state: a file that cannot be read or parsed is moved to `<file>.bad`, and the server starts with an empty layout. The file is never overwritten, so it can be inspected. If only some sessions could be restored, the original is copied to `<file>.partial.bad`.
- `tmuxlike server stop`：保存後停止伺服器，下次啟動會還原。`tmuxlike kill-server`：關閉所有 session 並刪除保存檔，下次不會還原。
  `server stop` saves and stops the server, so the next start restores the layout. `kill-server` closes everything and deletes the saved file, so nothing is restored.

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
persist = true          # 保存並還原版面 / save and restore the layout across server restarts

unbind = ["x"]          # 移除預設按鍵；"all" 移除全部 / remove defaults ("all" = every default)
commands = ["set busy-secs 2"]   # 載入後執行的指令 / commands run after loading

[bind]                  # prefix 後的自訂按鍵 / custom keys after the prefix
"C-k" = "kill-pane"
v = "split-window -h"
"M-1" = "select-window -t 1"
```

`set` 可用選項 options: `prefix`, `shell`, `mouse`, `sidebar`, `sidebar-width`, `scrollback`/`history-limit`（僅影響新窗格 new panes only）, `busy-secs`, `automatic-rename`, `osc52`/`set-clipboard`, `persist`.

重新載入 Reload: `prefix r`、`:source-file [PATH]` 或 `tmuxlike source-file`。`.toml` 檔會整個取代設定；其他副檔名視為每行一條指令（類似 `.tmux.conf`）。
A `.toml` file replaces the whole config; any other file is run line by line as commands (like `.tmux.conf`).

環境變數 Env: `TMUXLIKE_SOCKET` 可指定 socket 名稱以執行多個獨立伺服器 / set to run independent servers; `TMUXLIKE_STATE` 指定版面保存檔路徑 / path of the saved-layout file.

## 限制 Limitations

- 視窗自動命名僅支援 Linux / macOS；Windows 使用 shell 名稱 / Automatic window naming works on Linux/macOS only; Windows uses the shell name
- OSC 52 需要外層終端支援（Windows Terminal、iTerm2、kitty、WezTerm 等）；不支援時仍可用內部緩衝區 `prefix ]`
  OSC 52 requires a supporting outer terminal; the internal buffer (`prefix ]`) always works
- 只有一個按鍵表（prefix 表），尚無 `-n` 免前綴綁定、複製模式內搜尋、多個貼上緩衝區
  Single key table (prefix only): no `-n` root bindings, no search in copy mode, single paste buffer
- 窗格內程式的滑鼠事件不會轉送（滾輪在全螢幕程式中轉為方向鍵）/ Mouse events are not forwarded to apps (wheel becomes arrow keys in full-screen apps)
- 終端模擬由 `vt100` crate 提供，少數進階序列可能不支援 / Terminal emulation via the `vt100` crate; some advanced sequences may be unsupported
- 窗格工作目錄只有 Linux 即時追蹤；Windows／macOS 保存與回報的 `cwd` 是窗格啟動目錄 / Live working-directory tracking is Linux only; on Windows and macOS the saved and reported `cwd` is the pane's start directory
- 還原的 resume 指令只依 POSIX shell 與 PowerShell 的引號規則組成，不支援 cmd.exe / Restored resume commands are quoted for POSIX shells and PowerShell only; cmd.exe is not supported
- 尚未支援 SSH 遠端機器 / No SSH remote machines yet
- 尚未支援外掛 / No plugin system yet
- 不會自動偵測 agent 種類，需由 agent 或 hook 呼叫 `pane report-agent` 回報；否則只有輸出活動推定 / No automatic agent detection: agents or hooks must call `pane report-agent`, otherwise only output-activity inference is available
- 尚未支援 git worktree，以及 workspace／tab API（目前以 session／視窗對應）/ No git worktrees and no workspace/tab API yet (sessions and windows are used instead)

## 疑難排解 Troubleshooting

設定 `TMUXLIKE_LOG=路徑` 後啟動，伺服器會寫入除錯紀錄（窗格啟動、讀到的輸出等）。
Set `TMUXLIKE_LOG=/path/to/file` before starting to get a server debug log (pane spawns, pty output, etc).

## 測試 Testing

`cargo test`（單元測試 unit tests）、`python scripts/smoke.py`（CI 在 Linux / macOS / Windows 上執行：背景 session、確認 PowerShell/shell 提示字元、send-keys、ANSI 畫面 / runs on all three OSes in CI）、`python scripts/api_smoke.py`（自動化 API：分割、輸入、換行輸出、agent 狀態、錯誤碼、版面保存，以及關閉最後一個 session 後清除保存檔；三個平台的 CI 皆執行 / automation API: splits, input, wrapped output, agent status, error codes, layout persistence, and clearing the saved state when the last session closes; runs on all three OSes in CI）與 `python3 scripts/e2e_linux.py`（需 `pip install pyte`，在 Linux 上以虛擬終端驅動實際操作 / drives a real session through a pty on Linux）。

## License

MIT
