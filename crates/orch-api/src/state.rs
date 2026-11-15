//! The application state every route module receives.

use crate::error::ApiError;
use crate::token::Token;
use orch_config::tunables::Tunables;
use orch_core::paths::ORCH_DIR;
use orch_core::store::{DB_FILE, Store};
use orch_herdr::RuntimeAdapter;
use orch_herdr::cli::CliHerdr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::watch;

/// Shared by all handlers (cheap to clone).
#[derive(Clone)]
pub struct AppState {
    project_root: Arc<PathBuf>,
    token: Token,
    shutdown: Arc<watch::Sender<bool>>,
    herdr: Arc<dyn RuntimeAdapter + Send + Sync>,
}

impl AppState {
    /// State for the project whose main repository root is `project_root`.
    pub fn new(project_root: PathBuf, token: Token) -> AppState {
        let tunables = Tunables::from_env()
            .unwrap_or_else(|_| Tunables::from_lookup(|_| None).expect("defaults always parse"));
        let herdr = Arc::new(CliHerdr::from_tunables(&tunables));
        Self::with_herdr(project_root, token, herdr)
    }

    /// State with an explicit Herdr runtime adapter (e.g. `FakeHerdr` in tests).
    pub fn with_herdr(
        project_root: PathBuf,
        token: Token,
        herdr: Arc<dyn RuntimeAdapter + Send + Sync>,
    ) -> AppState {
        AppState {
            project_root: Arc::new(project_root),
            token,
            shutdown: Arc::new(watch::channel(false).0),
            herdr,
        }
    }

    /// The Herdr runtime adapter.
    pub fn herdr(&self) -> &Arc<dyn RuntimeAdapter + Send + Sync> {
        &self.herdr
    }

    /// The project (main repository) root being served.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// The run folder, `<project>/.orchestrate` (it may not exist).
    pub fn orchestrate_dir(&self) -> PathBuf {
        self.project_root.join(ORCH_DIR)
    }

    pub(crate) fn token(&self) -> &Token {
        &self.token
    }

    /// Whether a run exists: its database is there.
    pub fn has_run(&self) -> bool {
        self.orchestrate_dir().join(DB_FILE).is_file()
    }

    /// Open the run's store. Fails with 404 `no_run` when there is no run; it never creates
    /// one. Opening blocks on SQLite: call it (and use the store) inside
    /// `tokio::task::spawn_blocking`.
    pub fn open_store(&self) -> Result<Store, ApiError> {
        if !self.has_run() {
            return Err(ApiError::no_run());
        }
        Store::open(&self.orchestrate_dir().join(DB_FILE)).map_err(ApiError::internal)
    }

    /// Turns `true` when the server is shutting down: long-lived responses (the stream) end
    /// on it so that graceful shutdown does not wait for them forever.
    pub fn shutdown_signal(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }

    pub(crate) fn begin_shutdown(&self) {
        self.shutdown.send_replace(true);
    }
}
