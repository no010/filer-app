import type { Rule } from "../types";

function splitList(s: string): string[] {
  return s.split(/[,\s]+/).map((t) => t.trim()).filter(Boolean);
}
function joinList(arr: string[]): string {
  return arr.join(", ");
}

/** 规则编辑：增删/排序/字段编辑。顺序敏感（首条命中胜出，misc 兜底放最后）。 */
export function RulesEditor({ rules, onChange }: {
  rules: Rule[];
  onChange: (rules: Rule[]) => void;
}) {
  const update = (i: number, patch: Partial<Rule>) => {
    onChange(rules.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));
  };
  const remove = (i: number) => onChange(rules.filter((_, idx) => idx !== i));
  const add = () => onChange([
    ...rules,
    { id: `rule-${Date.now()}`, category: "Misc", extensions: [], keywords: [],
      content_match: "", dest_template: "${yyyy-mm}", filename_template: "${original_name}", action: "" },
  ]);
  const move = (i: number, dir: -1 | 1) => {
    const j = i + dir;
    if (j < 0 || j >= rules.length) return;
    const next = [...rules];
    [next[i], next[j]] = [next[j], next[i]];
    onChange(next);
  };

  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <p className="text-[11px] text-slate-400">
          按顺序匹配，首条命中胜出。建议 misc 兜底放最后。
        </p>
        <button onClick={add} className="rounded bg-slate-100 px-2 py-1 text-xs text-slate-600 hover:bg-slate-200">+ 添加规则</button>
      </div>

      <div className="space-y-2">
        {rules.map((r, i) => (
          <div key={i} className="rounded border border-slate-200 p-2">
            <div className="mb-1 flex items-center gap-2">
              <span className="text-[11px] text-slate-400">#{i + 1}</span>
              <input
                value={r.id}
                onChange={(e) => update(i, { id: e.target.value })}
                placeholder="规则 id"
                className="w-28 rounded border border-slate-300 px-2 py-0.5 text-xs"
              />
              <input
                value={r.category}
                onChange={(e) => update(i, { category: e.target.value })}
                placeholder="分类"
                className="w-28 rounded border border-slate-300 px-2 py-0.5 text-xs"
              />
              <select
                value={r.content_match}
                onChange={(e) => update(i, { content_match: e.target.value })}
                className="rounded border border-slate-300 px-1 py-0.5 text-xs"
              >
                <option value="">无内容匹配</option>
                <option value="pdf_vendor">pdf 厂商识别</option>
              </select>
              <select
                value={r.action}
                onChange={(e) => update(i, { action: e.target.value })}
                className="rounded border border-slate-300 px-1 py-0.5 text-xs"
              >
                <option value="">默认动作</option>
                <option value="move">move</option>
                <option value="copy">copy</option>
              </select>
              <div className="ml-auto flex gap-1">
                <button onClick={() => move(i, -1)} disabled={i === 0} className="rounded bg-slate-100 px-1.5 py-0.5 text-xs text-slate-500 hover:bg-slate-200 disabled:opacity-30">↑</button>
                <button onClick={() => move(i, 1)} disabled={i === rules.length - 1} className="rounded bg-slate-100 px-1.5 py-0.5 text-xs text-slate-500 hover:bg-slate-200 disabled:opacity-30">↓</button>
                <button onClick={() => remove(i)} className="rounded bg-red-100 px-1.5 py-0.5 text-xs text-red-700 hover:bg-red-200">删</button>
              </div>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <label className="block">
                <span className="text-[10px] text-slate-400">扩展名（逗号分隔）</span>
                <input
                  value={joinList(r.extensions)}
                  onChange={(e) => update(i, { extensions: splitList(e.target.value) })}
                  placeholder="pdf, zip"
                  className="w-full rounded border border-slate-300 px-2 py-0.5 text-xs font-mono"
                />
              </label>
              <label className="block">
                <span className="text-[10px] text-slate-400">关键词（逗号分隔，文件名含任一即命中）</span>
                <input
                  value={joinList(r.keywords)}
                  onChange={(e) => update(i, { keywords: splitList(e.target.value) })}
                  placeholder="发票, invoice"
                  className="w-full rounded border border-slate-300 px-2 py-0.5 text-xs font-mono"
                />
              </label>
            </div>
            <label className="mt-1 block">
              <span className="text-[10px] text-slate-400">目标子路径（相对 dest_root）</span>
              <input
                value={r.dest_template}
                onChange={(e) => update(i, { dest_template: e.target.value })}
                placeholder="Datasheets\\${vendor}"
                className="w-full rounded border border-slate-300 px-2 py-0.5 text-xs font-mono"
              />
            </label>
            <label className="mt-1 block">
              <span className="text-[10px] text-slate-400">文件名模板</span>
              <input
                value={r.filename_template}
                onChange={(e) => update(i, { filename_template: e.target.value })}
                placeholder="${title_or_name}.pdf"
                className="w-full rounded border border-slate-300 px-2 py-0.5 text-xs font-mono"
              />
            </label>
          </div>
        ))}
      </div>
    </div>
  );
}
