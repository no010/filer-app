//! Filesystem watcher + download-completion detection.
//!
//! `notify` watches the configured download dir (non-recursive). Temp files
//! (`.part` / `.crdownload` / `.download` / `~$` / leading-dot) are ignored.
//! A file is considered "download complete" when its size has been stable for
//! ≥3s with no further events — then it's analyzed, rule-matched, inserted
//! into the inbox, and the UI is notified via `new-inbox-item`.
//!
//! The loop re-reads config every few seconds so a changed `watch_dir` (from
//! Settings) is picked up without restarting the app.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::analyze;
use crate::rules;
use crate::store::NewRecord;
use crate::timeutil;
use crate::AppState;

/// Stability window: a file is "done" once no size change has been seen for
/// this long.
const STABLE_FOR: Duration = Duration::from_secs(3);
/// How often the loop ticks to check pending files + re-read config.
const TICK: Duration = Duration::from_secs(1);
/// How often to re-check config for a changed watch_dir.
const CFG_RECHECK: Duration = Duration::from_secs(5);

struct Pending {
    last_change: Instant,
    last_size: u64,
}

#[derive(Debug, Clone)]
struct ProcessResult {
    sha: String,
    meta: analyze::SubMeta,
    filename: String,
    size: i64,
    suggestion: Option<rules::Suggestion>,
}

/// Background watcher handle. Dropping it signals shutdown (the app owns it
/// for the process lifetime, so in practice it never drops before exit).
pub struct WatcherHandle {
    shutdown: Arc<AtomicBool>,
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

/// Start the background watcher loop. Returns immediately.
///
/// Uses `tauri::async_runtime::spawn` (not `tokio::spawn`) because setup()
/// runs outside a tokio runtime context — Tauri's async runtime owns the
/// global tokio reactor the loop needs for `sleep` / `spawn_blocking`.
pub fn start(app: AppHandle) -> WatcherHandle {
    let shutdown = Arc::new(AtomicBool::new(false));
    let s = shutdown.clone();
    tauri::async_runtime::spawn(async move {
        run_loop(app, s).await;
    });
    WatcherHandle { shutdown }
}

#[allow(unused_assignments, unused_variables)]
async fn run_loop(app: AppHandle, shutdown: Arc<AtomicBool>) {
    let pending: Arc<Mutex<HashMap<PathBuf, Pending>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut current_dir: String = String::new();
    let mut watcher: Option<notify::RecommendedWatcher> = None;
    let mut last_cfg_check = Instant::now();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Periodically re-read config so a changed watch_dir takes effect.
        if last_cfg_check.elapsed() >= CFG_RECHECK {
            last_cfg_check = Instant::now();
            let cfg = { let s = app.state::<AppState>(); s.load_cfg().ok() };
            if let Some(cfg) = cfg {
                if cfg.watch_dir != current_dir {
                    // Drop the old watcher (stops watching), create a new one.
                    watcher = None;
                    current_dir.clear();
                    if !cfg.watch_dir.is_empty() {
                        match make_watcher(&cfg.watch_dir, pending.clone()) {
                            Ok(w) => {
                                watcher = Some(w);
                                current_dir = cfg.watch_dir.clone();
                                eprintln!("[watcher] watching {current_dir}");
                            }
                            Err(e) => {
                                eprintln!("[watcher] watch failed for {}: {e}", cfg.watch_dir);
                            }
                        }
                    }
                }
            }
        }

        // Drain stable pending files.
        let ready = drain_stable(&pending);
        for p in ready {
            process_path(app.clone(), p).await;
        }

        tokio::time::sleep(TICK).await;
    }
}

fn make_watcher(
    watch_dir: &str,
    pending: Arc<Mutex<HashMap<PathBuf, Pending>>>,
) -> anyhow::Result<notify::RecommendedWatcher> {
    // notify callback: record/refresh pending candidates on Create/Modify.
    let mut w = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        if let Ok(ev) = res {
            handle_event(ev, &pending);
        }
    })?;
    w.watch(Path::new(watch_dir), RecursiveMode::NonRecursive)?;
    Ok(w)
}

fn handle_event(ev: notify::Event, pending: &Arc<Mutex<HashMap<PathBuf, Pending>>>) {
    if !matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_)) {
        // Remove events clear the pending entry (file deleted mid-download).
        if matches!(ev.kind, EventKind::Remove(_)) {
            let mut g = pending.lock().unwrap();
            for p in &ev.paths {
                g.remove(p);
            }
        }
        return;
    }
    for p in &ev.paths {
        let name = match p.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if analyze::is_temp_filename(name) {
            continue;
        }
        if !p.is_file() {
            continue;
        }
        let size = match std::fs::metadata(p) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        let mut g = pending.lock().unwrap();
        let entry = g.entry(p.clone()).or_insert(Pending { last_change: Instant::now(), last_size: 0 });
        if entry.last_size != size {
            // size changed → reset the stability clock
            entry.last_size = size;
            entry.last_change = Instant::now();
        }
    }
}

/// Take out paths whose size has been stable for ≥ STABLE_FOR.
fn drain_stable(pending: &Arc<Mutex<HashMap<PathBuf, Pending>>>) -> Vec<PathBuf> {
    let now = Instant::now();
    let mut g = pending.lock().unwrap();
    let mut ready = Vec::new();
    g.retain(|path, p| {
        if now.duration_since(p.last_change) >= STABLE_FOR {
            // Confirm the file still exists and size matches (not deleted/truncated).
            if let Ok(m) = std::fs::metadata(path) {
                if m.len() == p.last_size {
                    ready.push(path.clone());
                    return false; // remove from pending
                }
            }
            // file gone or size changed → keep pending? drop it; next event re-adds.
            return false;
        }
        true
    });
    ready
}

/// Analyze + rule-match + insert into inbox. Returns the new record id, or
/// None if the file was a duplicate (already recorded for this path) or
/// failed to process. Shared by the watcher loop and `scan_now`.
pub async fn process_path(app: AppHandle, path: PathBuf) -> Option<i64> {
    let path_str = path.to_string_lossy().to_string();

    // Duplicate check: this exact path already recorded → skip.
    {
        let s = app.state::<AppState>();
        let guard = s.store().ok()?;
        let store = guard.as_ref().unwrap();
        if store.exists_for_path(&path_str).unwrap_or(false) {
            return None;
        }
    }

    let cfg = { let s = app.state::<AppState>(); s.load_cfg().ok()? };
    let tz = cfg.tz();
    let auto_file = cfg.auto_file;

    // Stream-analyze off the async thread. analyze_file reads only 512 B for
    // magic + hashes in chunks, so a multi-GB file can't OOM or hang here.
    let path_for_blocking = path.clone();
    let pr = tokio::task::spawn_blocking(move || -> Option<ProcessResult> {
        let filename = path_for_blocking
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let (sha, meta) = analyze::analyze_file(&path_for_blocking, &filename).ok()?;
        let size = std::fs::metadata(&path_for_blocking).map(|m| m.len() as i64).unwrap_or(0);
        let ext = meta.ext.clone();
        let suggestion = rules::match_first(&cfg.rules, &filename, &ext, &meta)
            .map(|r| rules::build_suggestion(r, &cfg, &meta, &filename));
        Some(ProcessResult {
            sha,
            meta,
            filename,
            size,
            suggestion,
        })
    })
    .await
    .ok()??;

    let now = timeutil::now_rfc3339(tz);
    let sub_meta_json = serde_json::to_string(&pr.meta).unwrap_or_else(|_| "{}".into());
    let (category, rule_id, dest, fname) = match &pr.suggestion {
        Some(s) => (s.category.clone(), s.rule_id.clone(), s.dest_dir.clone(), s.filename.clone()),
        None => (String::new(), String::new(), String::new(), String::new()),
    };

    let id = {
        let s = app.state::<AppState>();
        let guard = s.store().ok()?;
        let store = guard.as_ref().unwrap();
        // Re-check under the lock to avoid a race with a concurrent event.
        if store.exists_for_path(&path_str).unwrap_or(false) {
            return None;
        }
        // Dedup: is there already a filed record with the same content hash?
        // (Content dedup is necessarily post-download — we need the bytes.)
        let duplicate_of = store
            .find_filed_by_sha(&pr.sha)
            .ok()
            .flatten()
            .map(|r| r.id)
            .unwrap_or(0);
        let nr = NewRecord {
            sha256: pr.sha,
            original_path: path_str,
            original_filename: pr.filename,
            size_bytes: pr.size,
            detected_at: now,
            category,
            sub_meta: sub_meta_json,
            rule_id,
            suggested_dest: dest,
            suggested_filename: fname,
            duplicate_of,
        };
        match store.insert_inbox(&nr) {
            Ok(id) => id,
            Err(e) => {
                eprintln!("[watcher] insert failed for {}: {e}", nr.original_path);
                return None;
            }
        }
    };

    let _ = app.emit("new-inbox-item", id);
    // If auto-file is on, pop the confirm modal (semi-automatic: pre-filled
    // suggestion, user adds searchable metadata, then files).
    if auto_file {
        let _ = app.emit("auto-file-prompt", id);
    }
    Some(id)
}

/// One-shot scan of the watch dir: backfill any non-temp files not already
/// recorded. Used by the `scan_now` command and the `--scan` launch flag.
///
/// Each file is wrapped in a 60s timeout so one stuck/unreadable file can't
/// make the whole scan hang (the original "扫描不结束" bug). Progress is
/// emitted as `scan-progress` so the UI can show X/Y.
pub async fn scan_now(app: AppHandle, watch_dir: String) -> anyhow::Result<usize> {
    let candidates: Vec<PathBuf> = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&watch_dir)?.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if analyze::is_temp_filename(name) {
                continue;
            }
            out.push(p);
        }
        Ok(out)
    })
    .await??;

    let total = candidates.len();
    let mut added = 0usize;
    for (i, p) in candidates.into_iter().enumerate() {
        // Per-file timeout: a single bad file must not block the scan.
        let res = tokio::time::timeout(Duration::from_secs(60), process_path(app.clone(), p.clone())).await;
        match res {
            Ok(Some(_id)) => added += 1,
            Ok(None) => {} // skipped (duplicate path / read error / not new)
            Err(_elapsed) => {
                eprintln!("[scan] timeout on {}, skipping", p.display());
            }
        }
        let _ = app.emit("scan-progress", ScanProgress {
            processed: i + 1,
            total,
            added,
        });
    }
    Ok(added)
}

#[derive(Serialize, Clone)]
struct ScanProgress {
    processed: usize,
    total: usize,
    added: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_stable_removes_old_entries() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        // Insert a "stale" entry by backdating last_change via a fake file.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hello").unwrap();
        let size = std::fs::metadata(tmp.path()).unwrap().len();
        {
            let mut g = pending.lock().unwrap();
            g.insert(tmp.path().to_path_buf(), Pending {
                last_change: Instant::now() - Duration::from_secs(10),
                last_size: size,
            });
        }
        let ready = drain_stable(&pending);
        assert_eq!(ready.len(), 1);
        assert!(pending.lock().unwrap().is_empty());
    }

    #[test]
    fn drain_stable_keeps_fresh_entries() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"x").unwrap();
        let size = std::fs::metadata(tmp.path()).unwrap().len();
        {
            let mut g = pending.lock().unwrap();
            g.insert(tmp.path().to_path_buf(), Pending {
                last_change: Instant::now(),
                last_size: size,
            });
        }
        let ready = drain_stable(&pending);
        assert!(ready.is_empty());
        assert_eq!(pending.lock().unwrap().len(), 1);
    }
}
