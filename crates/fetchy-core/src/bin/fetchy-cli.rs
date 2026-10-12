//! Tiny CLI over the engine: `fetchy-cli <url> [-o dir] [-n connections] [-l bytes/s]`.
//! Mainly used to benchmark the engine without any UI.

use fetchy_core::{AddOptions, Event, Manager, Status};
use std::io::Write;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut url, mut dir, mut conns, mut limit) = (None, None, None, 0u64);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => dir = args.next(),
            "-n" => conns = args.next().and_then(|v| v.parse().ok()),
            "-l" => limit = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            _ => url = Some(a),
        }
    }
    let Some(url) = url else {
        eprintln!("usage: fetchy-cli <url> [-o dir] [-n connections] [-l bytes/s]");
        std::process::exit(2);
    };
    let data = std::env::temp_dir().join(format!("fetchy-cli-{}", std::process::id()));
    let m = Manager::new(&data)?;
    let mut s = m.settings();
    s.speed_limit = limit;
    m.set_settings(s);
    let mut rx = m.subscribe();
    let started = std::time::Instant::now();
    let info = m
        .add(
            &url,
            AddOptions {
                dir,
                connections: conns,
                ..Default::default()
            },
        )
        .map_err(anyhow::Error::msg)?;
    loop {
        match rx.recv().await {
            Ok(Event::Updated(i)) if i.id == info.id => {
                let total = i.size.map(|s| s as f64 / 1e6).unwrap_or(0.0);
                print!(
                    "\r{:<30.30} {:>8.1}/{:.1} MB  {:>7.2} MB/s  {:>2} conns ",
                    i.filename,
                    i.downloaded as f64 / 1e6,
                    total,
                    i.speed as f64 / 1e6,
                    i.active_connections
                );
                std::io::stdout().flush().ok();
                match i.status {
                    Status::Complete => {
                        let secs = started.elapsed().as_secs_f64();
                        println!(
                            "\nDone in {:.2}s, avg {:.2} MB/s",
                            secs,
                            i.downloaded as f64 / 1e6 / secs
                        );
                        break;
                    }
                    Status::Error => {
                        println!("\nError: {}", i.error.unwrap_or_default());
                        std::process::exit(1);
                    }
                    _ => {}
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(_) => break,
            _ => {}
        }
    }
    let _ = std::fs::remove_dir_all(data);
    Ok(())
}
