use anyhow::{Result, bail};
use hive_core::lock::FileLock;
use hive_core::state::TransitionAction;
use hive_core::storage::{self, HivePaths};
use hive_git::worktree;

pub fn run(task_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let _lock = FileLock::try_acquire(&paths.lock_file(&task_id))?;
    let mut state = storage::read_task_state(&paths, &task_id)?;

    if state.state != hive_core::TaskState::Assigned {
        bail!(
            "task {} is in state '{}', must be 'assigned' to isolate",
            task_id,
            state.state
        );
    }

    let wt_path = paths.worktree_path(&task_id);
    let base_commit = worktree::create(&cwd, &wt_path, &task_id)?;

    state.base_commit = Some(base_commit.clone());
    state.state = state.state.transition(TransitionAction::Start, 0, true)?;
    state.touch();
    storage::write_task_state(&paths, &state)?;

    println!(
        "task {task_id}: isolated (worktree at {}, base {})",
        wt_path.display(),
        &base_commit[..8]
    );
    Ok(())
}
