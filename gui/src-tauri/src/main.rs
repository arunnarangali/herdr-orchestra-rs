//! The desktop shell: opens the app in a window, lets the user choose a project folder, starts
//! `orch serve` for it and hands the page its address and token. It contains no orchestration
//! logic: everything about the run goes through the page's calls to the local API.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_commands;
mod app_settings;
mod create;
mod projects;
mod serve;
mod server;
mod store;

use server::{Connection, Servers};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;

/// Ask for a project folder; `None` when the user cancels.
#[tauri::command]
async fn choose_project_folder(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(folder) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let path = folder.into_path().map_err(|e| e.to_string())?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Start `orch serve` in `project` (or return the one already running for it) and return its
/// address and token.
#[tauri::command]
async fn start_server(app: AppHandle, project: String) -> Result<Connection, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Servers>().start(&PathBuf::from(project))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stop the server of `project`; with no project, every server.
#[tauri::command]
fn stop_server(state: State<'_, Servers>, project: Option<String>) {
    match project {
        Some(p) => state.stop(&PathBuf::from(p)),
        None => state.stop_all(),
    }
}

/// The running server's address and token for `project`; with no project, the first running
/// server (what a reloaded window picks up). `None` when there is none.
#[tauri::command]
fn get_connection(state: State<'_, Servers>, project: Option<String>) -> Option<Connection> {
    match project {
        Some(p) => state.current(&PathBuf::from(p)),
        None => state
            .open_paths()
            .first()
            .and_then(|p| state.current(&PathBuf::from(p))),
    }
}

/// Path of the `orch` binary (`ORCH_BIN`, else the PATH), or why it was not found.
#[tauri::command]
fn find_orch() -> Result<String, String> {
    serve::find_in_environment("orch", "ORCH_BIN").map(|p| p.display().to_string())
}

/// Path of the Herdr executable (`HERDR_BIN`, else the PATH), or why it was not found.
#[tauri::command]
fn find_herdr() -> Result<String, String> {
    serve::find_in_environment("herdr", "HERDR_BIN").map(|p| p.display().to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Servers::default())
        .invoke_handler(tauri::generate_handler![
            choose_project_folder,
            start_server,
            stop_server,
            get_connection,
            find_orch,
            find_herdr,
            app_commands::list_projects,
            app_commands::add_project,
            app_commands::remove_project,
            app_commands::switch_project,
            app_commands::open_project,
            app_commands::retry_project,
            app_commands::close_project,
            app_commands::restore_projects,
            app_commands::create_project,
            app_commands::get_app_settings,
            app_commands::save_app_settings
        ])
        .on_window_event(|window, event| {
            // Closing the window ends every server the app started.
            if let WindowEvent::Destroyed = event {
                window.app_handle().state::<Servers>().stop_all();
            }
        })
        .build(tauri::generate_context!())
        .expect("cannot build the desktop shell")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                app.state::<Servers>().stop_all();
            }
        });
}
