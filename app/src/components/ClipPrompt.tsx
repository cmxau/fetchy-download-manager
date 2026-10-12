import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { getApi } from "../api";
import type { LinkInfo, Settings } from "../types";
import { categoryLabel, formatBytes } from "../format";

declare global {
  interface Window {
    __CLIP__?: LinkInfo;
  }
}

const folder: Record<string, string> = {
  music: "Music", compressed: "Compressed", video: "Videos", program: "Programs",
  document: "Documents", apk: "APKs", image: "Images", other: "Other",
};

async function closeWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().close();
}

/** The small popup shown when a downloadable link is copied. */
export function ClipPrompt() {
  const info = window.__CLIP__;
  const [settings, setSettings] = useState<Settings | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [hover, setHover] = useState(false);

  useEffect(() => {
    getApi().then((a) => a.getSettings().then(setSettings)).catch(() => {});
  }, []);

  // Go away on its own so it never lingers, but not while the pointer is on it.
  useEffect(() => {
    if (hover) return;
    const t = window.setTimeout(() => void closeWindow(), 14000);
    return () => window.clearTimeout(t);
  }, [hover]);

  if (!info) return null;

  const dest = settings
    ? settings.organize ? `${settings.default_dir}\\${folder[info.category]}` : settings.default_dir
    : "";

  const download = async () => {
    setBusy(true);
    try {
      await (await getApi()).add(info.url);
      await closeWindow();
    } catch (e) {
      setErr(String(e));
      setBusy(false);
    }
  };

  return (
    <div className="clip" onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}>
      <div className="clip-head">
        <span className="clip-badge">Fetchy</span>
        <span className="clip-title">{info.stream ? "Video stream copied" : "Download this link?"}</span>
        <button className="icon-btn" onClick={() => void closeWindow()} aria-label="Dismiss"><X size={16} /></button>
      </div>
      <div className="clip-file">
        <b title={info.filename}>{info.filename}</b>
        <span>
          {categoryLabel[info.category]}
          {info.size != null ? ` · ${formatBytes(info.size)}` : info.stream ? " · HLS stream" : ""}
          {info.resumable ? " · resumable" : ""}
        </span>
        {dest && <small title={dest}>Saves to {dest}</small>}
      </div>
      {err && <div className="clip-err">{err}</div>}
      <div className="clip-actions">
        <button className="btn" onClick={() => void closeWindow()}>Not now</button>
        <button className="btn primary" autoFocus disabled={busy} onClick={download}>Download</button>
      </div>
    </div>
  );
}
