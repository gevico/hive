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

    let states = storage::load_all_states(&paths)?;
    let draft_tasks: Vec<_> = states.iter().filter(|s| s.draft_id == draft_id).collect();

    if draft_tasks.is_empty() {
        bail!("draft not found: {draft_id}");
    }

    // GitHub advisory check: warn if RFC PR is not merged
    let hive_config = config::load_config(&paths.hive_dir())?;
    if hive_config.rfc.platform == "github"
        && merge::check_tool_available("gh").is_ok() {
            let rfc_branch = format!("hive/rfc/{draft_id}");
            let output = std::process::Command::new("gh")
                .args(["pr", "view", &rfc_branch, "--json", "state", "-q", ".state"])
                .output();
            if let Ok(o) = output
                && o.status.success() {
                    let state = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if state != "MERGED" {
                        eprintln!(
                            "advisory: RFC PR for {draft_id} is in state '{state}', not merged"
                        );
                    }
                }
        }

    // Idempotent: already approved is fine
    let mut count = 0;
    for s in &draft_tasks {
        if s.approval_status == ApprovalStatus::Approved {
            continue;
        }
        let mut state = storage::read_task_state(&paths, &s.task_id)?;
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
