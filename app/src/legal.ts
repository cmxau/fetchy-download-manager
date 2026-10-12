export const LEGAL = {
  license: {
    title: "License",
    body: `MIT License

Copyright (c) 2026 cmxau

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.`,
  },
  terms: {
    title: "Terms of use",
    body: `Fetchy is free, open-source software provided as is, without warranty of any kind.

You are responsible for what you download. Only download content you have the right to access, and respect the copyright, licences and terms of the sites you use.

Fetchy does not bypass paywalls, DRM or other access controls, and it must not be used to overload or attack a server. Use a sensible number of connections.

The authors are not liable for any loss or damage resulting from the use of the software.`,
  },
  privacy: {
    title: "Privacy",
    body: `Fetchy keeps everything on your computer.

- No accounts, no analytics, no telemetry.
- Your download list, settings and projects are stored in Fetchy's data folder on this device.
- The browser extension talks only to Fetchy on this computer (127.0.0.1), protected by a pairing token. Cookies and the referer of a download are sent to Fetchy only so the file can be fetched the same way your browser would.
- The only requests Fetchy makes by itself are the downloads you start, links you ask it to inspect (for example a copied link), site-grabber crawls you start, and the optional weekly check for a new version on GitHub.

Delete the data folder to remove everything Fetchy has stored.`,
  },
} as const;
