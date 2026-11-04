# Releasing Fetchy

Everything is automated by `.github/workflows/release.yml`. A release gives you:

- Windows: NSIS installer (`.exe`) and MSI
- macOS: `.dmg` for Apple silicon and Intel
- Linux: `.AppImage`, `.deb`, `.rpm`
- `latest.json`, the manifest the in-app updater reads
- the browser extension zips (Chromium and Firefox)

## One-time setup

1. Create the repository `cmxau/fetchy-download-manager` and push this project to it.
2. Add the **updater signing key** as repository secrets (Settings → Secrets and variables → Actions):
   - `TAURI_SIGNING_PRIVATE_KEY` — the contents of the private key file
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — its password (empty for the key made during development)

   The matching public key is already in `app/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`).
   **If you lose the private key, installed copies can no longer be updated**, so keep a backup somewhere safe.
   To make a new pair: `cd app && npx tauri signer generate -w fetchy-updater.key`, then put the new public key in
   `tauri.conf.json` (users on old builds would have to reinstall once).
3. Settings → Actions → General → Workflow permissions: **Read and write**. (The release workflow declares `contents: write` itself; the CI workflow is limited to `contents: read`.)

## Every release

1. Bump the version in `app/src-tauri/tauri.conf.json`, `app/src-tauri/Cargo.toml`, `crates/fetchy-core/Cargo.toml`,
   `app/package.json` and `extension/manifest.json`.
2. Commit, then tag and push:

   ```
   git tag v0.2.0
   git push origin v0.2.0
   ```
3. Wait for the **Release** workflow. The installers appear under Releases. Installed copies find the new version
   (Settings → Updates & About → Check for updates, or automatically on the schedule each user chose: every launch, daily, weekly, monthly or never) and install it with one click, or without asking if the user turned on "Install updates automatically".

## Code signing (optional, recommended before wide distribution)

Unsigned installers work, but Windows SmartScreen and macOS Gatekeeper show warnings.

- Windows: buy a code-signing certificate and add `certificateThumbprint` under `bundle.windows` (or use Azure Trusted Signing).
- macOS: needs an Apple Developer account ($99/year); set `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` as secrets
  (see https://tauri.app/distribute/sign/macos/).

## Browser extension stores

See `extension/STORE.md`.
