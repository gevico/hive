use std::path::{Path, PathBuf};

use crate::frontmatter;
use crate::{HiveError, HiveResult};

const MAX_SKILL_NAME_LEN: usize = 64;

#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub content: String,
    pub source_path: PathBuf,
}

/// Discover and load skills from 3-tier sources.
/// Priority: repo (.hive/skills/) > user (~/.config/hive/skills/) > system plugins.
pub fn discover_skills(
    repo_skills: &Path,
    user_skills: Option<&Path>,
    default_names: &[String],
    task_skills: &[String],
    exclude_skills: &[String],
) -> HiveResult<Vec<SkillInfo>> {
    let mut loaded = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Collect all skill names to load
    let mut names_to_load: Vec<String> = default_names.to_vec();
    for name in task_skills {
        if !names_to_load.contains(name) {
            names_to_load.push(name.clone());
        }
    }

    // Remove excluded
    names_to_load.retain(|n| !exclude_skills.contains(n));

    for name in &names_to_load {
        if seen.contains(name) {
            continue;
        }

        // Validate skill name
        if !is_valid_skill_name(name) {
            return Err(HiveError::Skill(format!(
                "invalid skill name '{name}': only alphanumeric and hyphen allowed"
            )));
        }

        // Try repo first, then user, then system
        let skill = try_load_skill(repo_skills, name)
            .or_else(|| user_skills.and_then(|p| try_load_skill(p, name)));

        if let Some(skill) = skill {
            seen.insert(name.clone());
            loaded.push(skill);
        } else {
            eprintln!("warning: skill '{name}' not found, skipping");
        }
    }

    Ok(loaded)
}

fn try_load_skill(base: &Path, name: &str) -> Option<SkillInfo> {
    let skill_dir = base.join(name);
    let skill_md = skill_dir.join("SKILL.md");

    if !skill_md.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&skill_md).ok()?;
    let fm = frontmatter::parse(&content).ok()?;

    let skill_name = fm.get_str("name")?.to_string();
    let description = fm.get_str("description")?.to_string();

    // Validate constraints
    if description.len() > 500 {
        eprintln!("warning: skill '{skill_name}' description exceeds 500 chars, skipping");
        return None;
    }

    if content.find("---").map_or(0, |start| {
        content[start + 3..].find("---").map_or(0, |end| end)
    }) > 1024
    {
        eprintln!("warning: skill '{skill_name}' frontmatter exceeds 1024 chars, skipping");
        return None;
    }

    Some(SkillInfo {
        name: skill_name,
        description,
        content: fm.body,
        source_path: skill_md,
    })
}

fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SKILL_NAME_LEN
        && name.chars().all(|c| c.is_alphanumeric() || c == '-')
}

/// Build the agent context string from loaded skills.
pub fn build_skill_context(skills: &[SkillInfo]) -> String {
    let mut ctx = String::new();
    for skill in skills {
        ctx.push_str(&format!("## Skill: {}\n\n", skill.name));
        ctx.push_str(&skill.content);
        ctx.push_str("\n\n");
    }
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_skill(base: &Path, name: &str, desc: &str) {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {desc}\n---\nSkill content for {name}.\n"),
        )
        .unwrap();
    }

    #[test]
    fn discover_from_repo() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        create_skill(&skills_dir, "my-skill", "A test skill");

        let loaded =
            discover_skills(&skills_dir, None, &["my-skill".to_string()], &[], &[]).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "my-skill");
    }

    #[test]
    fn exclude_skill() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        create_skill(&skills_dir, "skill-a", "Skill A");
        create_skill(&skills_dir, "skill-b", "Skill B");

        let loaded = discover_skills(
            &skills_dir,
            None,
            &["skill-a".to_string(), "skill-b".to_string()],
            &[],
            &["skill-b".to_string()],
        )
        .unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "skill-a");
    }

    #[test]
    fn repo_overrides_user() {
        let tmp = TempDir::new().unwrap();
        let repo_dir = tmp.path().join("repo");
        let user_dir = tmp.path().join("user");
        create_skill(&repo_dir, "shared", "Repo version");
        create_skill(&user_dir, "shared", "User version");

        let loaded = discover_skills(
            &repo_dir,
            Some(&user_dir),
            &["shared".to_string()],
            &[],
            &[],
        )
        .unwrap();

        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].source_path.starts_with(&repo_dir));
    }

    #[test]
    fn invalid_skill_name_rejected() {
        let tmp = TempDir::new().unwrap();
        let result = discover_skills(tmp.path(), None, &["bad name!".to_string()], &[], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn missing_skill_md_skipped() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        std::fs::create_dir_all(skills_dir.join("empty-skill")).unwrap();
        // No SKILL.md

        let loaded =
            discover_skills(&skills_dir, None, &["empty-skill".to_string()], &[], &[]).unwrap();

        assert!(loaded.is_empty());
    }

    #[test]
    fn build_context() {
        let skills = vec![SkillInfo {
            name: "test".into(),
            description: "desc".into(),
            content: "content here".into(),
            source_path: PathBuf::from("/tmp"),
        }];
        let ctx = build_skill_context(&skills);
        assert!(ctx.contains("## Skill: test"));
        assert!(ctx.contains("content here"));
    }
}
