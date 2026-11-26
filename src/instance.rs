use std::{
    fs, io,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::{null_mut, without_provenance_mut},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM},
    UI::WindowsAndMessaging::{
        EnumWindows, FlashWindow, GetPropW, IsIconic, RemovePropW, SW_RESTORE, SetForegroundWindow,
        SetPropW, ShowWindowAsync,
    },
};

const REGISTERED: usize = 1;
const UPDATE_REQUESTED: usize = 2;

/// Identifies the main window associated with a locked profile in this Windows session.
pub struct ProfileWindow {
    property: Vec<u16>,
    hwnd: HWND,
}

impl ProfileWindow {
    pub fn new(directory: &Path) -> io::Result<Self> {
        let directory = fs::canonicalize(directory)?;
        // FNV-1a keeps the Windows property name stable across browser builds.
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for unit in directory.as_os_str().encode_wide() {
            for byte in unit.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
            }
        }
        Ok(Self {
            property: crate::chrome::wide(&format!("Brisa.Profile.{hash:016x}")),
            hwnd: null_mut(),
        })
    }

    pub fn register(&mut self, hwnd: HWND) {
        // SAFETY: hwnd is the application's live window and the property has a
        // nul terminator. The value is a marker that is never dereferenced.
        if unsafe {
            SetPropW(
                hwnd,
                self.property.as_ptr(),
                without_provenance_mut(REGISTERED),
            )
        } != 0
        {
            self.hwnd = hwnd;
        }
    }

    pub fn activate(&self, check_updates: bool) -> bool {
        let mut search = WindowSearch {
            property: self.property.as_ptr(),
            found: null_mut(),
        };
        // SAFETY: EnumWindows calls the callback synchronously. The query and
        // its string stay valid until enumeration returns.
        unsafe { EnumWindows(Some(find_window), (&raw mut search) as LPARAM) };
        if search.found.is_null() {
            return false;
        }
        // SAFETY: These APIs receive only a window handle. Windows validates the
        // handle if the other process closes its window during activation.
        unsafe {
            // Save the request on the registered window so it survives modal
            // loops and launches that arrive before the browser's first poll.
            if check_updates {
                SetPropW(
                    search.found,
                    self.property.as_ptr(),
                    without_provenance_mut(UPDATE_REQUESTED),
                );
            }
            if IsIconic(search.found) != 0 {
                ShowWindowAsync(search.found, SW_RESTORE);
            }
            if SetForegroundWindow(search.found) == 0 {
                FlashWindow(search.found, 1);
            }
        }
        true
    }

    pub fn take_update_request(&self) -> bool {
        if self.hwnd.is_null() {
            return false;
        }
        // SAFETY: The registration owns the live window identity and the
        // property string is nul-terminated. Both marker values are integers.
        unsafe {
            GetPropW(self.hwnd, self.property.as_ptr()).addr() == UPDATE_REQUESTED
                && SetPropW(
                    self.hwnd,
                    self.property.as_ptr(),
                    without_provenance_mut(REGISTERED),
                ) != 0
        }
    }
}

impl Drop for ProfileWindow {
    fn drop(&mut self) {
        if !self.hwnd.is_null() {
            // SAFETY: This is the registration's window. A destroyed window is
            // ignored by the OS, and the marker owns no backing allocation.
            unsafe { RemovePropW(self.hwnd, self.property.as_ptr()) };
        }
    }
}

struct WindowSearch {
    property: *const u16,
    found: HWND,
}

unsafe extern "system" fn find_window(hwnd: HWND, context: LPARAM) -> i32 {
    // SAFETY: The only caller passes its live WindowSearch to EnumWindows.
    let search = unsafe { &mut *(context as *mut WindowSearch) };
    // SAFETY: EnumWindows supplies hwnd, and property is a live nul-terminated string.
    if unsafe { GetPropW(hwnd, search.property) }.is_null() {
        return 1;
    }
    search.found = hwnd;
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::{
        Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow},
        w,
    };

    #[test]
    fn relaunch_sets_a_request_the_existing_window_can_consume() -> io::Result<()> {
        struct HiddenWindow(HWND);
        impl Drop for HiddenWindow {
            fn drop(&mut self) {
                // SAFETY: This test owns the window on the creating thread.
                unsafe { DestroyWindow(self.0) };
            }
        }

        // SAFETY: STATIC is a built-in class. This top-level window starts hidden.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                w!("STATIC"),
                w!("Brisa (teste)"),
                0,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err(io::Error::last_os_error());
        }
        let window = HiddenWindow(hwnd);
        let path = std::env::current_exe()?;
        let mut running = ProfileWindow::new(&path)?;
        let launcher = ProfileWindow::new(&path)?;
        running.register(window.0);

        assert!(launcher.activate(false));
        assert!(!running.take_update_request());
        assert!(launcher.activate(true));
        assert!(running.take_update_request());
        assert!(!running.take_update_request());
        assert!(launcher.activate(true));
        assert!(running.take_update_request());
        Ok(())
    }
}
