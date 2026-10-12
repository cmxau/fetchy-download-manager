# Publishing the Fetchy browser extension

Build the store packages:

```
node scripts/pack-extension.cjs
```

This writes `dist-extension/fetchy-chromium.zip` (Chrome Web Store, Edge Add-ons, Opera, Brave) and
`dist-extension/fetchy-firefox.zip` (Firefox Add-ons). The release workflow attaches both to every GitHub Release.

I cannot submit these for you: each store needs your developer account.

| Store | Where | One-time fee |
|---|---|---|
| Chrome Web Store | https://chrome.google.com/webstore/devconsole | US$5 |
| Microsoft Edge Add-ons | https://partner.microsoft.com/dashboard/microsoftedge | free |
| Firefox Add-ons | https://addons.mozilla.org/developers/ | free |
| Opera add-ons | https://addons.opera.com/developer/ | free (accepts the Chromium zip) |

## Listing text

**Name:** Fetchy Integration

**Summary (132 characters max):** Send downloads and video streams from your browser to the Fetchy download manager.

**Description**

Fetchy is a free, open-source download manager that splits files across several connections so they download faster, and resumes them if anything goes wrong.

This extension connects your browser to the Fetchy app on your computer:

- Browser downloads are handed to Fetchy automatically, with your cookies and referer so protected links still work. If Fetchy is not running, the browser keeps the download.
- Right-click any link and choose "Download with Fetchy".
- "Download all links on this page" lists the files on a page (pictures, videos, documents, archives…) so you can pick and send them together.
- Video streams (HLS) that a page plays are listed in the popup.

Fetchy must be installed and running: https://github.com/cmxau/fetchy-download-manager

**Category:** Productivity (Chrome / Edge), Download Management (Firefox)

## Why each permission is needed (reviewers ask)

| Permission | Why |
|---|---|
| `downloads` | Notice a download the browser starts, cancel the browser's copy once Fetchy has accepted it. |
| `cookies` | Send the site's cookies for that URL to Fetchy so logged-in downloads work. |
| `contextMenus` | The "Download with Fetchy" link menu. |
| `webRequest` | Spot video playlists (.m3u8) a page loads, to list them in the popup. Request bodies are never read. |
| `tabs`, `scripting` | Read the links of the page you ask to scan with "Download all links". Only runs when you press the button. |
| `storage` | Remember the pairing token and the few settings in the popup. |
| Host access `<all_urls>` | Downloads and streams can come from any site. |

## Privacy statement (paste into the store form)

The extension sends data only to the Fetchy application on the same computer (127.0.0.1), and only the address, cookies and
referer of a download you start or let it capture. It has no analytics, no remote servers and no accounts. The pairing token you
enter in the popup is stored in the browser's local extension storage.

## Screenshots to prepare

1. The popup showing "Connected" and the stream list.
2. The "Download all links on this page" list.
3. The right-click menu with "Download with Fetchy".
