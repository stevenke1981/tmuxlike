# Windows terminal and clipboard

tmuxlike is a terminal application. Open PowerShell or Windows Terminal and run
`tmuxlike new -s work`. ConPTY needs Windows 10 version 1809 or newer; that API
minimum is not a claim that every supported Windows build has been tested.
See Microsoft's [CreatePseudoConsole requirements](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole).

With the default prefix, press **Ctrl-b**, release it, then:

- **[** enters copy mode; move with arrows, **v** starts selection, **y** copies.
  Mouse drag also copies when mouse support is enabled.
- **]** pastes tmuxlike's internal buffer.
- **Shift-p (P)** reads and pastes the attaching Windows client's system clipboard.
  The `paste-clipboard` command can be bound to another prefix key.

Ordinary **Ctrl-c** still interrupts the pane's program; **Ctrl-v** remains a
program key. Windows Terminal may consume Ctrl-Shift-C/V itself. Prefer the prefix
bindings when you need tmuxlike's selection or atomic, confirmed paste.

Copy writes Unicode text through the Windows clipboard API. It does not depend
on OSC 52 support in the outer terminal. It targets the client that copied, not
other attached clients. Set `osc52 = false` (or `set-clipboard off`) to keep copies
in the internal buffer only. Clipboard failures appear in the status bar; the
internal buffer is retained. Startup does not read or write the clipboard.
System paste replies are matched to a request and its original pane. A changed
pane or an opened prompt cancels the reply instead of inserting into a new input.

A paste containing a newline, including a trailing newline, is held for review.
**y** confirms; **Esc**, **n**, or **Ctrl-c** cancels; Enter does not confirm.
After confirmation, a shell without bracketed paste may execute the pasted
commands. Single-line paste does not append Enter. CRLF and CR are normalized;
Native repeated y key-down events are held until a release and a fresh press.
terminal control characters are removed. Applications that enable bracketed
paste receive the bracketed sequence with LF, while plain shell input uses CR.
Terminal-native paste shortcuts that arrive as individual key events cannot be
identified as one paste on all Windows hosts. Use **Ctrl-b P** for the confirmation
guarantee on Windows. `send-keys` is an intentional command interface and is not
subject to paste confirmation.

For an executable path containing spaces, use Windows command-line quoting:

```toml
shell = '"C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo -NoProfile'
```

After upgrading, detach from and restart an old server before attaching with the
new client; client and server must use the same binary version. Restarting a
server ends its sessions, so preserve any running work first.

## Validation

`cargo test --locked` includes Unicode/wide-cell extraction, paste sanitization
and consent, client-specific copy requests, Windows shell path parsing, a native
clipboard round trip in a **private window station**, and real PowerShell/ConPTY
acceptance. The private station owns its own clipboard and never reads, saves,
or replaces the interactive user's clipboard. Acceptance uses mock clipboard
messages and self-created text; it tests copy, internal/system paste dispatch,
CRLF confirmation/cancel, resize, Ctrl-c interruption, detach/attach, and shell exit.
Acceptance and smoke use an isolated PSReadLine history path with saving disabled.

To repeat acceptance against a release build:

```powershell
cargo build --release --locked
$env:TMUXLIKE_TEST_EXE = (Resolve-Path target/release/tmuxlike.exe).Path
cargo test --test windows_acceptance --locked -- --nocapture
python scripts/smoke.py target/release/tmuxlike.exe
```

These tests check real terminal output and ANSI frames, not physical keyboard
delivery or visible desktop pixels. The test matrix and desktop verification
results for this change are recorded in the accompanying validation report.
PowerShell 5.1 is the automated Windows shell; PowerShell 7, cmd, Git Bash/WSL,
IME composition, complex emoji clusters, Windows 10/Server, and different terminal
host shortcuts need separate interactive checks.
