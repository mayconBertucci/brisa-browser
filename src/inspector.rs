use std::{
    cell::Cell,
    io,
    ptr::null_mut,
    rc::Rc,
    sync::OnceLock,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, SetLastError, WPARAM},
    Graphics::Dwm::{DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DwmSetWindowAttribute},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{AreDpiAwarenessContextsEqual, GetDpiForWindow, GetWindowDpiAwarenessContext},
        WindowsAndMessaging::*,
    },
};
use windows_webview::WebView;

use crate::{
    chrome::{self, Palette},
    frame::{self, ViewHost},
};

pub const DETACHED: u16 = 217;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub const PHONE: Self = Self {
        width: 390,
        height: 844,
    };
    pub const COMPACT: Self = Self {
        width: 360,
        height: 800,
    };
    pub const TABLET: Self = Self {
        width: 768,
        height: 1024,
    };

    pub fn custom(width: u32, height: u32) -> Option<Self> {
        ((240..=2560).contains(&width) && (240..=2560).contains(&height))
            .then_some(Self { width, height })
    }

    pub fn rotated(self) -> Self {
        Self {
            width: self.height,
            height: self.width,
        }
    }

    pub fn label(self) -> String {
        format!("{} × {} px", self.width, self.height)
    }

    fn fit(self, area: RECT, dpi: u32) -> (RECT, u32) {
        let width = (area.right - area.left).max(1);
        let height = (area.bottom - area.top).max(1);
        let density = f64::from(dpi.max(96)) / 96.0;
        let natural_width = f64::from(self.width) * density;
        let natural_height = f64::from(self.height) * density;
        let zoom = (f64::from(width) / natural_width)
            .min(f64::from(height) / natural_height)
            .min(1.0);
        let scale = ((zoom * 1000.0).floor() as u32).max(1);
        let zoom = f64::from(scale) / 1000.0;
        let fitted_width = ((natural_width * zoom).round() as i32).clamp(1, width);
        let fitted_height = ((natural_height * zoom).round() as i32).clamp(1, height);
        let left = area.left + (width - fitted_width) / 2;
        let top = area.top + (height - fitted_height) / 2;
        (
            RECT {
                left,
                top,
                right: left + fitted_width,
                bottom: top + fitted_height,
            },
            scale,
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Bottom,
    Right,
}

impl Side {
    pub fn command(self) -> u16 {
        match self {
            Self::Left => chrome::INSPECT_LEFT,
            Self::Bottom => chrome::INSPECT_BOTTOM,
            Self::Right => chrome::INSPECT_RIGHT,
        }
    }

    pub fn split(self, area: RECT) -> (RECT, RECT) {
        if self == Self::Bottom {
            let edge = area.bottom - (area.bottom - area.top).max(0) * 2 / 5;
            return (
                RECT {
                    bottom: edge,
                    ..area
                },
                RECT { top: edge, ..area },
            );
        }
        let width = ((area.right - area.left) * 2 / 5).min(760);
        if self == Self::Left {
            let edge = area.left + width;
            (
                RECT { left: edge, ..area },
                RECT {
                    right: edge,
                    ..area
                },
            )
        } else {
            let edge = area.right - width;
            (
                RECT {
                    right: edge,
                    ..area
                },
                RECT { left: edge, ..area },
            )
        }
    }
}

struct Search {
    pid: u32,
    before: Vec<HWND>,
    until: Instant,
}

pub struct Panel {
    pub tab: usize,
    owner: HWND,
    palette: Palette,
    web: WebView,
    host: ViewHost,
    detached: Option<DetachedFrame>,
    target: Option<DevToolsWindow>,
    search: Option<Search>,
    dock: bool,
    area: Cell<Option<(i32, i32, i32, i32)>>,
    viewport: Option<Viewport>,
    applied_viewport: Cell<Option<(Viewport, u32)>>,
    viewport_error: Rc<Cell<bool>>,
}

impl Panel {
    pub fn open(
        owner: HWND,
        view_host: HWND,
        palette: Palette,
        tab: usize,
        web: &WebView,
        dock: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let host = ViewHost::new(owner, palette.surface)?;
        let search = browser_pid(view_host).map(|pid| Search {
            pid,
            before: devtools_windows(pid),
            until: Instant::now() + Duration::from_secs(4),
        });
        web.open_dev_tools_window()?;
        Ok(Self {
            tab,
            owner,
            palette,
            web: web.clone(),
            host,
            detached: None,
            target: None,
            search,
            dock,
            area: Cell::new(None),
            viewport: None,
            applied_viewport: Cell::new(None),
            viewport_error: Rc::new(Cell::new(false)),
        })
    }

    pub fn viewport(&self) -> Option<Viewport> {
        self.viewport
    }

    pub fn set_viewport(&mut self, viewport: Option<Viewport>) -> windows_result::Result<()> {
        if self.viewport.is_none() && viewport.is_some() {
            let failed = Rc::clone(&self.viewport_error);
            self.web.call_dev_tools_protocol_method(
                "Emulation.setTouchEmulationEnabled",
                r#"{"enabled":true,"maxTouchPoints":5}"#,
                move |result| {
                    if result.is_err() {
                        failed.set(true);
                    }
                },
            )?;
        } else if self.viewport.is_some() && viewport.is_none() {
            self.clear_viewport();
        }
        self.viewport_error.set(false);
        self.viewport = viewport;
        Ok(())
    }

    fn clear_viewport(&self) {
        self.applied_viewport.set(None);
        let _ = self.web.call_dev_tools_protocol_method(
            "Emulation.clearDeviceMetricsOverride",
            "{}",
            |_| {},
        );
        let _ = self.web.call_dev_tools_protocol_method(
            "Emulation.setTouchEmulationEnabled",
            r#"{"enabled":false}"#,
            |_| {},
        );
    }

    pub fn take_viewport_error(&self) -> bool {
        self.viewport_error.replace(false) && self.viewport.is_some()
    }

    pub fn layout_page(&self, area: RECT) -> RECT {
        let Some(viewport) = self.viewport else {
            return area;
        };
        // SAFETY: The browser owns this top-level window throughout the panel lifetime.
        let dpi = unsafe { GetDpiForWindow(self.owner) };
        let (bounds, scale) = viewport.fit(area, dpi);
        if self.applied_viewport.get() != Some((viewport, scale)) {
            let params = serde_json::json!({
                "width": viewport.width, "height": viewport.height,
                "screenWidth": viewport.width, "screenHeight": viewport.height,
                "mobile": true, "deviceScaleFactor": 2,
                "scale": f64::from(scale) / 1000.0,
                "screenOrientation": if viewport.width > viewport.height {
                    serde_json::json!({ "type": "landscapePrimary", "angle": 90 })
                } else {
                    serde_json::json!({ "type": "portraitPrimary", "angle": 0 })
                },
            });
            let failed = Rc::clone(&self.viewport_error);
            if self
                .web
                .call_dev_tools_protocol_method(
                    "Emulation.setDeviceMetricsOverride",
                    &params.to_string(),
                    move |result| {
                        if result.is_err() {
                            failed.set(true);
                        }
                    },
                )
                .is_err()
            {
                self.viewport_error.set(true);
            }
            self.applied_viewport.set(Some((viewport, scale)));
        }
        bounds
    }

    pub fn poll(&mut self) -> bool {
        if let Some(window) = &self.target {
            if !window.alive() {
                self.target = None;
                return true;
            }
            return false;
        }
        let Some(search) = &self.search else {
            return false;
        };
        let windows: Vec<_> = devtools_windows(search.pid)
            .into_iter()
            .filter(|hwnd| !search.before.contains(hwnd))
            .collect();
        if let [hwnd] = windows.as_slice() {
            self.target = Some(DevToolsWindow::new(*hwnd, search.pid));
            self.search = None;
            let presented = if self.dock {
                self.attach()
            } else {
                self.detach()
            };
            if presented.is_err() {
                self.dock = false;
                if let Some(window) = &self.target {
                    window.brand();
                }
            }
            return true;
        }
        if Instant::now() >= search.until {
            self.search = None;
            return true;
        }
        false
    }

    pub fn finished(&self) -> bool {
        self.target.is_none() && self.search.is_none()
    }

    pub fn docked(&self) -> bool {
        self.dock && self.target.as_ref().is_some_and(|window| window.embedded)
    }

    pub fn attach(&mut self) -> io::Result<()> {
        if let Some(window) = &mut self.target {
            window.attach(self.host.hwnd())?;
        }
        self.detached = None;
        self.dock = true;
        Ok(())
    }

    pub fn detach(&mut self) -> io::Result<()> {
        if let Some(window) = &mut self.target {
            if let Some(frame) = &self.detached {
                frame.show();
                return Ok(());
            }
            let frame = DetachedFrame::new(self.owner, self.palette)?;
            window.attach(frame.0)?;
            frame.set_content(window.hwnd);
            frame.show();
            self.detached = Some(frame);
        }
        self.host.show(false)?;
        self.area.set(None);
        self.dock = false;
        Ok(())
    }

    pub fn layout(&self, area: Option<RECT>) -> io::Result<()> {
        let bounds = area.map(|area| {
            (
                area.left,
                area.top,
                (area.right - area.left).max(0),
                (area.bottom - area.top).max(0),
            )
        });
        if self.area.get() == bounds {
            return Ok(());
        }
        if let Some((left, top, width, height)) = bounds {
            self.host.position(left, top, width, height)?;
            if let Some(window) = &self.target {
                resize_content(window.hwnd, width, height)?;
            }
        }
        self.host.show(bounds.is_some())?;
        self.area.set(bounds);
        Ok(())
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        if self.viewport.is_some() {
            self.clear_viewport();
        }
        if let Some(window) = &mut self.target
            && window.alive()
        {
            let _ = window.restore();
            // SAFETY: The retained handle was checked again against its WebView2
            // process and class. WM_CLOSE carries no borrowed data.
            unsafe { PostMessageW(window.hwnd, WM_CLOSE, 0, 0) };
        }
    }
}

struct DetachedFrame(HWND);

impl DetachedFrame {
    fn new(owner: HWND, palette: Palette) -> io::Result<Self> {
        static REGISTERED: OnceLock<bool> = OnceLock::new();
        let class = windows_sys::w!("Brisa.DevToolsWindow");
        // SAFETY: The module, application icon and default cursor are shared
        // resources that stay valid for the lifetime of the class and its windows.
        let instance = unsafe { GetModuleHandleW(null_mut()) };
        let icon = frame::app_icon();
        if !*REGISTERED.get_or_init(|| unsafe {
            RegisterClassW(&WNDCLASSW {
                hInstance: instance,
                lpszClassName: class,
                lpfnWndProc: Some(detached_proc),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hIcon: icon,
                ..WNDCLASSW::default()
            }) != 0
        }) {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: The browser passes its live owner, and Windows copies the title.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_APPWINDOW,
                class,
                windows_sys::w!("DevTools · Brisa"),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                980,
                700,
                owner,
                null_mut(),
                instance,
                null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: The icon is shared and the color values are borrowed synchronously.
        unsafe {
            for size in [ICON_BIG, ICON_SMALL] {
                SendMessageW(hwnd, WM_SETICON, size as usize, icon as LPARAM);
            }
            for (property, color) in [
                (DWMWA_CAPTION_COLOR, palette.band),
                (DWMWA_TEXT_COLOR, palette.tab_ink),
            ] {
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    property as u32,
                    (&raw const color).cast(),
                    size_of::<u32>() as u32,
                );
            }
        }
        Ok(Self(hwnd))
    }

    fn set_content(&self, child: HWND) {
        // SAFETY: The sole user-data field is reserved for the child handle.
        unsafe { SetWindowLongPtrW(self.0, GWLP_USERDATA, child as isize) };
        resize_detached(self.0);
    }

    fn show(&self) {
        // SAFETY: This is the live top-level window owned by this UI thread.
        unsafe {
            ShowWindow(self.0, SW_SHOW);
            SetForegroundWindow(self.0);
        }
    }
}

impl Drop for DetachedFrame {
    fn drop(&mut self) {
        // SAFETY: The content has already moved or received its close command.
        unsafe { DestroyWindow(self.0) };
    }
}

fn resize_detached(hwnd: HWND) {
    let mut rect = RECT::default();
    // SAFETY: This window class stores its current child HWND in GWLP_USERDATA.
    let child = unsafe {
        let child = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as HWND;
        if child.is_null() || GetParent(child) != hwnd || GetClientRect(hwnd, &mut rect) == 0 {
            return;
        }
        child
    };
    let _ = resize_content(child, rect.right, rect.bottom);
}

unsafe extern "system" fn detached_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: Windows invokes the registered procedure with live message arguments.
    unsafe {
        match msg {
            WM_SIZE => {
                resize_detached(hwnd);
                0
            }
            WM_CLOSE => {
                let owner = GetWindow(hwnd, GW_OWNER);
                if owner.is_null()
                    || PostMessageW(owner, WM_COMMAND, chrome::INSPECT_CLOSE as usize, 0) == 0
                {
                    DestroyWindow(hwnd);
                }
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn resize_content(hwnd: HWND, width: i32, height: i32) -> io::Result<()> {
    if width <= 0 || height <= 0 {
        return Ok(());
    }
    // SAFETY: The child HWND is owned by the tracked DevTools instance. The
    // host clips the Edge window's own caption and border, leaving its tools visible.
    let resized = unsafe {
        let dpi = GetDpiForWindow(hwnd).clamp(96, 768) as i32;
        let border = 7 * dpi / 96;
        let caption = 30 * dpi / 96;
        SetWindowPos(
            hwnd,
            null_mut(),
            -border,
            -caption,
            width + 2 * border,
            height + caption + border,
            SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_ASYNCWINDOWPOS,
        )
    };
    if resized == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

struct BrowserProcess {
    host_pid: u32,
    browser_pid: u32,
}

fn browser_pid(host: HWND) -> Option<u32> {
    let mut process = BrowserProcess {
        host_pid: process_for(host),
        browser_pid: 0,
    };
    // SAFETY: The output is an exclusive stack slot for this synchronous enumeration.
    unsafe { EnumChildWindows(host, Some(find_browser), (&raw mut process) as LPARAM) };
    (process.browser_pid != 0).then_some(process.browser_pid)
}

unsafe extern "system" fn find_browser(hwnd: HWND, slot: LPARAM) -> i32 {
    if !chromium_window(hwnd) {
        return 1;
    }
    // SAFETY: browser_pid passes its stack output and Windows provides the HWND.
    let process = unsafe { &mut *(slot as *mut BrowserProcess) };
    let pid = process_for(hwnd);
    // WebView2 can create a Chromium wrapper in the Brisa process. The
    // browser-owned widget is further down in that same controller's tree.
    if pid == 0 || pid == process.host_pid {
        return 1;
    }
    process.browser_pid = pid;
    0
}

fn devtools_windows(pid: u32) -> Vec<HWND> {
    let mut result = (pid, Vec::<HWND>::new());
    // SAFETY: EnumWindows is synchronous and only this callback uses the mutable buffer.
    unsafe { EnumWindows(Some(find_devtools), (&raw mut result) as LPARAM) };
    result.1
}

unsafe extern "system" fn find_devtools(hwnd: HWND, context: LPARAM) -> i32 {
    // SAFETY: devtools_windows provides this initialized exclusive pointer
    // for the duration of the synchronous EnumWindows call.
    let (pid, windows) = unsafe { &mut *(context as *mut (u32, Vec<HWND>)) };
    if process_for(hwnd) == *pid && chromium_window(hwnd) && devtools_title(hwnd) {
        windows.push(hwnd);
    }
    1
}

fn process_for(hwnd: HWND) -> u32 {
    let mut pid = 0;
    // SAFETY: Windows initializes the borrowed stack output.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

fn chromium_window(hwnd: HWND) -> bool {
    let mut name = [0; 48];
    // SAFETY: The stack buffer fits the passed size. An invalid HWND returns 0.
    unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) };
    name.into_iter()
        .zip("Chrome_WidgetWin_".encode_utf16())
        .all(|(actual, expected)| actual == expected)
}

fn devtools_title(hwnd: HWND) -> bool {
    let mut title = [0; 24];
    // SAFETY: The stack buffer fits the passed size. Only the caption prefix is needed.
    unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
    title
        .into_iter()
        .zip("DevTools".encode_utf16())
        .all(|(actual, expected)| actual == expected)
}

struct DevToolsWindow {
    hwnd: HWND,
    pid: u32,
    saved_style: isize,
    saved_owner: HWND,
    saved_position: RECT,
    embedded: bool,
}

impl DevToolsWindow {
    fn new(hwnd: HWND, pid: u32) -> Self {
        Self {
            hwnd,
            pid,
            saved_style: 0,
            saved_owner: null_mut(),
            saved_position: RECT::default(),
            embedded: false,
        }
    }

    fn alive(&self) -> bool {
        process_for(self.hwnd) == self.pid && chromium_window(self.hwnd)
    }

    fn brand(&self) {
        let icon = frame::app_icon();
        if !self.alive() || icon.is_null() {
            return;
        }
        // SAFETY: WM_SETICON receives a shared USER icon handle and the timeout
        // bounds messaging to the runtime's UI thread.
        unsafe {
            for size in [ICON_BIG, ICON_SMALL] {
                SendMessageTimeoutW(
                    self.hwnd,
                    WM_SETICON,
                    size as usize,
                    icon as LPARAM,
                    SMTO_ABORTIFHUNG | SMTO_BLOCK,
                    160,
                    null_mut(),
                );
            }
        }
    }

    fn attach(&mut self, host: HWND) -> io::Result<()> {
        if !self.alive() {
            return Err(io::Error::other("A janela DevTools foi fechada."));
        }
        // SAFETY: The two live HWNDs are the Brisa panel's host and the matched
        // DevTools window. This saves its native state before changing its presentation.
        unsafe {
            if AreDpiAwarenessContextsEqual(
                GetWindowDpiAwarenessContext(host),
                GetWindowDpiAwarenessContext(self.hwnd),
            ) == 0
            {
                return Err(io::Error::other(
                    "DevTools usa um modo de DPI diferente do Brisa.",
                ));
            }
            if self.embedded {
                SetLastError(0);
                if SetParent(self.hwnd, host).is_null() && GetLastError() != 0 {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            self.saved_style = GetWindowLongPtrW(self.hwnd, GWL_STYLE);
            self.saved_owner = GetWindow(self.hwnd, GW_OWNER);
            if self.saved_style & WS_CHILD as isize != 0
                || GetWindowRect(self.hwnd, &mut self.saved_position) == 0
            {
                return Err(io::Error::other(
                    "A janela DevTools não está disponível para encaixe.",
                ));
            }
            let decorations = (WS_POPUP
                | WS_CAPTION
                | WS_THICKFRAME
                | WS_SYSMENU
                | WS_MINIMIZEBOX
                | WS_MAXIMIZEBOX) as isize;
            let child_style =
                (self.saved_style & !decorations) | (WS_CHILD | WS_CLIPSIBLINGS) as isize;
            SetLastError(0);
            if SetWindowLongPtrW(self.hwnd, GWL_STYLE, child_style) == 0 && GetLastError() != 0 {
                return Err(io::Error::last_os_error());
            }
            SetLastError(0);
            if SetParent(self.hwnd, host).is_null() && GetLastError() != 0 {
                let failure = io::Error::last_os_error();
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, self.saved_style);
                return Err(failure);
            }
            self.embedded = true;
        }
        Ok(())
    }

    fn restore(&mut self) -> io::Result<()> {
        if !self.embedded || !self.alive() {
            return Ok(());
        }
        // SAFETY: Restore the tracked window's top-level frame and screen
        // position. Windows checks any saved owner before we reuse it.
        unsafe {
            SetLastError(0);
            if SetParent(self.hwnd, null_mut()).is_null() && GetLastError() != 0 {
                return Err(io::Error::last_os_error());
            }
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, self.saved_style);
            if IsWindow(self.saved_owner) != 0 {
                SetWindowLongPtrW(self.hwnd, GWLP_HWNDPARENT, self.saved_owner as isize);
            }
            let rect = self.saved_position;
            SetWindowPos(
                self.hwnd,
                null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_FRAMECHANGED
                    | SWP_NOZORDER
                    | SWP_NOACTIVATE
                    | SWP_ASYNCWINDOWPOS
                    | SWP_HIDEWINDOW,
            );
        }
        self.embedded = false;
        Ok(())
    }
}
