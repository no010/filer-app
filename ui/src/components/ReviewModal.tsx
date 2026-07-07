import { useEffect, useMemo, useState } from "react";
import type { Config, Record as Rec } from "../types";
import { fileRecord, loadConfig } from "../api";

function parseTags(s: string): string[] {
  try { const r = JSON.parse(s); return Array.isArray(r) ? r : []; } catch { return []; }
}

/** 归档前编辑：分类/目标目录/文件名/动作/标签，实时预览最终路径。 */
export function ReviewModal(props: {
  record: Rec;
  title?: string;
  onClose: () => void;
  onFiled: () => void;
}) {
  const { record, title, onClose, onFiled } = props;
  const [cfg, setCfg] = useState<Config | null>(null);
  const [category, setCategory] = useState(record.category);
  const [destDir, setDestDir] = useState(record.suggested_dest);
  const [filename, setFilename] = useState(record.suggested_filename || record.original_filename);
  const [action, setAction] = useState("move");
  const [tags, setTags] = useState<string[]>(parseTags(record.tags));
  const [tagInput, setTagInput] = useState("");
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    loadConfig().then((c) => {
      setCfg(c);
      setAction(c.default_action || "move");
    }).catch(() => {});
  }, []);

  const categories = useMemo(() => {
    const set = new Set<string>();
    cfg?.rules.forEach((r) => set.add(r.category));
    return [...set];
  }, [cfg]);

  const preview = useMemo(() => {
    const sep = destDir.includes("\\") ? "\\" : "/";
    const base = destDir.replace(/[\\/]+$/, "");
    return `${base}${sep}${filename}`;
  }, [destDir, filename]);

  const addTag = () => {
    const t = tagInput.trim();
    if (t && !tags.includes(t)) setTags([...tags, t]);
    setTagInput("");
  };

  const submit = async () => {
    setSaving(true); setErr(null);
    try {
      const res = await fileRecord(record.id, {
        category: category || undefined,
        dest_dir: destDir || undefined,
        filename: filename || undefined,
        action,
        tags,
      });
      if (res.skipped) {
        setErr(res.duplicate_of
          ? `已跳过：与已归档记录 #${res.duplicate_of} 内容相同（${res.filed_path}）`
          : `已跳过：目标已存在同名文件（${res.filed_path}）`);
        return;
      }
      onFiled();
    } catch (e: any) {
      setErr(e.message || String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="w-[560px] overflow-hidden rounded-lg bg-white shadow-xl">
        <div className="flex items-center justify-between border-b border-slate-200 px-5 py-3">
          <h2 className="text-sm font-semibold text-slate-800">{title || "编辑后归档"}</h2>
          <button onClick={onClose} className="text-slate-400 hover:text-slate-600">✕</button>
        </div>

        <div className="max-h-[60vh] overflow-y-auto px-5 py-4 text-sm text-slate-700">
          <div className="mb-3 truncate text-xs text-slate-400" title={record.original_path}>
            {record.original_path}
          </div>

          <div className="grid grid-cols-2 gap-3">
            <label className="block">
              <span className="mb-1 block text-xs text-slate-500">分类</span>
              <input
                list="filer-categories"
                value={category}
                onChange={(e) => setCategory(e.target.value)}
                className="w-full rounded border border-slate-300 px-2 py-1 text-sm"
              />
              <datalist id="filer-categories">
                {categories.map((c) => <option key={c} value={c} />)}
              </datalist>
            </label>
            <label className="block">
              <span className="mb-1 block text-xs text-slate-500">动作</span>
              <select
                value={action}
                onChange={(e) => setAction(e.target.value)}
                className="w-full rounded border border-slate-300 px-2 py-1 text-sm"
              >
                <option value="move">move（移动）</option>
                <option value="copy">copy（复制）</option>
              </select>
            </label>
          </div>

          <label className="mt-3 block">
            <span className="mb-1 block text-xs text-slate-500">目标目录</span>
            <input
              value={destDir}
              onChange={(e) => setDestDir(e.target.value)}
              className="w-full rounded border border-slate-300 px-2 py-1 text-sm font-mono"
            />
          </label>

          <label className="mt-3 block">
            <span className="mb-1 block text-xs text-slate-500">文件名</span>
            <input
              value={filename}
              onChange={(e) => setFilename(e.target.value)}
              className="w-full rounded border border-slate-300 px-2 py-1 text-sm font-mono"
            />
          </label>

          <div className="mt-3">
            <span className="mb-1 block text-xs text-slate-500">标签</span>
            <div className="flex flex-wrap items-center gap-1 rounded border border-slate-300 px-2 py-1">
              {tags.map((t) => (
                <span key={t} className="inline-flex items-center rounded bg-blue-50 px-1.5 py-0.5 text-[11px] text-blue-700">
                  {t}
                  <button onClick={() => setTags(tags.filter((x) => x !== t))} className="ml-1 text-blue-400 hover:text-blue-600">×</button>
                </span>
              ))}
              <input
                value={tagInput}
                onChange={(e) => setTagInput(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); addTag(); } }}
                placeholder="+ 标签"
                className="flex-1 min-w-[80px] border-none text-[12px] outline-none"
              />
            </div>
          </div>

          <div className="mt-4 rounded bg-slate-50 px-3 py-2">
            <div className="text-xs text-slate-500">最终路径</div>
            <div className="break-all font-mono text-xs text-slate-700">{preview}</div>
          </div>
        </div>

        {err && <div className="mx-5 mb-2 rounded bg-amber-50 px-2 py-1 text-xs text-amber-700">{err}</div>}

        <div className="flex justify-end gap-2 border-t border-slate-200 px-5 py-3">
          <button onClick={onClose} className="rounded px-3 py-1.5 text-sm text-slate-500 hover:bg-slate-100">取消</button>
          <button
            onClick={submit}
            disabled={saving || !destDir.trim() || !filename.trim()}
            className="rounded bg-slate-800 px-4 py-1.5 text-sm text-white hover:bg-slate-700 disabled:opacity-40"
          >{saving ? "归档中…" : "归档"}</button>
        </div>
      </div>
    </div>
  );
}
