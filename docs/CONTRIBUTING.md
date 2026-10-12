# Contributing

Thanks for helping. Bug reports, fixes, features and docs are all welcome.

## Before you start

- Search the [issues](https://github.com/cmxau/fetchy-download-manager/issues). For anything bigger than a small fix, open an issue first so we can agree on the approach.
- Be kind. Disagree with ideas, not people.

## Set up

You need [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 22+, and on Linux the
[Tauri prerequisites](https://tauri.app/start/prerequisites) (`webkit2gtk-4.1` etc.). On Windows you also need the
WebView2 runtime (already in Windows 11).

```
git clone https://github.com/cmxau/fetchy-download-manager
cd fetchy-download-manager/app
npm install
```

## Run and build

```
cd app
npm run tauri dev             # full app with hot reload
npm run dev                   # interface only, in a browser, with a mock engine
npm run tauri build           # installers for your OS (updater artifacts need TAURI_SIGNING_PRIVATE_KEY)
npx tauri build --no-bundle   # just the executable
```

Engine on its own:

```
cargo run --release -p fetchy-core --bin fetchy-cli -- <url> -n 16
```

Extension: load the `extension/` folder unpacked in your browser. Build store zips with `node scripts/pack-extension.cjs`.
Close a running Fetchy before rebuilding on Windows; the executable is locked while it runs.

## Test

```
cargo test --release -p fetchy-core -- --test-threads=1     # engine: unit and end-to-end
cd app && npx tsc --noEmit && npm run build                  # interface type-check and build
```

CI runs both on Windows, macOS and Linux for every pull request. Please add a test for engine changes; the end-to-end tests in
`crates/fetchy-core/tests/e2e.rs` show how to start a local test server.

## Code style

- Rust: `cargo fmt`, and no new `cargo clippy` warnings (the workspace is currently warning-free). Match the surrounding code; comments explain why, not what.
- TypeScript/React: strict TypeScript, function components, no new dependencies without a reason.
- Interface: keep it minimal and in the existing palette (green, blue, black, white) for both themes. Check light and dark.
- Styles: all CSS is in `app/src/styles.css`. Edit the existing rule instead of appending an override, and delete rules when you remove the interface that used them.
- Keep the app local-first: no analytics, and no network calls except to addresses the user enters (plus the update check).

## Pull requests

1. Branch from `main` (`fix/short-name` or `feat/short-name`).
2. Keep each pull request to one change, with a description of what and why, and screenshots for UI changes.
3. Make sure the tests and type-check above pass.
4. Update `docs/` and the README if behaviour changes.

By contributing you agree that your work is released under the project's [MIT licence](../LICENSE).

## Project map

See [ARCHITECTURE.md](ARCHITECTURE.md).
