# Security

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private reporting: the repository's **Security** tab →
**Report a vulnerability**. Include the version, steps to reproduce and the impact.

## What Fetchy exposes

- A local HTTP API for the browser extension, bound to `127.0.0.1` only (port 6800 or the next free one).
- Every state-changing call needs the secret pairing token, and requests from web-page origins are rejected,
  so a website cannot drive Fetchy.
- Updates are verified against the public key built into the app; unsigned or mismatched updates are refused.
- Site logins, cookies and proxy passwords you enter are stored in Fetchy's settings file on your computer. They are sent only to the site or proxy they are for.
- Fetchy has no analytics or telemetry. Its only own network request is the update check against GitHub releases.

## Keys

The updater private key is never stored in the repository; it lives in the `TAURI_SIGNING_PRIVATE_KEY` secret
(see [RELEASING.md](RELEASING.md)). Anyone who holds it can ship updates to every installed copy, so protect it.
