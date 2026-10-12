// Fetchy browser integration (Manifest V3, Chrome/Edge/Brave/Firefox).
// Talks to the desktop app over http://127.0.0.1:<port> using a pairing token.

const api = globalThis.browser ?? globalThis.chrome;
const DEFAULTS = { port: 6800, token: "", capture: true, minSizeKb: 0, ignoreExt: "" };
const PORT_SPAN = 10; // the app binds the first free port in port..port+9

async function cfg() {
  return { ...DEFAULTS, ...(await api.storage.local.get(null)) };
}

async function findApp(c) {
  for (let p = c.port; p < c.port + PORT_SPAN; p++) {
    try {
      const r = await fetch(`http://127.0.0.1:${p}/ping`, { signal: AbortSignal.timeout(800) });
      if (r.ok && (await r.json()).app === "Fetchy") return p;
    } catch { /* try next port */ }
  }
  return null;
}

async function cookieHeader(url) {
  try {
    const list = await api.cookies.getAll({ url });
    return list.map((x) => `${x.name}=${x.value}`).join("; ");
  } catch {
    return "";
  }
}

function flash(text, color) {
  api.action.setBadgeBackgroundColor({ color });
  api.action.setBadgeText({ text });
  setTimeout(() => api.action.setBadgeText({ text: "" }), 2500);
}

/** Returns true if Fetchy accepted the download. Never throws. */
async function sendToApp({ url, referer, filename }) {
  const c = await cfg();
  if (!c.token) {
    flash("?", "#e0a030");
    return false;
  }
  const port = await findApp(c);
  if (port == null) {
    flash("off", "#888");
    return false; // app not running: let the browser keep the download
  }
  try {
    const r = await fetch(`http://127.0.0.1:${port}/add`, {
      method: "POST",
      headers: { "content-type": "application/json", "x-fetchy-token": c.token },
      body: JSON.stringify({
        url,
        referer: referer || undefined,
        filename: filename || undefined,
        cookie: (await cookieHeader(url)) || undefined,
        user_agent: navigator.userAgent,
      }),
    });
    if (r.ok) {
      flash("✓", "#2dd4bf");
      return true;
    }
    flash(r.status === 401 ? "key" : "!", "#f2556b");
  } catch {
    flash("!", "#f2556b");
  }
  return false;
}

const baseName = (p) => (p || "").split(/[\\/]/).pop();

// 1. Intercept normal browser downloads.
api.downloads.onCreated.addListener(async (item) => {
  const c = await cfg();
  if (!c.capture || !c.token) return;
  const url = item.finalUrl || item.url;
  if (!/^https?:/i.test(url)) return; // blob:, data:, file: stay in the browser
  if (item.byExtensionId) return; // started by an extension (incl. us); don't loop
  if (c.minSizeKb > 0 && item.fileSize > 0 && item.fileSize < c.minSizeKb * 1024) return;
  const ext = (baseName(item.filename).split(".").pop() || "").toLowerCase();
  const skip = c.ignoreExt.split(/[\s,;]+/).filter(Boolean).map((e) => e.replace(/^\./, "").toLowerCase());
  if (ext && skip.includes(ext)) return;

  // Hand off first; only cancel the browser's copy once Fetchy has accepted it.
  if (await sendToApp({ url, referer: item.referrer, filename: baseName(item.filename) })) {
    try {
      await api.downloads.cancel(item.id);
      await api.downloads.erase({ id: item.id });
    } catch { /* already gone */ }
  }
});

// 2. Right-click → "Download with Fetchy".
api.runtime.onInstalled.addListener(() => {
  api.contextMenus.create({
    id: "fetchy-link",
    title: "Download with Fetchy",
    contexts: ["link", "video", "audio", "image"],
  });
});

api.contextMenus.onClicked.addListener((info) => {
  if (info.menuItemId !== "fetchy-link") return;
  const url = info.linkUrl || info.srcUrl;
  if (url && /^https?:/i.test(url)) void sendToApp({ url, referer: info.pageUrl });
});

// 3. Detect HLS streams (.m3u8) a page loads, so the popup can offer them.
const streamKey = (tabId) => `streams:${tabId}`;
const sessionStore = api.storage.session ?? api.storage.local;

// Requests arrive in bursts; handle them one at a time so list updates never clobber each other.
let streamQueue = Promise.resolve();
api.webRequest.onBeforeRequest.addListener(
  (d) => {
    if (d.tabId < 0 || !/\.m3u8$/i.test(new URL(d.url).pathname)) return;
    streamQueue = streamQueue.then(() => recordStream(d)).catch(() => {});
  },
  { urls: ["<all_urls>"] },
);

async function recordStream(d) {
  const key = streamKey(d.tabId);
  const list = (await sessionStore.get(key))[key] || [];
  if (list.some((s) => s.url === d.url)) return;
  const tab = await api.tabs.get(d.tabId).catch(() => null);
  // A master playlist (lists all qualities) is the best thing to hand to Fetchy.
  const master = await fetch(d.url).then((r) => r.text()).then((t) => t.includes("#EXT-X-STREAM-INF")).catch(() => false);
  list.unshift({ url: d.url, title: tab?.title || "", page: tab?.url || d.documentUrl || "", master });
  list.sort((x, y) => Number(y.master) - Number(x.master));
  await sessionStore.set({ [key]: list.slice(0, 10) });
  api.action.setBadgeBackgroundColor({ tabId: d.tabId, color: "#8b5cf6" });
  api.action.setBadgeText({ tabId: d.tabId, text: String(Math.min(list.length, 9)) });
}

// New page in a tab: forget its old streams.
api.tabs.onUpdated.addListener((tabId, change) => {
  if (change.url) {
    sessionStore.remove(streamKey(tabId));
    api.action.setBadgeText({ tabId, text: "" });
  }
});
api.tabs.onRemoved.addListener((tabId) => sessionStore.remove(streamKey(tabId)));

// Popup asks us to hand a stream to the app.
api.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg?.type === "grabMany") {
    (async () => {
      let sent = 0;
      for (const url of msg.urls) if (await sendToApp({ url, referer: msg.page })) sent++;
      reply({ ok: sent > 0, sent });
    })();
    return true;
  }
  if (msg?.type !== "grab") return;
  sendToApp({ url: msg.url, referer: msg.page, filename: msg.title }).then((ok) => reply({ ok }));
  return true; // async reply
});
