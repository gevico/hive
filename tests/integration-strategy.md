# Hive v1 Integration Test Strategy

## Goal

Define a temp-repo-based integration test harness for Hive v1 with emphasis on:

- `hive init`
- worktree lifecycle
- `exec` state transitions and crash recovery
- concurrent claim behavior

This strategy extends the current unit coverage in `hive-core/src/storage.rs` and `hive-core/src/lock.rs` with process-level tests.

## Fixture Repository Design

Use ephemeral git repositories created inside `TempDir`, not committed fixture repos.

Recommended shared helper:

- path: `hive/crates/hive-cli/tests/common.rs`
- type: `TestRepo`

Recommended helper surface:

- `fn new_git_repo(config: &str) -> TestRepo`
- `fn write_task(task_id, draft_id, state, spec_body)`
- `fn write_task_with(task_id, draft_id, state, spec, with_plan)`
- `fn run_hive(args: &[&str]) -> Output`
- `fn git(args: &[&str])`
- `fn current_head() -> String`

Minimal repo bootstrap:

1. `git init -b main`
2. set local `user.name` and `user.email`
3. create one committed file so worktree/base-commit operations have a stable HEAD
4. create `.hive/` required directories plus config files

This is already close to the current `exec_flow.rs` fixture; the next step is to extract and reuse it.

## Crash Recovery Strategy

Unit tests alone cannot prove crash behavior. Add test-only failpoints controlled by environment variables.

Suggested hooks:

- `HIVE_TEST_CRASH_AT=write_task_state_before_rename`
- `HIVE_TEST_CRASH_AT=exec_after_assign_write`
- `HIVE_TEST_CRASH_AT=exec_after_isolate`
- `HIVE_TEST_CRASH_AT=exec_after_in_progress_write`
- `HIVE_TEST_CRASH_AT=exec_after_failed_write`

Implementation rule:

- the child process exits immediately with a distinctive code after persisting the target intermediate state
- the parent test then reruns `hive exec` or the relevant command and verifies convergence

## Concurrent Claim Strategy

Use two real subprocesses against the same temp repo and the same task.

Test shape:

1. seed one `pending` approved task
2. start process A: `hive claim --task <id>`
3. start process B at the same time
4. assert exactly one exits `0`
5. assert the other exits non-zero with lock or transition failure
6. assert final `state.json` is `assigned`
7. assert retry count remains `0`

This complements `lock.rs`, which currently proves lock contention only through separate file descriptors in one process.

## Specific Tests To Add

### Storage / Recovery

- `write_task_state_crash_before_rename_preserves_previous_state`
- `write_task_state_leaves_tmp_file_but_keeps_last_committed_json`
- `exec_resume_after_assign_crash_continues_to_isolate_and_launch`
- `exec_resume_after_isolate_crash_reuses_existing_worktree`
- `exec_resume_after_in_progress_crash_reports_and_checks_result`
- `exec_resume_after_failed_write_does_not_double_increment_retry`

### Init / Worktree

- `init_creates_expected_hive_layout_in_temp_git_repo`
- `init_is_idempotent_in_existing_hive_repo`
- `isolate_creates_worktree_branch_and_records_base_commit`
- `cleanup_removes_worktree_and_branch_for_completed_task`

### Concurrency

- `claim_two_processes_same_task_exactly_one_succeeds`
- `orchestrator_lock_two_exec_processes_only_one_runs`

### Execution Flow

- `exec_skips_missing_plan_without_stalling`
- `exec_skips_dependents_of_missing_plan_tasks`
- `exec_fails_fast_on_malformed_spec`
- `exec_blocks_downstream_when_dependency_blocks`
- `exec_retries_failed_verification_until_blocked`

## Prioritization

1. `claim_two_processes_same_task_exactly_one_succeeds`
2. `write_task_state_crash_before_rename_preserves_previous_state`
3. `exec_resume_after_failed_write_does_not_double_increment_retry`
4. `isolate_creates_worktree_branch_and_records_base_commit`
5. `orchestrator_lock_two_exec_processes_only_one_runs`

The first three tests cover the highest-risk gaps that are not currently locked down by unit tests or the existing binary fixtures.
