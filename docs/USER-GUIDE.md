# User guide

## The window

- **Overview** (home): four cards (downloading now, completed, downloaded today, needs attention) and your recent downloads.
  Switch between list and grid with the buttons at the top right of the list. Click a card to jump to the matching list.
- **Sidebar:** All Downloads with categories (Applications, Compressed, Documents, Music, Videos), Unfinished, Finished,
  Grabber Projects and Queues. Click one to filter the list.
- **Top bar:** search, light/dark switch and Settings. Press `Ctrl+,` for Settings and `Esc` to go back.
- **Bottom bar:** today's data usage and the live speed graph.

## Adding downloads

- Paste a link into **Add Url** and press Enter. Several links (one per line) work too.
- Drag a link, or a `.txt` list of links, onto the window.
- With the browser extension, downloads and right-click → **Download with Fetchy** arrive automatically.
- **Batch** creates many addresses from a pattern such as `https://site/img*.jpg` with a number or letter range.
- Copy a link and Fetchy offers to download it (switch off in Settings → General).
- Supported: HTTP, HTTPS, FTP, HLS (`.m3u8`) and DASH (`.mpd`) video.

## Managing downloads

Select rows, then use **Resume All**, **Stop**, **Stop All** and **Delete** in the toolbar, or right-click for more:
show in folder, open with, properties (checksum check, description, referer), copy link.
Columns can be reordered and switched on or off.
Each download splits into up to 32 connections, rebalanced as it runs; paused or interrupted downloads resume where they stopped.
Failed downloads retry automatically with growing delays and show a plain-language reason.

## Video streams

Adding a `.m3u8` or `.mpd` link opens a **quality picker**. Pick a resolution and Fetchy downloads the segments,
decrypts AES-128 streams and merges separate audio with ffmpeg. Live HLS streams are recorded until they end or you press **Stop**.
DRM-protected streams (Widevine, SAMPLE-AES) and live DASH are not supported.

## Queues and scheduler (Settings → Queues & Scheduler)

- **Main download queue:** everything goes here by default. Set how many downloads run at once.
- **Synchronization queue:** periodically re-checks its files and downloads the ones that changed.
- **Download limits:** a queue with its own speed cap and hourly quota.
- Each queue can start at a time, repeat, run on chosen days, stop at a time, retry failed files, and finally open a file, exit Fetchy or turn the computer off.
- **Start now** and **Stop** run a queue manually; **Apply** saves your changes.

## Site grabber

**Grabber** opens a four-step wizard: choose a template and start address, choose where to save, choose how many link levels to follow,
choose which files to keep. Files are saved in a folder named after the project and listed under **Grabber Projects** in the sidebar.
Projects can make an offline copy with rewritten links and re-run on a timer.

## Settings (Settings → General)

- **Download folder, connections, speed limit, user agent.**
- **Sorting:** files go into `Music`, `Videos`, `Documents`, `Compressed`, `Programs`, `APKs`, `Images`; add rules by file type, site or file-name pattern.
- **Network:** HTTP or SOCKS5 proxy; per-site logins and cookies.
- **When finished:** desktop notification, sound, a Windows Defender scan, and a command to run after every download.
- **Auto retry** count.
- **System:** start with Windows (starts hidden in the tray), minimize or close to the tray.
- **When all downloads finish:** do nothing, exit, sleep, hibernate or shut down (applies once).
- **Browser extension:** pairing token and port.
- **Theme:** dark (pure black) or light.

## Updates & About

Check for updates now, pick how often Fetchy checks, and optionally let it install updates automatically.
Licence, terms and privacy are one click away.

## Keyboard shortcuts

| Key | Action |
|---|---|
| `Ctrl+V` | Focus the Add Url box |
| `Ctrl+,` | Open or close Settings |
| `Esc` | Back to Overview |
