import { useEffect, useMemo, useState } from "react";
import { Activity, AlertTriangle, CheckCircle2, FolderOpen, LayoutGrid, List, CloudDownload } from "lucide-react";
import { AppWindow, FileArchive, FileText, File as FileIcon, Image as ImageIcon, Music, Smartphone, Video } from "lucide-react";
import type { Api } from "../api";
import type { Category, DayStat, DownloadInfo } from "../types";
import { categoryLabel, formatBytes, formatDate, formatSpeed, percent } from "../format";

const icons: Record<Category, React.ReactNode> = {
  music: <Music size={20} />,
  compressed: <FileArchive size={20} />,
  video: <Video size={20} />,
  program: <AppWindow size={20} />,
  document: <FileText size={20} />,
  apk: <Smartphone size={20} />,
  image: <ImageIcon size={20} />,
  other: <FileIcon size={20} />,
};

interface Props {
  api: Api | null;
  items: DownloadInfo[];
  onOpenList: (f: "unfinished" | "finished" | "all") => void;
  onProps: (id: string) => void;
}

const stamp = (i: DownloadInfo) => i.completed_at ?? i.last_try_at ?? i.added_at;

function statusText(i: DownloadInfo): string {
  switch (i.status) {
    case "downloading": return `${percent(i)}% · ${formatSpeed(i.speed)}`;
    case "complete": return formatDate(i.completed_at ?? i.added_at);
    case "error": return "Failed";
    case "paused": return `Paused · ${percent(i)}%`;
    default: return "Queued";
  }
}

export function Overview({ api, items, onOpenList, onProps }: Props) {
  const [mode, setMode] = useState<"list" | "grid">(() => {
    try { return localStorage.getItem("fetchy.overview") === "grid" ? "grid" : "list"; } catch { return "list"; }
  });
  const [stats, setStats] = useState<DayStat[]>([]);
  useEffect(() => { void api?.getStats().then(setStats).catch(() => {}); }, [api, items.length]);
  const setViewMode = (m: "list" | "grid") => {
    setMode(m);
    try { localStorage.setItem("fetchy.overview", m); } catch { /* ignore */ }
  };

  const active = items.filter((i) => i.status === "downloading");
  const done = items.filter((i) => i.status === "complete");
  const attention = items.filter((i) => i.status === "error" || i.status === "paused" || i.status === "queued");
  const failed = items.filter((i) => i.status === "error").length;
  const speed = active.reduce((a, i) => a + i.speed, 0);
  const today = new Date();
  const key = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
  const todayBytes = stats.find((s) => s.date === key)?.bytes ?? 0;
  const weekBytes = stats.slice(-7).reduce((a, s) => a + s.bytes, 0);
  const doneBytes = done.reduce((a, i) => a + (i.size ?? i.downloaded), 0);

  const recent = useMemo(() => [...items].sort((a, b) => stamp(b) - stamp(a)).slice(0, 24), [items]);
  const open = (i: DownloadInfo) => (i.status === "complete" ? void api?.openFile(i.id) : onProps(i.id));

  return (
    <div className="overview">
      <div className="ov-cards">
        <button className="ov-card" onClick={() => onOpenList("unfinished")}>
          <span className="ov-ic"><Activity size={18} /></span>
          <b>{active.length}</b>
          <span className="ov-label">Downloading now</span>
          <em>{active.length ? formatSpeed(speed) : "Idle"}</em>
        </button>
        <button className="ov-card" onClick={() => onOpenList("finished")}>
          <span className="ov-ic"><CheckCircle2 size={18} /></span>
          <b>{done.length}</b>
          <span className="ov-label">Completed</span>
          <em>{formatBytes(doneBytes) || "0 B"} in total</em>
        </button>
        <button className="ov-card" onClick={() => onOpenList("all")}>
          <span className="ov-ic"><CloudDownload size={18} /></span>
          <b>{formatBytes(todayBytes) || "0 B"}</b>
          <span className="ov-label">Downloaded today</span>
          <em>{formatBytes(weekBytes) || "0 B"} this week</em>
        </button>
        <button className={failed ? "ov-card warn" : "ov-card"} onClick={() => onOpenList("unfinished")}>
          <span className="ov-ic"><AlertTriangle size={18} /></span>
          <b>{attention.length}</b>
          <span className="ov-label">Needs attention</span>
          <em>{failed ? `${failed} failed` : "Queued, paused or failed"}</em>
        </button>
      </div>

      <div className="ov-head">
        <h3>Recent downloads</h3>
        <div className="layout-toggle" role="group" aria-label="Layout">
          <button className={mode === "list" ? "on" : ""} onClick={() => setViewMode("list")} title="List view"><List size={15} /></button>
          <button className={mode === "grid" ? "on" : ""} onClick={() => setViewMode("grid")} title="Grid view"><LayoutGrid size={15} /></button>
        </div>
      </div>

      {recent.length === 0 ? (
        <div className="ov-empty">Nothing here yet. Paste a link in the box above, or drop one onto the window.</div>
      ) : mode === "list" ? (
        <div className="ov-list">
          {recent.map((i) => (
            <div key={i.id} className="ov-row" onDoubleClick={() => open(i)}>
              <span className="ficon">{icons[i.category]}</span>
              <span className="ov-name" title={i.filename}>{i.filename}</span>
              <span className="ov-meta">{categoryLabel[i.category]}</span>
              <span className="ov-meta">{formatBytes(i.size ?? i.downloaded)}</span>
              <span className={`ov-meta st-${i.status}`}>{statusText(i)}</span>
              {i.status === "complete" ? (
                <button className="icon-btn" title="Show in folder" onClick={() => void api?.openFolder(i.id)}><FolderOpen size={15} /></button>
              ) : <span />}
            </div>
          ))}
        </div>
      ) : (
        <div className="ov-grid">
          {recent.map((i) => (
            <div key={i.id} className="ov-tile" onDoubleClick={() => open(i)}>
              <span className="ov-big">{icons[i.category]}</span>
              <span className="ov-name" title={i.filename}>{i.filename}</span>
              <span className="ov-meta">{formatBytes(i.size ?? i.downloaded)} · {categoryLabel[i.category]}</span>
              {i.status === "complete" || i.status === "error" ? (
                <span className={`ov-meta st-${i.status}`}>{statusText(i)}</span>
              ) : (
                <>
                  <span className="ov-bar"><span style={{ width: `${percent(i)}%` }} /></span>
                  <span className={`ov-meta st-${i.status}`}>{statusText(i)}</span>
                </>
              )}
              {i.status === "complete" && (
                <button className="icon-btn ov-open" title="Show in folder" onClick={() => void api?.openFolder(i.id)}><FolderOpen size={15} /></button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
