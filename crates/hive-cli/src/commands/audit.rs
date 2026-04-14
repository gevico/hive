use anyhow::{Result, bail};
use hive_core::storage::HivePaths;

pub fn run(task: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    if let Some(task_id) = task {
        let audit_path = paths.audit_file(&task_id);
        let content = hive_audit::read_audit(&audit_path)?;
        println!("{content}");
    } else {
        // Show audit from all tasks
        let ids = hive_core::storage::list_task_ids(&paths)?;
        if ids.is_empty() {
            println!("no tasks found");
            return Ok(());
        }
        for id in &ids {
            let audit_path = paths.audit_file(id);
            if audit_path.exists() {
                println!("=== {id} ===");
                let content = hive_audit::read_audit(&audit_path)?;
                println!("{content}");
            }
        }
    }

    Ok(())
}
