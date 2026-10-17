//! Agent targets (pane id or agent name), read/focus, prompt, wait and rename.
use super::{is_err, run};
use crate::api::{ReadSource, Request, Status, Target, WaitSpec};

#[test]
fn agent_target_is_pane_id_or_name() {
    assert_eq!(
        run(&["agent", "get", "p3"]),
        Ok(Request::AgentGet {
            target: Target::Pane(3)
        })
    );
    assert_eq!(
        run(&["agent", "get", "%8"]),
        Ok(Request::AgentGet {
            target: Target::Pane(8)
        })
    );
    assert_eq!(
        run(&["agent", "get", "claude-1_b"]),
        Ok(Request::AgentGet {
            target: Target::Name("claude-1_b".into())
        })
    );
    assert_eq!(
        run(&["agent", "get", "p0"]),
        Ok(Request::AgentGet {
            target: Target::Name("p0".into())
        })
    );
    assert_eq!(
        run(&["agent", "get", "p1x"]),
        Ok(Request::AgentGet {
            target: Target::Name("p1x".into())
        })
    );
    let max = "a".repeat(32);
    assert!(run(&["agent", "get", max.as_str()]).is_ok());
    let too_long = "a".repeat(33);
    assert!(is_err(&["agent", "get", too_long.as_str()]));
    assert!(is_err(&["agent", "get", "42"]));
    assert!(is_err(&["agent", "get", "Claude"]));
    assert!(is_err(&["agent", "get", "1abc"]));
    assert!(is_err(&["agent", "get"]));
    assert!(is_err(&["agent", "get", "claude", "x"]));
}

#[test]
fn agent_read_and_focus() {
    assert_eq!(
        run(&["agent", "read", "p2", "--source", "visible"]),
        Ok(Request::AgentRead {
            target: Target::Pane(2),
            source: ReadSource::Visible,
            lines: None
        })
    );
    assert_eq!(
        run(&["agent", "focus", "reviewer"]),
        Ok(Request::AgentFocus {
            target: Target::Name("reviewer".into())
        })
    );
    assert!(is_err(&["agent", "focus", "p1", "p2"]));
}

#[test]
fn agent_prompt_wait_options() {
    assert_eq!(
        run(&["agent", "prompt", "claude", "hello world"]),
        Ok(Request::AgentPrompt {
            target: Target::Name("claude".into()),
            text: "hello world".into(),
            wait: None,
        })
    );
    assert_eq!(
        run(&["agent", "prompt", "p2", "hi", "--wait"]),
        Ok(Request::AgentPrompt {
            target: Target::Pane(2),
            text: "hi".into(),
            wait: Some(WaitSpec {
                until: vec![],
                timeout_ms: None
            }),
        })
    );
    assert_eq!(
        run(&[
            "agent",
            "prompt",
            "p2",
            "hi",
            "--wait",
            "--until",
            "idle",
            "--until=done",
            "--timeout",
            "5000"
        ]),
        Ok(Request::AgentPrompt {
            target: Target::Pane(2),
            text: "hi".into(),
            wait: Some(WaitSpec {
                until: vec![Status::Idle, Status::Done],
                timeout_ms: Some(5000),
            }),
        })
    );
    assert_eq!(
        run(&["agent", "prompt", "claude", "--", "--help me"]),
        Ok(Request::AgentPrompt {
            target: Target::Name("claude".into()),
            text: "--help me".into(),
            wait: None,
        })
    );
    assert!(is_err(&[
        "agent", "prompt", "claude", "hi", "--until", "idle"
    ]));
    assert!(is_err(&[
        "agent",
        "prompt",
        "claude",
        "hi",
        "--timeout",
        "10"
    ]));
    assert!(is_err(&[
        "agent", "prompt", "claude", "hi", "--wait", "--until", "sleeping"
    ]));
    assert!(is_err(&[
        "agent",
        "prompt",
        "claude",
        "hi",
        "--wait",
        "--timeout",
        "-5"
    ]));
}

#[test]
fn agent_wait_defaults_to_empty_until() {
    assert_eq!(
        run(&["agent", "wait", "claude"]),
        Ok(Request::AgentWait {
            target: Target::Name("claude".into()),
            wait: WaitSpec {
                until: vec![],
                timeout_ms: None
            },
        })
    );
    assert_eq!(
        run(&[
            "agent",
            "wait",
            "p7",
            "--until",
            "working",
            "--timeout",
            "250"
        ]),
        Ok(Request::AgentWait {
            target: Target::Pane(7),
            wait: WaitSpec {
                until: vec![Status::Working],
                timeout_ms: Some(250)
            },
        })
    );
    assert!(is_err(&["agent", "wait"]));
    assert!(is_err(&["agent", "wait", "claude", "--until", "nope"]));
}

#[test]
fn agent_rename_sets_or_clears_name() {
    assert_eq!(
        run(&["agent", "rename", "p3", "reviewer"]),
        Ok(Request::AgentRename {
            target: Target::Pane(3),
            name: Some("reviewer".into())
        })
    );
    assert_eq!(
        run(&["agent", "rename", "reviewer", "--clear"]),
        Ok(Request::AgentRename {
            target: Target::Name("reviewer".into()),
            name: None
        })
    );
    assert!(is_err(&["agent", "rename", "p3"]));
    assert!(is_err(&["agent", "rename", "p3", "Bad Name"]));
    assert!(is_err(&["agent", "rename", "p3", "x", "--clear"]));
}

#[test]
fn agent_rename_rejects_pane_shaped_and_overlong_names() {
    // a name shaped like a pane id would shadow the pane target, so it is refused
    assert!(is_err(&["agent", "rename", "p3", "p1"]));
    assert!(is_err(&["agent", "rename", "p3", "p12"]));
    assert!(is_err(&["agent", "rename", "reviewer", "p7"]));
    // p0 is not a pane id (0 is rejected), so it is a valid name
    assert!(run(&["agent", "rename", "p3", "p0"]).is_ok());
    assert!(run(&["agent", "rename", "p3", "pa"]).is_ok());
    assert!(run(&["agent", "rename", "p3", "p1x"]).is_ok());
    let max = "a".repeat(32);
    assert!(run(&["agent", "rename", "p3", max.as_str()]).is_ok());
    let too_long = "a".repeat(33);
    assert!(is_err(&["agent", "rename", "p3", too_long.as_str()]));
}
