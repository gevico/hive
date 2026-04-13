use anyhow::{Result, bail};
use hive_core::lock::FileLock;
use hive_core::state::{TaskState, TransitionAction, retry_limit};
use hive_core::storage::{self, HivePaths};

pub fn run(task_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let _lock = FileLock::try_acquire(&paths.lock_file(&task_id))?;
    let mut state = storage::read_task_state(&paths, &task_id)?;

    if state.state != TaskState::Failed {
        bail!(
            "task {} is in state '{}', must be 'failed' to retry",
            task_id,
            state.state
        );
    }

    if state.retry_count >= retry_limit() {
        bail!(
            "task {}: retry count {} >= limit {}, use `hive unblock` instead",
            task_id,
            state.retry_count,
            retry_limit()
        );
    }

    state.state = state
        .state
        .transition(TransitionAction::Retry, state.retry_count, true)?;
    state.retry_count += 1;
    state.touch();
    storage::write_task_state(&paths, &state)?;

    // Log audit
    if let Ok(config) = hive_core::config::load_config(&paths.hive_dir()) {
        let _ = hive_audit::log_state_change(
            &paths.audit_file(&task_id),
            config.audit_level,
            &task_id,
            "failed",
            "pending",
        );
    }

    println!(
        "task {task_id}: retrying ({}/{})",
        state.retry_count,
        retry_limit()
    );
    Ok(())
}
