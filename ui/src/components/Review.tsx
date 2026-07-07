import { useCallback, useEffect, useState } from "react";
import type { ReviewRow } from "../types";
import { deleteFiledFile, listReview, openRecord, touchReviewed, undoFile } from "../api";

/** Memory-curve decay tiers (days since last touch). */
function tier(days: number): { label: string; cls: string } {
  if (days < 30) return { label: "近期", cls: "bg-emerald-50 text-emerald-700" };
  if (days < 90) return { label: "未用", cls: "bg-slate-100 text-slate-600" };
  if (days < 180) return { label: "久未打开", cls: "bg-amber-50 text-amber-700" };
  if (days < 365) return { label: "建议清理", cls: "bg-orange-100 text-orange-700" };
  return { label: "很可能不需要", cls: "bg-red-100 text-red-700" };
}

function parseTags(s: string): string[] {
  try { const r = JSON.parse(s); return Array.isArray(r) ? r : []; } catch { return []; }
}

function fmtTime(rfc: string): string {
  return rfc.length >= 16 ? rfc.slice(0, 10) + " " + rfc.slice(11, 16) : rfc || "—";
}

export function Review({ refreshKey }: { refreshKey: number }) {
  const [rows, setRows] = useState<ReviewRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try { setRows(await listReview()); }
    catch (e: any) { setError(e.message || String(e)); }
  }, []);

  useEffect(() => { refresh(); }, [refresh, refreshKey]);

  const open = async (r: ReviewRow) => {
    setBusyId(r.id); setError(null);
    try { await openRecord(r.id); await refresh(); }
    catch (e: any) { setError(e.message || String(e)); }
    finally { setBusyId(null); }
  };
  const keep = async (r: ReviewRow) => {
    setBusyId(r.id); setError(null);
    try { await touchReviewed(r.id); await refresh(); }
    catch (e: any) { setError(e.message || String(e)); }
    finally { setBusyId(null); }
  };
  const del = async (r: ReviewRow) => {
    if (!confirm(`确认删除归档文件？\n${r.filed_path}`)) return;
    setBusyId(r.id); setError(null);
    try { await deleteFiledFile(r.id); await refresh(); }
    catch (e: any) { setError(e.message || String(e)); }
    finally { setBusyId(null); }
  };
  const undo = async (r: ReviewRow) => {
    setBusyId(r.id); setError(null);
    try { await undoFile(r.id); await refresh(); }
    catch (e: any) { setError(e.message || String(e)); }
    finally { setBusyId(null); }
  };

  return (
    <div className="flex h-full flex-col bg-slate-50">
      <div className="border-b border-slate-200 bg-white px-4 py-2 text-xs text-slate-500">
        回顾 · {rows.length} 个已归档文件，按陈旧度排序（最久未触达在前）
      </div>

      {error && <div className="mx-4 mt-2 rounded bg-amber-50 px-2 py-1 text-xs text-amber-700">{error}</div>}

      <div className="flex-1 overflow-y-auto px-4 py-2">
        {rows.length === 0 && (
          <div className="py-10 text-center text-xs text-slate-400">尚无可回顾的已归档文件</div>
        )}
        <table className="w-full text-left text-sm">
          <thead className="text-xs text-slate-400">
            <tr className="border-b border-slate-200">
              <th className="py-2 pr-2 font-medium">文件名</th>
              <th className="px-2 font-medium">归档路径</th>
              <th className="px-2 font-medium">最近触达</th>
              <th className="px-2 font-medium">陈旧度</th>
              <th className="px-2 font-medium">状态</th>
              <th className="px-2 font-medium text-right">操作</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => {
              const t = tier(r.staleness_days);
              const tags = parseTags(r.tags);
              const lastTouch = r.last_opened_at || r.last_reviewed_at || r.filed_at;
              const busy = busyId === r.id;
              return (
                <tr key={r.id} className="border-b border-slate-100 align-top">
                  <td className="py-2 pr-2">
                    <div className="truncate text-slate-800" style={{ maxWidth: 200 }} title={r.original_filename}>
                      {r.original_filename}
                    </div>
                    <div className="text-[11px] text-slate-400">
                      {r.category || "—"} · {r.action}
                      {tags.length > 0 && ` · ${tags.join(" ")}`}
                    </div>
                  </td>
                  <td className="px-2 text-slate-600 break-all" style={{ maxWidth: 240 }}>{r.filed_path}</td>
                  <td className="px-2 text-slate-500 whitespace-nowrap">{fmtTime(lastTouch)}</td>
                  <td className="px-2">
                    <span className={`inline-block rounded px-1.5 py-0.5 text-[11px] ${t.cls}`}>{t.label}</span>
                    <div className="text-[11px] text-slate-400">{r.staleness_days} 天</div>
                  </td>
                  <td className="px-2 text-[11px]">
                    {r.file_missing && <span className="text-red-600">文件缺失</span>}
                    {!r.file_missing && r.updated_since_filed && <span className="text-emerald-600">已更新</span>}
                    {!r.file_missing && !r.updated_since_filed && <span className="text-slate-400">未改动</span>}
                  </td>
                  <td className="px-2 text-right">
                    <div className="inline-flex flex-wrap gap-1 justify-end">
                      <button onClick={() => open(r)} disabled={busy}
                        className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-600 hover:bg-slate-200 disabled:opacity-40">打开</button>
                      <button onClick={() => keep(r)} disabled={busy}
                        className="rounded bg-emerald-100 px-2 py-1 text-xs text-emerald-700 hover:bg-emerald-200 disabled:opacity-40">仍需要</button>
                      <button onClick={() => undo(r)} disabled={busy}
                        className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-500 hover:bg-slate-200 disabled:opacity-40">撤销</button>
                      <button onClick={() => del(r)} disabled={busy}
                        className="rounded bg-red-100 px-2 py-1 text-xs text-red-700 hover:bg-red-200 disabled:opacity-40">删除文件</button>
                    </div>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
