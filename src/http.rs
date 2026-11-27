use std::{
    ffi::c_void,
    io,
    mem::size_of,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};

use windows_sys::{Win32::Networking::WinHttp::*, w};

struct Handle(*mut c_void);

impl Handle {
    fn open(raw: *mut c_void) -> io::Result<Self> {
        if raw.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: This is the only owner of the non-null WinHTTP handle.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn succeeded(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn get(host: &str, path: &str, accept: &str, max_response: usize) -> io::Result<Vec<u8>> {
    match request(host, path, accept, max_response, false)? {
        Reply::Body(body) => Ok(body),
        Reply::Redirect(_) => Err(io::Error::other("Redireção HTTPS inesperada.")),
    }
}

pub fn get_release_asset(mut url: url::Url, max_response: usize) -> io::Result<Vec<u8>> {
    for _ in 0..5 {
        let host = release_host(&url).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Origem do instalador inválida.")
        })?;
        let mut path = url.path().to_owned();
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }
        match request(host, &path, "application/octet-stream", max_response, true)? {
            Reply::Body(body) => return Ok(body),
            Reply::Redirect(location) => {
                url = url.join(&location).map_err(io::Error::other)?;
            }
        }
    }
    Err(io::Error::other(
        "Redireções demais ao baixar o instalador.",
    ))
}

fn release_host(url: &url::Url) -> Option<&str> {
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.host_str().filter(|host| {
        matches!(
            *host,
            "github.com" | "release-assets.githubusercontent.com" | "objects.githubusercontent.com"
        )
    })
}

enum Reply {
    Body(Vec<u8>),
    Redirect(String),
}

fn request(
    host: &str,
    path: &str,
    accept: &str,
    max_response: usize,
    allow_redirect: bool,
) -> io::Result<Reply> {
    let host = crate::chrome::wide(host);
    let path = crate::chrome::wide(path);
    let accept = crate::chrome::wide(&format!("Accept: {accept}\r\n"));
    // SAFETY: All request strings are nul-terminated. Each WinHTTP handle is
    // owned for the duration of the calls that use it and closed in reverse order.
    unsafe {
        let agent = crate::chrome::wide(concat!("Brisa/", env!("CARGO_PKG_VERSION")));
        let session = Handle::open(WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        ))?;
        succeeded(WinHttpSetTimeouts(session.0, 3000, 4000, 4000, 4000))?;
        let connection = Handle::open(WinHttpConnect(session.0, host.as_ptr(), 443, 0))?;
        let request = Handle::open(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            path.as_ptr(),
            null(),
            null(),
            null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        let disabled =
            WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION;
        succeeded(WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_DISABLE_FEATURE,
            (&raw const disabled).cast(),
            size_of::<u32>() as u32,
        ))?;
        succeeded(WinHttpSendRequest(
            request.0,
            accept.as_ptr(),
            u32::MAX,
            null(),
            0,
            0,
            0,
        ))?;
        succeeded(WinHttpReceiveResponse(request.0, null_mut()))?;
        let mut status = 0u32;
        let mut length = size_of::<u32>() as u32;
        succeeded(WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&raw mut status).cast(),
            &mut length,
            null_mut(),
        ))?;
        if allow_redirect && matches!(status, 301 | 302 | 303 | 307 | 308) {
            let mut location = [0u16; 8192];
            let mut length = size_of::<[u16; 8192]>() as u32;
            succeeded(WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_LOCATION,
                null(),
                location.as_mut_ptr().cast(),
                (&raw mut length).cast(),
                null_mut(),
            ))?;
            let end = location
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(location.len());
            return String::from_utf16(&location[..end])
                .map(Reply::Redirect)
                .map_err(io::Error::other);
        }
        if status != 200 {
            return Err(io::Error::other(format!(
                "Servidor respondeu HTTP {status}."
            )));
        }
        let deadline = Instant::now() + Duration::from_secs(if allow_redirect { 120 } else { 10 });
        let mut body = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Resposta HTTPS demorou demais.",
                ));
            }
            let mut read = 0;
            succeeded(WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
            ))?;
            if read == 0 {
                return Ok(Reply::Body(body));
            }
            let read = read as usize;
            if read > buffer.len() || body.len() + read > max_response {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Resposta HTTPS excede o limite.",
                ));
            }
            body.extend_from_slice(&buffer[..read]);
        }
    }
}
