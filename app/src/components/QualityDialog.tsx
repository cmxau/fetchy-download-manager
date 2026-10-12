import { useState } from "react";
import type { StreamVariant } from "../types";
import { Modal } from "./Dialogs";

export function QualityDialog({ url, variants, onPick, onClose }: {
  url: string;
  variants: StreamVariant[];
  onPick: (v: StreamVariant) => void;
  onClose: () => void;
}) {
  const [sel, setSel] = useState(0);
  return (
    <Modal title="Choose a quality" onClose={onClose}>
      <p className="msg" title={url}>This video comes in {variants.length} qualities. The best one is selected.</p>
      <div className="wstack">
        {variants.map((v, i) => (
          <label key={v.url} className={sel === i ? "opt on" : "opt"}>
            <input type="radio" name="quality" checked={sel === i} onChange={() => setSel(i)} />
            <div>
              <b>{v.label}</b>
              <small>{v.audio_url ? "Video and audio are downloaded separately and merged at the end (needs ffmpeg)." : "Video and audio together"}</small>
            </div>
          </label>
        ))}
      </div>
      <div className="modal-foot">
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn primary" autoFocus onClick={() => { onPick(variants[sel]); onClose(); }}>Download</button>
      </div>
    </Modal>
  );
}
