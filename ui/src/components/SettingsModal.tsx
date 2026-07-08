import { useEffect, useState } from "react";
import type { Config } from "../types";
import { pickFolder, removeContextMenu, saveConfig } from "../api";
import { RulesEditor } from "./RulesEditor";

const COMMON_TZ = [
  "Asia/Shanghai", "Asia/Tokyo", "Asia/Singapore", "Asia/Kolkata",
  "Europe/London", "Europe/Berlin", "America/New_York", "America/Chicago",
  "America/Los_Angeles", "America/Sao_Paulo", "UTC",
];

/** First-run wizard +日常设置。force=true 时不可关闭直到保存。 */
export function SettingsModal(props: {
  initial: Config;
  force?: boolean;
  onClose: () => void;
  onSaved: (c: Config) => void;
}) {
  const { initial, force, onClose, onSaved } = props;
  const [cfg, setCfg] = useState<Config>(initial);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => { setCfg(initial); }, [initial]);

  const set = (patch: Partial<Config>) => setCfg((c) => ({ ...c, ...patch }));

  const pick = async (key: "watch_dir" | "dest_root") => {
    const p = await pickFolder();
    if (p) set({ [key]: p } as Partial<Config>);
  };

  const configured = cfg.watch_dir.trim() !== "" && cfg.dest_root.trim() !== "";

  const save = async () => {
    if (!configured) { setErr("请先选择下载监听目录"); return; }
    setSaving(true); setErr(null);
    try {
      await saveConfig(cfg);
      onSaved(cfg);
    } catch (e: any) {
      setErr(e.message || String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="flex max-h-[90vh] w-[640px] flex-col overflow-hidden rounded-lg bg-white shadow-xl">
        <div className="flex items-center justify-between border-b border-slate-200 px-5 py-3">
          <div>
            <h2 className="text-sm font-semibold text-slate-800">{force ? "首次启动配置" : "设置"}</h2>
            {force && <p className="text-xs text-amber-600">⚠ 需配置下载监听目录后才能进入应用</p>}
          </div>
          {!force && (
            <button onClick={onClose} className="text-slate-400 hover:text-slate-600">✕</button>
          )}
        </div>

        <div className="flex-1 overflow-y-auto px-5 py-4 text-sm text-slate-700">
          {/* 监听目录 */}
          <Section title="下载监听目录">
            <PathRow label="watch_dir" value={cfg.watch_dir} onPick={() => pick("watch_dir")}
                     onClear={() => set({ watch_dir: "" })} placeholder="如 C:\Users\You\Downloads" />
          </Section>

          {/* 单一目标根目录 */}
          <Section title="归档根目录（所有文件按规则落到此目录下的子文件夹）">
            <PathRow label="dest_root" value={cfg.dest_root} onPick={() => pick("dest_root")}
                     onClear={() => set({ dest_root: "" })} placeholder="如 D:\Filer" />
            <p className="mt-1 text-[11px] text-slate-400">
              规则的子路径会拼到此目录下，例如 D:\Filer\Datasheets\ST\、D:\Filer\Receipts\2026-07\
            </p>
          </Section>

          {/* 时区与策略 */}
          <Section title="时区与归档策略">
            <label className="mb-1 block text-xs text-slate-500">时区（留空=系统本地时区）</label>
            <div className="mb-1 flex gap-2">
              <input
                value={cfg.timezone}
                onChange={(e) => set({ timezone: e.target.value })}
                placeholder="系统时区"
                className="flex-1 rounded border border-slate-300 px-2 py-1 text-sm"
              />
              <select
                value=""
                onChange={(e) => e.target.value && set({ timezone: e.target.value })}
                className="rounded border border-slate-300 px-2 py-1 text-sm text-slate-500"
              >
                <option value="">常用…</option>
                {COMMON_TZ.map((tz) => <option key={tz} value={tz}>{tz}</option>)}
              </select>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <label className="block">
                <span className="mb-1 block text-xs text-slate-500">冲突策略</span>
                <select
                  value={cfg.conflict_strategy}
                  onChange={(e) => set({ conflict_strategy: e.target.value })}
                  className="w-full rounded border border-slate-300 px-2 py-1 text-sm"
                >
                  <option value="rename">rename（加后缀）</option>
                  <option value="skip">skip（跳过）</option>
                  <option value="overwrite">overwrite（覆盖）</option>
                </select>
              </label>
              <label className="block">
                <span className="mb-1 block text-xs text-slate-500">默认动作</span>
                <select
                  value={cfg.default_action}
                  onChange={(e) => set({ default_action: e.target.value })}
                  className="w-full rounded border border-slate-300 px-2 py-1 text-sm"
                >
                  <option value="move">move（移动）</option>
                  <option value="copy">copy（复制）</option>
                </select>
              </label>
            </div>
          </Section>

          {/* 杂项 */}
          <Section title="其他">
            <label className="mb-2 block">
              <span className="mb-1 block text-xs text-slate-500">署名（可选，用于历史归因）</span>
              <input
                value={cfg.member}
                onChange={(e) => set({ member: e.target.value })}
                placeholder="你的名字"
                className="w-full rounded border border-slate-300 px-2 py-1 text-sm"
              />
            </label>
            <label className="flex items-center gap-2 text-xs text-slate-600">
              <input type="checkbox" checked={cfg.autostart}
                     onChange={(e) => set({ autostart: e.target.checked })} />
              开机自动启动
            </label>
            <label className="mt-2 flex items-start gap-2 text-xs text-slate-600">
              <input type="checkbox" checked={cfg.auto_file}
                     onChange={(e) => set({ auto_file: e.target.checked })} className="mt-0.5" />
              <span>
                自动归档（下载完成弹确认小窗，预填建议，补完关键字段回车即归档）<br/>
                <span className="text-slate-400">关闭则新文件静默进收件箱待手动处理</span>
              </span>
            </label>
            <div className="mt-3 flex items-center gap-2 text-xs">
              <button
                onClick={() => removeContextMenu().then(() => setErr("右键菜单已移除")).catch((e) => setErr(e.message || String(e)))}
                className="rounded bg-slate-100 px-2 py-1 text-slate-600 hover:bg-slate-200"
              >移除右键「用 filer 归档」</button>
              <span className="text-slate-400">卸载时会自动清理；这里可手动移除。</span>
            </div>
          </Section>

          {/* 规则编辑 */}
          <Section title={`规则集（${cfg.rules.length} 条，按顺序匹配）`}>
            <RulesEditor rules={cfg.rules} onChange={(rules) => set({ rules })} />
          </Section>
        </div>

        {err && <div className="mx-5 mb-2 rounded bg-red-50 px-2 py-1 text-xs text-red-600">{err}</div>}

        <div className="flex justify-end gap-2 border-t border-slate-200 px-5 py-3">
          {!force && (
            <button onClick={onClose} className="rounded px-3 py-1.5 text-sm text-slate-500 hover:bg-slate-100">取消</button>
          )}
          <button
            onClick={save}
            disabled={saving || !configured}
            className="rounded bg-slate-800 px-4 py-1.5 text-sm text-white hover:bg-slate-700 disabled:opacity-40"
          >
            {saving ? "保存中…" : force ? "保存并进入应用" : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="mb-4">
      <h3 className="mb-2 text-xs font-semibold text-slate-500">{title}</h3>
      {children}
    </div>
  );
}

function PathRow({ label, value, onPick, onClear, placeholder }: {
  label: string; value: string; onPick: () => void; onClear: () => void; placeholder?: string;
}) {
  return (
    <div className="mb-2 flex items-center gap-2">
      <span className="w-56 shrink-0 text-xs text-slate-500">{label}</span>
      <input
        value={value}
        readOnly
        placeholder={placeholder}
        className="flex-1 rounded border border-slate-300 px-2 py-1 text-sm text-slate-700"
      />
      <button onClick={onPick} className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-600 hover:bg-slate-200">选择…</button>
      {value && <button onClick={onClear} className="text-xs text-slate-400 hover:text-slate-600">清空</button>}
    </div>
  );
}
