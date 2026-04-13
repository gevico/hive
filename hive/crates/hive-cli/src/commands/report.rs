use anyhow::{Result, bail};
use hive_core::frontmatter;
use hive_core::lock::FileLock;
use hive_core::state::TransitionAction;
use hive_core::storage::{self, HivePaths};

// Exit codes per AC-11
const EXIT_SUCCESS: i32 = 0;
const EXIT_RESULT_INVALID: i32 = 1;
const EXIT_WRONG_STATE: i32 = 2;

pub fn run(task_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let _lock = FileLock::try_acquire(&paths.lock_file(&task_id))?;
    let mut state = storage::read_task_state(&paths, &task_id)?;

    if state.state != hive_core::TaskState::InProgress {
        eprintln!(
            "error: task {} is in state '{}', must be 'in_progress' to report",
            task_id, state.state
        );
        std::process::exit(EXIT_WRONG_STATE);
    }

    let wt_path = paths.worktree_path(&task_id);
    let result_path = wt_path.join("result.md");
    let result_content = match std::fs::read_to_string(&result_path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("error: result.md not found at {}", result_path.display());
            std::process::exit(EXIT_RESULT_INVALID);
        }
    };

    let fm = match frontmatter::parse(&result_content) {
        Ok(fm) => fm,
        Err(e) => {
            eprintln!("error: malformed result.md frontmatter: {e}");
            std::process::exit(EXIT_RESULT_INVALID);
        }
    };

    let status = fm.get_str("status").unwrap_or("unknown");

    let action = match status {
        "completed" => TransitionAction::SubmitForReview,
        "failed" => TransitionAction::Fail,
        _ => {
            eprintln!("error: unknown result status: {status}");
            std::process::exit(EXIT_RESULT_INVALID);
        }
    };

    let new_state = state
        .state
        .transition(action, state.retry_count, true)?;

    state.state = new_state;
    state.touch();
    storage::write_task_state(&paths, &state)?;
    storage::regenerate_state_md(&paths)?;

    println!(
        "task {task_id}: reported as {status} (in_progress -> {})",
        state.state
    );
    std::process::exit(EXIT_SUCCESS);
}
