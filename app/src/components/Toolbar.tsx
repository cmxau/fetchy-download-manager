import { useEffect, useRef, useState } from "react";
import { CalendarClock, CircleStop, CloudDownload, Globe, Link2, ListPlus, Play, Share2, Square, Trash2 } from "lucide-react";

interface Props {
  onAdd: (text: string) => Promise<boolean>;
  inputRef: React.RefObject<HTMLInputElement | null>;
  hasSelection: boolean;
  onResume: () => void;
  onStop: () => void;
  onStopAll: () => void;
  onDelete: () => void;
  onScheduler: () => void;
  onGrabber: () => void;
  onBatch: () => void;
  onResumeAll: () => void;
  onCopyLinks: () => void;
  onExport: () => void;
  onImport: () => void;
}

function Tool({ icon, label, onClick, disabled }: { icon: React.ReactNode; label: string; onClick: () => void; disabled?: boolean }) {
  return (
    <button className="tool" onClick={onClick} disabled={disabled}>
      {icon}
      <span>{label}</span>
    </button>
  );
}

export function Toolbar(p: Props) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [share, setShare] = useState(false);
  const shareRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!share) return;
    const close = (e: MouseEvent) => {
      if (!shareRef.current?.contains(e.target as Node)) setShare(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [share]);

  const submit = async () => {
    if (!text.trim() || busy) return;
    setBusy(true);
    if (await p.onAdd(text)) setText("");
    setBusy(false);
  };

  return (
    <div className="toolbar">
      <div className="addurl">
        <Link2 size={20} />
        <input
          ref={p.inputRef}
          value={text}
          placeholder="Add Url"
          spellCheck={false}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submit()}
          onPaste={(e) => {
            // Several links pasted at once: queue them all immediately.
            const t = e.clipboardData.getData("text");
            if (/\n/.test(t.trim())) {
              e.preventDefault();
              void p.onAdd(t);
            }
          }}
        />
        <button className="go" aria-label="Download" onClick={submit} disabled={busy}>
          <CloudDownload size={20} />
        </button>
      </div>
      <i className="vsep" />
      <Tool icon={<Play size={22} />} label={p.hasSelection ? "Resume" : "Resume All"} onClick={p.hasSelection ? p.onResume : p.onResumeAll} />
      <Tool icon={<Square size={22} />} label="Stop" onClick={p.onStop} disabled={!p.hasSelection} />
      <Tool icon={<CircleStop size={22} />} label="Stop All" onClick={p.onStopAll} />
      <i className="vsep" />
      <Tool icon={<Trash2 size={22} />} label="Delete" onClick={p.onDelete} disabled={!p.hasSelection} />
      <Tool icon={<CalendarClock size={22} />} label="Scheduler" onClick={p.onScheduler} />
      <Tool icon={<ListPlus size={22} />} label="Batch" onClick={p.onBatch} />
      <Tool icon={<Globe size={22} />} label="Grabber" onClick={p.onGrabber} />
      <div className="share-wrap" ref={shareRef}>
        <Tool icon={<Share2 size={22} />} label="Share" onClick={() => setShare(!share)} />
        {share && (
          <div className="dropdown share-menu">
            <button onClick={() => { setShare(false); p.onCopyLinks(); }}>
              {p.hasSelection ? "Copy links of selected" : "Copy all links"}
            </button>
            <button onClick={() => { setShare(false); p.onExport(); }}>Export links to file…</button>
            <button onClick={() => { setShare(false); p.onImport(); }}>Import links from file…</button>
          </div>
        )}
      </div>
    </div>
  );
}
