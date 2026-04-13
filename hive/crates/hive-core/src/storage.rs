use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::state::TaskState;
use crate::task::ApprovalStatus;
use crate::{HiveError, HiveResult};

/// Per-task persistent state stored in .hive/tasks/<id>/state.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStateFile {
    pub schema_version: u32,
    pub task_id: String,
    pub draft_id: String,
    pub state: TaskState,
    pub retry_count: u32,
    pub approval_status: ApprovalStatus,
    pub spec_content_hash: String,
    pub base_commit: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TaskStateFile {
    pub fn new(task_id: String, draft_id: String, spec_hash: String) -> Self {
        let now = Utc::now();
        Self {
            schema_version: 1,
            task_id,
            draft_id,
            state: TaskState::Pending,
            retry_count: 0,
            approval_status: ApprovalStatus::Draft,
            spec_content_hash: spec_hash,
            base_commit: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }
}

/// Path helpers for .hive directory structure.
pub struct HivePaths {
    root: PathBuf,
}

impl HivePaths {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn hive_dir(&self) -> PathBuf {
        self.root.join(".hive")
    }

    pub fn config_yml(&self) -> PathBuf {
        self.hive_dir().join("config.yml")
    }

    pub fn config_local_yml(&self) -> PathBuf {
        self.hive_dir().join("config.local.yml")
    }

    pub fn specs_dir(&self) -> PathBuf {
        self.hive_dir().join("specs")
    }

    pub fn plans_dir(&self) -> PathBuf {
        self.hive_dir().join("plans")
    }

    pub fn rfcs_dir(&self) -> PathBuf {
        self.hive_dir().join("rfcs")
    }

    pub fn reports_dir(&self) -> PathBuf {
        self.hive_dir().join("reports")
    }

    pub fn tasks_dir(&self) -> PathBuf {
        self.hive_dir().join("tasks")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.hive_dir().join("skills")
    }

    pub fn worktrees_dir(&self) -> PathBuf {
        self.hive_dir().join("worktrees")
    }

    pub fn task_dir(&self, task_id: &str) -> PathBuf {
        self.tasks_dir().join(task_id)
    }

    pub fn state_json(&self, task_id: &str) -> PathBuf {
        self.task_dir(task_id).join("state.json")
    }

    pub fn lock_file(&self, task_id: &str) -> PathBuf {
        self.task_dir(task_id).join("lock")
    }

    pub fn audit_file(&self, task_id: &str) -> PathBuf {
        self.task_dir(task_id).join("audit.md")
    }

    pub fn orchestrator_lock(&self) -> PathBuf {
        self.hive_dir().join("orchestrator.lock")
    }

    pub fn state_md(&self) -> PathBuf {
        self.hive_dir().join("state.md")
    }

    pub fn spec_file(&self, task_id: &str) -> PathBuf {
        self.specs_dir().join(format!("{task_id}.md"))
    }

    pub fn plan_file(&self, draft_id: &str, task_id: &str) -> PathBuf {
        self.plans_dir()
            .join(draft_id)
            .join(format!("{task_id}.md"))
    }

    pub fn rfc_file(&self, draft_id: &str) -> PathBuf {
        self.rfcs_dir().join(format!("{draft_id}.md"))
    }

    pub fn worktree_path(&self, task_id: &str) -> PathBuf {
        self.worktrees_dir().join(task_id)
    }

    /// All required directories for `hive init`.
    pub fn required_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.specs_dir(),
            self.plans_dir(),
            self.rfcs_dir(),
            self.reports_dir(),
            self.tasks_dir(),
            self.skills_dir(),
            self.worktrees_dir(),
        ]
    }
}

/// Read a task's state.json.
pub fn read_task_state(paths: &HivePaths, task_id: &str) -> HiveResult<TaskStateFile> {
    let path = paths.state_json(task_id);
    let content =
        std::fs::read_to_string(&path).map_err(|_| HiveError::TaskNotFound(task_id.to_string()))?;
    let state: TaskStateFile = serde_json::from_str(&content)?;
    Ok(state)
}

/// Write a task's state.json atomically (write to .tmp then rename).
pub fn write_task_state(paths: &HivePaths, state: &TaskStateFile) -> HiveResult<()> {
    let dir = paths.task_dir(&state.task_id);
    std::fs::create_dir_all(&dir)?;
    let path = paths.state_json(&state.task_id);
    let tmp = path.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(state)?;
    std::fs::write(&tmp, &content)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// List all task IDs by scanning .hive/tasks/ directories.
pub fn list_task_ids(paths: &HivePaths) -> HiveResult<Vec<String>> {
    let tasks_dir = paths.tasks_dir();
    if !tasks_dir.exists() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(&tasks_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Some(name) = entry.file_name().to_str()
        {
            ids.push(name.to_string());
        }
    }
    ids.sort();
    Ok(ids)
}

/// Load all task states.
pub fn load_all_states(paths: &HivePaths) -> HiveResult<Vec<TaskStateFile>> {
    let ids = list_task_ids(paths)?;
    let mut states = Vec::new();
    for id in ids {
        states.push(read_task_state(paths, &id)?);
    }
    Ok(states)
}

/// Regenerate state.md from all task state.json files.
pub fn regenerate_state_md(paths: &HivePaths) -> HiveResult<()> {
    let states = load_all_states(paths)?;
    let mut md = String::from("# Hive Task Status\n\n");
    md.push_str("| Task ID | Draft | State | Retries | Approval | Updated |\n");
    md.push_str("|---------|-------|-------|---------|----------|---------|\n");
    for s in &states {
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
    std::fs::write(paths.state_md(), md)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, HivePaths) {
        let tmp = TempDir::new().unwrap();
        let paths = HivePaths::new(tmp.path());
        std::fs::create_dir_all(paths.hive_dir()).unwrap();
        (tmp, paths)
    }

    #[test]
    fn task_state_roundtrip() {
        let (_tmp, paths) = setup();
        let state = TaskStateFile::new(
            "user-01TEST".into(),
            "user-01DRAFT".into(),
            "abcd1234".into(),
        );
        write_task_state(&paths, &state).unwrap();
        let loaded = read_task_state(&paths, "user-01TEST").unwrap();
        assert_eq!(loaded.task_id, "user-01TEST");
        assert_eq!(loaded.state, TaskState::Pending);
        assert_eq!(loaded.spec_content_hash, "abcd1234");
    }

    #[test]
    fn read_nonexistent_task() {
        let (_tmp, paths) = setup();
        assert!(matches!(
            read_task_state(&paths, "nonexistent"),
            Err(HiveError::TaskNotFound(_))
        ));
    }

    #[test]
    fn list_task_ids_empty() {
        let (_tmp, paths) = setup();
        std::fs::create_dir_all(paths.tasks_dir()).unwrap();
        let ids = list_task_ids(&paths).unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn list_task_ids_multiple() {
        let (_tmp, paths) = setup();
        let s1 = TaskStateFile::new("a-01".into(), "d".into(), "h".into());
        let s2 = TaskStateFile::new("b-02".into(), "d".into(), "h".into());
        write_task_state(&paths, &s1).unwrap();
        write_task_state(&paths, &s2).unwrap();
        let ids = list_task_ids(&paths).unwrap();
        assert_eq!(ids, vec!["a-01", "b-02"]);
    }

    #[test]
    fn regenerate_state_md_creates_file() {
        let (_tmp, paths) = setup();
        let s = TaskStateFile::new("t-01".into(), "d-01".into(), "hash".into());
        write_task_state(&paths, &s).unwrap();
        regenerate_state_md(&paths).unwrap();
        let md = std::fs::read_to_string(paths.state_md()).unwrap();
        assert!(md.contains("t-01"));
        assert!(md.contains("pending"));
    }

    #[test]
    fn state_md_reflects_state_json() {
        let (_tmp, paths) = setup();
        let mut s = TaskStateFile::new("t-01".into(), "d-01".into(), "hash".into());
        write_task_state(&paths, &s).unwrap();
        regenerate_state_md(&paths).unwrap();
        let md1 = std::fs::read_to_string(paths.state_md()).unwrap();
        assert!(md1.contains("pending"));

        // Modify state
        s.state = TaskState::InProgress;
        s.touch();
        write_task_state(&paths, &s).unwrap();
        regenerate_state_md(&paths).unwrap();
        let md2 = std::fs::read_to_string(paths.state_md()).unwrap();
        assert!(md2.contains("in_progress"));
        assert!(!md2.contains("pending"));
    }

    #[test]
    fn atomic_write_survives_concurrent_read() {
        let (_tmp, paths) = setup();
        let s = TaskStateFile::new("t-01".into(), "d-01".into(), "h".into());
        write_task_state(&paths, &s).unwrap();
        // Concurrent read should see consistent data
        let loaded = read_task_state(&paths, "t-01").unwrap();
        assert_eq!(loaded.task_id, "t-01");
    }

    #[test]
    fn path_helpers() {
        let paths = HivePaths::new(Path::new("/repo"));
        assert_eq!(paths.hive_dir(), PathBuf::from("/repo/.hive"));
        assert_eq!(
            paths.state_json("t-01"),
            PathBuf::from("/repo/.hive/tasks/t-01/state.json")
        );
        assert_eq!(
            paths.plan_file("d-01", "t-01"),
            PathBuf::from("/repo/.hive/plans/d-01/t-01.md")
        );
        assert_eq!(
            paths.worktree_path("t-01"),
            PathBuf::from("/repo/.hive/worktrees/t-01")
        );
    }
}
