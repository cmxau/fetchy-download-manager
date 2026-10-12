import type { ApiInfo, CoreEvent, Details, DownloadInfo, ChecksumResult, DayStat, StreamVariant, FoundFile, GrabOptions, GrabProject, Settings, UpdateInfo } from "./types";

export interface AddOptions {
  dir?: string;
  filename?: string;
  referer?: string;
  cookie?: string;
  connections?: number;
  queue?: string;
  username?: string;
  password?: string;
  overwrite?: boolean;
  description?: string;
  checksum?: string;
}

export interface Api {
  list(): Promise<DownloadInfo[]>;
  add(url: string, options?: AddOptions): Promise<DownloadInfo>;
  pause(id: string): Promise<void>;
  resume(id: string): Promise<void>;
  pauseAll(): Promise<void>;
  resumeAll(): Promise<void>;
  remove(id: string, deleteFile: boolean): Promise<void>;
  redownload(id: string): Promise<void>;
  openFile(id: string): Promise<void>;
  openFolder(id: string): Promise<void>;
  getSettings(): Promise<Settings>;
  setSettings(s: Settings): Promise<void>;
  apiInfo(): Promise<ApiInfo>;
  details(id: string): Promise<Details | null>;
  moveToQueue(id: string, queue: string): Promise<void>;
  exportLinks(path: string, ids: string[]): Promise<number>;
  importLinks(path: string): Promise<number>;
  grabSite(url: string, options: GrabOptions): Promise<FoundFile[]>;
  grabAdd(url: string, options: GrabOptions, selected: string[], queue: string): Promise<number>;
  setDescription(id: string, text: string): Promise<void>;
  openDir(path: string): Promise<void>;
  getStats(): Promise<DayStat[]>;
  verifyChecksum(id: string, expected: string): Promise<ChecksumResult>;
  openWith(id: string): Promise<void>;
  streamVariants(url: string): Promise<StreamVariant[]>;
  addStream(variant: StreamVariant, name?: string, options?: AddOptions): Promise<DownloadInfo>;
  checkUpdate(): Promise<UpdateInfo | null>;
  syncNow(): Promise<void>;
  startQueue(id: string): Promise<void>;
  stopQueue(id: string): Promise<void>;
  moveInQueue(id: string, delta: number): Promise<void>;
  listProjects(): Promise<GrabProject[]>;
  saveProject(project: GrabProject): Promise<GrabProject>;
  deleteProject(id: string): Promise<void>;
  runProject(id: string): Promise<number>;
  powerAction(kind: string, force?: boolean): Promise<void>;
  /** Subscribe to engine events. Returns an unsubscribe function. */
  onEvent(cb: (e: CoreEvent) => void, onResync: () => void): Promise<() => void>;
}

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriApi(): Promise<Api> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  return {
    list: () => invoke("list_downloads"),
    add: (url, options) => invoke("add_download", { url, options }),
    pause: (id) => invoke("pause", { id }),
    resume: (id) => invoke("resume", { id }),
    pauseAll: () => invoke("pause_all"),
    resumeAll: () => invoke("resume_all"),
    remove: (id, deleteFile) => invoke("remove", { id, deleteFile }),
    redownload: (id) => invoke("redownload", { id }),
    openFile: (id) => invoke("open_file", { id }),
    openFolder: (id) => invoke("open_folder", { id }),
    getSettings: () => invoke("get_settings"),
    setSettings: (settings) => invoke("set_settings", { settings }),
    apiInfo: () => invoke("api_info"),
    details: (id) => invoke("get_details", { id }),
    moveToQueue: (id, queue) => invoke("move_to_queue", { id, queue }),
    exportLinks: (path, ids) => invoke("export_links", { path, ids }),
    importLinks: (path) => invoke("import_links", { path }),
    grabSite: (url, options) => invoke("grab_site", { url, options }),
    grabAdd: (url, options, selected, queue) => invoke("grab_add", { url, options, selected, queue }),
    setDescription: (id, text) => invoke("set_description", { id, text }),
    openDir: (path) => invoke("open_dir", { path }),
    getStats: () => invoke("get_stats"),
    verifyChecksum: (id, expected) => invoke("verify_checksum", { id, expected }),
    openWith: (id) => invoke("open_with", { id }),
    streamVariants: (url) => invoke("stream_variants", { url }),
    addStream: (variant, name, options) => invoke("add_stream", { variant, name, options }),
    checkUpdate: () => invoke("check_update"),
    syncNow: () => invoke("sync_now"),
    startQueue: (id) => invoke("start_queue", { id }),
    stopQueue: (id) => invoke("stop_queue", { id }),
    moveInQueue: (id, delta) => invoke("move_in_queue", { id, delta }),
    listProjects: () => invoke("list_grab_projects"),
    saveProject: (project) => invoke("save_grab_project", { project }),
    deleteProject: (id) => invoke("delete_grab_project", { id }),
    runProject: (id) => invoke("run_grab_project", { id }),
    powerAction: (kind, force) => invoke("power_action", { kind, force }),
    async onEvent(cb, onResync) {
      const a = await listen<CoreEvent>("fetchy://event", (e) => cb(e.payload));
      const b = await listen("fetchy://resync", onResync);
      return () => {
        a();
        b();
      };
    },
  };
}

export async function getApi(): Promise<Api> {
  if (isTauri) return tauriApi();
  return (await import("./mockApi")).mockApi();
}
