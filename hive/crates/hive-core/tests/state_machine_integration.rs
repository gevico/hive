use hive_core::state::{TaskState, TransitionAction};
use hive_core::storage::{HivePaths, TaskStateFile, read_task_state, write_task_state};
use hive_core::task;

use tempfile::TempDir;

fn setup_hive() -> (TempDir, HivePaths) {
    let tmp = TempDir::new().unwrap();
    let paths = HivePaths::new(tmp.path());
    for dir in paths.required_dirs() {
        std::fs::create_dir_all(&dir).unwrap();
    }
    // Write minimal config for commands that load it
    std::fs::write(
        paths.config_yml(),
        "launch:\n  tool: custom\nrfc:\n  platform: none\naudit_level: standard\n",
    )
    .unwrap();
    (tmp, paths)
}

fn create_task(paths: &HivePaths, task_id: &str, draft_id: &str) {
    let state = TaskStateFile::new(task_id.into(), draft_id.into(), "testhash".into());
    write_task_state(paths, &state).unwrap();
}

#[test]
fn full_state_transition_happy_path() {
    let state = TaskState::Pending;

    // pending -> assigned
    let state = state.transition(TransitionAction::Assign, 0, true).unwrap();
    assert_eq!(state, TaskState::Assigned);

    // assigned -> in_progress
    let state = state.transition(TransitionAction::Start, 0, true).unwrap();
    assert_eq!(state, TaskState::InProgress);

    // in_progress -> review
    let state = state
        .transition(TransitionAction::SubmitForReview, 0, true)
        .unwrap();
    assert_eq!(state, TaskState::Review);

    // review -> completed
    let state = state
        .transition(TransitionAction::Complete, 0, true)
        .unwrap();
    assert_eq!(state, TaskState::Completed);
}

#[test]
fn retry_cycle() {
    let state = TaskState::InProgress;

    // in_progress -> failed
    let state = state.transition(TransitionAction::Fail, 0, true).unwrap();
    assert_eq!(state, TaskState::Failed);

    // failed -> pending (retry, count=0 < 3)
    let state = state.transition(TransitionAction::Retry, 0, true).unwrap();
    assert_eq!(state, TaskState::Pending);

    // Simulate going back through to failed again
    let state = state.transition(TransitionAction::Assign, 0, true).unwrap();
    let state = state.transition(TransitionAction::Start, 0, true).unwrap();
    let state = state.transition(TransitionAction::Fail, 0, true).unwrap();

    // retry count=1 < 3, still ok
    let state = state.transition(TransitionAction::Retry, 1, true).unwrap();
    assert_eq!(state, TaskState::Pending);
}

#[test]
fn retry_limit_blocks() {
    let state = TaskState::Failed;

    // retry count=3 >= 3, should fail
    let result = state.transition(TransitionAction::Retry, 3, true);
    assert!(result.is_err());

    // Can still block explicitly
    let blocked = state.transition(TransitionAction::Block, 3, true).unwrap();
    assert_eq!(blocked, TaskState::Blocked);

    // Blocked can be unblocked manually
    let pending = blocked
        .transition(TransitionAction::Unblock, 0, true)
        .unwrap();
    assert_eq!(pending, TaskState::Pending);
}

#[test]
fn state_persistence_roundtrip() {
    let (_tmp, paths) = setup_hive();
    create_task(&paths, "test-001", "draft-001");

    let loaded = read_task_state(&paths, "test-001").unwrap();
    assert_eq!(loaded.task_id, "test-001");
    assert_eq!(loaded.state, TaskState::Pending);
    assert_eq!(loaded.retry_count, 0);
}

#[test]
fn task_not_found() {
    let (_tmp, paths) = setup_hive();
    let result = read_task_state(&paths, "nonexistent");
    assert!(result.is_err());
}

#[test]
fn spec_content_hash_is_sha256() {
    let hash = task::spec_content_hash("hello world");
    assert_eq!(hash.len(), 8);
    // SHA-256 of "hello world" starts with b94d27b9
    assert_eq!(hash, "b94d27b9");
}

#[test]
fn invalid_transition_shows_target_state() {
    let result = TaskState::Pending.transition(TransitionAction::Complete, 0, true);
    match result {
        Err(hive_core::HiveError::InvalidTransition { from, to }) => {
            assert_eq!(from, "pending");
            assert_eq!(to, "completed");
        }
        _ => panic!("expected InvalidTransition error"),
    }
}

#[test]
fn invalid_transition_pending_to_review() {
    let result = TaskState::Pending.transition(TransitionAction::SubmitForReview, 0, true);
    match result {
        Err(hive_core::HiveError::InvalidTransition { from, to }) => {
            assert_eq!(from, "pending");
            assert_eq!(to, "review");
        }
        _ => panic!("expected InvalidTransition error"),
    }
}

#[test]
fn depends_on_string_rejected_in_spec() {
    let content = "---\nid: t\ndraft_id: d\ncomplexity: S\ndepends_on: not-a-list\n---\n";
    let result = task::parse_spec(content);
    assert!(result.is_err());
}

#[test]
fn spec_sha256_deterministic() {
    let h1 = task::spec_content_hash("test");
    let h2 = task::spec_content_hash("test");
    assert_eq!(h1, h2);
}

#[test]
fn schema_version_string_type_rejected() {
    // schema_version: "1" (string) must be rejected, not defaulted
    let content = "---\nid: t\ndraft_id: d\ncomplexity: S\nschema_version: \"1\"\n---\n";
    let result = task::parse_spec(content);
    assert!(result.is_err(), "string-typed schema_version should be rejected");
}

#[test]
fn rlcr_max_rounds_string_type_rejected() {
    let content =
        "---\nid: t\ndraft_id: d\ncomplexity: S\nschema_version: 1\nrlcr_max_rounds: \"3\"\n---\n";
    let result = task::parse_spec(content);
    assert!(result.is_err(), "string-typed rlcr_max_rounds should be rejected");
}

#[test]
fn rlcr_max_rounds_exceeding_limit_rejected() {
    // S complexity allows max 2 rounds
    let content =
        "---\nid: t\ndraft_id: d\ncomplexity: S\nschema_version: 1\nrlcr_max_rounds: 5\n---\n";
    let result = task::parse_spec(content);
    assert!(
        matches!(result, Err(hive_core::HiveError::ConstraintViolation(_))),
        "rlcr_max_rounds exceeding complexity limit should be rejected"
    );
}

#[test]
fn rlcr_max_rounds_within_limit_accepted() {
    let content =
        "---\nid: t\ndraft_id: d\ncomplexity: M\nschema_version: 1\nrlcr_max_rounds: 3\n---\n";
    let spec = task::parse_spec(content).unwrap();
    assert_eq!(spec.complexity, task::Complexity::M);
}

#[test]
fn state_json_without_merged_field_loads() {
    // Test backward compatibility: old state.json without "merged" field
    let (_tmp, paths) = setup_hive();
    create_task(&paths, "old-task", "d1");
    // Read back — merged should default to false
    let state = read_task_state(&paths, "old-task").unwrap();
    assert!(!state.merged);
}
