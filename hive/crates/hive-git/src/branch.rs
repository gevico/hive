use std::path::Path;
use std::process::Command;

use hive_core::{HiveError, HiveResult};

/// Rebase a branch onto target (typically main).
pub fn rebase(repo_root: &Path, branch: &str, onto: &str) -> HiveResult<()> {
    let output = Command::new("git")
        .args(["rebase", onto, branch])
        .current_dir(repo_root)
        .output()
        .map_err(|e| HiveError::Git(format!("failed to rebase: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Abort rebase on conflict
        let _ = Command::new("git")
            .args(["rebase", "--abort"])
            .current_dir(repo_root)
            .output();
        return Err(HiveError::MergeConflict(format!(
            "rebase {branch} onto {onto} failed: {stderr}"
        )));
    }

    Ok(())
}

/// Get the default branch name (main or master).
pub fn default_branch(repo_root: &Path) -> HiveResult<String> {
    // Try remote default
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(repo_root)
        .output();

    if let Ok(o) = output
        && o.status.success() {
            let refname = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if let Some(branch) = refname.strip_prefix("refs/remotes/origin/") {
                return Ok(branch.to_string());
            }
        }

    // Fall back to local main/master
    for name in ["main", "master"] {
        let output = Command::new("git")
            .args(["show-ref", "--verify", &format!("refs/heads/{name}")])
            .current_dir(repo_root)
            .output();
        if let Ok(o) = output
            && o.status.success() {
                return Ok(name.to_string());
            }
    }

    Err(HiveError::Git("cannot determine default branch".into()))
}

/// Merge a branch into current HEAD using fast-forward or merge commit.
pub fn merge_branch(repo_root: &Path, branch: &str) -> HiveResult<()> {
    let output = Command::new("git")
        .args(["merge", "--ff-only", branch])
        .current_dir(repo_root)
        .output()
        .map_err(|e| HiveError::Git(format!("failed to merge: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HiveError::MergeConflict(format!(
            "merge {branch} failed: {stderr}"
        )));
    }

    Ok(())
}
