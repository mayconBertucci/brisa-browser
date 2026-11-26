use std::{io, ptr::null_mut, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, DC_BRUSH, EndPaint, FillRect, GetMonitorInfoW, GetStockObject, InvalidateRect,
        MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, PAINTSTRUCT, SetDCBrushColor,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::GetDpiForWindow,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

// The tab strip provides the caption. Keeping the resize and system-menu
// styles lets the shell treat this as an ordinary resizable application.
pub const STYLE: u32 =
    WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_CLIPCHILDREN;

const FRAME_SUBCLASS: usize = 1;

pub fn app_icon() -> HICON {
    // SAFETY: The executable supplies icon resource 1. LoadIconW returns a shared handle.
    unsafe {
        LoadIconW(
            GetModuleHandleW(null_mut()),
            std::ptr::without_provenance(1),
        )
    }
}

pub fn integrate(hwnd: HWND) -> io::Result<()> {
    let icon = app_icon();
    // SAFETY: The root window is live on this thread, and the subclass keeps
    // no borrowed state. The icon is shared from this executable.
    unsafe {
        if SetWindowSubclass(hwnd, Some(frame_proc), FRAME_SUBCLASS, 0) == 0 {
            return Err(io::Error::other(
                "Não foi possível integrar a faixa de abas.",
            ));
        }
        if !icon.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon as LPARAM);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, icon as LPARAM);
        }
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    Ok(())
}

unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    // SAFETY: The OS provides this live root window and its message arguments
    // through the subclass registered in integrate on this same UI thread.
    unsafe {
        if message == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(frame_proc), FRAME_SUBCLASS);
        }
        frame_message(hwnd, message, lparam)
            .unwrap_or_else(|| DefSubclassProc(hwnd, message, wparam, lparam))
    }
}

fn frame_message(hwnd: HWND, msg: u32, lparam: LPARAM) -> Option<LRESULT> {
    match msg {
        // Keep erasure suppressed even if controller creation or a native
        // menu pumps messages while the browser callback is already in use.
        WM_ERASEBKGND => Some(1),
        WM_NCCALCSIZE => Some(0),
        WM_NCHITTEST => Some(hit_test(hwnd, lparam)),
        WM_GETMINMAXINFO if lparam != 0 => {
            let mut monitor = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..MONITORINFO::default()
            };
            // SAFETY: WM_GETMINMAXINFO supplies a writable descriptor for this call.
            unsafe {
                let info = &mut *(lparam as *mut MINMAXINFO);
                let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                info.ptMinTrackSize.x = 560 * dpi / 96;
                info.ptMinTrackSize.y = 350 * dpi / 96;
                if GetMonitorInfoW(
                    MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                    &mut monitor,
                ) != 0
                {
                    info.ptMaxPosition.x = monitor.rcWork.left - monitor.rcMonitor.left;
                    info.ptMaxPosition.y = monitor.rcWork.top - monitor.rcMonitor.top;
                    info.ptMaxSize.x = monitor.rcWork.right - monitor.rcWork.left;
                    info.ptMaxSize.y = monitor.rcWork.bottom - monitor.rcWork.top;
                }
            }
            Some(0)
        }
        _ => None,
    }
}

/// A hidden native parent keeps a new WebView2 off the visible window during
/// asynchronous construction and provides a themed surface while it paints.
pub struct ViewHost(HWND);

impl ViewHost {
    pub fn new(parent: HWND, color: u32) -> io::Result<Self> {
        static REGISTERED: OnceLock<bool> = OnceLock::new();
        let class = windows_sys::w!("Brisa.WebViewHost");
        // SAFETY: This module handle is shared for the life of the process.
        let instance = unsafe { GetModuleHandleW(null_mut()) };
        let registered = REGISTERED.get_or_init(|| {
            let descriptor = WNDCLASSW {
                hInstance: instance,
                lpszClassName: class,
                lpfnWndProc: Some(view_host_proc),
                ..WNDCLASSW::default()
            };
            // SAFETY: The descriptor is fully initialized, and its class
            // name and callback remain available for the process lifetime.
            unsafe { RegisterClassW(&descriptor) != 0 }
        });
        if !registered {
            return Err(io::Error::other(
                "Não foi possível registrar a área da página.",
            ));
        }
        // SAFETY: The browser supplies its live top-level HWND on this UI
        // thread. The new host starts hidden and owns no borrowed data.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class,
                null_mut(),
                WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                0,
                0,
                parent,
                null_mut(),
                instance,
                null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err(io::Error::last_os_error());
        }
        let host = Self(hwnd);
        host.set_color(color);
        Ok(host)
    }

    pub fn hwnd(&self) -> HWND {
        self.0
    }

    pub fn position(&self, left: i32, top: i32, width: i32, height: i32) -> io::Result<()> {
        // SAFETY: The tab owns this HWND and positions it on the UI thread.
        if unsafe {
            SetWindowPos(
                self.0,
                HWND_TOP,
                left,
                top,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn show(&self, visible: bool) -> io::Result<()> {
        let flags = if visible {
            SWP_SHOWWINDOW
        } else {
            SWP_HIDEWINDOW | SWP_NOZORDER
        };
        // SAFETY: The tab owns this HWND and changes visibility on the UI
        // thread. Raising the incoming sibling leaves the outgoing view intact.
        if unsafe {
            SetWindowPos(
                self.0,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | flags,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn set_color(&self, color: u32) {
        // SAFETY: This window class uses its user-data slot exclusively for
        // the COLORREF value. Invalidating does not retain a rectangle.
        unsafe {
            SetWindowLongPtrW(self.0, GWLP_USERDATA, color as isize);
            InvalidateRect(self.0, null_mut(), 0);
        }
    }
}

impl Drop for ViewHost {
    fn drop(&mut self) {
        // SAFETY: The controller closes first. DestroyWindow accepts this
        // HWND even if the root window has already destroyed its children.
        unsafe { DestroyWindow(self.0) };
    }
}

unsafe extern "system" fn view_host_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: This callback receives its live HWND and message arguments from
    // Windows. The paint DC is borrowed through the paired Begin/EndPaint.
    unsafe {
        match message {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                if !dc.is_null() {
                    let color = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as u32;
                    let previous = SetDCBrushColor(dc, color);
                    FillRect(dc, &paint.rcPaint, GetStockObject(DC_BRUSH));
                    SetDCBrushColor(dc, previous);
                    EndPaint(hwnd, &paint);
                }
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

fn hit_test(hwnd: HWND, coordinates: LPARAM) -> LRESULT {
    let mut area = RECT::default();
    // SAFETY: The frame handler receives a live root window and owns the output rectangle.
    let (dpi, maximized) = unsafe {
        if GetWindowRect(hwnd, &mut area) == 0 {
            return HTCLIENT as LRESULT;
        }
        (GetDpiForWindow(hwnd).max(96) as i32, IsZoomed(hwnd) != 0)
    };
    let x = coordinates as u16 as i16 as i32;
    let y = (coordinates >> 16) as u16 as i16 as i32;
    let edge = 6 * dpi / 96;
    let left = !maximized && x < area.left + edge;
    let right = !maximized && x >= area.right - edge;
    let top = !maximized && y < area.top + edge;
    let bottom = !maximized && y >= area.bottom - edge;
    let hit = match (left, right, top, bottom) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ if y < area.top + 44 * dpi / 96 => HTCAPTION,
        _ => HTCLIENT,
    };
    hit as LRESULT
}
