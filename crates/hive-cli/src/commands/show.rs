use anyhow::{Result, bail};
use hive_core::storage::{self, HivePaths};

pub fn run(task_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let state = storage::read_task_state(&paths, &task_id)?;

    println!("Task: {}", state.task_id);
    println!("Draft: {}", state.draft_id);
    println!("State: {}", state.state);
    println!("Approval: {}", state.approval_status);
    println!("Retries: {}", state.retry_count);
    println!(
        "Base commit: {}",
        state.base_commit.as_deref().unwrap_or("(none)")
    );
    println!("Spec hash: {}", state.spec_content_hash);
    println!("Created: {}", state.created_at.format("%Y-%m-%d %H:%M:%S"));
    println!("Updated: {}", state.updated_at.format("%Y-%m-%d %H:%M:%S"));

    // Show spec if exists
    let spec_path = paths.spec_file(&task_id);
    if spec_path.exists()
        && let Ok(content) = std::fs::read_to_string(&spec_path)
        && let Ok(spec) = hive_core::task::parse_spec(&content)
    {
        println!("Complexity: {}", spec.complexity);
        if !spec.depends_on.is_empty() {
            println!("Depends on: {}", spec.depends_on.join(", "));
        }
    }

    Ok(())
}
