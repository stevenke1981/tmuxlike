#!/usr/bin/env python3
"""Automation API smoke test: drives a real tmuxlike server through its JSON CLI.

Each step prints "ok <step>". Covers session/pane listing, split with and without
focus, wait-output, text input with special characters, send-keys validation,
agent report/wait/prompt/rename/release, error exit codes, zoom/rename/close and
layout persistence across `server stop` and a fresh start. Regression steps cover
pane-id-shaped agent names, `--help` exit 0, wrapped wait-output, and the no-server
JSON error plus clearing the saved layout when the last session closes.
Works on Windows (ConPTY) and Linux/macOS (PTY); stdlib only.
Usage: python scripts/api_smoke.py [path-to-tmuxlike]
"""
import collections
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import traceback

WIN = os.name == "nt"
EXE = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join("target", "release", "tmuxlike" + (".exe" if WIN else "")))
PROMPT_RE = r"PS [^\r\n]*>" if WIN else r"\S"
PANE_ID_RE = re.compile(r"^p\d+$")
SPECIAL = 'x\\y 中文 "q"'  # backslash, CJK and quotes; typed without Enter
SPECIAL_CHECK = "x\\y 中文"
AGENT_SOURCE = "api-smoke"
AGENT_KIND = "fake-agent"
CALL_TIMEOUT = 60  # seconds per CLI call; ConPTY is slow
POLL_TIMEOUT = 60  # seconds per poll()
WRAP_FILL = "A" * 110  # makes START + WRAP_FILL + END wider than a narrow pane

WORK = tempfile.mkdtemp(prefix="tmuxlike-api-")
LOG = os.path.join(WORK, "server.log")
STATE = os.path.join(WORK, "state.json")
CWD_DIR = os.path.join(WORK, "cwd-test")
ENV = dict(os.environ, TMUXLIKE_SOCKET="tmuxlike-api-%d.sock" % os.getpid(), TMUXLIKE_LOG=LOG, TMUXLIKE_STATE=STATE)
ENV.pop("TMUXLIKE", None)
ENV.pop("TMUXLIKE_PANE_ID", None)


class Fail(Exception):
    """An assertion failed; the message is printed after "FAIL: "."""


def fail(message):
    raise Fail(message)


def ok(step):
    print("ok " + step, flush=True)


def run(args, timeout=CALL_TIMEOUT):
    """Run one tmuxlike command. Returns CompletedProcess; only a hang raises Fail."""
    try:
        return subprocess.run([EXE, *args], env=ENV, capture_output=True, text=True,
                              encoding="utf-8", errors="replace", timeout=timeout)
    except subprocess.TimeoutExpired:
        fail(f"tmuxlike {' '.join(args)} did not return within {timeout}s")


def describe(r):
    return f"exit {r.returncode}\n--- stdout ---\n{r.stdout}\n--- stderr ---\n{r.stderr}"


def expect_ok(args, what, timeout=CALL_TIMEOUT):
    r = run(args, timeout)
    if r.returncode != 0:
        fail(f"{what}: {describe(r)}")
    return r


def expect_json(args, what, timeout=CALL_TIMEOUT):
    """Successful call with JSON stdout; returns the value under "result"."""
    r = expect_ok(args, what, timeout)
    try:
        data = json.loads(r.stdout)
    except json.JSONDecodeError as exc:
        fail(f"{what}: stdout is not JSON ({exc}): {r.stdout[:500]!r}")
    if not isinstance(data, dict) or "result" not in data:
        fail(f"{what}: JSON has no 'result' key: {r.stdout[:500]!r}")
    return data["result"]


def expect_error(args, what, code, timeout=CALL_TIMEOUT):
    """Call that must exit 1 with {"error": {"code": code, ...}} on stderr."""
    r = run(args, timeout)
    if r.returncode != 1:
        fail(f"{what}: expected exit 1, {describe(r)}")
    try:
        err = json.loads(r.stderr)["error"]
    except (json.JSONDecodeError, KeyError, TypeError) as exc:
        fail(f"{what}: stderr is not an error JSON ({exc}): {r.stderr[:500]!r}")
    if err.get("code") != code:
        fail(f"{what}: expected error.code {code!r}, got {err!r}")
    return err


def poll(get, done, what, timeout=POLL_TIMEOUT, interval=0.3):
    """Call get() until done(value) is true; fail with the last value on timeout."""
    end = time.monotonic() + timeout
    while True:
        value = get()
        if done(value):
            return value
        if time.monotonic() >= end:
            fail(f"timed out waiting for {what}; last value: {repr(value)[-1500:]}")
        time.sleep(interval)


def walk(node):
    """Yield every node of a JSON document, depth first."""
    yield node
    if isinstance(node, dict):
        children = list(node.values())
    elif isinstance(node, list):
        children = node
    else:
        children = []
    for child in children:
        yield from walk(child)


def same_path(a, b):
    if not isinstance(a, str) or not isinstance(b, str):
        return False
    return os.path.normcase(os.path.realpath(a)) == os.path.normcase(os.path.realpath(b))


def check_pane_id(pid, what):
    if not isinstance(pid, str) or not PANE_ID_RE.match(pid):
        fail(f"{what}: pane id {pid!r} does not match ^p\\d+$")
    return pid


def focused_id(panes):
    focused = [p["pane_id"] for p in panes if p.get("focused") is True]
    return focused[0] if len(focused) == 1 else None


def require_matched(result, what):
    if result.get("matched") is not True:
        fail(f"wait-output did not match {what}: {result!r}")


def pane_list(session=None):
    args = ["pane", "list"] + (["--session", session] if session else [])
    return expect_json(args, "pane list")["panes"]


def pane_get(pid):
    return expect_json(["pane", "get", pid], f"pane get {pid}")["pane"]


def pane_read(pid, source="visible"):
    return expect_ok(["pane", "read", pid, "--source", source], f"pane read {pid}").stdout


def error_markers(text):
    return text.count("not recognized") + text.count("not found")


def report(pid, state, seq):
    """Reports agent state for pane `pid`; returns the server's `applied` flag."""
    args = ["pane", "report-agent", pid, "--source", AGENT_SOURCE, "--agent", AGENT_KIND,
            "--state", state, "--seq", str(seq)]
    return expect_json(args, f"report-agent {state} seq {seq}").get("applied")


def agent_entry(pid):
    agents = expect_json(["agent", "list"], "agent list")["agents"]
    for entry in agents:
        if entry.get("pane_id") == pid:
            return entry
    return None


def step_create_session():
    expect_ok(["new", "-d", "-s", "api"], "new -d -s api")
    panes = poll(lambda: pane_list("api"), lambda ps: len(ps) == 1, "one pane in session api")
    pid = check_pane_id(panes[0].get("pane_id"), "new session")
    if panes[0].get("focused") is not True:
        fail(f"first pane is not focused: {panes[0]!r}")
    ok("new-session-pane-list")
    return pid


def step_split(p1):
    right = expect_json(["pane", "split", p1, "--direction", "right", "--no-focus"], "split right")["pane"]
    p2 = check_pane_id(right.get("pane_id"), "split right")
    if p2 == p1:
        fail("split returned the id of its target pane")
    if focused_id(pane_list("api")) != p1:
        fail("--no-focus moved the focus away from the original pane")
    ok("split-no-focus")
    down = expect_json(["pane", "split", p2, "--direction", "down", "--focus"], "split down --focus")["pane"]
    p3 = check_pane_id(down.get("pane_id"), "split down")
    poll(lambda: focused_id(pane_list("api")), lambda f: f == p3, "focus on the new pane")
    ok("split-focus-moves")
    return p2, p3


def step_commands(p1):
    res = expect_json(["pane", "wait-output", p1, "--regex", PROMPT_RE, "--timeout", "60000"],
                      "wait-output shell prompt", timeout=90)
    require_matched(res, "shell prompt")
    ok("wait-output-prompt")

    pid = os.getpid()
    marker = f"api-marker-{pid}"
    # The typed text contains `""` between the parts, so only the command output holds the marker.
    expect_ok(["pane", "run", p1, f'echo api-mark""er-{pid}'], "pane run echo")
    res = expect_json(["pane", "wait-output", p1, "--match", marker, "--timeout", "60000"],
                      "wait-output marker", timeout=90)
    require_matched(res, marker)
    if marker not in pane_read(p1):
        fail(f"pane read --source visible does not show {marker}")
    ok("run-wait-output")

    expect_error(["pane", "wait-output", p1, "--match", "never-appears-zz", "--timeout", "500"],
                 "wait-output timeout", "timeout")
    ok("wait-output-timeout")

    expect_ok(["pane", "send-text", p1, SPECIAL], "send-text special characters")
    poll(lambda: pane_read(p1), lambda t: SPECIAL_CHECK in t, "special characters on screen")
    ok("send-text-special-chars")


def step_keys(p1):
    expect_ok(["pane", "send-keys", p1, "ctrl+c"], "send-keys ctrl+c (clears typed line)")
    expect_ok(["pane", "send-text", p1, "probe-line"], "send-text probe")
    before = poll(lambda: pane_read(p1), lambda t: "probe-line" in t, "probe-line on screen")
    expect_error(["pane", "send-keys", p1, "enter", "no-such-key"], "send-keys invalid key",
                 "invalid_argument")
    after = pane_read(p1)
    if error_markers(after) != error_markers(before):
        fail("an invalid key sequence still ran the typed line (new shell error appeared)")
    if "probe-line" not in after:
        fail("typed probe disappeared after a rejected send-keys")
    expect_ok(["pane", "send-keys", p1, "ctrl+c"], "send-keys ctrl+c cleanup")
    ok("send-keys-ctrl-c-and-invalid")


def step_agent_report(p1):
    if report(p1, "working", 1) is not True:
        fail("report working seq 1 was not applied")
    entry = agent_entry(p1)
    if entry is None or entry.get("status") != "working":
        fail(f"agent list does not show {p1} as working: {entry!r}")
    ok("agent-report-working")
    expect_error(["agent", "wait", p1, "--until", "idle", "--timeout", "300"],
                 "agent wait timeout", "timeout")
    ok("agent-wait-timeout")


def step_agent_wait_idle(p1):
    thread_errors = []

    def report_idle_after_delay():
        time.sleep(1.0)
        try:
            if report(p1, "idle", 2) is not True:
                thread_errors.append("idle report seq 2 was not applied")
        except Fail as exc:
            thread_errors.append(str(exc))

    worker = threading.Thread(target=report_idle_after_delay, daemon=True)
    worker.start()
    result = expect_json(["agent", "wait", p1, "--until", "idle", "--timeout", "10000"],
                         "agent wait until idle", timeout=40)
    worker.join(15)
    if thread_errors:
        fail("background idle report failed: " + "; ".join(thread_errors))
    if result.get("status") not in ("idle", "done"):
        fail(f"agent wait returned status {result.get('status')!r}, expected idle or done")
    ok("agent-wait-until-idle")

    if report(p1, "working", 1) is not False:
        fail("stale --seq 1 report was applied after seq 2")
    entry = agent_entry(p1)
    if entry is None or entry.get("status") not in ("idle", "done"):
        fail(f"stale report changed the agent status: {entry!r}")
    ok("stale-seq-ignored")


def step_agent_prompt_and_identity(p1):
    if report(p1, "blocked", 3) is not True:
        fail("report blocked seq 3 was not applied")
    expect_error(["agent", "prompt", p1, "hello from api smoke"], "agent prompt while blocked",
                 "agent_blocked")
    ok("agent-prompt-blocked")
    if report(p1, "idle", 4) is not True:
        fail("report idle seq 4 was not applied")

    expect_ok(["agent", "rename", p1, "apiworker"], "agent rename")
    got = expect_json(["agent", "get", "apiworker"], "agent get by name")
    if not any(n == p1 for n in walk(got)):
        fail(f"agent get apiworker does not resolve to {p1}: {got!r}")
    ok("agent-rename-get")

    expect_ok(["pane", "release-agent", p1, "--source", AGENT_SOURCE, "--agent", AGENT_KIND],
              "pane release-agent")
    if pane_get(p1).get("agent") is not None:
        fail("pane still carries an agent after release-agent")
    ok("release-agent")


def step_agents(p1):
    step_agent_report(p1)
    step_agent_wait_idle(p1)
    step_agent_prompt_and_identity(p1)


def step_errors(p1):
    expect_error(["pane", "get", "p999999"], "pane get missing pane", "pane_not_found")
    ok("error-pane-not-found")
    r = run(["pane", "split", p1, "--direction", "left"])
    if r.returncode != 2:
        fail(f"invalid --direction should exit 2, {describe(r)}")
    ok("usage-error-exit-2")


def step_pane_ops(p2, p3):
    expect_ok(["pane", "zoom", p3, "--on"], "pane zoom --on")
    poll(lambda: pane_get(p3).get("zoomed"), lambda z: z is True, "zoomed true")
    expect_ok(["pane", "zoom", p3, "--off"], "pane zoom --off")
    poll(lambda: pane_get(p3).get("zoomed"), lambda z: z is False, "zoomed false")
    ok("zoom-on-off")

    expect_ok(["pane", "rename", p3, "mylabel"], "pane rename")
    if pane_get(p3).get("label") != "mylabel":
        fail("label 'mylabel' is not shown by pane get")
    ok("rename-label")

    closed = expect_json(["pane", "close", p2], "pane close")
    if closed.get("closed") != p2:
        fail(f"pane close did not report closed: {closed!r}")
    if p2 in [p["pane_id"] for p in pane_list("api")]:
        fail(f"{p2} still listed after pane close")
    ok("close-pane")


def layout(session):
    counts = collections.Counter(p["window"] for p in pane_list(session))
    return dict(sorted(counts.items()))


def cwd_pane_exists(session):
    return any(same_path(p.get("cwd"), CWD_DIR) for p in pane_list(session))


def snapshot_has_session(snap, name):
    return any(isinstance(n, dict) and n.get("name") == name for n in walk(snap))


def step_persistence(p1):
    os.makedirs(CWD_DIR, exist_ok=True)
    expect_json(["pane", "split", p1, "--direction", "down", "--cwd", CWD_DIR, "--no-focus"],
                "split with --cwd")
    poll(lambda: cwd_pane_exists("api"), lambda found: found, "pane created with --cwd")
    before = layout("api")

    expect_ok(["server", "stop"], "server stop")
    poll(lambda: os.path.exists(STATE), lambda exists: exists, "state file written by server stop")
    time.sleep(1.0)  # give the old server process time to exit
    expect_ok(["new", "-d", "-s", "other"], "new -d -s other (fresh server restores api)")

    poll(lambda: layout("api"), lambda lay: lay == before, "restored pane counts per window")
    poll(lambda: cwd_pane_exists("api"), lambda found: found, "restored pane with its cwd")
    if not pane_list("other"):
        fail("session other is missing after the restart")
    ok("persist-restore-layout")

    snap = expect_json(["api", "snapshot"], "api snapshot")
    if not snapshot_has_session(snap, "api"):
        fail("api snapshot has no session named api")
    if not any(same_path(n, CWD_DIR) for n in walk(snap)):
        fail("api snapshot does not contain the --cwd pane's cwd")
    ok("snapshot-has-session")

    expect_ok(["kill-server"], "kill-server")
    poll(lambda: os.path.exists(STATE), lambda exists: not exists, "state file removed by kill-server")
    time.sleep(1.0)
    if os.path.exists(STATE):
        fail("state file reappeared after kill-server")
    ok("kill-server-removes-state")


def step_agent_rename_rejects_pane_id(p1):
    if report(p1, "idle", 10) is not True:
        fail("report idle seq 10 was not applied before the pane-id name check")
    # rejected either by the CLI (usage error, exit 2) or by the server (invalid_argument)
    r = run(["agent", "rename", p1, "p1"])
    if r.returncode == 2:
        pass
    elif r.returncode == 1 and "invalid_argument" in r.stderr:
        pass
    else:
        fail(f"agent rename to a pane-id-shaped name was not rejected, {describe(r)}")
    ok("agent-rename-rejects-pane-id")
    expect_ok(["pane", "release-agent", p1, "--source", AGENT_SOURCE, "--agent", AGENT_KIND],
              "release-agent after pane-id name check")


def step_help():
    r = run(["pane", "--help"])
    if r.returncode != 0:
        fail(f"pane --help should exit 0, {describe(r)}")
    if "pane list" not in r.stdout:
        fail(f"pane --help does not print the help text on stdout: {r.stdout[:500]!r}")
    ok("help-exit-0")


def step_wait_output_wrapped(p1):
    narrow = expect_json(["pane", "split", p1, "--direction", "right", "--ratio", "0.4", "--no-focus"],
                         "split narrow pane")["pane"]
    pid = check_pane_id(narrow.get("pane_id"), "split narrow pane")
    # A 30-column pane can break the prompt across rows, so join the visible rows before matching.
    poll(lambda: pane_read(pid).replace("\n", ""), lambda t: re.search(PROMPT_RE, t) is not None,
         "shell prompt in narrow pane", timeout=60)
    # sh and PowerShell both drop the empty "" pieces, so the typed command never shows the joined marker.
    expect_ok(["pane", "run", pid, 'echo STAR""T' + WRAP_FILL + 'EN""D'], "pane run wrapped echo")
    res = expect_json(["pane", "wait-output", pid, "--regex", "START[A]+END", "--timeout", "60000"],
                      "wait-output wrapped marker", timeout=90)
    require_matched(res, "wrapped START...END marker")
    line = res.get("line")
    if not isinstance(line, str) or WRAP_FILL not in line:
        fail(f"wait-output did not return the joined logical line: {res!r}")
    ok("wait-output-wrapped")
    expect_json(["pane", "close", pid], "close narrow pane")


def no_server_error(r):
    """The error object of a `server_not_running` reply (exit 1, JSON on stderr), else None."""
    if r.returncode != 1 or r.stdout.strip():
        return None
    try:
        err = json.loads(r.stderr)["error"]
    except (json.JSONDecodeError, KeyError, TypeError):
        return None
    if isinstance(err, dict) and err.get("code") == "server_not_running":
        return err
    return None


def step_close_last_session():
    expect_ok(["new", "-d", "-s", "last"], "new -d -s last (fresh server)")
    poll(lambda: os.path.exists(STATE), lambda exists: exists,
         "state file written for session last", timeout=10)
    panes = poll(lambda: pane_list("last"), lambda ps: len(ps) == 1,
                 "one pane in session last", timeout=10)
    pid = check_pane_id(panes[0].get("pane_id"), "session last")
    closed = expect_json(["pane", "close", pid], "pane close of the last session's only pane")
    if closed.get("closed") != pid:
        fail(f"pane close did not report closed: {closed!r}")
    reply, _ = poll(lambda: (run(["pane", "list"]), os.path.exists(STATE)),
                    lambda v: no_server_error(v[0]) is not None and not v[1],
                    "server exit and state file removal after the last session closed", timeout=10)
    err = no_server_error(reply)
    if not isinstance(err.get("message"), str):
        fail(f"server_not_running error has no message: {err!r}")
    ok("no-server-json-error")
    time.sleep(1.0)
    if os.path.exists(STATE):
        fail("state file reappeared after the last session closed")
    ok("close-last-session-clears-state")


def scenario():
    p1 = step_create_session()
    p2, p3 = step_split(p1)
    step_commands(p1)
    step_keys(p1)
    step_agents(p1)
    step_agent_rename_rejects_pane_id(p1)
    step_errors(p1)
    step_help()
    step_pane_ops(p2, p3)
    step_wait_output_wrapped(p1)
    step_persistence(p1)
    step_close_last_session()


def cleanup(failed):
    try:
        run(["kill-server"], timeout=30)
    except Fail:
        pass
    if failed and os.path.exists(LOG):
        print("---- server log ----")
        with open(LOG, encoding="utf-8", errors="replace") as fh:
            print(fh.read()[-6000:])
    shutil.rmtree(WORK, ignore_errors=True)


def main():
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(errors="replace")
    if not os.path.exists(EXE):
        print(f"FAIL: tmuxlike binary not found: {EXE}", file=sys.stderr)
        shutil.rmtree(WORK, ignore_errors=True)
        return 1
    failed = True
    try:
        scenario()
        failed = False
    except Fail as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
    except Exception:  # unexpected: keep the traceback for triage
        print("FAIL: unexpected error", file=sys.stderr)
        traceback.print_exc()
    finally:
        cleanup(failed)
    if failed:
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
