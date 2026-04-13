use std::path::Path;

use chrono::Utc;
use hive_core::config::AuditLevel;
use hive_core::HiveResult;

/// Append an audit entry to a task's audit.md file.
pub fn append_entry(
    audit_path: &Path,
    level: AuditLevel,
    event_type: &str,
    detail: &str,
    min_level: AuditLevel,
) -> HiveResult<()> {
    if !should_log(level, min_level) {
        return Ok(());
    }

    let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let entry = format!("- [{timestamp}] [{event_type}] {detail}\n");

    if let Some(parent) = audit_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Append-only write
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(audit_path)?;

    // If file is new, write header
    if file.metadata()?.len() == 0 {
        writeln!(file, "# Audit Log\n")?;
    }

    write!(file, "{entry}")?;
    Ok(())
}

fn should_log(entry_level: AuditLevel, config_level: AuditLevel) -> bool {
    let rank = |l: AuditLevel| match l {
        AuditLevel::Minimal => 0,
        AuditLevel::Standard => 1,
        AuditLevel::Full => 2,
    };
    rank(entry_level) <= rank(config_level)
}

/// Log a state change event (minimal level).
pub fn log_state_change(
    audit_path: &Path,
    config_level: AuditLevel,
    task_id: &str,
    from: &str,
    to: &str,
) -> HiveResult<()> {
    append_entry(
        audit_path,
        AuditLevel::Minimal,
        "state_change",
        &format!("{task_id}: {from} -> {to}"),
        config_level,
    )
}

/// Log a merge event (minimal level).
pub fn log_merge(
    audit_path: &Path,
    config_level: AuditLevel,
    task_id: &str,
    detail: &str,
) -> HiveResult<()> {
    append_entry(
        audit_path,
        AuditLevel::Minimal,
        "merge",
        &format!("{task_id}: {detail}"),
        config_level,
    )
}

/// Log an RLCR round summary (standard level).
pub fn log_round_summary(
    audit_path: &Path,
    config_level: AuditLevel,
    task_id: &str,
    round: u32,
    summary: &str,
) -> HiveResult<()> {
    append_entry(
        audit_path,
        AuditLevel::Standard,
        "rlcr_round",
        &format!("{task_id} round {round}: {summary}"),
        config_level,
    )
}

/// Log agent decision rationale (full level).
pub fn log_decision(
    audit_path: &Path,
    config_level: AuditLevel,
    task_id: &str,
    decision: &str,
) -> HiveResult<()> {
    append_entry(
        audit_path,
        AuditLevel::Full,
        "decision",
        &format!("{task_id}: {decision}"),
        config_level,
    )
}

/// Read audit log for display.
pub fn read_audit(audit_path: &Path) -> HiveResult<String> {
    if !audit_path.exists() {
        return Ok("(no audit entries)".into());
    }
    Ok(std::fs::read_to_string(audit_path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn append_and_read() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("audit.md");

        log_state_change(&path, AuditLevel::Standard, "t-01", "pending", "assigned").unwrap();
        log_state_change(&path, AuditLevel::Standard, "t-01", "assigned", "in_progress").unwrap();

        let content = read_audit(&path).unwrap();
        assert!(content.contains("pending -> assigned"));
        assert!(content.contains("assigned -> in_progress"));
    }

    #[test]
    fn level_filtering() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("audit.md");

        // Config at minimal — standard-level events should be skipped
        log_round_summary(&path, AuditLevel::Minimal, "t-01", 1, "summary").unwrap();
        let content = read_audit(&path).unwrap();
        assert!(!content.contains("summary"));

        // Config at full — all events logged
        log_decision(&path, AuditLevel::Full, "t-01", "chose approach A").unwrap();
        let content = read_audit(&path).unwrap();
        assert!(content.contains("chose approach A"));
    }

    #[test]
    fn append_only() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("audit.md");

        log_state_change(&path, AuditLevel::Standard, "t-01", "a", "b").unwrap();
        let len1 = std::fs::read_to_string(&path).unwrap().len();

        log_state_change(&path, AuditLevel::Standard, "t-01", "b", "c").unwrap();
        let len2 = std::fs::read_to_string(&path).unwrap().len();

        assert!(len2 > len1, "audit file should grow, not be replaced");
    }
}
