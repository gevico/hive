use anyhow::{Result, bail};
use hive_core::lock::FileLock;
use hive_core::storage::{self, HivePaths};
use hive_git::worktree;

pub fn run(task_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let base_commit = isolate_task(&cwd, &paths, &task_id)?;

    let wt_path = paths.worktree_path(&task_id);
    println!(
        "task {task_id}: isolated (worktree at {}, base {})",
        wt_path.display(),
        &base_commit[..8]
    );
    Ok(())
}

pub(crate) fn isolate_task(
    repo_root: &std::path::Path,
    paths: &HivePaths,
    task_id: &str,
) -> Result<String> {
    let _lock = FileLock::try_acquire(&paths.lock_file(task_id))?;
    let mut state = storage::read_task_state(paths, task_id)?;

    if state.state != hive_core::TaskState::Assigned {
        bail!(
            "task {} is in state '{}', must be 'assigned' to isolate",
            task_id,
            state.state
        );
    }

    let wt_path = paths.worktree_path(task_id);
    let base_commit = worktree::create(repo_root, &wt_path, task_id)?;

    state.base_commit = Some(base_commit.clone());
    state.touch();
    storage::write_task_state(paths, &state)?;

    Ok(base_commit)
}
