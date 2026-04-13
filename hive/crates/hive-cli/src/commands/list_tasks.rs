use anyhow::{Result, bail};
use hive_core::storage::{self, HivePaths};

pub fn run(state_filter: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let states = storage::load_all_states(&paths)?;
    if states.is_empty() {
        println!("no tasks found");
        return Ok(());
    }

    let filtered: Vec<_> = if let Some(ref filter) = state_filter {
        states
            .iter()
            .filter(|s| s.state.to_string() == *filter)
            .collect()
    } else {
        states.iter().collect()
    };

    if filtered.is_empty() {
        if let Some(filter) = state_filter {
            println!("no tasks in state '{filter}'");
        }
        return Ok(());
    }

    for s in &filtered {
        println!(
            "{:<30} {:<12} {:<10} {}",
            s.task_id,
            s.state,
            s.approval_status,
            s.draft_id,
        );
    }

    Ok(())
}
