//! Parser tests: shared helpers plus the general tests (help, groups, option-free commands,
//! pane list and ids, option rules). Command groups live in the `tests/` submodules.
mod agent;
mod pane_io;
mod pane_ops;

use super::{HELP, parse};
use crate::api::Request;

fn v(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

fn run(xs: &[&str]) -> Result<Request, String> {
    parse(&v(xs), None)
}

fn run_at(xs: &[&str], current: Option<usize>) -> Result<Request, String> {
    parse(&v(xs), current)
}

fn is_err(xs: &[&str]) -> bool {
    run(xs).is_err()
}

#[test]
fn help_and_missing_subcommand_return_help_text() {
    assert_eq!(run(&["pane"]), Err(HELP.to_string()));
    assert_eq!(run(&["agent", "--help"]), Err(HELP.to_string()));
    assert_eq!(run(&["server", "-h"]), Err(HELP.to_string()));
    assert_eq!(run(&["session", "help"]), Err(HELP.to_string()));
    assert_eq!(run(&["pane", "list", "--help"]), Err(HELP.to_string()));
    assert_eq!(
        run_at(&["pane", "zoom", "--help"], Some(2)),
        Err(HELP.to_string())
    );
}

#[test]
fn unknown_group_and_subcommand_are_usage_errors() {
    assert!(run(&["nope", "list"]).unwrap_err().contains("group"));
    let e = run(&["pane", "frob"]).unwrap_err();
    assert!(e.contains("frob") && e.contains("report-agent"), "{e}");
    assert!(run(&["agent", "frob"]).unwrap_err().contains("prompt"));
    assert!(is_err(&["api", "stop"]));
    assert!(is_err(&["server", "start"]));
}

#[test]
fn option_free_commands_parse() {
    assert_eq!(run(&["api", "snapshot"]), Ok(Request::ApiSnapshot));
    assert_eq!(run(&["session", "list"]), Ok(Request::SessionList));
    assert_eq!(
        run(&["session", "list", "--json"]),
        Ok(Request::SessionList)
    );
    assert_eq!(run(&["server", "stop"]), Ok(Request::ServerStop));
    assert_eq!(run(&["agent", "list"]), Ok(Request::AgentList));
    assert!(is_err(&["server", "stop", "now"]));
    assert!(is_err(&["api", "snapshot", "--json"]));
    assert!(is_err(&["agent", "list", "x"]));
}

#[test]
fn pane_list_session_option() {
    assert_eq!(
        run(&["pane", "list"]),
        Ok(Request::PaneList { session: None })
    );
    assert_eq!(
        run(&["pane", "list", "--session", "work"]),
        Ok(Request::PaneList {
            session: Some("work".into())
        })
    );
    assert_eq!(
        run(&["pane", "list", "--session=a=b=c"]),
        Ok(Request::PaneList {
            session: Some("a=b=c".into())
        })
    );
    assert!(is_err(&["pane", "list", "--session="]));
    assert!(is_err(&["pane", "list", "extra"]));
}

#[test]
fn single_id_commands_accept_p_and_percent_forms() {
    assert_eq!(
        run(&["pane", "get", "p12"]),
        Ok(Request::PaneGet { pane: 12 })
    );
    assert_eq!(
        run(&["pane", "get", "%7"]),
        Ok(Request::PaneGet { pane: 7 })
    );
    assert_eq!(
        run(&["pane", "close", "p3"]),
        Ok(Request::PaneClose { pane: 3 })
    );
    assert_eq!(
        run(&["pane", "focus", "p4"]),
        Ok(Request::PaneFocus { pane: 4 })
    );
}

#[test]
fn bad_pane_ids_and_extra_words_are_rejected() {
    for bad in ["p0", "w1:p1", "42", "p01", "p"] {
        assert!(is_err(&["pane", "get", bad]), "{bad}");
    }
    assert!(is_err(&["pane", "get"]));
    assert!(is_err(&["pane", "get", "p1", "p2"]));
    assert!(is_err(&["pane", "get", "p1", "--", "x"]));
    assert!(is_err(&["pane", "close", "p1", "--force"]));
}

#[test]
fn unknown_and_duplicate_options_are_rejected() {
    assert!(is_err(&["pane", "list", "--bogus"]));
    assert!(is_err(&["pane", "list", "-s"]));
    assert!(is_err(&["pane", "list", "--session"]));
    assert!(is_err(&["pane", "list", "--session", "--other"]));
    assert!(is_err(&[
        "pane",
        "list",
        "--session",
        "a",
        "--session",
        "b"
    ]));
    assert!(is_err(&["pane", "get", "p1", "--json"]));
    assert!(is_err(&["session", "list", "--json=1"]));
    assert!(run(&["agent", "wait", "p1", "--until", "idle", "--until", "done"]).is_ok());
}
