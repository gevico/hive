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
    hive_audit::ensure_audit_key().with_context(|| "failed to generate audit key")?;
    if let Ok(kp) = hive_audit::audit_key_path() {
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
        println!("detected claude CLI -- generated Claude Code plugin");
    }

    if has_codex {
        generate_codex_adapter(repo_root)?;
        println!("detected codex CLI -- generated Codex adapter");
    }

    // Always generate generic adapter (AGENTS.md) for opencode and other tools
    generate_generic_adapter(repo_root, paths)?;

    if !has_claude && !has_codex {
        eprintln!("warning: neither claude nor codex CLI detected; generic adapter generated");
    }

    Ok(())
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .is_ok_and(|o| o.status.success())
}

const HIVE_SKILLS: &[(&str, &str, &str)] = &[
    ("init", "Initialize hive project in the current git repository", "Run `hive init` to create the `.hive/` directory structure, generate config files, detect agent CLIs, and install default plugins.\n\nRequires: current directory must be a git repository."),
    ("exec", "Orchestrate full execution chain for approved tasks", "Run `hive exec` to process all approved tasks through the full pipeline:\nclaim → isolate → launch → check → report\n\nRespects dependency order and retries failed tasks up to the configured limit."),
    ("status", "Show task status overview", "Run `hive status` to display a table of all tasks with their current state, approval status, and dependencies."),
    ("merge", "Merge completed task branches via rebase + PR", "Usage:\n- `hive merge --task <id>` — rebase task branch onto main, create PR\n- `hive merge --all` — merge all completed tasks in dependency order\n- `hive merge --task <id> --mode direct` — merge directly without PR"),
    ("rfc", "Generate RFC document for team review", "Run `hive rfc --draft <draft_id>` to aggregate all specs and plans for a draft into a single RFC document at `.hive/rfcs/<draft_id>.md`."),
    ("approve", "Approve a draft for execution after team review", "Run `hive approve --draft <draft_id>` to transition all specs under the draft to approved status, enabling `hive exec` to schedule them."),
    ("check", "Verify acceptance criteria for a task in review state", "Run `hive check --task <id>` to run all verifiers (command, file, manual) defined in the task spec.\n\nExit codes: 0=all pass, 1=some fail, 2=spec not found, 3=wrong state."),
    ("doctor", "Diagnose environment and project health", "Run `hive doctor` to validate git setup, agent tool availability, config syntax, state consistency, stale locks, worktree health, and audit integrity.\n\nExit codes: 0=healthy, 1=warnings, 2=errors."),
    ("graph", "Display task dependency graph", "Run `hive graph` to visualize the dependency relationships between all tasks."),
];

fn generate_claude_adapter(repo_root: &Path) -> Result<()> {
    // .claude-plugin/plugin.json — metadata only
    let plugin_dir = repo_root.join(".claude-plugin");
    std::fs::create_dir_all(&plugin_dir)?;
    let plugin_json = serde_json::json!({
        "name": "hive",
        "description": "Hive multi-agent orchestration harness",
        "version": "0.1.0"
    });
    std::fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&plugin_json)?,
    )?;

    // skills/ — one directory per command with SKILL.md
    let skills_dir = repo_root.join("skills");
    for (name, desc, body) in HIVE_SKILLS {
        let skill_dir = skills_dir.join(format!("hive-{name}"));
        std::fs::create_dir_all(&skill_dir)?;
        let skill_path = skill_dir.join("SKILL.md");
        if !skill_path.exists() {
            std::fs::write(
                &skill_path,
                format!("---\nname: hive:{name}\ndescription: \"{desc}\"\n---\n\n{body}\n"),
            )?;
        }
    }

    // hooks/ — orchestrator guard
    let hooks_dir = repo_root.join("hooks");
    std::fs::create_dir_all(&hooks_dir)?;

    let hooks_json_path = hooks_dir.join("hooks.json");
    if !hooks_json_path.exists() {
        let hooks = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Write|Edit|NotebookEdit",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "${CLAUDE_PLUGIN_ROOT}/hooks/orchestrator-guard.sh"
                            }
                        ]
                    }
                ]
            }
        });
        std::fs::write(&hooks_json_path, serde_json::to_string_pretty(&hooks)?)?;
    }

    let guard_script = hooks_dir.join("orchestrator-guard.sh");
    if !guard_script.exists() {
        std::fs::write(
            &guard_script,
            r#"#!/usr/bin/env bash
# Orchestrator guard: blocks write tools when HIVE_ROLE=orchestrator
if [ "$HIVE_ROLE" = "orchestrator" ]; then
    echo "BLOCKED: orchestrator role must not write files directly" >&2
    exit 2
fi
"#,
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&guard_script, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    Ok(())
}

fn generate_codex_adapter(repo_root: &Path) -> Result<()> {
    let codex_dir = repo_root.join(".codex");
    std::fs::create_dir_all(&codex_dir)?;

    let instructions_path = codex_dir.join("instructions.md");
    if !instructions_path.exists() {
        let mut content = String::from(
            "# Hive CLI Integration\n\n\
             This project uses the Hive multi-agent orchestration harness.\n\
             Each task runs in an isolated git worktree. Do not modify files outside your assigned worktree.\n\n\
             ## Commands\n\n",
        );
        for (name, desc, _) in HIVE_SKILLS {
            content.push_str(&format!("- `hive {name}` — {desc}\n"));
        }
        content.push_str(
            "\n## Task Lifecycle\n\n\
             - `hive claim --task <id>` — claim a pending task\n\
             - `hive isolate --task <id>` — create isolated worktree\n\
             - `hive launch --task <id>` — start agent in worktree\n\
             - `hive report --task <id>` — process worker results\n\
             - `hive retry --task <id>` — retry a failed task\n\
             - `hive cleanup --task <id>` — remove worktree after merge\n\
             - `hive audit --task <id>` — query per-task audit log\n\
             - `hive show --task <id>` — show detailed task info\n\
             - `hive list-tasks` — list all tasks\n",
        );
        std::fs::write(&instructions_path, content)?;
    }

    Ok(())
}

fn generate_generic_adapter(repo_root: &Path, paths: &HivePaths) -> Result<()> {
    // AGENTS.md at repo root — works with opencode and other AI coding tools
    let agents_md = repo_root.join("AGENTS.md");
    if !agents_md.exists() {
        let mut content = String::from(
            "# Hive — Multi-Agent Orchestration\n\n\
             This project uses the `hive` CLI for multi-agent task orchestration.\n\
             Each task runs in an isolated git worktree. Do not modify files outside your assigned worktree.\n\n\
             ## Core Commands\n\n",
        );
        for (name, desc, _) in HIVE_SKILLS {
            content.push_str(&format!("- `hive {name}` — {desc}\n"));
        }
        content.push_str(
            "\n## Task Lifecycle Commands\n\n\
             - `hive claim --task <id>` — claim a pending task (acquires lock)\n\
             - `hive isolate --task <id>` — create git worktree for the task\n\
             - `hive launch --task <id>` — start agent in the task worktree\n\
             - `hive check --task <id>` — verify acceptance criteria (exit: 0/1/2/3)\n\
             - `hive report --task <id>` — process worker result.md\n\
             - `hive retry --task <id>` — retry a failed task\n\
             - `hive cleanup --task <id>` — remove worktree after merge\n\n\
             ## Diagnostics\n\n\
             - `hive audit --task <id>` — query per-task audit log\n\
             - `hive show --task <id>` — show detailed task information\n\
             - `hive list-tasks [--state <state>]` — list all tasks\n\
             - `hive config --show` — display merged configuration\n\n\
             ## Constraints\n\n\
             - Workers must stay within their assigned worktree\n\
             - Do not push to protected branches\n\
             - State transitions are enforced by the CLI — do not edit state.json directly\n",
        );
        std::fs::write(&agents_md, content)?;
        println!("generated AGENTS.md for generic/opencode tools");
    }

    // Also install to .hive/skills/ for skill-based tools
    let skill_dir = paths.skills_dir().join("hive-commands");
    std::fs::create_dir_all(&skill_dir)?;
    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.exists() {
        std::fs::write(
            &skill_md,
            "---\nname: hive-commands\ndescription: Hive CLI orchestration commands reference\n---\n\n\
             See AGENTS.md at repo root for full command reference.\n",
        )?;
    }

    Ok(())
}
