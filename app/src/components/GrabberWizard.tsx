import { useState } from "react";
import { X } from "lucide-react";
import type { FoundFile, GrabOptions, GrabProject } from "../types";
import { isTauri } from "../api";

export const DEFAULT_GRAB: GrabOptions = {
  username: "", password: "", skip_urls: ["logout", "signout"],
  project_name: "", save_mode: "project", save_category: "Other", save_dir: "", keep_relative: false, offline_links: false, overwrite: false,
  whole_site: false, depth: 2, levels_other: 0, no_parent: false, same_domain: false, process_js: false,
  exts: [], exclude_exts: [], same_host: false, hide_duplicates: true, min_size_kb: 0, max_pages: 150, max_files: 2000,
};

const PICTURES = ["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "avif", "heic", "ico"];
const AUDIO = ["mp3", "flac", "wav", "aac", "ogg", "m4a", "opus", "wma"];
const VIDEO = ["mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "ts"];
const ARCHIVES = ["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "tgz", "iso"];
const DOCS = ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "epub", "odt", "csv"];
const PROGRAMS = ["exe", "msi", "dmg", "pkg", "deb", "rpm", "appimage", "apk"];

const FILTERS: { label: string; exts: string[] }[] = [
  { label: "All files (*.*)", exts: [] },
  { label: "Pictures", exts: PICTURES },
  { label: "Audio", exts: AUDIO },
  { label: "Video", exts: VIDEO },
  { label: "Archives", exts: ARCHIVES },
  { label: "Documents", exts: DOCS },
  { label: "Programs", exts: PROGRAMS },
];

const FOLDERS = ["Music", "Videos", "Compressed", "Programs", "Documents", "APKs", "Images", "Other"];

interface Template {
  label: string;
  patch: Partial<GrabOptions>;
}
const TEMPLATES: Template[] = [
  { label: "Custom settings", patch: {} },
  { label: "All pictures of the web site", patch: { whole_site: true, exts: PICTURES, exclude_exts: [], offline_links: false } },
  { label: "Audio files from the start page and from one next level", patch: { whole_site: false, depth: 1, exts: AUDIO, exclude_exts: [] } },
  { label: "All video files of the web site", patch: { whole_site: true, exts: VIDEO, exclude_exts: [] } },
  { label: "All files of the web site except web pages and images", patch: { whole_site: true, exts: [], exclude_exts: PICTURES } },
  { label: "The whole web site", patch: { whole_site: true, exts: [], exclude_exts: [], offline_links: true, save_mode: "folder", keep_relative: true } },
  {
    label: "Complete start page with all pictures, styles and scripts for offline browsing",
    patch: { whole_site: false, depth: 0, exts: [], exclude_exts: [], offline_links: true, save_mode: "folder", keep_relative: true, process_js: true },
  },
];

const STEP_TITLES = ["Set start page", "Save files to", "Set explorer filters", "Set file filters", "Explored files"];

interface Props {
  queues: { id: string; name: string }[];
  defaultDir: string;
  project?: GrabProject;
  onGrab: (url: string, o: GrabOptions) => Promise<FoundFile[]>;
  onAdd: (url: string, o: GrabOptions, selected: string[], queue: string) => Promise<number>;
  onSaveProject: (p: GrabProject) => Promise<GrabProject>;
  onDone: (message: string) => void;
  onClose: () => void;
}

const listToText = (l: string[]) => l.join(", ");
const textToList = (t: string) => t.split(/[ ,;]+/).map((e) => e.replace(/^\*?\./, "").trim().toLowerCase()).filter(Boolean);

function FilterRow({ label, none, value, onChange }: { label: string; none: string; value: string[]; onChange: (l: string[]) => void }) {
  const preset = FILTERS.find((f) => f.exts.join() === value.join());
  return (
    <div className="wfield">
      <span>{label}</span>
      <div className="inline">
        <select value={preset?.label ?? "custom"} onChange={(e) => { const f = FILTERS.find((x) => x.label === e.target.value); if (f) onChange(f.exts); }}>
          {FILTERS.map((f, i) => <option key={f.label} value={f.label}>{i === 0 ? none : f.label}</option>)}
          <option value="custom">Custom…</option>
        </select>
        <input value={listToText(value)} placeholder="extensions, e.g. jpg, png" onChange={(e) => onChange(textToList(e.target.value))} />
      </div>
    </div>
  );
}

export function GrabberWizard({ queues, defaultDir, project, onGrab, onAdd, onSaveProject, onDone, onClose }: Props) {
  const [step, setStep] = useState(1);
  const [name, setName] = useState(project?.name ?? "");
  const [url, setUrl] = useState(project?.url ?? "");
  const [o, setO] = useState<GrabOptions>({ ...DEFAULT_GRAB, ...(project?.options ?? {}) });
  const [template, setTemplate] = useState(0);
  const [auth, setAuth] = useState(!!project?.options.username);
  const [adv, setAdv] = useState(false);
  const [queue, setQueue] = useState(project?.queue ?? queues[0]?.id ?? "main");
  const [every, setEvery] = useState(project?.every_minutes ?? 0);
  const [startNow, setStartNow] = useState(false);
  const [addOnClose, setAddOnClose] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [found, setFound] = useState<FoundFile[] | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [added, setAdded] = useState(false);

  const set = (p: Partial<GrabOptions>) => setO((x) => ({ ...x, ...p }));

  const browse = async () => {
    if (!isTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const dir = await open({ directory: true, defaultPath: o.save_dir || defaultDir });
    if (typeof dir === "string") set({ save_dir: dir });
  };

  const finalOptions = (): GrabOptions => ({ ...o, project_name: name.trim() || hostName || "project_1", username: auth ? o.username : "", password: auth ? o.password : "" });

  const finish = async () => {
    setBusy(true);
    setErr("");
    const opts = finalOptions();
    try {
      const saved = await onSaveProject({
        id: project?.id ?? "", name: name.trim() || "", url: url.trim(), options: opts, queue, every_minutes: every,
        last_run: project?.last_run ?? null, last_found: project?.last_found ?? 0, last_added: project?.last_added ?? 0, last_error: null,
      });
      if (!name.trim()) setName(saved.name);
      const list = await onGrab(url.trim(), opts);
      if (startNow) {
        // Queue everything that matched, no review step.
        const n = await onAdd(url.trim(), opts, list.map((f) => f.url), queue);
        onDone(`${saved.name}: ${n} file${n === 1 ? "" : "s"} added`);
        onClose();
        return;
      }
      setFound(list);
      setPicked(new Set(list.map((f) => f.url)));
      setStep(5);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const addPicked = async () => {
    setBusy(true);
    try {
      const n = await onAdd(url.trim(), finalOptions(), [...picked], queue);
      setAdded(true);
      onDone(`${n} file${n === 1 ? "" : "s"} added to the download list`);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };
  const close = async () => {
    if (step === 5 && addOnClose && !added && picked.size > 0) await addPicked();
    onClose();
  };

  const canNext = step !== 1 || url.trim().length > 0;
  const toggle = (u: string) => {
    const n = new Set(picked);
    if (n.has(u)) n.delete(u); else n.add(u);
    setPicked(n);
  };
  const offline = o.offline_links;
  const hostName = (() => { try { return new URL(/^[a-z]+:\/\//i.test(url.trim()) ? url.trim() : "http://" + url.trim()).hostname; } catch { return ""; } })();
  const folderName = (name.trim() || hostName || "project_1").replace(/[\\/:*?"<>|]/g, "_");
  const projectDir = `${defaultDir.replace(/[\\/]+$/, "")}\\${folderName}`;

  return (
    <div className="scrim" onMouseDown={close}>
      <div className="modal wizard" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="Site grabber">
        <div className="modal-head">
          <h3>Site grabber{step <= 4 ? ` · step ${step} of 4` : ""} — {STEP_TITLES[step - 1]}</h3>
          <button className="icon-btn" onClick={close} aria-label="Close"><X size={18} /></button>
        </div>

        <div className="wbody">
          {step === 1 && (
            <div className="form">
              <label>
                <span>Grabber project name</span>
                <input value={name} placeholder="project_1" onChange={(e) => setName(e.target.value)} />
              </label>
              <label>
                <span>Start page / address</span>
                <input value={url} placeholder="https://example.com/gallery/" onChange={(e) => setUrl(e.target.value)} />
              </label>
              <label>
                <span>Project template</span>
                <select
                  value={template}
                  onChange={(e) => { const i = +e.target.value; setTemplate(i); set(TEMPLATES[i].patch); }}
                >
                  {TEMPLATES.map((t, i) => <option key={t.label} value={i}>{t.label}</option>)}
                </select>
                <small>With a template the wizard fills in the settings on the next steps. You can still change any of them.</small>
              </label>
              <label className="checkline">
                <input type="checkbox" checked={auth} onChange={(e) => setAuth(e.target.checked)} />
                Use authorization
              </label>
              <div className="two">
                <label><span>Login</span><input disabled={!auth} value={o.username} onChange={(e) => set({ username: e.target.value })} autoComplete="off" /></label>
                <label><span>Password</span><input type="password" disabled={!auth} value={o.password} onChange={(e) => set({ password: e.target.value })} autoComplete="off" /></label>
              </div>
              <button className="btn" onClick={() => setAdv(!adv)}>Advanced {adv ? "<<" : ">>"}</button>
              {adv && (
                <label>
                  <span>Never follow links whose address contains</span>
                  <input value={listToText(o.skip_urls)} onChange={(e) => set({ skip_urls: textToList(e.target.value) })} />
                  <small>Keeps the grabber from opening a logout page and ending your session.</small>
                </label>
              )}
            </div>
          )}

          {step === 2 && (
            <div className="wstack">
              <div className="wlabel">Where should the files go?</div>
              <label className={o.save_mode === "project" && !offline ? "opt on" : "opt"}>
                <input type="radio" name="save" checked={o.save_mode === "project"} onChange={() => set({ save_mode: "project" })} />
                <div>
                  <b>A folder named after the project</b>
                  <small title={projectDir}>{projectDir}</small>
                </div>
              </label>
              <label className={o.save_mode === "category" && !offline ? "opt on" : "opt"}>
                <input type="radio" name="save" checked={o.save_mode === "category"} disabled={offline} onChange={() => set({ save_mode: "category" })} />
                <div>
                  <b>Sort by file type</b>
                  <small>Pictures, music, videos… each goes to its usual Fetchy folder.</small>
                </div>
              </label>
              <label className={o.save_mode === "category_folder" && !offline ? "opt on" : "opt"}>
                <input type="radio" name="save" checked={o.save_mode === "category_folder"} disabled={offline} onChange={() => set({ save_mode: "category_folder" })} />
                <div>
                  <b>One category folder</b>
                  <select disabled={o.save_mode !== "category_folder" || offline} value={o.save_category} onChange={(e) => set({ save_category: e.target.value })}>
                    {FOLDERS.map((f) => <option key={f}>{f}</option>)}
                  </select>
                </div>
              </label>
              <label className={o.save_mode === "folder" && !offline ? "opt on" : "opt"}>
                <input type="radio" name="save" checked={o.save_mode === "folder"} onChange={() => set({ save_mode: "folder" })} />
                <div className="grow">
                  <b>A folder I choose</b>
                  <div className="inline">
                    <input disabled={o.save_mode !== "folder"} value={o.save_dir} placeholder={defaultDir} onChange={(e) => set({ save_dir: e.target.value })} />
                    <button className="btn" disabled={o.save_mode !== "folder"} onClick={browse}>Browse…</button>
                  </div>
                </div>
              </label>
              <div className="wlabel">Options</div>
              <label className="checkline">
                <input type="checkbox" disabled={o.save_mode === "category" || o.save_mode === "category_folder"} checked={o.keep_relative || offline} onChange={(e) => set({ keep_relative: e.target.checked })} />
                Keep the site&apos;s sub-folders (site / folder / file)
              </label>
              <label className="checkline">
                <input
                  type="checkbox" checked={offline}
                  onChange={(e) => set(e.target.checked ? { offline_links: true, keep_relative: true, save_mode: o.save_mode === "folder" ? "folder" : "project" } : { offline_links: false })}
                />
                Make an offline copy (rewrite links in saved pages to the local files)
              </label>
              <label className="checkline">
                <input type="checkbox" checked={o.overwrite || offline} disabled={offline} onChange={(e) => set({ overwrite: e.target.checked })} />
                Overwrite files that already exist
              </label>
              <label className="checkline">
                <input type="checkbox" checked={addOnClose} onChange={(e) => setAddOnClose(e.target.checked)} />
                Add the ticked files to the download list when I close the grabber
              </label>
            </div>
          )}

          {step === 3 && (
            <div className="wstack">
              <div className="wlabel">Which pages should be explored?</div>
              <label className={o.whole_site ? "opt on" : "opt"}>
                <input type="radio" name="lv" checked={o.whole_site} onChange={() => set({ whole_site: true })} />
                <div>
                  <b>The whole site</b>
                  <small>Follow every link on the start site (up to the page limit).</small>
                </div>
              </label>
              <label className={!o.whole_site ? "opt on" : "opt"}>
                <input type="radio" name="lv" checked={!o.whole_site} onChange={() => set({ whole_site: false })} />
                <div className="grow">
                  <b>A number of link levels</b>
                  <small>0 = only the start page, 1 = also the pages it links to, and so on.</small>
                  <div className="two">
                    <label className="mini"><span>On the start site</span><input type="number" min={0} max={20} disabled={o.whole_site} value={o.depth} onChange={(e) => set({ depth: Math.max(0, +e.target.value) })} /></label>
                    <label className="mini"><span>On other sites</span><input type="number" min={0} max={5} value={o.levels_other} onChange={(e) => set({ levels_other: Math.max(0, +e.target.value) })} /></label>
                  </div>
                </div>
              </label>
              <div className="wlabel">Limits</div>
              <label className="checkline">
                <input type="checkbox" checked={o.no_parent} onChange={(e) => set({ no_parent: e.target.checked })} />
                Stay inside the start page&apos;s folder (don&apos;t go to parent directories)
              </label>
              <label className="checkline">
                <input type="checkbox" checked={o.same_domain} onChange={(e) => set({ same_domain: e.target.checked })} />
                Treat sub-domains of the main domain as the same site
              </label>
              <label className="checkline">
                <input type="checkbox" checked={o.process_js} onChange={(e) => set({ process_js: e.target.checked })} />
                Look for links inside scripts (nothing is run, addresses are only read)
              </label>
            </div>
          )}

          {step === 4 && (
            <div className="form">
              <FilterRow label="Download the following files (file types)" none="All files (*.*)" value={o.exts} onChange={(l) => set({ exts: l })} />
              <FilterRow label="Don't download the following files (file types)" none="Don't use exclude filter" value={o.exclude_exts} onChange={(l) => set({ exclude_exts: l })} />
              <label className="checkline">
                <input type="checkbox" checked={o.same_host} onChange={(e) => set({ same_host: e.target.checked })} />
                Search files on this site only
              </label>
              <label className="checkline">
                <input type="checkbox" checked={o.hide_duplicates} onChange={(e) => set({ hide_duplicates: e.target.checked })} />
                Hide duplicate files found in different locations
              </label>
              <label className="checkline">
                <input type="checkbox" checked={startNow} onChange={(e) => setStartNow(e.target.checked)} />
                Start downloading all matched files at once
              </label>
              <small>Otherwise you can explore first, tick the files you want, and add them to the list yourself.</small>
              <button className="btn" onClick={() => setAdv(!adv)}>Advanced {adv ? "<<" : ">>"}</button>
              {adv && (
                <div className="two">
                  <label><span>Skip files smaller than (KB)</span><input type="number" min={0} value={o.min_size_kb} onChange={(e) => set({ min_size_kb: Math.max(0, +e.target.value) })} /></label>
                  <label>
                    <span>Explore again automatically</span>
                    <select value={every} onChange={(e) => setEvery(+e.target.value)}>
                      <option value={0}>Only when I run it</option>
                      <option value={30}>Every 30 minutes</option>
                      <option value={60}>Every hour</option>
                      <option value={360}>Every 6 hours</option>
                      <option value={1440}>Every day</option>
                    </select>
                  </label>
                  <label><span>Max pages to explore</span><input type="number" min={1} max={1000} value={o.max_pages} onChange={(e) => set({ max_pages: Math.max(1, +e.target.value) })} /></label>
                  <label>
                    <span>Add to queue</span>
                    <select value={queue} onChange={(e) => setQueue(e.target.value)}>
                      {queues.map((q) => <option key={q.id} value={q.id}>{q.name}</option>)}
                    </select>
                  </label>
                </div>
              )}
            </div>
          )}

          {step === 5 && found && (
            <>
              <div className="graph-head">
                <span>{found.length} file{found.length === 1 ? "" : "s"} found · {picked.size} selected</span>
                <em>
                  <button className="btn" onClick={() => setPicked(new Set(found.map((f) => f.url)))}>All</button>{" "}
                  <button className="btn" onClick={() => setPicked(new Set())}>None</button>
                </em>
              </div>
              <div className="found tall">
                {found.map((f) => (
                  <label key={f.url} className="checkline" title={f.url}>
                    <input type="checkbox" checked={picked.has(f.url)} onChange={() => toggle(f.url)} />
                    <span>{f.filename}</span>
                  </label>
                ))}
                {found.length === 0 && <p className="msg">No files matched. Try more link levels or a wider file filter.</p>}
              </div>
              <label className="checkline">
                <input type="checkbox" checked={addOnClose} onChange={(e) => setAddOnClose(e.target.checked)} />
                Add the checked files to the download list when I close the grabber
              </label>
            </>
          )}
          {err && <p className="msg st-error">{err}</p>}
        </div>

        <div className="modal-foot">
          {step === 5 ? (
            <>
              <button className="btn" onClick={() => { setStep(4); setAdded(false); }}>&lt; Back</button>
              <div className="spacer" />
              <button className="btn" onClick={close}>Close</button>
              <button className="btn primary" disabled={busy || picked.size === 0 || added} onClick={addPicked}>
                {added ? "Added" : `Download ${picked.size} selected`}
              </button>
            </>
          ) : (
            <>
              <button className="btn" disabled={step === 1} onClick={() => setStep(step - 1)}>&lt; Back</button>
              <div className="spacer" />
              <button className="btn" onClick={close}>Close</button>
              {step < 4 ? (
                <button className="btn primary" disabled={!canNext} onClick={() => setStep(step + 1)}>Next &gt;</button>
              ) : (
                <button className="btn primary" disabled={busy || !url.trim()} onClick={finish}>{busy ? "Exploring…" : "Finish"}</button>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
