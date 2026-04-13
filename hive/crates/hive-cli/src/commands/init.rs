use std::path::Path;

use anyhow::{Context, Result, bail};
use hive_core::storage::HivePaths;
use hive_git::worktree;

pub fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;

    if !worktree::is_git_repo(&cwd) {
        bail!("not a git repository. Run `git init` first.");
    }

    let paths = HivePaths::new(&cwd);

    // Create all required directories
    for dir in paths.required_dirs() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
    }

    // Generate config.yml if not exists
    if !paths.config_yml().exists() {
        let default_config = generate_default_config();
        std::fs::write(paths.config_yml(), default_config)?;
        println!("created {}", paths.config_yml().display());
    }

    // Generate config.local.yml if not exists
    if !paths.config_local_yml().exists() {
        let local_config = generate_local_config()?;
        std::fs::write(paths.config_local_yml(), local_config)?;
        println!("created {}", paths.config_local_yml().display());
    }

    // Generate audit key at ~/.config/hive/audit.key (outside repo tree)
    // Workers in worktrees cannot access this key, making HMAC CLI-exclusive
    hive_audit::ensure_audit_key()
        .with_context(|| "failed to generate audit key")?;
    if let Some(kp) = hive_audit::audit_key_path()
        && kp.exists() {
            println!("audit key at {}", kp.display());
        }

    // Update .gitignore
    update_gitignore(&cwd)?;

    // Detect agent CLIs and generate adapters
    detect_and_generate_adapters(&cwd, &paths)?;

    println!("hive initialized successfully");
    Ok(())
}

fn generate_default_config() -> String {
    r#"# Hive configuration

launch:
  tool: claude
  # custom_command: "my-tool exec {task_id} --workdir {worktree_path}"

rfc:
  platform: none  # github, gitlab, or none

audit_level: standard  # minimal, standard, or full

skills:
  default:
    - humanize
"#
    .to_string()
}

fn generate_local_config() -> Result<String> {
    let name = git_config_value("user.name").unwrap_or_default();
    let email = git_config_value("user.email").unwrap_or_default();

    Ok(format!(
        r#"# Local config overrides (gitignored)

user:
  name: {name}
  email: {email}
"#
    ))
}

fn git_config_value(key: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["config", key])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

const GITIGNORE_ENTRIES: &[&str] = &[
    ".hive/config.local.yml",
    ".hive/state.md",
    ".hive/specs/",
    ".hive/plans/",
    ".hive/tasks/",
    ".hive/worktrees/",
];

fn update_gitignore(repo_root: &Path) -> Result<()> {
    let gitignore_path = repo_root.join(".gitignore");
    let existing = if gitignore_path.exists() {
        std::fs::read_to_string(&gitignore_path)?
    } else {
        String::new()
    };

    let mut additions = Vec::new();
    for entry in GITIGNORE_ENTRIES {
        if !existing.lines().any(|line| line.trim() == *entry) {
            additions.push(*entry);
        }
    }

    if !additions.is_empty() {
        let mut content = existing;
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("\n# Hive local files\n");
        for entry in additions {
            content.push_str(entry);
            content.push('\n');
        }
        std::fs::write(&gitignore_path, content)?;
        println!("updated .gitignore");
    }

    Ok(())
}

fn detect_and_generate_adapters(repo_root: &Path, paths: &HivePaths) -> Result<()> {
    let has_claude = which_exists("claude");
    let has_codex = which_exists("codex");

    if has_claude {
        generate_claude_adapter(repo_root)?;
        println!("detected claude CLI -- generated Claude Code plugin adapter");
    }

    if has_codex {
        generate_codex_adapter(repo_root)?;
        println!("detected codex CLI -- generated Codex adapter");
    }

    if !has_claude && !has_codex {
        generate_generic_adapter(paths)?;
        eprintln!("warning: neither claude nor codex CLI detected, using generic fallback");
    }

    install_humanize_plugin(repo_root, paths, has_claude, has_codex)?;

    Ok(())
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn generate_claude_adapter(repo_root: &Path) -> Result<()> {
    let plugin_dir = repo_root.join(".claude-plugin");
    std::fs::create_dir_all(&plugin_dir)?;

    let plugin_json = plugin_dir.join("plugin.json");
    let content = serde_json::json!({
        "name": "hive",
        "description": "Hive multi-agent orchestration harness",
        "version": "0.1.0",
        "hooks": {
            "pre-tool-use": "orchestrator-guard.sh"
        },
        "skills": ["humanize"]
    });
    // Always write to ensure hooks/skills are registered
    std::fs::write(&plugin_json, serde_json::to_string_pretty(&content)?)?;

    let skills = [
        ("init", "Initialize hive project"),
        ("exec", "Execute approved tasks"),
        ("status", "Show task status"),
        ("merge", "Merge completed tasks"),
        ("audit", "Query audit log"),
        ("skill", "Manage skills"),
        ("doctor", "Diagnose project health"),
        ("graph", "Display dependency graph"),
        ("rfc", "Generate RFC document"),
    ];

    // Generate orchestrator guard hook
    let guard_hook = plugin_dir.join("orchestrator-guard.sh");
    if !guard_hook.exists() {
        std::fs::write(
            &guard_hook,
            "#!/usr/bin/env bash\n\
             # Orchestrator guard hook: blocks Write/Edit/NotebookEdit when HIVE_ROLE=orchestrator\n\
             if [ \"$HIVE_ROLE\" = \"orchestrator\" ]; then\n\
             \tTOOL=\"$1\"\n\
             \tcase \"$TOOL\" in\n\
             \t\tWrite|Edit|NotebookEdit)\n\
             \t\t\techo \"BLOCKED: orchestrator must not write files directly\" >&2\n\
             \t\t\texit 2\n\
             \t\t\t;;\n\
             \tesac\n\
             fi\n",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&guard_hook, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    for (name, desc) in skills {
        let skill_path = plugin_dir.join(format!("hive-{name}.md"));
        if !skill_path.exists() {
            std::fs::write(
                &skill_path,
                format!(
                    "---\nname: hive:{name}\ndescription: {desc}\n---\n\nRun `hive {name}` to {desc}.\n"
                ),
            )?;
        }
    }

    Ok(())
}

fn generate_codex_adapter(repo_root: &Path) -> Result<()> {
    let codex_dir = repo_root.join(".codex");
    std::fs::create_dir_all(&codex_dir)?;

    let instructions_path = codex_dir.join("instructions.md");
    if !instructions_path.exists() {
        std::fs::write(
            &instructions_path,
            "# Hive CLI Integration\n\n\
             This project uses the Hive multi-agent orchestration harness.\n\n\
             ## Available Commands\n\n\
             - `hive init` -- Initialize a new hive project\n\
             - `hive status` -- Show task status overview\n\
             - `hive exec` -- Execute approved tasks\n\
             - `hive check --task <id>` -- Verify acceptance criteria\n\
             - `hive report --task <id>` -- Process task results\n\
             - `hive merge --task <id>` -- Merge completed task branches\n\
             - `hive retry --task <id>` -- Retry a failed task\n\
             - `hive doctor` -- Diagnose project health\n\
             - `hive audit` -- Query audit log\n\
             - `hive graph` -- Display task dependency graph\n\n\
             ## Working with Tasks\n\n\
             Each task runs in an isolated git worktree. Do not modify files outside your assigned worktree.\n",
        )?;
    }

    // Generate hooks.json
    let hooks_path = codex_dir.join("hooks.json");
    if !hooks_path.exists() {
        let hooks = serde_json::json!({
            "hooks": [
                {
                    "event": "pre-tool-use",
                    "command": "if [ \"$HIVE_ROLE\" = \"orchestrator\" ] && echo \"$TOOL_NAME\" | grep -qE '^(Write|Edit|NotebookEdit)$'; then echo 'BLOCKED: orchestrator must not write files' >&2; exit 2; fi"
                }
            ]
        });
        std::fs::write(&hooks_path, serde_json::to_string_pretty(&hooks)?)?;
    }

    Ok(())
}

fn generate_generic_adapter(paths: &HivePaths) -> Result<()> {
    let skill_dir = paths.skills_dir().join("hive-commands");
    std::fs::create_dir_all(&skill_dir)?;

    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.exists() {
        std::fs::write(
            &skill_md,
            "---\nname: hive-commands\ndescription: Hive CLI orchestration commands\n---\n\n\
             Generic adapter for hive CLI commands.\n",
        )?;
    }

    Ok(())
}

fn install_humanize_plugin(
    repo_root: &Path,
    paths: &HivePaths,
    has_claude: bool,
    has_codex: bool,
) -> Result<()> {
    // Always install to .hive/skills/humanize/ for all agent tools
    let humanize_dir = paths.skills_dir().join("humanize");
    if !humanize_dir.exists() {
        std::fs::create_dir_all(&humanize_dir)?;
        std::fs::write(
            humanize_dir.join("SKILL.md"),
            "---\nname: humanize\ndescription: Humanize RLCR quality loop integration\n---\n\n\
             Default humanize plugin for quality assurance workflows.\n",
        )?;
    }

    // For Claude Code: register in plugin.json
    if has_claude {
        let plugin_json_path = repo_root.join(".claude-plugin/plugin.json");
        if plugin_json_path.exists() {
            // Plugin reference is already in the generated adapter
            println!("humanize: registered for Claude Code");
        }
    }

    // For Codex: merge into instructions
    if has_codex {
        let instructions_path = repo_root.join(".codex/instructions.md");
        if instructions_path.exists() {
            let content = std::fs::read_to_string(&instructions_path)?;
            if !content.contains("humanize") {
                let mut updated = content;
                updated.push_str(
                    "\n## Humanize Integration\n\n\
                     This project uses the humanize RLCR quality loop.\n\
                     Use `gen-plan` for plan generation and `start-rlcr-loop` for iterative development.\n",
                );
                std::fs::write(&instructions_path, updated)?;
            }
            println!("humanize: merged into Codex instructions");
        }
    }

    if !has_claude && !has_codex {
        println!("humanize: installed to .hive/skills/humanize/");
    }

    Ok(())
}
