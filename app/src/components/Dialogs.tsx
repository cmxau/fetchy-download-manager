import { useEffect, useState } from "react";
import { X } from "lucide-react";
import type { DownloadInfo, GrabProject, UpdateInfo } from "../types";
import { explainError, formatBytes, formatDate } from "../format";
import type { ChecksumResult } from "../types";

export function Modal({ title, onClose, children, wide }: { title: string; onClose: () => void; children: React.ReactNode; wide?: boolean }) {
  return (
    <div className="scrim" onMouseDown={onClose}>
      <div className={wide ? "modal wide" : "modal"} onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label={title}>
        <div className="modal-head">
          <h3>{title}</h3>
          <button className="icon-btn" onClick={onClose} aria-label="Close"><X size={18} /></button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function PropertiesDialog({ item, onSaveDescription, onVerify, onClose }: {
  item: DownloadInfo;
  onSaveDescription: (text: string) => void;
  onVerify: (expected: string) => Promise<ChecksumResult>;
  onClose: () => void;
}) {
  const [desc, setDesc] = useState(item.description);
  const [expected, setExpected] = useState("");
  const [result, setResult] = useState<ChecksumResult | string | null>(null);
  const [busy, setBusy] = useState(false);
  const verify = async () => {
    setBusy(true);
    setResult(null);
    try {
      setResult(await onVerify(expected));
    } catch (e) {
      setResult(String(e));
    } finally {
      setBusy(false);
    }
  };
  const hint = item.status === "error" ? explainError(item.error) : null;
  const rows: [string, string][] = [
    ["File name", item.filename],
    ["Saved in", item.dir],
    ["URL", item.url],
    ["Size", item.size != null ? `${formatBytes(item.size)} (${item.size.toLocaleString()} bytes)` : "Unknown"],
    ["Downloaded", formatBytes(item.downloaded)],
    ["Status", item.status],
    ["Resumable", item.resumable ? "Yes" : "No"],
    ["Connections", String(item.connections)],
    ["Added", formatDate(item.added_at)],
    ["Referer", item.referer ?? ""],
    ["Parent page", item.parent ?? ""],
    ...(item.verified != null ? [["Checksum", item.verified ? "Matches the expected checksum" : "DOES NOT match the expected checksum"] as [string, string]] : []),
    ...(item.scan_status ? [["Virus scan", { clean: "Clean (Windows Defender)", threat: "A threat was found - do not open this file", unavailable: "Scan was not available" }[item.scan_status] ?? item.scan_status] as [string, string]] : []),
  ];
  return (
    <Modal title="Properties" onClose={onClose} wide>
      <dl className="props">
        {rows.map(([k, v]) => (
          <div key={k}><dt>{k}</dt><dd title={v}>{v}</dd></div>
        ))}
      </dl>
      {hint && <div className="hint">{hint}</div>}
      {item.status === "complete" && (
        <div className="form verify">
          <span>Verify the file</span>
          <div className="inline">
            <input value={expected} placeholder="Paste the expected MD5 / SHA-1 / SHA-256 / SHA-512" spellCheck={false} onChange={(e) => setExpected(e.target.value)} />
            <button className="btn" disabled={busy || !expected.trim()} onClick={verify}>{busy ? "Checking…" : "Verify"}</button>
          </div>
          {typeof result === "string" && <small className="st-error">{result}</small>}
          {result && typeof result !== "string" && (
            <small className={result.ok ? "ok-text" : "st-error"}>
              {result.ok ? `Match: the ${result.algo.toUpperCase()} is correct.` : `No match. The file's ${result.algo.toUpperCase()} is ${result.actual}`}
            </small>
          )}
        </div>
      )}
      <label className="form">
        <span>Description</span>
        <input value={desc} placeholder="Add a note about this download" onChange={(e) => setDesc(e.target.value)} onBlur={() => desc !== item.description && onSaveDescription(desc)} />
      </label>
    </Modal>
  );
}

export function ConfirmDelete({ count, onConfirm, onClose }: { count: number; onConfirm: (deleteFiles: boolean) => void; onClose: () => void }) {
  const [files, setFiles] = useState(true);
  return (
    <Modal title="Delete" onClose={onClose}>
      <p className="msg">Remove {count} download{count > 1 ? "s" : ""} from the list?</p>
      <label className="checkline">
        <input type="checkbox" checked={files} onChange={(e) => setFiles(e.target.checked)} />
        Also delete the downloaded files from disk (sent to the Recycle Bin / Trash)
      </label>
      <div className="modal-foot">
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn danger" autoFocus onClick={() => { onConfirm(files); onClose(); }}>Delete</button>
      </div>
    </Modal>
  );
}

export function UpdateDialog({ info, auto, onOpen, onClose }: { info: UpdateInfo; auto?: boolean; onOpen: () => void; onClose: () => void }) {
  const [status, setStatus] = useState("");
  const [current, setCurrent] = useState("");
  useEffect(() => { void import("@tauri-apps/api/app").then((a) => a.getVersion()).then(setCurrent).catch(() => {}); }, []);
  const install = async () => {
    setStatus("Downloading…");
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const u = await check();
      if (!u) {
        setStatus("This release has no installer for your system yet. Opening the download page…");
        onOpen();
        return;
      }
      let total = 0, got = 0;
      await u.downloadAndInstall((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? 0;
        if (ev.event === "Progress") {
          got += ev.data.chunkLength;
          setStatus(`Downloading… ${total ? Math.round((got / total) * 100) : 0}%`);
        }
        if (ev.event === "Finished") setStatus("Installing…");
      });
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      setStatus(`Could not install automatically (${String(e)}). You can download it from the release page.`);
    }
  };
  useEffect(() => { if (auto) void install(); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <Modal title="Update available" onClose={onClose} wide>
      <p className="msg">Fetchy <b>{info.version}</b> is available {current && `(you have ${current})`}.</p>
      {info.notes && <pre className="notes">{info.notes}</pre>}
      {status && <p className="msg">{status}</p>}
      <div className="modal-foot">
        <button className="btn" onClick={onClose}>Later</button>
        <button className="btn" onClick={() => { onOpen(); onClose(); }}>Release page</button>
        <button className="btn primary" disabled={!!status && !status.startsWith("Could not")} onClick={install}>Install and restart</button>
      </div>
    </Modal>
  );
}

export function ProjectsList({ projects, onRun, onEdit, onDelete, onOpen }: {
  onOpen?: (p: GrabProject) => void;
  projects: GrabProject[];
  onRun: (p: GrabProject) => Promise<void>;
  onEdit: (p: GrabProject) => void;
  onDelete: (p: GrabProject) => void;
}) {
  const [running, setRunning] = useState<string | null>(null);
  const every = (m: number) => (m === 0 ? "manual" : m < 60 ? `every ${m} min` : m < 1440 ? `every ${m / 60} h` : "daily");
  return (
    <>
      {projects.length === 0 && <p className="msg">No projects yet. A project remembers a site and its filters, so you can re-run it later (or on a timer) and only new files are downloaded.</p>}
      <div className="projects">
        {projects.map((p) => (
          <div className="project" key={p.id}>
            <div className="p-main">
              <b>{p.name}</b>
              <small title={p.url}>{p.url}</small>
              <small>
                {every(p.every_minutes)} ·{" "}
                {p.last_error ? <span className="st-error">{p.last_error}</span> : p.last_run ? `last run ${new Date(p.last_run * 1000).toLocaleString()}: ${p.last_found} found, ${p.last_added} new` : "never run"}
              </small>
            </div>
            <button className="btn" disabled={running === p.id} onClick={async () => { setRunning(p.id); await onRun(p); setRunning(null); }}>
              {running === p.id ? "Running…" : "Run"}
            </button>
            {onOpen && <button className="btn" onClick={() => onOpen(p)}>Open folder</button>}
            <button className="btn" onClick={() => onEdit(p)}>Edit</button>
            <button className="btn" onClick={() => onDelete(p)}>Delete</button>
          </div>
        ))}
      </div>
    </>
  );
}

export function ProjectsDialog(p: {
  projects: GrabProject[];
  onRun: (p: GrabProject) => Promise<void>;
  onEdit: (p: GrabProject) => void;
  onDelete: (p: GrabProject) => void;
  onNew: () => void;
  onClose: () => void;
}) {
  return (
    <Modal title="Grabber projects" onClose={p.onClose} wide>
      <ProjectsList {...p} />
      <div className="modal-foot">
        <button className="btn" onClick={p.onClose}>Close</button>
        <button className="btn primary" onClick={p.onNew}>New project</button>
      </div>
    </Modal>
  );
}
