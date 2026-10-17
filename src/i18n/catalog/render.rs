//! render strings: (key, en, zh-TW). Keys start with "render.".
//!
//! Placeholders are filled by `trf` in argument order. Dynamic user values (session and window
//! names) are always passed last, so an inserted name cannot be read as a placeholder.
use super::super::Entry;

pub const ENTRIES: &[Entry] = &[
    // status bar
    ("render.status.prefix_chip", " PREFIX ", " 前綴鍵 "),
    ("render.status.copy_chip", " COPY ", " 複製模式 "),
    (
        "render.status.copy_hint",
        "hjkl move  v/V select  y copy  q quit",
        "hjkl 移動  v/V 選取  y 複製  q 結束",
    ),
    ("render.status.help", "? help", "? 說明"),
    ("render.which.new", "new", "新增"),
    ("render.which.win", "win", "視窗"),
    ("render.which.split", "split", "分割"),
    ("render.which.move", "move", "移動"),
    ("render.which.zoom", "zoom", "放大"),
    ("render.which.kill", "kill", "關閉"),
    ("render.which.alert", "alert", "提醒"),
    ("render.which.tree", "tree", "選單"),
    ("render.which.copy", "copy", "複製"),
    ("render.which.detach", "detach", "離開"),
    (
        "render.which.end",
        " ? all  Esc cancel",
        " ? 全部  Esc 取消",
    ),
    // agent state words (pane titles, chooser rows, filter titles)
    ("render.state.working", "working", "執行中"),
    ("render.state.blocked", "blocked", "待回應"),
    ("render.state.done", "done", "已完成"),
    ("render.state.idle", "idle", "閒置"),
    ("render.state.unknown", "unknown", "未知"),
    // short sidebar legend words (the legend must fit a 25-column sidebar)
    ("render.legend.working", "work", "執行"),
    ("render.legend.blocked", "block", "待回應"),
    ("render.legend.done", "done", "完成"),
    ("render.legend.idle", "idle", "閒置"),
    // sidebar
    ("render.sidebar.title", "agents", "代理"),
    (
        "render.sidebar.hint",
        "{prefix} a hide  {prefix} s switch",
        "{prefix} a 隱藏  {prefix} s 切換",
    ),
    (
        "render.sidebar.empty",
        "no agent reports {dash} see: tmuxlike pane report-agent --help",
        "尚無代理回報 {dash} 請見：tmuxlike pane report-agent --help",
    ),
    ("render.sidebar.more", "{ell} +{n} more", "{ell} +{n} 更多"),
    // chooser (overlay)
    ("render.chooser.title", "Sessions", "工作階段"),
    (
        "render.chooser.title_filter",
        "Sessions [{state}]",
        "工作階段 [{state}]",
    ),
    (
        "render.chooser.hints",
        "↑↓ move  ⏎ open  x kill  b/w/d/i filter  esc close",
        "↑↓ 移動  ⏎ 開啟  x 關閉  b/w/d/i 篩選  esc 關閉",
    ),
    (
        "render.chooser.hints_ascii",
        "up/dn move  enter open  x kill  b/w/d/i filter  esc close",
        "up/dn 移動  enter 開啟  x 關閉  b/w/d/i 篩選  esc 關閉",
    ),
    ("render.chooser.empty_all", "no sessions", "沒有工作階段"),
    (
        "render.chooser.empty_filter",
        "no {state} panes",
        "沒有{state}的窗格",
    ),
    ("render.chooser.attached", " (attached)", "（已連線）"),
    (
        "render.session.text",
        "{name}: {count}{attached}",
        "{name}：{count}{attached}",
    ),
    (
        "render.window.text",
        "{index}: {name}{star}  {count}",
        "{index}：{name}{star}  {count}",
    ),
    ("render.count.window", "{n} window", "{n} 個視窗"),
    ("render.count.windows", "{n} windows", "{n} 個視窗"),
    ("render.count.pane", "{n} pane", "{n} 個窗格"),
    ("render.count.panes", "{n} panes", "{n} 個窗格"),
    (
        "render.kill.session",
        "kill session {name} ({windows}, {blocked} blocked)?",
        "關閉工作階段 {name}（{windows}，待回應 {blocked}）？",
    ),
    (
        "render.kill.window",
        "kill window {index} {name} ({panes}, {blocked} blocked)?",
        "關閉視窗 {index} {name}（{panes}，待回應 {blocked}）？",
    ),
    ("render.popup.title", "keys", "按鍵"),
    (
        "render.popup.hint",
        "j/k scroll  esc close",
        "j/k 捲動  esc 關閉",
    ),
    // pane titles and copy mode
    ("render.pane.untitled", "pane", "窗格"),
    (
        "render.copy.label",
        "[copy {pos}/{max}{sel}]",
        "[複製 {pos}/{max}{sel}]",
    ),
    ("render.copy.sel", " sel", " 選取"),
    // scroll markers on the chooser and key popup top border
    ("render.scroll.up", "▲ more", "▲ 更多"),
    ("render.scroll.down", "▼ more", "▼ 更多"),
    ("render.scroll.up_ascii", "^ more", "^ 更多"),
    ("render.scroll.down_ascii", "v more", "v 更多"),
];
