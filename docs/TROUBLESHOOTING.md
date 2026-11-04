# Troubleshooting

**The extension says "Not connected".**
Fetchy must be running. Copy the pairing token again (Settings → General → Browser extension) and paste it in the popup.
The extension talks to `127.0.0.1` on port 6800 or the next free port; the port is shown in the same settings section.
A firewall or security tool that blocks local connections will also break it.

**A download fails immediately.**
Open the download's Properties for the reason. Common ones: the link expired (get a fresh one from the site),
the site needs a login (add it under the per-site logins in Settings), or the site needs cookies
(use the browser extension so cookies travel with the link).

**"Server does not support resuming".**
Some servers cannot continue a partial download. Fetchy restarts that file from the beginning instead of failing.

**Speed is no faster than my browser.**
Some servers allow only one connection per file. Try more connections (Settings → General), check the
per-queue speed limit, and check that no proxy is throttling you.

**The video has no sound, or the merged file was not made.**
Install ffmpeg (see [INSTALLATION.md](INSTALLATION.md)) or set its path in Settings.

**A video link opens no quality picker or fails.**
The stream may be DRM-protected (not supported), a live DASH stream (not supported), or the playlist link may have expired.

**Windows SmartScreen or macOS blocks the installer.**
Installers are not code-signed yet. See [INSTALLATION.md](INSTALLATION.md) for the one-time bypass.

**On macOS the window buttons are on the left and the green one goes full screen.**
That is intended. Click it again to leave full screen.

**The app disappeared when I closed it.**
Minimize/close-to-tray is on. Click the tray icon to restore, or use its menu to quit. Change this in Settings → General.

**Updates fail to install.**
Use **Check for updates → Release page** to download the installer by hand. If you built Fetchy yourself, the update
signature will not match the official key.

**Where are my settings and download list?**
In the app data folder (Windows: `%APPDATA%\dev.fetchy.app`). Back it up to move to another computer.

Still stuck? Open an [issue](https://github.com/cmxau/fetchy-download-manager/issues) with your Fetchy version, system, and the error text.
