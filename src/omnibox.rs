use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    io,
    ptr::null_mut,
    sync::{
        OnceLock,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::{DRAWITEMSTRUCT, ODS_SELECTED},
        HiDpi::GetDpiForWindow,
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

use crate::{
    chrome::{self, Palette},
    profile::{Link, display_title},
    protection::{self, SearchEngine},
};

pub const PICK: u32 = WM_APP + 8;
pub const STEP: u32 = WM_APP + 9;
pub const DISMISS: u32 = WM_APP + 10;
const MAX_LOCAL: usize = 5;
const MAX_SUGGESTIONS: usize = 4;

pub struct Choice {
    title: String,
    subtitle: String,
    target: String,
}

pub fn choices(
    input: &str,
    engine: SearchEngine,
    bookmarks: &[Link],
    history: &[Link],
) -> Vec<Choice> {
    let input = input.trim();
    if input.is_empty() || input.len() > 4096 || input.chars().any(char::is_control) {
        return Vec::new();
    }
    let query = fold(input);
    let mut ranked: Vec<_> = bookmarks
        .iter()
        .map(|link| (link, "Marcador"))
        .chain(history.iter().map(|link| (link, "Histórico")))
        .filter_map(|(link, source)| {
            let title = fold(&link.title);
            let address = link
                .url
                .split_once("://")
                .map_or(&*link.url, |(_, address)| address);
            let address = fold(address.strip_prefix("www.").unwrap_or(address));
            let rank = if title.starts_with(&query) || address.starts_with(&query) {
                0
            } else if title.contains(&query) || address.contains(&query) {
                1
            } else {
                return None;
            };
            Some((rank, link, source))
        })
        .collect();
    ranked.sort_by_key(|(rank, _, _)| *rank);
    let mut seen = HashSet::new();
    let mut output = Vec::with_capacity(MAX_LOCAL + 1);
    if let Some(target) = protection::address_or_search(input, engine) {
        let subtitle = if protection::looks_like_address(input) {
            seen.insert(target.as_str().to_owned());
            "Abrir endereço".to_owned()
        } else {
            format!("Pesquisar no {}", engine.name())
        };
        output.push(Choice {
            title: display_title(input, 100),
            subtitle,
            target: target.into(),
        });
    }
    let default_rows = output.len();
    for (_, link, source) in ranked {
        if output.len() == MAX_LOCAL + default_rows {
            break;
        }
        let Some(url) = protection::web_url(&link.url) else {
            continue;
        };
        if url.host_str() == Some("brisa.invalid") || !seen.insert(url.as_str().to_owned()) {
            continue;
        }
        let location = format!("{}{}", url.host_str().unwrap_or_default(), url.path());
        output.push(Choice {
            title: display_title(
                if link.title.is_empty() {
                    &location
                } else {
                    &link.title
                },
                110,
            ),
            subtitle: format!("{source} · {}", display_title(&location, 115)),
            target: url.into(),
        });
    }
    output
}

pub fn append_google(rows: &mut Vec<Choice>, input: &str, terms: &[String]) {
    for term in terms
        .iter()
        .filter(|term| term.as_str() != input.trim())
        .take(MAX_SUGGESTIONS)
    {
        if let Some(target) = protection::address_or_search(term, SearchEngine::Google)
            && target.host_str() == Some("www.google.com")
            && target.path() == "/search"
            && !rows.iter().any(|row| row.target == target.as_str())
        {
            rows.push(Choice {
                title: display_title(term, 100),
                subtitle: "Sugestão do Google".into(),
                target: target.into(),
            });
        }
    }
}

#[derive(Default)]
pub struct Remote {
    revision: u64,
    pending: Option<(String, &'static str, Instant)>,
    worker: Option<(u64, Receiver<Vec<String>>)>,
    terms: Vec<String>,
}

impl Remote {
    pub fn queue(&mut self, input: &str, language: &'static str, enabled: bool) {
        self.clear();
        let input = input.trim();
        if enabled
            && (2..=120).contains(&input.chars().count())
            && input.chars().any(char::is_alphabetic)
            && !input.chars().any(char::is_control)
            && !input.contains(['/', '\\'])
            && !protection::looks_like_address(input)
        {
            self.pending = Some((
                input.to_owned(),
                language,
                Instant::now() + Duration::from_millis(220),
            ));
        }
    }

    pub fn clear(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.pending = None;
        self.terms.clear();
    }

    pub fn terms(&self) -> &[String] {
        &self.terms
    }

    pub fn poll(&mut self) -> bool {
        if let Some((revision, worker)) = &self.worker {
            match worker.try_recv() {
                Ok(terms) => {
                    let current = *revision == self.revision;
                    self.worker = None;
                    if current && !terms.is_empty() {
                        self.terms = terms;
                        return true;
                    }
                }
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => self.worker = None,
            }
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, _, due)| Instant::now() >= *due)
            && let Some((input, language, _)) = self.pending.take()
        {
            let (sender, receiver) = mpsc::sync_channel(1);
            if thread::Builder::new()
                .name("brisa-suggestions".into())
                .spawn(move || {
                    let _ = sender.send(google_terms(&input, language).unwrap_or_default());
                })
                .is_ok()
            {
                self.worker = Some((self.revision, receiver));
            }
        }
        false
    }
}

fn google_terms(input: &str, language: &str) -> io::Result<Vec<String>> {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client", "firefox")
        .append_pair("hl", language)
        .append_pair("q", input)
        .finish();
    let response = crate::http::get(
        "suggestqueries.google.com",
        &format!("/complete/search?{query}"),
        "application/json",
        24 * 1024,
    )?;
    let (echo, terms, ..): (String, Vec<String>, serde_json::Value, serde_json::Value) =
        serde_json::from_slice(&response)?;
    if echo != input {
        return Ok(Vec::new());
    }
    Ok(terms
        .into_iter()
        .filter(|term| term.len() <= 512 && !term.chars().any(char::is_control))
        .take(12)
        .collect())
}

fn fold(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|letter| match letter {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            _ => letter,
        })
        .collect()
}

struct State {
    owner: HWND,
    popup: Cell<HWND>,
    palette: Cell<Palette>,
    rows: RefCell<Vec<Choice>>,
    font: Cell<HFONT>,
    dpi: Cell<i32>,
}

/// A modeless suggestion list. Window callbacks borrow the heap state only while
/// the popup owns its HWND on the UI thread.
pub struct Popup {
    state: Box<State>,
    list: HWND,
}

impl Popup {
    pub fn new(owner: HWND, palette: Palette) -> io::Result<Self> {
        let state = Box::new(State {
            owner,
            popup: Cell::new(null_mut()),
            palette: Cell::new(palette),
            rows: RefCell::default(),
            font: Cell::new(null_mut()),
            dpi: Cell::new(96),
        });
        let class = window_class()?;
        let title = chrome::wide("Sugestões de endereço");
        // SAFETY: The class pointer is static, the title is copied, and the box
        // remains alive until this owned popup and its child are destroyed.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                0,
                0,
                owner,
                null_mut(),
                GetModuleHandleW(null_mut()),
                (&*state as *const State).cast(),
            )
        };
        if hwnd.is_null() {
            return Err(io::Error::last_os_error());
        }
        state.popup.set(hwnd);
        let name = chrome::wide("LISTBOX");
        // SAFETY: The list is owned by the popup. Win32 copies the UTF-16 names.
        let list = unsafe {
            CreateWindowExW(
                0,
                name.as_ptr(),
                title.as_ptr(),
                WS_CHILD | WS_VISIBLE | (LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOTIFY) as u32,
                0,
                0,
                0,
                0,
                hwnd,
                null_mut(),
                GetModuleHandleW(null_mut()),
                null_mut(),
            )
        };
        let popup = Self { state, list };
        // SAFETY: SetWindowSubclass associates the live list and same-thread
        // owner. The pointer-sized callback value is an HWND, never dereferenced.
        if list.is_null()
            || unsafe { SetWindowSubclass(list, Some(list_input), 1, owner as usize) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(popup)
    }

    pub fn show(&self, anchor: HWND, palette: Palette, rows: Vec<Choice>) {
        let hwnd = self.state.popup.get();
        let mut anchor_rect = RECT::default();
        // SAFETY: The address edit is a live toolbar control during this call.
        if rows.is_empty() || unsafe { GetWindowRect(anchor, &mut anchor_rect) } == 0 {
            self.hide();
            return;
        }
        // SAFETY: DPI is read from the same live edit control.
        let dpi = unsafe { GetDpiForWindow(anchor) }.clamp(96, 768) as i32;
        if self.state.font.get().is_null() || self.state.dpi.get() != dpi {
            let font = chrome::create_font(dpi);
            if font.is_null() {
                self.hide();
                return;
            }
            let previous = self.state.font.replace(font);
            self.state.dpi.set(dpi);
            if !previous.is_null() {
                // SAFETY: No popup paint is running on this thread and draw_text restores its font.
                unsafe { DeleteObject(previous) };
            }
        }

        let pad = scale(4, dpi);
        let row_height = scale(41, dpi);
        let count = rows.len();
        let fit = card_rect(anchor_rect, pad, row_height, count, dpi);
        self.state.palette.set(palette);
        *self.state.rows.borrow_mut() = rows;
        // SAFETY: These controls belong to this popup and run on the current
        // thread. LB_ADDSTRING copies each nul-terminated string synchronously.
        unsafe {
            SendMessageW(self.list, WM_SETREDRAW, 0, 0);
            SendMessageW(self.list, LB_RESETCONTENT, 0, 0);
            SendMessageW(self.list, LB_SETITEMHEIGHT, 0, row_height as LPARAM);
            for item in self.state.rows.borrow().iter() {
                let spoken = chrome::wide(&format!("{} — {}", item.title, item.subtitle));
                SendMessageW(self.list, LB_ADDSTRING, 0, spoken.as_ptr() as LPARAM);
            }
            SendMessageW(self.list, LB_SETCURSEL, 0, 0);
            SetWindowPos(
                self.list,
                null_mut(),
                pad,
                pad,
                fit.right - fit.left - 2 * pad,
                fit.bottom - fit.top - 2 * pad,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            SendMessageW(self.list, WM_SETREDRAW, 1, 0);
            SetWindowPos(
                hwnd,
                HWND_TOP,
                fit.left,
                fit.top,
                fit.right - fit.left,
                fit.bottom - fit.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            InvalidateRect(hwnd, std::ptr::null(), 0);
            InvalidateRect(self.list, std::ptr::null(), 0);
        }
    }

    pub fn hide(&self) {
        if self.state.rows.borrow().is_empty() {
            return;
        }
        self.state.rows.borrow_mut().clear();
        // SAFETY: The popup belongs to this UI thread. Clearing list strings
        // releases the displayed history and the typed text at dismissal.
        unsafe {
            ShowWindow(self.state.popup.get(), SW_HIDE);
            SendMessageW(self.list, LB_RESETCONTENT, 0, 0);
        }
    }

    pub fn step(&self, backwards: bool) {
        let count = self.state.rows.borrow().len();
        if count == 0 {
            return;
        }
        let current = self.selected().unwrap_or(0);
        let next = (current + if backwards { count - 1 } else { 1 }) % count;
        // SAFETY: The index is bounded by the current list and the HWND is live.
        unsafe { SendMessageW(self.list, LB_SETCURSEL, next, 0) };
    }

    fn selected(&self) -> Option<usize> {
        // SAFETY: The popup owns the live list HWND throughout its use.
        unsafe { SendMessageW(self.list, LB_GETCURSEL, 0, 0) }
            .try_into()
            .ok()
    }

    pub fn target(&self, row: Option<usize>) -> Option<String> {
        let row = row.or_else(|| self.selected())?;
        self.state
            .rows
            .borrow()
            .get(row)
            .map(|choice| choice.target.clone())
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        // SAFETY: NCDESTROY clears the HWND. The class callback can read the box
        // throughout destruction, and the owned GDI font then has no consumers.
        unsafe {
            let hwnd = self.state.popup.get();
            if !hwnd.is_null() {
                DestroyWindow(hwnd);
            }
            let font = self.state.font.get();
            if !font.is_null() {
                DeleteObject(font);
            }
        }
    }
}

fn scale(value: i32, dpi: i32) -> i32 {
    (value * dpi + 48) / 96
}

fn card_rect(anchor: RECT, pad: i32, row_height: i32, count: usize, dpi: i32) -> RECT {
    let mut work = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: Both structs are initialized and Windows borrows them only for the calls.
    let found = unsafe {
        let monitor = MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut work)
    } != 0;
    let height = row_height * count as i32 + 2 * pad;
    let mut width = (anchor.right - anchor.left).max(scale(230, dpi));
    let mut x = anchor.left;
    let mut y = anchor.bottom;
    if found {
        let area = work.rcWork;
        width = width.min((area.right - area.left).max(2 * pad + 1));
        x = x.clamp(area.left, (area.right - width).max(area.left));
        if y + height > area.bottom && anchor.top - height >= area.top {
            y = anchor.top - height;
        }
        y = y.clamp(area.top, (area.bottom - height).max(area.top));
    }
    RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}

fn window_class() -> io::Result<&'static [u16]> {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    let name = NAME.get_or_init(|| chrome::wide("Brisa.Omnibox"));
    // SAFETY: The class name is static and the callback follows the WNDPROC ABI.
    if *REGISTERED.get_or_init(|| unsafe {
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(popup_proc),
            hInstance: GetModuleHandleW(null_mut()),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: name.as_ptr(),
            ..Default::default()
        }) != 0
    }) {
        Ok(name)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// # Safety
/// Called by Win32 for a popup created with a live `State` pointer. The box is
/// retained by Popup until all HWND callbacks have returned.
unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: Windows supplies a readable CREATESTRUCTW and DRAWITEMSTRUCT
    // during their matching messages. GWLP_USERDATA retains the live box.
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(l as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let state = (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref();
        if let Some(state) = state {
            match msg {
                WM_NCDESTROY => {
                    state.popup.set(null_mut());
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
                WM_ERASEBKGND => return 1,
                WM_PAINT => {
                    let mut paint = PAINTSTRUCT::default();
                    let dc = BeginPaint(hwnd, &mut paint);
                    if !dc.is_null() {
                        fill(dc, &paint.rcPaint, state.palette.get().surface);
                        EndPaint(hwnd, &paint);
                    }
                    return 0;
                }
                WM_DRAWITEM if l != 0 => {
                    let item = &*(l as *const DRAWITEMSTRUCT);
                    draw_row(state, item);
                    return 1;
                }
                WM_COMMAND if (w >> 16) as u32 == LBN_SELCHANGE => {
                    let row = SendMessageW(l as HWND, LB_GETCURSEL, 0, 0);
                    if row >= 0 {
                        PostMessageW(state.owner, PICK, row as usize, 0);
                    }
                    return 0;
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, msg, w, l)
    }
}

/// # Safety
/// Win32 calls this only for the initialized list control. The callback value
/// is the root HWND, which outlives the popup it owns.
unsafe extern "system" fn list_input(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _: usize,
    owner: usize,
) -> LRESULT {
    // SAFETY: The messages apply to the live list and parent on this UI thread.
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
            WM_MOUSEMOVE => {
                let result = SendMessageW(hwnd, LB_ITEMFROMPOINT, 0, l);
                if result >> 16 == 0 {
                    let row = usize::from(result as u16);
                    if SendMessageW(hwnd, LB_GETCURSEL, 0, 0) != row as LRESULT {
                        SendMessageW(hwnd, LB_SETCURSEL, row, 0);
                    }
                }
                0
            }
            WM_LBUTTONDOWN => {
                let result = SendMessageW(hwnd, LB_ITEMFROMPOINT, 0, l);
                if result >> 16 == 0 {
                    PostMessageW(owner as HWND, PICK, usize::from(result as u16), 0);
                }
                0
            }
            WM_LBUTTONUP => 0,
            _ => DefSubclassProc(hwnd, msg, w, l),
        }
    }
}

fn fill(dc: HDC, rect: &RECT, color: u32) {
    // SAFETY: The DC is borrowed during the paint message and the stock brush is unmanaged.
    unsafe {
        SetDCBrushColor(dc, color);
        FillRect(dc, rect, GetStockObject(DC_BRUSH));
    }
}

fn draw_row(state: &State, item: &DRAWITEMSTRUCT) {
    let palette = state.palette.get();
    let selected = item.itemState & ODS_SELECTED != 0;
    let base = if selected {
        palette.hover
    } else {
        palette.surface
    };
    fill(item.hDC, &item.rcItem, base);
    let rows = state.rows.borrow();
    let Some(row) = rows.get(item.itemID as usize) else {
        return;
    };
    let mut line = item.rcItem;
    let dpi = state.dpi.get();
    if selected {
        let marker = RECT {
            left: line.left,
            right: line.left + scale(3, dpi),
            top: line.top + scale(7, dpi),
            bottom: line.bottom - scale(7, dpi),
        };
        fill(item.hDC, &marker, palette.accent);
    }
    line.left += scale(13, dpi);
    line.right -= scale(10, dpi);
    line.bottom = line.top + scale(22, dpi);
    let font = state.font.get();
    chrome::draw_text(item.hDC, font, line, &row.title, palette.ink, DT_LEFT);
    line.top += scale(19, dpi);
    line.bottom += scale(17, dpi);
    chrome::draw_text(item.hDC, font, line, &row.subtitle, palette.muted, DT_LEFT);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(title: &str, url: &str) -> Link {
        Link {
            title: title.into(),
            url: url.into(),
            folder: 0,
            visited_at: 0,
            favicon: None,
            order: None,
        }
    }

    #[test]
    fn ranks_saved_pages_and_offers_an_explicit_search() {
        let saved = [link(
            "Páginas Rust",
            "https://rust.example/manual?token=abc",
        )];
        let history = [
            saved[0].clone(),
            link("Links de páginas", "https://docs.example/links"),
        ];
        let rows = choices("paginas", SearchEngine::Google, &saved, &history);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].target, "https://www.google.com/search?q=paginas");
        assert_eq!(rows[1].title, "Páginas Rust");
        assert_eq!(rows[1].subtitle, "Marcador · rust.example/manual");
        assert_eq!(rows[2].title, "Links de páginas");
    }
}
