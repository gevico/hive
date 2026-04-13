use std::fmt;

use serde::{Deserialize, Serialize};

use crate::HiveError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Assigned,
    InProgress,
    Review,
    Completed,
    Failed,
    Blocked,
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Assigned => write!(f, "assigned"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Review => write!(f, "review"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionAction {
    Assign,
    Start,
    SubmitForReview,
    Fail,
    Complete,
    Block,
    Retry,
    Unblock,
}

const RETRY_LIMIT: u32 = 3;

impl TaskState {
    /// Validate and return the next state for a given action.
    /// `retry_count` is required for retry transitions.
    /// `deps_completed` indicates whether all dependency tasks are completed.
    pub fn transition(
        self,
        action: TransitionAction,
        retry_count: u32,
        deps_completed: bool,
    ) -> Result<Self, HiveError> {
        match (self, action) {
            // pending → assigned (requires deps completed)
            (Self::Pending, TransitionAction::Assign) => {
                if !deps_completed {
                    return Err(HiveError::DependencyNotMet(
                        "not all dependency tasks are completed".into(),
                    ));
                }
                Ok(Self::Assigned)
            }

            // assigned → in_progress
            (Self::Assigned, TransitionAction::Start) => Ok(Self::InProgress),

            // in_progress → review
            (Self::InProgress, TransitionAction::SubmitForReview) => Ok(Self::Review),

            // in_progress → failed
            (Self::InProgress, TransitionAction::Fail) => Ok(Self::Failed),

            // review → completed
            (Self::Review, TransitionAction::Complete) => Ok(Self::Completed),

            // review → failed
            (Self::Review, TransitionAction::Fail) => Ok(Self::Failed),

            // failed → pending (retry, if under limit)
            (Self::Failed, TransitionAction::Retry) => {
                if retry_count >= RETRY_LIMIT {
                    return Err(HiveError::RetryLimitExceeded(format!(
                        "retry count {retry_count} >= limit {RETRY_LIMIT}"
                    )));
                }
                Ok(Self::Pending)
            }

            // failed → blocked (when retry limit reached)
            (Self::Failed, TransitionAction::Block) => Ok(Self::Blocked),

            // blocked → pending (manual unblock)
            (Self::Blocked, TransitionAction::Unblock) => Ok(Self::Pending),

            _ => {
                let target = match action {
                    TransitionAction::Assign => "assigned",
                    TransitionAction::Start => "in_progress",
                    TransitionAction::SubmitForReview => "review",
                    TransitionAction::Fail => "failed",
                    TransitionAction::Complete => "completed",
                    TransitionAction::Block => "blocked",
                    TransitionAction::Retry => "pending",
                    TransitionAction::Unblock => "pending",
                };
                Err(HiveError::InvalidTransition {
                    from: self.to_string(),
                    to: target.to_string(),
                })
            }
        }
    }

    /// Auto-transition failed task: retry if under limit, block otherwise.
    pub fn auto_retry_or_block(self, retry_count: u32) -> Result<Self, HiveError> {
        if self != Self::Failed {
            return Err(HiveError::InvalidTransition {
                from: self.to_string(),
                to: "auto_retry_or_block".into(),
            });
        }
        if retry_count < RETRY_LIMIT {
            Ok(Self::Pending)
        } else {
            Ok(Self::Blocked)
        }
    }
}

pub const fn retry_limit() -> u32 {
    RETRY_LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_to_assigned_with_deps() {
        let result = TaskState::Pending.transition(TransitionAction::Assign, 0, true);
        assert_eq!(result.unwrap(), TaskState::Assigned);
    }

    #[test]
    fn pending_to_assigned_without_deps() {
        let result = TaskState::Pending.transition(TransitionAction::Assign, 0, false);
        assert!(matches!(result, Err(HiveError::DependencyNotMet(_))));
    }

    #[test]
    fn assigned_to_in_progress() {
        let result = TaskState::Assigned.transition(TransitionAction::Start, 0, true);
        assert_eq!(result.unwrap(), TaskState::InProgress);
    }

    #[test]
    fn in_progress_to_review() {
        let result = TaskState::InProgress.transition(TransitionAction::SubmitForReview, 0, true);
        assert_eq!(result.unwrap(), TaskState::Review);
    }

    #[test]
    fn in_progress_to_failed() {
        let result = TaskState::InProgress.transition(TransitionAction::Fail, 0, true);
        assert_eq!(result.unwrap(), TaskState::Failed);
    }

    #[test]
    fn review_to_completed() {
        let result = TaskState::Review.transition(TransitionAction::Complete, 0, true);
        assert_eq!(result.unwrap(), TaskState::Completed);
    }

    #[test]
    fn review_to_failed() {
        let result = TaskState::Review.transition(TransitionAction::Fail, 0, true);
        assert_eq!(result.unwrap(), TaskState::Failed);
    }

    #[test]
    fn failed_to_pending_retry_under_limit() {
        let result = TaskState::Failed.transition(TransitionAction::Retry, 2, true);
        assert_eq!(result.unwrap(), TaskState::Pending);
    }

    #[test]
    fn failed_to_pending_retry_at_limit() {
        let result = TaskState::Failed.transition(TransitionAction::Retry, 3, true);
        assert!(matches!(result, Err(HiveError::RetryLimitExceeded(_))));
    }

    #[test]
    fn failed_to_blocked() {
        let result = TaskState::Failed.transition(TransitionAction::Block, 3, true);
        assert_eq!(result.unwrap(), TaskState::Blocked);
    }

    #[test]
    fn blocked_to_pending_unblock() {
        let result = TaskState::Blocked.transition(TransitionAction::Unblock, 0, true);
        assert_eq!(result.unwrap(), TaskState::Pending);
    }

    // Negative tests: invalid transitions
    #[test]
    fn pending_to_review_rejected() {
        let result = TaskState::Pending.transition(TransitionAction::SubmitForReview, 0, true);
        assert!(matches!(result, Err(HiveError::InvalidTransition { .. })));
    }

    #[test]
    fn pending_to_completed_rejected() {
        let result = TaskState::Pending.transition(TransitionAction::Complete, 0, true);
        assert!(matches!(result, Err(HiveError::InvalidTransition { .. })));
    }

    #[test]
    fn assigned_to_completed_rejected() {
        let result = TaskState::Assigned.transition(TransitionAction::Complete, 0, true);
        assert!(matches!(result, Err(HiveError::InvalidTransition { .. })));
    }

    #[test]
    fn auto_retry_under_limit() {
        let result = TaskState::Failed.auto_retry_or_block(2);
        assert_eq!(result.unwrap(), TaskState::Pending);
    }

    #[test]
    fn auto_block_at_limit() {
        let result = TaskState::Failed.auto_retry_or_block(3);
        assert_eq!(result.unwrap(), TaskState::Blocked);
    }
}
