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

    // Discover specs for this draft by scanning .hive/specs/ directory
    let specs_dir = paths.specs_dir();
    let mut draft_specs: Vec<(String, hive_core::task::Spec)> = Vec::new();

    if specs_dir.exists() {
        for entry in std::fs::read_dir(&specs_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md")
                && let Ok(content) = std::fs::read_to_string(&path)
                    && let Ok(spec) = hive_core::task::parse_spec(&content)
                        && spec.draft_id == draft_id {
                            draft_specs.push((spec.id.clone(), spec));
                        }
        }
    }

    if draft_specs.is_empty() {
        bail!("no specs found for draft {draft_id}");
    }

    // Check approval state from state.json if it exists, otherwise treat as draft
    for (task_id, _) in &draft_specs {
        if let Ok(state) = storage::read_task_state(&paths, task_id)
            && (state.approval_status == ApprovalStatus::Rfc
                || state.approval_status == ApprovalStatus::Approved)
            {
                bail!("draft {draft_id} already in '{}' state", state.approval_status);
            }
    }

    // Validate plans exist for all specs
    for (task_id, _) in &draft_specs {
        let plan_path = paths.plan_file(&draft_id, task_id);
        if !plan_path.exists() {
            bail!("plan not found for task {task_id}");
        }
    }

    // Generate RFC document
    let mut rfc_content = format!("# RFC: {draft_id}\n\n");
    rfc_content.push_str("## Overview\n\n");
    rfc_content.push_str(&format!(
        "This RFC covers {} tasks for draft `{draft_id}`.\n\n",
        draft_specs.len()
    ));

    // Dependency graph (text)
    rfc_content.push_str("## Dependency Graph\n\n");
    for (task_id, spec) in &draft_specs {
        if spec.depends_on.is_empty() {
            rfc_content.push_str(&format!("- `{task_id}` (no dependencies)\n"));
        } else {
            rfc_content.push_str(&format!(
                "- `{task_id}` -> {}\n",
                spec.depends_on.join(", ")
            ));
        }
    }

    // Complexity summary
    rfc_content.push_str("\n## Complexity Summary\n\n");
    for (task_id, spec) in &draft_specs {
        rfc_content.push_str(&format!(
            "- `{task_id}`: {} (RLCR max rounds: {})\n",
            spec.complexity,
            spec.complexity.rlcr_max_rounds()
        ));
    }

    // Embed full spec and plan content
    rfc_content.push_str("\n## Specs and Plans\n\n");
    for (task_id, _) in &draft_specs {
        rfc_content.push_str(&format!("### Task: {task_id}\n\n"));

        let spec_path = paths.spec_file(task_id);
        if let Ok(content) = std::fs::read_to_string(&spec_path) {
            rfc_content.push_str("#### Spec\n\n```\n");
            rfc_content.push_str(&content);
            rfc_content.push_str("\n```\n\n");
        }

        let plan_path = paths.plan_file(&draft_id, task_id);
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

    // Update approval_status to rfc (create state.json if it doesn't exist)
    for (task_id, _spec) in &draft_specs {
        let mut state = match storage::read_task_state(&paths, task_id) {
            Ok(s) => s,
            Err(_) => {
                // Bootstrap state from spec
                let hash = hive_core::task::spec_content_hash(
                    &std::fs::read_to_string(paths.spec_file(task_id)).unwrap_or_default(),
                );
                storage::TaskStateFile::new(task_id.clone(), draft_id.clone(), hash)
            }
        };
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
