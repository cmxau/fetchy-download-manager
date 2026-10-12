const api = globalThis.browser ?? globalThis.chrome;
const $ = (id) => document.getElementById(id);
const DEFAULTS = { port: 6800, token: "", capture: true, minSizeKb: 0, ignoreExt: "" };

async function check(c) {
  const el = $("status");
  for (let p = c.port; p < c.port + 10; p++) {
    try {
      const r = await fetch(`http://127.0.0.1:${p}/ping`, { signal: AbortSignal.timeout(800) });
      if (!r.ok || (await r.json()).app !== "Fetchy") continue;
      if (!c.token) return setStatus(el, "warn", `Fetchy found on port ${p} — paste the token to pair.`);
      const s = await fetch(`http://127.0.0.1:${p}/status`, { headers: { "x-fetchy-token": c.token } });
      if (s.ok) {
        const j = await s.json();
        return setStatus(el, "ok", `Connected · ${j.active} active, ${j.downloads} total`);
      }
      return setStatus(el, "bad", "Wrong token — copy it again from Fetchy Settings.");
    } catch { /* next */ }
  }
  setStatus(el, "bad", "Fetchy is not running. Downloads stay in the browser.");
}

function setStatus(el, cls, text) {
  el.className = `status ${cls}`;
  el.textContent = text;
}

(async () => {
  const c = { ...DEFAULTS, ...(await api.storage.local.get(null)) };
  $("capture").checked = c.capture;
  $("token").value = c.token;
  $("port").value = c.port;
  $("minSizeKb").value = c.minSizeKb;
  $("ignoreExt").value = c.ignoreExt;
  check(c);

  $("capture").addEventListener("change", () => api.storage.local.set({ capture: $("capture").checked }));
  $("save").addEventListener("click", async () => {
    const next = {
      capture: $("capture").checked,
      token: $("token").value.trim(),
      port: Math.max(1, +$("port").value || 6800),
      minSizeKb: Math.max(0, +$("minSizeKb").value || 0),
      ignoreExt: $("ignoreExt").value.trim(),
    };
    await api.storage.local.set(next);
    setStatus($("status"), "warn", "Testing…");
    check({ ...DEFAULTS, ...next });
  });
})();

// Streams detected on the active tab (?tab=<id> overrides, handy for testing).
(async () => {
  const override = new URLSearchParams(location.search).get("tab");
  const tabId = override ? +override : (await api.tabs.query({ active: true, currentWindow: true }))[0]?.id;
  if (tabId == null) return;
  const store = api.storage.session ?? api.storage.local;
  const key = `streams:${tabId}`;
  const list = (await store.get(key))[key] || [];
  if (!list.length) return;
  $("streams").hidden = false;
  for (const s of list) {
    const row = document.createElement("div");
    row.className = "stream";
    const label = document.createElement("span");
    label.title = s.url;
    label.textContent = (s.master ? "All qualities · " : "Single quality · ") + (s.title || new URL(s.url).pathname.split("/").slice(-2).join("/"));
    const btn = document.createElement("button");
    btn.textContent = "Download";
    btn.addEventListener("click", async () => {
      btn.disabled = true;
      const r = await api.runtime.sendMessage({ type: "grab", url: s.url, page: s.page, title: s.title });
      btn.textContent = r?.ok ? "Sent ✓" : "Failed";
      btn.disabled = !!r?.ok;
    });
    row.append(label, btn);
    $("streamList").append(row);
  }
})();

// Download All: collect file links from the page, let the user pick, hand them to Fetchy.
const KINDS = {
  video: "mp4 mkv avi mov webm flv wmv m4v ts m3u8",
  audio: "mp3 flac wav aac ogg m4a opus wma",
  image: "png jpg jpeg gif webp svg bmp avif heic ico",
  doc: "pdf doc docx xls xlsx ppt pptx txt epub odt csv",
  archive: "zip rar 7z tar gz bz2 xz zst tgz iso",
  program: "exe msi dmg pkg deb rpm appimage apk",
};
const PAGE_EXTS = new Set(["", "html", "htm", "php", "asp", "aspx", "jsp", "xhtml", "shtml"]);
let foundLinks = [];

$("scan").addEventListener("click", async () => {
  const override = new URLSearchParams(location.search).get("tab");
  const tab = override ? { id: +override } : (await api.tabs.query({ active: true, currentWindow: true }))[0];
  if (!tab?.id) return;
  let urls = [];
  try {
    const res = await api.scripting.executeScript({
      target: { tabId: tab.id },
      func: () => {
        const out = new Set();
        document.querySelectorAll("a[href], img[src], video[src], audio[src], source[src], link[rel=stylesheet]").forEach((e) => {
          const u = e.href || e.src;
          if (u && /^https?:/.test(u)) out.add(u);
        });
        return { page: location.href, urls: [...out] };
      },
    });
    urls = res[0]?.result?.urls || [];
    var page = res[0]?.result?.page;
  } catch (e) {
    $("count").textContent = "Cannot read this page";
    $("found").hidden = false;
    return;
  }
  const kind = $("kind").value;
  const allowed = kind === "all" ? null : new Set(KINDS[kind].split(" "));
  foundLinks = urls
    .map((u) => {
      const path = new URL(u).pathname;
      const name = decodeURIComponent(path.split("/").pop() || "");
      const ext = name.includes(".") ? name.split(".").pop().toLowerCase() : "";
      return { url: u, name, ext, page };
    })
    .filter((f) => (allowed ? allowed.has(f.ext) : !PAGE_EXTS.has(f.ext) && f.ext !== "css"));
  const list = $("foundList");
  list.textContent = "";
  for (const f of foundLinks) {
    const l = document.createElement("label");
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = true;
    cb.dataset.url = f.url;
    const s = document.createElement("span");
    s.textContent = f.name || f.url;
    s.title = f.url;
    l.append(cb, s);
    list.append(l);
  }
  $("count").textContent = foundLinks.length + " file" + (foundLinks.length === 1 ? "" : "s") + " found";
  $("sendAll").hidden = foundLinks.length === 0;
  $("found").hidden = false;
});
const setAll = (v) => document.querySelectorAll("#foundList input").forEach((c) => (c.checked = v));
$("sall").addEventListener("click", (e) => { e.preventDefault(); setAll(true); });
$("snone").addEventListener("click", (e) => { e.preventDefault(); setAll(false); });
$("sendAll").addEventListener("click", async () => {
  const urls = [...document.querySelectorAll("#foundList input:checked")].map((c) => c.dataset.url);
  if (!urls.length) return;
  $("sendAll").disabled = true;
  const r = await api.runtime.sendMessage({ type: "grabMany", urls, page: foundLinks[0]?.page });
  $("sendAll").textContent = r?.ok ? "Sent " + r.sent + " ✓" : "Failed - is Fetchy running and paired?";
  $("sendAll").disabled = false;
});
