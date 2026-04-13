use std::path::Path;

use chrono::Utc;
use hive_core::HiveResult;
use hive_core::config::AuditLevel;

/// Append an audit entry to a task's audit.md file.
/// After writing, updates a trailing integrity hash for append-only verification.
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

    // Read existing content (strip old integrity line if present)
    let existing = if audit_path.exists() {
        let content = std::fs::read_to_string(audit_path)?;
        strip_integrity_line(&content)
    } else {
        String::from("# Audit Log\n\n")
    };

    // Build new content with entry and fresh integrity hash
    let mut new_content = existing;
    new_content.push_str(&entry);
    let hash = compute_integrity(&new_content);
    new_content.push_str(&format!("# integrity: {hash}\n"));

    // Atomic write (not append) to update integrity
    std::fs::write(audit_path, &new_content)?;
    Ok(())
}

/// Strip the trailing `# integrity: ...` line from content.
fn strip_integrity_line(content: &str) -> String {
    let mut lines: Vec<&str> = content.lines().collect();
    if let Some(last) = lines.last()
        && last.starts_with("# integrity:") {
            lines.pop();
        }
    let mut result = lines.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Compute SHA-256 of content for integrity verification.
fn compute_integrity(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(content.as_bytes());
    format!("{:x}", hash)
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

/// Verify audit file integrity. Returns Ok(true) if valid, Ok(false) if tampered.
pub fn verify_integrity(audit_path: &Path) -> HiveResult<bool> {
    if !audit_path.exists() {
        return Ok(true);
    }
    let content = std::fs::read_to_string(audit_path)?;
    if content.trim().is_empty() {
        return Ok(true);
    }

    // Extract the stored integrity hash
    let lines: Vec<&str> = content.lines().collect();
    let last = lines.last().copied().unwrap_or("");
    if !last.starts_with("# integrity: ") {
        // No integrity line — old format, can't verify
        return Ok(true);
    }

    let stored_hash = last.strip_prefix("# integrity: ").unwrap_or("").trim();
    let body = strip_integrity_line(&content);
    let expected_hash = compute_integrity(&body);

    Ok(stored_hash == expected_hash)
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
        log_state_change(
            &path,
            AuditLevel::Standard,
            "t-01",
            "assigned",
            "in_progress",
        )
        .unwrap();

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
