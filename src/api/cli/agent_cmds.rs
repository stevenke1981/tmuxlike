//! `agent` group: argv -> `Request::Agent*`. A target is a pane id or a live agent name.
use super::args::{
    agent_target, check_count, lines_opt, read_source, single, unknown_sub, wait_spec,
};
use super::scan::{PROMPT_OPTS, READ_OPTS, WAIT_OPTS, flag, no_opts, scan};
use crate::api::{Request, is_agent_name};

const AGENT_SUBS: &[&str] = &[
    "list",
    "get",
    "read",
    "send-keys",
    "focus",
    "prompt",
    "wait",
    "rename",
];

pub(super) fn dispatch(sub: &str, rest: &[String]) -> Result<Request, String> {
    match sub {
        "list" => agent_list(rest),
        "get" => agent_get(rest),
        "read" => agent_read(rest),
        "send-keys" => agent_send_keys(rest),
        "focus" => agent_focus(rest),
        "prompt" => agent_prompt(rest),
        "wait" => agent_wait(rest),
        "rename" => agent_rename(rest),
        _ => Err(unknown_sub("agent", sub, AGENT_SUBS)),
    }
}

fn agent_list(rest: &[String]) -> Result<Request, String> {
    check_count(&no_opts(rest)?, 0, 0, "agent list")?;
    Ok(Request::AgentList)
}

fn agent_get(rest: &[String]) -> Result<Request, String> {
    let words = no_opts(rest)?;
    Ok(Request::AgentGet {
        target: agent_target(single(&words, "agent get <target>")?)?,
    })
}

fn agent_read(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "agent read <target> [--source visible|recent] [--lines N]";
    let a = scan(rest, READ_OPTS)?;
    let words = a.words();
    Ok(Request::AgentRead {
        target: agent_target(single(&words, USAGE)?)?,
        source: read_source(&a)?,
        lines: lines_opt(&a)?,
    })
}

fn agent_send_keys(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "agent send-keys <target> <key>...";
    let words = no_opts(rest)?;
    check_count(&words, 2, usize::MAX, USAGE)?;
    Ok(Request::AgentSendKeys {
        target: agent_target(&words[0])?,
        keys: words[1..].to_vec(),
    })
}

fn agent_focus(rest: &[String]) -> Result<Request, String> {
    let words = no_opts(rest)?;
    Ok(Request::AgentFocus {
        target: agent_target(single(&words, "agent focus <target>")?)?,
    })
}

fn agent_prompt(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "agent prompt <target> <text> [--wait [--until S]... [--timeout MS]]";
    let a = scan(rest, PROMPT_OPTS)?;
    let words = a.words();
    check_count(&words, 2, 2, USAGE)?;
    let wait = match (a.has("wait"), a.has("until") || a.has("timeout")) {
        (true, _) => Some(wait_spec(&a)?),
        (false, true) => return Err("--until and --timeout need --wait".into()),
        (false, false) => None,
    };
    Ok(Request::AgentPrompt {
        target: agent_target(&words[0])?,
        text: words[1].clone(),
        wait,
    })
}

fn agent_wait(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "agent wait <target> [--until S]... [--timeout MS]";
    let a = scan(rest, WAIT_OPTS)?;
    let words = a.words();
    Ok(Request::AgentWait {
        target: agent_target(single(&words, USAGE)?)?,
        wait: wait_spec(&a)?,
    })
}

fn agent_rename(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "agent rename <target> <name> | agent rename <target> --clear";
    let a = scan(rest, &[flag("clear")])?;
    let words = a.words();
    if a.has("clear") {
        check_count(&words, 1, 1, USAGE)?;
        return Ok(Request::AgentRename {
            target: agent_target(&words[0])?,
            name: None,
        });
    }
    check_count(&words, 2, 2, USAGE)?;
    if !is_agent_name(&words[1]) {
        return Err(format!(
            "invalid agent name '{}' (matching [a-z][a-z0-9_-]{{0,31}}, and not shaped like a pane id such as p1)",
            words[1]
        ));
    }
    Ok(Request::AgentRename {
        target: agent_target(&words[0])?,
        name: Some(words[1].clone()),
    })
}
