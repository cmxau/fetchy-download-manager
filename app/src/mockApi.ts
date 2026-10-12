// Browser-only stand-in for the Rust engine so the UI can be developed with
// `npm run dev` without launching Tauri. Never loaded inside the real app.
import type { Api } from "./api";
import type { CoreEvent, DownloadInfo, GrabProject, Settings } from "./types";

const MB = 1024 * 1024;
const now = Math.floor(Date.now() / 1000);
const day = (n: number) => now - n * 86400;

function mk(
  id: string, filename: string, category: DownloadInfo["category"], size: number,
  status: DownloadInfo["status"], pct: number, ts: number,
): DownloadInfo {
  return {
    id, url: `https://example.com/files/${filename}`, filename, dir: "C:\\Downloads", size,
    downloaded: Math.floor((size * pct) / 100), status, speed: 0, eta_secs: null, error: null,
    added_at: ts, completed_at: status === "complete" ? ts : null, resumable: true, category, last_try_at: ts, scan_status: null, verified: null, project: null, description: "", referer: null, parent: null,
    connections: 16, active_connections: 0, queue: "main",
  };
}

export async function mockApi(): Promise<Api> {
  let items: DownloadInfo[] = [
    mk("1", "UIUXMonster.png", "image", 745 * 1024, "complete", 100, day(2)),
    mk("2", "2pacCover.mp3", "music", 3 * MB, "downloading", 80, now),
    mk("3", "UIUXMonster.Zip", "compressed", 12 * MB, "complete", 100, day(2)),
    mk("4", "Details.pdf", "document", 2 * MB, "downloading", 50, day(2)),
    mk("5", "Better.Call.Saul.S02E10.720p.mkv", "video", 2.5 * 1024 * MB, "queued", 0, day(2)),
    mk("6", "Call.Of.Duty.apk", "apk", 12 * MB, "paused", 35, day(2)),
    mk("7", "2pacCover-archive.rar", "compressed", 12 * MB, "downloading", 10, now),
    mk("8", "Mima.exe", "program", 32 * MB, "complete", 100, day(2)),
  ];
  let settings: Settings = {
    default_dir: "C:\\Downloads", connections: 16, max_concurrent: 4, speed_limit: 0, user_agent: "Fetchy", api_port: 6800, organize: true, clipboard_monitor: true, theme: "dark", auto_update: true, update_check: "weekly", auto_install: false, minimize_to_tray: true, close_to_tray: false, categories: [], notifications: true, sound: false, start_with_system: false, proxy: { kind: "none", host: "", port: 8080, username: "", password: "" }, site_profiles: [], auto_retry: 3, scan_defender: false, post_command: "", ffmpeg_path: "",
    queues: [
      { id: "main", name: "Main download queue", max_concurrent: 4, schedule: null, speed_limit: 0, quota_per_hour: 0, sync: false, repeat_minutes: 0, start_on_startup: false, max_retries: 0, open_when_done: null, exit_when_done: false, power_when_done: null, force_power: false },
      { id: "sync", name: "Synchronization queue", max_concurrent: 2, schedule: null, speed_limit: 0, quota_per_hour: 0, sync: true, repeat_minutes: 60, start_on_startup: false, max_retries: 0, open_when_done: null, exit_when_done: false, power_when_done: null, force_power: false },
      { id: "limits", name: "Download limits", max_concurrent: 2, schedule: null, speed_limit: 0, quota_per_hour: 0, sync: false, repeat_minutes: 0, start_on_startup: false, max_retries: 0, open_when_done: null, exit_when_done: false, power_when_done: null, force_power: false },
    ],
  };
  let projects: GrabProject[] = [];
  const subs = new Set<(e: CoreEvent) => void>();
  const emit = (i: DownloadInfo) => subs.forEach((s) => s({ type: "updated", data: { ...i } }));

  setInterval(() => {
    for (const i of items) {
      if (i.status !== "downloading" || !i.size) continue;
      const speed = (6 + Math.random() * 4) * MB;
      i.speed = speed;
      i.active_connections = 12 + Math.floor(Math.random() * 4);
      i.downloaded = Math.min(i.size, i.downloaded + speed / 4);
      i.eta_secs = Math.round((i.size - i.downloaded) / speed);
      if (i.downloaded >= i.size) {
        Object.assign(i, { status: "complete", speed: 0, eta_secs: null, active_connections: 0, completed_at: Math.floor(Date.now() / 1000) });
      }
      emit(i);
    }
  }, 250);

  const find = (id: string) => items.find((i) => i.id === id);
  const set = (id: string, patch: Partial<DownloadInfo>) => {
    const i = find(id);
    if (i) { Object.assign(i, patch); emit(i); }
  };
  return {
    list: async () => items.map((i) => ({ ...i })),
    add: async (url) => {
      const name = decodeURIComponent(url.split("/").pop()?.split("?")[0] || "download");
      const i = mk(String(Math.random()), name, "other", 40 * MB, "downloading", 0, Math.floor(Date.now() / 1000));
      i.url = url;
      items = [i, ...items];
      emit(i);
      return { ...i };
    },
    pause: async (id) => set(id, { status: "paused", speed: 0, eta_secs: null, active_connections: 0 }),
    resume: async (id) => set(id, { status: "downloading" }),
    pauseAll: async () => items.forEach((i) => i.status === "downloading" && set(i.id, { status: "paused", speed: 0 })),
    resumeAll: async () => items.forEach((i) => i.status === "paused" && set(i.id, { status: "downloading" })),
    remove: async (id) => {
      items = items.filter((i) => i.id !== id);
      subs.forEach((s) => s({ type: "removed", data: id }));
    },
    redownload: async (id) => set(id, { status: "downloading", downloaded: 0 }),
    openFile: async () => {},
    openFolder: async () => {},
    getSettings: async () => settings,
    setSettings: async (s) => { settings = s; },
    grabAdd: async (_u, _o, selected) => selected.length,
    setDescription: async () => {},
    openDir: async () => {},
    getStats: async () => [],
    verifyChecksum: async (_id, expected) => ({ algo: "sha256", actual: expected, expected, ok: true }),
    openWith: async () => {},
    streamVariants: async () => [],
    addStream: async (v) => { const i = mk(String(Math.random()), "stream.mp4", "video", 40 * MB, "downloading", 0, Math.floor(Date.now() / 1000)); i.url = v.url; items = [i, ...items]; emit(i); return i; },
    grabSite: async (url) => [
      { url: url + "/photo-1.jpg", filename: "photo-1.jpg", page: url },
      { url: url + "/guide.pdf", filename: "guide.pdf", page: url },
    ],
    checkUpdate: async () => null,
    syncNow: async () => {},
    startQueue: async () => {},
    stopQueue: async () => {},
    moveInQueue: async () => {},
    listProjects: async () => projects,
    saveProject: async (p) => {
      const saved = { ...p, id: p.id || `p${projects.length + 1}` };
      projects = [...projects.filter((x) => x.id !== saved.id), saved];
      return saved;
    },
    deleteProject: async (id) => { projects = projects.filter((p) => p.id !== id); },
    runProject: async () => 2,
    powerAction: async () => {},
    details: async (id) => {
      const i = find(id);
      if (!i?.size) return null;
      const n = 8, part = Math.floor(i.size / n);
      return {
        hls: null,
        segments: Array.from({ length: n }, (_, k) => {
          const start = k * part, end = k === n - 1 ? i.size! : start + part;
          const frac = Math.min(1, (i.downloaded / i.size!) * (0.7 + (k % 3) * 0.3));
          const pos = start + (end - start) * frac;
          return { start, end, pos, active: i.status === "downloading" && pos < end };
        }),
      };
    },
    apiInfo: async () => ({ port: 6800, token: "demo-token-demo-token-demo-token-0000" }),
    moveToQueue: async (id, queue) => set(id, { queue }),
    exportLinks: async () => 0,
    importLinks: async () => 0,
    onEvent: async (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
  };
}
