export type Status = "queued" | "downloading" | "paused" | "complete" | "error";
export type Category = "music" | "compressed" | "video" | "program" | "document" | "apk" | "image" | "other";

export interface DownloadInfo {
  id: string;
  url: string;
  filename: string;
  dir: string;
  size: number | null;
  downloaded: number;
  status: Status;
  speed: number;
  eta_secs: number | null;
  error: string | null;
  added_at: number;
  completed_at: number | null;
  resumable: boolean;
  category: Category;
  connections: number;
  active_connections: number;
  queue: string;
  last_try_at: number | null;
  scan_status: string | null;
  verified: boolean | null;
  project: string | null;
  description: string;
  referer: string | null;
  parent: string | null;
}

export interface Settings {
  default_dir: string;
  connections: number;
  max_concurrent: number;
  speed_limit: number;
  user_agent: string;
  queues: Queue[];
  api_port: number;
  organize: boolean;
  clipboard_monitor: boolean;
  theme: "dark" | "light" | "system";
  auto_update: boolean;
  update_check: "startup" | "daily" | "weekly" | "monthly" | "never";
  auto_install: boolean;
  minimize_to_tray: boolean;
  close_to_tray: boolean;
  categories: { name: string; folder: string; exts: string[]; sites: string[]; names: string[] }[];
  notifications: boolean;
  sound: boolean;
  start_with_system: boolean;
  proxy: ProxySettings;
  site_profiles: SiteProfile[];
  auto_retry: number;
  scan_defender: boolean;
  post_command: string;
  ffmpeg_path: string;
}

export interface ProxySettings {
  kind: "none" | "http" | "socks5";
  host: string;
  port: number;
  username: string;
  password: string;
}

export interface SiteProfile {
  host: string;
  username: string;
  password: string;
  cookie: string;
  referer: string;
}

export interface DayStat {
  date: string;
  bytes: number;
}

export interface ChecksumResult {
  algo: string;
  actual: string;
  expected: string;
  ok: boolean;
}


export type CoreEvent =
  | { type: "updated"; data: DownloadInfo }
  | { type: "removed"; data: string };

export type Filter = "all" | "unfinished" | "finished" | Category | `queue:${string}` | `project:${string}`;

export interface Schedule {
  enabled: boolean;
  /** Local "HH:MM" */
  start: string;
  /** Local "HH:MM"; null = until end of day. Earlier than start = overnight. */
  stop: string | null;
  /** bit 0 = Monday … bit 6 = Sunday */
  days: number;
}

export interface Queue {
  id: string;
  name: string;
  max_concurrent: number;
  schedule: Schedule | null;
  speed_limit: number;
  quota_per_hour: number;
  sync: boolean;
  repeat_minutes: number;
  start_on_startup: boolean;
  max_retries: number;
  open_when_done: string | null;
  exit_when_done: boolean;
  power_when_done: string | null;
  force_power: boolean;
}

export interface ApiInfo {
  port: number;
  token: string;
}

export interface SegView {
  start: number;
  pos: number;
  end: number;
  active: boolean;
}

export interface Details {
  segments: SegView[];
  hls: { next: number; bytes: number; total: number } | null;
}

export interface LinkInfo {
  url: string;
  filename: string;
  size: number | null;
  category: Category;
  resumable: boolean;
  stream: boolean;
}

export interface GrabOptions {
  username: string;
  password: string;
  skip_urls: string[];
  project_name: string;
  save_mode: "project" | "category" | "category_folder" | "folder" | string;
  save_category: string;
  save_dir: string;
  keep_relative: boolean;
  offline_links: boolean;
  overwrite: boolean;
  whole_site: boolean;
  depth: number;
  levels_other: number;
  no_parent: boolean;
  same_domain: boolean;
  process_js: boolean;
  exts: string[];
  exclude_exts: string[];
  same_host: boolean;
  hide_duplicates: boolean;
  min_size_kb: number;
  max_pages: number;
  max_files: number;
}

export interface FoundFile {
  url: string;
  filename: string;
  page: string;
}

export interface UpdateInfo {
  version: string;
  notes: string;
  url: string;
}

export type WhenDone = "none" | "exit" | "sleep" | "hibernate" | "shutdown";

export interface GrabProject {
  id: string;
  name: string;
  url: string;
  options: GrabOptions;
  queue: string;
  every_minutes: number;
  last_run: number | null;
  last_found: number;
  last_added: number;
  last_error: string | null;
}

export interface StreamVariant {
  url: string;
  label: string;
  bandwidth: number;
  height: number | null;
  audio_url: string | null;
}
