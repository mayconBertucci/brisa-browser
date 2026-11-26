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
        if status != 200 {
            return Err(io::Error::other(format!(
                "Servidor respondeu HTTP {status}."
            )));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
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
                return Ok(body);
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
