//! Smaller features that sit around the engine: checksums, name patterns,
//! post-download hooks and the daily transfer totals.
use crate::types::Error;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

// ---- checksums -------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct ChecksumResult {
    pub algo: String,
    pub actual: String,
    pub expected: String,
    pub ok: bool,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Which hash an expected checksum is, judged by its length.
pub fn algo_for(expected: &str) -> Option<&'static str> {
    match expected.len() {
        32 => Some("md5"),
        40 => Some("sha1"),
        64 => Some("sha256"),
        128 => Some("sha512"),
        _ => None,
    }
}

pub fn hash_file(path: &Path, algo: &str) -> std::io::Result<String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    macro_rules! run {
        ($h:expr) => {{
            let mut h = $h;
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            hex(&h.finalize())
        }};
    }
    Ok(match algo {
        "md5" => run!(md5::Md5::new()),
        "sha1" => run!(sha1::Sha1::new()),
        "sha512" => run!(sha2::Sha512::new()),
        _ => run!(sha2::Sha256::new()),
    })
}

pub fn verify(path: &Path, expected: &str) -> Result<ChecksumResult, Error> {
    let expected = expected
        .trim()
        .trim_start_matches("sha256:")
        .to_ascii_lowercase();
    let algo = algo_for(&expected).ok_or_else(|| {
        Error::Fatal("That does not look like an MD5, SHA-1, SHA-256 or SHA-512 checksum".into())
    })?;
    let actual =
        hash_file(path, algo).map_err(|e| Error::Fatal(format!("Cannot read the file: {e}")))?;
    Ok(ChecksumResult {
        algo: algo.into(),
        ok: actual == expected,
        actual,
        expected,
    })
}

// ---- name patterns -------------------------------------------------------------------

/// Case-insensitive match with `*` (any run) and `?` (one character).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None::<usize>, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

pub fn host_matches(pattern: &str, host: &str) -> bool {
    let p = pattern
        .trim()
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let h = host.to_ascii_lowercase();
    !p.is_empty() && (h == p || h.ends_with(&format!(".{p}")))
}

// ---- post-download hooks -----------------------------------------------------------------

/// Runs `cmd` (with {file} {dir} {name} {url} filled in) through the platform shell and waits for it.
pub fn run_command(cmd: &str, file: &Path, url: &str) -> std::io::Result<i32> {
    let dir = file
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = file
        .file_name()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let q = |s: &str| format!("\"{s}\"");
    let cmd = cmd
        .replace("{file}", &q(&file.to_string_lossy()))
        .replace("{dir}", &q(&dir))
        .replace("{name}", &q(&name))
        .replace("{url}", &q(url));
    #[cfg(windows)]
    let mut c = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("cmd");
        // raw_arg: the user's quoting must reach cmd untouched
        c.arg("/C")
            .raw_arg(format!("\"{cmd}\""))
            .creation_flags(0x0800_0000);
        c
    };
    #[cfg(not(windows))]
    let mut c = {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(&cmd);
        c
    };
    let status = c.status()?;
    Ok(status.code().unwrap_or(-1))
}

/// Windows Defender custom scan of one file. Returns "clean", "threat" or "unavailable".
pub fn defender_scan(file: &Path) -> &'static str {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
        let exe = Path::new(&pf).join("Windows Defender").join("MpCmdRun.exe");
        if !exe.exists() {
            return "unavailable";
        }
        let out = std::process::Command::new(exe)
            .args(["-Scan", "-ScanType", "3", "-DisableRemediation", "-File"])
            .arg(file)
            .creation_flags(0x0800_0000)
            .status();
        match out.map(|s| s.code()) {
            Ok(Some(0)) => "clean",
            Ok(Some(2)) => "threat",
            _ => "unavailable",
        }
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        "unavailable"
    }
}

// ---- daily totals -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DayStat {
    pub date: String,
    pub bytes: u64,
}

pub fn load_stats(dir: &Path) -> Vec<DayStat> {
    std::fs::read(dir.join("stats.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_stats(dir: &Path, list: &[DayStat]) {
    if let Ok(b) = serde_json::to_vec(list) {
        let _ = std::fs::write(dir.join("stats.json"), b);
    }
}

pub fn add_bytes(list: &mut Vec<DayStat>, today: &str, bytes: u64) {
    match list.iter_mut().find(|d| d.date == today) {
        Some(d) => d.bytes += bytes,
        None => list.push(DayStat {
            date: today.to_string(),
            bytes,
        }),
    }
    list.sort_by(|a, b| a.date.cmp(&b.date));
    let extra = list.len().saturating_sub(120);
    list.drain(..extra);
}

/// SHA-256 of an in-memory buffer (used by the tests to know what to expect).
pub fn hash_file_bytes_for_test(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(bytes))
}

// ---- ffmpeg -----------------------------------------------------------------------------------------

fn runs_ok(p: &Path) -> bool {
    let mut c = std::process::Command::new(p);
    c.arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.status().map(|s| s.success()).unwrap_or(false)
}

/// The ffmpeg to use: the one chosen in Settings, one next to Fetchy or in its data folder, or one on the PATH.
pub fn find_ffmpeg(custom: &str, data_dir: &Path) -> Option<std::path::PathBuf> {
    let exe = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    if !custom.trim().is_empty() {
        let p = std::path::PathBuf::from(custom.trim());
        return p.exists().then_some(p);
    }
    let mut cands = vec![data_dir.join("ffmpeg").join(exe), data_dir.join(exe)];
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        cands.push(d.join(exe));
    }
    if let Some(p) = cands.into_iter().find(|p| p.exists()) {
        return Some(p);
    }
    let on_path = std::path::PathBuf::from(exe);
    runs_ok(&on_path).then_some(on_path)
}

/// Copy a video file and an audio file into one MP4 without re-encoding.
pub fn mux(ffmpeg: &Path, video: &Path, audio: &Path, out: &Path) -> Result<(), String> {
    let mut c = std::process::Command::new(ffmpeg);
    c.args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(video)
        .arg("-i")
        .arg(audio);
    c.args(["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"]);
    if audio
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ts") || e.eq_ignore_ascii_case("aac"))
    {
        c.args(["-bsf:a", "aac_adtstoasc"]);
    }
    c.args(["-movflags", "+faststart"]).arg(out);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let o = c
        .output()
        .map_err(|e| format!("could not start ffmpeg: {e}"))?;
    if o.status.success() && out.exists() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr)
            .lines()
            .last()
            .unwrap_or("ffmpeg failed")
            .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob() {
        assert!(glob_match("invoice*.pdf", "Invoice-2024-03.PDF"));
        assert!(glob_match("*.tar.gz", "a.tar.gz"));
        assert!(glob_match("img_??.jpg", "img_07.jpg"));
        assert!(!glob_match("img_??.jpg", "img_7.jpg"));
        assert!(!glob_match("*.zip", "a.zip.part"));
    }

    #[test]
    fn hosts() {
        assert!(host_matches("example.com", "cdn.example.com"));
        assert!(host_matches("*.example.com", "example.com"));
        assert!(!host_matches("example.com", "notexample.com"));
    }

    #[test]
    fn checksums() {
        let d = std::env::temp_dir().join(format!("fetchy-ck-{}", std::process::id()));
        std::fs::write(&d, b"hello").unwrap();
        let sha = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert!(verify(&d, sha).unwrap().ok);
        assert!(
            verify(&d, "5d41402abc4b2a76b9719d911017c592").unwrap().ok,
            "md5"
        );
        assert!(!verify(&d, &sha.replace('2', "3")).unwrap().ok);
        assert!(verify(&d, "nothex").is_err());
        let _ = std::fs::remove_file(d);
    }
}
