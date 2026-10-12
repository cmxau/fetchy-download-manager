import { useEffect, useRef, useState } from "react";
import { ArrowDownToLine } from "lucide-react";
import type { Api } from "../api";
import type { DayStat } from "../types";
import { formatBytes, formatSpeed } from "../format";

const POINTS = 60;

function Spark({ values }: { values: number[] }) {
  const max = Math.max(1, ...values);
  const w = 120, h = 24;
  const pts = values.map((v, i) => `${(i / (POINTS - 1)) * w},${h - (v / max) * (h - 3) - 1.5}`);
  return (
    <svg className="foot-spark" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" aria-label="Speed, last minute">
      <polyline points={pts.join(" ")} fill="none" stroke="#22c55e" strokeWidth="1.5" />
      <polygon points={`0,${h} ${pts.join(" ")} ${w},${h}`} fill="#22c55e" opacity="0.15" />
    </svg>
  );
}

export function StatusBar({ api, speed }: { api: Api | null; speed: number }) {
  const [hist, setHist] = useState<number[]>(() => Array(POINTS).fill(0));
  const [stats, setStats] = useState<DayStat[]>([]);
  const [open, setOpen] = useState(false);
  const latest = useRef(speed);
  latest.current = speed;

  useEffect(() => {
    const t = window.setInterval(() => setHist((h) => [...h.slice(1), latest.current]), 1000);
    return () => window.clearInterval(t);
  }, []);
  useEffect(() => {
    if (!api) return;
    const load = () => api.getStats().then(setStats).catch(() => {});
    load();
    const t = window.setInterval(load, 8000);
    return () => window.clearInterval(t);
  }, [api]);

  const todayKey = new Date().toLocaleDateString("en-CA");
  const today = stats.find((s) => s.date === todayKey)?.bytes ?? 0;
  const last7 = [...Array(7)].map((_, i) => {
    const d = new Date(Date.now() - (6 - i) * 86400_000);
    const key = d.toLocaleDateString("en-CA");
    return { key, label: d.toLocaleDateString(undefined, { weekday: "short" }), bytes: stats.find((s) => s.date === key)?.bytes ?? 0 };
  });
  const peak = Math.max(1, ...last7.map((d) => d.bytes));
  const total7 = last7.reduce((a, d) => a + d.bytes, 0);

  return (
    <footer className="footer">
      <button className="today" onClick={() => setOpen(!open)} title="Data downloaded">
        Today <b>{formatBytes(today)}</b>
      </button>
      {open && (
        <div className="today-pop" onMouseLeave={() => setOpen(false)}>
          <div className="pop-title">Last 7 days · {formatBytes(total7)}</div>
          <div className="bars">
            {last7.map((d) => (
              <div key={d.key} className="bar-col" title={`${d.key}: ${formatBytes(d.bytes)}`}>
                <div className="bar-fill" style={{ height: `${Math.max(3, (d.bytes / peak) * 100)}%` }} />
                <span>{d.label}</span>
              </div>
            ))}
          </div>
        </div>
      )}
      <div className="spacer" />
      <Spark values={hist} />
      <div className="speed">
        <ArrowDownToLine size={18} className="dl" />
        <b>{formatSpeed(speed)}</b>
        <span className="unit">MB/s</span>
      </div>
    </footer>
  );
}
