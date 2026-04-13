use anyhow::{Result, bail};
use hive_core::storage::{self, HivePaths};
use hive_git::worktree;

pub fn run(task_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let state = storage::read_task_state(&paths, &task_id)?;
    if state.state != hive_core::TaskState::Completed {
        bail!(
            "task {} is in state '{}', must be 'completed' to cleanup",
            task_id,
            state.state
        );
    }

    let wt_path = paths.worktree_path(&task_id);
    worktree::remove(&cwd, &wt_path, &task_id)?;

    println!("task {task_id}: worktree cleaned up");
    Ok(())
}
