# Windows validation, 2026-10-13

Base: `a2214442fc3b7c4d1804e94cb6076910c88b5b23`.
Local host: Windows build `10.0.26300`, Rust/Cargo 1.99.0, PowerShell 5.1.26100.9444.

| Check | Result |
| --- | --- |
| Locked dependencies | All 83 cached package archives match Cargo.lock SHA-256; lock unchanged |
| Formatting and patch whitespace | Pass |
| Strict all-target clippy | Pass, warnings denied |
| Unit tests | 30 pass; isolated native clipboard worker is invoked by its parent test |
| Native Unicode clipboard | Pass in a private window station; no interactive clipboard access |
| Debug ConPTY acceptance | Pass: single paste, multiline cancel/confirm, CRLF, Unicode mouse copy, internal and mock system paste, resize, Ctrl+C, detach/attach, shell exit |
| Windows release build | Pass |
| Release ConPTY acceptance | Pass against the release executable; actual ANSI frame replays to the expected Unicode and interrupt output |
| Release smoke | Pass: PowerShell prompt, command output, 3,953-byte ANSI frame |
| Visible app window / physical keyboard / physical mouse | Not verified; desktop readiness returned false with cursor-unavailable |

Before correction, paste regressions failed on multiline consent, control-byte
sanitization, and bracketed LF framing. The wide-cell regression also reproduced
an empty copy when the selection started on a glyph's continuation cell.

Real ConPTY testing found a separate Ctrl+C failure: PowerShell's running
`Start-Sleep` did not interrupt. Removing `CREATE_NEW_PROCESS_GROUP` from the
detached server restored interruption without ending the session. Microsoft's
[process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)
describe this flag's effect on Ctrl+C. The regression verifies prompt recovery
and subsequent command output within five seconds.

Tests use independent socket names, self-made text, isolated configuration,
and a private PSReadLine history path. Clipboard round-trip tests create a private
window station in a separate test process. Creating only a desktop would share
the user's clipboard, so the test explicitly isolates the station too. No test
switches the interactive desktop, injects physical input, saves existing user
clipboard data, or restores it afterward. Release ConPTY acceptance uses mock
clipboard messages; the native clipboard API is tested separately.

The native desktop control readiness check failed in session 1 while the console
session was 3. No session transfer, unlock, alternate input tool, or foreground
application control was attempted. Process and terminal output validation do
not prove that a visible app window or physical input worked.

The branch CI runs formatting, strict clippy, build, and tests on Windows,
Linux and macOS, plus real shell smoke and Windows release acceptance. Check
the exact pushed commit's Actions result separately; local results do not
establish those runner results.

Windows 10/Server, PowerShell 7, cmd, Git Bash/WSL, IME composition, complex emoji
clusters, different fonts/terminal hosts, and native terminal shortcut interception
remain separate test matrix entries. This report does not claim complete Windows
version or terminal host compatibility.
