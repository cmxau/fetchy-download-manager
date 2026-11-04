# Changelog

## 0.2.0

- macOS and Linux support alongside Windows; CI tests all three and releases ship for each.
- macOS window chrome: native-style rounded corners and close / minimize / full screen buttons on the left.
- App icon follows the macOS icon grid.
- Security hardening: the API token file is created owner-only (0600) on Unix, the Host header check is stricter, and CI runs with a least-privilege token.

## 0.1.0

First release.

- Multi-connection downloads (up to 32 per file) with dynamic segmentation, resume and automatic retry.
- HTTP, HTTPS and FTP; HTTP/SOCKS5 proxy; per-site logins and cookies.
- HLS and DASH video with a quality picker, AES-128 decryption, audio/video merge (ffmpeg) and live HLS recording.
- Queues with schedules, speed limits and hourly quotas; synchronization queue.
- Site grabber with saved projects and offline copies; batch downloads.
- Overview page with summary cards and recent downloads as a list or grid.
- Auto-sort rules, checksum verification, desktop notifications, optional sound, Windows Defender scan and post-download command.
- System tray, start with Windows, dark and light themes.
- Signed in-app updates with a choice of check frequency and automatic install.
- Browser extension for Chromium browsers and Firefox.
