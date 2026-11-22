//! The desktop shell: opens the app in a window, lets the user choose a project folder, starts
//! `orch serve` for it and hands the page its address and token. It contains no orchestration
//! logic: everything about the run goes through the page's calls to the local API.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_commands;
mod app_settings;
mod create;
mod herdr;
mod project_settings;
mod projects;
mod serve;
mod server;
mod store;

use herdr::HerdrManager;
use server::{Connection, Servers};
use std::path::PathBuf;
use std::sync::Arc;
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
        // the page only ever sees the one spelling the project list stores
        Ok(Some(projects::canonical(&path.display().to_string())))
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

/// Run `f` with the app's Herdr off the main thread (it may start Herdr's server and wait).
async fn with_herdr<T: Send + 'static>(
    manager: State<'_, Arc<HerdrManager>>,
    f: impl FnOnce(&HerdrManager) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let mgr = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || f(&mgr))
        .await
        .map_err(|e| e.to_string())?
}

/// Path of the Herdr the app runs on, once its server runs; else what is wrong, in plain words.
#[tauri::command]
async fn find_herdr(manager: State<'_, Arc<HerdrManager>>) -> Result<String, String> {
    with_herdr(manager, |m| {
        let info = m.ensure_running();
        match (info.state, info.path) {
            (herdr::HerdrState::Running, Some(path)) => Ok(path),
            _ => Err(info.error.unwrap_or_else(|| herdr::NOT_FOUND.into())),
        }
    })
    .await
}

/// How Herdr stands (its server is started first when it is installed and not running).
#[tauri::command]
async fn get_herdr_info(manager: State<'_, Arc<HerdrManager>>) -> Result<herdr::HerdrInfo, String> {
    with_herdr(manager, |m| Ok(m.ensure_running())).await
}

/// Prepare Herdr's install or update for the developer to read; nothing runs.
#[tauri::command]
async fn prepare_herdr_fix(
    manager: State<'_, Arc<HerdrManager>>,
    action: herdr::FixAction,
) -> Result<herdr::FixPlan, String> {
    with_herdr(manager, move |m| m.prepare_fix(action)).await
}

/// Run the install or update the developer read and chose (`token` from its plan).
#[tauri::command]
async fn run_herdr_fix(
    manager: State<'_, Arc<HerdrManager>>,
    token: String,
) -> Result<herdr::HerdrInfo, String> {
    with_herdr(manager, move |m| m.run_fix(&token)).await
}

/// Open Herdr's official site in the default browser.
#[tauri::command]
fn open_herdr_url() -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(herdr::OFFICIAL_URL)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut child| {
            // reap the opener once it hands the page to the browser: no zombie left behind
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        })
        .map_err(|e| format!("The app could not open a browser ({e}). Open {} yourself.", herdr::OFFICIAL_URL))
}

fn main() {
    let herdr = Arc::new(HerdrManager::default());
    let servers = Servers::with_herdr(std::time::Duration::from_secs(10), herdr.clone());

    // make sure Herdr runs as the app opens, without holding the window up
    let herdr_startup = herdr.clone();
    std::thread::spawn(move || {
        let _ = herdr_startup.ensure_running();
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(servers)
        .manage(herdr)
        .invoke_handler(tauri::generate_handler![
            choose_project_folder,
            start_server,
            stop_server,
            get_connection,
            find_orch,
            find_herdr,
            get_herdr_info,
            prepare_herdr_fix,
            run_herdr_fix,
            open_herdr_url,
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
            app_commands::save_app_settings,
            app_commands::detect_agent_clis,
            app_commands::get_project_settings,
            app_commands::save_project_settings
        ])
        .on_window_event(|window, event| {
            // Closing the window ends every server the app started and stops Herdr if started by app.
            if let WindowEvent::Destroyed = event {
                window.app_handle().state::<Servers>().stop_all();
                window.app_handle().state::<Arc<HerdrManager>>().stop_managed();
            }
        })
        .build(tauri::generate_context!())
        .expect("cannot build the desktop shell")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                app.state::<Servers>().stop_all();
                app.state::<Arc<HerdrManager>>().stop_managed();
            }
        });
}
