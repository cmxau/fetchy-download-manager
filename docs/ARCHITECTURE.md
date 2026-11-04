# Architecture

```
extension/ (MV3)  --HTTP, 127.0.0.1, token-->  fetchy-core  <--commands/events--  app/ (Tauri 2 + React)
                                                  |
                                                  +-- fetchy-cli (same engine, terminal)
```

## `crates/fetchy-core` (Rust engine)

| Module | Job |
|---|---|
| `engine.rs` | One download: ranged requests, segments, resume, restart for servers that cannot resume |
| `manager/mod.rs` | All downloads: adding, pausing, retries, stats, post-download hooks, settings |
| `manager/queues.rs` | Queue schedules, start/stop/reorder, synchronization, per-queue limiters |
| `manager/grabber.rs` | Grabber projects and crawling |
| `manager/streams.rs` | HLS/DASH quality listing, adding streams, merging audio with video |
| `manager/driver.rs` | The per-download loops and the once-a-tick housekeeping |
| `limiter.rs` | Speed and hourly-quota limits, per queue chained to the global one |
| `hls.rs`, `dash.rs` | Playlist/manifest parsing, quality choice, segment download, AES-128, live HLS recording |
| `ftp.rs` | FTP downloads |
| `grab.rs` | Site crawler for the grabber |
| `extras.rs` | Checksums, rule matching, post-download command, Defender scan, ffmpeg lookup and merge |
| `server.rs` | Local HTTP API used by the browser extension |
| `update.rs` | Release check against GitHub |
| `types.rs` | Settings, queues and the data shapes shared with the UI |

Key ideas:
- **Dynamic segmentation:** a file is split into byte ranges, one connection each. When one finishes, it takes half of the largest remaining range, so no connection sits idle.
- **Resume:** progress is persisted; on restart the engine validates the remote file before continuing.
- **Queues:** each queue has its own concurrency, schedule and limiter. The Manager starts the next queued job when a slot frees.
- **Streams:** HLS and DASH are reduced to the same list of media segments and downloaded by one code path.
- **State** is JSON in the app data folder: downloads, settings, daily statistics, grabber projects.

## `app/` (desktop app)

- `src-tauri`: Tauri 2 shell. Registers commands (add, pause, queues, settings, grabber, update...), forwards engine events to the UI,
  and handles the tray, notifications, autostart, window controls and the signed updater.
- `src`: React 19 + TypeScript + Vite. `App.tsx` holds state; `components/` has Overview, Sidebar, DownloadTable, SettingsPage,
  GrabberWizard, BatchDialog, dialogs and the title bar. `api.ts` wraps the Tauri commands; `mockApi.ts` lets `npm run dev`
  run the interface in a plain browser with fake data. `styles.css` is the single stylesheet for both the dark and light themes.
- **Platform look:** `main.tsx` adds a `mac` class to `<html>` when the user agent contains "Mac"; `styles.css` uses it for macOS-style window corners (16px radius) and left-side traffic-light buttons (close, minimize, full screen). Windows and Linux keep the default right-side controls. The title bar's green button toggles full screen on macOS (and on any platform with Alt-click); the Tauri capability file allows `set-fullscreen` and `is-fullscreen` for this.

## `extension/`

Manifest V3, Chromium and Firefox. `background.js` captures downloads (with cookies and referer), the context menu and
stream detection; `popup.*` is the pairing screen and "Download all links". It only ever talks to `127.0.0.1`.

## Tests

`crates/fetchy-core/tests/e2e.rs` starts small local servers (including a fake proxy, FTP, HLS/DASH sources and a fake ffmpeg)
and runs real downloads end to end. Unit tests live next to the code.
