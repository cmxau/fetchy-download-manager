import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { getApi, type Api } from "./api";
import type { ApiInfo, CoreEvent, DownloadInfo, Filter, Queue, GrabProject, Settings, UpdateInfo, WhenDone } from "./types";
import { DetailsPanel } from "./components/DetailsPanel";
import { SettingsPage, type Section } from "./components/SettingsPage";
import { percent } from "./format";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { Toolbar } from "./components/Toolbar";
import { DownloadTable, type SortKey } from "./components/DownloadTable";
import { ContextMenu, type CtxItem } from "./components/ContextMenu";
import { ConfirmDelete, Modal, ProjectsDialog, PropertiesDialog, UpdateDialog } from "./components/Dialogs";
import { GrabberWizard } from "./components/GrabberWizard";
import { BatchDialog } from "./components/BatchDialog";
import { Overview } from "./components/Overview";
import { QualityDialog } from "./components/QualityDialog";
import type { StreamVariant } from "./types";
import { StatusBar } from "./components/StatusBar";

type Action = { t: "set"; items: DownloadInfo[] } | { t: "ev"; e: CoreEvent };

function reduce(state: DownloadInfo[], a: Action): DownloadInfo[] {
  if (a.t === "set") return a.items;
  if (a.e.type === "removed") return state.filter((i) => i.id !== a.e.data);
  const info = a.e.data as DownloadInfo;
  const idx = state.findIndex((i) => i.id === info.id);
  if (idx < 0) return [info, ...state];
  const next = state.slice();
  next[idx] = info;
  return next;
}

const sortVal: Record<SortKey, (i: DownloadInfo) => string | number> = {
  name: (i) => i.filename.toLowerCase(),
  queue: (i) => i.queue,
  size: (i) => i.size ?? 0,
  status: (i) => percent(i),
  eta: (i) => i.eta_secs ?? Number.MAX_SAFE_INTEGER,
  speed: (i) => i.speed,
  lasttry: (i) => i.last_try_at ?? 0,
  description: (i) => i.description.toLowerCase(),
  added: (i) => i.added_at,
  saveto: (i) => i.dir.toLowerCase(),
  referer: (i) => (i.referer ?? "").toLowerCase(),
  parent: (i) => (i.parent ?? "").toLowerCase(),
};

type Dialog =
  | { k: "props"; id: string }
  | { k: "delete"; ids: string[] }
  | { k: "grab"; project?: GrabProject }
  | { k: "batch" }
  | { k: "quality"; url: string; variants: StreamVariant[] }
  | { k: "projects" }
  | { k: "update"; info: UpdateInfo; auto?: boolean }
  | { k: "power"; kind: "shutdown" | "sleep" | "hibernate" };

export default function App() {
  const [api, setApi] = useState<Api | null>(null);
  const [items, dispatch] = useReducer(reduce, []);
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [sort, setSort] = useState<{ key: SortKey; dir: 1 | -1 } | null>(null);
  const [ctx, setCtx] = useState<{ x: number; y: number; id: string } | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [view, setView] = useState<"overview" | "downloads" | "settings">("overview");
  const [section, setSection] = useState<Section>("general");
  const openSettings = (s?: Section) => {
    if (s) setSection(s);
    setView("settings");
  };
  const [settings, setSettings] = useState<Settings | null>(null);
  const theme = settings?.theme ?? "dark";
  const [isDark, setIsDark] = useState(true);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => {
      const light = theme === "light" || (theme === "system" && mq.matches);
      document.documentElement.dataset.theme = light ? "light" : "dark";
      setIsDark(!light);
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
  const [apiInfo, setApiInfo] = useState<ApiInfo | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const sidebarOpen = true;
  const [detailsClosed, setDetailsClosed] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const addRef = useRef<HTMLInputElement>(null);
  const toastTimer = useRef<number>(0);

  const say = useCallback((m: string) => {
    setToast(m);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 4000);
  }, []);

  // Boot: connect to the engine, load state, subscribe to live updates.
  useEffect(() => {
    let off: (() => void) | undefined;
    let dead = false;
    (async () => {
      const a = await getApi();
      if (dead) return;
      setApi(a);
      const resync = () => a.list().then((items) => dispatch({ t: "set", items }));
      off = await a.onEvent((e) => dispatch({ t: "ev", e }), resync);
      await resync();
      setSettings(await a.getSettings());
      a.apiInfo().then(setApiInfo).catch(() => {});
    })();
    return () => {
      dead = true;
      off?.();
    };
  }, []);


  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    let v = items.filter((i) => {
      if (filter === "unfinished" && i.status === "complete") return false;
      if (filter === "finished" && i.status !== "complete") return false;
      if (filter.startsWith("project:")) {
        if (i.project !== filter.slice(8)) return false;
      } else if (filter.startsWith("queue:")) {
        if (i.queue !== filter.slice(6) || i.status === "complete") return false;
      } else if (filter !== "all" && filter !== "unfinished" && filter !== "finished" && i.category !== filter && !(filter === "program" && i.category === "apk")) return false;
      return !q || i.filename.toLowerCase().includes(q) || i.url.toLowerCase().includes(q);
    });
    if (sort) {
      const f = sortVal[sort.key];
      v = [...v].sort((a, b) => (f(a) > f(b) ? 1 : f(a) < f(b) ? -1 : 0) * sort.dir);
    }
    return v;
  }, [items, filter, query, sort]);

  // Quick update ---------------------------------------------------------------
  const checkUpdate = async (silent: boolean) => {
    if (!api) return;
    try {
      const info = await api.checkUpdate();
      if (info) setDialog({ k: "update", info, auto: !!settings?.auto_install });
      else if (!silent) say("You are on the latest version");
    } catch (e) {
      if (!silent) say(String(e));
    }
  };
  useEffect(() => {
    if (!api || !settings || settings.update_check === "never") return;
    const every = { startup: 0, daily: 1, weekly: 7, monthly: 30 }[settings.update_check] ?? 7;
    let last = 0;
    try { last = +(localStorage.getItem("fetchy.lastUpdateCheck") ?? 0); } catch { /* storage unavailable */ }
    if (Date.now() - last < every * 86400_000) return;
    try { localStorage.setItem("fetchy.lastUpdateCheck", String(Date.now())); } catch { /* ignore */ }
    void checkUpdate(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, settings?.update_check]);

  // When all downloads finish ----------------------------------------------------
  const [whenDone, setWhenDone] = useState<WhenDone>("none");
  const busy = items.some((i) => i.status === "downloading" || i.status === "queued");
  const wasBusy = useRef(false);
  useEffect(() => {
    if (wasBusy.current && !busy && whenDone !== "none" && items.some((i) => i.status === "complete")) {
      if (whenDone === "exit") void import("@tauri-apps/api/window").then((w) => w.getCurrentWindow().close());
      else {
        setDialog({ k: "power", kind: whenDone });
        if (whenDone === "shutdown") void api?.powerAction("shutdown");
      }
      setWhenDone("none");
    }
    wasBusy.current = busy;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [busy]);

  const [projects, setProjects] = useState<GrabProject[]>([]);
  const refreshProjects = async () => { if (api) setProjects(await api.listProjects()); };
  useEffect(() => { void refreshProjects(); }, [api]); // eslint-disable-line react-hooks/exhaustive-deps

  // Per-queue "when done" actions: open a file, exit, or power off.
  const busyQueues = new Set(items.filter((i) => i.status === "downloading" || i.status === "queued").map((i) => i.queue));
  const prevBusy = useRef<Set<string>>(new Set());
  useEffect(() => {
    for (const id of prevBusy.current) {
      if (busyQueues.has(id)) continue;
      const q = settings?.queues.find((x) => x.id === id);
      if (!q || !items.some((i) => i.queue === id && i.status === "complete")) continue;
      if (q.open_when_done) void api?.openDir(q.open_when_done).catch(() => say("Could not open " + q.open_when_done));
      if (q.power_when_done) {
        setDialog({ k: "power", kind: q.power_when_done as "shutdown" | "sleep" | "hibernate" });
        if (q.power_when_done === "shutdown") void api?.powerAction("shutdown", q.force_power);
      } else if (q.exit_when_done) {
        void import("@tauri-apps/api/window").then((w) => w.getCurrentWindow().close());
      }
    }
    prevBusy.current = busyQueues;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [busy, items.map((i) => i.queue + i.status).join(",")]);

  const totalSpeed = items.reduce((s, i) => s + (i.status === "downloading" ? i.speed : 0), 0);

  // Actions ----------------------------------------------------------------
  const addUrls = async (text: string): Promise<boolean> => {
    if (!api) return false;
    const urls = text.split(/[\s]+/).map((s) => s.trim()).filter(Boolean);
    let ok = 0;
    for (const u of urls) {
      try {
        // A streaming playlist with several qualities: let the user choose.
        if (/\.(m3u8|mpd)([?#]|$)/i.test(u)) {
          const variants = await api.streamVariants(u).catch(() => []);
          if (variants.length > 1) {
            setDialog({ k: "quality", url: u, variants });
            ok++;
            continue;
          }
        }
        await api.add(u);
        ok++;
      } catch (e) {
        say(String(e));
      }
    }
    return ok > 0;
  };
  const each = (ids: string[], fn: (id: string) => Promise<void>) => ids.forEach((id) => void fn(id));
  const resumeSel = () => api && each([...selected], api.resume);
  const stopSel = () => api && each([...selected], api.pause);
  const askDelete = (ids: string[]) => ids.length && setDialog({ k: "delete", ids });
  const doDelete = (ids: string[], files: boolean) => {
    if (!api) return;
    each(ids, (id) => api.remove(id, files));
    setSelected(new Set());
  };

  const select = (id: string, additive: boolean) =>
    setSelected((s) => {
      if (!additive) return new Set([id]);
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });
  const toggle = (id: string) =>
    setSelected((s) => {
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });
  const toggleAll = () =>
    setSelected((s) => (visible.every((i) => s.has(i.id)) ? new Set() : new Set(visible.map((i) => i.id))));

  const open = (id: string) => {
    const i = items.find((x) => x.id === id);
    if (i?.status === "complete") void api?.openFile(id).catch((e) => say(String(e)));
  };

  const saveSettings = (next: Settings) => {
    setSettings(next);
    void api?.setSettings(next);
  };
  const runProject = async (p: GrabProject) => {
    try {
      const n = await api!.runProject(p.id);
      say(`${p.name}: ${n} new file${n === 1 ? "" : "s"} queued`);
    } catch (e) {
      say(String(e));
    }
    await refreshProjects();
  };

  const saveQueues = (queues: Queue[]) => {
    if (!settings) return;
    const next = { ...settings, queues };
    setSettings(next);
    void api?.setSettings(next);
    if (filter.startsWith("queue:") && !queues.some((q) => `queue:${q.id}` === filter)) setFilter("all");
  };

  // Share ------------------------------------------------------------------
  const shareIds = () => (selected.size ? [...selected] : items.map((i) => i.id));
  const copyLinks = () => {
    const ids = new Set(shareIds());
    const links = items.filter((i) => ids.has(i.id)).map((i) => i.url);
    void navigator.clipboard?.writeText(links.join("\n"));
    say(`Copied ${links.length} link${links.length === 1 ? "" : "s"}`);
  };
  const exportLinks = async () => {
    if (!api) return;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({ defaultPath: "fetchy-links.txt", filters: [{ name: "Text", extensions: ["txt"] }] });
      if (path) say(`Exported ${await api.exportLinks(path, shareIds())} links`);
    } catch (e) {
      say(String(e));
    }
  };
  const importLinks = async () => {
    if (!api) return;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const path = await open({ filters: [{ name: "Text", extensions: ["txt", "list", "csv"] }] });
      if (typeof path === "string") say(`Added ${await api.importLinks(path)} links`);
    } catch (e) {
      say(String(e));
    }
  };

  // Drop links, text or a list file onto the window to download them.
  const [dropping, setDropping] = useState(false);
  useEffect(() => {
    const urlsIn = (text: string) => [...new Set(text.match(/(?:https?|ftp):\/\/[^\s"'<>]+/gi) ?? [])];
    let depth = 0;
    const over = (e: DragEvent) => { e.preventDefault(); };
    const enter = (e: DragEvent) => { e.preventDefault(); depth++; setDropping(true); };
    const leave = () => { depth = Math.max(0, depth - 1); if (!depth) setDropping(false); };
    const drop = async (e: DragEvent) => {
      e.preventDefault();
      depth = 0;
      setDropping(false);
      const dt = e.dataTransfer;
      if (!dt) return;
      let text = dt.getData("text/uri-list") + "\n" + dt.getData("text/plain");
      for (const f of Array.from(dt.files)) {
        if (/\.(txt|csv|list|url|html?|md)$/i.test(f.name) && f.size < 5_000_000) text += "\n" + (await f.text());
      }
      const urls = urlsIn(text);
      if (!urls.length) { say("Nothing to download in what was dropped"); return; }
      for (const u of urls) { try { await api?.add(u); } catch { /* skip */ } }
      say(`Added ${urls.length} link${urls.length === 1 ? "" : "s"}`);
    };
    window.addEventListener("dragover", over);
    window.addEventListener("dragenter", enter);
    window.addEventListener("dragleave", leave);
    window.addEventListener("drop", drop);
    return () => {
      window.removeEventListener("dragover", over);
      window.removeEventListener("dragenter", enter);
      window.removeEventListener("dragleave", leave);
      window.removeEventListener("drop", drop);
    };
  }, [api, say]);

  // A short chime when a download finishes.
  const prevStatus = useRef<Map<string, string>>(new Map());
  useEffect(() => {
    for (const i of items) {
      const was = prevStatus.current.get(i.id);
      if (settings?.sound && was && was !== "complete" && i.status === "complete") playChime();
      prevStatus.current.set(i.id, i.status);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items]);

  // Keyboard shortcuts (ignored while typing in a field).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.tagName === "INPUT" || t.tagName === "TEXTAREA") return;
      if (e.key === "Delete" && selected.size) askDelete([...selected]);
      if ((e.ctrlKey || e.metaKey) && e.key === "a") {
        e.preventDefault();
        setSelected(new Set(visible.map((i) => i.id)));
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "f") {
        e.preventDefault();
        searchRef.current?.focus();
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "v") addRef.current?.focus();
      if ((e.ctrlKey || e.metaKey) && e.key === ",") setView((v) => (v === "settings" ? "overview" : "settings"));
      if (e.key === "Escape") setView((v) => (v === "downloads" ? v : "overview"));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const none = selected.size === 0;
  const ctxItems = (id: string): CtxItem[] => {
    const i = items.find((x) => x.id === id);
    if (!i) return [];
    const done = i.status === "complete";
    const running = i.status === "downloading" || i.status === "queued";
    // Act on the whole selection when the clicked row is part of it.
    const ids = selected.has(id) ? [...selected] : [id];
    return [
      { label: "Open", action: () => open(id), disabled: !done },
      { label: "Open Folder", action: () => void api?.openFolder(id).catch((e) => say(String(e))) },
      { label: "Open with…", action: () => void api?.openWith(id).catch((e) => say(String(e))), disabled: !done },
      { label: "", action: () => {}, separator: true },
      { label: "Resume Download", action: () => api && each(ids, api.resume), disabled: done || running },
      { label: "Stop Download", action: () => api && each(ids, api.pause), disabled: !running },
      { label: "Redownload", action: () => api && each(ids, api.redownload) },
      { label: "", action: () => {}, separator: true },
      ...(settings && settings.queues.length > 1
        ? [
            { label: "", action: () => {}, separator: true },
            ...settings.queues
              .filter((q) => q.id !== i.queue)
              .map((q) => ({ label: `Move to queue: ${q.name}`, action: () => api && each(ids, (x) => api.moveToQueue(x, q.id)) })),
          ]
        : []),
      { label: "", action: () => {}, separator: true },
      { label: "Copy Link", action: () => void navigator.clipboard?.writeText(i.url) },
      { label: "Properties", action: () => setDialog({ k: "props", id }) },
      { label: "", action: () => {}, separator: true },
      { label: ids.length > 1 ? `Delete ${ids.length} items…` : "Delete…", action: () => askDelete(ids) },
    ];
  };

  // A new selection re-opens the details panel if it was closed.
  useEffect(() => setDetailsClosed(false), [selected]);

  // Details panel: shown for a single selected download.
  const detailsItem = selected.size === 1 && !detailsClosed ? items.find((i) => selected.has(i.id)) : undefined;
  const propsItem = dialog?.k === "props" ? items.find((i) => i.id === dialog.id) : undefined;

  return (
    <div className="window">
      <TitleBar
        query={query}
        onQuery={(q) => { setQuery(q); if (view !== "downloads") setView("downloads"); }}
        searchRef={searchRef}
        dark={isDark}
        onToggleTheme={() => settings && saveSettings({ ...settings, theme: isDark ? "light" : "dark" })}
        onSettings={() => (view === "settings" ? setView("overview") : openSettings())}
        settingsOpen={view === "settings"}
      />
      {view === "settings" && settings ? (
        <SettingsPage
          section={section} onSection={setSection} settings={settings} onChange={saveSettings}
          apiInfo={apiInfo} api={api} items={items} projects={projects} whenDone={whenDone} onWhenDone={setWhenDone}
          onBack={() => setView("overview")} onSaveQueues={saveQueues}
          onStartQueue={(id) => { void api?.startQueue(id); say("Queue started"); }}
          onStopQueue={(id) => void api?.stopQueue(id)}
          onMove={(id, d) => void api?.moveInQueue(id, d).then(() => api.list().then((l) => dispatch({ t: "set", items: l })))}
          onSyncNow={() => { void api?.syncNow(); say("Checking synchronized files for changes…"); }}
          onRunProject={runProject}
          onEditProject={(p) => setDialog({ k: "grab", project: p })}
          onDeleteProject={(p) => void api?.deleteProject(p.id).then(refreshProjects)}
          onNewProject={() => setDialog({ k: "grab" })}
          onOpenProject={(p) => void api?.openDir(`${settings.default_dir.replace(/[\\/]+$/, "")}\\${p.name.replace(/[\\/:*?"<>|]/g, "_")}`).catch((e) => say(String(e)))}
          onCheckUpdate={() => void checkUpdate(false)}
        />
      ) : (
      <div className="body">
        {sidebarOpen && (
          <Sidebar items={items} filter={view === "overview" ? ("none" as Filter) : filter} onFilter={(f) => { setFilter(f); setView("downloads"); }} onOverview={() => setView("overview")} overviewOn={view === "overview"} projects={projects} onProjects={() => openSettings("grabber")} onQueues={() => openSettings("queues")} queues={settings?.queues ?? []} />
        )}
        <main className="main">
          <Toolbar
            onAdd={addUrls}
            inputRef={addRef}
            hasSelection={!none}
            onResume={resumeSel}
            onStop={stopSel}
            onStopAll={() => void api?.pauseAll()}
            onDelete={() => askDelete([...selected])}
            onScheduler={() => openSettings("queues")}
            onGrabber={() => setDialog({ k: "grab" })}
            onBatch={() => setDialog({ k: "batch" })}
            onResumeAll={() => void api?.resumeAll()}
            onCopyLinks={copyLinks}
            onExport={exportLinks}
            onImport={importLinks}
          />
          {view === "overview" ? (
            <Overview api={api} items={items} onOpenList={(f) => { setFilter(f); setView("downloads"); }} onProps={(id) => setDialog({ k: "props", id })} />
          ) : (
          <DownloadTable
            items={visible}
            queueNames={Object.fromEntries((settings?.queues ?? []).map((q) => [q.id, q.name]))}
            selected={selected}
            onSelect={select}
            onToggle={toggle}
            onToggleAll={toggleAll}
            onContext={(e, id) => {
              e.preventDefault();
              if (!selected.has(id)) setSelected(new Set([id]));
              setCtx({ x: e.clientX, y: e.clientY, id });
            }}
            onOpen={open}
            sort={sort}
            onSort={(key) => setSort((s) => (s?.key === key ? (s.dir === 1 ? { key, dir: -1 } : null) : { key, dir: 1 }))}
          />
          )}
          {detailsItem && api && (
            <DetailsPanel key={detailsItem.id} item={detailsItem} api={api} onClose={() => setDetailsClosed(true)} />
          )}
          <StatusBar api={api} speed={totalSpeed} />
        </main>
      </div>
      )}

      {ctx && <ContextMenu x={ctx.x} y={ctx.y} items={ctxItems(ctx.id)} onClose={() => setCtx(null)} />}
      {propsItem && <PropertiesDialog item={propsItem} onSaveDescription={(t) => void api?.setDescription(propsItem.id, t)} onVerify={(x) => api!.verifyChecksum(propsItem.id, x)} onClose={() => setDialog(null)} />}
      {dialog?.k === "delete" && (
        <ConfirmDelete count={dialog.ids.length} onConfirm={(f) => doDelete(dialog.ids, f)} onClose={() => setDialog(null)} />
      )}
      {dialog?.k === "quality" && api && (
        <QualityDialog
          url={dialog.url} variants={dialog.variants} onClose={() => setDialog(null)}
          onPick={(v) => void api.addStream(v).catch((e) => say(String(e)))}
        />
      )}
      {dialog?.k === "batch" && api && (
        <BatchDialog
          onClose={() => setDialog(null)}
          onAdd={async (urls, auth) => {
            let ok = 0;
            for (const u of urls) {
              try {
                await api.add(u, auth ? { username: auth.user, password: auth.pass } : undefined);
                ok++;
              } catch { /* skip bad address */ }
            }
            say(`Added ${ok} of ${urls.length} files`);
          }}
        />
      )}
      {dialog?.k === "grab" && settings && api && (
        <GrabberWizard
          queues={settings.queues} defaultDir={settings.default_dir} project={dialog.project}
          onGrab={api.grabSite} onAdd={api.grabAdd}
          onSaveProject={async (p) => { const s = await api.saveProject(p); void refreshProjects(); return s; }}
          onDone={say}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.k === "projects" && api && (
        <ProjectsDialog
          projects={projects}
          onRun={runProject}
          onEdit={(p) => setDialog({ k: "grab", project: p })}
          onDelete={(p) => void api.deleteProject(p.id).then(refreshProjects)}
          onNew={() => setDialog({ k: "grab" })}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.k === "update" && (
        <UpdateDialog
          info={dialog.info} auto={dialog.auto}
          onOpen={() => void import("@tauri-apps/plugin-opener").then((o) => o.openUrl(dialog.info.url))}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.k === "power" && (
        <PowerDialog api={api} kind={dialog.kind} onClose={() => setDialog(null)} />
      )}
      {dropping && <div className="drop-hint">Drop links or a list file to download</div>}
      {toast && <div className="toast" onClick={() => setToast(null)}>{toast}</div>}
    </div>
  );
}

function PowerDialog({ api, kind, onClose }: { api: Api | null; kind: "shutdown" | "sleep" | "hibernate"; onClose: () => void }) {
  const [left, setLeft] = useState(kind === "shutdown" ? 60 : 10);
  useEffect(() => {
    const t = window.setInterval(() => setLeft((n) => Math.max(0, n - 1)), 1000);
    return () => window.clearInterval(t);
  }, []);
  useEffect(() => {
    if (left === 0 && kind !== "shutdown") {
      void api?.powerAction(kind);
      onClose();
    }
  }, [left]); // eslint-disable-line react-hooks/exhaustive-deps
  const cancel = () => {
    if (kind === "shutdown") void api?.powerAction("cancel");
    onClose();
  };
  return (
    <Modal title="Downloads finished" onClose={cancel}>
      <p className="msg">
        The computer will {kind === "shutdown" ? "shut down" : kind} in {left} seconds.
      </p>
      <div className="modal-foot">
        <button className="btn primary" autoFocus onClick={cancel}>Cancel</button>
      </div>
    </Modal>
  );
}

function playChime() {
  try {
    const Ctx = window.AudioContext ?? (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    const ctx = new Ctx();
    [660, 880].forEach((f, i) => {
      const o = ctx.createOscillator();
      const g = ctx.createGain();
      o.frequency.value = f;
      o.connect(g);
      g.connect(ctx.destination);
      const t = ctx.currentTime + i * 0.14;
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(0.18, t + 0.02);
      g.gain.exponentialRampToValueAtTime(0.0001, t + 0.35);
      o.start(t);
      o.stop(t + 0.4);
    });
    setTimeout(() => void ctx.close(), 900);
  } catch { /* no audio available */ }
}
