use std::process::Command;

use anyhow::{Result, bail};
use hive_core::storage::{self, HivePaths};

// Exit codes per AC-10
pub(crate) const EXIT_ALL_PASS: i32 = 0;
pub(crate) const EXIT_SOME_FAIL: i32 = 1;
pub(crate) const EXIT_SPEC_NOT_FOUND: i32 = 2;
pub(crate) const EXIT_WRONG_STATE: i32 = 3;

pub fn run(task_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    let exit_code = check_task(&paths, &task_id)?;
    std::process::exit(exit_code);
}

pub(crate) fn check_task(paths: &HivePaths, task_id: &str) -> Result<i32> {
    let state = match storage::read_task_state(paths, task_id) {
        Ok(state) => state,
        Err(hive_core::HiveError::TaskNotFound(_)) => {
            eprintln!("error: task not found: {task_id}");
            return Ok(EXIT_SPEC_NOT_FOUND);
        }
        Err(err) => return Err(err.into()),
    };

    if state.state != hive_core::TaskState::Review {
        eprintln!(
            "error: task {} is in state '{}', must be 'review' to check",
            task_id, state.state
        );
        return Ok(EXIT_WRONG_STATE);
    }

    let spec_path = paths.spec_file(task_id);
    let spec_content = match std::fs::read_to_string(&spec_path) {
        Ok(content) => content,
        Err(_) => {
            eprintln!("error: spec not found for task {task_id}");
            return Ok(EXIT_SPEC_NOT_FOUND);
        }
    };

    let spec = hive_core::task::parse_spec(&spec_content)?;
    let wt_path = paths.worktree_path(task_id);

    let criteria = parse_criteria(&spec.body);
    if criteria.is_empty() {
        println!("task {task_id}: no verifiable criteria found in spec");
        return Ok(EXIT_ALL_PASS);
    }

    let mut all_pass = true;
    let mut results_log = String::from("# Verification Results\n\n");
    for criterion in &criteria {
        let result = match criterion {
            Criterion::Command(cmd) => verify_command(&wt_path, cmd),
            Criterion::File { path, pattern } => verify_file(&wt_path, path, pattern.as_deref()),
            Criterion::Manual(desc) => verify_manual(desc),
        };

        match result {
            Ok(true) => {
                println!("  PASS: {criterion}");
                results_log.push_str(&format!("- PASS: {criterion}\n"));
            }
            Ok(false) => {
                println!("  FAIL: {criterion}");
                results_log.push_str(&format!("- FAIL: {criterion}\n"));
                all_pass = false;
            }
            Err(e) => {
                println!("  FAIL: {criterion} ({e})");
                results_log.push_str(&format!("- FAIL: {criterion} ({e})\n"));
                all_pass = false;
            }
        }
    }

    // Record results to task directory
    let task_dir = paths.task_dir(task_id);
    if task_dir.exists() {
        let _ = std::fs::write(task_dir.join("check-results.md"), &results_log);
    }

    if all_pass {
        println!("task {task_id}: all criteria passed");
        Ok(EXIT_ALL_PASS)
    } else {
        println!("task {task_id}: some criteria failed");
        Ok(EXIT_SOME_FAIL)
    }
}

#[derive(Debug)]
enum Criterion {
    Command(String),
    File {
        path: String,
        pattern: Option<String>,
    },
    Manual(String),
}

impl std::fmt::Display for Criterion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(cmd) => write!(f, "[command] {cmd}"),
            Self::File { path, pattern } => {
                if let Some(p) = pattern {
                    write!(f, "[file] {path} matches '{p}'")
                } else {
                    write!(f, "[file] {path} exists")
                }
            }
            Self::Manual(desc) => write!(f, "[manual] {desc}"),
        }
    }
}

fn parse_criteria(body: &str) -> Vec<Criterion> {
    let mut criteria = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if let Some(cmd) = line.strip_prefix("verify-command:") {
            criteria.push(Criterion::Command(cmd.trim().to_string()));
        } else if let Some(rest) = line.strip_prefix("verify-file:") {
            let parts: Vec<&str> = rest.trim().splitn(2, ' ').collect();
            criteria.push(Criterion::File {
                path: parts[0].to_string(),
                pattern: parts.get(1).map(|s| s.to_string()),
            });
        } else if let Some(desc) = line.strip_prefix("verify-manual:") {
            criteria.push(Criterion::Manual(desc.trim().to_string()));
        }
    }
    criteria
}

fn verify_command(worktree: &std::path::Path, cmd: &str) -> Result<bool> {
    let output = Command::new("sh")
        .args(["-c", cmd])
        .current_dir(worktree)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("    stderr: {stderr}");
    }
    Ok(output.status.success())
}

fn verify_file(worktree: &std::path::Path, path: &str, pattern: Option<&str>) -> Result<bool> {
    let file_path = worktree.join(path);
    if !file_path.exists() {
        return Ok(false);
    }
    if let Some(pattern) = pattern {
        let content = std::fs::read_to_string(&file_path)?;
        Ok(content.contains(pattern))
    } else {
        Ok(true)
    }
}

fn verify_manual(desc: &str) -> Result<bool> {
    eprint!("manual verification: {desc} [y/n]? ");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_lowercase().starts_with('y'))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[test]
    fn missing_task_returns_exit_code_2() {
        let tmp = TempDir::new().unwrap();
        let paths = HivePaths::new(tmp.path());
        std::fs::create_dir_all(paths.hive_dir()).unwrap();

        let exit_code = check_task(&paths, "missing-task").unwrap();
        assert_eq!(exit_code, EXIT_SPEC_NOT_FOUND);
    }
}
