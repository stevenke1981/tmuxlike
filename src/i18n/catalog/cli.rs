//! cli strings: (key, en, zh-TW). Keys start with "cli.".
//!
//! Only descriptions are translated. Command names, flags, key names, option values, placeholder
//! syntax and JSON examples stay identical in both columns (tests and scripts match on them).
//! Usage errors and API JSON are not in this catalog on purpose; they stay English.
use super::super::Entry;

pub const ENTRIES: &[Entry] = &[
    (
        "cli.help",
        r#"tmuxlike - a cross-platform terminal multiplexer

USAGE:
  tmuxlike                       attach to the last session, or create one
  tmuxlike new [-d] [-s NAME]    create a new session and attach (-d: in background)
  tmuxlike attach [-t NAME]      attach to an existing session (alias: a)
  tmuxlike ls                    list sessions
  tmuxlike kill-session -t NAME  kill a session
  tmuxlike kill-server           kill the server and all sessions
  tmuxlike keys                  show default key bindings
  tmuxlike capture-pane -p [-t S] print the active pane of a session
  tmuxlike send-keys [-t S] KEYS  type text / keys (Enter, C-c, ...) into it
  tmuxlike source-file [PATH]    reload config in the running server
  tmuxlike <command> [args]      run any server command, e.g. `tmuxlike set mouse off`

AUTOMATION (herdr-style, JSON output; see `tmuxlike pane --help`):
  tmuxlike pane list|get|split|read|send-text|send-keys|run|wait-output|...
  tmuxlike agent list|get|read|prompt|wait|rename|focus|send-keys
  tmuxlike session list   tmuxlike api snapshot   tmuxlike server [stop]

Config: $TMUXLIKE_CONFIG or <config_dir>/tmuxlike/config.toml"#,
        r#"tmuxlike - 跨平台終端多工器

用法 USAGE:
  tmuxlike                       連線到最近的工作階段，若無則建立一個
  tmuxlike new [-d] [-s NAME]    建立新工作階段並連線（-d：在背景建立）
  tmuxlike attach [-t NAME]      連線到既有的工作階段（別名 alias：a）
  tmuxlike ls                    列出工作階段
  tmuxlike kill-session -t NAME  關閉一個工作階段
  tmuxlike kill-server           關閉伺服器與所有工作階段
  tmuxlike keys                  顯示預設按鍵
  tmuxlike capture-pane -p [-t S] 印出工作階段的使用中窗格
  tmuxlike send-keys [-t S] KEYS  將文字或按鍵（Enter、C-c 等）輸入到窗格
  tmuxlike source-file [PATH]    在執行中的伺服器重新載入設定
  tmuxlike <command> [args]      執行任何伺服器指令，例如 `tmuxlike set mouse off`

自動化 AUTOMATION（herdr 風格，JSON 輸出；見 `tmuxlike pane --help`）：
  tmuxlike pane list|get|split|read|send-text|send-keys|run|wait-output|...
  tmuxlike agent list|get|read|prompt|wait|rename|focus|send-keys
  tmuxlike session list   tmuxlike api snapshot   tmuxlike server [stop]

設定檔 Config：$TMUXLIKE_CONFIG，或 <config_dir>/tmuxlike/config.toml"#,
    ),
    (
        "cli.keys",
        r#"Prefix: Ctrl-b (configurable), then:
  c          new window           n / p      next / previous window
  0-9        select window        %  or |    split left/right
  "  or -     split top/bottom     arrows/hjkl move focus
  Ctrl/Alt+arrows, H J K L  resize pane
  o          next pane            z          zoom pane
  x          kill pane (y/n)      &          kill window (y/n)
  Tab        next pane needing you (blocked first, then done)
  BTab       previous such pane   g          tree chooser (like s / w)
  [ / PgUp   copy mode (hjkl, v/V select, y copy, q exit)
  ]          paste buffer         :          command prompt
  ,          rename window        $          rename session
  s / w      session chooser      ( )        prev/next session
  a  or b    toggle sidebar       r          reload config
  ?          list keys            d          detach
  Ctrl-b     send prefix to the program
Options (`set NAME VALUE` or config.toml):
  hints on|off            status-bar key hints and the first-attach tip (default on)
  confirm-close on|off    y/n before x and & (default on; off = kill at once)
  pane-titles off|auto|always  title row per pane (auto: split window + agent/label)
Mouse: click a pane or its title to focus, a window label to switch, the session chip for
the chooser, the counters for the next alert, a sidebar row to jump; wheel scrolls; drag copies.
Custom bindings: [bind] in config.toml, or `bind-key KEY COMMAND` at the : prompt."#,
        r#"前綴鍵 Prefix：Ctrl-b（可設定），之後按：
  c          新增視窗              n / p      下一個／上一個視窗
  0-9        選取視窗              %  或 |    左右分割
  "  或 -     上下分割              方向鍵／hjkl  移動焦點
  Ctrl/Alt+方向鍵、H J K L  調整窗格大小
  o          下一個窗格            z          放大窗格
  x          關閉窗格（y/n）       &          關閉視窗（y/n）
  Tab        下一個需要你處理的窗格（待回應優先，其次已完成）
  BTab       上一個需要你處理的窗格  g          選單（同 s / w）
  [ / PgUp   複製模式（hjkl，v/V 選取，y 複製，q 離開）
  ]          貼上緩衝區            :          指令列
  ,          重新命名視窗          $          重新命名工作階段
  s / w      工作階段選單          ( )        上一個／下一個工作階段
  a  或 b    切換側邊欄            r          重新載入設定
  ?          列出按鍵              d          離開（detach）
  Ctrl-b     把前綴鍵送給程式
選項 Options（`set NAME VALUE` 或 config.toml）：
  hints on|off            狀態列按鍵提示與首次連線提示（預設 on）
  confirm-close on|off    x 與 & 之前先以 y/n 確認（預設 on；off = 立即關閉）
  pane-titles off|auto|always  窗格標題列（auto：分割視窗且窗格有 agent 或標籤時顯示）
滑鼠 Mouse：點窗格或其標題以聚焦，點視窗標籤切換視窗，點工作階段名稱開啟選單，點提醒計數跳到下一個提醒，點側邊欄列跳轉；滾輪捲動；拖曳即複製。
自訂綁定 Custom bindings：config.toml 的 [bind]，或在 : 指令列輸入 `bind-key KEY COMMAND`。"#,
    ),
    (
        "cli.api_help",
        r#"tmuxlike pane|agent|api|session|server  (herdr-style JSON automation)

Pane ids: p<N> (tmux-style %<N> also accepted). Pane TARGET is one of:
  <id>  |  --pane ID  |  --current   (--current = $TMUXLIKE_PANE_ID inside a pane)
Omitted pane targets use the caller's pane ($TMUXLIKE_PANE_ID) when set, else the focused
pane of the most recently used session.
Output: success prints {"result": ...} on stdout, exit 0; text replies print raw.
Errors print {"error":{"code","message"}} on stderr, exit 1. Usage errors exit 2.
Use -- before text that starts with '-'. Use --opt=value to pass values starting with '--'.

api snapshot
session list [--json]
server stop
pane list [--session NAME]
pane get <id>
pane current [--pane ID | --current]
pane split [TARGET] --direction right|down [--ratio F] [--cwd PATH] [--focus | --no-focus]
pane focus <id>
pane focus --direction left|right|up|down [--pane ID | --current]
pane zoom [TARGET] [--toggle | --on | --off]
pane rename <id> <label>  |  pane rename <id> --clear
pane close <id>
pane read <id> [--source visible|recent] [--lines N]
pane send-text <id> <text>        (no Enter is added)
pane send-keys <id> <key>...      (e.g. Enter C-c Up)
pane run <id> <command>           (command + Enter)
pane wait-output <id> (--match TEXT | --regex PAT) [--source S] [--lines N] [--timeout MS]
pane report-agent <id> --source ID --agent LABEL --state idle|working|blocked|unknown
                  [--message TEXT] [--seq N] [-- RESUME_CMD...]
pane release-agent <id> --source ID --agent LABEL [--seq N]
agent list
agent get <target>
agent read <target> [--source visible|recent] [--lines N]
agent send-keys <target> <key>...
agent focus <target>
agent prompt <target> <text> [--wait [--until S]... [--timeout MS]]
agent wait <target> [--until S]... [--timeout MS]
agent rename <target> <name>  |  agent rename <target> --clear

<target> = pane id or agent name [a-z][a-z0-9_-]{0,31} (a name may not look like a pane id, e.g. p1).
--until S: idle|working|blocked|done|unknown; repeatable (default: idle, done, blocked).
--timeout MS: milliseconds; omitted = wait without limit."#,
        r#"tmuxlike pane|agent|api|session|server  （herdr 風格 JSON 自動化）

窗格 ID：p<N>（也接受 tmux 風格的 %<N>）。窗格目標 TARGET 為下列其一：
  <id>  |  --pane ID  |  --current   （--current 指窗格內的 $TMUXLIKE_PANE_ID）
省略窗格目標時，使用呼叫者的窗格（若已設定 $TMUXLIKE_PANE_ID），否則使用最近使用之工作階段的焦點窗格。
輸出：成功時在 stdout 印出 {"result": ...} 並以 0 結束；文字回覆則原樣輸出。
錯誤時在 stderr 印出 {"error":{"code","message"}} 並以 1 結束。用法錯誤以 2 結束。
文字若以 '-' 開頭，前面加 --。值若以 '--' 開頭，請用 --opt=value。

api snapshot
session list [--json]
server stop
pane list [--session NAME]
pane get <id>
pane current [--pane ID | --current]
pane split [TARGET] --direction right|down [--ratio F] [--cwd PATH] [--focus | --no-focus]
pane focus <id>
pane focus --direction left|right|up|down [--pane ID | --current]
pane zoom [TARGET] [--toggle | --on | --off]
pane rename <id> <label>  |  pane rename <id> --clear
pane close <id>
pane read <id> [--source visible|recent] [--lines N]
pane send-text <id> <text>        （不會加上 Enter）
pane send-keys <id> <key>...      （例如 Enter C-c Up）
pane run <id> <command>           （指令加上 Enter）
pane wait-output <id> (--match TEXT | --regex PAT) [--source S] [--lines N] [--timeout MS]
pane report-agent <id> --source ID --agent LABEL --state idle|working|blocked|unknown
                  [--message TEXT] [--seq N] [-- RESUME_CMD...]
pane release-agent <id> --source ID --agent LABEL [--seq N]
agent list
agent get <target>
agent read <target> [--source visible|recent] [--lines N]
agent send-keys <target> <key>...
agent focus <target>
agent prompt <target> <text> [--wait [--until S]... [--timeout MS]]
agent wait <target> [--until S]... [--timeout MS]
agent rename <target> <name>  |  agent rename <target> --clear

<target> = 窗格 ID 或代理名稱 [a-z][a-z0-9_-]{0,31}（名稱不可長得像窗格 ID，例如 p1）。
--until S：idle|working|blocked|done|unknown；可重複指定（預設：idle、done、blocked）。
--timeout MS：毫秒；省略則無限等待。"#,
    ),
    (
        "cli.err.no_server",
        "no server running ({err})",
        "沒有執行中的伺服器（{err}）",
    ),
    (
        "cli.err.start_failed",
        "failed to start server",
        "無法啟動伺服器",
    ),
    (
        "cli.err.nested",
        "sessions should be nested with care, unset $TMUXLIKE to force",
        "工作階段不宜巢狀使用，請先取消設定 $TMUXLIKE 以強制執行",
    ),
    (
        "cli.exit.detached",
        "detached (from session {session})",
        "已離開（工作階段 {session}）",
    ),
    ("cli.exit.exited", "exited", "已結束"),
    ("cli.exit.connection_lost", "connection lost", "連線中斷"),
    ("cli.exit.server_exited", "server exited", "伺服器已結束"),
    ("cli.exit.error", "error: {msg}", "錯誤：{msg}"),
];
