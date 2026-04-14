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
    audit_key_path: PathBuf,
}

impl TestRepo {
    fn new(config: &str) -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        // Per-test isolated audit key (NOT set as process env var — passed only to subprocess)
        let key_path = tmp.path().join("test-audit.key");
        std::fs::write(&key_path, b"test-key-32-bytes-exactly-right!").unwrap();

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
            audit_key_path: key_path,
        }
    }

    fn write_task(&self, task_id: &str, draft_id: &str, state: TaskState, spec_body: &str) {
        self.write_task_with(task_id, draft_id, state, &format!(
            "---\nid: {task_id}\ndraft_id: {draft_id}\ncomplexity: S\napproval_status: approved\nschema_version: 1\n---\n{spec_body}\n"
        ), true);
    }

    fn write_task_with(
        &self,
        task_id: &str,
        draft_id: &str,
        state: TaskState,
        spec: &str,
        with_plan: bool,
    ) {
        std::fs::write(self.paths.spec_file(task_id), spec).unwrap();

        let plan_dir = self.paths.plans_dir().join(draft_id);
        std::fs::create_dir_all(&plan_dir).unwrap();
        if with_plan {
            std::fs::write(self.paths.plan_file(draft_id, task_id), "# plan\n").unwrap();
        }

        let mut task_state = TaskStateFile::new(
            task_id.to_string(),
            draft_id.to_string(),
            spec_content_hash(spec),
        );
        task_state.state = state;
        task_state.approval_status = ApprovalStatus::Approved;
        write_task_state(&self.paths, &task_state).unwrap();
    }

    fn run_hive(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_hive"))
            .args(args)
            .current_dir(&self.root)
            .env("HIVE_AUDIT_KEY_PATH", &self.audit_key_path)
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

#[test]
fn exec_skips_missing_plan_without_stalling() {
    let task_id = "test-03TASK";
    let repo = TestRepo::new(&custom_launch_config(task_id));
    let spec = format!(
        "---\nid: {task_id}\ndraft_id: draft-03\ncomplexity: S\napproval_status: approved\nschema_version: 1\n---\nverify-command: true\n"
    );
    repo.write_task_with(task_id, "draft-03", TaskState::Pending, &spec, false);

    let output = repo.run_hive(&["exec"]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("plan not found for task"),
        "{output:?}"
    );

    let state = read_task_state(&repo.paths, task_id).unwrap();
    assert_eq!(state.state, TaskState::Pending);
    assert_eq!(state.retry_count, 0);
}

#[test]
fn exec_skips_dependents_of_missing_plan_tasks() {
    let upstream = "test-03UP";
    let downstream = "test-03DOWN";
    let repo = TestRepo::new(&custom_launch_config(upstream));

    let upstream_spec = format!(
        "---\nid: {upstream}\ndraft_id: draft-03b\ncomplexity: S\napproval_status: approved\nschema_version: 1\n---\nverify-command: true\n"
    );
    repo.write_task_with(upstream, "draft-03b", TaskState::Pending, &upstream_spec, false);

    let downstream_spec = format!(
        "---\nid: {downstream}\ndraft_id: draft-03b\ndepends_on:\n  - {upstream}\ncomplexity: S\napproval_status: approved\nschema_version: 1\n---\nverify-command: true\n"
    );
    repo.write_task_with(
        downstream,
        "draft-03b",
        TaskState::Pending,
        &downstream_spec,
        true,
    );

    let output = repo.run_hive(&["exec"]);
    assert!(output.status.success(), "{output:?}");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("plan not found for task test-03UP, skipping"),
        "{stderr}"
    );
    assert!(
        stderr.contains("task test-03DOWN depends on skipped task test-03UP, skipping"),
        "{stderr}"
    );

    let upstream_state = read_task_state(&repo.paths, upstream).unwrap();
    let downstream_state = read_task_state(&repo.paths, downstream).unwrap();
    assert_eq!(upstream_state.state, TaskState::Pending);
    assert_eq!(downstream_state.state, TaskState::Pending);
}

#[test]
fn exec_fails_fast_on_malformed_spec() {
    let task_id = "test-04TASK";
    let repo = TestRepo::new(&custom_launch_config(task_id));
    let spec = format!(
        "---\nid: {task_id}\ndraft_id: draft-04\ncomplexity: S\napproval_status: approved\ndepends_on: invalid\nschema_version: 1\n---\nverify-command: true\n"
    );
    repo.write_task_with(task_id, "draft-04", TaskState::Pending, &spec, true);

    let output = repo.run_hive(&["exec"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid spec for task"),
        "{output:?}"
    );

    let state = read_task_state(&repo.paths, task_id).unwrap();
    assert_eq!(state.state, TaskState::Pending);
}

#[test]
fn exec_blocks_downstream_when_dependency_blocks() {
    let upstream = "test-05UP";
    let downstream = "test-05DOWN";
    let repo = TestRepo::new(&custom_launch_config(upstream));
    repo.write_task(
        upstream,
        "draft-05",
        TaskState::Pending,
        "verify-command: false",
    );

    let downstream_spec = format!(
        "---\nid: {downstream}\ndraft_id: draft-05\ndepends_on:\n  - {upstream}\ncomplexity: S\napproval_status: approved\nschema_version: 1\n---\nverify-command: true\n"
    );
    repo.write_task_with(
        downstream,
        "draft-05",
        TaskState::Pending,
        &downstream_spec,
        true,
    );

    let output = repo.run_hive(&["exec"]);
    assert!(output.status.success(), "{output:?}");

    let upstream_state = read_task_state(&repo.paths, upstream).unwrap();
    let downstream_state = read_task_state(&repo.paths, downstream).unwrap();
    assert_eq!(upstream_state.state, TaskState::Blocked);
    assert_eq!(downstream_state.state, TaskState::Blocked);
    assert_eq!(downstream_state.retry_count, 0);
}

#[test]
fn merge_rejects_dependency_completed_but_not_merged() {
    let repo = TestRepo::new("launch:\n  tool: custom\n  custom_command: 'true'\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n");

    let dep = "dep-task";
    let child = "child-task";

    // Create dependency task in completed state but NOT merged
    repo.write_task(dep, "draft-merge", TaskState::Completed, "");
    let mut dep_state = read_task_state(&repo.paths, dep).unwrap();
    assert!(!dep_state.merged); // Not yet merged

    // Create child task that depends on dep
    let child_spec = format!(
        "---\nid: {child}\ndraft_id: draft-merge\ndepends_on:\n  - {dep}\ncomplexity: S\napproval_status: approved\nschema_version: 1\n---\n"
    );
    repo.write_task_with(child, "draft-merge", TaskState::Completed, &child_spec, true);

    // Merge child should fail because dep is completed but not merged
    let output = repo.run_hive(&["merge", "--task", child]);
    assert!(
        !output.status.success(),
        "merge should reject when dependency is completed but not merged"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not yet merged"),
        "error should mention 'not yet merged', got: {stderr}"
    );
}

#[test]
fn check_file_verifier_records_detail() {
    let repo = TestRepo::new("launch:\n  tool: custom\n  custom_command: 'true'\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n");
    let task_id = "check-file-task";

    // Create task in review state with file verifier
    repo.write_task(
        task_id,
        "draft-chk",
        TaskState::Review,
        "verify-file: nonexistent.txt",
    );

    let output = repo.run_hive(&["check", "--task", task_id]);
    // Should exit with code 1 (some fail)
    assert_eq!(output.status.code(), Some(1));

    // Check that results file was written with "file not found" detail
    let results_path = repo.paths.task_dir(task_id).join("check-results.md");
    let results = std::fs::read_to_string(&results_path).unwrap_or_default();
    assert!(
        results.contains("file not found"),
        "check-results.md should contain 'file not found', got: {results}"
    );
}

#[test]
fn approve_works_from_spec_only_without_state_json() {
    let repo = TestRepo::new("launch:\n  tool: custom\n  custom_command: 'true'\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n");
    let task_id = "spec-only-task";
    let draft_id = "draft-approve";

    // Write spec file directly without creating state.json
    let spec = format!(
        "---\nid: {task_id}\ndraft_id: {draft_id}\ncomplexity: S\nschema_version: 1\n---\nGoal\n"
    );
    std::fs::write(repo.paths.spec_file(task_id), &spec).unwrap();

    // Also need a plan file
    let plan_dir = repo.paths.plans_dir().join(draft_id);
    std::fs::create_dir_all(&plan_dir).unwrap();
    std::fs::write(
        repo.paths.plan_file(draft_id, task_id),
        "# plan\n",
    )
    .unwrap();

    // Approve should succeed even without pre-existing state.json
    let output = repo.run_hive(&["approve", "--draft", draft_id]);
    assert!(
        output.status.success(),
        "approve should work from spec-only draft: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify state was bootstrapped and approved
    let state = read_task_state(&repo.paths, task_id).unwrap();
    assert_eq!(state.approval_status, ApprovalStatus::Approved);
}

#[test]
fn approve_idempotent_no_state_change() {
    let config = "launch:\n  tool: custom\n  custom_command: 'true'\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n";
    let repo = TestRepo::new(config);
    let task_id = "idem-task";
    let draft_id = "draft-idem";

    // Write spec and bootstrap state via first approve
    let spec = format!(
        "---\nid: {task_id}\ndraft_id: {draft_id}\ncomplexity: S\nschema_version: 1\n---\nGoal\n"
    );
    std::fs::write(repo.paths.spec_file(task_id), &spec).unwrap();
    let plan_dir = repo.paths.plans_dir().join(draft_id);
    std::fs::create_dir_all(&plan_dir).unwrap();
    std::fs::write(repo.paths.plan_file(draft_id, task_id), "# plan\n").unwrap();

    let output1 = repo.run_hive(&["approve", "--draft", draft_id]);
    assert!(output1.status.success());

    let state_after_first = read_task_state(&repo.paths, task_id).unwrap();
    assert_eq!(state_after_first.approval_status, ApprovalStatus::Approved);
    let updated1 = state_after_first.updated_at;

    // Second approve should be idempotent
    let output2 = repo.run_hive(&["approve", "--draft", draft_id]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("already approved"),
        "second approve should say already approved, got: {stdout2}"
    );

    // State should not have changed
    let state_after_second = read_task_state(&repo.paths, task_id).unwrap();
    assert_eq!(state_after_second.updated_at, updated1);
}

#[test]
fn check_command_verifier_records_stderr_detail() {
    let config = "launch:\n  tool: custom\n  custom_command: 'true'\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n";
    let repo = TestRepo::new(config);
    let task_id = "check-cmd-task";

    // Create task in review state with a command that fails and prints to stderr
    repo.write_task(
        task_id,
        "draft-cmd",
        TaskState::Review,
        "verify-command: echo 'test-error-output' >&2 && false",
    );

    let output = repo.run_hive(&["check", "--task", task_id]);
    assert_eq!(output.status.code(), Some(1));

    // Check that results file captures the stderr detail
    let results_path = repo.paths.task_dir(task_id).join("check-results.md");
    let results = std::fs::read_to_string(&results_path).unwrap_or_default();
    assert!(
        results.contains("stderr:") || results.contains("test-error-output"),
        "check-results.md should contain stderr detail, got: {results}"
    );
}

#[test]
fn check_manual_verifier_records_rejection_reason() {
    let config = "launch:\n  tool: custom\n  custom_command: 'true'\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n";
    let repo = TestRepo::new(config);
    let task_id = "check-manual-task";

    repo.write_task(
        task_id,
        "draft-manual",
        TaskState::Review,
        "verify-manual: Does the output look correct?",
    );

    // Run hive check with stdin piped: "n\nmy rejection reason\n"
    let hive_bin = env!("CARGO_BIN_EXE_hive");
    let mut child = std::process::Command::new(hive_bin)
        .args(["check", "--task", task_id])
        .current_dir(&repo.root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    // Write rejection to stdin
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, "n").unwrap();
        writeln!(stdin, "my rejection reason").unwrap();
    }

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1), "check should exit 1 on manual rejection");

    let results_path = repo.paths.task_dir(task_id).join("check-results.md");
    let results = std::fs::read_to_string(&results_path).unwrap_or_default();
    assert!(
        results.contains("reason:") || results.contains("my rejection reason"),
        "check-results.md should contain manual rejection reason, got: {results}"
    );
}

#[test]
fn doctor_detects_tampered_audit() {
    let config = "launch:\n  tool: custom\n  custom_command: 'true'\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n";
    let repo = TestRepo::new(config);
    let task_id = "tamper-task";

    // Create a task and write some audit entries via the CLI path
    repo.write_task(task_id, "draft-tamper", TaskState::InProgress, "");

    // Write a legitimate audit entry (set key path for in-process call, restore after)
    let old_key = std::env::var("HIVE_AUDIT_KEY_PATH").ok();
    unsafe { std::env::set_var("HIVE_AUDIT_KEY_PATH", &repo.audit_key_path) };
    hive_audit::log_state_change(
        &repo.paths.audit_file(task_id),
        hive_core::config::AuditLevel::Standard,
        task_id,
        "pending",
        "assigned",
    )
    .unwrap();
    // Restore
    match old_key {
        Some(v) => unsafe { std::env::set_var("HIVE_AUDIT_KEY_PATH", v) },
        None => unsafe { std::env::remove_var("HIVE_AUDIT_KEY_PATH") },
    }

    // Verify doctor passes on untampered file
    let output1 = repo.run_hive(&["doctor"]);
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(
        !stdout1.contains("invalid integrity hash"),
        "untampered audit should pass doctor"
    );

    // Now tamper with the audit file: rewrite content but preserve format
    let audit_path = repo.paths.audit_file(task_id);
    let content = std::fs::read_to_string(&audit_path).unwrap();
    let tampered = content.replace("pending -> assigned", "pending -> TAMPERED");
    std::fs::write(&audit_path, &tampered).unwrap();

    // Doctor should detect the tampering via integrity hash mismatch
    let output2 = repo.run_hive(&["doctor"]);
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("invalid integrity hash"),
        "tampered audit should be detected by doctor, got: {stdout2}"
    );
}

#[test]
fn doctor_detects_missing_footer_audit() {
    let config = "launch:\n  tool: custom\n  custom_command: 'true'\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n";
    let repo = TestRepo::new(config);
    let task_id = "no-footer-task";

    repo.write_task(task_id, "draft-nf", TaskState::InProgress, "");

    // Write audit file directly (simulating external/worker write) — no integrity footer
    let audit_path = repo.paths.audit_file(task_id);
    std::fs::create_dir_all(audit_path.parent().unwrap()).unwrap();
    std::fs::write(
        &audit_path,
        "# Audit Log\n\n- [2024-01-01 00:00:00 UTC] [state_change] fake entry\n",
    )
    .unwrap();

    let output = repo.run_hive(&["doctor"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("invalid integrity hash") || stdout.contains("content was modified"),
        "missing-footer audit should be flagged by doctor, got: {stdout}"
    );
}

#[test]
fn merge_all_non_direct_downstream_skipped() {
    let config = "launch:\n  tool: custom\n  custom_command: 'true'\nrfc:\n  platform: none\naudit_level: standard\nskills:\n  default: []\n";
    let repo = TestRepo::new(config);

    let upstream = "up-merge-all";
    let downstream = "down-merge-all";

    // Create upstream completed task
    repo.write_task(upstream, "draft-ma", TaskState::Completed, "");

    // Create downstream that depends on upstream
    let down_spec = format!(
        "---\nid: {downstream}\ndraft_id: draft-ma\ndepends_on:\n  - {upstream}\ncomplexity: S\napproval_status: approved\nschema_version: 1\n---\n"
    );
    repo.write_task_with(downstream, "draft-ma", TaskState::Completed, &down_spec, true);

    // Create task branches so merge can rebase them
    git(&repo.root, &["branch", &format!("hive/{upstream}"), "HEAD"]);
    git(&repo.root, &["branch", &format!("hive/{downstream}"), "HEAD"]);

    // Run merge --all (default mode is "pr" with platform: none -> branch ready for review)
    let output = repo.run_hive(&["merge", "--all"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}{stderr}");

    // Upstream should be "pending review / not yet on main" (not actually merged)
    // Downstream should be skipped because upstream is not in merged set
    // Must see downstream-specific skip message
    assert!(
        combined.contains(&format!("skipping {downstream}"))
            || combined.contains(&format!("dependencies not yet merged: {upstream}")),
        "merge --all must show downstream-specific skip with upstream name, got: {combined}"
    );

    // Verify upstream is NOT marked as merged in state.json
    let up_state = read_task_state(&repo.paths, upstream).unwrap();
    assert!(
        !up_state.merged,
        "upstream should not be marked merged in non-direct mode"
    );

    // Verify downstream state has NOT changed (no merge attempt side effects)
    let down_state = read_task_state(&repo.paths, downstream).unwrap();
    assert!(
        !down_state.merged,
        "downstream should not be marked merged when upstream isn't"
    );
    assert_eq!(
        down_state.state,
        TaskState::Completed,
        "downstream state should remain completed (not blocked or failed)"
    );
}
