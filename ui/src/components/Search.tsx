import { useCallback, useEffect, useState } from "react";
import type { Record as Rec } from "../types";
import { openPath, search, undoFile } from "../api";

function parseTags(s: string): string[] {
  try { const r = JSON.parse(s); return Array.isArray(r) ? r : []; } catch { return []; }
}
function fmtTime(rfc: string): string {
  return rfc.length >= 16 ? rfc.slice(0, 10) + " " + rfc.slice(11, 16) : rfc || "—";
}

export function Search({ refreshKey }: { refreshKey: number }) {
  const [q, setQ] = useState("");
  const [results, setResults] = useState<Rec[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<number | null>(null);

  const run = useCallback(async (query: string) => {
    if (query.trim().length === 0) { setResults([]); setError(null); return; }
    setError(null);
    try { setResults(await search(query)); }
    catch (e: any) { setError(e.message || String(e)); }
  }, []);

  useEffect(() => { run(q); /* re-run on refreshKey in case records changed */ }, [refreshKey, run, q]);

  const open = async (r: Rec) => {
    setBusyId(r.id);
    try { await openPath(r.filed_path); } catch (e: any) { setError(e.message || String(e)); }
    finally { setBusyId(null); }
  };
  const undo = async (r: Rec) => {
    setBusyId(r.id);
    try { await undoFile(r.id); await run(q); } catch (e: any) { setError(e.message || String(e)); }
    finally { setBusyId(null); }
  };

  return (
    <div className="flex h-full flex-col bg-slate-50">
      <div className="border-b border-slate-200 bg-white px-4 py-2">
        <input
          autoFocus
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") run(q); }}
          placeholder="搜索归档文件：文件名 / 分类 / 标签 / 厂商 / 路径"
          className="w-full rounded border border-slate-300 px-3 py-1.5 text-sm"
        />
        <div className="mt-1 text-[11px] text-slate-400">
          {results.length > 0 ? `${results.length} 条结果` : q.trim() ? "无结果" : "输入关键词回车搜索"}
        </div>
      </div>

      {error && <div className="mx-4 mt-2 rounded bg-amber-50 px-2 py-1 text-xs text-amber-700">{error}</div>}

      <div className="flex-1 overflow-y-auto px-4 py-2">
        <table className="w-full text-left text-sm">
          <thead className="text-xs text-slate-400">
            <tr className="border-b border-slate-200">
              <th className="py-2 pr-2 font-medium">文件名</th>
              <th className="px-2 font-medium">分类</th>
              <th className="px-2 font-medium">归档路径</th>
              <th className="px-2 font-medium">归档时间</th>
              <th className="px-2 font-medium text-right">操作</th>
            </tr>
          </thead>
          <tbody>
            {results.map((r) => {
              const tags = parseTags(r.tags);
              const busy = busyId === r.id;
              return (
                <tr key={r.id} className="border-b border-slate-100 align-top">
                  <td className="py-2 pr-2">
                    <div className="truncate text-slate-800" style={{ maxWidth: 220 }} title={r.original_filename}>
                      {r.original_filename}
                    </div>
                    {tags.length > 0 && <div className="text-[11px] text-blue-700">{tags.join(" ")}</div>}
                  </td>
                  <td className="px-2 text-slate-600">{r.category || "—"}</td>
                  <td className="px-2 text-slate-600 break-all" style={{ maxWidth: 260 }}>{r.filed_path}</td>
                  <td className="px-2 text-slate-500 whitespace-nowrap">{fmtTime(r.filed_at)}</td>
                  <td className="px-2 text-right">
                    <div className="inline-flex gap-1">
                      <button onClick={() => open(r)} disabled={busy}
                        className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-600 hover:bg-slate-200 disabled:opacity-40">打开</button>
                      <button onClick={() => undo(r)} disabled={busy}
                        className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-500 hover:bg-slate-200 disabled:opacity-40">撤销</button>
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
