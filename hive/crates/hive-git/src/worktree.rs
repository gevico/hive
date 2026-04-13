use std::path::{Path, PathBuf};
use std::process::Command;

use hive_core::{HiveError, HiveResult};

/// Branch name convention: hive/<task_id>
pub fn branch_name(task_id: &str) -> String {
    format!("hive/{task_id}")
}

/// Create a git worktree for a task.
pub fn create(
    repo_root: &Path,
    worktree_path: &Path,
    task_id: &str,
) -> HiveResult<String> {
    if worktree_path.exists() {
        return Err(HiveError::WorktreeExists(task_id.to_string()));
    }

    let branch = branch_name(task_id);

    // Get current HEAD as base commit
    let base_commit = get_head_sha(repo_root)?;

    // Create new branch from HEAD
    let output = Command::new("git")
        .args(["branch", &branch, "HEAD"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| HiveError::Git(format!("failed to create branch: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Branch might already exist from a previous attempt
        if !stderr.contains("already exists") {
            return Err(HiveError::Git(format!("git branch failed: {stderr}")));
        }
    }

    // Create worktree
    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            &branch,
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|e| HiveError::Git(format!("failed to create worktree: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HiveError::Worktree(format!(
            "git worktree add failed: {stderr}"
        )));
    }

    Ok(base_commit)
}

/// Remove a git worktree and its local branch.
pub fn remove(repo_root: &Path, worktree_path: &Path, task_id: &str) -> HiveResult<()> {
    // Remove worktree
    let output = Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            &worktree_path.to_string_lossy(),
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|e| HiveError::Git(format!("failed to remove worktree: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Worktree might already be gone
        if !stderr.contains("is not a working tree") {
            return Err(HiveError::Worktree(format!(
                "git worktree remove failed: {stderr}"
            )));
        }
    }

    // Delete local branch
    let branch = branch_name(task_id);
    let output = Command::new("git")
        .args(["branch", "-D", &branch])
        .current_dir(repo_root)
        .output()
        .map_err(|e| HiveError::Git(format!("failed to delete branch: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("not found") {
            eprintln!("warning: failed to delete branch {branch}: {stderr}");
        }
    }

    Ok(())
}

/// List active worktrees.
pub fn list(repo_root: &Path) -> HiveResult<Vec<WorktreeInfo>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| HiveError::Git(format!("failed to list worktrees: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HiveError::Git(format!("git worktree list failed: {stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = Vec::new();
    let mut path = None;
    let mut branch = None;

    for line in stdout.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            branch = Some(b.to_string());
        } else if line.is_empty() {
            if let (Some(p), Some(b)) = (path.take(), branch.take()) {
                worktrees.push(WorktreeInfo { path: p, branch: b });
            } else {
                path = None;
                branch = None;
            }
        }
    }
    // Handle last entry if no trailing newline
    if let (Some(p), Some(b)) = (path, branch) {
        worktrees.push(WorktreeInfo { path: p, branch: b });
    }

    Ok(worktrees)
}

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
}

pub fn get_head_sha(repo_root: &Path) -> HiveResult<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| HiveError::Git(format!("failed to get HEAD: {e}")))?;

    if !output.status.success() {
        return Err(HiveError::Git("failed to get HEAD SHA".into()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .output()
        .is_ok_and(|o| o.status.success())
}
