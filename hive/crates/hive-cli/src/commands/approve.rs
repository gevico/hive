use anyhow::{Result, bail};
use hive_core::storage::{self, HivePaths};
use hive_core::task::ApprovalStatus;

pub fn run(draft_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let states = storage::load_all_states(&paths)?;
    let draft_tasks: Vec<_> = states.iter().filter(|s| s.draft_id == draft_id).collect();

    if draft_tasks.is_empty() {
        bail!("draft not found: {draft_id}");
    }

    // Idempotent: already approved is fine
    let mut count = 0;
    for s in &draft_tasks {
        if s.approval_status == ApprovalStatus::Approved {
            continue;
        }
        let mut state = storage::read_task_state(&paths, &s.task_id)?;
        state.approval_status = ApprovalStatus::Approved;
        state.touch();
        storage::write_task_state(&paths, &state)?;
        count += 1;
    }

    if count == 0 {
        println!("draft {draft_id}: already approved (no changes)");
    } else {
        println!("draft {draft_id}: approved {count} tasks");
    }

    Ok(())
}
