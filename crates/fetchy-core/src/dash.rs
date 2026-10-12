//! MPEG-DASH (`.mpd`) support for on-demand streams.
//!
//! A manifest is turned into the same segment list the HLS downloader uses:
//! one init segment plus the media segments of ONE representation, which
//! concatenate into a fragmented MP4. Video and audio are separate
//! representations, so a quality is offered as video + audio and merged
//! afterwards (see `Manager::add_stream`).
//!
//! A representation is addressed as `<manifest url>#rep=<id>`.
//! Not supported: live (dynamic) manifests, DRM, and multi-period manifests
//! (only the first period is used).

use crate::hls::{HSeg, Media};
use crate::types::{AddOptions, Error, StreamVariant};
use reqwest::Client;
use roxmltree::{Document, Node};
use url::Url;

pub fn is_dash_url(u: &str) -> bool {
    Url::parse(u)
        .map(|u| u.path().to_ascii_lowercase().ends_with(".mpd"))
        .unwrap_or(false)
}

fn bad(m: &str) -> Error {
    Error::Fatal(m.to_string())
}

/// ISO-8601 duration ("PT1H2M3.5S") in seconds.
fn iso_duration(s: &str) -> Option<f64> {
    let s = s.strip_prefix('P')?;
    let (date, time) = match s.split_once('T') {
        Some((d, t)) => (d, t),
        None => (s, ""),
    };
    let mut total = 0.0;
    let mut num = String::new();
    for c in date.chars() {
        match c {
            '0'..='9' | '.' => num.push(c),
            'D' => {
                total += num.parse::<f64>().ok()? * 86400.0;
                num.clear();
            }
            _ => num.clear(),
        }
    }
    for c in time.chars() {
        match c {
            '0'..='9' | '.' => num.push(c),
            'H' => {
                total += num.parse::<f64>().ok()? * 3600.0;
                num.clear();
            }
            'M' => {
                total += num.parse::<f64>().ok()? * 60.0;
                num.clear();
            }
            'S' => {
                total += num.parse::<f64>().ok()?;
                num.clear();
            }
            _ => {}
        }
    }
    Some(total)
}

fn child<'a, 'b>(n: Node<'a, 'b>, name: &str) -> Option<Node<'a, 'b>> {
    n.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

fn attr<'a>(n: Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attribute(name)
}

/// First value of an inheritable attribute: Representation, then AdaptationSet.
fn inherited<'a>(rep: Node<'a, '_>, set: Node<'a, '_>, name: &str) -> Option<&'a str> {
    attr(rep, name).or_else(|| attr(set, name))
}

/// $RepresentationID$, $Number$, $Bandwidth$, $Time$ with optional %0Nd.
fn fill(template: &str, id: &str, bandwidth: u64, number: u64, time: u64) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(j) = after.find('$') else { break };
        let token = &after[..j];
        let (name, fmt) = token
            .split_once('%')
            .map_or((token, None), |(n, f)| (n, Some(f)));
        let val: Option<u64> = match name {
            "Number" => Some(number),
            "Bandwidth" => Some(bandwidth),
            "Time" => Some(time),
            "" => {
                out.push('$');
                None
            }
            _ => None,
        };
        match (name, val) {
            ("RepresentationID", _) => out.push_str(id),
            (_, Some(v)) => {
                let width = fmt
                    .and_then(|f| {
                        f.trim_start_matches('0')
                            .trim_end_matches('d')
                            .parse::<usize>()
                            .ok()
                    })
                    .unwrap_or(0);
                out.push_str(&format!("{v:0width$}"));
            }
            _ => {}
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    out
}

struct Rep<'a, 'b> {
    node: Node<'a, 'b>,
    set: Node<'a, 'b>,
    id: String,
    bandwidth: u64,
    height: Option<u32>,
    audio: bool,
    video: bool,
}

fn representations<'a, 'b>(period: Node<'a, 'b>) -> Vec<Rep<'a, 'b>> {
    let mut out = vec![];
    for set in period
        .children()
        .filter(|c| c.is_element() && c.tag_name().name() == "AdaptationSet")
    {
        for rep in set
            .children()
            .filter(|c| c.is_element() && c.tag_name().name() == "Representation")
        {
            let mime = inherited(rep, set, "mimeType")
                .unwrap_or("")
                .to_ascii_lowercase();
            let ct = attr(set, "contentType").unwrap_or("").to_ascii_lowercase();
            let video = mime.starts_with("video") || ct == "video" || attr(rep, "height").is_some();
            let audio = !video && (mime.starts_with("audio") || ct == "audio");
            out.push(Rep {
                node: rep,
                set,
                id: attr(rep, "id").unwrap_or("").to_string(),
                bandwidth: attr(rep, "bandwidth")
                    .and_then(|b| b.parse().ok())
                    .unwrap_or(0),
                height: inherited(rep, set, "height").and_then(|h| h.parse().ok()),
                audio,
                video,
            });
        }
    }
    out
}

fn first_period<'a, 'b>(doc: &'a Document<'b>) -> Result<(Node<'a, 'b>, Node<'a, 'b>), Error> {
    let mpd = doc.root_element();
    if mpd.tag_name().name() != "MPD" {
        return Err(bad("Not a DASH manifest"));
    }
    if attr(mpd, "type") == Some("dynamic") {
        return Err(bad("Live DASH streams are not supported yet"));
    }
    let period = child(mpd, "Period").ok_or_else(|| bad("Manifest has no Period"))?;
    Ok((mpd, period))
}

fn drm(period: Node) -> bool {
    period.descendants().any(|n| {
        n.is_element()
            && n.tag_name().name() == "ContentProtection"
            && !attr(n, "schemeIdUri")
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("mp4protection")
    })
}

/// The qualities of a manifest, best first, each with the best audio representation.
pub fn variants(text: &str, manifest: &Url) -> Result<Vec<StreamVariant>, Error> {
    let doc = Document::parse(text).map_err(|e| bad(&format!("Bad manifest: {e}")))?;
    let (_, period) = first_period(&doc)?;
    if drm(period) {
        return Err(bad("This stream is DRM-protected and cannot be downloaded"));
    }
    let reps = representations(period);
    let audio = reps.iter().filter(|r| r.audio).max_by_key(|r| r.bandwidth);
    let link = |id: &str| {
        let mut u = manifest.clone();
        u.set_fragment(Some(&format!("rep={id}")));
        u.to_string()
    };
    let mut out: Vec<StreamVariant> = reps
        .iter()
        .filter(|r| r.video)
        .map(|r| StreamVariant {
            url: link(&r.id),
            label: match r.height {
                Some(h) => format!("{h}p · {:.1} Mbps", r.bandwidth as f64 / 1e6),
                None => format!("{:.1} Mbps", r.bandwidth as f64 / 1e6),
            },
            bandwidth: r.bandwidth,
            height: r.height,
            audio_url: audio.map(|a| link(&a.id)),
        })
        .collect();
    if out.is_empty() {
        // audio-only manifest
        out = reps
            .iter()
            .filter(|r| r.audio)
            .map(|r| StreamVariant {
                url: link(&r.id),
                label: format!("{:.0} kbps audio", r.bandwidth as f64 / 1e3),
                bandwidth: r.bandwidth,
                height: None,
                audio_url: None,
            })
            .collect();
    }
    out.sort_by_key(|v| std::cmp::Reverse(v.bandwidth));
    Ok(out)
}

/// Segment list of one representation (`id`), ready for the HLS downloader.
pub fn media(text: &str, manifest: &Url, id: &str) -> Result<Media, Error> {
    let doc = Document::parse(text).map_err(|e| bad(&format!("Bad manifest: {e}")))?;
    let (mpd, period) = first_period(&doc)?;
    if drm(period) {
        return Err(bad("This stream is DRM-protected and cannot be downloaded"));
    }
    let reps = representations(period);
    let rep = reps
        .iter()
        .find(|r| r.id == id)
        .or(reps.iter().find(|r| r.video))
        .ok_or_else(|| bad("Representation not found in the manifest"))?;
    let (rn, sn) = (rep.node, rep.set);

    // BaseURL chain: MPD / Period / AdaptationSet / Representation
    let mut base = manifest.clone();
    base.set_fragment(None);
    for n in [mpd, period, sn, rn] {
        if let Some(b) = child(n, "BaseURL").and_then(|b| b.text()) {
            base = base
                .join(b.trim())
                .map_err(|e| bad(&format!("bad BaseURL: {e}")))?;
        }
    }
    let join = |s: &str| {
        base.join(s)
            .map_err(|e| bad(&format!("bad segment URL: {e}")))
    };

    let total_secs = attr(period, "duration")
        .or_else(|| attr(mpd, "mediaPresentationDuration"))
        .and_then(iso_duration);

    // SegmentTemplate (on the Representation or its AdaptationSet)
    if let Some(t) = child(rn, "SegmentTemplate").or_else(|| child(sn, "SegmentTemplate")) {
        let timescale: f64 = attr(t, "timescale")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0);
        let start_number: u64 = attr(t, "startNumber")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let media_t = attr(t, "media").ok_or_else(|| bad("SegmentTemplate without media"))?;
        let init = match attr(t, "initialization") {
            Some(i) => Some((join(&fill(i, &rep.id, rep.bandwidth, 0, 0))?, None)),
            None => None,
        };
        let mut segments = vec![];
        let mut dur_total = 0.0;
        if let Some(tl) = child(t, "SegmentTimeline") {
            let (mut time, mut number) = (0u64, start_number);
            for s in tl
                .children()
                .filter(|c| c.is_element() && c.tag_name().name() == "S")
            {
                if let Some(tt) = attr(s, "t").and_then(|v| v.parse().ok()) {
                    time = tt;
                }
                let d: u64 = attr(s, "d")
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| bad("timeline entry without d"))?;
                let repeat: i64 = attr(s, "r").and_then(|v| v.parse().ok()).unwrap_or(0);
                if repeat < 0 {
                    return Err(bad("Open-ended timeline (live) is not supported"));
                }
                for _ in 0..=repeat {
                    segments.push(HSeg {
                        url: join(&fill(media_t, &rep.id, rep.bandwidth, number, time))?,
                        duration: d as f64 / timescale,
                        seq: number,
                        key: None,
                        range: None,
                    });
                    dur_total += d as f64 / timescale;
                    time += d;
                    number += 1;
                }
            }
        } else {
            let d: f64 = attr(t, "duration")
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| bad("SegmentTemplate needs a duration or a timeline"))?;
            let seg_secs = d / timescale;
            let count = match attr(t, "endNumber").and_then(|v| v.parse::<u64>().ok()) {
                Some(e) => e + 1 - start_number,
                None => (total_secs.ok_or_else(|| bad("Manifest has no duration"))? / seg_secs)
                    .ceil() as u64,
            };
            for i in 0..count {
                let n = start_number + i;
                segments.push(HSeg {
                    url: join(&fill(
                        media_t,
                        &rep.id,
                        rep.bandwidth,
                        n,
                        (i as f64 * d) as u64,
                    ))?,
                    duration: seg_secs,
                    seq: n,
                    key: None,
                    range: None,
                });
                dur_total += seg_secs;
            }
        }
        if segments.is_empty() {
            return Err(bad("Manifest lists no segments"));
        }
        return Ok(Media {
            segments,
            init,
            fmp4: true,
            duration: dur_total,
            live: false,
            target: 6.0,
        });
    }

    // SegmentList
    if let Some(l) = child(rn, "SegmentList").or_else(|| child(sn, "SegmentList")) {
        let init = match child(l, "Initialization").and_then(|i| attr(i, "sourceURL")) {
            Some(u) => Some((join(u)?, None)),
            None => None,
        };
        let timescale: f64 = attr(l, "timescale")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0);
        let d: f64 = attr(l, "duration")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
            / timescale;
        let mut segments = vec![];
        for (i, s) in l
            .children()
            .filter(|c| c.is_element() && c.tag_name().name() == "SegmentURL")
            .enumerate()
        {
            let u = attr(s, "media").ok_or_else(|| bad("SegmentURL without media"))?;
            segments.push(HSeg {
                url: join(u)?,
                duration: d,
                seq: i as u64,
                key: None,
                range: None,
            });
        }
        if segments.is_empty() {
            return Err(bad("Manifest lists no segments"));
        }
        let duration = d * segments.len() as f64;
        return Ok(Media {
            segments,
            init,
            fmp4: true,
            duration,
            live: false,
            target: 6.0,
        });
    }

    // A single file addressed by BaseURL (SegmentBase): one "segment", no init.
    if child(rn, "BaseURL").is_some() {
        let url = base.clone();
        return Ok(Media {
            segments: vec![HSeg {
                url,
                duration: total_secs.unwrap_or(0.0),
                seq: 0,
                key: None,
                range: None,
            }],
            init: None,
            fmp4: true,
            duration: total_secs.unwrap_or(0.0),
            live: false,
            target: 6.0,
        });
    }
    Err(bad(
        "This manifest uses a segment addressing mode Fetchy does not support",
    ))
}

async fn fetch_text(client: &Client, url: &Url, opts: &AddOptions) -> Result<String, Error> {
    let mut u = url.clone();
    u.set_fragment(None);
    let mut req = client.get(u);
    if let Some(r) = &opts.referer {
        req = req.header(reqwest::header::REFERER, r);
    }
    if let Some(c) = &opts.cookie {
        req = req.header(reqwest::header::COOKIE, c);
    }
    if let Some(user) = &opts.username {
        req = req.basic_auth(user, opts.password.as_deref());
    }
    let resp = req.send().await.map_err(|e| Error::Retry(e.to_string()))?;
    if !resp.status().is_success() {
        let m = format!("HTTP {}", resp.status().as_u16());
        return Err(if resp.status().is_server_error() {
            Error::Retry(m)
        } else {
            Error::Fatal(m)
        });
    }
    resp.text().await.map_err(|e| Error::Retry(e.to_string()))
}

/// Download the manifest and list its qualities.
pub async fn load_variants(
    client: &Client,
    url: &str,
    opts: &AddOptions,
) -> Result<Vec<StreamVariant>, Error> {
    let u = Url::parse(url).map_err(|e| bad(&e.to_string()))?;
    variants(&fetch_text(client, &u, opts).await?, &u)
}

/// Download the manifest and build the segment list of the representation named in the URL fragment.
pub async fn load(client: &Client, url: &str, opts: &AddOptions) -> Result<(Media, Url), Error> {
    let u = Url::parse(url).map_err(|e| bad(&e.to_string()))?;
    let id = u
        .fragment()
        .and_then(|f| f.strip_prefix("rep="))
        .unwrap_or("")
        .to_string();
    let text = fetch_text(client, &u, opts).await?;
    Ok((media(&text, &u, &id)?, u))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MPD: &str = r#"<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT10S" xmlns="urn:mpeg:dash:schema:mpd:2011">
  <BaseURL>media/</BaseURL>
  <Period>
    <AdaptationSet mimeType="video/mp4">
      <SegmentTemplate timescale="1000" duration="4000" startNumber="1" initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/seg-$Number%03d$.m4s"/>
      <Representation id="v1" bandwidth="800000" width="640" height="360"/>
      <Representation id="v2" bandwidth="3000000" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4" lang="en">
      <SegmentTemplate timescale="48000" initialization="a/init.mp4" media="a/$Time$.m4s">
        <SegmentTimeline><S t="0" d="192000" r="1"/><S d="96000"/></SegmentTimeline>
      </SegmentTemplate>
      <Representation id="a1" bandwidth="128000"/>
    </AdaptationSet>
  </Period>
</MPD>"#;

    fn base() -> Url {
        Url::parse("http://x.test/d/manifest.mpd").unwrap()
    }

    #[test]
    fn duration_and_template_numbers() {
        assert_eq!(iso_duration("PT1H2M3.5S"), Some(3723.5));
        assert_eq!(
            fill("a/$RepresentationID$/s$Number%05d$.m4s", "v1", 0, 7, 0),
            "a/v1/s00007.m4s"
        );
        assert_eq!(fill("$Time$", "x", 0, 0, 96000), "96000");
    }

    #[test]
    fn lists_qualities_with_audio() {
        let v = variants(MPD, &base()).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].height, Some(720));
        assert!(v[0].url.ends_with("manifest.mpd#rep=v2"));
        assert!(v[0].audio_url.as_deref().unwrap().ends_with("#rep=a1"));
    }

    #[test]
    fn number_template_gives_ceil_of_duration_over_segment() {
        let m = media(MPD, &base(), "v2").unwrap();
        assert_eq!(m.segments.len(), 3); // 10 s / 4 s
        assert_eq!(
            m.segments[0].url.as_str(),
            "http://x.test/d/media/v2/seg-001.m4s"
        );
        assert_eq!(
            m.init.as_ref().unwrap().0.as_str(),
            "http://x.test/d/media/v2/init.mp4"
        );
        assert!(m.fmp4 && !m.live);
    }

    #[test]
    fn timeline_expands_repeats_and_times() {
        let m = media(MPD, &base(), "a1").unwrap();
        let urls: Vec<_> = m
            .segments
            .iter()
            .map(|s| s.url.path().to_string())
            .collect();
        assert_eq!(
            urls,
            vec![
                "/d/media/a/0.m4s",
                "/d/media/a/192000.m4s",
                "/d/media/a/384000.m4s"
            ]
        );
    }

    #[test]
    fn live_and_drm_are_rejected() {
        assert!(media(&MPD.replace("static", "dynamic"), &base(), "v1").is_err());
        let drm = MPD.replace("<AdaptationSet mimeType=\"video/mp4\">", "<AdaptationSet mimeType=\"video/mp4\"><ContentProtection schemeIdUri=\"urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed\"/>");
        assert!(
            variants(&drm, &base())
                .unwrap_err()
                .to_string()
                .contains("DRM")
        );
    }
}
