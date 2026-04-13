use std::process::Command;

use anyhow::{Result, bail};
use hive_core::config;
use hive_core::lock::FileLock;
use hive_core::state::TransitionAction;
use hive_core::storage::{self, HivePaths};
use hive_git::merge::check_tool_available;

use crate::commands::runtime;

pub fn run(task_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    launch_task(&paths, &task_id)?;
    Ok(())
}

pub(crate) fn launch_task(paths: &HivePaths, task_id: &str) -> Result<()> {
    launch_task_inner(paths, task_id, true)
}

pub(crate) fn launch_task_unlocked(paths: &HivePaths, task_id: &str) -> Result<()> {
    launch_task_inner(paths, task_id, false)
}

fn launch_task_inner(paths: &HivePaths, task_id: &str, acquire_lock: bool) -> Result<()> {
    let _lock = if acquire_lock {
        Some(FileLock::try_acquire(&paths.lock_file(task_id))?)
    } else {
        None
    };
    let mut state = storage::read_task_state(paths, task_id)?;

    if state.state != hive_core::TaskState::Assigned {
        bail!(
            "task {} is in state '{}', must be 'assigned' to launch",
            task_id,
            state.state
        );
    }

    let wt_path = paths.worktree_path(task_id);
    if !wt_path.exists() {
        bail!(
            "worktree not found at {}. Run `hive isolate` first",
            wt_path.display()
        );
    }

    let hive_config = config::load_config(&paths.hive_dir())?;
    let tool = &hive_config.launch.tool;
    let plan_path = paths.plan_file(&state.draft_id, task_id);
    let plan_content = std::fs::read_to_string(&plan_path).ok();

    let custom_command = match tool.as_str() {
        "claude" => {
            check_tool_available("claude")?;
            None
        }
        "codex" => {
            check_tool_available("codex")?;
            None
        }
        "custom" => Some(
            hive_config
                .launch
                .custom_command
                .as_ref()
                .ok_or_else(|| {
                    anyhow::anyhow!("custom launch requires launch.custom_command in config")
                })?
                .clone(),
        ),
        _ => bail!("unsupported launch tool: {tool}"),
    };

    let from_state = state.state.to_string();
    state.state = state.state.transition(TransitionAction::Start, 0, true)?;
    state.touch();
    storage::write_task_state(paths, &state)?;
    runtime::log_state_change(paths, task_id, &from_state, &state.state.to_string())?;

    match tool.as_str() {
        "claude" => launch_claude(&wt_path, task_id, plan_content.as_deref())?,
        "codex" => launch_codex(&wt_path, task_id, plan_content.as_deref())?,
        "custom" => {
            let cmd = custom_command.as_deref().ok_or_else(|| {
                anyhow::anyhow!("custom launch requires launch.custom_command in config")
            })?;
            launch_custom(&wt_path, task_id, cmd)?;
        }
        _ => unreachable!("tool validity was checked before state transition"),
    }

    Ok(())
}

fn launch_claude(worktree: &std::path::Path, task_id: &str, plan: Option<&str>) -> Result<()> {
    let mut prompt = format!("Execute task {task_id}.");
    if let Some(plan) = plan {
        prompt.push_str(&format!("\n\nPlan:\n{plan}"));
    }

    let status = Command::new("claude")
        .args(["--print", &prompt])
        .current_dir(worktree)
        .status()?;

    if !status.success() {
        eprintln!("warning: claude exited with status {status}");
    }

    ensure_result_file(worktree, task_id)
}

fn launch_codex(worktree: &std::path::Path, task_id: &str, plan: Option<&str>) -> Result<()> {
    let mut prompt = format!("Execute task {task_id}.");
    if let Some(plan) = plan {
        prompt.push_str(&format!("\n\nPlan:\n{plan}"));
    }

    let status = Command::new("codex")
        .args(["exec", "--approval-mode", "full-auto", &prompt])
        .current_dir(worktree)
        .status()?;

    if !status.success() {
        eprintln!("warning: codex exited with status {status}");
    }

    ensure_result_file(worktree, task_id)
}

fn launch_custom(worktree: &std::path::Path, task_id: &str, cmd_template: &str) -> Result<()> {
    let cmd = cmd_template
        .replace("{task_id}", task_id)
        .replace("{worktree_path}", &worktree.to_string_lossy());

    let status = Command::new("sh")
        .args(["-c", &cmd])
        .current_dir(worktree)
        .status()?;

    if !status.success() {
        eprintln!("warning: custom command exited with status {status}");
    }

    ensure_result_file(worktree, task_id)
}

fn ensure_result_file(worktree: &std::path::Path, task_id: &str) -> Result<()> {
    let result_path = worktree.join("result.md");
    if !result_path.exists() {
        bail!(
            "task {task_id}: agent exited but result.md not found at {}",
            result_path.display()
        );
    }

    println!("task {task_id}: agent completed, result.md found");
    Ok(())
}
