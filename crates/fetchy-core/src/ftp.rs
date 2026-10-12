//! Minimal FTP client: enough to probe a file and stream a byte range of it.
//! Every range uses its own control + data connection, so the segmented engine
//! can run several of them in parallel exactly like HTTP range requests.
use crate::types::Error;
use bytes::Bytes;
use futures_util::Stream;
use std::pin::Pin;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use url::Url;

const TIMEOUT: Duration = Duration::from_secs(20);

pub fn is_ftp(url: &str) -> bool {
    url.len() > 6 && url[..6].eq_ignore_ascii_case("ftp://")
}

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>;

pub struct FtpProbe {
    pub size: Option<u64>,
    pub resumable: bool,
    pub filename: String,
}

struct Target {
    host: String,
    port: u16,
    user: String,
    pass: String,
    path: String,
}

fn parse(url: &str) -> Result<Target, Error> {
    let u = Url::parse(url).map_err(|e| Error::Fatal(format!("Invalid URL: {e}")))?;
    let host = u
        .host_str()
        .ok_or_else(|| Error::Fatal("FTP URL has no host".into()))?
        .to_string();
    let dec = |s: &str| {
        percent_encoding::percent_decode_str(s)
            .decode_utf8_lossy()
            .into_owned()
    };
    let user = if u.username().is_empty() {
        "anonymous".to_string()
    } else {
        dec(u.username())
    };
    let pass = u
        .password()
        .map(dec)
        .unwrap_or_else(|| "fetchy@localhost".to_string());
    let path = dec(u.path());
    if path.is_empty() || path.ends_with('/') {
        return Err(Error::Fatal("That FTP link is a folder, not a file".into()));
    }
    Ok(Target {
        host,
        port: u.port().unwrap_or(21),
        user,
        pass,
        path,
    })
}

struct Ctrl {
    rd: BufReader<TcpStream>,
    peer_ip: std::net::IpAddr,
}

fn net<E: std::fmt::Display>(e: E) -> Error {
    Error::Retry(e.to_string())
}

impl Ctrl {
    /// Reads one (possibly multi-line) reply and returns (code, text).
    async fn reply(&mut self) -> Result<(u16, String), Error> {
        let mut text = String::new();
        let mut code = 0u16;
        loop {
            let mut line = String::new();
            let n = timeout(TIMEOUT, self.rd.read_line(&mut line))
                .await
                .map_err(|_| net("FTP server timed out"))?
                .map_err(net)?;
            if n == 0 {
                return Err(net("FTP connection closed"));
            }
            text.push_str(&line);
            let b = line.as_bytes();
            if b.len() >= 4 && b[..3].iter().all(u8::is_ascii_digit) {
                let c: u16 = line[..3].parse().unwrap_or(0);
                if code == 0 {
                    code = c;
                }
                if b[3] == b' ' && c == code {
                    return Ok((code, text));
                }
            }
        }
    }

    async fn cmd(&mut self, c: &str) -> Result<(u16, String), Error> {
        let line = format!("{c}\r\n");
        timeout(TIMEOUT, self.rd.get_mut().write_all(line.as_bytes()))
            .await
            .map_err(|_| net("FTP write timed out"))?
            .map_err(net)?;
        self.reply().await
    }

    async fn open(t: &Target) -> Result<Ctrl, Error> {
        let s = timeout(TIMEOUT, TcpStream::connect((t.host.as_str(), t.port)))
            .await
            .map_err(|_| net("cannot reach FTP server (timeout)"))?
            .map_err(net)?;
        let peer_ip = s.peer_addr().map_err(net)?.ip();
        let _ = s.set_nodelay(true);
        let mut c = Ctrl {
            rd: BufReader::new(s),
            peer_ip,
        };
        let (code, text) = c.reply().await?;
        if code != 220 {
            return Err(Error::Fatal(format!(
                "FTP server refused the connection: {}",
                text.trim()
            )));
        }
        let (code, text) = c.cmd(&format!("USER {}", t.user)).await?;
        let code = if code == 331 {
            c.cmd(&format!("PASS {}", t.pass)).await?.0
        } else {
            code
        };
        if code != 230 {
            return Err(Error::Fatal(format!("FTP login failed: {}", text.trim())));
        }
        let (code, text) = c.cmd("TYPE I").await?;
        if code != 200 {
            return Err(Error::Fatal(format!(
                "FTP: cannot switch to binary mode: {}",
                text.trim()
            )));
        }
        Ok(c)
    }

    /// Opens a passive data connection.
    async fn data(&mut self) -> Result<TcpStream, Error> {
        let (code, text) = self.cmd("PASV").await?;
        let addr = if code == 227 {
            let nums: Vec<u16> = text
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .skip(1) // the reply code
                .filter_map(|s| s.parse().ok())
                .collect();
            if nums.len() < 6 {
                return Err(net("bad PASV reply"));
            }
            let port = nums[4] * 256 + nums[5];
            // Use the control connection's address: PASV often reports a private one behind NAT.
            (self.peer_ip, port)
        } else {
            let (code, text) = self.cmd("EPSV").await?;
            if code != 229 {
                return Err(net("server supports neither PASV nor EPSV"));
            }
            let port = text
                .split('|')
                .nth(3)
                .and_then(|p| p.parse::<u16>().ok())
                .ok_or_else(|| net("bad EPSV reply"))?;
            (self.peer_ip, port)
        };
        timeout(TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| net("FTP data connection timed out"))?
            .map_err(net)
    }
}

pub async fn probe(url: &str) -> Result<FtpProbe, Error> {
    let t = parse(url)?;
    let mut c = Ctrl::open(&t).await?;
    let (code, text) = c.cmd(&format!("SIZE {}", t.path)).await?;
    let size = if code == 213 {
        text[4..].trim().parse::<u64>().ok()
    } else {
        None
    };
    if code == 550 {
        return Err(Error::Fatal(format!(
            "FTP: file not found or no access: {}",
            text.trim()
        )));
    }
    let resumable = c.cmd("REST 0").await.map(|r| r.0 == 350).unwrap_or(false);
    let _ = c.cmd("QUIT").await;
    let filename = t.path.rsplit('/').next().unwrap_or("file").to_string();
    Ok(FtpProbe {
        size,
        resumable,
        filename,
    })
}

struct St {
    _ctrl: Ctrl,
    data: TcpStream,
    left: u64,
}

/// Stream bytes `[pos, end)` of the file (`end == u64::MAX` = to EOF).
pub async fn open_range(url: &str, pos: u64, end: u64) -> Result<ByteStream, Error> {
    let t = parse(url)?;
    let mut c = Ctrl::open(&t).await?;
    let data = c.data().await?;
    if pos > 0 {
        let (code, text) = c.cmd(&format!("REST {pos}")).await?;
        if code != 350 {
            return Err(Error::Fatal(format!(
                "FTP server cannot resume: {}",
                text.trim()
            )));
        }
    }
    let (code, text) = c.cmd(&format!("RETR {}", t.path)).await?;
    if code != 125 && code != 150 {
        return Err(Error::Fatal(format!(
            "FTP: cannot download: {}",
            text.trim()
        )));
    }
    let left = if end == u64::MAX { u64::MAX } else { end - pos };
    let st = St {
        _ctrl: c,
        data,
        left,
    };
    Ok(Box::pin(futures_util::stream::unfold(
        st,
        |mut st| async move {
            if st.left == 0 {
                return None;
            }
            let mut buf = vec![0u8; 64 * 1024];
            let want = buf.len().min(st.left.min(usize::MAX as u64) as usize);
            match timeout(TIMEOUT, st.data.read(&mut buf[..want])).await {
                Err(_) => Some((
                    Err("FTP data connection timed out".to_string()),
                    St { left: 0, ..st },
                )),
                Ok(Err(e)) => Some((Err(e.to_string()), St { left: 0, ..st })),
                Ok(Ok(0)) => None,
                Ok(Ok(n)) => {
                    buf.truncate(n);
                    if st.left != u64::MAX {
                        st.left -= n as u64;
                    }
                    Some((Ok(Bytes::from(buf)), st))
                }
            }
        },
    )))
}
