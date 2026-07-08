import { useCallback, useEffect, useRef, useState } from "react";
import type { Record as Rec } from "../types";
import { fileRecord, ignoreRecord, listInbox, onScanProgress, scanNow, setTags, deleteSourceFile } from "../api";
import type { ScanProgress } from "../api";
import { ReviewModal } from "./ReviewModal";

interface SubMeta {
  kind?: string; mime?: string; ext?: string;
  n_pages?: number; vendor?: string; title?: string;
  width?: number; height?: number;
}

function parse<T>(s: string, fallback: T): T {
  try { return JSON.parse(s) as T; } catch { return fallback; }
}

function fmtSize(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(0)} KB`;
  return `${(b / 1024 / 1024).toFixed(1)} MB`;
}

export function Inbox({ refreshKey, onBusy }: { refreshKey: number; onBusy?: (b: boolean) => void }) {
  const [records, setRecords] = useState<Rec[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState<Rec | null>(null);
  const [scanning, setScanning] = useState(false);
  const [scanProg, setScanProg] = useState<ScanProgress | null>(null);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [bulkBusy, setBulkBusy] = useState(false);
  const [focusIdx, setFocusIdx] = useState<number | null>(null);
  // Refs so the keydown handler (subscribed once) always sees fresh data.
  const recordsRef = useRef(records); recordsRef.current = records;
  const focusIdxRef = useRef(focusIdx); focusIdxRef.current = focusIdx;
  const reviewingRef = useRef(reviewing); reviewingRef.current = reviewing;

  // Keyboard triage: j/k move focus, f file, i ignore, e edit. Ignored while
  // typing in an input/textarea/select or when the review modal is open.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable)) return;
      if (reviewingRef.current) return;
      const recs = recordsRef.current;
      if (recs.length === 0) return;
      const cur = focusIdxRef.current ?? 0;
      const r = recs[cur];
      if (e.key === "j" || e.key === "ArrowDown") { e.preventDefault(); setFocusIdx(Math.min(cur + 1, recs.length - 1)); }
      else if (e.key === "k" || e.key === "ArrowUp") { e.preventDefault(); setFocusIdx(Math.max(cur - 1, 0)); }
      else if (e.key === "f" && r && !r.duplicate_of) { e.preventDefault(); quickFile(r); }
      else if (e.key === "i" && r) { e.preventDefault(); skip(r); }
      else if (e.key === "e" && r) { e.preventDefault(); setReviewing(r); }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
    // quickFile/skip are stable enough (useCallback refresh + setters); re-sub
    // only when record identity changes would matter — keep [] for stable handler.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const refresh = useCallback(async () => {
    try {
      const r = await listInbox();
      setRecords(r.records);
      setSelected((prev) => {
        const ids = new Set(r.records.map((x) => x.id));
        return new Set([...prev].filter((id) => ids.has(id)));
      });
    } catch (e: any) {
      setError(e.message || String(e));
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh, refreshKey]);

  useEffect(() => {
    const un = onScanProgress((p) => setScanProg(p));
    return () => { un.then((fn) => fn()).catch(() => {}); };
  }, []);

  const toggleSel = (id: number) =>
    setSelected((s) => { const n = new Set(s); n.has(id) ? n.delete(id) : n.add(id); return n; });
  const allSelected = records.length > 0 && records.every((r) => selected.has(r.id));
  const toggleAll = () =>
    setSelected(allSelected ? new Set() : new Set(records.map((r) => r.id)));

  const bulkFile = async () => {
    setBulkBusy(true); setError(null);
    let ok = 0, skipped = 0, dups = 0, failed = 0;
    for (const id of [...selected]) {
      try {
        const res = await fileRecord(id, {});
        if (res.skipped) skipped++; else ok++;
      } catch (e: any) {
        if ((e.message || "").includes("重复下载")) dups++; else failed++;
      }
    }
    setSelected(new Set()); setBulkBusy(false); await refresh();
    const parts = [`归档 ${ok}`, skipped ? `跳过 ${skipped}` : "", dups ? `重复未处理 ${dups}` : "", failed ? `失败 ${failed}` : ""].filter(Boolean);
    setError(parts.join("，"));
  };

  const bulkDelete = async () => {
    if (!confirm(`确认删除选中的 ${selected.size} 个文件？\n将删除下载源文件并从收件箱移除，不可恢复。`)) return;
    setBulkBusy(true); setError(null);
    let ok = 0, failed = 0;
    for (const id of [...selected]) {
      try { await deleteSourceFile(id); ok++; } catch { failed++; }
    }
    setSelected(new Set()); setBulkBusy(false); await refresh();
    setError(`删除 ${ok}${failed ? `，失败 ${failed}` : ""}`);
  };

  const quickFile = async (r: Rec) => {
    setBusyId(r.id); onBusy?.(true); setError(null);
    try {
      const res = await fileRecord(r.id, {});
      if (res.skipped) {
        setError(res.duplicate_of
          ? `已跳过：与已归档记录 #${res.duplicate_of} 内容相同（${res.filed_path}）`
          : `已跳过：目标已存在同名文件（${res.filed_path}）`);
      }
      await refresh();
    } catch (e: any) {
      setError(e.message || String(e));
    } finally {
      setBusyId(null); onBusy?.(false);
    }
  };

  const skip = async (r: Rec) => {
    setBusyId(r.id); setError(null);
    try { await ignoreRecord(r.id); await refresh(); }
    catch (e: any) { setError(e.message || String(e)); }
    finally { setBusyId(null); }
  };

  /** 处理重复下载：用户在 4 个动作里选一个，写回 DB。 */
  const dupFile = async (r: Rec, decision: string) => {
    setBusyId(r.id); onBusy?.(true); setError(null);
    try {
      const res = await fileRecord(r.id, { dedup_decision: decision });
      if (res.skipped) {
        const msg =
          decision === "skip" ? `已跳过：保留已归档的 #${res.duplicate_of}，新文件留在下载目录`
          : decision === "delete_new" ? `已删除新下载的文件，保留原归档 #${res.duplicate_of}`
          : `已跳过：${res.filed_path}`;
        setError(msg);
      }
      await refresh();
    } catch (e: any) { setError(e.message || String(e)); }
    finally { setBusyId(null); onBusy?.(false); }
  };

  const scan = async () => {
    setScanning(true); setError(null); setScanProg(null);
    try {
      const n = await scanNow();
      if (n > 0) await refresh();
      setError(n > 0 ? `扫描完成，新增 ${n} 项` : "扫描完成，无新文件");
    } catch (e: any) { setError(e.message || String(e)); }
    finally { setScanning(false); setScanProg(null); }
  };

  const addTag = async (r: Rec, tag: string) => {
    const t = parse<string[]>(r.tags, []);
    if (t.includes(tag) || !tag.trim()) return;
    const next = [...t, tag.trim()];
    try { await setTags(r.id, next); await refresh(); }
    catch (e: any) { setError(e.message || String(e)); }
  };

  const removeTag = async (r: Rec, tag: string) => {
    const t = parse<string[]>(r.tags, []);
    const next = t.filter((x) => x !== tag);
    try { await setTags(r.id, next); await refresh(); }
    catch (e: any) { setError(e.message || String(e)); }
  };

  return (
    <div className="flex h-full flex-col bg-slate-50">
      <div className="flex items-center justify-between border-b border-slate-200 bg-white px-4 py-2">
        <div className="text-xs text-slate-500">
          {records.length > 0 ? `收件箱 · ${records.length} 项待确认` : "收件箱为空"}
          {records.length > 0 && <span className="ml-2 text-slate-400">j/k 移动 · f 归档 · i 跳过 · e 编辑</span>}
        </div>
        <button
          onClick={scan}
          disabled={scanning}
          className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-600 hover:bg-slate-200 disabled:opacity-40"
        >{scanning && scanProg ? `扫描中… ${scanProg.processed}/${scanProg.total}（新增 ${scanProg.added}）` : scanning ? "扫描中…" : "立即扫描下载目录"}</button>
      </div>

      {error && <div className="mx-4 mt-2 rounded bg-amber-50 px-2 py-1 text-xs text-amber-700">{error}</div>}

      {selected.size > 0 && (
        <div className="mx-4 mt-2 flex items-center gap-2 rounded bg-slate-800 px-3 py-1.5 text-xs text-white">
          <span>已选 {selected.size} 项</span>
          <button onClick={bulkFile} disabled={bulkBusy}
            className="rounded bg-slate-100 px-2 py-1 text-slate-800 hover:bg-white disabled:opacity-40">一键归档</button>
          <button onClick={bulkDelete} disabled={bulkBusy}
            className="rounded bg-red-500 px-2 py-1 text-white hover:bg-red-400 disabled:opacity-40">一键删除</button>
          <button onClick={() => setSelected(new Set())} disabled={bulkBusy}
            className="rounded bg-slate-700 px-2 py-1 text-slate-200 hover:bg-slate-600 disabled:opacity-40">取消选择</button>
          {bulkBusy && <span className="text-slate-300">处理中…</span>}
        </div>
      )}

      <div className="flex-1 overflow-y-auto px-4 py-2">
        {records.length === 0 && (
          <div className="py-10 text-center text-xs text-slate-400">
            尚无待归档文件。往下载目录丢个文件试试，或点「立即扫描」回填已有文件。
          </div>
        )}
        <table className="w-full text-left text-sm">
          <thead className="text-xs text-slate-400">
            <tr className="border-b border-slate-200">
              <th className="py-2 pr-1 w-8"><input type="checkbox" checked={allSelected} onChange={toggleAll} /></th>
              <th className="py-2 pr-2 font-medium">文件名</th>
              <th className="px-2 font-medium">类型</th>
              <th className="px-2 font-medium">大小</th>
              <th className="px-2 font-medium">分类</th>
              <th className="px-2 font-medium">建议目标</th>
              <th className="px-2 font-medium">标签</th>
              <th className="px-2 font-medium text-right">操作</th>
            </tr>
          </thead>
          <tbody>
            {records.map((r, idx) => {
              const m = parse<SubMeta>(r.sub_meta, {});
              const tags = parse<string[]>(r.tags, []);
              const busy = busyId === r.id;
              const typeLabel = m.vendor ? `${m.kind || "?"} · ${m.vendor}` : (m.kind || "?");
              const checked = selected.has(r.id);
              const focused = focusIdx === idx;
              return (
                <tr key={r.id} className={`border-b border-slate-100 align-top ${checked ? "bg-blue-50/50" : ""} ${focused ? "ring-2 ring-inset ring-slate-400" : ""}`}>
                  <td className="py-2 pr-1"><input type="checkbox" checked={checked} onChange={() => toggleSel(r.id)} /></td>
                  <td className="py-2 pr-2">
                    <div className="truncate text-slate-800" style={{ maxWidth: 260 }} title={r.original_filename}>
                      {r.original_filename}
                    </div>
                    <div className="text-[11px] text-slate-400">{r.sha256.slice(0, 12)}…</div>
                  </td>
                  <td className="px-2 text-slate-600">
                    {typeLabel}
                    {m.n_pages ? <div className="text-[11px] text-slate-400">{m.n_pages} 页</div> : null}
                    {m.width ? <div className="text-[11px] text-slate-400">{m.width}×{m.height}</div> : null}
                  </td>
                  <td className="px-2 text-slate-600">{fmtSize(r.size_bytes)}</td>
                  <td className="px-2 text-slate-600">{r.category || "—"}</td>
                  <td className="px-2 text-slate-500 break-all" style={{ maxWidth: 220 }}>
                    {r.duplicate_of ? (
                      <span className="text-amber-600">⚠ 与 #{r.duplicate_of} 内容相同</span>
                    ) : (
                      r.suggested_dest || <span className="text-slate-400">（无建议，点编辑手选）</span>
                    )}
                    {r.suggested_filename && (
                      <div className="text-[11px] text-slate-400">→ {r.suggested_filename}</div>
                    )}
                  </td>
                  <td className="px-2">
                    <div className="flex flex-wrap gap-1">
                      {tags.map((t) => (
                        <span key={t} className="inline-flex items-center rounded bg-blue-50 px-1.5 py-0.5 text-[11px] text-blue-700">
                          {t}
                          <button onClick={() => removeTag(r, t)} className="ml-1 text-blue-400 hover:text-blue-600">×</button>
                        </span>
                      ))}
                      <TagAdd onAdd={(t) => addTag(r, t)} />
                    </div>
                  </td>
                  <td className="px-2 text-right">
                    {r.duplicate_of ? (
                      <div className="inline-flex flex-wrap gap-1 justify-end">
                        <button
                          onClick={() => dupFile(r, "skip")}
                          disabled={busy}
                          className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-600 hover:bg-slate-200 disabled:opacity-40"
                          title="保留已归档的旧文件，新的留在下载目录"
                        >跳过</button>
                        <button
                          onClick={() => dupFile(r, "keep_both")}
                          disabled={busy}
                          className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-600 hover:bg-slate-200 disabled:opacity-40"
                          title="新文件也归档，加后缀"
                        >保留两者</button>
                        <button
                          onClick={() => dupFile(r, "replace")}
                          disabled={busy}
                          className="rounded bg-amber-100 px-2 py-1 text-xs text-amber-700 hover:bg-amber-200 disabled:opacity-40"
                          title="删除旧归档，用新文件顶替"
                        >替换</button>
                        <button
                          onClick={() => dupFile(r, "delete_new")}
                          disabled={busy}
                          className="rounded bg-red-100 px-2 py-1 text-xs text-red-700 hover:bg-red-200 disabled:opacity-40"
                          title="删除新下载的文件，保留旧归档"
                        >删除新文件</button>
                      </div>
                    ) : (
                      <div className="inline-flex gap-1">
                        <button
                          onClick={() => quickFile(r)}
                          disabled={busy}
                          className="rounded bg-slate-800 px-2 py-1 text-xs text-white hover:bg-slate-700 disabled:opacity-40"
                        >归档</button>
                        <button
                          onClick={() => setReviewing(r)}
                          className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-600 hover:bg-slate-200"
                        >编辑</button>
                        <button
                          onClick={() => skip(r)}
                          disabled={busy}
                          className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-400 hover:bg-slate-200"
                        >跳过</button>
                      </div>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {reviewing && (
        <ReviewModal
          record={reviewing}
          onClose={() => setReviewing(null)}
          onFiled={async () => { setReviewing(null); await refresh(); }}
        />
      )}
    </div>
  );
}

function TagAdd({ onAdd }: { onAdd: (t: string) => void }) {
  const [v, setV] = useState("");
  return (
    <input
      value={v}
      onChange={(e) => setV(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && v.trim()) { onAdd(v.trim()); setV(""); }
      }}
      placeholder="+标签"
      className="w-16 rounded border border-slate-200 px-1 py-0.5 text-[11px] text-slate-600"
    />
  );
}
