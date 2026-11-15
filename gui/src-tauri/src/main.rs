//! The desktop shell: opens the app in a window, lets the user choose a project folder, starts
//! `orch serve` for it and hands the page its address and token. It contains no orchestration
//! logic: everything about the run goes through the page's calls to the local API.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod serve;
mod server;

use server::{Connection, ServerState};
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

/// Start `orch serve` in `project` and return its address and token.
#[tauri::command]
async fn start_server(app: AppHandle, project: String) -> Result<Connection, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<ServerState>().start(&PathBuf::from(project))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn stop_server(state: State<'_, ServerState>) {
    state.stop();
}

/// The running server's address and token, or `None`.
#[tauri::command]
fn get_connection(state: State<'_, ServerState>) -> Option<Connection> {
    state.current()
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
        .manage(ServerState::default())
        .invoke_handler(tauri::generate_handler![
            choose_project_folder,
            start_server,
            stop_server,
            get_connection,
            find_orch,
            find_herdr
        ])
        .on_window_event(|window, event| {
            // Closing the window ends the server it started.
            if let WindowEvent::Destroyed = event {
                window.app_handle().state::<ServerState>().stop();
            }
        })
        .build(tauri::generate_context!())
        .expect("cannot build the desktop shell")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                app.state::<ServerState>().stop();
            }
        });
}
