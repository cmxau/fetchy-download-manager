use serde::{Deserialize, Serialize};

pub type DownloadId = String;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Downloading,
    Paused,
    Complete,
    Error,
}

/// A byte range `[pos, end)` still owed to the file. `start` is where the
/// range began and never changes, which lets us show per-segment progress.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Segment {
    pub start: u64,
    pub pos: u64,
    pub end: u64,
    #[serde(skip)]
    pub active: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AddOptions {
    pub dir: Option<String>,
    pub filename: Option<String>,
    pub referer: Option<String>,
    pub cookie: Option<String>,
    pub user_agent: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    pub connections: Option<u32>,
    pub queue: Option<String>,
    /// Start immediately (default true). When false the item waits paused.
    pub start: Option<bool>,
    /// Page the link came from (defaults to the referer).
    pub parent: Option<String>,
    pub project: Option<String>,
    /// Expected MD5 / SHA-1 / SHA-256 / SHA-512 of the finished file.
    pub checksum: Option<String>,
    /// Video and audio tracks that are merged into one file once both are downloaded.
    pub mux_group: Option<String>,
    /// "video" | "audio"
    pub mux_role: Option<String>,
    pub description: Option<String>,
    /// HTTP basic authorization.
    pub username: Option<String>,
    pub password: Option<String>,
    /// Replace an existing file of the same name instead of picking a new name.
    #[serde(default)]
    pub overwrite: bool,
}

/// Everything the UI needs to render one row.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DownloadInfo {
    pub id: DownloadId,
    pub url: String,
    pub filename: String,
    pub dir: String,
    pub size: Option<u64>,
    pub downloaded: u64,
    pub status: Status,
    pub speed: u64,
    pub eta_secs: Option<u64>,
    pub error: Option<String>,
    pub added_at: i64,
    pub completed_at: Option<i64>,
    pub resumable: bool,
    pub category: Category,
    pub connections: u32,
    pub active_connections: u32,
    #[serde(default = "main_queue")]
    pub queue: String,
    /// Unix seconds of the last time a transfer was started for this download.
    #[serde(default)]
    pub last_try_at: Option<i64>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub referer: Option<String>,
    /// The web page the link was found on.
    #[serde(default)]
    pub parent: Option<String>,
    /// Name of the site-grabber project this download belongs to.
    #[serde(default)]
    pub project: Option<String>,
    /// Result of the optional virus scan: "clean" | "threat" | "unavailable".
    #[serde(default)]
    pub scan_status: Option<String>,
    /// Whether the file matched the checksum it was added with.
    #[serde(default)]
    pub verified: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Music,
    Compressed,
    Video,
    Program,
    Document,
    Apk,
    Image,
    Other,
}

/// Persisted form of a download.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Record {
    pub info: DownloadInfo,
    pub opts: AddOptions,
    pub segments: Vec<Segment>,
    pub etag: Option<String>,
    /// Resume state for HLS downloads (segment index + bytes written so far).
    #[serde(default)]
    pub hls: Option<HlsState>,
    /// Automatic retries used since the last manual start.
    #[serde(default)]
    pub retries_used: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HlsState {
    pub next: usize,
    pub bytes: u64,
    pub total: usize,
}

pub const MAIN_QUEUE: &str = "main";

pub fn main_queue() -> String {
    MAIN_QUEUE.to_string()
}

/// Weekly time window during which a queue is allowed to run.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Schedule {
    pub enabled: bool,
    /// Local time "HH:MM".
    pub start: String,
    /// Local time "HH:MM"; `None` = until the end of the day. May be earlier
    /// than `start` for an overnight window.
    pub stop: Option<String>,
    /// Bit 0 = Monday .. bit 6 = Sunday. The day a window *starts* on.
    pub days: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Queue {
    pub id: String,
    pub name: String,
    pub max_concurrent: u32,
    #[serde(default)]
    pub schedule: Option<Schedule>,
    /// Bytes per second for this queue on top of the global limit; 0 = none.
    #[serde(default)]
    pub speed_limit: u64,
    /// Bytes per rolling hour for this queue (fair-access quota); 0 = none.
    #[serde(default)]
    pub quota_per_hour: u64,
    /// Synchronization queue: finished files are re-checked and re-downloaded when they change on the server.
    #[serde(default)]
    pub sync: bool,
    /// "Start again every": re-start the queue (or, for a synchronization queue, re-check files) this often. 0 = off.
    #[serde(default, alias = "sync_minutes")]
    pub repeat_minutes: u32,
    /// Start this queue as soon as Fetchy launches, ignoring its schedule.
    #[serde(default)]
    pub start_on_startup: bool,
    /// Attempts per file before giving up (0 = engine default).
    #[serde(default)]
    pub max_retries: u32,
    /// Open this file/program when the queue finishes.
    #[serde(default)]
    pub open_when_done: Option<String>,
    #[serde(default)]
    pub exit_when_done: bool,
    /// "shutdown" | "sleep" | "hibernate" when the queue finishes.
    #[serde(default)]
    pub power_when_done: Option<String>,
    /// Force running programs to close on shutdown.
    #[serde(default)]
    pub force_power: bool,
}

pub const SYNC_QUEUE: &str = "sync";
pub const LIMITS_QUEUE: &str = "limits";

impl Queue {
    pub fn new(id: &str, name: &str, max_concurrent: u32) -> Queue {
        Queue {
            id: id.into(),
            name: name.into(),
            max_concurrent,
            schedule: None,
            speed_limit: 0,
            quota_per_hour: 0,
            sync: false,
            repeat_minutes: 0,
            start_on_startup: false,
            max_retries: 0,
            open_when_done: None,
            exit_when_done: false,
            power_when_done: None,
            force_power: false,
        }
    }
}

/// The three queues every install has; adds any that are missing, in this order at the front.
pub fn ensure_builtin_queues(queues: &mut Vec<Queue>) {
    let defaults = [
        Queue::new(MAIN_QUEUE, "Main download queue", 4),
        Queue {
            sync: true,
            repeat_minutes: 60,
            ..Queue::new(SYNC_QUEUE, "Synchronization queue", 2)
        },
        Queue::new(LIMITS_QUEUE, "Download limits", 2),
    ];
    for (i, d) in defaults.into_iter().enumerate() {
        match queues.iter().position(|q| q.id == d.id) {
            Some(p) => {
                // Built-ins keep their identity; the user's own settings stay.
                queues[p].name = d.name;
                queues[p].sync = d.sync;
                if p != i {
                    let q = queues.remove(p);
                    queues.insert(i.min(queues.len()), q);
                }
            }
            None => queues.insert(i.min(queues.len()), d),
        }
    }
}

fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl Schedule {
    /// `weekday`: 0 = Monday. `minutes`: minutes since local midnight.
    pub fn is_active(&self, weekday: u32, minutes: u32) -> bool {
        if !self.enabled {
            return true;
        }
        let Some(start) = parse_hm(&self.start) else {
            return true;
        };
        let day_on = |d: u32| self.days & (1 << (d % 7)) != 0;
        match self.stop.as_deref().and_then(parse_hm) {
            None => day_on(weekday) && minutes >= start,
            Some(stop) if stop > start => day_on(weekday) && minutes >= start && minutes < stop,
            // Overnight (or zero-length -> treated as a full-day window).
            Some(stop) => {
                (day_on(weekday) && minutes >= start) || (day_on(weekday + 6) && minutes < stop)
            }
        }
    }
}

impl Queue {
    pub fn is_active_now(&self) -> bool {
        use chrono::{Datelike, Timelike};
        match &self.schedule {
            None => true,
            Some(s) => {
                let n = chrono::Local::now();
                s.is_active(
                    n.weekday().num_days_from_monday(),
                    n.hour() * 60 + n.minute(),
                )
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub default_dir: String,
    pub connections: u32,
    pub max_concurrent: u32,
    /// Bytes per second across all downloads; 0 = unlimited.
    pub speed_limit: u64,
    pub user_agent: String,
    pub queues: Vec<Queue>,
    /// Browser-extension API port (0 = disabled).
    pub api_port: u16,
    /// Sort new downloads into per-category subfolders (Music, Videos, ...).
    pub organize: bool,
    /// Offer to download links as soon as they are copied.
    pub clipboard_monitor: bool,
    /// "dark" | "light" | "system"
    pub theme: String,
    /// Check GitHub releases for a newer version about once a week.
    pub auto_update: bool,
    /// How often to check: "startup" | "daily" | "weekly" | "monthly" | "never".
    pub update_check: String,
    /// Download and install a found update without asking.
    pub auto_install: bool,
    /// The minimize button hides the window in the system tray.
    pub minimize_to_tray: bool,
    /// The close button hides the window in the tray instead of quitting.
    pub close_to_tray: bool,
    /// User-editable category -> extensions / folder rules.
    pub categories: Vec<CategoryRule>,
    pub notifications: bool,
    /// Play a short sound when a download finishes.
    pub sound: bool,
    pub start_with_system: bool,
    pub proxy: ProxySettings,
    pub site_profiles: Vec<SiteProfile>,
    /// Automatic retries (with a growing delay) for downloads that fail on a network error.
    pub auto_retry: u32,
    /// Scan finished files with Windows Defender.
    pub scan_defender: bool,
    /// Command to run after each download: {file} {dir} {name} {url}.
    pub post_command: String,
    /// Path to ffmpeg (used to merge separate video and audio tracks). Empty = look for it.
    pub ffmpeg_path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CategoryRule {
    pub name: String,
    /// Folder name under the download directory.
    pub folder: String,
    /// Lower-case extensions without the dot.
    pub exts: Vec<String>,
    /// Sites (host names, sub-domains included) whose downloads go to this folder.
    #[serde(default)]
    pub sites: Vec<String>,
    /// File-name patterns with * and ?, e.g. "invoice*.pdf".
    #[serde(default)]
    pub names: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ProxySettings {
    /// "none" | "http" | "socks5"
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl Default for ProxySettings {
    fn default() -> Self {
        ProxySettings {
            kind: "none".into(),
            host: String::new(),
            port: 8080,
            username: String::new(),
            password: String::new(),
        }
    }
}

/// Login / cookie / referer applied automatically to every download from a site.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct SiteProfile {
    pub host: String,
    pub username: String,
    pub password: String,
    pub cookie: String,
    pub referer: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            queues: {
                let mut q = Vec::new();
                ensure_builtin_queues(&mut q);
                q
            },
            api_port: 6800,
            organize: true,
            clipboard_monitor: true,
            theme: "dark".into(),
            auto_update: true,
            update_check: "weekly".into(),
            auto_install: false,
            minimize_to_tray: true,
            close_to_tray: false,
            categories: Vec::new(),
            notifications: true,
            sound: false,
            start_with_system: false,
            proxy: ProxySettings::default(),
            site_profiles: Vec::new(),
            auto_retry: 3,
            scan_defender: false,
            post_command: String::new(),
            ffmpeg_path: String::new(),
            default_dir: dirs::download_dir()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
                .join("Fetchy")
                .to_string_lossy()
                .into_owned(),
            connections: 16,
            max_concurrent: 4,
            speed_limit: 0,
            user_agent: format!("Fetchy/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)] // events are short-lived; boxing would touch every send site
pub enum Event {
    Updated(DownloadInfo),
    Removed(DownloadId),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Fatal(String),
    #[error("{0}")]
    Retry(String),
    #[error("cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched(start: &str, stop: Option<&str>, days: u8) -> Schedule {
        Schedule {
            enabled: true,
            start: start.into(),
            stop: stop.map(Into::into),
            days,
        }
    }

    #[test]
    fn daytime_window() {
        let s = sched("09:00", Some("17:00"), 0b0011111); // Mon-Fri
        assert!(s.is_active(0, 9 * 60));
        assert!(!s.is_active(0, 17 * 60));
        assert!(!s.is_active(0, 8 * 60 + 59));
        assert!(!s.is_active(5, 12 * 60)); // Saturday
    }

    #[test]
    fn overnight_window_spans_midnight() {
        let s = sched("23:00", Some("06:00"), 0b0000001); // starts Monday night
        assert!(s.is_active(0, 23 * 60 + 30)); // Mon 23:30
        assert!(s.is_active(1, 5 * 60)); // Tue 05:00 (still Monday's window)
        assert!(!s.is_active(1, 7 * 60));
        assert!(!s.is_active(1, 23 * 60 + 30)); // Tue night not selected
        assert!(!s.is_active(0, 5 * 60)); // Mon morning belongs to Sunday's window
    }

    #[test]
    fn no_stop_runs_until_midnight_and_disabled_is_always_on() {
        let s = sched("20:00", None, 127);
        assert!(s.is_active(3, 23 * 60 + 59));
        assert!(!s.is_active(3, 19 * 60));
        let mut d = s.clone();
        d.enabled = false;
        assert!(d.is_active(3, 0));
    }

    #[test]
    fn old_settings_files_still_load() {
        let s: Settings = serde_json::from_str(r#"{"default_dir":"/x","connections":8}"#).unwrap();
        assert_eq!(s.connections, 8);
        assert_eq!(s.queues[0].id, MAIN_QUEUE);
    }
}

/// One byte range of a download, as shown in the details view.
#[derive(Serialize, Clone, Debug)]
pub struct SegView {
    pub start: u64,
    pub pos: u64,
    /// For streams of unknown length this equals `pos`.
    pub end: u64,
    pub active: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Details {
    pub segments: Vec<SegView>,
    pub hls: Option<HlsState>,
}

/// What we learned about a link the user copied.
#[derive(Serialize, Clone, Debug)]
pub struct LinkInfo {
    pub url: String,
    pub filename: String,
    pub size: Option<u64>,
    pub category: Category,
    pub resumable: bool,
    pub stream: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct GrabOptions {
    // -- step 1: where to start
    pub username: String,
    pub password: String,
    /// Links containing any of these (case-insensitive) are never followed, e.g. "logout".
    pub skip_urls: Vec<String>,
    // -- step 2: where to save
    /// Name of the project the files belong to (also the folder name in save_mode "project").
    pub project_name: String,
    /// "project" | "category" | "category_folder" | "folder"
    pub save_mode: String,
    /// Folder name (Music, Videos, ...) for save_mode = "category_folder".
    pub save_category: String,
    pub save_dir: String,
    /// Keep the site's folder layout (host/dir/file) under the save folder.
    pub keep_relative: bool,
    /// Download pages too and rewrite their links so the copy works offline.
    pub offline_links: bool,
    pub overwrite: bool,
    // -- step 3: which pages to explore
    pub whole_site: bool,
    /// Link levels followed within the start site (0 = just the start page).
    pub depth: u32,
    /// Link levels followed on other sites.
    pub levels_other: u32,
    pub no_parent: bool,
    /// Treat sub-domains of the start site's domain as the same site.
    pub same_domain: bool,
    /// Also look for links inside scripts.
    pub process_js: bool,
    // -- step 4: which files to keep
    /// Only keep these extensions (empty = every file).
    pub exts: Vec<String>,
    pub exclude_exts: Vec<String>,
    /// Only keep files hosted on the start site.
    pub same_host: bool,
    pub hide_duplicates: bool,
    pub min_size_kb: u32,
    pub max_pages: u32,
    pub max_files: u32,
}

impl Default for GrabOptions {
    fn default() -> Self {
        GrabOptions {
            username: String::new(),
            password: String::new(),
            skip_urls: vec!["logout".into(), "signout".into()],
            project_name: String::new(),
            save_mode: "project".into(),
            save_category: "Other".into(),
            save_dir: String::new(),
            keep_relative: false,
            offline_links: false,
            overwrite: false,
            whole_site: false,
            depth: 2,
            levels_other: 0,
            no_parent: false,
            same_domain: false,
            process_js: false,
            exts: Vec::new(),
            exclude_exts: Vec::new(),
            same_host: false,
            hide_duplicates: true,
            min_size_kb: 0,
            max_pages: 150,
            max_files: 2000,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct FoundFile {
    pub url: String,
    pub filename: String,
    pub page: String,
}

/// A saved site-grabber job that can be re-run by hand or on a timer.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct GrabProject {
    pub id: String,
    pub name: String,
    pub url: String,
    pub options: GrabOptions,
    pub queue: String,
    /// Re-run every N minutes to pick up new files (0 = only when started by hand).
    pub every_minutes: u32,
    /// Unix seconds of the last run.
    pub last_run: Option<i64>,
    pub last_found: u32,
    pub last_added: u32,
    pub last_error: Option<String>,
}

impl Default for GrabProject {
    fn default() -> Self {
        GrabProject {
            id: String::new(),
            name: String::new(),
            url: String::new(),
            options: GrabOptions::default(),
            queue: main_queue(),
            every_minutes: 0,
            last_run: None,
            last_found: 0,
            last_added: 0,
            last_error: None,
        }
    }
}

/// One quality of an HLS stream.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StreamVariant {
    pub url: String,
    pub label: String,
    pub bandwidth: u64,
    pub height: Option<u32>,
    /// A separate audio track that has to be merged with this video.
    pub audio_url: Option<String>,
}
