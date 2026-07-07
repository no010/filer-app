import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  Config, FileOverrides, FileResult, ListHistoryResult, ListInboxResult, Record as Rec, ReviewRow, Stats,
} from "./types";

export const loadConfig = (): Promise<Config> => invoke("load_config");
export const saveConfig = (cfg: Config): Promise<void> => invoke("save_config", { cfg });
export const ping = (): Promise<string> => invoke("ping");

/** Native OS folder picker. Returns absolute path or null if cancelled. */
export async function pickFolder(): Promise<string | null> {
  const r = await open({ directory: true, multiple: false });
  if (typeof r === "string" && r.length > 0) return r;
  return null;
}

/** Open a path in the system file manager (and select the file if given). */
export const openPath = (path: string): Promise<void> => invoke("open_path", { path });

// ---- inbox / history (Phase 5 commands; declared here for forward use) ----

export const listInbox = (): Promise<ListInboxResult> => invoke("list_inbox");
export const getRecord = (id: number): Promise<Rec | null> => invoke("get_record", { id });
export function listHistory(after: number | null, limit = 100): Promise<ListHistoryResult> {
  return invoke("list_history", { afterId: after, limit });
}
export const scanNow = (): Promise<number> => invoke("scan_now");
export function fileRecord(id: number, overrides: FileOverrides): Promise<FileResult> {
  return invoke("file_record", { id, overrides });
}
export const ignoreRecord = (id: number): Promise<void> => invoke("ignore_record", { id });
export const undoFile = (id: number): Promise<void> => invoke("undo_file", { id });
export const deleteRecord = (id: number): Promise<void> => invoke("delete_record", { id });
export const deleteSourceFile = (id: number): Promise<void> => invoke("delete_source_file", { id });
export function setTags(id: number, tags: string[]): Promise<void> {
  return invoke("set_tags", { id, tags });
}

// ---- 回顾 (staleness tracking) ----

export const listReview = (): Promise<ReviewRow[]> => invoke("list_review");
export const countStale = (days: number): Promise<number> => invoke("count_stale", { days });
export const openRecord = (id: number): Promise<void> => invoke("open_record", { id });
export const touchReviewed = (id: number): Promise<void> => invoke("touch_reviewed", { id });
export const deleteFiledFile = (id: number): Promise<void> => invoke("delete_filed_file", { id });

// ---- 检索 + 统计 ----

export function search(q: string, limit = 200): Promise<Rec[]> {
  return invoke("search", { q, limit });
}
export const getStats = (): Promise<Stats> => invoke("stats");

// ---- events from Rust ----

export function onNewInboxItem(cb: (id: number) => void): Promise<UnlistenFn> {
  return listen<number>("new-inbox-item", (e) => cb(e.payload));
}
export function onAutoFilePrompt(cb: (id: number) => void): Promise<UnlistenFn> {
  return listen<number>("auto-file-prompt", (e) => cb(e.payload));
}
export function onItemUpdated(cb: (id: number) => void): Promise<UnlistenFn> {
  return listen<number>("item-updated", (e) => cb(e.payload));
}

export interface ScanProgress { processed: number; total: number; added: number; }
export function onScanProgress(cb: (p: ScanProgress) => void): Promise<UnlistenFn> {
  return listen<ScanProgress>("scan-progress", (e) => cb(e.payload));
}
