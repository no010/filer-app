import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { check as checkUpdate, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import type { Config, Record as Rec } from "./types";
import { countStale, getRecord, importPath, loadConfig, onAutoFilePrompt, onConfigUpdated, onItemUpdated, onNewInboxItem, onProcessError } from "./api";
import { Inbox } from "./components/Inbox";
import { History } from "./components/History";
import { Review } from "./components/Review";
import { ReviewModal } from "./components/ReviewModal";
import { Search } from "./components/Search";
import { Stats } from "./components/Stats";
import { SettingsModal } from "./components/SettingsModal";

type Tab = "inbox" | "history" | "search" | "review" | "stats";

export default function App() {
  const [cfg, setCfg] = useState<Config | null>(null);
  const [tab, setTab] = useState<Tab>("inbox");
  const [showSettings, setShowSettings] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [staleCount, setStaleCount] = useState(0);
  const [autoQueue, setAutoQueue] = useState<Rec[]>([]);
  const [toast, setToast] = useState<string | null>(null);
  const [upd, setUpd] = useState<Update | null>(null);
  const [updBusy, setUpdBusy] = useState(false);

  // Check for an app update on startup (Tauri updater; signature verified
  // against the pubkey in tauri.conf.json). Failures are silent — no network
  // = no update check, not an error.
  useEffect(() => {
    checkUpdate().then((u) => { if (u?.available) setUpd(u); }).catch(() => {});
  }, []);

  useEffect(() => {
    loadConfig().then(setCfg).catch(() => setCfg(null));
    const un1 = onNewInboxItem(() => setRefreshKey((k) => k + 1));
    const un2 = onItemUpdated(() => setRefreshKey((k) => k + 1));
    // auto-file: watcher emits the id → fetch the record → queue the modal.
    const un3 = onAutoFilePrompt(async (id) => {
      const r = await getRecord(id);
      if (r) setAutoQueue((q) => [...q, r]);
    });
    // Processing/scan failures surface as a transient toast.
    const un4 = onProcessError((p) => {
      const name = p.path ? p.path.replace(/^.*[\\/]/, "") : "";
      setToast(`${name}: ${p.message}`);
    });
    // Config changed on the Rust side (first-close tray prompt) → reload so
    // the Settings checkbox stays in sync with the persisted choice.
    const un5 = onConfigUpdated(() => { loadConfig().then(setCfg).catch(() => {}); });
    // Drag-and-drop files onto the window → import each into the inbox.
    let unDrop: (() => void) | undefined;
    getCurrentWebview().onDragDropEvent((e: any) => {
      if (e?.payload?.type === "drop" && e.payload.paths?.length) {
        let n = 0;
        for (const p of e.payload.paths as string[]) {
          importPath(p).then((id) => { if (id != null) n++; }).catch(() => {});
        }
        setToast(`已导入 ${e.payload.paths.length} 个文件到收件箱`);
      }
    }).then((un: () => void) => { unDrop = un; });
    return () => {
      un1.then((fn) => fn()).catch(() => {});
      un2.then((fn) => fn()).catch(() => {});
      un3.then((fn) => fn()).catch(() => {});
      un4.then((fn) => fn()).catch(() => {});
      un5.then((fn) => fn()).catch(() => {});
      unDrop?.();
    };
  }, []);

  // Auto-clear the toast after 6s.
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 6000);
    return () => clearTimeout(t);
  }, [toast]);

  // Launch banner: count filed files untouched for >180d. Refreshed with the
  // shared refreshKey so acting on an item re-tallies.
  useEffect(() => {
    if (cfg && cfg.watch_dir.trim() && cfg.dest_root.trim()) {
      countStale(180).then(setStaleCount).catch(() => setStaleCount(0));
    }
  }, [cfg, refreshKey]);

  if (cfg === null) {
    return <div className="flex h-full items-center justify-center text-sm text-slate-400">加载中…</div>;
  }

  const configured = cfg.watch_dir.trim() !== "" && cfg.dest_root.trim() !== "";
  if (!configured) {
    return (
      <SettingsModal
        initial={cfg}
        force
        onClose={() => {}}
        onSaved={(c) => setCfg(c)}
      />
    );
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-slate-200 bg-white px-4 py-2">
        <div className="flex items-center gap-2">
          <span className="text-base font-semibold text-slate-800">filer</span>
          <span className="text-xs text-slate-400">下载整理</span>
        </div>
        <div className="flex items-center gap-3 text-xs">
          <span className="text-slate-500">{cfg.member || "未署名"}</span>
          <button
            onClick={() => setShowSettings(true)}
            className="rounded bg-slate-100 px-2 py-1 text-slate-600 hover:bg-slate-200"
          >设置</button>
        </div>
      </header>

      <nav className="flex gap-1 border-b border-slate-200 bg-white px-4">
        {(["inbox", "history", "search", "review", "stats"] as Tab[]).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`-mb-px border-b-2 px-3 py-2 text-sm ${
              tab === t
                ? "border-slate-800 text-slate-800"
                : "border-transparent text-slate-400 hover:text-slate-600"
            }`}
          >
            {t === "inbox" ? "收件箱" : t === "history" ? "历史" : t === "search" ? "搜索" : t === "review" ? `回顾${staleCount > 0 ? ` · ${staleCount}` : ""}` : "统计"}
          </button>
        ))}
      </nav>

      {staleCount > 0 && (
        <div className="border-b border-amber-200 bg-amber-50 px-4 py-1.5 text-xs text-amber-700">
          ⚠ 你有 {staleCount} 个已归档文件超过 180 天未触达，到「回顾」tab 处理一下。
        </div>
      )}

      <div className="flex-1 overflow-hidden">
        {tab === "inbox" ? (
          <Inbox refreshKey={refreshKey} />
        ) : tab === "history" ? (
          <History refreshKey={refreshKey} />
        ) : tab === "search" ? (
          <Search refreshKey={refreshKey} />
        ) : tab === "review" ? (
          <Review refreshKey={refreshKey} />
        ) : (
          <Stats refreshKey={refreshKey} />
        )}
        <span className="hidden">{refreshKey}</span>
      </div>

      {showSettings && (
        <SettingsModal
          initial={cfg}
          onClose={() => setShowSettings(false)}
          onSaved={(c) => { setCfg(c); setShowSettings(false); }}
        />
      )}

      {autoQueue.length > 0 && (
        <ReviewModal
          record={autoQueue[0]}
          title={`归档确认（自动归档，剩余 ${autoQueue.length - 1} 项待处理）`}
          onClose={() => setAutoQueue((q) => q.slice(1))}
          onFiled={async () => {
            setAutoQueue((q) => q.slice(1));
            setRefreshKey((k) => k + 1);
          }}
        />
      )}

      {toast && (
        <div className="fixed bottom-4 right-4 z-50 max-w-md rounded-lg bg-red-600 px-4 py-2 text-sm text-white shadow-lg">
          {toast}
        </div>
      )}

      {upd && (
        <div className="fixed bottom-4 left-1/2 z-50 -translate-x-1/2 rounded-lg bg-slate-800 px-4 py-2 text-sm text-white shadow-lg">
          <span className="mr-3">新版本 filer {upd.version} 可用</span>
          <button
            disabled={updBusy}
            onClick={async () => {
              setUpdBusy(true);
              try {
                await upd.downloadAndInstall();
                await relaunch();
              } catch (e: any) {
                setToast(`更新失败：${e.message || e}`);
                setUpdBusy(false);
              }
            }}
            className="rounded bg-white px-2 py-1 text-xs text-slate-800 hover:bg-slate-100 disabled:opacity-40"
          >{updBusy ? "下载安装中…" : "下载并重启"}</button>
          <button onClick={() => setUpd(null)} className="ml-2 text-slate-400 hover:text-white">稍后</button>
        </div>
      )}
    </div>
  );
}
