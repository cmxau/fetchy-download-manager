// Builds store-ready zips of the browser extension:
//   dist-extension/fetchy-chromium.zip   Chrome, Edge, Brave, Opera (service worker only)
//   dist-extension/fetchy-firefox.zip    Firefox (event page + gecko id)
// Usage: node scripts/pack-extension.cjs
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const src = path.join(__dirname, "..", "extension");
const out = path.join(__dirname, "..", "dist-extension");
const base = JSON.parse(fs.readFileSync(path.join(src, "manifest.json"), "utf8"));

fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });

function stage(name, manifest) {
  const dir = path.join(out, name);
  fs.cpSync(src, dir, { recursive: true, filter: (p) => !/STORE\.md$/.test(p) });
  fs.writeFileSync(path.join(dir, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  return dir;
}

function zip(dir, file) {
  const target = path.join(out, file);
  if (process.platform === "win32") {
    execSync(`powershell -NoProfile -Command "Compress-Archive -Path '${dir}\\*' -DestinationPath '${target}' -Force"`, { stdio: "inherit" });
  } else {
    execSync(`cd "${dir}" && zip -r -q "${target}" .`, { stdio: "inherit" });
  }
  console.log("wrote", target);
}

const chromium = JSON.parse(JSON.stringify(base));
chromium.background = { service_worker: base.background.service_worker };
delete chromium.browser_specific_settings;
zip(stage("chromium", chromium), "fetchy-chromium.zip");

const firefox = JSON.parse(JSON.stringify(base));
firefox.background = { scripts: base.background.scripts };
zip(stage("firefox", firefox), "fetchy-firefox.zip");
