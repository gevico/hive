use std::path::Path;
use std::process::Command;

use hive_core::{HiveError, HiveResult};

/// Platform for PR/MR creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Platform {
    Github,
    Gitlab,
    None,
}

impl Platform {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "github" => Self::Github,
            "gitlab" => Self::Gitlab,
            _ => Self::None,
        }
    }
}

/// Create a PR/MR for the given branch.
pub fn create_pr(
    repo_root: &Path,
    platform: &Platform,
    branch: &str,
    title: &str,
    body: &str,
    labels: &[&str],
) -> HiveResult<Option<String>> {
    match platform {
        Platform::Github => create_github_pr(repo_root, branch, title, body, labels),
        Platform::Gitlab => {
            eprintln!("warning: GitLab MR creation not yet implemented");
            Ok(None)
        }
        Platform::None => {
            println!("Branch ready for review: {branch}");
            Ok(None)
        }
    }
}

fn create_github_pr(
    repo_root: &Path,
    branch: &str,
    title: &str,
    body: &str,
    labels: &[&str],
) -> HiveResult<Option<String>> {
    check_tool_available("gh")?;

    let mut args = vec![
        "pr",
        "create",
        "--head",
        branch,
        "--title",
        title,
        "--body",
        body,
    ];

    for label in labels {
        args.push("--label");
        args.push(label);
    }

    let output = Command::new("gh")
        .args(&args)
        .current_dir(repo_root)
        .output()
        .map_err(|e| HiveError::Git(format!("failed to run gh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HiveError::Git(format!("gh pr create failed: {stderr}")));
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(Some(url))
}

/// Check if a CLI tool is available in PATH.
pub fn check_tool_available(tool: &str) -> HiveResult<()> {
    let result = Command::new("which")
        .arg(tool)
        .output();

    match result {
        Ok(o) if o.status.success() => Ok(()),
        _ => Err(HiveError::AgentToolNotFound(format!(
            "'{tool}' not found in PATH"
        ))),
    }
}
