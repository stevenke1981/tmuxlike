# Herdr alignment delivery

## Scope and baseline

- Request: use agent-team-workflow, Astra coordination and GPT-6.1 Sol implementation, align tmuxlike with https://github.com/herdrdev/herdr.git.
- tmuxlike baseline: `a2214442fc3b7c4d1804e94cb6076910c88b5b23`.
- Reference snapshot: `143bf5fd4781d164260c63fe28e7dc08054ad32f` (Herdr 0.9.5), read-only checkout `D:/tmuxlike-herdr-reference`.
- Delivery scope: native agent automation and explicit structural session save/restore. Existing terminal controls and CLI remain compatible.
- Reference behavior is used for design; no upstream source is copied. This is not full Herdr API compatibility.
- Existing untracked `.opencode/` and `.sites-runtime/` are outside scope.

## Ownership and dependencies

| Task | Owner / model | Write scope | Dependency |
|---|---|---|---|
| PLAN-01 | plan_alignment / GPT-6.1 Sol high | Read-only feature and contract research | baseline |
| RESEARCH-02 | explore_runtime / GPT-6.1 Sol high | Read-only runtime extension analysis | baseline |
| CORE-01 | runtime_impl / GPT-6.1 Sol high | `src/**`, essential Cargo changes, isolated `D:/tmuxlike-agent-work` | frozen contract |
| SAVE-02 | runtime_impl / GPT-6.1 Sol high | same author, serial structural snapshots | CORE-01 |
| QA-01 | qa_alignment / GPT-6.1 Sol high | `scripts/agent_smoke.py`, README, CI; isolated `D:/tmuxlike-agent-qa` | contract, then candidate binary |
| REVIEW-01 | independent Rust reviewer | Read-only candidate review | CORE-01 + SAVE-02 + QA-01 |
| INTEGRATE-01 | root coordinator | `docs/team/herdr-alignment/**`, integrate verified author files | review and local verification |

No push, remote merge or release publishing is included. Each author owns one worktree. Root integrates exact approved files and records their hashes; local commits are not required.

## Frozen contract (root decision, supersedes discovery drafts)

```text
pane list [--session NAME]
pane split --target %ID --direction right|down [--no-focus] [--cwd PATH]
pane read --target %ID
pane send --target %ID --text TEXT [--enter]
agent report --target %ID --status working|blocked|idle [--name NAME]
agent wait --target %ID --until working|blocked|idle --timeout MS
session save PATH
session restore PATH
```

New commands emit one JSON envelope `{success, data, error}`. Failures exit nonzero. `pane list` returns `data.server_instance_id` and `data.panes`; pane IDs use `%<number>` and are stable only for one running server. `pane read` returns `data.text`; split returns `data.pane_id`. Agent reports/waits expose `pane_id`, `status`, and `status_source`. The CLI parses argv directly, without reconstructing a shell command.

Explicit reports take precedence over output activity and are displayed consistently in the sidebar. Wait checks only reported status, with a mandatory finite timeout (0 immediate; maximum 300000 ms), without holding the runtime mutex. It is a state predicate, not proof of completion of a newly submitted task. Unreported panes wait until a report or timeout. Legacy capture/send gain precise `%ID` targeting.

Save/restore records versioned session/window/split layout, focus, dimensions and startup cwd. Restore requires an empty server, fully validates before spawning, rolls back on failure, and returns old/new ID mappings. Restored shells are new processes; statuses and terminal input are not replayed. Save must preserve existing snapshots on failures.

## Acceptance and evidence

| ID | Required observable behavior |
|---|---|
| AC1 | JSON discovery and precise pane control work without an attached TUI. |
| AC2 | Read/send/report and no-focus split preserve focus; invalid targets fail. |
| AC3 | Literal text survives quotes, backslashes and Unicode; real shells produce expected output. |
| AC4 | Explicit working/blocked/idle is consistent in JSON and rendered sidebar. |
| AC5 | Concurrent report unblocks wait; quiet output cannot satisfy it; timeout and removed targets fail. |
| AC6 | Save/restart/restore rebuilds layout/startup cwd/focus with fresh responsive shells. |
| AC7 | Invalid snapshots, unavailable cwd and occupied restore fail without partial sessions or loss of existing files. |
| AC8 | Existing unit/ConPTY smoke passes, formatter and applicable static checks pass, independent review findings resolved. |

Baseline Windows: `cargo test` exit 0, 13 tests; `cargo build` exit 0; `python scripts/smoke.py target/debug/tmuxlike.exe` exit 0. Coverage must be measured; test count does not establish the requested 80%.

## Remaining reference gaps

SSH and multi-machine aggregation; plugin marketplace/runtime; automatic agent detection and provider hooks; agent launch/readiness and prompt-turn tracking; conversation resumption; automatic crash persistence; screen history; process handoff across upgrades; live shell cwd tracking. These require separate acceptance and are not represented as implemented by this slice.
