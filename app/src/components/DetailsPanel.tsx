import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import type { Api } from "../api";
import type { Details, DownloadInfo } from "../types";
import { categoryLabel, explainError, formatBytes, formatEta, percent } from "../format";

const HISTORY = 60; // samples (≈ 30 s at 2 Hz)

function Sparkline({ values: raw }: { values: number[] }) {
  // Pad on the left so the graph scrolls in from the right instead of stretching.
  const values = [...Array(Math.max(0, HISTORY - raw.length)).fill(0), ...raw];
  const max = Math.max(1, ...values);
  const w = 240, h = 46;
  const pts = values.map((v, i) => `${(i / (HISTORY - 1)) * w},${h - (v / max) * (h - 4) - 2}`);
  return (
    <svg className="spark" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" aria-label="Speed history">
      <defs>
        <linearGradient id="sg" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#22c55e" stopOpacity="0.45" />
          <stop offset="1" stopColor="#22c55e" stopOpacity="0" />
        </linearGradient>
      </defs>
      {pts.length > 1 && (
        <>
          <polygon points={`0,${h} ${pts.join(" ")} ${((values.length - 1) / (HISTORY - 1)) * w},${h}`} fill="url(#sg)" />
          <polyline points={pts.join(" ")} fill="none" stroke="#4ade80" strokeWidth="1.6" />
        </>
      )}
    </svg>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="stat">
      <span>{label}</span>
      <b>{value}</b>
    </div>
  );
}

const statusText: Record<DownloadInfo["status"], string> = {
  queued: "Queued",
  downloading: "Receiving data…",
  paused: "Paused",
  complete: "Complete",
  error: "Error",
};

export function DetailsPanel({ item, api, onClose }: { item: DownloadInfo; api: Api; onClose: () => void }) {
  const [details, setDetails] = useState<Details | null>(null);
  const [history, setHistory] = useState<number[]>([]);
  const live = item.status === "downloading";
  const latest = useRef(item);
  latest.current = item;

  // Per-connection data comes from the engine; poll while it can change.
  useEffect(() => {
    let dead = false;
    const load = () => api.details(item.id).then((d) => !dead && setDetails(d)).catch(() => {});
    load();
    if (item.status === "complete") return () => { dead = true; };
    const t = window.setInterval(load, 500);
    return () => {
      dead = true;
      window.clearInterval(t);
    };
  }, [api, item.id, item.status]);

  // Speed history for the graph.
  useEffect(() => {
    const t = window.setInterval(() => {
      setHistory((h) => [...h, latest.current.status === "downloading" ? latest.current.speed : 0].slice(-HISTORY));
    }, 500);
    return () => window.clearInterval(t);
  }, []);

  const pct = percent(item);
  const size = item.size;
  const segs = details?.segments ?? [];
  const active = segs.filter((s) => s.active);
  const isHls = !!details?.hls;
  const avg = history.filter((v) => v > 0);
  const peak = Math.max(0, ...history);

  return (
    <section className="details">
      <header>
        <div className="d-title">
          <b title={item.filename}>{item.filename}</b>
          <small title={item.url}>{item.url}</small>
        </div>
        <button className="icon-btn" onClick={onClose} aria-label="Close details"><X size={18} /></button>
      </header>

      <div className="d-body">
        <div className="d-left">
          <div className="d-progress">
            <div className="bar big"><div className={item.status === "paused" ? "fill paused" : "fill"} style={{ width: `${pct}%` }} /></div>
            <span>{pct}%</span>
          </div>

          {item.status === "error" && explainError(item.error) && <div className="hint">{explainError(item.error)}</div>}
          <div className="stats">
            <Stat label="Status" value={<span className={item.status === "error" ? "st-error" : ""}>{item.status === "error" ? item.error : statusText[item.status]}</span>} />
            <Stat label="File size" value={size != null ? `${formatBytes(size)}${isHls ? " (estimated)" : ""}` : "Unknown"} />
            <Stat label="Downloaded" value={`${formatBytes(item.downloaded)}${size ? ` (${pct}%)` : ""}`} />
            <Stat label="Transfer rate" value={live ? `${formatBytes(item.speed)}/s` : "—"} />
            <Stat label="Time left" value={formatEta(item.eta_secs, live)} />
            <Stat label="Resume capability" value={item.resumable ? "Yes" : "No"} />
            <Stat label="Connections" value={live ? `${item.active_connections} active of ${item.connections}` : `${item.connections} max`} />
            <Stat label="Type" value={categoryLabel[item.category]} />
            <Stat label="Save to" value={<span className="path" title={`${item.dir}`}>{item.dir}</span>} />
            {isHls && details?.hls && <Stat label="Segments" value={`${details.hls.next} / ${details.hls.total}`} />}
          </div>
        </div>

        <div className="d-right">
          <div className="graph-head">
            <span>Speed</span>
            <em>{avg.length ? `avg ${formatBytes(avg.reduce((a, b) => a + b, 0) / avg.length)}/s · peak ${formatBytes(peak)}/s` : "idle"}</em>
          </div>
          <Sparkline values={history} />

          {!isHls && size ? (
            <>
              <div className="graph-head">
                <span>Segments</span>
                <em>{segs.length} split{segs.length === 1 ? "" : "s"} · {active.length} active</em>
              </div>
              <div className="segmap" role="img" aria-label="Download segments">
                {segs.map((s, i) => {
                  const total = Math.max(1, s.end - s.start);
                  return (
                    <div
                      key={i}
                      className={s.active ? "seg active" : "seg"}
                      style={{ left: `${(s.start / size) * 100}%`, width: `${(total / size) * 100}%` }}
                      title={`#${i + 1}: ${formatBytes(s.start)} – ${formatBytes(s.end)}`}
                    >
                      <i style={{ width: `${((s.pos - s.start) / total) * 100}%`, background: `linear-gradient(90deg, hsl(142 72% ${24 + ((i * 9) % 22)}%), hsl(142 72% ${42 + ((i * 9) % 24)}%))` }} />
                    </div>
                  );
                })}
              </div>
              <div className="conns">
                <div className="crow chead"><span>#</span><span>Range</span><span>Done</span><span>Left</span><span>State</span></div>
                {segs.map((s, i) => (
                  <div className="crow" key={i}>
                    <span>{i + 1}</span>
                    <span>{formatBytes(s.start)} – {formatBytes(s.end)}</span>
                    <span>{formatBytes(s.pos - s.start)}</span>
                    <span>{formatBytes(Math.max(0, s.end - s.pos))}</span>
                    <span className={s.active ? "on" : ""}>{s.pos >= s.end ? "Done" : s.active ? "Receiving" : "Waiting"}</span>
                  </div>
                ))}
                {segs.length === 0 && <div className="crow none">No connection data yet.</div>}
              </div>
            </>
          ) : null}
        </div>
      </div>
    </section>
  );
}
