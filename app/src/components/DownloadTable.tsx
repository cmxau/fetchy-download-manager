import { useState } from "react";
import { AppWindow, FileArchive, FileText, File as FileIcon, Image as ImageIcon, Music, Smartphone, Video } from "lucide-react";
import type { Category, DownloadInfo } from "../types";
import { categoryLabel, formatBytes, formatDate, formatEta, percent } from "../format";
import { ContextMenu } from "./ContextMenu";

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

export type SortKey =
  | "name" | "queue" | "size" | "status" | "eta" | "speed" | "lasttry" | "description" | "added" | "saveto" | "referer" | "parent";

type ColId = SortKey;

interface Col {
  id: ColId;
  label: string;
  /** Visible the first time the app runs. */
  on: boolean;
  /** Longest the column may grow before its text is cut with an ellipsis. */
  max?: number;
  locked?: boolean;
}

const COLUMNS: Col[] = [
  { id: "name", label: "File Name", on: true, locked: true, max: 420 },
  { id: "queue", label: "Q", on: true, max: 120 },
  { id: "size", label: "Size", on: true },
  { id: "status", label: "Status", on: true, max: 220 },
  { id: "eta", label: "Time Left", on: true },
  { id: "speed", label: "Transfer Rate", on: true },
  { id: "lasttry", label: "Last Try Date", on: true },
  { id: "description", label: "Description", on: true, max: 260 },
  { id: "added", label: "Date Added", on: false },
  { id: "saveto", label: "Save To", on: false, max: 300 },
  { id: "referer", label: "Referer", on: false, max: 300 },
  { id: "parent", label: "Parent Web Page", on: false, max: 300 },
];

interface Layout {
  order: ColId[];
  hidden: ColId[];
}

const STORE = "fetchy.columns";

function loadLayout(): Layout {
  const fresh: Layout = { order: COLUMNS.map((c) => c.id), hidden: COLUMNS.filter((c) => !c.on).map((c) => c.id) };
  try {
    const raw = JSON.parse(localStorage.getItem(STORE) ?? "null") as Layout | null;
    if (!raw) return fresh;
    // Columns added in later versions slot in at the end with their default visibility.
    const known = new Set(raw.order);
    const order = [...raw.order.filter((id) => COLUMNS.some((c) => c.id === id)), ...COLUMNS.filter((c) => !known.has(c.id)).map((c) => c.id)];
    const hidden = [...raw.hidden, ...COLUMNS.filter((c) => !known.has(c.id) && !c.on).map((c) => c.id)];
    return { order, hidden };
  } catch {
    return fresh;
  }
}

interface Props {
  items: DownloadInfo[];
  queueNames: Record<string, string>;
  selected: Set<string>;
  onSelect: (id: string, additive: boolean) => void;
  onToggle: (id: string) => void;
  onToggleAll: () => void;
  onContext: (e: React.MouseEvent, id: string) => void;
  onOpen: (id: string) => void;
  sort: { key: SortKey; dir: 1 | -1 } | null;
  onSort: (k: SortKey) => void;
}

function Check({ on, onChange }: { on: boolean; onChange: () => void }) {
  return (
    <button
      role="checkbox"
      aria-checked={on}
      className={on ? "check on" : "check"}
      onClick={(e) => {
        e.stopPropagation();
        onChange();
      }}
    >
      {on && (
        <svg viewBox="0 0 12 12" width="11" height="11">
          <path d="M2.5 6.5l2.3 2.3L9.5 3.8" stroke="#fff" strokeWidth="1.8" fill="none" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      )}
    </button>
  );
}

function StatusCell({ i }: { i: DownloadInfo }) {
  const pct = percent(i);
  switch (i.status) {
    case "complete":
      return <span>Complete</span>;
    case "error":
      return <span className="st-error" title={i.error ?? ""}>Error — {i.error}</span>;
    case "queued":
      return <span className="muted">Queued</span>;
    default:
      return (
        <div className="st-prog">
          <span>{i.status === "paused" ? `Paused ${pct}%` : `${pct}%`}</span>
          <div className="bar">
            <div className={i.status === "paused" ? "fill paused" : "fill"} style={{ width: `${pct}%` }} />
          </div>
        </div>
      );
  }
}

function fullDate(ts: number | null): string {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  const p = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}/${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function Text({ v }: { v: string | null | undefined }) {
  return <span className="ellip" title={v ?? ""}>{v ?? ""}</span>;
}

export function DownloadTable(p: Props) {
  const [layout, setLayout] = useState<Layout>(loadLayout);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [dragging, setDragging] = useState<ColId | null>(null);
  const [over, setOver] = useState<ColId | null>(null);

  const save = (l: Layout) => {
    setLayout(l);
    try { localStorage.setItem(STORE, JSON.stringify(l)); } catch { /* storage unavailable */ }
  };
  const visible = layout.order.filter((id) => !layout.hidden.includes(id)).map((id) => COLUMNS.find((c) => c.id === id)!);
  const all = p.items.length > 0 && p.items.every((i) => p.selected.has(i.id));

  const move = (from: ColId, to: ColId) => {
    if (from === to) return;
    const order = layout.order.filter((id) => id !== from);
    order.splice(order.indexOf(to), 0, from);
    save({ ...layout, order });
  };

  const cell = (id: ColId, i: DownloadInfo): React.ReactNode => {
    switch (id) {
      case "name":
        return (
          <div className="name">
            <span className="ficon">{icons[i.category]}</span>
            <div className="nm">
              <div className="fn" title={i.filename}>{i.filename}</div>
              <small>{categoryLabel[i.category]}{i.status === "downloading" && i.active_connections > 0 && ` · ${i.active_connections} conn`}</small>
            </div>
          </div>
        );
      case "queue": return <Text v={p.queueNames[i.queue] ?? i.queue} />;
      case "size": return <span>{formatBytes(i.size)}</span>;
      case "status": return <StatusCell i={i} />;
      case "eta": return <span>{formatEta(i.eta_secs, i.status === "downloading")}</span>;
      case "speed": return <span>{i.status === "downloading" && i.speed > 0 ? `${formatBytes(i.speed)}/s` : ""}</span>;
      case "lasttry": return <span>{formatDate(i.last_try_at)}</span>;
      case "description": return <Text v={i.description} />;
      case "added": return <span title={fullDate(i.added_at)}>{formatDate(i.added_at)}</span>;
      case "saveto": return <Text v={i.dir} />;
      case "referer": return <Text v={i.referer} />;
      case "parent": return <Text v={i.parent} />;
    }
  };

  // Every column except the name takes the width of its content; the name takes what is left.
  const template = ["22px", ...visible.map((c) => (c.id === "name" ? "minmax(220px, 1fr)" : "max-content"))].join(" ");

  return (
    <div className="dtable" style={{ gridTemplateColumns: template }}>
      <div
        className="row head"
        onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY }); }}
      >
        <Check on={all} onChange={p.onToggleAll} />
        {visible.map((c) => {
          const on = p.sort?.key === c.id;
          return (
            <button
              key={c.id}
              className={`th${on ? " on" : ""}${over === c.id && dragging !== c.id ? " drop" : ""}`}
              draggable
              onDragStart={() => setDragging(c.id)}
              onDragOver={(e) => { e.preventDefault(); setOver(c.id); }}
              onDragLeave={() => setOver(null)}
              onDrop={() => { if (dragging) move(dragging, c.id); setDragging(null); setOver(null); }}
              onDragEnd={() => { setDragging(null); setOver(null); }}
              onClick={() => p.onSort(c.id)}
              title="Drag to reorder · right-click for more columns"
            >
              {c.label}
              <svg width="8" height="12" viewBox="0 0 8 12" aria-hidden>
                <path d="M4 1 1.5 4.5h5z" fill="currentColor" opacity={on && p.sort?.dir === 1 ? 1 : 0.45} />
                <path d="M4 11 1.5 7.5h5z" fill="currentColor" opacity={on && p.sort?.dir === -1 ? 1 : 0.45} />
              </svg>
            </button>
          );
        })}
      </div>

      {p.items.length === 0 && (
        <div className="empty">
          <div>No downloads here yet</div>
          <small>Paste a link in “Add Url” above to start.</small>
        </div>
      )}
      {p.items.map((i) => {
        const sel = p.selected.has(i.id);
        return (
          <div
            key={i.id}
            className={sel ? "row item sel" : "row item"}
            onClick={(e) => p.onSelect(i.id, e.ctrlKey || e.metaKey)}
            onDoubleClick={() => p.onOpen(i.id)}
            onContextMenu={(e) => p.onContext(e, i.id)}
          >
            <Check on={sel} onChange={() => p.onToggle(i.id)} />
            {visible.map((c) => <div key={c.id} className={`cell c-${c.id}`} style={c.id === "name" ? undefined : { maxWidth: c.max }}>{cell(c.id, i)}</div>)}
          </div>
        );
      })}

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            ...layout.order.map((id) => {
              const c = COLUMNS.find((x) => x.id === id)!;
              const shown = !layout.hidden.includes(id);
              return {
                label: `${shown ? "✓" : "  "}  ${c.label}`,
                disabled: c.locked,
                action: () => save({ ...layout, hidden: shown ? [...layout.hidden, id] : layout.hidden.filter((x) => x !== id) }),
              };
            }),
            { label: "", action: () => {}, separator: true },
            { label: "Reset columns", action: () => save({ order: COLUMNS.map((c) => c.id), hidden: COLUMNS.filter((c) => !c.on).map((c) => c.id) }) },
          ]}
        />
      )}
    </div>
  );
}
