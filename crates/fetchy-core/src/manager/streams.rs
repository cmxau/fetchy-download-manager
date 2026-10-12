//! Video streams (HLS/DASH): quality listing, adding, and merging audio with video.

use super::*;

impl Manager {
    /// Both tracks of a split video are on disk: merge them into one MP4 with ffmpeg.
    pub(super) fn try_mux(&self, group: &str) {
        let parts: Vec<(DownloadInfo, AddOptions)> = {
            let st = self.inner.state.lock().unwrap();
            st.items
                .iter()
                .filter(|i| i.rec.opts.mux_group.as_deref() == Some(group))
                .map(|i| (i.rec.info.clone(), i.rec.opts.clone()))
                .collect()
        };
        if parts.len() < 2 || parts.iter().any(|(i, _)| i.status != Status::Complete) {
            return;
        }
        let Some((v, _)) = parts
            .iter()
            .find(|(_, o)| o.mux_role.as_deref() == Some("video"))
            .cloned()
        else {
            return;
        };
        let Some((a, _)) = parts
            .iter()
            .find(|(_, o)| o.mux_role.as_deref() == Some("audio"))
            .cloned()
        else {
            return;
        };
        if !self.inner.muxing.lock().unwrap().insert(group.to_string()) {
            return;
        }
        let me = self.clone();
        let group = group.to_string();
        let settings = self.settings();
        tokio::task::spawn_blocking(move || {
            let (vp, ap) = (
                Path::new(&v.dir).join(&v.filename),
                Path::new(&a.dir).join(&a.filename),
            );
            let note = |text: String| {
                for id in [&v.id, &a.id] {
                    let t = text.clone();
                    me.mutate(id, |i| {
                        i.rec.info.description = t;
                        i.rec.opts.mux_group = None;
                    });
                }
            };
            let Some(ff) = crate::extras::find_ffmpeg(&settings.ffmpeg_path, &me.inner.dir) else {
                note("Video and audio were saved as two files. Install ffmpeg (Settings > Video) to merge them.".into());
                me.save();
                me.inner.muxing.lock().unwrap().remove(&group);
                return;
            };
            let stem = Path::new(&v.filename)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "video".into());
            // Merge into a temporary name first: the video track may already be called "<stem>.mp4".
            let tmp_out = Path::new(&v.dir).join(format!(".{stem}.merging.mp4"));
            match crate::extras::mux(&ff, &vp, &ap, &tmp_out) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&vp);
                    let _ = std::fs::remove_file(&ap);
                    let (out, out_name) =
                        unique_path(Path::new(&v.dir), &format!("{stem}.mp4"), |_| false);
                    if std::fs::rename(&tmp_out, &out).is_err() {
                        note("Could not finish the merged file".into());
                        me.save();
                        me.inner.muxing.lock().unwrap().remove(&group);
                        return;
                    }
                    let size = std::fs::metadata(&out).map(|m| m.len()).ok();
                    me.mutate(&v.id, |i| {
                        i.rec.info.filename = out_name.clone();
                        i.rec.info.size = size;
                        i.rec.info.downloaded = size.unwrap_or(0);
                        i.rec.info.category = categorize(&out_name);
                        i.rec.opts.mux_group = None;
                        i.rec.info.description = "Video and audio merged".into();
                    });
                    let (m2, aid) = (me.clone(), a.id.clone());
                    me.inner
                        .rt
                        .spawn(async move { m2.remove(&aid, false).await });
                }
                Err(e) => note(format!(
                    "Could not merge video and audio ({e}). Both files were kept."
                )),
            }
            me.save();
            me.inner.muxing.lock().unwrap().remove(&group);
        });
    }

    /// The qualities offered by an HLS master playlist (best first).
    pub async fn stream_variants(&self, url: &str) -> Result<Vec<StreamVariant>, Error> {
        if crate::dash::is_dash_url(url) {
            return crate::dash::load_variants(&self.client(), url, &AddOptions::default()).await;
        }
        hls::variants(&self.client(), url, &AddOptions::default()).await
    }

    /// Add one chosen quality. A separate audio track becomes a second download that is merged afterwards.
    pub fn add_stream(
        &self,
        v: &StreamVariant,
        name: Option<String>,
        opts: AddOptions,
    ) -> Result<DownloadInfo, String> {
        let base = name
            .filter(|n| !n.trim().is_empty())
            .or_else(|| opts.filename.clone());
        match &v.audio_url {
            Some(audio) => {
                let group = uuid::Uuid::new_v4().to_string();
                let stem = base.clone().unwrap_or_else(|| hls_stem(&v.url));
                let mut vo = opts.clone();
                vo.mux_group = Some(group.clone());
                vo.mux_role = Some("video".into());
                vo.filename = Some(stem.clone());
                let mut ao = opts;
                ao.mux_group = Some(group);
                ao.mux_role = Some("audio".into());
                ao.filename = Some(format!("{stem} (audio)"));
                let info = self.add(&v.url, vo)?;
                self.add(audio, ao)?;
                Ok(info)
            }
            None => {
                let mut o = opts;
                o.filename = base;
                self.add(&v.url, o)
            }
        }
    }

    /// Like `add`, but an HLS master playlist is resolved to its best quality (with its audio track).
    pub async fn add_smart(&self, url: &str, opts: AddOptions) -> Result<DownloadInfo, String> {
        if hls::is_hls_url(url) || crate::dash::is_dash_url(url) {
            let listed = if crate::dash::is_dash_url(url) {
                crate::dash::load_variants(&self.client(), url, &opts).await
            } else {
                hls::variants(&self.client(), url, &opts).await
            };
            if let Ok(v) = listed
                && let Some(best) = v.first()
            {
                return self.add_stream(best, opts.filename.clone(), opts);
            }
        }
        self.add(url, opts)
    }
}
