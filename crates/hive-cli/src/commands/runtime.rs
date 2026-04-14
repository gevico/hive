use anyhow::Result;
use hive_core::config;
use hive_core::storage::HivePaths;

#[derive(Debug, Clone)]
pub(crate) struct CommandFailure {
    pub exit_code: i32,
    pub message: String,
}

impl CommandFailure {
    pub(crate) fn new(exit_code: i32, message: impl Into<String>) -> Self {
        Self {
            exit_code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CommandFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CommandFailure {}

pub(crate) fn log_state_change(
    paths: &HivePaths,
    task_id: &str,
    from: &str,
    to: &str,
) -> Result<()> {
    let config = config::load_config(&paths.hive_dir())?;
    hive_audit::log_state_change(
        &paths.audit_file(task_id),
        config.audit_level,
        task_id,
        from,
        to,
    )?;
    Ok(())
}
