//! Pane and agent I/O (send-text, send-keys, run, wait-output) and agent reports.
use super::{is_err, run};
use crate::api::{Pattern, ReadSource, Request, Status, Target};

/// `pane report-agent p4 --source s --agent a` plus caller-supplied extras.
fn report(extra: &[&str]) -> Result<Request, String> {
    let mut xs = vec![
        "pane",
        "report-agent",
        "p4",
        "--source",
        "s",
        "--agent",
        "a",
    ];
    xs.extend_from_slice(extra);
    run(&xs)
}

#[test]
fn send_text_and_run_keep_text_byte_for_byte() {
    let text = "  say \"hi\" \\n\\t  \nline2\r\n 你好 世界  ";
    assert_eq!(
        run(&["pane", "send-text", "p1", text]),
        Ok(Request::PaneSendText {
            pane: 1,
            text: text.into()
        })
    );
    assert_eq!(
        run(&["pane", "send-text", "p1", "--", "-rf"]),
        Ok(Request::PaneSendText {
            pane: 1,
            text: "-rf".into()
        })
    );
    assert_eq!(
        run(&["pane", "send-text", "p1", "--", "--text=x"]),
        Ok(Request::PaneSendText {
            pane: 1,
            text: "--text=x".into()
        })
    );
    assert_eq!(
        run(&["pane", "run", "p2", "--", "echo \"a\" 'b' \\ c"]),
        Ok(Request::PaneRun {
            pane: 2,
            command: "echo \"a\" 'b' \\ c".into()
        })
    );
    assert!(is_err(&["pane", "send-text", "p1"]));
    assert!(is_err(&["pane", "send-text", "p1", "a", "b"]));
    assert!(is_err(&["pane", "send-text", "p1", "-rf"]));
    assert!(is_err(&["pane", "run", "p1"]));
}

#[test]
fn send_keys_needs_at_least_one_key() {
    assert_eq!(
        run(&["pane", "send-keys", "p1", "Enter", "C-c"]),
        Ok(Request::PaneSendKeys {
            pane: 1,
            keys: vec!["Enter".into(), "C-c".into()]
        })
    );
    assert_eq!(
        run(&["agent", "send-keys", "claude", "Up"]),
        Ok(Request::AgentSendKeys {
            target: Target::Name("claude".into()),
            keys: vec!["Up".into()],
        })
    );
    assert_eq!(
        run(&["agent", "send-keys", "p3", "-", "Esc"]),
        Ok(Request::AgentSendKeys {
            target: Target::Pane(3),
            keys: vec!["-".into(), "Esc".into()],
        })
    );
    assert!(is_err(&["pane", "send-keys", "p1"]));
    assert!(is_err(&["agent", "send-keys", "claude"]));
}

#[test]
fn wait_output_needs_exactly_one_pattern() {
    assert_eq!(
        run(&["pane", "wait-output", "p1", "--match", "$ "]),
        Ok(Request::PaneWaitOutput {
            pane: 1,
            pattern: Pattern::Match("$ ".into()),
            source: ReadSource::Recent,
            lines: None,
            timeout_ms: None,
        })
    );
    assert_eq!(
        run(&[
            "pane",
            "wait-output",
            "p1",
            "--regex",
            "^ok\\d+$",
            "--source",
            "visible",
            "--lines",
            "20",
            "--timeout",
            "1500"
        ]),
        Ok(Request::PaneWaitOutput {
            pane: 1,
            pattern: Pattern::Regex("^ok\\d+$".into()),
            source: ReadSource::Visible,
            lines: Some(20),
            timeout_ms: Some(1500),
        })
    );
    assert!(is_err(&["pane", "wait-output", "p1"]));
    assert!(is_err(&[
        "pane",
        "wait-output",
        "p1",
        "--match",
        "a",
        "--regex",
        "b"
    ]));
    assert!(is_err(&["pane", "wait-output", "p1", "--regex", "("]));
    assert!(is_err(&[
        "pane",
        "wait-output",
        "p1",
        "--match",
        "a",
        "--timeout",
        "soon"
    ]));
}

#[test]
fn report_agent_parses_message_seq_and_resume() {
    assert_eq!(
        report(&["--state", "working"]),
        Ok(Request::PaneReportAgent {
            pane: 4,
            source: "s".into(),
            agent: "a".into(),
            state: Status::Working,
            message: None,
            seq: None,
            resume: vec![],
        })
    );
    let text = "  Waiting: ok -- sure\n";
    assert_eq!(
        report(&[
            "--state",
            "blocked",
            "--message",
            text,
            "--seq",
            "7",
            "--",
            "claude",
            "--resume",
            "--last"
        ]),
        Ok(Request::PaneReportAgent {
            pane: 4,
            source: "s".into(),
            agent: "a".into(),
            state: Status::Blocked,
            message: Some(text.into()),
            seq: Some(7),
            resume: vec!["claude".into(), "--resume".into(), "--last".into()],
        })
    );
    assert!(report(&["--state", "idle", "--message=--x"]).is_ok());
}

#[test]
fn report_agent_rejects_bad_fields() {
    assert!(report(&["--state", "idle", "--"]).is_err());
    assert!(report(&["--state", "done"]).is_err());
    assert!(report(&["--state", "sleeping"]).is_err());
    assert!(report(&["--seq", "x", "--state", "idle"]).is_err());
    assert!(is_err(&[
        "pane",
        "report-agent",
        "p4",
        "--source",
        "bad source",
        "--agent",
        "a",
        "--state",
        "idle"
    ]));
    assert!(is_err(&[
        "pane",
        "report-agent",
        "p4",
        "--source",
        "",
        "--agent",
        "a",
        "--state",
        "idle"
    ]));
    assert!(is_err(&[
        "pane",
        "report-agent",
        "p4",
        "--agent",
        "a",
        "--state",
        "idle"
    ]));
    assert!(is_err(&[
        "pane",
        "report-agent",
        "p4",
        "--source",
        "s",
        "--agent",
        "",
        "--state",
        "idle"
    ]));
    let long = "x".repeat(81);
    assert!(is_err(&[
        "pane",
        "report-agent",
        "p4",
        "--source",
        "s",
        "--agent",
        long.as_str(),
        "--state",
        "idle"
    ]));
    let edge = "x".repeat(80);
    assert!(
        run(&[
            "pane",
            "report-agent",
            "p4",
            "--source",
            "s",
            "--agent",
            edge.as_str(),
            "--state",
            "idle"
        ])
        .is_ok()
    );
}

#[test]
fn release_agent_parses() {
    assert_eq!(
        run(&[
            "pane",
            "release-agent",
            "p4",
            "--source",
            "hook:claude",
            "--agent",
            "claude"
        ]),
        Ok(Request::PaneReleaseAgent {
            pane: 4,
            source: "hook:claude".into(),
            agent: "claude".into(),
            seq: None,
        })
    );
    assert_eq!(
        run(&[
            "pane",
            "release-agent",
            "p4",
            "--source",
            "s",
            "--agent",
            "a",
            "--seq=9"
        ]),
        Ok(Request::PaneReleaseAgent {
            pane: 4,
            source: "s".into(),
            agent: "a".into(),
            seq: Some(9)
        })
    );
    assert!(is_err(&["pane", "release-agent", "p4", "--source", "s"]));
    assert!(is_err(&[
        "pane",
        "release-agent",
        "p4",
        "--source",
        "s",
        "--agent",
        "a",
        "--",
        "x"
    ]));
}
