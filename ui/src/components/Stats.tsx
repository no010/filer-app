import { useEffect, useState } from "react";
import type { Stats as StatsT } from "../types";
import { getStats } from "../api";

const STALE_TIERS = [
  { key: "d30", label: "<30天 近期", cls: "bg-emerald-400" },
  { key: "d90", label: "30-90 未用", cls: "bg-slate-300" },
  { key: "d180", label: "90-180 久未打开", cls: "bg-amber-400" },
  { key: "d365", label: "180-365 建议清理", cls: "bg-orange-400" },
  { key: "more", label: ">365 很可能不需要", cls: "bg-red-400" },
] as const;

export function Stats({ refreshKey }: { refreshKey: number }) {
  const [s, setS] = useState<StatsT | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    getStats().then(setS).catch((e: any) => setErr(e.message || String(e)));
  }, [refreshKey]);

  if (err) return <div className="p-4 text-xs text-red-600">{err}</div>;
  if (!s) return <div className="p-4 text-xs text-slate-400">加载中…</div>;

  const maxCat = Math.max(1, ...s.by_category.map(([, n]) => n));
  const staleTotal = s.staleness.d30 + s.staleness.d90 + s.staleness.d180 + s.staleness.d365 + s.staleness.more;

  return (
    <div className="h-full overflow-y-auto bg-slate-50 p-4">
      {/* top cards */}
      <div className="grid grid-cols-4 gap-3">
        <Card label="已归档" value={s.total_filed} />
        <Card label="收件箱待办" value={s.total_inbox} />
        <Card label="近 7 天归档" value={s.last_7_days} />
        <Card label="待清理(>180天)" value={s.staleness.d365 + s.staleness.more} accent />
      </div>

      {/* by category */}
      <Section title="按分类">
        {s.by_category.length === 0 ? <Empty /> : (
          <div className="space-y-1.5">
            {s.by_category.map(([cat, n]) => (
              <div key={cat} className="flex items-center gap-2 text-xs">
                <span className="w-28 truncate text-slate-600" title={cat}>{cat || "(未分类)"}</span>
                <div className="flex-1 rounded bg-slate-100">
                  <div className="rounded bg-slate-700" style={{ width: `${(n / maxCat) * 100}%`, height: 14 }} />
                </div>
                <span className="w-8 text-right text-slate-500">{n}</span>
              </div>
            ))}
          </div>
        )}
      </Section>

      {/* by action */}
      <Section title="按动作">
        {s.by_action.length === 0 ? <Empty /> : (
          <div className="flex flex-wrap gap-2 text-xs">
            {s.by_action.map(([a, n]) => (
              <span key={a} className="rounded bg-white px-2 py-1 text-slate-600 border border-slate-200">
                {a || "—"}: <b className="text-slate-800">{n}</b>
              </span>
            ))}
          </div>
        )}
      </Section>

      {/* staleness distribution */}
      <Section title="陈旧度分布">
        {staleTotal === 0 ? <Empty /> : (
          <div className="space-y-1.5">
            {STALE_TIERS.map((t) => {
              const n = (s.staleness as any)[t.key] as number;
              const pct = staleTotal > 0 ? (n / staleTotal) * 100 : 0;
              return (
                <div key={t.key} className="flex items-center gap-2 text-xs">
                  <span className="w-32 text-slate-600">{t.label}</span>
                  <div className="flex-1 rounded bg-slate-100">
                    <div className={`rounded ${t.cls}`} style={{ width: `${pct}%`, height: 14 }} />
                  </div>
                  <span className="w-8 text-right text-slate-500">{n}</span>
                </div>
              );
            })}
          </div>
        )}
      </Section>
    </div>
  );
}

function Card({ label, value, accent }: { label: string; value: number; accent?: boolean }) {
  return (
    <div className={`rounded-lg border p-3 ${accent ? "border-orange-200 bg-orange-50" : "border-slate-200 bg-white"}`}>
      <div className="text-xs text-slate-500">{label}</div>
      <div className={`text-2xl font-semibold ${accent ? "text-orange-700" : "text-slate-800"}`}>{value}</div>
    </div>
  );
}
function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="mt-4 rounded-lg border border-slate-200 bg-white p-3">
      <h3 className="mb-2 text-xs font-semibold text-slate-500">{title}</h3>
      {children}
    </div>
  );
}
function Empty() { return <div className="text-xs text-slate-400">无数据</div>; }
