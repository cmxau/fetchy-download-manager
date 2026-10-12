//! Site spider / grabber: crawls pages from a start URL, lists the files they link to,
//! and can write an offline copy of the pages with their links rewritten.
use crate::types::{Error, FoundFile, GrabOptions};
use crate::util::sanitize_filename;
use futures_util::future::join_all;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use url::Url;

const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
const WHOLE_SITE_LEVELS: u32 = 50;

pub struct CrawlResult {
    pub files: Vec<FoundFile>,
    /// Every page that was fetched (kept only when an offline copy was asked for).
    pub pages: Vec<(Url, String)>,
}

/// A URL-bearing attribute value inside an HTML document.
pub struct Attr {
    pub start: usize,
    pub end: usize,
    pub value: String,
    /// srcset holds several URLs; only its first one is used, and it is never rewritten.
    pub srcset: bool,
}

pub fn scan_attrs(html: &str) -> Vec<Attr> {
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    for attr in [
        "href=",
        "src=",
        "data-src=",
        "poster=",
        "data-href=",
        "srcset=",
    ] {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(attr) {
            let at = from + pos;
            from = at + attr.len();
            // must be a real attribute, not the tail of another word (e.g. "xhref=")
            if at > 0
                && !(bytes[at - 1] as char).is_ascii_whitespace()
                && bytes[at - 1] != b'"'
                && bytes[at - 1] != b'\''
            {
                continue;
            }
            let rest = &html[from..];
            let (offset, raw) = match rest.chars().next() {
                Some(q @ ('"' | '\'')) => (1, rest[1..].split(q).next().unwrap_or("")),
                Some(_) => (
                    0,
                    rest.split(|c: char| c.is_whitespace() || c == '>')
                        .next()
                        .unwrap_or(""),
                ),
                None => (0, ""),
            };
            let (start, end) = (from + offset, from + offset + raw.len());
            let srcset = attr == "srcset=";
            let val = if srcset {
                raw.split(',')
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
            } else {
                raw
            };
            let val = val.trim().replace("&amp;", "&");
            if val.is_empty()
                || val.starts_with('#')
                || val.starts_with("javascript:")
                || val.starts_with("mailto:")
                || val.starts_with("data:")
            {
                continue;
            }
            out.push(Attr {
                start,
                end,
                value: val,
                srcset,
            });
        }
    }
    out.sort_by_key(|a| a.start);
    out
}

/// Quoted strings that look like URLs / paths of files, found anywhere in a page or script.
fn js_urls(text: &str, base: &Url) -> Vec<Url> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' || b[i] == b'\'' {
            let q = b[i];
            let start = i + 1;
            let mut j = start;
            while j < b.len() && b[j] != q && b[j] != b'\n' && j - start < 300 {
                j += 1;
            }
            if j < b.len() && b[j] == q {
                let s = &text[start..j];
                let looks = (s.starts_with("http://")
                    || s.starts_with("https://")
                    || s.starts_with('/')
                    || s.starts_with("./"))
                    && !s.contains(|c: char| {
                        c.is_whitespace() || c == '<' || c == '{' || c == '(' || c == '\\'
                    })
                    && s.rsplit('/').next().is_some_and(|f| f.contains('.'));
                if looks
                    && let Ok(u) = base.join(s)
                    && matches!(u.scheme(), "http" | "https")
                {
                    out.push(u);
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Links of a page. `js` also looks inside scripts.
pub fn extract_links(html: &str, base: &Url, js: bool) -> Vec<Url> {
    let mut out: Vec<Url> = scan_attrs(html)
        .into_iter()
        .filter_map(|a| base.join(&a.value).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .collect();
    if js {
        out.extend(js_urls(html, base));
    }
    out
}

fn ext_of(u: &Url) -> String {
    u.path()
        .rsplit('/')
        .next()
        .and_then(|f| f.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Is this URL a page to crawl rather than a file to download?
fn is_page(u: &Url) -> bool {
    matches!(
        ext_of(u).as_str(),
        "" | "html" | "htm" | "php" | "asp" | "aspx" | "jsp" | "xhtml" | "shtml"
    )
}

fn filename_of(u: &Url) -> String {
    let last = u.path().rsplit('/').next().unwrap_or("");
    let decoded = percent_encoding::percent_decode_str(last)
        .decode_utf8_lossy()
        .into_owned();
    if decoded.is_empty() {
        u.host_str().unwrap_or("file").to_string()
    } else {
        decoded
    }
}

async fn fetch_page(client: &Client, u: &Url, opts: &GrabOptions) -> Option<String> {
    let mut req = client
        .get(u.as_str())
        .timeout(std::time::Duration::from_secs(15));
    if !opts.username.is_empty() {
        req = req.basic_auth(&opts.username, Some(&opts.password));
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    if let Some(ct) = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
    {
        let ct = ct.to_ascii_lowercase();
        if !(ct.contains("html") || ct.contains("xml") || ct.starts_with("text/")) {
            return None;
        }
    }
    let bytes = resp.bytes().await.ok()?;
    Some(String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_PAGE_BYTES)]).into_owned())
}

/// "www.example.co" -> "example.co": the last two labels, good enough to group sub-domains.
fn base_domain(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() <= 2 {
        host.to_string()
    } else {
        parts[parts.len() - 2..].join(".")
    }
}

pub async fn crawl(client: &Client, start: &str, opts: &GrabOptions) -> Result<CrawlResult, Error> {
    let start = Url::parse(start.trim())
        .or_else(|_| Url::parse(&format!("http://{}", start.trim())))
        .map_err(|e| Error::Fatal(format!("Invalid URL: {e}")))?;
    if !matches!(start.scheme(), "http" | "https") {
        return Err(Error::Fatal("Only http(s) sites can be grabbed".into()));
    }
    let host = start.host_str().unwrap_or("").to_string();
    let domain = base_domain(&host);
    let same_site = |u: &Url| {
        let h = u.host_str().unwrap_or("");
        h == host || (opts.same_domain && (h == domain || h.ends_with(&format!(".{domain}"))))
    };
    let start_dir = match start.path().rfind('/') {
        Some(i) => start.path()[..=i].to_string(),
        None => "/".to_string(),
    };
    let norm = |v: &[String]| -> HashSet<String> {
        v.iter()
            .map(|e| e.trim().trim_start_matches(['.', '*']).to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect()
    };
    let wanted = norm(&opts.exts);
    let excluded = norm(&opts.exclude_exts);
    let skip: Vec<String> = opts
        .skip_urls
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let max_pages = opts.max_pages.clamp(1, 1000) as usize;
    let max_files = opts.max_files.clamp(1, 10000) as usize;
    let levels_base = if opts.whole_site {
        WHOLE_SITE_LEVELS
    } else {
        opts.depth.min(WHOLE_SITE_LEVELS)
    };

    let mut seen_pages: HashSet<String> = HashSet::new();
    let mut seen_files: HashSet<String> = HashSet::new();
    let mut seen_names: HashSet<String> = HashSet::new();
    let mut files: Vec<FoundFile> = Vec::new();
    let mut kept_pages: Vec<(Url, String)> = Vec::new();
    // (page, link levels left on the start site, link levels left on other sites)
    let mut level: Vec<(Url, u32, u32)> = vec![(start.clone(), levels_base, opts.levels_other)];
    seen_pages.insert(start.as_str().to_string());
    let mut fetched = 0usize;

    while !level.is_empty() && fetched < max_pages {
        level.truncate(max_pages - fetched);
        fetched += level.len();
        let pages = join_all(level.iter().map(|(u, b, o)| async move {
            (u.clone(), *b, *o, fetch_page(client, u, opts).await)
        }))
        .await;
        let mut next = Vec::new();
        for (page_url, b, o, body) in pages {
            let Some(body) = body else { continue };
            for mut link in extract_links(&body, &page_url, opts.process_js) {
                link.set_fragment(None);
                if !skip.is_empty() {
                    let l = link.as_str().to_ascii_lowercase();
                    if skip.iter().any(|s| l.contains(s.as_str())) {
                        continue;
                    }
                }
                let site = same_site(&link);
                if is_page(&link) {
                    let child = if site {
                        if b == 0 || (opts.no_parent && !link.path().starts_with(&start_dir)) {
                            continue;
                        }
                        (b - 1, o)
                    } else {
                        if o == 0 {
                            continue;
                        }
                        (b, o - 1)
                    };
                    if seen_pages.insert(link.as_str().to_string()) {
                        next.push((link, child.0, child.1));
                    }
                } else {
                    let ext = ext_of(&link);
                    if (!wanted.is_empty() && !wanted.contains(&ext)) || excluded.contains(&ext) {
                        continue;
                    }
                    if opts.same_host && !site {
                        continue;
                    }
                    if files.len() >= max_files || !seen_files.insert(link.as_str().to_string()) {
                        continue;
                    }
                    let name = filename_of(&link);
                    if opts.hide_duplicates && !seen_names.insert(name.to_ascii_lowercase()) {
                        continue;
                    }
                    files.push(FoundFile {
                        url: link.as_str().to_string(),
                        filename: name,
                        page: page_url.as_str().to_string(),
                    });
                }
            }
            if opts.offline_links {
                kept_pages.push((page_url, body));
            }
        }
        level = next;
    }
    Ok(CrawlResult {
        files,
        pages: kept_pages,
    })
}

// ---- offline copy --------------------------------------------------------------

/// Where a URL lives inside an offline copy: `host/dir/file`.
pub fn local_rel(u: &Url, page: bool) -> PathBuf {
    let mut p = PathBuf::from(sanitize_filename(
        &u.host_str().unwrap_or("site").replace(':', "_"),
    ));
    let path = u.path();
    let mut segs: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| sanitize_filename(&percent_encoding::percent_decode_str(s).decode_utf8_lossy()))
        .collect();
    if path.ends_with('/') || segs.is_empty() {
        segs.push("index.html".into());
    } else if page {
        let last = segs.last_mut().unwrap();
        let l = last.to_ascii_lowercase();
        if !(l.ends_with(".html") || l.ends_with(".htm")) {
            last.push_str(".html");
        }
    }
    for s in segs {
        p.push(s);
    }
    p
}

fn rel_link(from_dir: &Path, to: &Path) -> String {
    let a: Vec<_> = from_dir.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".into(); a.len() - common];
    parts.extend(
        b[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().replace(' ', "%20")),
    );
    parts.join("/")
}

/// Point every link whose target is part of the offline copy at its local file.
pub fn rewrite_html(
    html: &str,
    page_url: &Url,
    page_rel: &Path,
    known: &HashMap<String, PathBuf>,
) -> String {
    let from_dir = page_rel.parent().unwrap_or(Path::new(""));
    let mut out = String::with_capacity(html.len());
    let mut last = 0;
    for a in scan_attrs(html) {
        if a.srcset || a.start < last {
            continue;
        }
        let Ok(mut target) = page_url.join(&a.value) else {
            continue;
        };
        let frag = target.fragment().map(|f| format!("#{f}"));
        target.set_fragment(None);
        if let Some(local) = known.get(target.as_str()) {
            out.push_str(&html[last..a.start]);
            out.push_str(&rel_link(from_dir, local));
            if let Some(f) = frag {
                out.push_str(&f);
            }
            last = a.end;
        }
    }
    out.push_str(&html[last..]);
    out
}

/// Write the crawled pages under `root`, links rewritten to the files that were / will be saved.
/// `file_urls` are the files the user chose to download.
pub fn write_mirror(
    root: &Path,
    crawl: &CrawlResult,
    file_urls: &[String],
) -> std::io::Result<usize> {
    let mut known: HashMap<String, PathBuf> = HashMap::new();
    for (u, _) in &crawl.pages {
        let mut k = u.clone();
        k.set_fragment(None);
        known.insert(k.as_str().to_string(), local_rel(u, true));
    }
    for f in file_urls {
        if let Ok(u) = Url::parse(f) {
            known.insert(u.as_str().to_string(), local_rel(&u, false));
        }
    }
    let mut n = 0;
    for (u, body) in &crawl.pages {
        let rel = local_rel(u, true);
        let target = root.join(&rel);
        if let Some(d) = target.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&target, rewrite_html(body, u, &rel, &known))?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_and_resolves() {
        let base = Url::parse("http://x.test/a/index.html").unwrap();
        let html = r##"<a href="b.zip">z</a> <img SRC='/i/p.png'> <a href=c.pdf>c</a> <a href="#top">t</a> <a xhref="no.zip"> <a href="mailto:a@b">m</a>"##;
        let l: Vec<String> = extract_links(html, &base, false)
            .into_iter()
            .map(|u| u.to_string())
            .collect();
        assert!(l.contains(&"http://x.test/a/b.zip".to_string()));
        assert!(l.contains(&"http://x.test/i/p.png".to_string()));
        assert!(l.contains(&"http://x.test/a/c.pdf".to_string()));
        assert_eq!(l.len(), 3);
    }

    #[test]
    fn finds_urls_inside_scripts() {
        let base = Url::parse("http://x.test/a/").unwrap();
        let html = r#"<script>var u = "/media/clip.mp4"; load('https://cdn.test/x/app.js'); var t = "not a url";</script>"#;
        let off: Vec<_> = extract_links(html, &base, false);
        assert!(off.is_empty());
        let on: Vec<String> = extract_links(html, &base, true)
            .into_iter()
            .map(|u| u.to_string())
            .collect();
        assert!(on.contains(&"http://x.test/media/clip.mp4".to_string()));
        assert!(on.contains(&"https://cdn.test/x/app.js".to_string()));
        assert_eq!(on.len(), 2);
    }

    #[test]
    fn offline_links_are_rewritten_relative() {
        let page = Url::parse("http://x.test/a/index.html").unwrap();
        let mut known = HashMap::new();
        known.insert(
            "http://x.test/a/b.zip".to_string(),
            local_rel(&Url::parse("http://x.test/a/b.zip").unwrap(), false),
        );
        known.insert(
            "http://x.test/img/p.png".to_string(),
            local_rel(&Url::parse("http://x.test/img/p.png").unwrap(), false),
        );
        let html =
            r#"<a href="b.zip">z</a><img src="/img/p.png"><a href="http://other.test/q.zip">o</a>"#;
        let out = rewrite_html(html, &page, &local_rel(&page, true), &known);
        assert_eq!(
            out,
            r#"<a href="b.zip">z</a><img src="../img/p.png"><a href="http://other.test/q.zip">o</a>"#
        );
    }

    #[test]
    fn local_paths_follow_the_site_layout() {
        let u = Url::parse("http://x.test/dir/page").unwrap();
        assert_eq!(
            local_rel(&u, true),
            PathBuf::from("x.test").join("dir").join("page.html")
        );
        let r = Url::parse("http://x.test/").unwrap();
        assert_eq!(
            local_rel(&r, true),
            PathBuf::from("x.test").join("index.html")
        );
    }
}
