use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow, bail};
use hive_core::lock::{FileLock, OrchestratorLock};
use hive_core::state::{TaskState, TransitionAction};
use hive_core::storage::{self, HivePaths};
use hive_core::task::ApprovalStatus;

use crate::commands::{check, claim, isolate, launch, report, runtime};

pub fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let _orchestrator_lock = OrchestratorLock::acquire(&paths.orchestrator_lock())?;
    run_with_paths(&cwd, &paths)
}

fn run_with_paths(repo_root: &std::path::Path, paths: &HivePaths) -> Result<()> {
    let states = storage::load_all_states(paths)?;
    let approved: Vec<_> = states
        .iter()
        .filter(|state| state.approval_status == ApprovalStatus::Approved)
        .collect();

    if approved.is_empty() {
        bail!("no approved tasks to execute");
    }

    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    for state in &approved {
        let spec_path = paths.spec_file(&state.task_id);
        let dep_list = if let Ok(content) = std::fs::read_to_string(&spec_path) {
            hive_core::task::parse_spec(&content)
                .map(|spec| spec.depends_on)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        deps.insert(state.task_id.clone(), dep_list);
    }

    if has_cycle(&deps) {
        bail!("circular dependency detected in task graph");
    }

    for state in &approved {
        let plan_path = paths.plan_file(&state.draft_id, &state.task_id);
        if !plan_path.exists() {
            eprintln!(
                "warning: plan not found for task {}, skipping",
                state.task_id
            );
        }
    }

    let order = topological_sort(&deps);
    let approved_ids: HashSet<String> =
        approved.iter().map(|state| state.task_id.clone()).collect();

    for task_id in &order {
        if !approved_ids.contains(task_id) {
            continue;
        }

        let state = match storage::read_task_state(paths, task_id) {
            Ok(state) => state,
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
        match execute_task(repo_root, paths, task_id)? {
            TaskExecutionResult::Completed => {
                println!("  task {task_id}: completed successfully");
            }
            TaskExecutionResult::Deferred => {
                println!("  task {task_id}: deferred");
            }
        }
    }

    let remaining: Vec<_> = approved_ids
        .iter()
        .filter_map(|task_id| {
            storage::read_task_state(paths, task_id)
                .ok()
                .filter(|state| {
                    state.state != TaskState::Completed && state.state != TaskState::Blocked
                })
                .map(|state| format!("{}={}", task_id, state.state))
        })
        .collect();
    if !remaining.is_empty() {
        bail!(
            "execution stalled with non-terminal tasks remaining: {}",
            remaining.join(", ")
        );
    }

    storage::regenerate_state_md(paths)?;
    println!("execution complete");
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskExecutionResult {
    Completed,
    Deferred,
}

fn execute_task(
    repo_root: &std::path::Path,
    paths: &HivePaths,
    task_id: &str,
) -> Result<TaskExecutionResult> {
    loop {
        let state = storage::read_task_state(paths, task_id)?;
        match state.state {
            TaskState::Pending => {
                if let Err(err) = claim::claim_task(paths, task_id) {
                    if let Some(hive_core::HiveError::DependencyNotMet(_)) =
                        err.downcast_ref::<hive_core::HiveError>()
                    {
                        return Ok(TaskExecutionResult::Deferred);
                    }
                    handle_task_failure(repo_root, paths, task_id, &err.to_string())?;
                    continue;
                }
            }
            TaskState::Assigned => {
                let worktree_path = paths.worktree_path(task_id);
                if !worktree_path.exists() {
                    if let Err(err) = isolate::isolate_task(repo_root, paths, task_id) {
                        handle_task_failure(repo_root, paths, task_id, &err.to_string())?;
                        continue;
                    }
                    continue;
                }

                if let Err(err) = launch::launch_task_unlocked(paths, task_id) {
                    handle_task_failure(repo_root, paths, task_id, &err.to_string())?;
                    continue;
                }
            }
            TaskState::InProgress => match report::report_task_unlocked(paths, task_id) {
                Ok(report::ReportOutcome::Review) => {}
                Ok(report::ReportOutcome::Failed) => {
                    handle_task_failure(repo_root, paths, task_id, "worker reported failure")?;
                    continue;
                }
                Err(err) => {
                    handle_task_failure(repo_root, paths, task_id, &err.to_string())?;
                    continue;
                }
            },
            TaskState::Review => match check::check_task(paths, task_id)? {
                check::EXIT_ALL_PASS => {
                    complete_task(paths, task_id)?;
                    return Ok(TaskExecutionResult::Completed);
                }
                check::EXIT_SOME_FAIL => {
                    handle_task_failure(repo_root, paths, task_id, "acceptance criteria failed")?;
                    continue;
                }
                check::EXIT_SPEC_NOT_FOUND => {
                    handle_task_failure(repo_root, paths, task_id, "spec not found")?;
                    continue;
                }
                check::EXIT_WRONG_STATE => return Ok(TaskExecutionResult::Deferred),
                code => return Err(anyhow!("unexpected hive check exit code {code}")),
            },
            TaskState::Completed => return Ok(TaskExecutionResult::Completed),
            TaskState::Failed => {
                handle_task_failure(repo_root, paths, task_id, "resuming from failed state")?;
                continue;
            }
            TaskState::Blocked => return Ok(TaskExecutionResult::Deferred),
        }
    }
}

fn complete_task(paths: &HivePaths, task_id: &str) -> Result<()> {
    let _lock = FileLock::try_acquire(&paths.lock_file(task_id)).ok();
    let mut state = storage::read_task_state(paths, task_id)?;
    if state.state != TaskState::Review {
        bail!(
            "task {} is in state '{}', must be 'review' to complete",
            task_id,
            state.state
        );
    }

    let from_state = state.state.to_string();
    state.state = state
        .state
        .transition(TransitionAction::Complete, 0, true)?;
    state.touch();
    storage::write_task_state(paths, &state)?;
    runtime::log_state_change(paths, task_id, &from_state, &state.state.to_string())?;
    Ok(())
}

fn handle_task_failure(
    repo_root: &std::path::Path,
    paths: &HivePaths,
    task_id: &str,
    reason: &str,
) -> Result<()> {
    eprintln!("  task {task_id}: failed: {reason}");

    let _lock = FileLock::try_acquire(&paths.lock_file(task_id)).ok();
    let mut state = storage::read_task_state(paths, task_id)?;
    if matches!(state.state, TaskState::Completed | TaskState::Blocked) {
        return Ok(());
    }

    if state.state != TaskState::Failed {
        let previous_state = state.state.to_string();
        state.state = match state
            .state
            .transition(TransitionAction::Fail, state.retry_count, true)
        {
            Ok(next) => next,
            Err(_) => TaskState::Failed,
        };
        state.retry_count += 1;
        state.touch();
        storage::write_task_state(paths, &state)?;
        runtime::log_state_change(paths, task_id, &previous_state, &state.state.to_string())?;
    } else {
        state.retry_count += 1;
        state.touch();
        storage::write_task_state(paths, &state)?;
    }

    let previous_state = state.state.to_string();
    state.state = state.state.auto_retry_or_block(state.retry_count)?;
    if state.state == TaskState::Pending {
        let worktree_path = paths.worktree_path(task_id);
        if worktree_path.exists() {
            let _ = hive_git::worktree::remove(repo_root, &worktree_path, task_id);
        }
        state.base_commit = None;
    }
    state.touch();
    storage::write_task_state(paths, &state)?;
    runtime::log_state_change(paths, task_id, &previous_state, &state.state.to_string())?;

    if state.state == TaskState::Blocked {
        eprintln!("  task {task_id}: blocked (retry limit exceeded)");
    } else {
        eprintln!("  task {task_id}: scheduled for retry");
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    use std::process::Command;

    use hive_core::storage::TaskStateFile;
    use tempfile::TempDir;

    fn init_repo() -> (TempDir, std::path::PathBuf, HivePaths) {
        let tmp = TempDir::new().unwrap();
        let repo_root = tmp.path().to_path_buf();

        Command::new("git")
            .args(["init"])
            .current_dir(&repo_root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Hive Test"])
            .current_dir(&repo_root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "hive-test@example.com"])
            .current_dir(&repo_root)
            .output()
            .unwrap();
        std::fs::write(repo_root.join("README.md"), "seed\n").unwrap();
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&repo_root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(&repo_root)
            .output()
            .unwrap();

        let paths = HivePaths::new(&repo_root);
        for dir in paths.required_dirs() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(
            paths.config_yml(),
            "launch:\n  tool: custom\n  custom_command: \"printf '%s\\n' '---' 'id: {task_id}' 'status: completed' 'branch: hive/{task_id}' 'commit: deadbeef' 'base_commit: cafebabe' 'schema_version: 1' '---' 'done' > result.md\"\n",
        )
        .unwrap();

        (tmp, repo_root, paths)
    }

    fn write_task_fixture(paths: &HivePaths, task_id: &str, draft_id: &str, spec_body: &str) {
        let spec = format!(
            "---\nid: {task_id}\ndraft_id: {draft_id}\ndepends_on: []\ncomplexity: S\napproval_status: approved\nschema_version: 1\n---\n{spec_body}\n"
        );
        std::fs::write(paths.spec_file(task_id), spec).unwrap();

        let plan_dir = paths.plans_dir().join(draft_id);
        std::fs::create_dir_all(&plan_dir).unwrap();
        std::fs::write(paths.plan_file(draft_id, task_id), "# plan\n").unwrap();

        let mut state = TaskStateFile::new(task_id.into(), draft_id.into(), "hash1234".into());
        state.approval_status = ApprovalStatus::Approved;
        storage::write_task_state(paths, &state).unwrap();
    }

    #[test]
    fn topological_sort_keeps_dependencies_first() {
        let mut deps = HashMap::new();
        deps.insert("task-b".to_string(), vec!["task-a".to_string()]);
        deps.insert("task-a".to_string(), Vec::new());

        let order = topological_sort(&deps);
        let a_pos = order.iter().position(|task| task == "task-a").unwrap();
        let b_pos = order.iter().position(|task| task == "task-b").unwrap();

        assert!(a_pos < b_pos);
    }

    #[test]
    fn exec_completes_only_after_check_passes() {
        let (_tmp, repo_root, paths) = init_repo();
        write_task_fixture(&paths, "task-01", "draft-01", "verify-file: result.md");

        run_with_paths(&repo_root, &paths).unwrap();

        let state = storage::read_task_state(&paths, "task-01").unwrap();
        assert_eq!(state.state, TaskState::Completed);
        assert_eq!(state.retry_count, 0);
    }

    #[test]
    fn exec_retries_failed_verification_until_blocked() {
        let (_tmp, repo_root, paths) = init_repo();
        write_task_fixture(&paths, "task-02", "draft-02", "verify-command: false");

        run_with_paths(&repo_root, &paths).unwrap();

        let state = storage::read_task_state(&paths, "task-02").unwrap();
        assert_eq!(state.state, TaskState::Blocked);
        assert_eq!(state.retry_count, 3);
    }
}
