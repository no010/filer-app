// Types mirror the Rust structs in src-tauri/src/config.rs & store.rs.

export interface Rule {
  id: string;
  category: string;
  extensions: string[];
  keywords: string[];
  content_match: string; // "" | "pdf_vendor"
  dest_template: string;
  filename_template: string;
  action: string; // "" | "move" | "copy"
}

export interface Config {
  member: string;
  autostart: boolean;
  minimize_to_tray: boolean; // close button hides to tray instead of quitting
  tray_prompted: boolean; // internal: first-close prompt shown & choice recorded (preserved on save, not UI-edited)
  timezone: string; // IANA, "" = system local
  watch_dir: string;
  dest_root: string; // single archive root
  conflict_strategy: string; // rename | skip | overwrite
  default_action: string; // move | copy
  auto_file: boolean;
  rules: Rule[];
}

export interface Record {
  id: number;
  sha256: string;
  original_path: string;
  original_filename: string;
  size_bytes: number;
  detected_at: string;
  status: string; // inbox | filed | ignored | error | missing | replaced
  category: string;
  sub_meta: string; // JSON string
  rule_id: string;
  suggested_dest: string;
  suggested_filename: string;
  filed_path: string;
  filed_at: string;
  action: string;
  tags: string; // JSON array string
  duplicate_of: number; // 0 = none; else id of the prior filed record this duplicates
  dedup_decision: string; // "" | skip | keep_both | replace | delete_new
  last_opened_at: string;
  last_reviewed_at: string;
  file_mtime_at_filed: string;
  error: string;
}

export interface ReviewRow {
  // flattened record fields
  id: number;
  sha256: string;
  original_filename: string;
  filed_path: string;
  filed_at: string;
  category: string;
  action: string;
  tags: string;
  last_opened_at: string;
  last_reviewed_at: string;
  staleness_days: number;
  updated_since_filed: boolean;
  file_missing: boolean;
}

export interface StalenessTiers {
  d30: number; d90: number; d180: number; d365: number; more: number;
}

export interface Stats {
  total_filed: number;
  total_inbox: number;
  by_category: [string, number][];
  by_action: [string, number][];
  staleness: StalenessTiers;
  last_7_days: number;
}

export interface ListInboxResult {
  records: Record[];
}

export interface ListHistoryResult {
  records: Record[];
  next: number | null;
}

export interface FileOverrides {
  category?: string;
  dest_dir?: string;
  filename?: string;
  action?: string;
  tags?: string[];
  dedup_decision?: string; // skip | keep_both | replace | delete_new
}

export interface FileResult {
  filed_path: string;
  duplicate_of?: number | null;
  skipped: boolean;
  error?: string;
}
