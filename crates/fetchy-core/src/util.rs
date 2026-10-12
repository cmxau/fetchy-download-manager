use crate::types::Category;
use percent_encoding::percent_decode_str;
use std::path::{Path, PathBuf};

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Remove characters Windows/macOS/Linux disagree about, and path separators.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'..='\x1f' => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "download".into()
    } else {
        cleaned
    }
}

/// Parse `Content-Disposition` for a filename (RFC 6266, `filename*` wins).
pub fn filename_from_disposition(v: &str) -> Option<String> {
    let mut plain = None;
    for part in v.split(';').skip(1) {
        let part = part.trim();
        if let Some(rest) = strip_prefix_ci(part, "filename*=") {
            // charset'lang'value
            let val = rest.splitn(3, '\'').nth(2).unwrap_or(rest);
            let dec = percent_decode_str(val).decode_utf8_lossy().into_owned();
            return Some(sanitize_filename(&dec));
        } else if let Some(rest) = strip_prefix_ci(part, "filename=") {
            plain = Some(sanitize_filename(rest.trim().trim_matches('"')));
        }
    }
    plain
}

fn strip_prefix_ci<'a>(s: &'a str, p: &str) -> Option<&'a str> {
    if s.len() >= p.len() && s.is_char_boundary(p.len()) && s[..p.len()].eq_ignore_ascii_case(p) {
        Some(&s[p.len()..])
    } else {
        None
    }
}

pub fn filename_from_url(u: &url::Url) -> String {
    let seg = u
        .path_segments()
        .and_then(|mut s| s.rfind(|x| !x.is_empty()))
        .unwrap_or("");
    let dec = percent_decode_str(seg).decode_utf8_lossy().into_owned();
    sanitize_filename(&dec)
}

pub fn part_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// `name.ext` -> `name (1).ext` until neither the file nor its `.part` exists.
pub fn unique_path(dir: &Path, name: &str, taken: impl Fn(&Path) -> bool) -> (PathBuf, String) {
    let exists = |p: &Path| p.exists() || part_path(p).exists() || taken(p);
    let first = dir.join(name);
    if !exists(&first) {
        return (first, name.to_string());
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 1.. {
        let cand = format!("{stem} ({n}){ext}");
        let p = dir.join(&cand);
        if !exists(&p) {
            return (p, cand);
        }
    }
    unreachable!()
}

pub fn categorize(name: &str) -> Category {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "opus" | "wma" => Category::Music,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" | "iso" => {
            Category::Compressed
        }
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "ts" => Category::Video,
        "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" | "bat" | "sh" => {
            Category::Program
        }
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "epub" | "odt"
        | "csv" => Category::Document,
        "apk" | "aab" | "xapk" => Category::Apk,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif" | "heic" | "ico" => {
            Category::Image
        }
        _ => Category::Other,
    }
}

/// Positional write that works on every platform without sharing a file cursor.
pub fn write_all_at(file: &std::fs::File, mut buf: &[u8], mut off: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::write_at(file, buf, off)?;
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_write(file, buf, off)?;
        if n == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        off += n as u64;
    }
    Ok(())
}

/// Sub-folder name used when organising downloads.
pub fn category_folder(c: Category) -> &'static str {
    match c {
        Category::Music => "Music",
        Category::Compressed => "Compressed",
        Category::Video => "Videos",
        Category::Program => "Programs",
        Category::Document => "Documents",
        Category::Apk => "APKs",
        Category::Image => "Images",
        Category::Other => "Other",
    }
}
