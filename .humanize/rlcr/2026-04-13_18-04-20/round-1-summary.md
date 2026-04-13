# Round 1 Summary

## Work Completed

- Repaired the execution path so `hive exec` now drives the real Layer 1 pipeline through shared helpers: `claim -> isolate -> launch -> report -> check`, instead of mutating state inline and force-completing on `result.md` presence.
- Realigned the Layer 1 state contract:
  - `hive isolate` now only creates the worktree and records `base_commit`
  - `hive launch` is the `assigned -> in_progress` entry point
  - `hive report` validates typed `result.md` frontmatter and updates state through the state machine
  - `hive check` returns deterministic `0/1/2/3` outcomes without hiding missing-task and wrong-state cases behind the top-level CLI error path
- Added shared command-runtime helpers for exit-code failures and state-change audit logging.
- Tightened frontmatter/result parsing in the execution path:
  - optional string-list fields now reject wrong item types instead of silently coercing
  - `task::parse_result()` now enforces `id/status/branch/commit/base_commit/schema_version`
- Added targeted binary-level integration tests for:
  - `launch` accepting `assigned` and moving the task to `in_progress`
  - `check` returning exit code `2` for a missing task
  - `exec` refusing to mark a task `completed` when verifier checks fail

## Files Changed

- `hive/crates/hive-core/src/frontmatter.rs`
- `hive/crates/hive-core/src/task.rs`
- `hive/crates/hive-cli/src/commands/mod.rs`
- `hive/crates/hive-cli/src/commands/runtime.rs`
- `hive/crates/hive-cli/src/commands/check.rs`
- `hive/crates/hive-cli/src/commands/exec.rs`
- `hive/crates/hive-cli/src/commands/launch.rs`
- `hive/crates/hive-cli/src/commands/report.rs`
- `hive/crates/hive-cli/Cargo.toml`
- `hive/crates/hive-cli/tests/exec_flow.rs`
- `.humanize/rlcr/2026-04-13_18-04-20/goal-tracker.md`

## Validation

- `cargo test -q` in `hive/`: pass
  - existing unit tests: pass
  - new `hive-cli` integration tests: 3 pass
- `cargo fmt --all` in `hive/`: pass

## Remaining Items

- `task13` is improved but not fully closed: broader end-to-end coverage is still missing, especially around dependency fan-out, crash recovery, and multi-pass retry behavior.
- `task12` is only partially closed: `report` now validates result frontmatter and records state-change audit, but full audit/event coverage is still pending.
- `task10` still lacks skill/context injection and any remaining `retry` command surface.
- The queued collaboration/adapter work remains untouched this round: RFC/approve/merge flow, adapter generation, doctor completeness, and the analyze deliverables.

## BitLesson Delta

- Action: none
- Lesson ID(s): NONE
- Notes: `.humanize/bitlesson.md` currently has no project lessons; `bitlesson-selector` was invoked for the mainline task but did not return a usable lesson selection, so no new lesson was added this round.
