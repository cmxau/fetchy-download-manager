import { useMemo, useState } from "react";
import { Modal } from "./Dialogs";

interface Props {
  onAdd: (urls: string[], auth: { user: string; pass: string } | null) => Promise<void>;
  onClose: () => void;
}

const MAX_BATCH = 5000;

/** Letters count like spreadsheet columns: a, b, ... z, aa, ab ... */
function letters(n: number, size: number, upper: boolean): string {
  let s = "";
  let x = n;
  do {
    s = String.fromCharCode(97 + (x % 26)) + s;
    x = Math.floor(x / 26) - 1;
  } while (x >= 0);
  s = s.padStart(size, "a");
  return upper ? s.toUpperCase() : s;
}
function letterIndex(t: string): number {
  let n = 0;
  for (const c of t.toLowerCase()) n = n * 26 + (c.charCodeAt(0) - 96);
  return n - 1;
}

export function BatchDialog({ onAdd, onClose }: Props) {
  const [address, setAddress] = useState("");
  const [mode, setMode] = useState<"numbers" | "letters">("numbers");
  const [from, setFrom] = useState("0");
  const [to, setTo] = useState("100");
  const [size, setSize] = useState(2);
  const [auth, setAuth] = useState(false);
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);

  const result = useMemo(() => {
    const star = address.indexOf("*");
    if (!address.trim() || star < 0) return { urls: [] as string[], error: address.trim() ? "Put a * in the address where the numbers or letters go." : "" };
    const head = address.slice(0, star);
    const tail = address.slice(star + 1);
    let a: number, b: number;
    if (mode === "numbers") {
      a = parseInt(from, 10);
      b = parseInt(to, 10);
      if (!Number.isFinite(a) || !Number.isFinite(b) || a < 0 || b < 0) return { urls: [], error: "From and To must be numbers." };
    } else {
      if (!/^[a-zA-Z]+$/.test(from) || !/^[a-zA-Z]+$/.test(to)) return { urls: [], error: "From and To must be letters." };
      a = letterIndex(from);
      b = letterIndex(to);
    }
    if (b < a) return { urls: [], error: "To must not be smaller than From." };
    if (b - a + 1 > MAX_BATCH) return { urls: [], error: `That is more than ${MAX_BATCH} files.` };
    const upper = mode === "letters" && /[A-Z]/.test(from);
    const urls: string[] = [];
    for (let i = a; i <= b; i++) {
      const mid = mode === "numbers" ? String(i).padStart(size, "0") : letters(i, size, upper);
      urls.push(head + mid + tail);
    }
    return { urls, error: "" };
  }, [address, mode, from, to, size]);

  const { urls, error } = result;
  const show = (i: number) => (urls.length > i ? urls[i] : "");

  return (
    <Modal title="Batch download" onClose={onClose} wide>
      <div className="form">
        <p className="msg">
          Add a group of numbered file names such as img001.jpg, img002.jpg … img100.jpg. Use * where the number goes,
          for example <code>https://example.com/pictures/img*.jpg</code>.
        </p>
        <label>
          <span>Address</span>
          <input value={address} placeholder="https://example.com/pictures/img*.jpg" spellCheck={false} autoFocus onChange={(e) => setAddress(e.target.value)} />
        </label>
        <fieldset>
          <legend>Replace the asterisk with</legend>
          <div className="radios">
            <label className="checkline"><input type="radio" name="bm" checked={mode === "numbers"} onChange={() => { setMode("numbers"); setFrom("0"); setTo("100"); }} /> Numbers</label>
            <label className="checkline"><input type="radio" name="bm" checked={mode === "letters"} onChange={() => { setMode("letters"); setFrom("a"); setTo("z"); }} /> Letters</label>
          </div>
          <div className="three">
            <label><span>From</span><input value={from} onChange={(e) => setFrom(e.target.value)} /></label>
            <label><span>To</span><input value={to} onChange={(e) => setTo(e.target.value)} /></label>
            <label><span>Wildcard size</span><input type="number" min={1} max={10} value={size} onChange={(e) => setSize(Math.min(10, Math.max(1, +e.target.value)))} /></label>
          </div>
        </fieldset>
        <fieldset>
          <legend className="checkline">
            <input type="checkbox" checked={auth} onChange={(e) => setAuth(e.target.checked)} /> Use authorization
          </legend>
          <div className="two">
            <label><span>Login</span><input disabled={!auth} value={user} onChange={(e) => setUser(e.target.value)} autoComplete="off" /></label>
            <label><span>Password</span><input type="password" disabled={!auth} value={pass} onChange={(e) => setPass(e.target.value)} autoComplete="off" /></label>
          </div>
        </fieldset>
        <div className="preview">
          <div><span>First file</span><code>{show(0)}</code></div>
          <div><span>Second file</span><code>{show(1)}</code></div>
          <div className="dots">…</div>
          <div><span>Last file</span><code>{urls.length > 2 ? urls[urls.length - 1] : ""}</code></div>
        </div>
        {error && <small className="st-error">{error}</small>}
        {urls.length > 0 && <small>{urls.length} file{urls.length === 1 ? "" : "s"} will be added.</small>}
      </div>
      <div className="modal-foot">
        <button className="btn" onClick={onClose}>Cancel</button>
        <button
          className="btn primary" disabled={urls.length === 0 || busy}
          onClick={async () => { setBusy(true); await onAdd(urls, auth && user ? { user, pass } : null); onClose(); }}
        >
          OK
        </button>
      </div>
    </Modal>
  );
}
