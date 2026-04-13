use anyhow::Result;
use hive_core::config;
use hive_core::storage::{self, HivePaths};
use hive_git::{merge, worktree};

// Exit codes per AC-18
const EXIT_HEALTHY: i32 = 0;
const EXIT_WARNINGS: i32 = 1;
const EXIT_ERRORS: i32 = 2;

pub fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        eprintln!("error: not a hive project. Run `hive init` first");
        std::process::exit(EXIT_ERRORS);
    }

    let mut warnings = 0u32;
    let mut errors = 0u32;

    // Check git
    if worktree::is_git_repo(&cwd) {
        println!("[ok] git repository detected");
    } else {
        println!("[error] not a git repository");
        errors += 1;
    }

    // Check config
    match config::load_config(&paths.hive_dir()) {
        Ok(cfg) => {
            println!("[ok] config.yml valid");

            // Check configured agent tool
            let tool = &cfg.launch.tool;
            match tool.as_str() {
                "claude" | "codex" => {
                    if merge::check_tool_available(tool).is_ok() {
                        println!("[ok] agent tool '{tool}' available");
                    } else {
                        println!("[warn] agent tool '{tool}' not found in PATH");
                        warnings += 1;
                    }
                }
                "custom" => println!("[ok] custom launch tool configured"),
                _ => {
                    println!("[warn] unknown launch tool: {tool}");
                    warnings += 1;
                }
            }

            // Check gh CLI if platform is github
            if cfg.rfc.platform == "github" {
                if merge::check_tool_available("gh").is_ok() {
                    println!("[ok] gh CLI available");
                } else {
                    println!("[warn] rfc.platform is 'github' but gh CLI not found");
                    warnings += 1;
                }
            }
        }
        Err(e) => {
            println!("[error] config invalid: {e}");
            errors += 1;
        }
    }

    // Check required directories
    for dir in paths.required_dirs() {
        if dir.exists() {
            // ok, don't print for each
        } else {
            println!("[warn] missing directory: {}", dir.display());
            warnings += 1;
        }
    }

    // Check stale locks (5-minute threshold per design)
    let stale_threshold = std::time::Duration::from_secs(5 * 60);
    let tasks_dir = paths.tasks_dir();
    if tasks_dir.exists() {
        for entry in std::fs::read_dir(&tasks_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let lock_path = entry.path().join("lock");
                if lock_path.exists()
                    && let Ok(content) = std::fs::read_to_string(&lock_path)
                    && let Some(pid_str) = content.lines().next()
                    && let Ok(pid) = pid_str.trim().parse::<u32>()
                {
                    let alive = std::path::Path::new(&format!("/proc/{pid}")).exists();
                    if !alive {
                        let age = std::fs::metadata(&lock_path)
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| std::time::SystemTime::now().duration_since(t).ok());
                        if let Some(age) = age
                            && age > stale_threshold {
                                println!(
                                    "[warn] stale lock: {} (pid {pid} dead, age {:.0}s > 300s)",
                                    lock_path.display(),
                                    age.as_secs_f64()
                                );
                                warnings += 1;
                            }
                    }
                }
            }
        }
    }

    // Check state consistency: state.json is authoritative
    let states = storage::load_all_states(&paths).unwrap_or_default();
    for s in &states {
        let spec_path = paths.spec_file(&s.task_id);
        if !spec_path.exists() {
            println!("[error] missing spec for task {}", s.task_id);
            errors += 1;
        }
    }

    // state.md vs state.json divergence is checked below in audit section

    // Check audit append-only integrity
    for s in &states {
        let audit_path = paths.audit_file(&s.task_id);
        if audit_path.exists()
            && let Ok(content) = std::fs::read_to_string(&audit_path)
        {
            if content.is_empty() {
                continue;
            }
            // Check CLI-written header
            if !content.starts_with("# Audit Log") {
                println!(
                    "[warn] audit file for {} may have been modified externally (missing header)",
                    s.task_id
                );
                warnings += 1;
            }
            // Check monotonic timestamps (append-only invariant)
            let mut prev_ts = String::new();
            for line in content.lines() {
                if let Some(ts_start) = line.find('[')
                    && let Some(ts_end) = line[ts_start + 1..].find(']') {
                        let ts = &line[ts_start + 1..ts_start + 1 + ts_end];
                        if !prev_ts.is_empty() && ts < prev_ts.as_str() {
                            println!(
                                "[warn] audit file for {} has non-monotonic timestamps (possible tampering)",
                                s.task_id
                            );
                            warnings += 1;
                            break;
                        }
                        prev_ts = ts.to_string();
                    }
            }
        }
    }

    // Check state.md vs state.json divergence
    let state_md_path = paths.state_md();
    if state_md_path.exists() {
        // Regenerate expected content and compare
        let expected = build_expected_state_md(&states);
        if let Ok(actual) = std::fs::read_to_string(&state_md_path) {
            if actual.trim() != expected.trim() {
                println!("[warn] state.md diverges from state.json (state.json is authoritative)");
                warnings += 1;
            } else {
                println!("[ok] state.md consistent with state.json");
            }
        }
    }

    // Check for orphaned worktrees
    let worktrees_dir = paths.worktrees_dir();
    if worktrees_dir.exists() {
        for entry in std::fs::read_dir(&worktrees_dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str()
                && !states.iter().any(|s| s.task_id == name)
            {
                println!("[warn] orphaned worktree: {name}");
                warnings += 1;
            }
        }
    }

    // Summary
    println!();
    if errors > 0 {
        println!("health check: {errors} error(s), {warnings} warning(s)");
        std::process::exit(EXIT_ERRORS);
    } else if warnings > 0 {
        println!("health check: {warnings} warning(s)");
        std::process::exit(EXIT_WARNINGS);
    } else {
        println!("health check: all clear");
        std::process::exit(EXIT_HEALTHY);
    }
}

fn build_expected_state_md(states: &[storage::TaskStateFile]) -> String {
    let mut md = String::from("# Hive Task Status\n\n");
    md.push_str("| Task ID | Draft | State | Retries | Approval | Updated |\n");
    md.push_str("|---------|-------|-------|---------|----------|---------|\n");
    for s in states {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            s.task_id,
            s.draft_id,
            s.state,
            s.retry_count,
            s.approval_status,
            s.updated_at.format("%Y-%m-%d %H:%M"),
        ));
    }
    md
}
