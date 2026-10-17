# herdr alignment — dispatch plan

- Request (2026-10-17): use agent-team-workflow; Opus 5.5 (high) coordinates, Haiku 5.5 (high) implements; align tmuxlike with herdr (https://github.com/herdrdev/herdr.git).
- Baseline: `main` a221444 — `cargo test` 13 pass, `cargo build` ok (Windows, rustc 1.94.1).
- Reference: herdr `143bf5f` (2026-10-16), shallow read-only clone in the coordinator scratchpad. Design reference only; no code copied (herdr is Apache-2.0, tmuxlike MIT).
- Supersedes `dispatch-plan.codex-draft.md` (earlier attempt; only a stub was produced).

## Scope (this slice)
herdr's agent-native automation surface and layout persistence, mapped onto tmuxlike's model
(workspace≈session, tab≈window): JSON CLI `pane …` / `agent …` / `session list` / `api snapshot` /
`server [stop]`; agent status working/blocked/done/idle/unknown (reported or output-heuristic) in
JSON and sidebar; waits without holding the server lock; `TMUXLIKE_PANE_ID` + `--current`;
automatic layout save/restore across server restarts with optional agent resume command.

Non-goals (documented as gaps): SSH machines, plugins/marketplace, manifest-based agent detection,
integrations install, worktree/workspace/tab API, notifications, event subscriptions, terminal
observe/control streams, live upgrade handoff, kitty graphics.

## Tasks
| Task | Owner (instance / model) | Write scope | Depends |
|---|---|---|---|
| SKEL-0 | coordinator / Opus 5.5 | `src/api/mod.rs`, `server.rs`, `main.rs`, `client.rs`, `proto.rs`, `ipc.rs`, `config.rs`, `layout.rs`, `Cargo.*` | baseline |
| H1-CLI | rust-worker-cli / Haiku 5.5 high | `src/api/cli.rs` @ `D:/tmuxlike-wt/cli` | SKEL-0 |
| H2-PANE | rust-worker-pane / Haiku 5.5 high | `src/api/pane.rs` @ `D:/tmuxlike-wt/pane` | SKEL-0 |
| H3-AGENT | rust-worker-agent / Haiku 5.5 high | `src/api/agent.rs`, `src/render.rs` @ `D:/tmuxlike-wt/agent` | SKEL-0, H2 signatures |
| H4-PERSIST | rust-worker-persist / Haiku 5.5 high | `src/persist.rs` @ `D:/tmuxlike-wt/persist` | SKEL-0 |
| H5-QA | qa-worker-01 / Haiku 5.5 high | `scripts/api_smoke.py`, `skills/tmuxlike/SKILL.md`, `README.md`, `ci.yml` @ `D:/tmuxlike-wt/qa` | contract |
| INT-1 | coordinator | integrate owned files, wire shims, full test + smoke | H1–H5 |
| REV-1 | independent reviewer (separate instance) | read-only review | INT-1 |

Shared contract and hard rules: `brief.md`. No push, PR, release or deploy without explicit approval.

## Acceptance
| ID | Observable behavior |
|---|---|
| AC1 | `pane list/get/current`, `session list`, `api snapshot` return JSON without an attached client. |
| AC2 | split/focus/zoom/rename/close act on exact pane ids; split does not steal focus unless `--focus`. |
| AC3 | send-text/run preserve quotes, backslashes, CJK; send-keys validates all keys before writing. |
| AC4 | wait-output and agent wait poll without holding the lock; timeout returns `timeout`. |
| AC5 | report/release/rename/prompt follow herdr semantics (stale seq ignored, blocked prompt refused). |
| AC6 | Sidebar and JSON show the same status. |
| AC7 | `server stop` + restart restores sessions/windows/splits/cwd; `kill-server` clears; invalid state file is quarantined, never deleted. |
| AC8 | `cargo test`, `cargo clippy`, `cargo fmt --check`, `scripts/smoke.py` and `scripts/api_smoke.py` pass on Windows locally; CI matrix covers Linux/macOS (requires push → not run without approval). |
