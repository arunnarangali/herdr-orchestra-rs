//! Tauri commands for the project list and the app's settings. Thin wrappers over
//! `projects.rs` and `app_settings.rs`: they store paths and preferences and start the server
//! for a chosen project, nothing more.

use crate::app_settings::{AppSettings, SettingsStore};
use crate::projects::{ProjectList, ProjectStore};
use crate::server::{Connection, ServerState};
use crate::store::Loaded;
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

#[tauri::command]
pub fn remove_project(app: AppHandle, path: String) -> Result<ProjectList, String> {
    let store = projects(&app)?;
    store.remove(&path)?;
    Ok(store.list())
}

/// Make `path` the active project and restart the server for it.
#[tauri::command]
pub async fn switch_project(app: AppHandle, path: String) -> Result<Connection, String> {
    tauri::async_runtime::spawn_blocking(move || {
        projects(&app)?.set_active(&path)?;
        let orch = settings(&app)?.load().value.orch_path;
        app.state::<ServerState>()
            .start_using(&PathBuf::from(&path), Some(orch))
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
