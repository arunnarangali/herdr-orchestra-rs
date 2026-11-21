//! Tauri commands for the project list and the app's settings. Thin wrappers over
//! `projects.rs`, `create.rs` and `app_settings.rs`: they store paths and preferences and start
//! or stop the server of a chosen project, nothing more. Every server belongs to one project;
//! no command here touches the server of another project.

use crate::app_settings::{AppSettings, SettingsStore};
use crate::create::{self, Created};
use crate::project_settings::{
    detect_agent_clis_with, validate_lead_agent, AgentCli, ProjectSettings, ProjectSettingsStore,
    SaveProjectSettingsInput,
};
use crate::projects::{canonical, ProjectList, ProjectStore};
use crate::server::{Connection, Servers};
use crate::store::Loaded;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|e| format!("cannot find the app's config folder: {e}"))
}

fn projects(app: &AppHandle) -> Result<ProjectStore, String> {
    config_dir(app).map(ProjectStore::new)
}

fn settings(app: &AppHandle) -> Result<SettingsStore, String> {
    config_dir(app).map(SettingsStore::new)
}

fn orch_setting(app: &AppHandle) -> Option<String> {
    settings(app).ok().map(|s| s.load().value.orch_path)
}

#[tauri::command]
pub fn list_projects(app: AppHandle) -> Result<ProjectList, String> {
    Ok(projects(&app)?.list())
}

#[tauri::command]
pub fn add_project(app: AppHandle, path: String) -> Result<ProjectList, String> {
    let store = projects(&app)?;
    store.add(&path)?;
    Ok(store.list())
}

/// Forget a project: its server (if open) is stopped first.
#[tauri::command]
pub fn remove_project(app: AppHandle, path: String) -> Result<ProjectList, String> {
    app.state::<Servers>().stop(&PathBuf::from(&path));
    let store = projects(&app)?;
    store.remove(&path)?;
    Ok(store.list())
}

/// Make `path` the active project and make sure its server runs (an already running one is
/// kept; other projects' servers are not touched).
#[tauri::command]
pub async fn switch_project(app: AppHandle, path: String) -> Result<Connection, String> {
    tauri::async_runtime::spawn_blocking(move || {
        projects(&app)?.set_active(&path)?;
        let orch = orch_setting(&app);
        app.state::<Servers>().start_using(&PathBuf::from(&path), orch)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Open a listed project: remember it as open and start its server. The project stays open
/// (and is retried by the next `open_project` or at the next start) when the server fails.
#[tauri::command]
pub async fn open_project(app: AppHandle, path: String) -> Result<Connection, String> {
    tauri::async_runtime::spawn_blocking(move || open(&app, &path, false))
        .await
        .map_err(|e| e.to_string())?
}

/// Start the server of a project whose server failed or died: a retry that never touches the
/// other projects.
#[tauri::command]
pub async fn retry_project(app: AppHandle, path: String) -> Result<Connection, String> {
    tauri::async_runtime::spawn_blocking(move || open(&app, &path, true))
        .await
        .map_err(|e| e.to_string())?
}

fn open(app: &AppHandle, path: &str, restart: bool) -> Result<Connection, String> {
    projects(app)?.set_open(path, true)?;
    let orch = orch_setting(app);
    let servers = app.state::<Servers>();
    let p = PathBuf::from(path);
    if restart {
        servers.restart_using(&p, orch)
    } else {
        servers.start_using(&p, orch)
    }
}

/// Close a project: forget that it is open and stop its server (and only its).
#[tauri::command]
pub fn close_project(app: AppHandle, path: String) -> Result<ProjectList, String> {
    let store = projects(&app)?;
    store.set_open(&path, false)?;
    app.state::<Servers>().stop(&PathBuf::from(&path));
    Ok(store.list())
}

/// One project of the restored set: its connection, or why its server did not start.
#[derive(Debug, Clone, Serialize)]
pub struct Restored {
    pub path: String,
    pub name: String,
    pub connection: Option<Connection>,
    pub error: Option<String>,
}

/// Start the server of every project that was open when the app last ran. They start at the
/// same time; one that fails is reported with its reason and does not stop the others.
#[tauri::command]
pub async fn restore_projects(app: AppHandle) -> Result<Vec<Restored>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let list = projects(&app)?.list();
        let orch = orch_setting(&app);
        let handles: Vec<_> = list
            .open
            .iter()
            .map(|path| {
                let app = app.clone();
                let orch = orch.clone();
                let path = path.clone();
                std::thread::spawn(move || {
                    let servers = app.state::<Servers>();
                    let result = servers.start_using(&PathBuf::from(&path), orch);
                    restored(&path, result)
                })
            })
            .collect();
        Ok(handles
            .into_iter()
            .zip(&list.open)
            .map(|(h, path)| {
                h.join()
                    .unwrap_or_else(|_| restored(path, Err("the start-up crashed".into())))
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn restored(path: &str, result: Result<Connection, String>) -> Restored {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    match result {
        Ok(c) => Restored { path: path.into(), name, connection: Some(c), error: None },
        Err(e) => Restored { path: path.into(), name, connection: None, error: Some(e) },
    }
}

/// What creating a project did, and how to reach its server.
#[derive(Debug, Clone, Serialize)]
pub struct CreatedProject {
    pub created: Created,
    pub connection: Connection,
}

/// Create a project in `path` (see `create.rs`), add it to the list and open it.
#[tauri::command]
pub async fn create_project(app: AppHandle, path: String) -> Result<CreatedProject, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let created = create::create_project(&path)?;
        projects(&app)?.add(&created.path)?;
        let connection = open(&app, &created.path, false)?;
        Ok(CreatedProject { created, connection })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_app_settings(app: AppHandle) -> Result<Loaded<AppSettings>, String> {
    Ok(settings(&app)?.load())
}

#[tauri::command]
pub fn save_app_settings(app: AppHandle, settings_in: AppSettings) -> Result<AppSettings, String> {
    settings(&app)?.save(&settings_in)
}

#[tauri::command]
pub fn detect_agent_clis() -> Result<Vec<AgentCli>, String> {
    let registry = orch_config::agents::load(None).map_err(|e| e.to_string())?;
    Ok(detect_agent_clis_with(
        &registry,
        std::env::var_os("PATH").as_deref(),
    ))
}

#[tauri::command]
pub fn get_project_settings(app: AppHandle, path: String) -> Result<ProjectSettings, String> {
    let canon = canonical(&path);
    let store = projects(&app)?;
    if !store.load().value.projects.contains(&canon) {
        return Err(format!("{path} is not in the project list"));
    }
    let p_store = ProjectSettingsStore::new(config_dir(&app)?);
    let loaded = p_store.load(&canon);
    Ok(loaded.value)
}

#[tauri::command]
pub fn save_project_settings(
    app: AppHandle,
    path: String,
    settings: SaveProjectSettingsInput,
) -> Result<ProjectSettings, String> {
    let old_canonical = canonical(&path);
    let proj_store = projects(&app)?;
    if !proj_store.load().value.projects.contains(&old_canonical) {
        return Err(format!("{path} is not in the project list"));
    }

    let registry = orch_config::agents::load(None).map_err(|e| e.to_string())?;
    let validated_lead = validate_lead_agent(
        settings.lead_agent.as_deref(),
        &registry,
        std::env::var_os("PATH").as_deref(),
    )?;

    let new_canonical = proj_store.update_folder(&old_canonical, &settings.folder)?;

    let p_store = ProjectSettingsStore::new(config_dir(&app)?);
    if new_canonical != old_canonical {
        app.state::<Servers>().stop(&PathBuf::from(&old_canonical));
        let _ = p_store.remove(&old_canonical);
    }

    let to_save = ProjectSettings {
        lead_agent: validated_lead,
        folder: new_canonical.clone(),
        warning: None,
    };
    p_store.save(&new_canonical, &to_save)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restored_project_carries_its_connection_or_its_reason() {
        let ok = restored("/p/a", Ok(Connection { url: "http://127.0.0.1:1".into(), token: "t".into() }));
        assert_eq!((ok.name.as_str(), ok.error.is_none()), ("a", true));
        assert!(ok.connection.is_some());
        let bad = restored("/p/b", Err("orch serve did not start: address in use".into()));
        assert!(bad.connection.is_none());
        assert!(bad.error.unwrap().contains("address in use"));
    }
}
