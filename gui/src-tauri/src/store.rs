//! The shell's own storage: small text files in the app's config folder. Only app-level data
//! lives here (project paths, app settings); nothing is ever written into a project.
//!
//! Format: a header line `herdr-orchestra v1`, then `key=value` lines. A file that does not
//! parse is set aside as `<name>.corrupt` and the defaults are used, with a warning to show.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const HEADER: &str = "herdr-orchestra v1";

/// A value read from disk, with a warning when the file had to be set aside.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Loaded<T> {
    pub value: T,
    pub warning: Option<String>,
}

/// Lines of `key=value`, repeated keys kept in order.
pub type Entries = Vec<(String, String)>;

pub fn parse(text: &str, allowed: &[&str]) -> Result<Entries, String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(HEADER) {
        return Err("it does not start with the expected header".into());
    }
    let mut out = Vec::new();
    for (i, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| format!("line {} is not key=value", i + 2))?;
        if !allowed.contains(&k) {
            return Err(format!("line {} has the unknown key `{k}`", i + 2));
        }
        out.push((k.to_string(), v.to_string()));
    }
    Ok(out)
}

pub fn render(entries: &[(&str, &str)]) -> String {
    let mut s = format!("{HEADER}\n");
    for (k, v) in entries {
        s.push_str(&format!("{k}={v}\n"));
    }
    s
}

/// Values go on one line; refuse anything that would break the format.
pub fn check_value(v: &str) -> Result<(), String> {
    if v.contains(['\n', '\r']) {
        Err("a value cannot contain a line break".into())
    } else {
        Ok(())
    }
}

/// Write via a temporary file and a rename, so a crash never leaves half a file.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("no folder for the file")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot save {}: {e}", path.display()))
}

/// Read `path`: `Ok(None)` when it does not exist; an unreadable or corrupt file is set aside
/// and reported as a warning.
pub fn read_or_set_aside(path: &Path, allowed: &[&str]) -> (Option<Entries>, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (None, None),
        Err(e) => {
            return (
                None,
                Some(format!("{} cannot be read: {e}", path.display())),
            );
        }
    };
    match parse(&text, allowed) {
        Ok(entries) => (Some(entries), None),
        Err(why) => {
            let aside = PathBuf::from(format!("{}.corrupt", path.display()));
            let moved = std::fs::rename(path, &aside).is_ok();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (
                None,
                Some(format!(
                    "{name} could not be read ({why}); {} Starting from the defaults.",
                    if moved {
                        format!("it was kept as {name}.corrupt.")
                    } else {
                        "it was left in place.".to_string()
                    }
                )),
            )
        }
    }
}

/// Last value per key.
pub fn last_values(entries: &Entries) -> BTreeMap<&str, &str> {
    entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

#[cfg(test)]
pub fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "orch-gui-store-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
