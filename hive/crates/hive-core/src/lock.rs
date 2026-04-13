use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::{HiveError, HiveResult};

const STALE_LOCK_AGE: Duration = Duration::from_secs(5 * 60);

/// A file-based lock using flock(2) for cross-process exclusion
/// and PID tracking for stale lock detection.
pub struct FileLock {
    #[allow(dead_code)]
    file: File,
}

impl FileLock {
    /// Acquire an exclusive lock. Non-blocking: returns error immediately if held.
    pub fn try_acquire(path: &Path) -> HiveResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        check_and_remove_stale(path);

        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| HiveError::LockFailed(format!("{}: {e}", path.display())))?;

        // Non-blocking exclusive flock
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if ret != 0 {
            return Err(HiveError::LockFailed(format!(
                "lock held by another process: {}",
                path.display()
            )));
        }

        // Write PID (truncate after acquiring lock)
        let mut file = file;
        file.set_len(0)
            .map_err(|e| HiveError::LockFailed(format!("truncate: {e}")))?;
        let _ = write!(file, "{}", std::process::id());
        let _ = file.flush();

        Ok(Self { file })
    }

    /// Create a second handle for same-file locking test (cross-fd within same process).
    /// This opens a NEW file descriptor so flock treats it as a separate lock.
    #[cfg(test)]
    pub fn try_acquire_separate_fd(path: &Path) -> HiveResult<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| HiveError::LockFailed(format!("{}: {e}", path.display())))?;

        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if ret != 0 {
            return Err(HiveError::LockFailed(format!(
                "lock held: {}",
                path.display()
            )));
        }

        Ok(Self { file })
    }
}

fn check_and_remove_stale(path: &Path) {
    if !path.exists() {
        return;
    }
    if let Ok(content) = std::fs::read_to_string(path)
        && let Some(pid_str) = content.lines().next()
        && let Ok(pid) = pid_str.trim().parse::<u32>()
    {
        let pid_alive = Path::new(&format!("/proc/{pid}")).exists();
        if !pid_alive
            && let Ok(metadata) = std::fs::metadata(path)
            && let Ok(modified) = metadata.modified()
            && let Ok(age) = SystemTime::now().duration_since(modified)
            && age > STALE_LOCK_AGE
        {
            eprintln!(
                "warning: removing stale lock {} (pid {pid} dead, age {:.0}s)",
                path.display(),
                age.as_secs_f64()
            );
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Orchestrator-level lock to prevent double-exec.
pub struct OrchestratorLock {
    _lock: FileLock,
}

impl OrchestratorLock {
    pub fn acquire(lock_path: &Path) -> HiveResult<Self> {
        let lock = FileLock::try_acquire(lock_path).map_err(|_| HiveError::OrchestratorLocked)?;
        Ok(Self { _lock: lock })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn acquire_and_release_lock() {
        let tmp = TempDir::new().unwrap();
        let lock_path = tmp.path().join("test.lock");
        {
            let _lock = FileLock::try_acquire(&lock_path).unwrap();
            assert!(lock_path.exists());
        }
        // After drop, flock is released — a new FD can acquire it
        let _lock2 = FileLock::try_acquire(&lock_path).unwrap();
    }

    #[test]
    fn double_lock_same_process_different_fd() {
        let tmp = TempDir::new().unwrap();
        let lock_path = tmp.path().join("test.lock");
        let _lock1 = FileLock::try_acquire(&lock_path).unwrap();
        // Open a separate FD to simulate a different "process" within the same test.
        // Note: on Linux, flock is per open-file-description, so a new open() creates
        // a new file description and flock will block/fail.
        let result = FileLock::try_acquire_separate_fd(&lock_path);
        // On Linux, same-process different FD: flock should conflict.
        // This behavior varies by OS, but on Linux it works.
        assert!(result.is_err(), "second FD should fail to acquire flock");
    }

    #[test]
    fn orchestrator_lock_blocks_second() {
        let tmp = TempDir::new().unwrap();
        let lock_path = tmp.path().join("orchestrator.lock");
        let _lock = OrchestratorLock::acquire(&lock_path).unwrap();
        // Second lock attempt via a different FD path
        let file = OpenOptions::new().write(true).open(&lock_path).unwrap();
        use std::os::unix::io::AsRawFd;
        let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        // Should fail because first lock is held
        assert_ne!(ret, 0, "second flock attempt should fail");
    }

    #[test]
    fn lock_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let lock_path = tmp.path().join("nested").join("dir").join("test.lock");
        let _lock = FileLock::try_acquire(&lock_path).unwrap();
        assert!(lock_path.exists());
    }
}
