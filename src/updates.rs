use std::{
    io,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use semver::Version;
use serde::Deserialize;

const ENDPOINT: &str = "/repos/mayconBertucci/brisa-browser/releases/latest";
const MAX_RESPONSE: usize = 128 * 1024;

pub struct Check {
    response: Receiver<io::Result<Option<Version>>>,
    pub notify: bool,
}

impl Check {
    pub fn start(notify: bool) -> io::Result<Self> {
        let (sender, response) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("brisa-updates".into())
            .spawn(move || {
                let installed = Version::parse(env!("CARGO_PKG_VERSION")).map_err(io::Error::other);
                let result = installed.and_then(|installed| {
                    latest().and_then(|body| newer_release(&body, &installed))
                });
                let _ = sender.send(result);
            })?;
        Ok(Self { response, notify })
    }

    pub fn poll(&self) -> Option<io::Result<Option<Version>>> {
        match self.response.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(io::Error::other("Consulta encerrada."))),
        }
    }
}

pub fn release_page(version: &Version) -> String {
    format!("https://github.com/mayconBertucci/brisa-browser/releases/tag/v{version}")
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    prerelease: bool,
    draft: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    state: String,
    size: u64,
}

fn newer_release(body: &[u8], installed: &Version) -> io::Result<Option<Version>> {
    let release: Release = serde_json::from_slice(body)?;
    let Some(version) = release
        .tag_name
        .strip_prefix('v')
        .and_then(|tag| Version::parse(tag).ok())
    else {
        return Ok(None);
    };
    let expected = format!("Brisa-{version}-Windows-x64.exe");
    Ok((!release.draft
        && !release.prerelease
        && version.pre.is_empty()
        && version.build.is_empty()
        && version > *installed
        && release
            .assets
            .iter()
            .any(|asset| asset.name == expected && asset.state == "uploaded" && asset.size > 0))
    .then_some(version))
}

fn latest() -> io::Result<Vec<u8>> {
    crate::http::get(
        "api.github.com",
        ENDPOINT,
        "application/vnd.github+json",
        MAX_RESPONSE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_announces_new_stable_versions_with_an_installer() {
        let installed = Version::new(1, 9, 0);
        for (tag, draft, prerelease, asset, accepted) in [
            (
                "v1.10.0",
                false,
                false,
                "Brisa-1.10.0-Windows-x64.exe",
                true,
            ),
            ("v1.8.0", false, false, "Brisa-1.8.0-Windows-x64.exe", false),
            ("v1.10.0", false, false, "README.md", false),
            (
                "v1.10.0",
                true,
                false,
                "Brisa-1.10.0-Windows-x64.exe",
                false,
            ),
            (
                "v1.10.0",
                false,
                true,
                "Brisa-1.10.0-Windows-x64.exe",
                false,
            ),
        ] {
            let payload = serde_json::json!({
                "tag_name": tag, "draft": draft, "prerelease": prerelease,
                "html_url": "https://example.invalid/",
                "assets": [{ "name": asset, "state": "uploaded", "size": 1024 }]
            });
            let result = newer_release(payload.to_string().as_bytes(), &installed).unwrap();
            assert_eq!(result.is_some(), accepted);
            if let Some(version) = result {
                assert!(
                    release_page(&version)
                        .starts_with("https://github.com/mayconBertucci/brisa-browser/")
                );
            }
        }
    }

    #[test]
    #[ignore = "consulta o GitHub público"]
    fn reads_the_official_releases_feed() -> io::Result<()> {
        let body = latest()?;
        let _: Release = serde_json::from_slice(&body)?;
        Ok(())
    }
}
