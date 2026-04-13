use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use hive_core::state::TaskState;
use hive_core::storage::{HivePaths, TaskStateFile, read_task_state, write_task_state};
use hive_core::task::{ApprovalStatus, spec_content_hash};
use tempfile::TempDir;

struct TestRepo {
    _tmp: TempDir,
    root: PathBuf,
    paths: HivePaths,
}

impl TestRepo {
    fn new(config: &str) -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        git(&root, &["init", "-b", "main"]);
        git(&root, &["config", "user.name", "Test User"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        std::fs::write(root.join("README.md"), "fixture\n").unwrap();
        git(&root, &["add", "README.md"]);
        git(&root, &["commit", "-m", "init"]);

        let paths = HivePaths::new(&root);
        for dir in paths.required_dirs() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(paths.config_yml(), config).unwrap();
        std::fs::write(
            paths.config_local_yml(),
            "# test local overrides\nuser:\n  name: test\n  email: test@example.com\n",
        )
        .unwrap();

        Self {
            _tmp: tmp,
            root,
            paths,
        }
    }

    fn write_task(&self, task_id: &str, draft_id: &str, state: TaskState, spec_body: &str) {
        let spec = format!(
            "---\nid: {task_id}\ndraft_id: {draft_id}\ncomplexity: S\napproval_status: approved\nschema_version: 1\n---\n{spec_body}\n"
        );
        std::fs::write(self.paths.spec_file(task_id), &spec).unwrap();

        let plan_dir = self.paths.plans_dir().join(draft_id);
        std::fs::create_dir_all(&plan_dir).unwrap();
        std::fs::write(self.paths.plan_file(draft_id, task_id), "# plan\n").unwrap();

        let mut task_state = TaskStateFile::new(
            task_id.to_string(),
            draft_id.to_string(),
            spec_content_hash(&spec),
        );
        task_state.state = state;
        task_state.approval_status = ApprovalStatus::Approved;
        write_task_state(&self.paths, &task_state).unwrap();
    }

    fn run_hive(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_hive"))
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn custom_launch_config(task_id: &str) -> String {
    format!(
        "launch:\n  tool: custom\n  custom_command: |\n    cat > result.md <<'EOF'\n    ---\n    id: {task_id}\n    status: completed\n    branch: hive/{task_id}\n    commit: abcdef1\n    base_commit: 1234567\n    schema_version: 1\n    ---\n    ## Summary\n    done\n    EOF\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n"
    )
}

#[test]
fn launch_accepts_assigned_state_and_marks_in_progress() {
    let task_id = "test-01TASK";
    let repo = TestRepo::new(&custom_launch_config(task_id));
    repo.write_task(
        task_id,
        "draft-01",
        TaskState::Assigned,
        "verify-command: true",
    );
    std::fs::create_dir_all(repo.paths.worktree_path(task_id)).unwrap();

    let output = repo.run_hive(&["launch", "--task", task_id]);
    assert!(output.status.success(), "{output:?}");

    let state = read_task_state(&repo.paths, task_id).unwrap();
    assert_eq!(state.state, TaskState::InProgress);
    assert!(repo.paths.worktree_path(task_id).join("result.md").exists());
}

#[test]
fn check_returns_spec_not_found_exit_code_for_missing_task() {
    let repo = TestRepo::new(
        "launch:\n  tool: custom\n  custom_command: \"true\"\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n",
    );

    let output = repo.run_hive(&["check", "--task", "missing-task"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn exec_does_not_complete_task_when_check_fails() {
    let task_id = "test-02TASK";
    let repo = TestRepo::new(&custom_launch_config(task_id));
    repo.write_task(
        task_id,
        "draft-02",
        TaskState::Pending,
        "verify-command: false",
    );

    let output = repo.run_hive(&["exec"]);
    assert!(output.status.success(), "{output:?}");

    let state = read_task_state(&repo.paths, task_id).unwrap();
    assert_eq!(state.state, TaskState::Blocked);
    assert_eq!(state.retry_count, 3);
}
