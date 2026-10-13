"""Cross-platform smoke test (used by CI on Linux, macOS and Windows).

Starts a detached session, checks the shell prompt shows up in the pane
(PowerShell's "PS " on Windows), types a command with send-keys, and checks
that the server renders real ANSI frames.
Usage: python scripts/smoke.py [path-to-tmuxlike]
"""
import os, subprocess, sys, time, re, json

exe = sys.argv[1] if len(sys.argv) > 1 else os.path.join("target", "release", "tmuxlike" + (".exe" if os.name == "nt" else ""))
import tempfile
work = tempfile.mkdtemp(prefix="tmuxlike-smoke-")
LOG = os.path.join(work, "server.log")
config = os.path.join(work, "config.toml")
with open(config, "w", encoding="utf-8") as f:
    history = os.path.join(work, "shell-history.txt").replace("'", "''")
    shell = 'powershell.exe -NoLogo -NoProfile -NoExit -Command "Set-PSReadLineOption -HistorySavePath \'%s\' -HistorySaveStyle SaveNothing"' % history if os.name == "nt" else "/bin/sh"
    f.write("shell = " + json.dumps(shell) + "\n")
    f.write("osc52 = false\n")
env = dict(os.environ, TMUXLIKE_SOCKET="tmuxlike-smoke-%d.sock" % os.getpid(), TMUXLIKE_LOG=LOG, TMUXLIKE_CONFIG=config)
env.pop("TMUXLIKE", None)
env.pop("ENV", None)
env.pop("BASH_ENV", None)
win = os.name == "nt"


def tl(*args, check=True):
    r = subprocess.run([exe, *args], env=env, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=30)
    if check and r.returncode != 0:
        raise SystemExit(f"FAIL: tmuxlike {' '.join(args)} -> {r.returncode}: {r.stdout}{r.stderr}")
    return r.stdout


def wait_for(pred, what, timeout=45):
    end = time.time() + timeout
    out = ""
    while time.time() < end:
        out = tl("capture-pane", "-p", "-t", "ci")
        if pred(out):
            return out
        time.sleep(0.5)
    print("---- pane ----\n" + out + "\n--------------")
    raise SystemExit(f"FAIL: timed out waiting for {what}")


try:
    tl("new", "-d", "-s", "ci")
    print(tl("ls"))
    if win:
        out = wait_for(lambda o: "PS " in o and ">" in o, "PowerShell prompt")
    else:
        out = wait_for(lambda o: o.strip() != "", "shell prompt")
    print("---- initial pane ----\n" + out)
    tl("send-keys", "-t", "ci", "echo tmuxlike-smoke-ok", "Enter")
    out = wait_for(lambda o: any(l.strip() == "tmuxlike-smoke-ok" for l in o.splitlines()), "command output")
    print("---- after send-keys ----\n" + out)
    frame = tl("debug-frame", "-t", "ci")
    assert "\x1b[" in frame, "frame has no escape sequences: %r" % frame[:200]
    assert re.search(r"\x1b\[\d+;1H", frame), "frame has no cursor positioning"
    assert "[ci]" in frame, "status bar missing"
    assert "tmuxlike-smoke-ok" in frame, "pane text missing from frame"
    print("frame: %d bytes of ANSI, OK" % len(frame))
    print("PASS")
    print("Evidence directory:", work)
finally:
    tl("kill-server", check=False)
    if os.path.exists(LOG):
        print("---- server log ----")
        print(open(LOG, encoding="utf-8", errors="replace").read()[-6000:])
