import { useCallback, useEffect, useState } from "react";
import type { Record as Rec } from "../types";
import { listHistory, openPath, setTags, undoFile } from "../api";

function parse<T>(s: string, fallback: T): T {
  try { return JSON.parse(s) as T; } catch { return fallback; }
}

function fmtSize(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(0)} KB`;
  return `${(b / 1024 / 1024).toFixed(1)} MB`;
}

function fmtTime(rfc: string): string {
  // Backend stores RFC3339 in configured tz; trim to "YYYY-MM-DD HH:MM" for display.
  return rfc.length >= 16 ? rfc.slice(0, 10) + " " + rfc.slice(11, 16) : rfc;
}

export function History({ refreshKey }: { refreshKey: number }) {
  const [records, setRecords] = useState<Rec[]>([]);
  const [next, setNext] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<number | null>(null);

  const loadFirst = useCallback(async () => {
    setError(null);
    try {
      const r = await listHistory(null, 100);
      setRecords(r.records);
      setNext(r.next);
    } catch (e: any) {
      setError(e.message || String(e));
    }
  }, []);

  useEffect(() => { loadFirst(); }, [loadFirst, refreshKey]);

  const loadMore = async () => {
    if (next == null) return;
    try {
      const r = await listHistory(next, 100);
      setRecords((prev) => [...prev, ...r.records]);
      setNext(r.next);
    } catch (e: any) { setError(e.message || String(e)); }
  };

  const undo = async (r: Rec) => {
    setBusyId(r.id); setError(null);
    try { await undoFile(r.id); await loadFirst(); }
    catch (e: any) { setError(e.message || String(e)); }
    finally { setBusyId(null); }
  };

  const open = async (r: Rec) => {
    try { await openPath(r.filed_path); }
    catch (e: any) { setError(e.message || String(e)); }
  };

  const addTag = async (r: Rec, tag: string) => {
    const t = parse<string[]>(r.tags, []);
    if (t.includes(tag) || !tag.trim()) return;
    try { await setTags(r.id, [...t, tag.trim()]); await loadFirst(); }
    catch (e: any) { setError(e.message || String(e)); }
  };
  const removeTag = async (r: Rec, tag: string) => {
    const t = parse<string[]>(r.tags, []);
    try { await setTags(r.id, t.filter((x) => x !== tag)); await loadFirst(); }
    catch (e: any) { setError(e.message || String(e)); }
  };

  return (
    <div className="flex h-full flex-col bg-slate-50">
      <div className="border-b border-slate-200 bg-white px-4 py-2 text-xs text-slate-500">
        已归档 · {records.length} 条{next != null ? "（还有更多）" : ""}
      </div>

      {error && <div className="mx-4 mt-2 rounded bg-amber-50 px-2 py-1 text-xs text-amber-700">{error}</div>}

      <div className="flex-1 overflow-y-auto px-4 py-2">
        {records.length === 0 && (
          <div className="py-10 text-center text-xs text-slate-400">尚无已归档记录</div>
        )}
        <table className="w-full text-left text-sm">
          <thead className="text-xs text-slate-400">
            <tr className="border-b border-slate-200">
              <th className="py-2 pr-2 font-medium">原文件名</th>
              <th className="px-2 font-medium">归档到</th>
              <th className="px-2 font-medium">时间</th>
              <th className="px-2 font-medium">大小</th>
              <th className="px-2 font-medium">标签</th>
              <th className="px-2 font-medium text-right">操作</th>
            </tr>
          </thead>
          <tbody>
            {records.map((r) => {
              const tags = parse<string[]>(r.tags, []);
              const busy = busyId === r.id;
              return (
                <tr key={r.id} className="border-b border-slate-100 align-top">
                  <td className="py-2 pr-2">
                    <div className="truncate text-slate-800" style={{ maxWidth: 200 }} title={r.original_filename}>
                      {r.original_filename}
                    </div>
                    <div className="text-[11px] text-slate-400">{r.category || "—"} · {r.action}</div>
                  </td>
                  <td className="px-2 text-slate-600 break-all" style={{ maxWidth: 260 }}>
                    {r.filed_path}
                  </td>
                  <td className="px-2 text-slate-500 whitespace-nowrap">{fmtTime(r.filed_at)}</td>
                  <td className="px-2 text-slate-500">{fmtSize(r.size_bytes)}</td>
                  <td className="px-2">
                    <div className="flex flex-wrap gap-1">
                      {tags.map((t) => (
                        <span key={t} className="inline-flex items-center rounded bg-blue-50 px-1.5 py-0.5 text-[11px] text-blue-700">
                          {t}
                          <button onClick={() => removeTag(r, t)} className="ml-1 text-blue-400 hover:text-blue-600">×</button>
                        </span>
                      ))}
                      <input
                        onKeyDown={(e) => {
                          const v = (e.target as HTMLInputElement).value.trim();
                          if (e.key === "Enter" && v) { addTag(r, v); (e.target as HTMLInputElement).value = ""; }
                        }}
                        placeholder="+"
                        className="w-12 rounded border border-slate-200 px-1 py-0.5 text-[11px] text-slate-600"
                      />
                    </div>
                  </td>
                  <td className="px-2 text-right">
                    <div className="inline-flex gap-1">
                      <button onClick={() => open(r)} className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-600 hover:bg-slate-200">打开</button>
                      <button
                        onClick={() => undo(r)}
                        disabled={busy}
                        className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-500 hover:bg-slate-200 disabled:opacity-40"
                      >撤销</button>
                    </div>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>

        {next != null && (
          <div className="py-3 text-center">
            <button onClick={loadMore} className="rounded bg-slate-100 px-3 py-1 text-xs text-slate-600 hover:bg-slate-200">加载更多</button>
          </div>
        )}
      </div>
    </div>
  );
}
