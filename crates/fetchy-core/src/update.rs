//! Quick update: asks GitHub Releases whether a newer version exists.
use crate::types::{Error, UpdateInfo};
use reqwest::Client;

/// Where releases are published.
pub const REPO: &str = "cmxau/fetchy-download-manager";

fn parse_ver(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .map_while(|p| p.parse().ok())
        .collect()
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    parse_ver(latest) > parse_ver(current)
}

/// `repo` is "owner/name". Returns Some when the latest release is newer than this build.
pub async fn check(client: &Client, repo: &str) -> Result<Option<UpdateInfo>, Error> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        repo.trim()
    );
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| Error::Fatal(format!("update check failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::Fatal(format!(
            "update check: GitHub answered {}",
            resp.status()
        )));
    }
    let body = resp.text().await.map_err(|e| Error::Fatal(e.to_string()))?;
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| Error::Fatal(e.to_string()))?;
    let tag = v["tag_name"].as_str().unwrap_or("").to_string();
    if tag.is_empty() || !is_newer(&tag, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    Ok(Some(UpdateInfo {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v["body"].as_str().unwrap_or("").to_string(),
        url: v["html_url"].as_str().unwrap_or("").to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
    }
}
