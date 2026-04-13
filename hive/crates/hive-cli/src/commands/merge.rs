use anyhow::{Result, bail};
use hive_core::config;
use hive_core::storage::{self, HivePaths};
use hive_git::{branch, merge, worktree};

pub fn run(task: Option<String>, all: bool, mode: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    if all {
        merge_all(&cwd, &paths, &mode)?;
    } else if let Some(task_id) = task {
        merge_task(&cwd, &paths, &task_id, &mode)?;
    } else {
        bail!("specify --task <id> or --all");
    }

    Ok(())
}

fn merge_task(
    repo_root: &std::path::Path,
    paths: &HivePaths,
    task_id: &str,
    mode: &str,
) -> Result<()> {
    let state = storage::read_task_state(paths, task_id)?;
    if state.state != hive_core::TaskState::Completed {
        bail!(
            "task {} is in state '{}', must be 'completed' to merge",
            task_id,
            state.state
        );
    }

    let task_branch = worktree::branch_name(task_id);
    let default = branch::default_branch(repo_root)?;

    // Rebase onto main
    branch::rebase(repo_root, &task_branch, &default)?;

    match mode {
        "direct" => {
            // Checkout default and merge
            let _ = std::process::Command::new("git")
                .args(["checkout", &default])
                .current_dir(repo_root)
                .output()?;
            branch::merge_branch(repo_root, &task_branch)?;
            println!("task {task_id}: merged directly to {default}");
        }
        _ => {
            let hive_config = config::load_config(&paths.hive_dir())?;
            let platform = merge::Platform::parse(&hive_config.rfc.platform);

            // Push branch
            let _ = std::process::Command::new("git")
                .args(["push", "-u", "origin", &task_branch])
                .current_dir(repo_root)
                .output();

            let url = merge::create_pr(
                repo_root,
                &platform,
                &task_branch,
                &format!("hive: merge task {task_id}"),
                &format!("Automated merge for hive task {task_id}"),
                &[],
            )?;

            if let Some(url) = url {
                println!("task {task_id}: PR created at {url}");
            } else {
                println!("task {task_id}: branch {task_branch} ready for review");
            }
        }
    }

    Ok(())
}

fn merge_all(
    repo_root: &std::path::Path,
    paths: &HivePaths,
    mode: &str,
) -> Result<()> {
    let states = storage::load_all_states(paths)?;
    let completed: Vec<_> = states
        .iter()
        .filter(|s| s.state == hive_core::TaskState::Completed)
        .collect();

    if completed.is_empty() {
        println!("no completed tasks to merge");
        return Ok(());
    }

    // TODO: dependency-ordered merge
    for s in completed {
        match merge_task(repo_root, paths, &s.task_id, mode) {
            Ok(()) => {}
            Err(e) => eprintln!("failed to merge {}: {e}", s.task_id),
        }
    }

    Ok(())
}
