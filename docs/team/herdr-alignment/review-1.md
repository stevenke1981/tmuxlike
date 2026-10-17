# REV-1 independent review (Sonnet 5.5, read-only) — range a221444..2fbdeec

Verdict: APPROVE WITH FIXES. No CRITICAL. Locking verified (no sleep under lock, no double lock).

Fixed by coordinator in 511ca59: H1 (closing last session leaves state file → resurrected), L1 (help to stdout/exit 0),
L2 (no-server JSON error), L4 (persist off / server stop), L10 (startup output marks done). Shared helpers added in
`src/api/mod.rs`: `lock(shared)`, `POLL_INTERVAL`, `LABEL_MAX`, `clean_text(s, max)`, `is_agent_name(s)`.

Open findings (assigned in fix wave 1):
- M1 `agent rename p2 p1` accepted; `p1` then resolves to pane p1, so prompts can hit the wrong pane. Use `api::is_agent_name`
  everywhere names are validated (cli, agent.rs normalize_name, persist restore of agent_name; drop invalid/duplicate on restore).
- M2 persist.rs:166-183 partial restore: when restored < attempted, copy original to `<file>.partial.bad` before returning.
- M3 persist.rs:156-160 non-UTF-8 / permission read errors are not quarantined; read bytes, quarantine on any error except NotFound.
- M4 skills/tmuxlike/SKILL.md: `agent rename` before any report-agent fails (agent_not_found); reorder.
- M5 README overstates cwd persistence: live cwd only on Linux; Windows/macOS save the start directory.
- M6 pane.rs wait-output matches per screen row; wrapped lines never match. Join soft-wrapped rows (vt100 `row_wrapped`).
- L3 `pane zoom` / `pane focus --direction` without target should fall back to `current` like split/current.
- L5 agent.rs:222 report without --seq resets stored seq to None; keep the stored seq.
- L6 restored agents have source "restore"; release-agent with the hook's source fails. Treat source "restore" as replaceable/releasable by a matching agent label.
- L7 persist quote_resume: also quote args starting with `@`, `=`, `~`; document supported shells (POSIX sh, PowerShell; not cmd.exe).
- L8 pane rename keeps control chars; use `api::clean_text(label, LABEL_MAX)`.
- L9 persist: fsync (`File::sync_all`) the tmp file before rename.
- L11 `--match` against right-trimmed lines never matches trailing spaces; match literal against untrimmed logical line.
- Maintainability: cli.rs 1739 lines, pane.rs 908, persist.rs 818 (> 800). Duplicates: POLL_INTERVAL, label cap, lock helpers with
  different poison policies, two poll loops.
