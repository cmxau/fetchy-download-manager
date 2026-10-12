# Installation

## Windows

1. Download `Fetchy_<version>_x64-setup.exe` (or the `.msi`) from the [latest release](https://github.com/cmxau/fetchy-download-manager/releases/latest).
2. Run it. The installers are not code-signed yet, so Windows SmartScreen may say "unknown publisher": choose **More info → Run anyway**.
3. Start Fetchy from the Start menu.

## macOS

1. Download the `.dmg` that matches your Mac: `aarch64` for Apple silicon, `x64` for Intel.
2. Drag Fetchy to Applications. The app is not notarized yet, so the first time, right-click it and choose **Open**.

## Linux

Download one of: `.AppImage` (any distribution; `chmod +x` it and run), `.deb` (Debian, Ubuntu) or `.rpm` (Fedora, openSUSE).
The `.deb` and `.rpm` pull in WebKitGTK (`webkit2gtk-4.1`) automatically.

## Updating

Settings → Updates & About. Choose how often Fetchy checks (every launch, daily, weekly, monthly, never) and whether it
installs updates automatically. **Check for updates** checks right now. Updates are signed; Fetchy refuses an update that
does not match its built-in public key.

## Browser extension

The extension sends your browser's downloads, link right-clicks and "Download all links on this page" to Fetchy.

1. Install it: from your browser's store once listed, or by hand: download `fetchy-chromium.zip` / `fetchy-firefox.zip`
   from the release and unzip it.
   - Chrome, Edge, Brave, Opera: `chrome://extensions` (or `edge://extensions`) → turn on **Developer mode** → **Load unpacked** → pick the unzipped folder.
   - Firefox: `about:debugging` → **This Firefox** → **Load Temporary Add-on** → pick `manifest.json` (temporary until the store version exists).
2. In Fetchy open Settings → General → Browser extension and copy the **pairing token**.
3. Open the extension's popup and paste the token. The popup should say **Connected**.

Fetchy must be running for the extension to hand over downloads; if it is not, the browser keeps the download itself.

## ffmpeg (optional, for video)

HLS and DASH videos often have the picture and the sound in separate streams. Fetchy joins them with [ffmpeg](https://ffmpeg.org/download.html).
It looks for `ffmpeg` in: the path set in Settings, Fetchy's data folder, next to the app, and your `PATH`.
Install it any of these ways:

- Windows: `winget install Gyan.FFmpeg`
- macOS: `brew install ffmpeg`
- Linux: `sudo apt install ffmpeg` (or your package manager)

Without ffmpeg, single-stream videos still download; only the audio/video merge is skipped.

## Uninstalling

Use your system's uninstaller (Windows: Settings → Apps). Your downloads are never deleted. Settings and the download
list live in Fetchy's app data folder (Windows: `%APPDATA%\dev.fetchy.app`); delete that folder for a completely clean removal.

## Build from source

See [CONTRIBUTING.md](CONTRIBUTING.md).
