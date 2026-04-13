use anyhow::{Result, bail};
use hive_core::storage::{self, HivePaths};

pub fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project (missing .hive/ directory). Run `hive init` first");
    }

    // Regenerate state.md from state.json files
    storage::regenerate_state_md(&paths)?;

    // Display the table
    let states = storage::load_all_states(&paths)?;
    if states.is_empty() {
        println!("no tasks found");
        return Ok(());
    }

    println!(
        "{:<30} {:<15} {:<12} {:>7} {:<10} UPDATED",
        "TASK ID", "DRAFT", "STATE", "RETRIES", "APPROVAL"
    );
    println!("{}", "-".repeat(95));
    for s in &states {
        println!(
            "{:<30} {:<15} {:<12} {:>7} {:<10} {}",
            s.task_id,
            truncate(&s.draft_id, 15),
            s.state,
            s.retry_count,
            s.approval_status,
            s.updated_at.format("%Y-%m-%d %H:%M"),
        );
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() > max {
        &s[..max]
    } else {
        s
    }
}
