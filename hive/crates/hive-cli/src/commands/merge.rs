use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use hive_core::config;
use hive_core::lock::FileLock;
use hive_core::state::TaskState;
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
    if state.state != TaskState::Completed {
        bail!(
            "task {} is in state '{}', must be 'completed' to merge",
            task_id,
            state.state
        );
    }

    // Verify dependencies are already merged (not just completed)
    let spec_path = paths.spec_file(task_id);
    if let Ok(content) = std::fs::read_to_string(&spec_path)
        && let Ok(spec) = hive_core::task::parse_spec(&content)
    {
        for dep in &spec.depends_on {
            let dep_state = storage::read_task_state(paths, dep)?;
            if dep_state.state != TaskState::Completed {
                bail!(
                    "cannot merge {task_id}: dependency {dep} is in state '{}'",
                    dep_state.state
                );
            }
            if !dep_state.merged {
                bail!(
                    "cannot merge {task_id}: dependency {dep} is completed but not yet merged"
                );
            }
        }
    }

    let task_branch = worktree::branch_name(task_id);
    let default = branch::default_branch(repo_root)?;

    // Rebase onto main — conflict directly marks task as blocked
    if branch::rebase(repo_root, &task_branch, &default).is_err() {
        eprintln!("task {task_id}: rebase conflict, marking as blocked");
        let _lock = FileLock::try_acquire(&paths.lock_file(task_id))?;
        let mut state = storage::read_task_state(paths, task_id)?;
        // Merge conflict is an external event; directly set blocked without going through failed
        state.state = TaskState::Blocked;
        state.touch();
        storage::write_task_state(paths, &state)?;
        bail!("merge conflict in task {task_id}, task blocked for manual resolution");
    }

    // Log audit
    if let Ok(cfg) = config::load_config(&paths.hive_dir()) {
        let _ = hive_audit::log_merge(
            &paths.audit_file(task_id),
            cfg.audit_level,
            task_id,
            &format!("merged via {mode}"),
        );
    }

    match mode {
        "direct" => {
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

    // Mark task as merged in state.json
    let _lock = FileLock::try_acquire(&paths.lock_file(task_id))?;
    let mut state = storage::read_task_state(paths, task_id)?;
    state.merged = true;
    state.touch();
    storage::write_task_state(paths, &state)?;

    Ok(())
}

fn merge_all(repo_root: &std::path::Path, paths: &HivePaths, mode: &str) -> Result<()> {
    let states = storage::load_all_states(paths)?;
    let completed: Vec<_> = states
        .iter()
        .filter(|s| s.state == TaskState::Completed)
        .collect();

    if completed.is_empty() {
        println!("no completed tasks to merge");
        return Ok(());
    }

    // Build dependency graph and topological sort for merge order
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    for s in &completed {
        let spec_path = paths.spec_file(&s.task_id);
        let dep_list = if let Ok(content) = std::fs::read_to_string(&spec_path) {
            hive_core::task::parse_spec(&content)
                .map(|spec| spec.depends_on)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        deps.insert(s.task_id.clone(), dep_list);
    }

    let order = topological_sort(&deps);
    let task_ids: HashSet<String> = completed.iter().map(|s| s.task_id.clone()).collect();
    let mut merged: HashSet<String> = HashSet::new();

    for task_id in &order {
        if !task_ids.contains(task_id) {
            continue;
        }
        // Verify all dependencies have been successfully merged in this pass
        if let Some(task_deps) = deps.get(task_id) {
            let unmerged: Vec<_> = task_deps
                .iter()
                .filter(|d| task_ids.contains(*d) && !merged.contains(*d))
                .collect();
            if !unmerged.is_empty() {
                eprintln!(
                    "skipping {task_id}: dependencies not yet merged: {}",
                    unmerged.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                );
                continue;
            }
        }
        match merge_task(repo_root, paths, task_id, mode) {
            Ok(()) => {
                merged.insert(task_id.clone());
            }
            Err(e) => {
                // Stop downstream processing when upstream merge fails
                eprintln!("failed to merge {task_id}: {e}");
                eprintln!("stopping merge --all: upstream failure prevents downstream merges");
                break;
            }
        }
    }

    Ok(())
}

fn topological_sort(deps: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut visited = HashSet::new();
    let mut order = Vec::new();
    for node in deps.keys() {
        topo_dfs(node, deps, &mut visited, &mut order);
    }
    // DFS post-order naturally gives dependencies before dependents — no reverse needed
    order
}

fn topo_dfs(
    node: &str,
    deps: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    order: &mut Vec<String>,
) {
    if visited.contains(node) {
        return;
    }
    visited.insert(node.to_string());
    if let Some(children) = deps.get(node) {
        for child in children {
            topo_dfs(child, deps, visited, order);
        }
    }
    order.push(node.to_string());
}
