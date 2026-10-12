import type { Category, DownloadInfo } from "./types";

export function formatBytes(n: number | null | undefined): string {
  if (n == null) return "—";
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? v.toFixed(0) : v.toFixed(2)} ${units[i]}`;
}

export function formatSpeed(bps: number): string {
  return `${(bps / 1024 / 1024).toFixed(2)}`;
}

export function formatEta(s: number | null, active: boolean): string {
  if (!active || s == null) return active ? "…" : "0 Sec";
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (d) return `${d} Day ${h} Hr`;
  if (h) return `${h} Hr ${m} Min`;
  if (m) return `${m} Min ${sec} Sec`;
  return `${sec} Sec`;
}

export function formatDate(ts: number | null): string {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return "Today";
  const p = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}/${p(d.getMonth() + 1)}/${p(d.getDate())}`;
}

export function percent(i: DownloadInfo): number {
  if (i.status === "complete") return 100;
  if (!i.size) return 0;
  return Math.min(100, Math.floor((i.downloaded / i.size) * 100));
}

export const categoryLabel: Record<Category, string> = {
  music: "Music",
  compressed: "Archive",
  video: "Video",
  program: "Program",
  document: "Document",
  apk: "App",
  image: "Image",
  other: "File",
};

/** A plain-language reason and a next step for an error message from the engine. */
export function explainError(err: string | null | undefined): string | null {
  if (!err) return null;
  const e = err.toLowerCase();
  if (e.includes("http 401")) return "The site wants a login. Add one under Settings → Network → Logins and cookies per site, or send the download from your browser with the extension.";
  if (e.includes("http 403")) return "The site refused the request. It often needs the page you came from or your browser's cookies: send it from your browser with the extension, or add the site's cookie in Settings.";
  if (e.includes("http 404") || e.includes("http 410")) return "The file is gone or the link has expired. Get a fresh link from the site.";
  if (e.includes("http 416")) return "The server could not continue the partial file. Use Redownload to start over.";
  if (e.includes("http 429")) return "The site is limiting you. Lower the connections per download in Settings and try again later.";
  if (/http 5\d\d/.test(e)) return "The server had a problem. Fetchy tries again by itself; if it keeps failing, try later.";
  if (e.includes("timed out") || e.includes("timeout")) return "The connection timed out. Check your network, or try again later.";
  if (e.includes("different range") || e.includes("changed")) return "The file changed on the server while downloading. Use Redownload to get the new version.";
  if (e.includes("space") || e.includes("allocate")) return "Not enough free disk space in the download folder.";
  if (e.includes("login failed")) return "The FTP login was refused. Check the user name and password in the link.";
  if (e.includes("live stream")) return "This is a live stream; it can only be recorded while it is running.";
  if (e.includes("connection") || e.includes("dns") || e.includes("error sending request")) return "Could not reach the server. Check your internet connection and any proxy in Settings.";
  return null;
}
