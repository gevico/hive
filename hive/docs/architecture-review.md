# Hive v1 Architecture Review

## Scope

This review covers the current Hive v1 implementation around:

- authoritative state in `.hive/tasks/<id>/state.json`
- per-task and orchestrator locking
- crash recovery behavior for `write_task_state()` and `hive exec`
- gaps that should drive the next recovery-focused integration tests

## Current Guarantees

### State Authority

- `state.json` is the only authoritative runtime record
- `state.md` is regenerated from `state.json` and is never read as input
- task-level state writes go through `write_task_state()` in `hive-core/src/storage.rs`

This is the right authority model for v1. It keeps recovery centered on a single file per task and avoids split-brain behavior between Markdown and JSON views.

### Atomic File Replacement

`write_task_state()` writes to `state.json.tmp` and then renames the temp file over `state.json`.

Implications:

- a process crash before `rename()` should leave the previous `state.json` intact
- a process crash after `rename()` should expose either the old file or the new file, not a partial JSON write
- a stale `.tmp` file can remain behind after a crash

This matches the current unit-test intent in `storage.rs`, but the implementation does not call `fsync()` on the temp file or parent directory. The current guarantee is therefore process-crash safety, not power-loss durability.

### Locking Model

- per-task exclusion uses `flock(2)` on `.hive/tasks/<id>/lock`
- orchestrator exclusion uses `flock(2)` on `.hive/orchestrator.lock`
- stale lock cleanup is attempted before acquisition when the PID is dead and the file is older than five minutes

The current `lock.rs` tests prove same-process/different-FD exclusion and orchestrator lock contention, which is a useful lower bound. They do not yet prove real cross-process contention.

## Recovery Semantics by Stage

### Crash During `write_task_state()`

Expected current behavior:

- crash before temp write finishes: previous `state.json` survives
- crash after temp write but before rename: previous `state.json` survives and `.tmp` may remain
- crash after rename: new `state.json` is visible

What is still missing:

- an integration test that injects a crash before `rename()`
- a doctor/anomaly policy for leftover `.tmp` files

### Crash Between `exec` State Transitions

The current `exec` loop is restartable in some stages because it re-enters based on persisted state:

- `pending -> assigned`: restart can continue with `isolate`
- `assigned` with existing worktree: restart can continue with `launch`
- `in_progress`: restart can continue with `report`
- `review`: restart can continue with `check` and completion

This is a good coarse-grained recovery shape, but it is not fully idempotent.

## Main Recovery Risk

The failure path is not restart-idempotent once a task is already in `failed`.

`handle_task_failure()` increments `retry_count`, persists `failed`, and then persists the retry/block transition. If the process dies after writing `failed` but before writing the follow-up `pending` or `blocked` state, a later restart enters the `TaskState::Failed` branch and increments `retry_count` again.

That means one worker failure can consume two retries across a crash boundary. This is the highest-value recovery bug to lock down next with an integration test before changing behavior.

## Test Coverage Assessment

### What Existing Tests Already Prove

- `storage.rs`: JSON round-trip, directory scanning, derived `state.md`, and basic atomic-write intent
- `lock.rs`: flock acquisition, second FD conflict inside one process, orchestrator lock conflict, parent dir creation
- `exec_flow.rs`: end-to-end binary coverage for launch contract, `check` exit code, malformed spec preflight, missing-plan skip, blocked dependency fan-out, and retry-to-block convergence

### What They Still Do Not Prove

- true two-process competition for the same task lock
- crash windows around `write_task_state()`
- restart semantics after a crash in the middle of `exec`
- worktree recovery after isolation succeeds but later steps do not

## Recommendations

1. Add test-only crash hooks around `write_task_state()` and around the major `exec` checkpoints.
2. Add a real subprocess-based claim contention test instead of relying only on same-process FD behavior.
3. Add restart tests that kill a child process after a persisted intermediate state and verify that the second run converges without extra retry consumption.
4. Treat leftover `state.json.tmp` as diagnosable state in `doctor` once the recovery tests exist.
