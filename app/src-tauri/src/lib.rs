use fetchy_core::{
    AddOptions, Details, DownloadInfo, FoundFile, GrabOptions, GrabProject, LinkInfo, Manager,
    Settings, UpdateInfo,
};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{Emitter, Manager as _, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

type Mgr<'a> = State<'a, Manager>;

#[tauri::command]
fn get_details(m: Mgr, id: String) -> Option<Details> {
    m.details(&id)
}

#[tauri::command]
fn list_downloads(m: Mgr) -> Vec<DownloadInfo> {
    m.list()
}

#[tauri::command]
async fn add_download(
    m: Mgr<'_>,
    url: String,
    options: Option<AddOptions>,
) -> Result<DownloadInfo, String> {
    m.add_smart(&url, options.unwrap_or_default()).await
}

#[tauri::command]
async fn stream_variants(
    m: Mgr<'_>,
    url: String,
) -> Result<Vec<fetchy_core::StreamVariant>, String> {
    m.stream_variants(&url).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn add_stream(
    m: Mgr,
    variant: fetchy_core::StreamVariant,
    name: Option<String>,
    options: Option<AddOptions>,
) -> Result<DownloadInfo, String> {
    m.add_stream(&variant, name, options.unwrap_or_default())
}

#[tauri::command]
fn pause(m: Mgr, id: String) {
    m.pause(&id)
}

#[tauri::command]
fn resume(m: Mgr, id: String) {
    m.resume(&id)
}

#[tauri::command]
fn pause_all(m: Mgr) {
    m.pause_all()
}

#[tauri::command]
fn resume_all(m: Mgr) {
    m.resume_all()
}

#[tauri::command]
async fn remove(m: Mgr<'_>, id: String, delete_file: bool) -> Result<(), ()> {
    m.remove(&id, delete_file).await;
    Ok(())
}

#[tauri::command]
async fn redownload(m: Mgr<'_>, id: String) -> Result<(), ()> {
    m.redownload(&id).await;
    Ok(())
}

#[tauri::command]
fn open_file(app: tauri::AppHandle, m: Mgr, id: String) -> Result<(), String> {
    let p = m.full_path(&id).ok_or("Unknown download")?;
    app.opener()
        .open_path(p.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_folder(app: tauri::AppHandle, m: Mgr, id: String) -> Result<(), String> {
    let p = m.full_path(&id).ok_or("Unknown download")?;
    if p.exists() {
        app.opener()
            .reveal_item_in_dir(&p)
            .map_err(|e| e.to_string())
    } else {
        let dir = p.parent().map(PathBuf::from).unwrap_or_default();
        app.opener()
            .open_path(dir.to_string_lossy(), None::<&str>)
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn get_settings(m: Mgr) -> Settings {
    m.settings()
}

#[tauri::command]
fn set_settings(app: tauri::AppHandle, m: Mgr, settings: Settings) {
    use tauri_plugin_autostart::ManagerExt;
    let want = settings.start_with_system;
    m.set_settings(settings);
    let auto = app.autolaunch();
    if auto.is_enabled().unwrap_or(false) != want {
        let _ = if want { auto.enable() } else { auto.disable() };
    }
}

#[tauri::command]
fn get_stats(m: Mgr) -> Vec<fetchy_core::extras::DayStat> {
    m.stats()
}

#[tauri::command]
async fn verify_checksum(
    m: Mgr<'_>,
    id: String,
    expected: String,
) -> Result<fetchy_core::extras::ChecksumResult, String> {
    m.verify_checksum(&id, &expected)
        .await
        .map_err(|e| e.to_string())
}

/// "Open with…": the system's application chooser for a finished file.
#[tauri::command]
fn open_with(m: Mgr, id: String) -> Result<(), String> {
    let p = m.full_path(&id).ok_or("Unknown download")?;
    if !p.exists() {
        return Err("The file is not there any more".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("rundll32.exe")
            .arg("shell32.dll,OpenAs_RunDLL")
            .arg(&p)
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(p.parent().unwrap_or(&p))
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
}

#[derive(Serialize, Clone)]
struct ApiInfo {
    port: u16,
    token: String,
}

#[tauri::command]
fn api_info(i: State<ApiInfo>) -> ApiInfo {
    i.inner().clone()
}

#[tauri::command]
fn move_to_queue(m: Mgr, id: String, queue: String) {
    m.move_to_queue(&id, &queue)
}

#[tauri::command]
fn export_links(m: Mgr, path: String, ids: Vec<String>) -> Result<usize, String> {
    let links: Vec<String> = m
        .list()
        .into_iter()
        .filter(|i| ids.is_empty() || ids.contains(&i.id))
        .map(|i| i.url)
        .collect();
    std::fs::write(&path, links.join("\n") + "\n").map_err(|e| e.to_string())?;
    Ok(links.len())
}

#[tauri::command]
fn import_links(m: Mgr, path: String) -> Result<usize, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut n = 0;
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
    {
        if m.add(line, AddOptions::default()).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

#[tauri::command]
async fn grab_site(
    m: Mgr<'_>,
    url: String,
    options: GrabOptions,
) -> Result<Vec<FoundFile>, String> {
    m.grab(&url, &options).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn list_grab_projects(m: Mgr) -> Vec<GrabProject> {
    m.grab_projects()
}

#[tauri::command]
fn save_grab_project(m: Mgr, project: GrabProject) -> GrabProject {
    m.save_grab_project(project)
}

#[tauri::command]
fn delete_grab_project(m: Mgr, id: String) {
    m.delete_grab_project(&id)
}

#[tauri::command]
async fn run_grab_project(m: Mgr<'_>, id: String) -> Result<u32, String> {
    m.run_grab_project(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn sync_now(m: Mgr) {
    m.sync_check(true)
}

#[tauri::command]
fn start_queue(m: Mgr, id: String) {
    m.start_queue(&id)
}

#[tauri::command]
fn stop_queue(m: Mgr, id: String) {
    m.stop_queue(&id)
}

#[tauri::command]
fn move_in_queue(m: Mgr, id: String, delta: i32) {
    m.move_in_queue(&id, delta)
}

#[tauri::command]
async fn grab_add(
    m: Mgr<'_>,
    url: String,
    options: GrabOptions,
    selected: Vec<String>,
    queue: String,
) -> Result<u32, String> {
    m.grab_add(&url, &options, Some(&selected), &queue)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_description(m: Mgr, id: String, text: String) {
    m.set_description(&id, &text)
}

/// The custom title-bar buttons: minimize / close, honouring the tray settings.
#[tauri::command]
fn window_action(app: tauri::AppHandle, m: Mgr, op: String) {
    let s = m.settings();
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    match op.as_str() {
        "minimize" => {
            if s.minimize_to_tray {
                let _ = w.hide();
            } else {
                let _ = w.minimize();
            }
        }
        "close" => {
            if s.close_to_tray {
                let _ = w.hide();
            } else {
                app.exit(0);
            }
        }
        _ => {}
    }
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let open = MenuItemBuilder::with_id("show", "Open Fetchy").build(app)?;
    let resume = MenuItemBuilder::with_id("resume", "Resume all").build(app)?;
    let pause = MenuItemBuilder::with_id("pause", "Pause all").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Fetchy").build(app)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&open, &sep, &resume, &pause, &sep2, &quit])
        .build()?;
    let mut b = TrayIconBuilder::with_id("main")
        .tooltip("Fetchy")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "resume" => app.state::<Manager>().resume_all(),
            "pause" => app.state::<Manager>().pause_all(),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    b.build(app)?;
    Ok(())
}

#[tauri::command]
fn open_dir(app: tauri::AppHandle, path: String) -> Result<(), String> {
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn check_update(m: Mgr<'_>) -> Result<Option<UpdateInfo>, String> {
    m.check_update().await.map_err(|e| e.to_string())
}

/// "shutdown" | "sleep" | "hibernate" | "cancel" - the OS-level action after all downloads finish.
#[tauri::command]
fn power_action(kind: String, force: Option<bool>) -> Result<(), String> {
    #[allow(unused_variables)]
    let force = force.unwrap_or(false);
    use std::process::Command;
    let run = |cmd: &mut Command| cmd.spawn().map(|_| ()).map_err(|e| e.to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        match kind.as_str() {
            "shutdown" if force => run(Command::new("shutdown")
                .args(["/s", "/f", "/t", "60"])
                .creation_flags(NO_WINDOW)),
            "shutdown" => run(Command::new("shutdown")
                .args(["/s", "/t", "60"])
                .creation_flags(NO_WINDOW)),
            "cancel" => run(Command::new("shutdown").arg("/a").creation_flags(NO_WINDOW)),
            "hibernate" => run(Command::new("shutdown").arg("/h").creation_flags(NO_WINDOW)),
            "sleep" => run(Command::new("rundll32.exe")
                .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                .creation_flags(NO_WINDOW)),
            _ => Err("unknown action".into()),
        }
    }
    #[cfg(target_os = "macos")]
    {
        return match kind.as_str() {
            "shutdown" => {
                run(Command::new("osascript")
                    .args(["-e", "tell app \"System Events\" to shut down"]))
            }
            "sleep" | "hibernate" => run(Command::new("pmset").arg("sleepnow")),
            "cancel" => Ok(()),
            _ => Err("unknown action".into()),
        };
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return match kind.as_str() {
            "shutdown" => run(Command::new("systemctl").arg("poweroff")),
            "sleep" => run(Command::new("systemctl").arg("suspend")),
            "hibernate" => run(Command::new("systemctl").arg("hibernate")),
            "cancel" => Ok(()),
            _ => Err("unknown action".into()),
        };
    }
}

const CLIP_W: f64 = 430.0;
const CLIP_H: f64 = 196.0;

/// A small always-on-top window (like IDM's "download this?" popup) that does not steal focus.
fn show_clip_popup(app: &tauri::AppHandle, info: &LinkInfo) {
    if let Some(old) = app.get_webview_window("clip") {
        let _ = old.close();
    }
    let Ok(json) = serde_json::to_string(info) else {
        return;
    };
    let mut b = WebviewWindowBuilder::new(app, "clip", WebviewUrl::App("index.html?clip=1".into()))
        .title("Fetchy")
        .inner_size(CLIP_W, CLIP_H)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .initialization_script(format!("window.__CLIP__ = {json};"));
    // Bottom-right of the primary monitor, above the taskbar.
    if let Ok(Some(mon)) = app.primary_monitor() {
        let sf = mon.scale_factor();
        let (p, sz) = (mon.position(), mon.size());
        let x = p.x as f64 / sf + sz.width as f64 / sf - CLIP_W - 20.0;
        let y = p.y as f64 / sf + sz.height as f64 / sf - CLIP_H - 72.0;
        b = b.position(x, y);
    }
    let _ = b.build();
}

/// Watches the clipboard; when a downloadable link is copied, offers to download it.
async fn watch_clipboard(app: tauri::AppHandle, m: Manager) {
    let read = |app: &tauri::AppHandle| app.clipboard().read_text().ok();
    // Whatever is already on the clipboard at startup is not "new".
    let mut last = read(&app).unwrap_or_default();
    let mut prompted: std::collections::VecDeque<String> = Default::default();
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        if !m.settings().clipboard_monitor {
            continue;
        }
        let Some(text) = read(&app) else { continue };
        if text == last {
            continue;
        }
        last = text.clone();
        let t = text.trim();
        // One link only; multi-line text is a list, not a "copied link".
        if t.is_empty()
            || t.len() > 2048
            || t.contains(char::is_whitespace)
            || !(t.starts_with("http://") || t.starts_with("https://"))
        {
            continue;
        }
        if prompted.iter().any(|u| u == t) {
            continue;
        }
        if let Some(info) = m.inspect(t).await {
            prompted.push_back(t.to_string());
            if prompted.len() > 50 {
                prompted.pop_front();
            }
            show_clip_popup(&app, &info);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Fetchy used to be called A-IDM; carry its list, projects, settings and pairing token over once.
fn migrate_from_aidm(new_dir: &std::path::Path) {
    if new_dir.join("settings.json").exists() || new_dir.join("downloads.json").exists() {
        return;
    }
    let Some(old_dir) = new_dir.parent().map(|p| p.join("dev.aidm.app")) else {
        return;
    };
    if !old_dir.is_dir() {
        return;
    }
    let _ = std::fs::create_dir_all(new_dir);
    for name in ["downloads.json", "projects.json", "api_token"] {
        let _ = std::fs::copy(old_dir.join(name), new_dir.join(name));
    }
    if let Ok(s) = std::fs::read_to_string(old_dir.join("settings.json")) {
        let _ = std::fs::write(new_dir.join("settings.json"), s.replace("A-IDM", "Fetchy"));
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            migrate_from_aidm(&dir);
            let manager = tauri::async_runtime::block_on(async { Manager::new(dir) })?;
            let mut rx = manager.subscribe();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use tauri_plugin_notification::NotificationExt;
                use tokio::sync::broadcast::error::RecvError;
                let mut last: std::collections::HashMap<String, fetchy_core::Status> =
                    Default::default();
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            if let fetchy_core::Event::Updated(info) = &ev {
                                let before = last.insert(info.id.clone(), info.status);
                                let notify = handle
                                    .try_state::<Manager>()
                                    .is_none_or(|m| m.settings().notifications);
                                if notify && before.is_some() && before != Some(info.status) {
                                    let (title, body) = match info.status {
                                        fetchy_core::Status::Complete => {
                                            ("Download complete", Some(info.filename.clone()))
                                        }
                                        fetchy_core::Status::Error
                                            if !info
                                                .error
                                                .as_deref()
                                                .unwrap_or("")
                                                .contains("retrying") =>
                                        {
                                            (
                                                "Download failed",
                                                Some(format!(
                                                    "{} - {}",
                                                    info.filename,
                                                    info.error.clone().unwrap_or_default()
                                                )),
                                            )
                                        }
                                        _ => ("", None),
                                    };
                                    if let Some(body) = body {
                                        let _ = handle
                                            .notification()
                                            .builder()
                                            .title(title)
                                            .body(body)
                                            .show();
                                    }
                                }
                            }
                            let _ = handle.emit("fetchy://event", ev);
                        }
                        Err(RecvError::Lagged(_)) => {
                            // UI missed updates; tell it to re-sync from scratch.
                            let _ = handle.emit("fetchy://resync", ());
                        }
                        Err(RecvError::Closed) => break,
                    }
                }
            });
            let token = fetchy_core::server::load_or_create_token(&app.path().app_data_dir()?);
            let port = manager.settings().api_port;
            let api = if port == 0 {
                ApiInfo { port: 0, token }
            } else {
                let m2 = manager.clone();
                let t2 = token.clone();
                let bound = tauri::async_runtime::block_on(async move {
                    fetchy_core::server::start(m2, port, t2).await
                });
                ApiInfo {
                    port: bound.unwrap_or(0),
                    token,
                }
            };
            app.manage(api);
            let clip_app = app.handle().clone();
            let clip_mgr = manager.clone();
            tauri::async_runtime::spawn(watch_clipboard(clip_app, clip_mgr));
            app.manage(manager);
            setup_tray(app)?;
            // Started by the system at sign-in: stay in the tray.
            if std::env::args().any(|a| a == "--minimized")
                && let Some(w) = app.get_webview_window("main")
            {
                let _ = w.hide();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_downloads,
            get_details,
            add_download,
            stream_variants,
            add_stream,
            pause,
            resume,
            pause_all,
            resume_all,
            remove,
            redownload,
            open_file,
            open_folder,
            get_settings,
            set_settings,
            grab_site,
            grab_add,
            open_dir,
            get_stats,
            verify_checksum,
            open_with,
            window_action,
            set_description,
            check_update,
            sync_now,
            start_queue,
            stop_queue,
            move_in_queue,
            list_grab_projects,
            save_grab_project,
            delete_grab_project,
            run_grab_project,
            power_action,
            api_info,
            move_to_queue,
            export_links,
            import_links,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fetchy");
}
