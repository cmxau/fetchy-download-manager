import { useState } from "react";
import { ArrowDown, ArrowUp } from "lucide-react";
import type { DownloadInfo, Queue, Schedule } from "../types";
import { isTauri } from "../api";
import { formatBytes, percent } from "../format";

const DAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const DEFAULT_SCHEDULE: Schedule = { enabled: false, start: "23:00", stop: null, days: 127 };
const BUILTIN = ["main", "sync", "limits"];
const MB = 1024 * 1024;

interface Props {
  queues: Queue[];
  items: DownloadInfo[];
  initial?: string;
  onSave: (q: Queue[]) => void;
  onStart: (id: string) => void;
  onStop: (id: string) => void;
  onMove: (id: string, delta: number) => void;
  onSyncNow?: () => void;
}

const newQueue = (id: string, name: string): Queue => ({
  id, name, max_concurrent: 2, schedule: null, speed_limit: 0, quota_per_hour: 0, sync: false, repeat_minutes: 0,
  start_on_startup: false, max_retries: 0, open_when_done: null, exit_when_done: false, power_when_done: null, force_power: false,
});

export function QueuesPanel({ queues, items, initial, onSave, onStart, onStop, onMove, onSyncNow }: Props) {
  const [list, setList] = useState<Queue[]>(queues);
  const [sel, setSel] = useState(initial && queues.some((q) => q.id === initial) ? initial : queues[0].id);
  const [tab, setTab] = useState<"schedule" | "files" | "limits">("schedule");
  const q = list.find((x) => x.id === sel) ?? list[0];
  const builtin = BUILTIN.includes(q.id);

  const patch = (p: Partial<Queue>) => setList(list.map((x) => (x.id === q.id ? { ...x, ...p } : x)));
  const sched = q.schedule ?? DEFAULT_SCHEDULE;
  const patchSched = (p: Partial<Schedule>) => patch({ schedule: { ...sched, ...p } });
  const hours = Math.floor(q.repeat_minutes / 60);
  const mins = q.repeat_minutes % 60;
  const setRepeat = (h: number, m: number) => patch({ repeat_minutes: Math.max(1, h * 60 + m) });

  const add = () => {
    const id = `q-${Date.now().toString(36)}`;
    setList([...list, newQueue(id, `Queue ${list.length + 1}`)]);
    setSel(id);
  };
  const del = () => {
    const rest = list.filter((x) => x.id !== q.id);
    setList(rest);
    setSel(rest[0].id);
  };
  const browse = async () => {
    if (!isTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const f = await open({ multiple: false });
    if (typeof f === "string") patch({ open_when_done: f });
  };

  // Engine order: oldest first.
  const files = items.filter((i) => i.queue === q.id).reverse();

  return (
    <div className="queues-panel">
      <div>
        <div className="q-layout">
          <div className="q-side">
            <div className="q-label">Queues</div>
            <div className="q-list">
              {list.map((x) => (
                <button key={x.id} className={x.id === q.id ? "q-item on" : "q-item"} onClick={() => setSel(x.id)}>
                  <span>{x.name}</span>
                  {x.schedule?.enabled && <em>⏲</em>}
                </button>
              ))}
            </div>
            <div className="q-btns">
              <button className="btn" onClick={add}>New queue</button>
              <button className="btn" disabled={builtin} onClick={del}>Delete</button>
              <button className="btn" onClick={() => { onSave(list); onStart(q.id); }}>Start now</button>
              <button className="btn" onClick={() => onStop(q.id)}>Stop</button>
            </div>
          </div>

          <div className="q-edit">
            <div className="q-title">{q.name}</div>
            <div className="tabs">
              <button className={tab === "schedule" ? "tab on" : "tab"} onClick={() => setTab("schedule")}>Schedule</button>
              <button className={tab === "files" ? "tab on" : "tab"} onClick={() => setTab("files")}>Files in the queue</button>
              <button className={tab === "limits" ? "tab on" : "tab"} onClick={() => setTab("limits")}>Limits</button>
            </div>

            {tab === "schedule" && (
              <div className="tabbody">
                <div className="radios">
                  <label className="checkline">
                    <input type="radio" name="mode" checked={!q.sync} disabled={builtin} onChange={() => patch({ sync: false })} />
                    One-time downloading
                  </label>
                  <label className="checkline">
                    <input type="radio" name="mode" checked={q.sync} disabled={builtin} onChange={() => patch({ sync: true })} />
                    Periodic synchronization
                  </label>
                </div>
                {q.sync && <small>Finished files are re-checked on the server; changed files are downloaded again.</small>}
                <hr />
                <label className="checkline">
                  <input type="checkbox" checked={q.start_on_startup} onChange={(e) => patch({ start_on_startup: e.target.checked })} />
                  Start download on Fetchy startup
                </label>
                <hr />
                <div className="line">
                  <label className="checkline">
                    <input type="checkbox" checked={sched.enabled} onChange={(e) => patchSched({ enabled: e.target.checked })} />
                    Start download at
                  </label>
                  <input type="time" disabled={!sched.enabled} value={sched.start} onChange={(e) => patchSched({ start: e.target.value || "00:00" })} />
                </div>
                <div className="line">
                  <label className="checkline">
                    <input type="checkbox" checked={q.repeat_minutes > 0} onChange={(e) => patch({ repeat_minutes: e.target.checked ? 60 : 0 })} />
                    Start again every
                  </label>
                  <input type="number" min={0} className="num" disabled={q.repeat_minutes === 0} value={hours} onChange={(e) => setRepeat(Math.max(0, +e.target.value), mins)} />
                  <span>hours</span>
                  <input type="number" min={0} max={59} className="num" disabled={q.repeat_minutes === 0} value={mins} onChange={(e) => setRepeat(hours, Math.min(59, Math.max(0, +e.target.value)))} />
                  <span>min</span>
                </div>
                <div className="days">
                  {DAYS.map((d, i) => (
                    <label key={d} className="checkline">
                      <input
                        type="checkbox" disabled={!sched.enabled} checked={!!(sched.days & (1 << i))}
                        onChange={() => patchSched({ days: sched.days ^ (1 << i) })}
                      />
                      {d}
                    </label>
                  ))}
                </div>
                <hr />
                <div className="line">
                  <label className="checkline">
                    <input
                      type="checkbox" checked={sched.stop !== null}
                      onChange={(e) => patch({ schedule: { ...sched, enabled: sched.enabled || e.target.checked, stop: e.target.checked ? "07:30" : null } })}
                    />
                    Stop download at
                  </label>
                  <input type="time" disabled={sched.stop === null} value={sched.stop ?? ""} onChange={(e) => patchSched({ stop: e.target.value || null })} />
                </div>
                <div className="line">
                  <label className="checkline">
                    <input type="checkbox" checked={q.max_retries > 0} onChange={(e) => patch({ max_retries: e.target.checked ? 10 : 0 })} />
                    Number of retries for each file if downloading failed:
                  </label>
                  <input type="number" min={1} className="num" disabled={q.max_retries === 0} value={q.max_retries || 10} onChange={(e) => patch({ max_retries: Math.max(1, +e.target.value) })} />
                </div>
                <hr />
                <label className="checkline">
                  <input type="checkbox" checked={q.open_when_done !== null} onChange={(e) => patch({ open_when_done: e.target.checked ? "" : null })} />
                  Open the following file when done:
                </label>
                <div className="line">
                  <input className="grow" disabled={q.open_when_done === null} value={q.open_when_done ?? ""} onChange={(e) => patch({ open_when_done: e.target.value })} />
                  <button className="btn" disabled={q.open_when_done === null} onClick={browse}>…</button>
                </div>
                <label className="checkline">
                  <input type="checkbox" checked={q.exit_when_done} onChange={(e) => patch({ exit_when_done: e.target.checked })} />
                  Exit Fetchy when done
                </label>
                <div className="line">
                  <label className="checkline">
                    <input type="checkbox" checked={q.power_when_done !== null} onChange={(e) => patch({ power_when_done: e.target.checked ? "shutdown" : null })} />
                    Turn off computer when done
                  </label>
                  <select disabled={q.power_when_done === null} value={q.power_when_done ?? "shutdown"} onChange={(e) => patch({ power_when_done: e.target.value })}>
                    <option value="shutdown">Shut down</option>
                    <option value="sleep">Sleep</option>
                    <option value="hibernate">Hibernate</option>
                  </select>
                </div>
                <label className="checkline indent">
                  <input type="checkbox" disabled={q.power_when_done !== "shutdown"} checked={q.force_power} onChange={(e) => patch({ force_power: e.target.checked })} />
                  Force processes to terminate
                </label>
              </div>
            )}

            {tab === "files" && (
              <div className="tabbody">
                <small>Downloads start from the top. Use the arrows to change the order.</small>
                <div className="qfiles">
                  {files.map((f, i) => (
                    <div className="qfile" key={f.id}>
                      <span className="qn" title={f.filename}>{f.filename}</span>
                      <span className="qs">{f.status === "complete" ? "Complete" : f.size ? `${percent(f)}% of ${formatBytes(f.size)}` : f.status}</span>
                      <button className="icon-btn" disabled={i === 0} onClick={() => onMove(f.id, -1)} aria-label="Move up"><ArrowUp size={15} /></button>
                      <button className="icon-btn" disabled={i === files.length - 1} onClick={() => onMove(f.id, 1)} aria-label="Move down"><ArrowDown size={15} /></button>
                    </div>
                  ))}
                  {files.length === 0 && <div className="qfile none">No files in this queue.</div>}
                </div>
                {q.sync && onSyncNow && <button className="btn" onClick={onSyncNow}>Check for changes now</button>}
              </div>
            )}

            {tab === "limits" && (
              <div className="tabbody form">
                <label>
                  <span>Name</span>
                  <input value={q.name} disabled={builtin} onChange={(e) => patch({ name: e.target.value })} />
                </label>
                <label>
                  <span>Simultaneous downloads <b>{q.max_concurrent}</b></span>
                  <input type="range" min={1} max={10} value={q.max_concurrent} onChange={(e) => patch({ max_concurrent: +e.target.value })} />
                </label>
                <div className="two">
                  <label>
                    <span>Speed limit (MB/s, 0 = none)</span>
                    <input type="number" min={0} step={0.5} value={+(q.speed_limit / MB).toFixed(2)} onChange={(e) => patch({ speed_limit: Math.max(0, Math.round(+e.target.value * MB)) })} />
                  </label>
                  <label>
                    <span>Quota (MB per hour, 0 = none)</span>
                    <input type="number" min={0} step={100} value={Math.round(q.quota_per_hour / MB)} onChange={(e) => patch({ quota_per_hour: Math.max(0, Math.round(+e.target.value * MB)) })} />
                  </label>
                </div>
                {q.id === "limits" && <small>Put downloads here when your connection has a fair-access policy.</small>}
              </div>
            )}
          </div>
        </div>
        <div className="modal-foot">
          <div className="spacer" />
          <button className="btn primary" onClick={() => onSave(list)}>Apply</button>
        </div>
      </div>
    </div>
  );
}
