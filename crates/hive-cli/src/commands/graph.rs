use anyhow::{Result, bail};
use hive_core::storage::{self, HivePaths};

pub fn run() -> Result<()> {
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

    println!("Task Dependency Graph:");
    println!();

    for s in &states {
        let spec_path = paths.spec_file(&s.task_id);
        let deps = if let Ok(content) = std::fs::read_to_string(&spec_path) {
            hive_core::task::parse_spec(&content)
                .map(|spec| spec.depends_on)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let state_marker = match s.state {
            hive_core::TaskState::Completed => "[x]",
            hive_core::TaskState::InProgress => "[>]",
            hive_core::TaskState::Failed => "[!]",
            hive_core::TaskState::Blocked => "[#]",
            _ => "[ ]",
        };

        if deps.is_empty() {
            println!("{state_marker} {}", s.task_id);
        } else {
            println!("{state_marker} {} <- {}", s.task_id, deps.join(", "));
        }
    }

    Ok(())
}
