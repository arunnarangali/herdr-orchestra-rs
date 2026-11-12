//! The single-driver lease: one process drives a run at a time, on
//! `<main repo>/.orchestrate/driver.lock` (`fcntl.flock` in Python; `flock` via `fs2` here).

use crate::error::EXIT_USAGE;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use fs2::FileExt;
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
    /// (Python's `driver_line`). The start time is local time.
    pub fn holder_line(&self) -> String {
        let started = self
            .started
            .map(hhmmss)
            .unwrap_or_else(|| "?".to_string());
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
            return Err(Error::from(LeaseError::Held(driver_info(path).holder_line())));
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
    Ok(DriverLease { file, path: path.to_path_buf(), token, info })
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

/// Local time of day of an epoch value, `HH:MM:SS`, like Python's
/// `time.strftime("%H:%M:%S", time.localtime(started))`.
fn hhmmss(epoch: i64) -> String {
    hhmmss_in(epoch, tz_source().as_deref())
}

/// Time of day of an epoch value in the timezone of a TZif file (UTC when `tz` is None).
fn hhmmss_in(epoch: i64, tz: Option<&Path>) -> String {
    let offset = tz.and_then(|p| tz_offset(p, epoch)).unwrap_or(0);
    let secs = (epoch + offset).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// The TZif file the system's local timezone comes from: `$TZ` (`/usr/share/zoneinfo/<TZ>`,
/// or the path itself when `$TZ` is absolute), else `/etc/localtime`. None means UTC.
fn tz_source() -> Option<PathBuf> {
    match std::env::var_os("TZ") {
        Some(tz) if !tz.is_empty() => {
            let name = tz.to_string_lossy().trim_start_matches(':').to_string();
            if name.starts_with('/') {
                Some(PathBuf::from(name))
            } else {
                Some(PathBuf::from("/usr/share/zoneinfo").join(name))
            }
        }
        _ => Some(PathBuf::from("/etc/localtime")),
    }
}

/// The UTC offset in seconds the TZif file gives for `epoch` (Python: `time.localtime`).
fn tz_offset(path: &Path, epoch: i64) -> Option<i64> {
    let data = std::fs::read(path).ok()?;
    let v1 = parse_block(&data, 0, 4)?;
    let block = if data.get(4).is_some_and(|v| (b'2'..=b'4').contains(v)) {
        parse_block(&data, v1.end, 8).unwrap_or(v1)
    } else {
        v1
    };
    let i = block.trans.partition_point(|&tt| tt <= epoch);
    let type_idx = if i == 0 {
        block.types.iter().position(|(_, dst)| !dst).unwrap_or(0)
    } else {
        usize::from(*block.idx.get(i - 1)?)
    };
    block.types.get(type_idx).map(|&(off, _)| off)
}

/// One TZif data block: the transition times (size `tsz`), their types, and the types
/// (gmtoff seconds, isdst).
struct TzBlock {
    trans: Vec<i64>,
    idx: Vec<u8>,
    types: Vec<(i64, bool)>,
    /// Offset just past the block, where a v2+ file keeps its 64-bit block.
    end: usize,
}

fn parse_block(data: &[u8], start: usize, tsz: usize) -> Option<TzBlock> {
    if data.len() < start + 44 || &data[start..start + 4] != b"TZif" {
        return None;
    }
    let counts = |i: usize| -> Option<usize> {
        Some(u32::from_be_bytes(
            data[start + 20 + 4 * i..start + 24 + 4 * i].try_into().ok()?,
        ) as usize)
    };
    let (isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt) =
        (counts(0)?, counts(1)?, counts(2)?, counts(3)?, counts(4)?, counts(5)?);
    let mut off = start + 44;
    let mut trans = Vec::with_capacity(timecnt);
    for i in 0..timecnt {
        let raw = data.get(off + i * tsz..off + (i + 1) * tsz)?;
        let mut be = [0u8; 8];
        be[8 - tsz..].copy_from_slice(raw);
        trans.push(i64::from_be_bytes(be));
    }
    off += timecnt * tsz;
    let idx = data.get(off..off + timecnt)?.to_vec();
    off += timecnt;
    let mut types = Vec::with_capacity(typecnt);
    for i in 0..typecnt {
        let raw = data.get(off + i * 6..off + (i + 1) * 6)?;
        types.push((i64::from(i32::from_be_bytes(raw[0..4].try_into().ok()?)), raw[4] != 0));
    }
    off += typecnt * 6 + charcnt + leapcnt * (tsz + 4) + isstdcnt + isutcnt;
    Some(TzBlock { trans, idx, types, end: off })
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
            assert!(text.contains(&format!("\"{key}\"")), "missing {key}: {text}");
        }
    }

    /// Serializes every test that reads or writes the process-wide `TZ` variable.
    static TZ_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn holder_line_names_pid_host_and_start_time() {
        let _g = TZ_LOCK.lock().unwrap();
        unsafe { std::env::set_var("TZ", "UTC") };
        let info = DriverInfo {
            pid: Some(123),
            host: Some("box".to_string()),
            started: Some(0),
            token: None,
        };
        assert_eq!(info.holder_line(), "orch autopilot (pid 123 on box, started 00:00:00)");
        assert_eq!(DriverInfo::default().holder_line(), "orch autopilot (pid ? on ?, started ?)");
        unsafe { std::env::remove_var("TZ") };
        assert_eq!(DriverInfo::default().holder_line(), "orch autopilot (pid ? on ?, started ?)");
    }

    /// A minimal TZif v1 file: `types` as (gmtoff, isdst), at most one transition at
    /// `trans_at` picking type 1.
    fn tzif(types: &[(i64, bool)], trans_at: Option<i64>) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"TZif");
        data.push(0);
        data.extend_from_slice(&[0; 15]);
        let charcnt = 4;
        let put = |data: &mut Vec<u8>, v: u32| data.extend_from_slice(&v.to_be_bytes());
        put(&mut data, 0); // isutcnt
        put(&mut data, 0); // isstdcnt
        put(&mut data, 0); // leapcnt
        put(&mut data, if trans_at.is_some() { 1 } else { 0 }); // timecnt
        put(&mut data, u32::try_from(types.len()).unwrap()); // typecnt
        put(&mut data, charcnt);
        if let Some(t) = trans_at {
            data.extend_from_slice(&i32::try_from(t).unwrap().to_be_bytes());
            data.push(1);
        }
        for (off, dst) in types {
            data.extend_from_slice(&i32::try_from(*off).unwrap().to_be_bytes());
            data.push(u8::from(*dst));
            data.push(0);
        }
        data.extend_from_slice("TZZ\0".as_bytes());
        data
    }

    #[test]
    fn local_time_uses_the_tzif_offset() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("tz");
        std::fs::write(&p, tzif(&[(7200, false)], None)).unwrap();
        assert_eq!(tz_offset(&p, 0), Some(7200));
        assert_eq!(hhmmss_in(0, Some(&p)), "02:00:00");
        assert_eq!(hhmmss_in(3601, Some(&p)), "03:00:01");
        assert_eq!(hhmmss_in(0, None), "00:00:00");
        assert_eq!(hhmmss_in(43_215, None), "12:00:15");
    }

    #[test]
    fn local_time_respects_transitions_and_dst() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("tz");
        std::fs::write(&p, tzif(&[(0, false), (3600, true)], Some(100_000))).unwrap();
        assert_eq!(tz_offset(&p, 99_999), Some(0));
        assert_eq!(hhmmss_in(99_999, Some(&p)), "03:46:39");
        assert_eq!(tz_offset(&p, 100_000), Some(3600));
        assert_eq!(hhmmss_in(100_000, Some(&p)), "04:46:40");
    }

    #[test]
    fn broken_tz_file_falls_back_to_utc() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("tz");
        std::fs::write(&p, "not a tzif file at all").unwrap();
        assert_eq!(tz_offset(&p, 0), None);
        assert_eq!(hhmmss_in(0, Some(&p)), "00:00:00");
    }

    #[test]
    fn the_system_tz_file_parses_when_present() {
        let _g = TZ_LOCK.lock().unwrap();
        let p = tz_source();
        let Some(p) = p else { return };
        if !p.exists() {
            return;
        }
        assert!(tz_offset(&p, 0).is_some(), "no offset from {}", p.display());
        assert_eq!(hhmmss_in(0, Some(&p)), hhmmss(0));
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
