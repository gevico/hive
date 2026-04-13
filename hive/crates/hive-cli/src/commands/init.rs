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

    install_humanize_plugin(paths, has_claude, has_codex)?;

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
    if !plugin_json.exists() {
        let content = serde_json::json!({
            "name": "hive",
            "description": "Hive multi-agent orchestration harness",
            "version": "0.1.0"
        });
        std::fs::write(&plugin_json, serde_json::to_string_pretty(&content)?)?;
    }

    let skills = [
        ("init", "Initialize hive project"),
        ("exec", "Execute approved tasks"),
        ("status", "Show task status"),
        ("merge", "Merge completed tasks"),
        ("audit", "Query audit log"),
        ("doctor", "Diagnose project health"),
        ("rfc", "Generate RFC document"),
    ];

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
             - `hive doctor` -- Diagnose project health\n",
        )?;
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

fn install_humanize_plugin(paths: &HivePaths, has_claude: bool, has_codex: bool) -> Result<()> {
    if has_claude || has_codex {
        // Adapter generation already handles plugin references
        return Ok(());
    }

    // Generic fallback: install to .hive/skills/humanize/
    let humanize_dir = paths.skills_dir().join("humanize");
    if !humanize_dir.exists() {
        std::fs::create_dir_all(&humanize_dir)?;
        std::fs::write(
            humanize_dir.join("SKILL.md"),
            "---\nname: humanize\ndescription: Humanize RLCR quality loop integration\n---\n\n\
             Default humanize plugin for quality assurance.\n",
        )?;
        println!("installed humanize plugin to .hive/skills/humanize/");
    }

    Ok(())
}
