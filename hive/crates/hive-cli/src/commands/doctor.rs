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

    // Check stale locks
    let tasks_dir = paths.tasks_dir();
    if tasks_dir.exists() {
        for entry in std::fs::read_dir(&tasks_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let lock_path = entry.path().join("lock");
                if lock_path.exists()
                    && let Ok(content) = std::fs::read_to_string(&lock_path)
                        && let Some(pid_str) = content.lines().next()
                            && let Ok(pid) = pid_str.trim().parse::<u32>() {
                                let alive =
                                    std::path::Path::new(&format!("/proc/{pid}")).exists();
                                if !alive {
                                    println!(
                                        "[warn] stale lock: {} (pid {pid} dead)",
                                        lock_path.display()
                                    );
                                    warnings += 1;
                                }
                            }
            }
        }
    }

    // Check state consistency
    let states = storage::load_all_states(&paths).unwrap_or_default();
    for s in &states {
        let spec_path = paths.spec_file(&s.task_id);
        if !spec_path.exists() {
            println!("[error] missing spec for task {}", s.task_id);
            errors += 1;
        }
    }

    // Check for orphaned worktrees
    let worktrees_dir = paths.worktrees_dir();
    if worktrees_dir.exists() {
        for entry in std::fs::read_dir(&worktrees_dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str()
                && !states.iter().any(|s| s.task_id == name) {
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
