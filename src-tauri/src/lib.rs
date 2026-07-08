//! filer — download organizer. Tauri (Rust) native, no sidecar.
//!
//! Watches a download directory, auto-suggests metadata + destination for
//! each finished download, and files it into a structured folder tree after
//! user confirmation. Local SQLite index; files are moved on the local FS.

// Several helpers/consts are kept for planned phases (rule editor, stats,
// timezone-formatted display) and are currently unused in the lib build.
#![allow(dead_code)]

mod analyze;
mod commands;
mod config;
mod filer;
mod pathutil;
mod pdfinfo;
mod rules;
mod store;
mod timeutil;
mod watcher;

use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::MacosLauncher;

use crate::config::Config;
use crate::store::Store;

/// Windows HKCU registry path for the "归档到 filer" right-click entry,
/// applied to all files (`*\shell\filer`). Folders are not affected.
#[cfg(target_os = "windows")]
const CTX_KEY: &str = r"HKCU\Software\Classes\*\shell\filer";

/// Register the "用 filer 归档" right-click entry for all files (HKCU, no
/// admin needed). Only writes if the entry is missing or the exe path
/// changed (e.g. after an update) — same fence as shelf.
#[cfg(target_os = "windows")]
fn register_context_menu_if_needed(exe: &str) {
    use std::process::Command;
    let cmd_key = format!("{}\\command", CTX_KEY);
    let already = Command::new("reg").args(["query", &cmd_key, "/ve"]).output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.contains(exe))
        .unwrap_or(false);
    if already { return; }
    let _ = Command::new("reg").args(["add", CTX_KEY, "/ve", "/d", "用 filer 归档", "/f"]).status();
    let _ = Command::new("reg").args(["add", CTX_KEY, "/v", "Icon", "/d", exe, "/f"]).status();
    let cmd = format!("\"{exe}\" --import \"%1\"");
    let _ = Command::new("reg").args(["add", &cmd_key, "/ve", "/d", &cmd, "/f"]).status();
}

/// Process a `--import <path>` (right-click): bring the app to front, then
/// analyze + insert the file into the inbox (or pop the auto-file modal if
/// auto_file is on). Reuses the watcher's process_path.
fn handle_import(app: tauri::AppHandle, path: String) {
    // Emit so the UI brings the window to front.
    let _ = app.emit("import-from-path", path.clone());
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
    let p = std::path::PathBuf::from(path);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        // Small delay so the app/store is ready on a cold start.
        std::thread::sleep(std::time::Duration::from_secs(1));
        let _ = watcher::process_path(app2, p).await;
    });
}

/// App-wide state: config file path + lazily-opened SQLite store.
pub struct AppState {
    cfg_path: PathBuf,
    db_path: PathBuf,
    store: Mutex<Option<Store>>,
}

impl AppState {
    pub fn new(cfg_path: PathBuf, db_path: PathBuf) -> Self {
        Self { cfg_path, db_path, store: Mutex::new(None) }
    }

    pub fn cfg_path(&self) -> &PathBuf { &self.cfg_path }

    pub fn load_cfg(&self) -> anyhow::Result<Config> {
        Config::load(&self.cfg_path)
    }

    /// Lazily open the SQLite store (rebuilt if the db is missing/closed).
    pub fn store(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Option<Store>>> {
        let mut g = self.store.lock().unwrap();
        if g.is_none() {
            *g = Some(Store::open(&self.db_path)?);
        }
        Ok(g)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let cfg_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&cfg_dir).ok();
            let cfg_path = cfg_dir.join("config.toml");
            let db_path = cfg_dir.join("filer.db");
            app.manage(AppState::new(cfg_path, db_path));

            // Apply auto-start setting from config on launch.
            if let Ok(cfg) = Config::load(&cfg_dir.join("config.toml")) {
                use tauri_plugin_autostart::ManagerExt;
                let mgr = app.autolaunch();
                let _ = if cfg.autostart { mgr.enable() } else { mgr.disable() };
            }

            // Start the download-dir watcher (no-op until watch_dir is set).
            // Manage it so Tauri owns the handle for the app lifetime (dropping
            // the handle signals shutdown, so we must not let it drop here).
            app.manage(watcher::start(app.handle().clone()));

            // Register the "用 filer 归档" right-click entry for all files
            // (HKCU, no admin). Only writes if missing/stale.
            #[cfg(target_os = "windows")]
            if let Ok(exe) = std::env::current_exe() {
                if let Some(exe_str) = exe.to_str() {
                    register_context_menu_if_needed(exe_str);
                }
            }

            // Handle `--import <path>` (first instance launched from right-click).
            let args: Vec<String> = std::env::args().collect();
            if let Some(idx) = args.iter().position(|a| a == "--import") {
                if let Some(path) = args.get(idx + 1).cloned() {
                    let app_handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        handle_import(app_handle, path);
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::load_config,
            commands::save_config,
            commands::list_inbox,
            commands::get_record,
            commands::list_history,
            commands::scan_now,
            commands::open_path,
            commands::file_record,
            commands::ignore_record,
            commands::undo_file,
            commands::import_path,
            commands::remove_context_menu,
            commands::set_tags,
            commands::delete_record,
            commands::delete_source_file,
            commands::open_record,
            commands::touch_reviewed,
            commands::delete_filed_file,
            commands::list_review,
            commands::count_stale,
            commands::search,
            commands::stats,
        ]);

    // Single-instance: if a second instance launches (right-click while app is
    // running), forward its `--import <path>` to the first instance.
    #[cfg(not(mobile))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(idx) = args.iter().position(|a| a == "--import") {
                if let Some(path) = args.get(idx + 1).cloned() {
                    handle_import(app.clone(), path);
                }
            }
        }));
    }

    builder.run(tauri::generate_context!()).expect("error while running filer");
}
