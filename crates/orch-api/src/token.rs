//! The per-install API token: a random secret in the user's config directory that every request
//! must carry. It is created on first use with owner-only permissions, reused afterwards, and
//! never printed: [`Token`]'s `Debug` is redacted and nothing here logs the value.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// File name of the token inside the config directory.
pub const TOKEN_FILE: &str = "api-token";
/// Random bytes in a token (hex-encoded to twice this many characters).
const TOKEN_BYTES: usize = 32;

/// The secret. Compare with [`Token::matches`], never with `==` on the text.
#[derive(Clone)]
pub struct Token(String);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

impl Token {
    /// Wrap an existing secret (tests; the server itself uses [`load_or_create`]).
    pub fn new(secret: impl Into<String>) -> Token {
        Token(secret.into())
    }

    /// `text` with every occurrence of the secret replaced by `[redacted]`: what is stored or
    /// logged on a client's behalf never carries the token.
    pub fn redact(&self, text: &str) -> String {
        if self.0.is_empty() {
            return text.to_string();
        }
        text.replace(&self.0, "[redacted]")
    }

    /// Constant-time comparison with a presented value.
    pub fn matches(&self, presented: &str) -> bool {
        let (a, b) = (self.0.as_bytes(), presented.as_bytes());
        let mut diff = a.len() ^ b.len();
        for i in 0..a.len().max(b.len()) {
            diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
        }
        diff == 0
    }
}

fn io_err(what: &str, path: &Path, e: std::io::Error) -> std::io::Error {
    std::io::Error::new(e.kind(), format!("{what} {}: {e}", path.display()))
}

fn random_hex() -> std::io::Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn valid(secret: &str) -> bool {
    secret.len() >= TOKEN_BYTES * 2 && secret.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn read_existing(path: &Path) -> std::io::Result<Token> {
    let text = fs::read_to_string(path).map_err(|e| io_err("reading", path, e))?;
    let secret = text.trim();
    if !valid(secret) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "{} does not hold a token; delete it and start again",
                path.display()
            ),
        ));
    }
    // A token another user could read is no token: tighten it rather than trust it.
    let mode = fs::metadata(path)
        .map_err(|e| io_err("reading", path, e))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| io_err("restricting", path, e))?;
    }
    Ok(Token(secret.to_string()))
}

/// The token file inside `dir`.
pub fn token_path(dir: &Path) -> PathBuf {
    dir.join(TOKEN_FILE)
}

/// Read the token in `dir`, creating `dir` (mode 0700) and the file (mode 0600) when missing.
/// Two servers starting at once end up with the same token: the file appears fully written or
/// not at all (written aside, then linked into place).
pub fn load_or_create(dir: &Path) -> std::io::Result<Token> {
    let path = token_path(dir);
    if path.exists() {
        return read_existing(&path);
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| io_err("creating", dir, e))?;
    let secret = random_hex()?;
    let tmp = dir.join(format!("{TOKEN_FILE}.{}.tmp", std::process::id()));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| io_err("creating", &tmp, e))?;
    f.write_all(secret.as_bytes())
        .map_err(|e| io_err("writing", &tmp, e))?;
    drop(f);
    let linked = fs::hard_link(&tmp, &path);
    let _ = fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(Token(secret)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => read_existing(&path),
        Err(e) => Err(io_err("creating", &path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_once_private_and_reused() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("cfg/herdr-orchestra");
        let a = load_or_create(&dir).unwrap();
        let file = token_path(&dir);
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let b = load_or_create(&dir).unwrap();
        assert!(a.matches(&b.0));
        assert_eq!(a.0.len(), 64);
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no temp file left behind"
        );
    }

    #[test]
    fn a_loose_existing_file_is_tightened_and_junk_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let file = token_path(t.path());
        fs::write(&file, "x").unwrap();
        assert!(load_or_create(t.path()).is_err());
        let secret = "a".repeat(64);
        fs::write(&file, format!("{secret}\n")).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_or_create(t.path()).unwrap().matches(&secret));
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn matching_and_redacted_debug() {
        let t = Token::new("s3cret");
        assert!(t.matches("s3cret"));
        assert!(!t.matches("s3cre"));
        assert!(!t.matches("s3crets"));
        assert!(!t.matches(""));
        assert!(!format!("{t:?}").contains("s3cret"));
    }
}
