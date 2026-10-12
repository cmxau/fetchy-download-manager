import { useEffect, useState } from "react";
import { CalendarClock, Globe, Home, Info, Moon, Settings2, Sun } from "lucide-react";
import type { Api } from "../api";
import { isTauri } from "../api";
import type { ApiInfo, DownloadInfo, GrabProject, Queue, Settings, SiteProfile, WhenDone } from "../types";
import { QueuesPanel } from "./QueuesDialog";
import { Modal, ProjectsList } from "./Dialogs";
import { Logo } from "./TitleBar";
import { LEGAL } from "../legal";

export type Section = "general" | "queues" | "grabber" | "about";

const NAV: { id: Section; label: string; icon: React.ReactNode }[] = [
  { id: "general", label: "General", icon: <Settings2 size={18} /> },
  { id: "queues", label: "Queues & Scheduler", icon: <CalendarClock size={18} /> },
  { id: "grabber", label: "Site grabber", icon: <Globe size={18} /> },
  { id: "about", label: "Updates & About", icon: <Info size={18} /> },
];

const MB = 1024 * 1024;

interface Props {
  section: Section;
  onSection: (s: Section) => void;
  settings: Settings;
  onChange: (s: Settings) => void;
  apiInfo: ApiInfo | null;
  api: Api | null;
  items: DownloadInfo[];
  projects: GrabProject[];
  whenDone: WhenDone;
  onWhenDone: (w: WhenDone) => void;
  onBack: () => void;
  onSaveQueues: (q: Queue[]) => void;
  onStartQueue: (id: string) => void;
  onStopQueue: (id: string) => void;
  onMove: (id: string, delta: number) => void;
  onSyncNow: () => void;
  onRunProject: (p: GrabProject) => Promise<void>;
  onEditProject: (p: GrabProject) => void;
  onDeleteProject: (p: GrabProject) => void;
  onNewProject: () => void;
  onOpenProject: (p: GrabProject) => void;
  onCheckUpdate: () => void;
}

function Row({ title, hint, children }: { title: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="srow">
      <div className="sinfo">
        <b>{title}</b>
        {hint && <small>{hint}</small>}
      </div>
      <div className="sctl">{children}</div>
    </div>
  );
}

function Switch({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button className={on ? "switch on" : "switch"} role="switch" aria-checked={on} onClick={() => onChange(!on)}>
      <i />
    </button>
  );
}

function SiteLogins({ list, onChange }: { list: SiteProfile[]; onChange: (l: SiteProfile[]) => void }) {
  const patch = (i: number, p: Partial<SiteProfile>) => onChange(list.map((x, j) => (j === i ? { ...x, ...p } : x)));
  return (
    <div className="profiles">
      {list.map((p, i) => (
        <div className="profile" key={i}>
          <input placeholder="site, e.g. example.com" value={p.host} onChange={(e) => patch(i, { host: e.target.value })} />
          <input placeholder="login" value={p.username} autoComplete="off" onChange={(e) => patch(i, { username: e.target.value })} />
          <input placeholder="password" type="password" value={p.password} autoComplete="off" onChange={(e) => patch(i, { password: e.target.value })} />
          <input placeholder="cookie (optional)" value={p.cookie} onChange={(e) => patch(i, { cookie: e.target.value })} />
          <button className="btn" onClick={() => onChange(list.filter((_, j) => j !== i))} aria-label="Remove">Remove</button>
        </div>
      ))}
      <button className="btn" onClick={() => onChange([...list, { host: "", username: "", password: "", cookie: "", referer: "" }])}>Add a site</button>
    </div>
  );
}

function GithubMark({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.5 7.5 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}

function openUrl(url: string) {
  void import("@tauri-apps/plugin-opener").then((o) => o.openUrl(url)).catch(() => window.open(url, "_blank"));
}

export function SettingsPage(p: Props) {
  const s = p.settings;
  const set = (patch: Partial<Settings>) => p.onChange({ ...s, ...patch });
  const [rules, setRules] = useState(
    s.categories
      .map((c) => `${c.folder}: ${[...c.exts, ...(c.sites ?? []).map((x) => "site:" + x), ...(c.names ?? []).map((x) => "name:" + x)].join(", ")}`)
      .join("\n"),
  );
  const [copied, setCopied] = useState(false);
  const [legal, setLegal] = useState<keyof typeof LEGAL | null>(null);
  const [version, setVersion] = useState("0.1.0");

  useEffect(() => {
    if (!isTauri) return;
    void import("@tauri-apps/api/app").then((a) => a.getVersion()).then(setVersion).catch(() => {});
  }, []);

  const saveRules = () =>
    set({
      categories: rules.split("\n").flatMap((line) => {
        const i = line.indexOf(":");
        if (i < 0) return [];
        const folder = line.slice(0, i).trim();
        const tokens = line.slice(i + 1).split(/[ ,;]+/).map((e) => e.trim()).filter(Boolean);
        const sites = tokens.filter((t) => /^site:/i.test(t)).map((t) => t.slice(5).toLowerCase());
        const names = tokens.filter((t) => /^name:/i.test(t)).map((t) => t.slice(5));
        const exts = tokens.filter((t) => !/^(site|name):/i.test(t)).map((t) => t.replace(/^\*?\./, "").toLowerCase());
        return folder && (exts.length || sites.length || names.length) ? [{ name: folder, folder, exts, sites, names }] : [];
      }),
    });

  const browse = async () => {
    if (!isTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const dir = await open({ directory: true, defaultPath: s.default_dir });
    if (typeof dir === "string") set({ default_dir: dir });
  };

  return (
    <div className="settings">
      <nav className="snav">
        <button className="sback" onClick={p.onBack}><Home size={18} /> Home</button>
        <div className="snav-title">Settings</div>
        {NAV.map((n) => (
          <button key={n.id} className={p.section === n.id ? "snav-item on" : "snav-item"} onClick={() => p.onSection(n.id)}>
            {n.icon}
            <span>{n.label}</span>
          </button>
        ))}
      </nav>

      <div className={p.section === "queues" ? "spanel stretch" : "spanel"}>
        {p.section === "general" && (
          <section>
            <h2>General</h2>

            <h3>Downloads</h3>
            <Row title="Download folder" hint="Fetchy sorts files into sub-folders inside it.">
              <div className="inline">
                <input value={s.default_dir} onChange={(e) => set({ default_dir: e.target.value })} />
                <button className="btn" onClick={browse}>Browse</button>
              </div>
            </Row>
            <Row title="Sort into folders by type" hint="Music, Videos, Documents, Compressed, Applications, APKs, Images.">
              <Switch on={s.organize} onChange={(v) => set({ organize: v })} />
            </Row>
            <Row title="Custom categories" hint='One per line, e.g. "Ebooks: epub, mobi" or "Bills: name:invoice*.pdf, site:mybank.com". These win over the built-in folders.'>
              <textarea rows={3} value={rules} spellCheck={false} onChange={(e) => setRules(e.target.value)} onBlur={saveRules} />
            </Row>
            <Row title="Offer to download copied links" hint="A small popup appears when you copy a link to a file or video.">
              <Switch on={s.clipboard_monitor} onChange={(v) => set({ clipboard_monitor: v })} />
            </Row>

            <Row title="Retry failed downloads automatically" hint="After a network error Fetchy waits a little longer each time and tries again. 0 turns it off.">
              <input type="number" min={0} max={20} value={s.auto_retry} onChange={(e) => set({ auto_retry: Math.max(0, +e.target.value) })} />
            </Row>

            <h3>After each download</h3>
            <Row title="Desktop notifications" hint="Tell me when a download finishes or fails.">
              <Switch on={s.notifications} onChange={(v) => set({ notifications: v })} />
            </Row>
            <Row title="Play a sound when a download finishes">
              <Switch on={s.sound} onChange={(v) => set({ sound: v })} />
            </Row>
            <Row title="Scan with Windows Defender" hint="Checks every finished file. A flagged file is marked in the list. Windows only.">
              <Switch on={s.scan_defender} onChange={(v) => set({ scan_defender: v })} />
            </Row>
            <Row title="Run a command" hint="Optional. {file} {dir} {name} {url} are filled in, e.g. clamscan {file}">
              <input value={s.post_command} placeholder="command to run after each download" spellCheck={false} onChange={(e) => set({ post_command: e.target.value })} />
            </Row>

            <h3>Network</h3>
            <Row title="Proxy" hint="Send downloads through an HTTP or SOCKS5 proxy.">
              <select value={s.proxy.kind} onChange={(e) => set({ proxy: { ...s.proxy, kind: e.target.value as Settings["proxy"]["kind"] } })}>
                <option value="none">No proxy</option>
                <option value="http">HTTP proxy</option>
                <option value="socks5">SOCKS5 proxy</option>
              </select>
            </Row>
            {s.proxy.kind !== "none" && (
              <>
                <Row title="Proxy address">
                  <div className="inline">
                    <input value={s.proxy.host} placeholder="host" onChange={(e) => set({ proxy: { ...s.proxy, host: e.target.value } })} />
                    <input className="port" type="number" min={1} max={65535} value={s.proxy.port} onChange={(e) => set({ proxy: { ...s.proxy, port: +e.target.value } })} />
                  </div>
                </Row>
                <Row title="Proxy login" hint="Leave empty if the proxy is open.">
                  <div className="inline">
                    <input value={s.proxy.username} placeholder="user" autoComplete="off" onChange={(e) => set({ proxy: { ...s.proxy, username: e.target.value } })} />
                    <input type="password" value={s.proxy.password} placeholder="password" autoComplete="off" onChange={(e) => set({ proxy: { ...s.proxy, password: e.target.value } })} />
                  </div>
                </Row>
              </>
            )}
            <div className="srow col">
              <div className="sinfo">
                <b>Logins and cookies per site</b>
                <small>Used automatically for every download from that site (sub-domains included).</small>
              </div>
              <SiteLogins list={s.site_profiles} onChange={(l) => set({ site_profiles: l })} />
            </div>

            <h3>Video</h3>
            <Row title="ffmpeg" hint="Needed to merge a video's separate audio track. Leave empty to look for it automatically (next to Fetchy or on the PATH).">
              <div className="inline">
                <input value={s.ffmpeg_path} placeholder="auto-detect" spellCheck={false} onChange={(e) => set({ ffmpeg_path: e.target.value })} />
                <button className="btn" onClick={async () => {
                  if (!isTauri) return;
                  const { open } = await import("@tauri-apps/plugin-dialog");
                  const f = await open({ multiple: false });
                  if (typeof f === "string") set({ ffmpeg_path: f });
                }}>Browse</button>
                <button className="btn" onClick={() => openUrl("https://ffmpeg.org/download.html")}>Get ffmpeg</button>
              </div>
            </Row>

            <h3>Connections</h3>
            <Row title={`Connections per download: ${s.connections}`} hint="More connections help on servers that throttle each one. 8–16 is a good range.">
              <input type="range" min={1} max={32} value={s.connections} onChange={(e) => set({ connections: +e.target.value })} />
            </Row>
            <Row title={`Simultaneous downloads: ${s.max_concurrent}`}>
              <input type="range" min={1} max={10} value={s.max_concurrent} onChange={(e) => set({ max_concurrent: +e.target.value })} />
            </Row>
            <Row title="Speed limit (MB/s)" hint="0 = unlimited. Applies to everything together; queues can add their own limits.">
              <input
                type="number" min={0} step={0.5} value={+(s.speed_limit / MB).toFixed(2)}
                onChange={(e) => set({ speed_limit: Math.max(0, Math.round(+e.target.value * MB)) })}
              />
            </Row>

            <h3>Appearance</h3>
            <Row title="Theme">
              <div className="choices">
                <button className={s.theme === "dark" ? "choice on" : "choice"} onClick={() => set({ theme: "dark" })}><Moon size={16} /> Dark</button>
                <button className={s.theme === "light" ? "choice on" : "choice"} onClick={() => set({ theme: "light" })}><Sun size={16} /> Light</button>
                <button className={s.theme === "system" ? "choice on" : "choice"} onClick={() => set({ theme: "system" })}>System</button>
              </div>
            </Row>
            <Row title="Start Fetchy when I sign in" hint="Starts hidden in the system tray.">
              <Switch on={s.start_with_system} onChange={(v) => set({ start_with_system: v })} />
            </Row>
            <Row title="Minimize to the system tray" hint="The minimize button hides Fetchy in the tray; downloads keep running.">
              <Switch on={s.minimize_to_tray} onChange={(v) => set({ minimize_to_tray: v })} />
            </Row>
            <Row title="Close to the system tray" hint="The close button hides Fetchy instead of quitting. Quit from the tray menu.">
              <Switch on={s.close_to_tray} onChange={(v) => set({ close_to_tray: v })} />
            </Row>

            <h3>When all downloads finish</h3>
            <Row title="Do this once, next time everything is done" hint="Per-queue actions are in Queues & Scheduler.">
              <select value={p.whenDone} onChange={(e) => p.onWhenDone(e.target.value as WhenDone)}>
                <option value="none">Do nothing</option>
                <option value="exit">Exit Fetchy</option>
                <option value="sleep">Sleep</option>
                <option value="hibernate">Hibernate</option>
                <option value="shutdown">Shut down</option>
              </select>
            </Row>

            <h3>Browser extension</h3>
            {p.apiInfo && p.apiInfo.port > 0 ? (
              <>
                <p className="lead">
                  Load the <code>extension</code> folder in Chrome, Edge, Brave, Opera or Firefox, then paste this token into its popup.
                  Fetchy listens on port <b>{p.apiInfo.port}</b>, on this computer only.
                </p>
                <Row title="Pairing token">
                  <div className="inline">
                    <input readOnly value={p.apiInfo.token} onFocus={(e) => e.target.select()} />
                    <button
                      className="btn"
                      onClick={() => {
                        void navigator.clipboard?.writeText(p.apiInfo!.token);
                        setCopied(true);
                        setTimeout(() => setCopied(false), 1500);
                      }}
                    >
                      {copied ? "Copied" : "Copy"}
                    </button>
                  </div>
                </Row>
              </>
            ) : (
              <p className="lead">The local extension API is not running (port unavailable).</p>
            )}
          </section>
        )}

        {p.section === "queues" && (
          <section>
            <h2>Queues &amp; Scheduler</h2>
            <QueuesPanel
              queues={s.queues} items={p.items} onSave={p.onSaveQueues} onStart={p.onStartQueue}
              onStop={p.onStopQueue} onMove={p.onMove} onSyncNow={p.onSyncNow}
            />
          </section>
        )}

        {p.section === "grabber" && (
          <section>
            <h2>Site grabber</h2>
            <p className="lead">Saved crawls of a site. Re-run one by hand or on a timer and only new files are downloaded.</p>
            <ProjectsList projects={p.projects} onRun={p.onRunProject} onEdit={p.onEditProject} onDelete={p.onDeleteProject} onOpen={p.onOpenProject} />
            <button className="btn primary" onClick={p.onNewProject}>New project</button>
          </section>
        )}

        {p.section === "about" && (
          <section className="about">
            <Logo size={104} />
            <h1>Fetchy</h1>
            <div className="ver">Version {version}</div>
            <div className="legal-links">
              <button onClick={() => setLegal("license")}>License info</button>
              <span>·</span>
              <button onClick={() => setLegal("terms")}>Terms</button>
              <span>·</span>
              <button onClick={() => setLegal("privacy")}>Privacy</button>
            </div>
            <p className="desc">A free, open-source download manager. It splits every file across several connections, resumes interrupted downloads, grabs whole sites and video streams, and works with your browser.</p>
            <button className="btn primary" onClick={p.onCheckUpdate}>Check for updates</button>
            <div className="upd-opts">
              <label className="upd-row">
                <span>Check for updates</span>
                <select value={s.update_check} onChange={(e) => set({ update_check: e.target.value as Settings["update_check"] })}>
                  <option value="startup">Every time Fetchy starts</option>
                  <option value="daily">Daily</option>
                  <option value="weekly">Weekly</option>
                  <option value="monthly">Monthly</option>
                  <option value="never">Never</option>
                </select>
              </label>
              <label className="upd-row">
                <span>Install updates automatically</span>
                <Switch on={s.auto_install} onChange={(v) => set({ auto_install: v })} />
              </label>
            </div>
            <div className="credit">
              <span>Designed and Developed by</span>
              <button className="gh" onClick={() => openUrl("https://github.com/cmxau")}>
                <GithubMark /> cmxau
              </button>
            </div>
          </section>
        )}
      </div>

      {legal && (
        <Modal title={LEGAL[legal].title} onClose={() => setLegal(null)} wide>
          <pre className="notes legal">{LEGAL[legal].body}</pre>
          <div className="modal-foot"><button className="btn primary" onClick={() => setLegal(null)}>Close</button></div>
        </Modal>
      )}
    </div>
  );
}
