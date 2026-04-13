use anyhow::{Result, bail};
use hive_core::config;
use hive_core::storage::{self, HivePaths};
use hive_core::task::ApprovalStatus;
use hive_git::merge;

pub fn run(draft_id: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project. Run `hive init` first");
    }

    // Find all specs for this draft
    let states = storage::load_all_states(&paths)?;
    let draft_tasks: Vec<_> = states.iter().filter(|s| s.draft_id == draft_id).collect();

    if draft_tasks.is_empty() {
        bail!("no specs found for draft {draft_id}");
    }

    // Check that no tasks are already in rfc or approved state
    for s in &draft_tasks {
        if s.approval_status == ApprovalStatus::Rfc || s.approval_status == ApprovalStatus::Approved
        {
            bail!("draft {draft_id} already in '{}' state", s.approval_status);
        }
    }

    // Validate plans exist for all specs
    for s in &draft_tasks {
        let plan_path = paths.plan_file(&draft_id, &s.task_id);
        if !plan_path.exists() {
            bail!("plan not found for task {}", s.task_id);
        }
    }

    // Generate RFC document
    let mut rfc_content = format!("# RFC: {draft_id}\n\n");
    rfc_content.push_str("## Overview\n\n");
    rfc_content.push_str(&format!(
        "This RFC covers {} tasks for draft `{draft_id}`.\n\n",
        draft_tasks.len()
    ));

    // Dependency graph (text)
    rfc_content.push_str("## Dependency Graph\n\n");
    for s in &draft_tasks {
        let spec_path = paths.spec_file(&s.task_id);
        if let Ok(content) = std::fs::read_to_string(&spec_path)
            && let Ok(spec) = hive_core::task::parse_spec(&content)
        {
            if spec.depends_on.is_empty() {
                rfc_content.push_str(&format!("- `{}` (no dependencies)\n", s.task_id));
            } else {
                rfc_content.push_str(&format!(
                    "- `{}` -> {}\n",
                    s.task_id,
                    spec.depends_on.join(", ")
                ));
            }
        }
    }

    // Complexity summary
    rfc_content.push_str("\n## Complexity Summary\n\n");
    for s in &draft_tasks {
        let spec_path = paths.spec_file(&s.task_id);
        if let Ok(content) = std::fs::read_to_string(&spec_path)
            && let Ok(spec) = hive_core::task::parse_spec(&content)
        {
            rfc_content.push_str(&format!(
                "- `{}`: {} (RLCR max rounds: {})\n",
                s.task_id,
                spec.complexity,
                spec.complexity.rlcr_max_rounds()
            ));
        }
    }

    // Embed full spec and plan content
    rfc_content.push_str("\n## Specs and Plans\n\n");
    for s in &draft_tasks {
        rfc_content.push_str(&format!("### Task: {}\n\n", s.task_id));

        let spec_path = paths.spec_file(&s.task_id);
        if let Ok(content) = std::fs::read_to_string(&spec_path) {
            rfc_content.push_str("#### Spec\n\n```\n");
            rfc_content.push_str(&content);
            rfc_content.push_str("\n```\n\n");
        }

        let plan_path = paths.plan_file(&draft_id, &s.task_id);
        if let Ok(content) = std::fs::read_to_string(&plan_path) {
            rfc_content.push_str("#### Plan\n\n```\n");
            rfc_content.push_str(&content);
            rfc_content.push_str("\n```\n\n");
        }
    }

    // Write RFC file
    std::fs::create_dir_all(paths.rfcs_dir())?;
    let rfc_path = paths.rfc_file(&draft_id);
    std::fs::write(&rfc_path, &rfc_content)?;
    println!("generated RFC at {}", rfc_path.display());

    // Update approval_status to rfc
    for s in &draft_tasks {
        let mut state = storage::read_task_state(&paths, &s.task_id)?;
        state.approval_status = ApprovalStatus::Rfc;
        state.touch();
        storage::write_task_state(&paths, &state)?;
    }

    // Commit RFC to branch (always, regardless of platform)
    let rfc_branch = format!("hive/rfc/{draft_id}");
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &rfc_branch])
        .current_dir(&cwd)
        .output();
    let _ = std::process::Command::new("git")
        .args(["add", &rfc_path.to_string_lossy()])
        .current_dir(&cwd)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", &format!("rfc: {draft_id}")])
        .current_dir(&cwd)
        .output();

    // Create PR if platform supports it
    let hive_config = config::load_config(&paths.hive_dir())?;
    let platform = merge::Platform::parse(&hive_config.rfc.platform);

    match platform {
        merge::Platform::Github => {
            match merge::check_tool_available("gh") {
                Ok(()) => {
                    let _ = std::process::Command::new("git")
                        .args(["push", "-u", "origin", &rfc_branch])
                        .current_dir(&cwd)
                        .output();

                    let url = merge::create_pr(
                        &cwd,
                        &platform,
                        &rfc_branch,
                        &format!("RFC: {draft_id}"),
                        &rfc_content,
                        &["rfc"],
                    )?;
                    if let Some(url) = url {
                        println!("RFC PR created: {url}");
                    }
                }
                Err(_) => {
                    bail!(
                        "rfc.platform is 'github' but 'gh' CLI not found. \
                         Set platform: none in config.yml to skip PR creation"
                    );
                }
            }
        }
        _ => {
            println!("RFC committed to branch {rfc_branch}");
        }
    }

    Ok(())
}
