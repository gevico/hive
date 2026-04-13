use anyhow::{Context, Result, bail};
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

    // Discover draft tasks by scanning specs/ (same as rfc)
    let specs_dir = paths.specs_dir();
    let mut draft_task_ids: Vec<String> = Vec::new();

    if specs_dir.exists() {
        for entry in std::fs::read_dir(&specs_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("failed to read spec: {}", path.display()))?;
                let spec = hive_core::task::parse_spec(&content)
                    .with_context(|| format!("invalid spec: {}", path.display()))?;
                if spec.draft_id == draft_id {
                    draft_task_ids.push(spec.id.clone());
                }
            }
        }
    }

    // Also check state.json for tasks that may not have spec files
    let states = storage::load_all_states(&paths)?;
    for s in &states {
        if s.draft_id == draft_id && !draft_task_ids.contains(&s.task_id) {
            draft_task_ids.push(s.task_id.clone());
        }
    }

    if draft_task_ids.is_empty() {
        bail!("draft not found: {draft_id}");
    }

    // GitHub advisory check
    let hive_config = config::load_config(&paths.hive_dir())?;
    if hive_config.rfc.platform == "github"
        && merge::check_tool_available("gh").is_ok()
    {
        let rfc_branch = format!("hive/rfc/{draft_id}");
        let output = std::process::Command::new("gh")
            .args(["pr", "view", &rfc_branch, "--json", "state", "-q", ".state"])
            .output();
        if let Ok(o) = output
            && o.status.success()
        {
            let state = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if state != "MERGED" {
                eprintln!(
                    "advisory: RFC PR for {draft_id} is in state '{state}', not merged"
                );
            }
        }
    }

    // Approve: bootstrap state if needed, idempotent for already-approved
    let mut count = 0;
    for task_id in &draft_task_ids {
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
        if state.approval_status == ApprovalStatus::Approved {
            continue;
        }
        state.approval_status = ApprovalStatus::Approved;
        state.touch();
        storage::write_task_state(&paths, &state)?;
        count += 1;
    }

    if count == 0 {
        println!("draft {draft_id}: already approved (no changes)");
    } else {
        println!("draft {draft_id}: approved {count} tasks");
    }

    Ok(())
}
