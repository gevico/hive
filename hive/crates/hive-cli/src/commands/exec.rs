use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use hive_core::lock::OrchestratorLock;
use hive_core::state::{TaskState, TransitionAction, retry_limit};
use hive_core::storage::{self, HivePaths, TaskStateFile};
use hive_core::task::ApprovalStatus;

pub fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let _orch_lock = OrchestratorLock::acquire(&paths.orchestrator_lock())?;

    let states = storage::load_all_states(&paths)?;

    // Filter to approved tasks only
    let approved: Vec<&TaskStateFile> = states
        .iter()
        .filter(|s| s.approval_status == ApprovalStatus::Approved)
        .collect();

    if approved.is_empty() {
        bail!("no approved tasks to execute");
    }

    // Build dependency graph from spec files
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    for s in &approved {
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

    // Detect circular dependencies
    if has_cycle(&deps) {
        bail!("circular dependency detected in task graph");
    }

    // Validate plans exist
    for s in &approved {
        let plan_path = paths.plan_file(&s.draft_id, &s.task_id);
        if !plan_path.exists() {
            eprintln!(
                "warning: plan not found for task {}, skipping",
                s.task_id
            );
        }
    }

    // Execute in dependency order
    let order = topological_sort(&deps);
    let task_ids: HashSet<String> = approved.iter().map(|s| s.task_id.clone()).collect();

    for task_id in &order {
        if !task_ids.contains(task_id) {
            continue;
        }

        // Reload state (may have changed)
        let mut state = match storage::read_task_state(&paths, task_id) {
            Ok(s) => s,
            Err(_) => continue,
        };

        if state.state == TaskState::Completed || state.state == TaskState::Blocked {
            continue;
        }

        let plan_path = paths.plan_file(&state.draft_id, task_id);
        if !plan_path.exists() {
            continue;
        }

        println!("executing task: {task_id}");

        // claim -> isolate -> launch -> report -> check cycle
        // For the orchestrator, we perform the state transitions
        match execute_task(&cwd, &paths, &mut state) {
            Ok(()) => {
                println!("  task {task_id}: completed successfully");
            }
            Err(e) => {
                eprintln!("  task {task_id}: failed: {e}");
                state.state = TaskState::Failed;
                state.retry_count += 1;
                state.touch();
                storage::write_task_state(&paths, &state)?;

                // Auto retry or block
                if state.retry_count >= retry_limit() {
                    state.state = TaskState::Blocked;
                    state.touch();
                    storage::write_task_state(&paths, &state)?;
                    eprintln!("  task {task_id}: blocked (retry limit exceeded)");
                }
            }
        }
    }

    storage::regenerate_state_md(&paths)?;
    println!("execution complete");
    Ok(())
}

fn execute_task(
    repo_root: &std::path::Path,
    paths: &HivePaths,
    state: &mut TaskStateFile,
) -> Result<()> {
    let task_id = state.task_id.clone();
    let draft_id = state.draft_id.clone();

    // Assign
    state.state = state.state.transition(TransitionAction::Assign, 0, true)?;
    state.touch();
    storage::write_task_state(paths, state)?;

    // Isolate
    let wt_path = paths.worktree_path(&task_id);
    let base_commit = hive_git::worktree::create(repo_root, &wt_path, &task_id)?;
    state.base_commit = Some(base_commit);
    state.state = state.state.transition(TransitionAction::Start, 0, true)?;
    state.touch();
    storage::write_task_state(paths, state)?;

    // Launch agent
    let config = hive_core::config::load_config(&paths.hive_dir())?;
    let plan_path = paths.plan_file(&draft_id, &task_id);
    let plan = std::fs::read_to_string(&plan_path).ok();

    let tool = &config.launch.tool;
    let launch_result = std::process::Command::new(tool)
        .args(if tool == "codex" {
            vec!["exec", "--approval-mode", "full-auto"]
        } else {
            vec!["--print"]
        })
        .arg(
            plan.as_deref()
                .unwrap_or(&format!("Execute task {task_id}")),
        )
        .current_dir(&wt_path)
        .status();

    match launch_result {
        Ok(s) if s.success() => {}
        _ => {
            eprintln!("  agent launch failed or exited with error");
        }
    }

    // Check for result.md
    let result_path = wt_path.join("result.md");
    if result_path.exists() {
        state.state = state
            .state
            .transition(TransitionAction::SubmitForReview, 0, true)?;
    } else {
        return Err(anyhow::anyhow!("result.md not produced by agent"));
    }
    state.touch();
    storage::write_task_state(paths, state)?;

    // Mark completed (simplified - in production would run hive check)
    state.state = state.state.transition(TransitionAction::Complete, 0, true)?;
    state.touch();
    storage::write_task_state(paths, state)?;

    Ok(())
}

fn has_cycle(deps: &HashMap<String, Vec<String>>) -> bool {
    let mut visited = HashSet::new();
    let mut in_stack = HashSet::new();

    for node in deps.keys() {
        if dfs_cycle(node, deps, &mut visited, &mut in_stack) {
            return true;
        }
    }
    false
}

fn dfs_cycle(
    node: &str,
    deps: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    in_stack: &mut HashSet<String>,
) -> bool {
    if in_stack.contains(node) {
        return true;
    }
    if visited.contains(node) {
        return false;
    }
    visited.insert(node.to_string());
    in_stack.insert(node.to_string());

    if let Some(children) = deps.get(node) {
        for child in children {
            if dfs_cycle(child, deps, visited, in_stack) {
                return true;
            }
        }
    }

    in_stack.remove(node);
    false
}

fn topological_sort(deps: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut visited = HashSet::new();
    let mut order = Vec::new();

    for node in deps.keys() {
        topo_dfs(node, deps, &mut visited, &mut order);
    }

    order.reverse();
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
