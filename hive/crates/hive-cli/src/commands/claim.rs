use anyhow::{Result, bail};
use hive_core::lock::FileLock;
use hive_core::state::TransitionAction;
use hive_core::storage::{self, HivePaths};

pub fn run(task_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    claim_task(&paths, &task_id)?;

    println!("task {task_id}: claimed (pending -> assigned)");
    Ok(())
}

pub(crate) fn claim_task(paths: &HivePaths, task_id: &str) -> Result<()> {
    let _lock = FileLock::try_acquire(&paths.lock_file(task_id))?;
    let mut state = storage::read_task_state(paths, task_id)?;

    // Check if all dependencies are completed
    let all_states = storage::load_all_states(paths)?;
    let spec_content = std::fs::read_to_string(paths.spec_file(task_id)).ok();
    let depends_on = if let Some(ref content) = spec_content {
        hive_core::task::parse_spec(content)
            .map(|s| s.depends_on)
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let deps_completed = depends_on.iter().all(|dep| {
        all_states
            .iter()
            .any(|s| s.task_id == *dep && s.state == hive_core::TaskState::Completed)
    });

    let new_state =
        state
            .state
            .transition(TransitionAction::Assign, state.retry_count, deps_completed)?;

    state.state = new_state;
    state.touch();
    storage::write_task_state(paths, &state)?;
    Ok(())
}
