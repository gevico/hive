use anyhow::{Result, bail};
use hive_core::lock::FileLock;
use hive_core::state::TransitionAction;
use hive_core::storage::{self, HivePaths};
use hive_core::task::{TaskResultStatus, parse_result};

use crate::commands::runtime::{self, CommandFailure};

// Exit codes per AC-11
const EXIT_SUCCESS: i32 = 0;
const EXIT_RESULT_INVALID: i32 = 1;
const EXIT_WRONG_STATE: i32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReportOutcome {
    Review,
    Failed,
}

pub fn run(task_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    match report_task(&paths, &task_id) {
        Ok(outcome) => {
            let state = match outcome {
                ReportOutcome::Review => "review",
                ReportOutcome::Failed => "failed",
            };
            println!("task {task_id}: reported successfully ({state})");
            std::process::exit(EXIT_SUCCESS);
        }
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(err.exit_code);
        }
    }
}

pub(crate) fn report_task(
    paths: &HivePaths,
    task_id: &str,
) -> std::result::Result<ReportOutcome, CommandFailure> {
    report_task_inner(paths, task_id, true)
}

pub(crate) fn report_task_unlocked(
    paths: &HivePaths,
    task_id: &str,
) -> std::result::Result<ReportOutcome, CommandFailure> {
    report_task_inner(paths, task_id, false)
}

fn report_task_inner(
    paths: &HivePaths,
    task_id: &str,
    acquire_lock: bool,
) -> std::result::Result<ReportOutcome, CommandFailure> {
    let _lock = if acquire_lock {
        Some(
            FileLock::try_acquire(&paths.lock_file(task_id))
                .map_err(|e| CommandFailure::new(EXIT_RESULT_INVALID, e.to_string()))?,
        )
    } else {
        None
    };
    let mut state = storage::read_task_state(paths, task_id)
        .map_err(|e| CommandFailure::new(EXIT_WRONG_STATE, e.to_string()))?;

    if state.state != hive_core::TaskState::InProgress {
        return Err(CommandFailure::new(
            EXIT_WRONG_STATE,
            format!(
                "task {} is in state '{}', must be 'in_progress' to report",
                task_id, state.state
            ),
        ));
    }

    let wt_path = paths.worktree_path(task_id);
    let result_path = wt_path.join("result.md");
    let result_content = std::fs::read_to_string(&result_path).map_err(|_| {
        CommandFailure::new(
            EXIT_RESULT_INVALID,
            format!("result.md not found at {}", result_path.display()),
        )
    })?;

    let result = parse_result(&result_content)
        .map_err(|e| CommandFailure::new(EXIT_RESULT_INVALID, e.to_string()))?;
    if result.id != task_id {
        return Err(CommandFailure::new(
            EXIT_RESULT_INVALID,
            format!(
                "result.md id '{}' does not match task '{}'",
                result.id, task_id
            ),
        ));
    }

    let previous_state = state.state.to_string();
    let action = match result.status {
        TaskResultStatus::Completed => TransitionAction::SubmitForReview,
        TaskResultStatus::Failed => TransitionAction::Fail,
    };

    state.state = state
        .state
        .transition(action, state.retry_count, true)
        .map_err(|e| CommandFailure::new(EXIT_RESULT_INVALID, e.to_string()))?;
    state.touch();
    storage::write_task_state(paths, &state)
        .map_err(|e| CommandFailure::new(EXIT_RESULT_INVALID, e.to_string()))?;
    storage::regenerate_state_md(paths)
        .map_err(|e| CommandFailure::new(EXIT_RESULT_INVALID, e.to_string()))?;
    runtime::log_state_change(paths, task_id, &previous_state, &state.state.to_string())
        .map_err(|e| CommandFailure::new(EXIT_RESULT_INVALID, e.to_string()))?;

    // Log structured result outcome — error propagated per AC-14
    if let Ok(config) = hive_core::config::load_config(&paths.hive_dir()) {
        hive_audit::log_round_summary(
            &paths.audit_file(task_id),
            config.audit_level,
            task_id,
            0,
            &format!(
                "report outcome: {} (branch: {}, commit: {})",
                result.status, result.branch, result.commit
            ),
        )
        .map_err(|e| CommandFailure::new(EXIT_RESULT_INVALID, format!("audit error: {e}")))?;
    }

    let outcome = match result.status {
        TaskResultStatus::Completed => ReportOutcome::Review,
        TaskResultStatus::Failed => ReportOutcome::Failed,
    };

    Ok(outcome)
}
