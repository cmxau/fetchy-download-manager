//! Site grabber projects and crawling.

use super::*;

impl Manager {
    // Grabber projects ---------------------------------------------------------

    pub fn grab_projects(&self) -> Vec<GrabProject> {
        self.inner.projects.lock().unwrap().clone()
    }

    pub(super) fn save_projects(&self) {
        let list = self.grab_projects();
        if let Ok(b) = serde_json::to_vec_pretty(&list) {
            let _ = std::fs::write(self.inner.dir.join("projects.json"), b);
        }
    }

    /// Create (empty id) or update a project; returns it with its id.
    pub fn save_grab_project(&self, mut p: GrabProject) -> GrabProject {
        if p.id.is_empty() {
            p.id = uuid::Uuid::new_v4().to_string();
        }
        if p.name.trim().is_empty() {
            p.name = url::Url::parse(&p.url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| "Project".into());
        }
        {
            let mut list = self.inner.projects.lock().unwrap();
            match list.iter_mut().find(|x| x.id == p.id) {
                Some(x) => {
                    // run statistics belong to the engine, not to the editor
                    p.last_run = x.last_run;
                    p.last_found = x.last_found;
                    p.last_added = x.last_added;
                    p.last_error = x.last_error.clone();
                    *x = p.clone();
                }
                None => list.push(p.clone()),
            }
        }
        self.save_projects();
        p
    }

    pub fn delete_grab_project(&self, id: &str) {
        self.inner.projects.lock().unwrap().retain(|p| p.id != id);
        self.save_projects();
    }

    /// Crawl a project and queue every file that is not in the list yet. Returns how many were added.
    pub async fn run_grab_project(&self, id: &str) -> Result<u32, Error> {
        let p = self
            .inner
            .projects
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| Error::Fatal("project not found".into()))?;
        if !self
            .inner
            .running_projects
            .lock()
            .unwrap()
            .insert(p.id.clone())
        {
            return Err(Error::Fatal("this project is already running".into()));
        }
        let mut options = p.options.clone();
        options.project_name = p.name.clone();
        let result = match self.grab(&p.url, &options).await {
            Ok(files) => self
                .grab_add(&p.url, &options, None, &p.queue)
                .await
                .map(|n| (files.len() as u32, n)),
            Err(e) => Err(e),
        };
        self.inner.running_projects.lock().unwrap().remove(&p.id);
        let (found, added, err) = match &result {
            Ok((found, added)) => (*found, *added, None),
            Err(e) => (0, 0, Some(e.to_string())),
        };
        {
            let mut list = self.inner.projects.lock().unwrap();
            if let Some(x) = list.iter_mut().find(|x| x.id == p.id) {
                x.last_run = Some(chrono::Local::now().timestamp());
                x.last_found = found;
                x.last_added = added;
                x.last_error = err;
            }
        }
        self.save_projects();
        result.map(|(_, added)| added)
    }

    /// Projects with a timer that are due, run in the background.
    pub(super) fn run_due_projects(&self) {
        let now = chrono::Local::now().timestamp();
        let due: Vec<String> = self
            .grab_projects()
            .into_iter()
            .filter(|p| {
                p.every_minutes > 0
                    && p.last_run
                        .is_none_or(|t| now - t >= p.every_minutes as i64 * 60)
            })
            .map(|p| p.id)
            .collect();
        for id in due {
            let me = self.clone();
            self.inner.rt.spawn(async move {
                let _ = me.run_grab_project(&id).await;
            });
        }
    }

    /// Site spider: list the files reachable from a page.
    pub async fn grab(
        &self,
        url: &str,
        opts: &crate::types::GrabOptions,
    ) -> Result<Vec<crate::types::FoundFile>, Error> {
        let result = Arc::new(crate::grab::crawl(&self.client(), url, opts).await?);
        let files = result.files.clone();
        *self.inner.last_crawl.lock().unwrap() = Some((url.to_string(), result));
        Ok(files)
    }

    /// Where a grabbed file goes: (folder, exact file name, overwrite).
    pub(super) fn grab_target(
        &self,
        o: &crate::types::GrabOptions,
        f: &crate::types::FoundFile,
    ) -> (Option<String>, Option<String>, bool) {
        let default = PathBuf::from(self.settings().default_dir);
        if o.save_mode == "project" && !o.offline_links && !o.keep_relative {
            return (
                Some(self.grab_root(o).to_string_lossy().into_owned()),
                None,
                o.overwrite,
            );
        }
        if (o.offline_links || (o.save_mode == "folder" && o.keep_relative))
            && let Ok(u) = url::Url::parse(&f.url)
        {
            let rel = crate::grab::local_rel(&u, false);
            let dir = self
                .grab_root(o)
                .join(rel.parent().unwrap_or(Path::new("")));
            let name = rel.file_name().map(|n| n.to_string_lossy().into_owned());
            return (Some(dir.to_string_lossy().into_owned()), name, true);
        }
        let dir = match o.save_mode.as_str() {
            "folder" if !o.save_dir.is_empty() => Some(o.save_dir.clone()),
            "folder" => Some(default.to_string_lossy().into_owned()),
            "category_folder" => Some(
                default
                    .join(sanitize_filename(&o.save_category))
                    .to_string_lossy()
                    .into_owned(),
            ),
            _ => None,
        };
        (dir, None, o.overwrite)
    }

    /// Root folder of an offline copy / a layout that mirrors the site.
    pub(super) fn grab_root(&self, o: &crate::types::GrabOptions) -> PathBuf {
        let default = PathBuf::from(self.settings().default_dir);
        if o.save_mode == "project" {
            let name = sanitize_filename(o.project_name.trim());
            return default.join(if name.is_empty() {
                "Grabber".to_string()
            } else {
                name
            });
        }
        if o.save_dir.is_empty() {
            default.join("Sites")
        } else {
            PathBuf::from(&o.save_dir)
        }
    }

    /// Queue grabbed files (all of them, or only `selected` URLs) with the project's save options.
    /// Files already in the list are skipped. Returns how many were added.
    pub async fn grab_add(
        &self,
        url: &str,
        opts: &crate::types::GrabOptions,
        selected: Option<&[String]>,
        queue: &str,
    ) -> Result<u32, Error> {
        let cached = self
            .inner
            .last_crawl
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(u, _)| u == url)
            .map(|(_, r)| r.clone());
        let crawl = match cached {
            Some(c) => c,
            None => Arc::new(crate::grab::crawl(&self.client(), url, opts).await?),
        };
        let known: std::collections::HashSet<String> =
            self.list().into_iter().map(|i| i.url).collect();
        let chosen: Vec<&crate::types::FoundFile> = crawl
            .files
            .iter()
            .filter(|f| selected.is_none_or(|s| s.contains(&f.url)))
            .filter(|f| !known.contains(&f.url))
            .collect();
        if opts.offline_links {
            let urls: Vec<String> = chosen.iter().map(|f| f.url.clone()).collect();
            crate::grab::write_mirror(&self.grab_root(opts), &crawl, &urls)
                .map_err(|e| Error::Fatal(format!("Cannot write offline pages: {e}")))?;
        }
        let mut added = 0;
        for f in chosen {
            let (dir, filename, overwrite) = self.grab_target(opts, f);
            let ao = AddOptions {
                queue: Some(queue.to_string()),
                referer: Some(f.page.clone()),
                parent: Some(f.page.clone()),
                project: (!opts.project_name.trim().is_empty())
                    .then(|| opts.project_name.trim().to_string()),
                dir,
                filename,
                overwrite,
                username: (!opts.username.is_empty()).then(|| opts.username.clone()),
                password: (!opts.username.is_empty()).then(|| opts.password.clone()),
                ..Default::default()
            };
            if self.add(&f.url, ao).is_ok() {
                added += 1;
            }
        }
        Ok(added)
    }
}
