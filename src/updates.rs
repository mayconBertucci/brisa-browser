use std::{
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use semver::Version;
use serde::Deserialize;
use windows_sys::{
    Win32::{
        Foundation::{ERROR_SUCCESS, HWND},
        Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash},
        System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
    w,
};

const ENDPOINT: &str = "/repos/mayconBertucci/brisa-browser/releases/latest";
const MAX_RESPONSE: usize = 128 * 1024;
const MAX_INSTALLER: u64 = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct Update {
    version: Version,
    size: usize,
    digest: [u8; 32],
}

impl fmt::Display for Update {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.version.fmt(formatter)
    }
}

pub struct Check {
    response: Receiver<io::Result<Option<Update>>>,
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

    pub fn poll(&self) -> Option<io::Result<Option<Update>>> {
        poll(&self.response)
    }
}

fn poll<T>(response: &Receiver<io::Result<T>>) -> Option<io::Result<T>> {
    match response.try_recv() {
        Ok(result) => Some(result),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err(io::Error::other("Operação encerrada."))),
    }
}

pub struct Download(Receiver<io::Result<Installer>>);

impl Download {
    pub fn start(update: Update) -> io::Result<Self> {
        let (sender, response) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("brisa-update-download".into())
            .spawn(move || {
                let _ = sender.send(update.download());
            })?;
        Ok(Self(response))
    }

    pub fn poll(&self) -> Option<io::Result<Installer>> {
        poll(&self.0)
    }
}

impl Update {
    fn download(&self) -> io::Result<Installer> {
        let filename = installer_name(&self.version);
        let address = format!(
            "https://github.com/mayconBertucci/brisa-browser/releases/download/v{}/{filename}",
            self.version
        );
        let url = url::Url::parse(&address).map_err(io::Error::other)?;
        let bytes = crate::http::get_release_asset(url, self.size)?;
        if bytes.len() != self.size || sha256(&bytes)? != self.digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "A integridade do instalador não pôde ser confirmada.",
            ));
        }
        Installer::stage(&filename, bytes, self.digest)
    }
}

fn sha256(bytes: &[u8]) -> io::Result<[u8; 32]> {
    let size = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    let mut digest = [0; 32];
    // SAFETY: The Windows SHA-256 pseudo-handle needs no cleanup. Both buffers are valid for their supplied lengths.
    let result = unsafe {
        BCryptHash(
            BCRYPT_SHA256_ALG_HANDLE,
            null(),
            0,
            bytes.as_ptr(),
            size,
            digest.as_mut_ptr(),
            32,
        )
    };
    if result < 0 {
        return Err(io::Error::other(format!(
            "Falha ao calcular SHA-256 ({result:#x})."
        )));
    }
    Ok(digest)
}

pub struct Installer {
    directory: PathBuf,
    path: PathBuf,
    locked_file: Option<File>,
    launched: bool,
}

impl Installer {
    fn stage(filename: &str, mut bytes: Vec<u8>, digest: [u8; 32]) -> io::Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("Brisa-update-{}-{timestamp}", std::process::id()));
        fs::create_dir(&directory)?;
        let mut installer = Self {
            path: directory.join(filename),
            directory,
            locked_file: None,
            launched: false,
        };
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&installer.path)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
        drop(output);
        let mut zone = installer.path.as_os_str().to_os_string();
        zone.push(":Zone.Identifier");
        fs::write(zone, b"[ZoneTransfer]\r\nZoneId=3\r\n")?;
        // Allow reads only while verifying the file and handing it to Windows.
        const FILE_SHARE_READ: u32 = 1;
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&installer.path)?;
        let expected_size = bytes.len();
        bytes.clear();
        (&file)
            .take(expected_size as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() != expected_size || sha256(&bytes)? != digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "O arquivo temporário do instalador não passou na verificação.",
            ));
        }
        installer.locked_file = Some(file);
        Ok(installer)
    }

    pub fn launch(mut self, owner: HWND) -> io::Result<()> {
        let mode = installed_mode()?;
        let parameters = mode
            .map(|mode| crate::chrome::wide(&format!("/SILENT /NORESTART /BRISAUPDATE=1 {mode}")));
        let path: Vec<_> = self.path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: The UI owner is live and all strings are terminated and retained through the shell call.
        // The verified download keeps its Internet zone marker so Windows can apply its trust policy.
        let status = unsafe {
            ShellExecuteW(
                owner,
                w!("open"),
                path.as_ptr(),
                parameters.as_ref().map_or(null(), |value| value.as_ptr()),
                null(),
                SW_SHOWNORMAL,
            )
        };
        if status as isize <= 32 {
            return Err(io::Error::other(
                "O Windows não iniciou o instalador. Você pode tentar novamente pelo menu.",
            ));
        }
        self.launched = true;
        Ok(())
    }
}

impl Drop for Installer {
    fn drop(&mut self) {
        drop(self.locked_file.take());
        if !self.launched {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_dir(&self.directory);
        }
    }
}

fn installed_mode() -> io::Result<Option<&'static str>> {
    let executable = std::env::current_exe()?.canonicalize()?;
    let key = w!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{BB6FD759-7F2E-4520-AC26-CCD7100E0DFB}_is1"
    );
    for (root, mode) in [
        (HKEY_LOCAL_MACHINE, "/ALLUSERS"),
        (HKEY_CURRENT_USER, "/CURRENTUSER"),
    ] {
        let mut value = [0u16; 32768];
        let mut size = size_of_val(&value) as u32;
        // SAFETY: Registry keys and value name are constant. The buffer size matches the initialized array.
        let status = unsafe {
            RegGetValueW(
                root,
                key,
                w!("InstallLocation"),
                RRF_RT_REG_SZ,
                null_mut(),
                value.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status == ERROR_SUCCESS {
            let end = value
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(value.len());
            let folder = OsString::from_wide(&value[..end]);
            if Path::new(&folder).is_absolute()
                && Path::new(&folder)
                    .join("brisa.exe")
                    .canonicalize()
                    .is_ok_and(|installed| installed == executable)
            {
                return Ok(Some(mode));
            }
        }
    }
    Ok(None)
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
    digest: Option<String>,
}

fn installer_name(version: &Version) -> String {
    format!("Brisa-{version}-Windows-x64.exe")
}

fn parse_digest(value: &str) -> Option<[u8; 32]> {
    let hex = value.strip_prefix("sha256:")?;
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(digest)
}

fn newer_release(body: &[u8], installed: &Version) -> io::Result<Option<Update>> {
    let release: Release = serde_json::from_slice(body)?;
    let Some(version) = release
        .tag_name
        .strip_prefix('v')
        .and_then(|tag| Version::parse(tag).ok())
    else {
        return Ok(None);
    };
    if release.draft
        || release.prerelease
        || !version.pre.is_empty()
        || !version.build.is_empty()
        || version <= *installed
    {
        return Ok(None);
    }
    let expected = installer_name(&version);
    let verified_asset = release.assets.into_iter().find_map(|asset| {
        if asset.name != expected
            || asset.state != "uploaded"
            || !(1..=MAX_INSTALLER).contains(&asset.size)
        {
            return None;
        }
        Some((asset.size as usize, parse_digest(asset.digest.as_deref()?)?))
    });
    Ok(verified_asset.map(|(size, digest)| Update {
        version,
        size,
        digest,
    }))
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
                "assets": [{ "name": asset, "state": "uploaded", "size": 1024,
                    "digest": format!("sha256:{}", "af".repeat(32)) }]
            });
            let result = newer_release(payload.to_string().as_bytes(), &installed).unwrap();
            assert_eq!(result.is_some(), accepted);
            if let Some(update) = result {
                assert_eq!(update.size, 1024);
                assert_eq!(update.digest, [0xaf; 32]);
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

    #[test]
    fn validates_sha256_metadata_and_bytes() -> io::Result<()> {
        let expected = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(parse_digest(expected), Some(sha256(b"abc")?));
        assert_ne!(parse_digest(expected), Some(sha256(b"abd")?));
        assert_eq!(parse_digest("sha256:0123"), None);
        assert_eq!(parse_digest(&format!("sha256:{}", "xx".repeat(32))), None);
        Ok(())
    }

    #[test]
    #[ignore = "baixa o instalador oficial sem executá-lo"]
    fn stages_the_official_installer_with_internet_zone() -> io::Result<()> {
        let update =
            newer_release(&latest()?, &Version::new(0, 0, 0))?.expect("release verificável");
        let installer = update.download()?;
        let mut zone = installer.path.as_os_str().to_os_string();
        zone.push(":Zone.Identifier");
        assert_eq!(fs::read_to_string(zone)?, "[ZoneTransfer]\r\nZoneId=3\r\n");
        assert_eq!(fs::metadata(&installer.path)?.len(), update.size as u64);
        let directory = installer.directory.clone();
        drop(installer);
        assert!(!directory.exists());
        Ok(())
    }
}
