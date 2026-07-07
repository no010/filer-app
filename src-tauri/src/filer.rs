//! The "file" action: move/copy a download into its structured destination,
//! with conflict resolution, sha256 dedup (with an explicit user decision),
//! and undo.
//!
//! Dedup is detected at insert time (the watcher sets `duplicate_of`). When a
//! record is a duplicate, the UI surfaces a prompt and the user's choice
//! (`skip` | `keep_both` | `replace` | `delete_new`) is passed via
//! `FileOverrides.dedup_decision` and recorded in the DB.
//!
//! `undo()` reverses a filed record. Both run heavy IO in `spawn_blocking`
//! and update the SQLite index + emit `item-updated` so the UI refreshes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::pathutil;
use crate::timeutil;
use crate::AppState;

/// Confirm-time overrides from the ReviewModal. All optional; empty fields
/// fall back to the stored suggestion / config default.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileOverrides {
    pub category: Option<String>,
    pub dest_dir: Option<String>,
    pub filename: Option<String>,
    pub action: Option<String>, // move | copy
    pub tags: Option<Vec<String>>,
    /// Dedup decision, required when the record is a duplicate
    /// (duplicate_of != 0): `skip` | `keep_both` | `replace` | `delete_new`.
    pub dedup_decision: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileResult {
    pub filed_path: String,
    pub duplicate_of: Option<i64>,
    pub skipped: bool,
}

/// File a record. For a duplicate, `overrides.dedup_decision` selects the
/// handling; for a non-duplicate it's ignored.
pub async fn file(app: AppHandle, id: i64, overrides: FileOverrides) -> anyhow::Result<FileResult> {
    // Load record + cfg + compute the resolved plan synchronously.
    let (record, cfg, plan) = {
        let s = app.state::<AppState>();
        let g = s.store()?;
        let store = g.as_ref().unwrap();
        let record = store.get(id)?
            .ok_or_else(|| anyhow::anyhow!("record {id} not found"))?;
        let cfg = s.load_cfg()?;
        let dest_dir = overrides.dest_dir.clone().unwrap_or_else(|| record.suggested_dest.clone());
        let filename = overrides.filename.clone().unwrap_or_else(|| record.suggested_filename.clone());
        let category = overrides.category.clone().unwrap_or_else(|| record.category.clone());
        let action = overrides.action.clone().filter(|a| !a.is_empty())
            .unwrap_or_else(|| if cfg.default_action.is_empty() { "move".into() } else { cfg.default_action.clone() });
        let tags_json = match &overrides.tags {
            Some(t) => serde_json::to_string(t).unwrap_or_else(|_| "[]".into()),
            None => record.tags.clone(),
        };
        (record, cfg, Plan { dest_dir, filename, category, action, tags_json })
    };

    let tz = cfg.tz();
    let dup_id = record.duplicate_of;
    let is_dup = dup_id != 0;
    let decision = overrides.dedup_decision.clone().unwrap_or_default();

    // Resolve the prior filed record (for skip/replace we need its path).
    let prior = if is_dup {
        let s = app.state::<AppState>();
        let g = s.store()?;
        g.as_ref().unwrap().get(dup_id).ok().flatten()
    } else {
        None
    };
    let prior_path = prior.as_ref().map(|p| p.filed_path.clone()).unwrap_or_default();

    if is_dup {
        match decision.as_str() {
            "skip" => {
                let s = app.state::<AppState>();
                let g = s.store()?;
                let store = g.as_ref().unwrap();
                store.mark_ignored(id)?;
                store.set_dedup_decision(id, "skip")?;
                let _ = app.emit("item-updated", id);
                return Ok(FileResult { filed_path: prior_path, duplicate_of: Some(dup_id), skipped: true });
            }
            "delete_new" => {
                // Delete the just-downloaded source file; keep the prior filed copy.
                let src = record.original_path.clone();
                let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                    if Path::new(&src).exists() {
                        std::fs::remove_file(&src)?;
                    }
                    Ok(())
                }).await;
                let s = app.state::<AppState>();
                let g = s.store()?;
                let store = g.as_ref().unwrap();
                store.mark_ignored(id)?;
                store.set_dedup_decision(id, "delete_new")?;
                let _ = app.emit("item-updated", id);
                return Ok(FileResult { filed_path: prior_path, duplicate_of: Some(dup_id), skipped: true });
            }
            "replace" => {
                // Delete the prior filed file, mark it replaced, then file the
                // new one normally (to its own suggested dest).
                let pp = prior_path.clone();
                let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                    if !pp.is_empty() && Path::new(&pp).exists() {
                        let _ = std::fs::remove_file(&pp);
                    }
                    Ok(())
                }).await;
                let s = app.state::<AppState>();
                let g = s.store()?;
                g.as_ref().unwrap().mark_replaced(dup_id)?;
                // fall through to the normal filing path below.
            }
            "keep_both" => {
                // fall through; force rename strategy so the new copy gets a suffix.
            }
            _ => {
                anyhow::bail!("检测到重复下载（与已归档记录 #{dup_id} 内容相同），请在收件箱选择处理方式");
            }
        }
    }

    // The dest dir must be absolute — a relative result means dest_root
    // isn't configured (the rule's sub-path has nothing to join under).
    if !Path::new(&plan.dest_dir).is_absolute() {
        let msg = "目标根目录未配置：请在设置里选择 dest_root".to_string();
        let s = app.state::<AppState>();
        let g = s.store()?;
        let _ = g.as_ref().unwrap().set_status(id, "error", &msg);
        let _ = app.emit("item-updated", id);
        anyhow::bail!(msg);
    }

    // Sanity: source must still exist (move / keep_both / replace all need it).
    if !Path::new(&record.original_path).exists() {
        let msg = format!("源文件已不存在：{}", record.original_path);
        let s = app.state::<AppState>();
        let g = s.store()?;
        let _ = g.as_ref().unwrap().set_status(id, "missing", &msg);
        let _ = app.emit("item-updated", id);
        anyhow::bail!(msg);
    }

    // Resolve conflict + move/copy off the async thread. For keep_both we
    // force `rename` so the new copy lands alongside the prior one with a
    // suffix; otherwise use the configured strategy.
    let plan_cloned = plan.clone();
    let src = record.original_path.clone();
    let strategy = if decision == "keep_both" {
        "rename".to_string()
    } else {
        cfg.conflict_strategy.clone()
    };
    let outcome = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<PathBuf>> {
        std::fs::create_dir_all(&plan_cloned.dest_dir)?;
        let target = pathutil::resolve_conflict(&plan_cloned.dest_dir, &plan_cloned.filename, &strategy);
        let target = match target {
            Some(t) => t,
            None => return Ok(None), // skip strategy + name clash
        };
        match plan_cloned.action.as_str() {
            "copy" => {
                std::fs::copy(&src, &target)?;
            }
            _ => {
                move_file(Path::new(&src), &target)?;
            }
        }
        Ok(Some(target))
    })
    .await??;

    let target = match outcome {
        Some(t) => t,
        None => {
            // skip-strategy clash: don't move, mark ignored.
            let s = app.state::<AppState>();
            let g = s.store()?;
            let _ = g.as_ref().unwrap().mark_ignored(id);
            let _ = app.emit("item-updated", id);
            return Ok(FileResult {
                filed_path: Path::new(&plan.dest_dir).join(&plan.filename).to_string_lossy().to_string(),
                duplicate_of: None,
                skipped: true,
            });
        }
    };

    let now = timeutil::now_rfc3339(tz);
    let filed_path = target.to_string_lossy().to_string();
    // Baseline the filed file's mtime at filing time (move preserves it; for
    // copy it's the freshly-copied mtime). Later the 回顾 view compares the
    // current mtime to this baseline to flag post-filing modification.
    let mtime_baseline = timeutil::mtime_rfc(&target).unwrap_or_default();
    {
        let s = app.state::<AppState>();
        let g = s.store()?;
        let store = g.as_ref().unwrap();
        store.mark_filed(id, &filed_path, &now, &plan.action, &plan.category, &plan.tags_json, &mtime_baseline)?;
        if is_dup {
            store.set_dedup_decision(id, &decision)?;
        }
    }
    let _ = app.emit("item-updated", id);
    Ok(FileResult { filed_path, duplicate_of: if is_dup { Some(dup_id) } else { None }, skipped: false })
}

/// Reverse a filed record: move (or copy-delete) the filed file back to its
/// original path, then revert the record to inbox.
pub async fn undo(app: AppHandle, id: i64) -> anyhow::Result<()> {
    let (record, action) = {
        let s = app.state::<AppState>();
        let g = s.store()?;
        let store = g.as_ref().unwrap();
        let record = store.get(id)?
            .ok_or_else(|| anyhow::anyhow!("record {id} not found"))?;
        if record.status != "filed" {
            anyhow::bail!("只能撤销已归档的记录（当前状态: {}）", record.status);
        }
        let action = record.action.clone();
        (record, action)
    };

    let filed_path = record.filed_path.clone();
    let original_path = record.original_path.clone();
    let _missing: Option<String> = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
        let filed = Path::new(&filed_path);
        if !filed.exists() {
            return Ok(Some(format!("归档文件已不存在：{filed_path}")));
        }
        match action.as_str() {
            "copy" => {
                // Undo of a copy = remove the copied file.
                std::fs::remove_file(filed)?;
            }
            _ => {
                // Undo of a move = move it back to original_path (rename-safe).
                let orig_dir = Path::new(&original_path).parent()
                    .ok_or_else(|| anyhow::anyhow!("无法解析原目录"))?;
                std::fs::create_dir_all(orig_dir)?;
                let orig_name = Path::new(&original_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| original_path.clone());
                let orig_dir_str = orig_dir.to_string_lossy().to_string();
                let target = pathutil::resolve_conflict(&orig_dir_str, &orig_name, "rename");
                if let Some(target) = target {
                    move_file(filed, &target)?;
                }
            }
        }
        Ok(None)
    })
    .await??;

    if let Some(msg) = _missing {
        // Filed file gone — mark missing but still revert the index.
        let s = app.state::<AppState>();
        let g = s.store()?;
        let _ = g.as_ref().unwrap().set_status(id, "missing", &msg);
    } else {
        let s = app.state::<AppState>();
        let g = s.store()?;
        let _ = g.as_ref().unwrap().revert_to_inbox(id);
    }
    let _ = app.emit("item-updated", id);
    Ok(())
}

#[derive(Clone)]
struct Plan {
    dest_dir: String,
    filename: String,
    category: String,
    action: String,
    tags_json: String,
}

/// `fs::rename` with a cross-device fallback (copy + delete). Windows returns
/// ERROR_NOT_SAME_DEVICE (17); Linux returns EXDEV (18); Rust 1.85+ also
/// exposes `ErrorKind::CrossesDevices`.
fn move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) => {
            let crosses = e.kind() == std::io::ErrorKind::CrossesDevices
                || matches!(e.raw_os_error(), Some(17) | Some(18));
            if crosses {
                std::fs::copy(src, dst)?;
                std::fs::remove_file(src)?;
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_file_same_device() {
        let dir = std::env::temp_dir();
        let a = dir.join(format!("filer_src_{}", std::process::id()));
        let b = dir.join(format!("filer_dst_{}", std::process::id()));
        std::fs::write(&a, b"hello").unwrap();
        move_file(&a, &b).unwrap();
        assert!(!a.exists());
        assert!(b.exists());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "hello");
        let _ = std::fs::remove_file(&b);
    }
}
