use thiserror::Error;

#[derive(Debug, Error)]
pub enum HiveError {
    #[error("not a hive project (missing .hive/ directory). Run `hive init` first")]
    NotInitialized,

    #[error("not a git repository")]
    NotGitRepo,

    #[error("already initialized")]
    AlreadyInitialized,

    #[error("config error: {0}")]
    Config(String),

    #[error("invalid state transition: {from} → {to}")]
    InvalidTransition { from: String, to: String },

    #[error("dependency not met: {0}")]
    DependencyNotMet(String),

    #[error("retry limit exceeded for task {0}")]
    RetryLimitExceeded(String),

    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("draft not found: {0}")]
    DraftNotFound(String),

    #[error("lock acquisition failed: {0}")]
    LockFailed(String),

    #[error("orchestrator already running")]
    OrchestratorLocked,

    #[error("schema validation error: {0}")]
    SchemaValidation(String),

    #[error("unsupported schema version: {0}")]
    UnsupportedSchemaVersion(u32),

    #[error("frontmatter parse error: {0}")]
    FrontmatterParse(String),

    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("invalid field value: {field}: {reason}")]
    InvalidFieldValue { field: String, reason: String },

    #[error("spec error: {0}")]
    Spec(String),

    #[error("plan not found for task {0}")]
    PlanNotFound(String),

    #[error("worktree error: {0}")]
    Worktree(String),

    #[error("worktree already exists for task {0}")]
    WorktreeExists(String),

    #[error("agent tool not found: {0}")]
    AgentToolNotFound(String),

    #[error("merge conflict in task {0}")]
    MergeConflict(String),

    #[error("circular dependency detected: {0}")]
    CircularDependency(String),

    #[error("audit error: {0}")]
    Audit(String),

    #[error("skill error: {0}")]
    Skill(String),

    #[error("constraint violation: {0}")]
    ConstraintViolation(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("git error: {0}")]
    Git(String),

    #[error("{0}")]
    Other(String),
}

pub type HiveResult<T> = Result<T, HiveError>;
