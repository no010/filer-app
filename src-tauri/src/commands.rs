//! Tauri commands invoked from the webview via `invoke()`.
//!
//! Grows across phases. Phase 2: config load/save + health-check ping.
//! Later phases add inbox/history listing, scan_now, file/ignore/undo, tags.

use tauri::State;

use crate::config::Config;
use crate::filer;
use crate::store::{Record, ReviewRow};
use crate::timeutil;
use crate::watcher;
use crate::AppState;

/// Health check — lets the UI verify the backend is alive before first run.
#[tauri::command]
pub fn ping() -> String {
    "filer ok".into()
}

#[tauri::command]
pub fn load_config(state: State<'_, AppState>) -> Result<Config, String> {
    state.load_cfg().map_err(|e| e.to_string())
}

/// Save config and reset any cached state derived from it (store stays open).
#[tauri::command]
pub fn save_config(state: State<'_, AppState>, app: tauri::AppHandle, cfg: Config) -> Result<(), String> {
    cfg.save(state.cfg_path()).map_err(|e| e.to_string())?;
    // Apply auto-start setting immediately.
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    let _ = if cfg.autostart { mgr.enable() } else { mgr.disable() };
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ListInboxResult {
    pub records: Vec<Record>,
}

#[derive(serde::Serialize)]
pub struct ListHistoryResult {
    pub records: Vec<Record>,
    pub next: Option<i64>,
}

#[tauri::command]
pub async fn list_inbox(state: State<'_, AppState>) -> Result<ListInboxResult, String> {
    let g = state.store().map_err(|e| e.to_string())?;
    let store = g.as_ref().unwrap();
    let records = store.list_inbox().map_err(|e| e.to_string())?;
    Ok(ListInboxResult { records })
}

#[tauri::command]
pub async fn get_record(state: State<'_, AppState>, id: i64) -> Result<Option<Record>, String> {
    let g = state.store().map_err(|e| e.to_string())?;
    g.as_ref().unwrap().get(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_history(
    state: State<'_, AppState>,
    after_id: Option<i64>,
    limit: Option<usize>,
) -> Result<ListHistoryResult, String> {
    let g = state.store().map_err(|e| e.to_string())?;
    let store = g.as_ref().unwrap();
    let limit = limit.unwrap_or(100);
    let (records, next) = store.list_history(after_id, limit).map_err(|e| e.to_string())?;
    Ok(ListHistoryResult { records, next })
}

#[tauri::command]
pub async fn scan_now(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<usize, String> {
    let cfg = state.load_cfg().map_err(|e| e.to_string())?;
    let watch_dir = cfg.watch_dir.trim().to_string();
    if watch_dir.is_empty() {
        return Err("未配置监听目录".into());
    }
    watcher::scan_now(app, watch_dir).await.map_err(|e| e.to_string())
}

/// Reveal a path in the system file manager (selects the file on Windows).
#[tauri::command]
pub async fn open_path(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path))
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let p = std::path::Path::new(&path);
        let target = if p.is_file() { p.parent().unwrap_or(p) } else { p };
        open::that(target).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn file_record(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: i64,
    overrides: filer::FileOverrides,
) -> Result<filer::FileResult, String> {
    let _ = state; // state available for future sync use; filer reads via app.state()
    filer::file(app, id, overrides).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ignore_record(app: tauri::AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    use tauri::Emitter;
    let g = state.store().map_err(|e| e.to_string())?;
    g.as_ref().unwrap().mark_ignored(id).map_err(|e| e.to_string())?;
    let _ = app.emit("item-updated", id);
    Ok(())
}

#[tauri::command]
pub async fn undo_file(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    filer::undo(app, id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_tags(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: i64,
    tags: Vec<String>,
) -> Result<(), String> {
    use tauri::Emitter;
    let g = state.store().map_err(|e| e.to_string())?;
    let tags_json = serde_json::to_string(&tags).map_err(|e| e.to_string())?;
    g.as_ref().unwrap().set_tags(id, &tags_json).map_err(|e| e.to_string())?;
    let _ = app.emit("item-updated", id);
    Ok(())
}

#[tauri::command]
pub async fn delete_record(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let g = state.store().map_err(|e| e.to_string())?;
    g.as_ref().unwrap().delete(id).map_err(|e| e.to_string())?;
    Ok(())
}

/// Delete an inbox record's source file (the download in watch_dir) AND the
/// record itself. Used by the inbox "一键删除" bulk action.
#[tauri::command]
pub async fn delete_source_file(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let path = {
        let g = state.store().map_err(|e| e.to_string())?;
        let store = g.as_ref().unwrap();
        let r = store.get(id).map_err(|e| e.to_string())?
            .ok_or_else(|| format!("record {id} not found"))?;
        r.original_path
    };
    if !path.is_empty() {
        let p = path.clone();
        let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            if std::path::Path::new(&p).exists() {
                let _ = std::fs::remove_file(&p);
            }
            Ok(())
        }).await;
    }
    let g = state.store().map_err(|e| e.to_string())?;
    g.as_ref().unwrap().delete(id).map_err(|e| e.to_string())?;
    Ok(())
}

// ---- 回顾 (staleness tracking) ----

/// Open a filed record's file in the file manager AND bump last_opened_at.
#[tauri::command]
pub async fn open_record(state: State<'_, AppState>, app: tauri::AppHandle, id: i64) -> Result<(), String> {
    use tauri::Emitter;
    let (path, tz) = {
        let s = state.load_cfg().map_err(|e| e.to_string())?;
        let g = state.store().map_err(|e| e.to_string())?;
        let store = g.as_ref().unwrap();
        let r = store.get(id).map_err(|e| e.to_string())?
            .ok_or_else(|| format!("record {id} not found"))?;
        (r.filed_path, s.tz())
    };
    if path.is_empty() {
        return Err("该记录无归档路径".into());
    }
    // reveal in explorer (reuse open_path's logic)
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path))
            .spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let p = std::path::Path::new(&path);
        let target = if p.is_file() { p.parent().unwrap_or(p) } else { p };
        open::that(target).map_err(|e| e.to_string())?;
    }
    let ts = timeutil::now_rfc3339(tz);
    {
        let g = state.store().map_err(|e| e.to_string())?;
        g.as_ref().unwrap().touch_opened(id, &ts).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("item-updated", id);
    Ok(())
}

/// Mark a filed record as "仍需要" — bumps last_reviewed_at, resetting the
/// staleness clock.
#[tauri::command]
pub async fn touch_reviewed(state: State<'_, AppState>, app: tauri::AppHandle, id: i64) -> Result<(), String> {
    use tauri::Emitter;
    let cfg = state.load_cfg().map_err(|e| e.to_string())?;
    let ts = timeutil::now_rfc3339(cfg.tz());
    {
        let g = state.store().map_err(|e| e.to_string())?;
        g.as_ref().unwrap().touch_reviewed(id, &ts).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("item-updated", id);
    Ok(())
}

/// Delete the on-disk filed file and mark the record 'deleted'.
#[tauri::command]
pub async fn delete_filed_file(state: State<'_, AppState>, app: tauri::AppHandle, id: i64) -> Result<(), String> {
    use tauri::Emitter;
    let path = {
        let g = state.store().map_err(|e| e.to_string())?;
        let store = g.as_ref().unwrap();
        let r = store.get(id).map_err(|e| e.to_string())?
            .ok_or_else(|| format!("record {id} not found"))?;
        r.filed_path
    };
    if !path.is_empty() {
        let p = path.clone();
        let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            if std::path::Path::new(&p).exists() {
                let _ = std::fs::remove_file(&p);
            }
            Ok(())
        }).await;
    }
    {
        let g = state.store().map_err(|e| e.to_string())?;
        g.as_ref().unwrap().mark_deleted(id).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("item-updated", id);
    Ok(())
}

/// Filed records ranked stales-first, enriched with staleness_days +
/// updated_since_filed + file_missing for the 回顾 view.
#[tauri::command]
pub async fn list_review(state: State<'_, AppState>) -> Result<Vec<ReviewRow>, String> {
    let records = {
        let g = state.store().map_err(|e| e.to_string())?;
        g.as_ref().unwrap().list_review(500).map_err(|e| e.to_string())?
    };
    let mut out = Vec::with_capacity(records.len());
    for r in records {
        let staleness_days = timeutil::staleness_days(&r.last_opened_at, &r.last_reviewed_at, &r.filed_at);
        let (updated_since_filed, file_missing) = if r.filed_path.is_empty() {
            (false, true)
        } else {
            let p = std::path::Path::new(&r.filed_path);
            if !p.exists() {
                (false, true)
            } else {
                let cur = timeutil::mtime_rfc(p).unwrap_or_default();
                (timeutil::mtime_changed(&cur, &r.file_mtime_at_filed), false)
            }
        };
        out.push(ReviewRow { record: r, staleness_days, updated_since_filed, file_missing });
    }
    Ok(out)
}

/// Count filed records whose most-recent-touch is older than `days` ago
/// (for the launch banner).
#[tauri::command]
pub async fn count_stale(state: State<'_, AppState>, days: i64) -> Result<usize, String> {
    let cutoff = timeutil::cutoff_rfc(days);
    let g = state.store().map_err(|e| e.to_string())?;
    let n = g.as_ref().unwrap().count_stale(&cutoff).map_err(|e| e.to_string())?;
    Ok(n)
}

// ---- 检索 + 统计 ----

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    q: String,
    limit: Option<usize>,
) -> Result<Vec<Record>, String> {
    let g = state.store().map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(200);
    g.as_ref().unwrap().search(&q, limit).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct StalenessTiers {
    pub d30: usize,
    pub d90: usize,
    pub d180: usize,
    pub d365: usize,
    pub more: usize,
}

#[derive(serde::Serialize)]
pub struct Stats {
    pub total_filed: usize,
    pub total_inbox: usize,
    pub by_category: Vec<(String, usize)>,
    pub by_action: Vec<(String, usize)>,
    pub staleness: StalenessTiers,
    pub last_7_days: usize,
}

#[tauri::command]
pub async fn stats(state: State<'_, AppState>) -> Result<Stats, String> {
    let g = state.store().map_err(|e| e.to_string())?;
    let store = g.as_ref().unwrap();
    let total_filed = store.count_status("filed").map_err(|e| e.to_string())?;
    let total_inbox = store.count_status("inbox").map_err(|e| e.to_string())?;
    let last_7_days = store.count_filed_since(&timeutil::cutoff_rfc(7)).map_err(|e| e.to_string())?;
    let by_category = store.group_count_filed("category").map_err(|e| e.to_string())?;
    let by_action = store.group_count_filed("action").map_err(|e| e.to_string())?;
    let c30 = store.count_stale(&timeutil::cutoff_rfc(30)).map_err(|e| e.to_string())?;
    let c90 = store.count_stale(&timeutil::cutoff_rfc(90)).map_err(|e| e.to_string())?;
    let c180 = store.count_stale(&timeutil::cutoff_rfc(180)).map_err(|e| e.to_string())?;
    let c365 = store.count_stale(&timeutil::cutoff_rfc(365)).map_err(|e| e.to_string())?;
    let staleness = StalenessTiers {
        d30: total_filed.saturating_sub(c30),
        d90: c30.saturating_sub(c90),
        d180: c90.saturating_sub(c180),
        d365: c180.saturating_sub(c365),
        more: c365,
    };
    Ok(Stats { total_filed, total_inbox, by_category, by_action, staleness, last_7_days })
}
