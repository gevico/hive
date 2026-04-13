use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::frontmatter;
use crate::{HiveError, HiveResult};

/// Complexity levels mapped to RLCR max rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Complexity {
    S,
    M,
    L,
}

impl Complexity {
    pub fn parse(s: &str) -> HiveResult<Self> {
        match s {
            "S" => Ok(Self::S),
            "M" => Ok(Self::M),
            "L" => Ok(Self::L),
            _ => Err(HiveError::InvalidFieldValue {
                field: "complexity".into(),
                reason: format!("expected S, M, or L, got '{s}'"),
            }),
        }
    }

    pub fn rlcr_max_rounds(self) -> u32 {
        match self {
            Self::S => 2,
            Self::M => 5,
            Self::L => 8,
        }
    }
}

impl std::fmt::Display for Complexity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::S => write!(f, "S"),
            Self::M => write!(f, "M"),
            Self::L => write!(f, "L"),
        }
    }
}

/// Generate a new ID in `<user_name>-<ulid>` format.
pub fn generate_id(user_name: &str) -> String {
    let ulid = Ulid::new();
    format!("{user_name}-{ulid}")
}

/// Parsed spec file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub id: String,
    pub draft_id: String,
    pub depends_on: Vec<String>,
    pub complexity: Complexity,
    pub approval_status: ApprovalStatus,
    pub schema_version: u32,
    pub skills: Vec<String>,
    pub exclude_skills: Vec<String>,
    pub goal: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Draft,
    Rfc,
    Approved,
}

impl std::fmt::Display for ApprovalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Rfc => write!(f, "rfc"),
            Self::Approved => write!(f, "approved"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskResultStatus {
    Completed,
    Failed,
}

impl std::fmt::Display for TaskResultStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResultFile {
    pub id: String,
    pub status: TaskResultStatus,
    pub branch: String,
    pub commit: String,
    pub base_commit: String,
    pub schema_version: u32,
    pub body: String,
}

const SPEC_KNOWN_FIELDS: &[&str] = &[
    "id",
    "draft_id",
    "depends_on",
    "complexity",
    "approval_status",
    "schema_version",
    "rlcr_max_rounds",
    "skills",
    "exclude_skills",
];

const RESULT_KNOWN_FIELDS: &[&str] = &[
    "id",
    "status",
    "branch",
    "commit",
    "base_commit",
    "schema_version",
];

/// Parse a spec file (.hive/specs/<id>.md).
pub fn parse_spec(content: &str) -> HiveResult<Spec> {
    let fm = frontmatter::parse(content)?;
    let schema_version = frontmatter::validate_schema_version(&fm)?;
    frontmatter::warn_unknown_fields(&fm, SPEC_KNOWN_FIELDS);

    let id = fm.require_str("id")?.to_string();
    let draft_id = fm.require_str("draft_id")?.to_string();
    let complexity_str = fm.require_str("complexity")?;
    let complexity = Complexity::parse(complexity_str)?;

    let approval_str = fm.get_str("approval_status").unwrap_or("draft");
    let approval_status = match approval_str {
        "draft" => ApprovalStatus::Draft,
        "rfc" => ApprovalStatus::Rfc,
        "approved" => ApprovalStatus::Approved,
        other => {
            return Err(HiveError::InvalidFieldValue {
                field: "approval_status".into(),
                reason: format!("expected draft, rfc, or approved, got '{other}'"),
            });
        }
    };

    let depends_on = fm.optional_string_list("depends_on")?.unwrap_or_default();
    let skills = fm.optional_string_list("skills")?.unwrap_or_default();
    let exclude_skills = fm.optional_string_list("exclude_skills")?.unwrap_or_default();

    // Validate rlcr_max_rounds if present: must not exceed complexity mapping
    if let Some(max_rounds) = fm.get_u32("rlcr_max_rounds") {
        let limit = complexity.rlcr_max_rounds();
        if max_rounds > limit {
            return Err(HiveError::ConstraintViolation(format!(
                "rlcr_max_rounds {max_rounds} exceeds complexity {complexity} limit {limit}"
            )));
        }
    }

    Ok(Spec {
        id,
        draft_id,
        depends_on,
        complexity,
        approval_status,
        schema_version,
        skills,
        exclude_skills,
        goal: String::new(),
        body: fm.body,
    })
}

/// Parse a task result file (.hive/tasks/<id>/result.md).
pub fn parse_result(content: &str) -> HiveResult<TaskResultFile> {
    let fm = frontmatter::parse(content)?;
    let schema_version = frontmatter::validate_schema_version(&fm)?;
    frontmatter::warn_unknown_fields(&fm, RESULT_KNOWN_FIELDS);

    let status = match fm.require_str("status")? {
        "completed" => TaskResultStatus::Completed,
        "failed" => TaskResultStatus::Failed,
        other => {
            return Err(HiveError::InvalidFieldValue {
                field: "status".into(),
                reason: format!("expected completed or failed, got '{other}'"),
            });
        }
    };

    Ok(TaskResultFile {
        id: fm.require_str("id")?.to_string(),
        status,
        branch: fm.require_str("branch")?.to_string(),
        commit: fm.require_str("commit")?.to_string(),
        base_commit: fm.require_str("base_commit")?.to_string(),
        schema_version,
        body: fm.body,
    })
}

/// Compute spec content hash (sha256[:8]) for state.json metadata.
pub fn spec_content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(content.as_bytes());
    format!("{:x}", hash)[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_id_format() {
        let id = generate_id("testuser");
        assert!(id.starts_with("testuser-"));
        // ULID is 26 chars
        assert_eq!(id.len(), "testuser-".len() + 26);
    }

    #[test]
    fn generate_id_uniqueness() {
        let id1 = generate_id("user");
        let id2 = generate_id("user");
        assert_ne!(id1, id2);
    }

    #[test]
    fn complexity_rlcr_mapping() {
        assert_eq!(Complexity::S.rlcr_max_rounds(), 2);
        assert_eq!(Complexity::M.rlcr_max_rounds(), 5);
        assert_eq!(Complexity::L.rlcr_max_rounds(), 8);
    }

    #[test]
    fn complexity_from_invalid() {
        assert!(matches!(
            Complexity::parse("XL"),
            Err(HiveError::InvalidFieldValue { .. })
        ));
    }

    #[test]
    fn parse_valid_spec() {
        let content = r#"---
id: user-01ABCDEF
draft_id: user-01DRAFT
depends_on:
  - user-01OTHER
complexity: M
approval_status: approved
schema_version: 1
---
## Goal
Implement feature X

## Acceptance Criteria
- Criterion 1
"#;
        let spec = parse_spec(content).unwrap();
        assert_eq!(spec.id, "user-01ABCDEF");
        assert_eq!(spec.draft_id, "user-01DRAFT");
        assert_eq!(spec.depends_on, vec!["user-01OTHER"]);
        assert_eq!(spec.complexity, Complexity::M);
        assert_eq!(spec.approval_status, ApprovalStatus::Approved);
        assert!(spec.body.contains("## Goal"));
    }

    #[test]
    fn parse_spec_missing_id() {
        let content = "---\ndraft_id: d\ncomplexity: S\n---\n";
        assert!(matches!(parse_spec(content), Err(HiveError::MissingField(f)) if f == "id"));
    }

    #[test]
    fn parse_spec_invalid_complexity() {
        let content = "---\nid: t\ndraft_id: d\ncomplexity: XL\n---\n";
        assert!(matches!(
            parse_spec(content),
            Err(HiveError::InvalidFieldValue { .. })
        ));
    }

    #[test]
    fn parse_spec_default_approval_status() {
        let content = "---\nid: t\ndraft_id: d\ncomplexity: S\n---\n";
        let spec = parse_spec(content).unwrap();
        assert_eq!(spec.approval_status, ApprovalStatus::Draft);
    }

    #[test]
    fn spec_content_hash_deterministic() {
        let h1 = spec_content_hash("test content");
        let h2 = spec_content_hash("test content");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 8);
    }

    #[test]
    fn spec_content_hash_differs() {
        let h1 = spec_content_hash("content A");
        let h2 = spec_content_hash("content B");
        assert_ne!(h1, h2);
    }

    #[test]
    fn depends_on_string_rejected() {
        let content = "---\nid: t\ndraft_id: d\ncomplexity: S\ndepends_on: not-a-list\n---\n";
        assert!(matches!(
            parse_spec(content),
            Err(HiveError::InvalidFieldValue { field, .. }) if field == "depends_on"
        ));
    }

    #[test]
    fn depends_on_null_accepted() {
        let content = "---\nid: t\ndraft_id: d\ncomplexity: S\ndepends_on:\n---\n";
        let spec = parse_spec(content).unwrap();
        assert!(spec.depends_on.is_empty());
    }

    #[test]
    fn parse_valid_result() {
        let content = r#"---
id: user-01ABCDEF
status: completed
branch: hive/user-01ABCDEF
commit: abcdef1
base_commit: "1234567"
schema_version: 1
---
## Summary
Done
"#;
        let result = parse_result(content).unwrap();
        assert_eq!(result.id, "user-01ABCDEF");
        assert_eq!(result.status, TaskResultStatus::Completed);
        assert_eq!(result.branch, "hive/user-01ABCDEF");
    }

    #[test]
    fn parse_result_requires_base_commit() {
        let content = "---\nid: t\nstatus: completed\nbranch: hive/t\ncommit: abc\n---\n";
        assert!(matches!(
            parse_result(content),
            Err(HiveError::MissingField(field)) if field == "base_commit"
        ));
    }
}
