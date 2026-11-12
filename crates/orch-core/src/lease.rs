//! The single-driver lease: one process drives a run at a time, on
//! `<main repo>/.orchestrate/driver.lock` (`fcntl.flock` in Python; `flock` via `fs2` here).

use crate::error::EXIT_USAGE;
use crate::{Error, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, thiserror::Error)]
pub enum LeaseError {
    /// Another process holds the lease; the message names pid, host and start time.
    #[error("this run is already driven by {0}")]
    Held(String),
    #[error("could not {op} {path}: {source}")]
    Io {
        op: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl From<LeaseError> for Error {
    fn from(e: LeaseError) -> Self {
        match e {
            LeaseError::Held(_) => Error::wrap(e, EXIT_USAGE),
            LeaseError::Io { .. } => Error::wrap(e, crate::error::EXIT_ERROR),
        }
    }
}

/// What the driver writes into `driver.lock` (`{"pid","host","started","token"}`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DriverInfo {
    /// Process id of the holder.
    #[serde(default)]
    pub pid: Option<i64>,
    /// Host the holder runs on.
    #[serde(default)]
    pub host: Option<String>,
    /// Epoch seconds when the holder took the lease.
    #[serde(default)]
    pub started: Option<i64>,
    /// Random token, also put into `ORCH_DRIVER` for the holder's own subcommands.
    #[serde(default)]
    pub token: Option<String>,
}

impl DriverInfo {
    /// `orch autopilot (pid 123 on host, started 14:05:09)`, with `?` for missing fields
    /// (Python's `driver_line`). The start time is UTC.
    pub fn holder_line(&self) -> String {
        let started = self.started.map(hhmmss).unwrap_or_else(|| "?".to_string());
        format!(
            "orch autopilot (pid {} on {}, started {})",
            self.pid.map_or("?".to_string(), |p| p.to_string()),
            self.host.as_deref().unwrap_or("?"),
            started
        )
    }
}

/// `driver_info`: the JSON in `driver.lock`, or an empty info when the file is missing
/// or broken.
pub fn driver_info(path: &Path) -> DriverInfo {
    let Ok(text) = std::fs::read_to_string(path) else {
        return DriverInfo::default();
    };
    if text.trim().is_empty() {
        return DriverInfo::default();
    }
    serde_json::from_str(&text).unwrap_or_default()
}

/// `driver_alive`: true while a process holds the lease. The probe takes a shared,
/// non-blocking lock on the file — it succeeds exactly when nobody holds the lease. The
/// OS drops the holder's lock when it dies, so there is no stale-lock cleanup.
pub fn driver_alive(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(_) => true,
    }
}

/// `take_lease`: hold `driver.lock` exclusively until the [`DriverLease`] is dropped (or
/// the process dies). A second driver gets [`LeaseError::Held`], naming the holder.
pub fn take_lease(path: &Path) -> Result<DriverLease> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| LeaseError::Io {
            op: "create".to_string(),
            path: dir.to_path_buf(),
            source: e,
        })?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| LeaseError::Io {
            op: "open".to_string(),
            path: path.to_path_buf(),
            source: e,
        })?;
    if let Err(e) = file.try_lock_exclusive() {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            return Err(Error::from(LeaseError::Held(
                driver_info(path).holder_line(),
            )));
        }
        return Err(Error::from(LeaseError::Io {
            op: "lock".to_string(),
            path: path.to_path_buf(),
            source: e,
        }));
    }
    let token = new_token();
    let info = DriverInfo {
        pid: Some(i64::from(std::process::id())),
        host: Some(hostname()),
        started: Some(epoch()),
        token: Some(token.clone()),
    };
    write_info(&mut file, path, &info)?;
    Ok(DriverLease {
        file,
        path: path.to_path_buf(),
        token,
        info,
    })
}

fn write_info(file: &mut File, path: &Path, info: &DriverInfo) -> Result<()> {
    file.set_len(0).map_err(|e| LeaseError::Io {
        op: "truncate".to_string(),
        path: path.to_path_buf(),
        source: e,
    })?;
    let json = serde_json::to_string(info).map_err(|e| LeaseError::Io {
        op: "write".to_string(),
        path: path.to_path_buf(),
        source: std::io::Error::other(e),
    })?;
    file.seek(std::io::SeekFrom::Start(0))
        .and_then(|_| file.write_all(json.as_bytes()))
        .and_then(|_| file.flush())
        .map_err(|e| LeaseError::Io {
            op: "write".to_string(),
            path: path.to_path_buf(),
            source: e,
        })?;
    Ok(())
}

/// The path of `driver.lock` inside a run folder.
pub fn driver_lock_path(orchestrate_dir: &Path) -> PathBuf {
    orchestrate_dir.join("driver.lock")
}

/// An exclusive hold on `driver.lock`. Dropping it (or the process dying) frees the
/// lease for the next driver.
#[derive(Debug)]
pub struct DriverLease {
    file: File,
    /// The lock file, for logging or tests.
    pub path: PathBuf,
    /// The random token of this holder.
    pub token: String,
    /// What this holder wrote into the file.
    pub info: DriverInfo,
}

impl Drop for DriverLease {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

/// A holder token without an RNG dependency: hash of pid + wall-clock nanoseconds.
fn new_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u128::from(u64::from(std::process::id())) ^ d.as_nanos())
        .unwrap_or(0);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    nanos.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// `socket.gethostname()` stand-in: the kernel hostname, "unknown" when unreadable.
fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// UTC time of day of an epoch value, `HH:MM:SS`.
fn hhmmss(epoch: i64) -> String {
    let secs = epoch.rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("driver.lock");
        (t, p)
    }

    #[test]
    fn probe_says_free_without_a_lock_file() {
        let (_t, p) = tmp();
        assert!(!driver_alive(&p));
        assert_eq!(driver_info(&p), DriverInfo::default());
    }

    #[test]
    fn take_lease_writes_pid_host_started_token() {
        let (_t, p) = tmp();
        let lease = take_lease(&p).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let info: DriverInfo = serde_json::from_str(&text).unwrap();
        assert_eq!(info.pid, Some(i64::from(std::process::id())));
        assert!(info.host.is_some());
        assert!(info.started.unwrap() > 1_600_000_000);
        assert_eq!(info.token.as_deref(), Some(lease.token.as_str()));
        assert_eq!(lease.info, info);
        for key in ["pid", "host", "started", "token"] {
            assert!(
                text.contains(&format!("\"{key}\"")),
                "missing {key}: {text}"
            );
        }
    }

    #[test]
    fn holder_line_names_pid_host_and_start_time() {
        let info = DriverInfo {
            pid: Some(123),
            host: Some("box".to_string()),
            started: Some(0),
            token: None,
        };
        assert_eq!(
            info.holder_line(),
            "orch autopilot (pid 123 on box, started 00:00:00)"
        );
        assert_eq!(
            DriverInfo::default().holder_line(),
            "orch autopilot (pid ? on ?, started ?)"
        );
    }

    #[test]
    fn broken_or_empty_lock_file_is_an_empty_info() {
        let (t, p) = tmp();
        std::fs::write(&p, "not json").unwrap();
        assert_eq!(driver_info(&p), DriverInfo::default());
        std::fs::write(&p, "").unwrap();
        assert_eq!(driver_info(&p), DriverInfo::default());
        drop(t);
    }
}
