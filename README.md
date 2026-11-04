# Fetchy Download Manager

<p align="center"><img src="app/src-tauri/icons/fetchy.svg" width="120" alt="Fetchy"></p>

Free, open-source download manager for Windows, macOS and Linux. Fetchy splits every file across several connections, re-balances them while it downloads, and resumes anything that gets interrupted.

## Features

**Speed and reliability**
- Multi-connection downloads, up to 32 per file (one TCP connection per byte range).
- Dynamic segmentation: a finished connection splits the largest remaining range, so none sits idle.
- Resume after pause, crash or restart; changed remote files are detected. Automatic retry with growing delays.
- HTTP, HTTPS and FTP. HTTP/SOCKS5 proxy. Per-site logins and cookies.
- Speed limit, plus per-queue speed limits and hourly quotas.

**Video**
- HLS (`.m3u8`) and DASH (`.mpd`): quality picker, AES-128 decryption, separate audio tracks merged with ffmpeg.
- Records live HLS streams until they end (or until you press Stop).

**Organisation**
- Sorted into `Music / Videos / Documents / Compressed / Programs / APKs / Images` folders, plus your own rules by file type, site or name pattern.
- Queues with schedules (start/stop times, days, repeat, retries, actions when done) and a synchronization queue that re-downloads changed files.
- Site Grabber: a 4-step wizard (templates, save location, link levels, file filters), offline copies with rewritten links, saved projects that can re-run on a timer.
- Batch download (`img*.jpg`, numbers or letters), drag links or list files onto the window, clipboard watcher.

**Everyday**
- Overview home page: four summary cards (downloading now, completed, downloaded today, needs attention) and recent downloads as a list or a grid.
- Detailed view per download (live speed graph, segment map, per-connection table).
- Desktop notifications, optional sound, start with Windows, system tray (minimize / close to tray).
- Verify a file against a pasted MD5 / SHA-1 / SHA-256 / SHA-512; optional Windows Defender scan and a command to run after each download.
- Dark (pure black) and light themes, minimal UI.
- Update check every launch, daily, weekly, monthly or never, with optional automatic install (Settings → Updates & About).
- Choose what happens when all downloads finish (nothing, exit, sleep, hibernate, shut down) from a drop-down in Settings.

## Install

Download the installer for your system from the [latest release](https://github.com/cmxau/fetchy-download-manager/releases/latest). Releases are built for Windows, macOS (Apple silicon and Intel) and Linux (Ubuntu 22.04 build).

On macOS the window uses native-style rounded corners, with the close / minimize / full screen buttons on the left, and the app icon follows the macOS icon grid. Windows and Linux keep the right-hand window controls.

Updates are offered inside the app (Settings → Updates & About), where you can pick how often Fetchy checks and whether it installs them by itself.

Merging a video's separate audio track needs [ffmpeg](https://ffmpeg.org/download.html); Fetchy finds it on your PATH or next to the app, or you can pick it in Settings.

## Browser extension

Chrome, Edge, Brave, Opera and Firefox. Until it is in the stores, install it by hand:

1. Download `fetchy-chromium.zip` (or `fetchy-firefox.zip`) from the release, unzip it, and load it
   (`chrome://extensions` → Developer mode → Load unpacked; Firefox: `about:debugging` → Load Temporary Add-on).
   From a source checkout, load the `extension/` folder.
2. In Fetchy: Settings → General → Browser extension → copy the pairing token, paste it into the extension popup.
3. Browser downloads, right-click → *Download with Fetchy*, and "Download all links on this page" now go to Fetchy.

Fetchy listens on `127.0.0.1` only (port 6800+). Every state-changing call needs the secret token, and requests from web-page origins are rejected.

## Develop

```
cd app && npm install
npm run tauri dev        # full app
npm run dev              # interface only, with a mock engine
cargo test --release -p fetchy-core -- --test-threads=1
cargo run --release -p fetchy-core --bin fetchy-cli -- <url> -n 16
npm run tauri build      # installers for the current OS
```

Layout: `crates/fetchy-core` (engine, CLI, end-to-end tests), `app/` (Tauri 2 + React), `extension/` (Manifest V3), `scripts/`, `docs/`.

Building from source needs [Rust](https://rustup.rs) and Node.js 22:
- **Windows**: the WebView2 runtime (preinstalled on Windows 11) and the MSVC build tools.
- **macOS**: Xcode Command Line Tools (`xcode-select --install`).
- **Linux**: `sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev libgtk-3-dev libayatana-appindicator3-dev` (Debian/Ubuntu). Other distros: <https://tauri.app/start/prerequisites>.

Then `cd app && npm i && npm run tauri dev`.

Documentation: [installation](docs/INSTALLATION.md), [user guide](docs/USER-GUIDE.md), [troubleshooting](docs/TROUBLESHOOTING.md), [architecture](docs/ARCHITECTURE.md), [contributing](docs/CONTRIBUTING.md), [security](docs/SECURITY.md).
Releasing and publishing the extension: [docs/RELEASING.md](docs/RELEASING.md), [extension/STORE.md](extension/STORE.md).

## Not supported yet

Torrents/magnet links, live DASH, DRM-protected streams (Widevine, SAMPLE-AES), FTPS, MMS.

## License

MIT — see [LICENSE](LICENSE).
