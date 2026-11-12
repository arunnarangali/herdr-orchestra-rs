//! The one error type the CLI turns into a message and an exit code.
//!
//! # Plugging a module's own error in
//!
//! A module (task, store, lease, config…) defines its own error enum in its own file and
//! converts it into [`Error`] by implementing `From<ModuleError> for Error` **in that module's
//! file**, mapping it to [`Error::Other`] (message + exit code) — or [`Error::wrap`]. Nobody
//! edits this file to add a variant; that keeps parallel milestones conflict-free.
//!
//! ```
//! use orch_core::Error;
//! #[derive(Debug, thiserror::Error)]
//! #[error("lease is held by {0}")]
//! struct LeaseError(String);
//! impl From<LeaseError> for Error {
//!     fn from(e: LeaseError) -> Self { Error::wrap(e, 3) }
//! }
//! assert_eq!(Error::from(LeaseError("pid 1".into())).exit_code(), 3);
//! ```

use std::path::PathBuf;

/// Exit code for a general error (Python: `die`).
pub const EXIT_ERROR: i32 = 1;
/// Exit code for usage / validation errors.
pub const EXIT_USAGE: i32 = 2;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The current directory is not inside a git repository.
    #[error("not inside a git repository (looked from {})", .0.display())]
    NotInGitRepo(PathBuf),
    /// `git` could not be run at all.
    #[error("could not run git: {0}")]
    GitUnavailable(#[source] std::io::Error),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    /// An error from another module, with the exit code the CLI should use.
    #[error("{source}")]
    Other {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
        code: i32,
    },
}

impl Error {
    /// Wrap a module error with the exit code it should produce.
    pub fn wrap<E>(source: E, code: i32) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::Other {
            source: Box::new(source),
            code,
        }
    }

    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io {
            context: context.into(),
            source,
        }
    }

    /// The process exit code for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Other { code, .. } => *code,
            _ => EXIT_ERROR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(Error::NotInGitRepo("/x".into()).exit_code(), EXIT_ERROR);
        let e = Error::wrap(std::fmt::Error, 3);
        assert_eq!(e.exit_code(), 3);
        assert!(!e.to_string().is_empty());
    }

    #[test]
    fn messages() {
        let e = Error::io("reading x", std::io::Error::other("boom"));
        assert_eq!(e.to_string(), "reading x: boom");
        assert!(
            Error::NotInGitRepo("/x".into())
                .to_string()
                .contains("not inside a git repository")
        );
    }
}
