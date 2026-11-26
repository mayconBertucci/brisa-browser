use std::{cell::Cell, io, ptr::null_mut, sync::OnceLock};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::{
        LibraryLoader::GetModuleHandleW,
        SystemServices::{SS_CENTERIMAGE, SS_ENDELLIPSIS, SS_NOPREFIX},
    },
    UI::{
        Controls::{DRAWITEMSTRUCT, EM_SETLIMITTEXT, EM_SETSEL, ODS_FOCUS, ODS_SELECTED},
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{
            GetFocus, GetKeyState, SetFocus, VK_APPS, VK_DOWN, VK_END, VK_ESCAPE, VK_F10, VK_HOME,
            VK_RETURN, VK_SHIFT, VK_TAB, VK_UP,
        },
        WindowsAndMessaging::*,
    },
};

use crate::{
    chrome::{self, Palette},
    chrome_draw::{Canvas, Icon},
    favicon::Favicon,
};

#[derive(Clone, Copy)]
pub enum Anchor {
    Control(HWND, bool),
    Point(POINT),
}

pub struct Item {
    id: u16,
    title: String,
    hint: String,
    icon: Option<Icon>,
    favicon: Option<Favicon>,
    gap: bool,
}

impl Item {
    pub fn action(id: u16, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            hint: String::new(),
            icon: None,
            favicon: None,
            gap: false,
        }
    }

    pub fn info(title: impl Into<String>) -> Self {
        Self::action(0, title)
    }

    pub fn shortcut(mut self, text: impl Into<String>) -> Self {
        self.hint = text.into();
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn favicon(mut self, stored: Option<&str>) -> Self {
        self.favicon = stored.and_then(Favicon::from_stored);
        self
    }

    pub fn section(mut self) -> Self {
        self.gap = true;
        self
    }
}

pub fn pick(owner: HWND, anchor: Anchor, palette: Palette, items: &[Item]) -> io::Result<u16> {
    Ok(run(
        owner,
        anchor,
        palette,
        items,
        Content::Menu { context: false },
    )?
    .id)
}

pub fn pick_context(
    owner: HWND,
    anchor: Anchor,
    palette: Palette,
    items: &[Item],
) -> io::Result<(u16, Option<POINT>)> {
    let result = run(
        owner,
        anchor,
        palette,
        items,
        Content::Menu { context: true },
    )?;
    Ok((result.id, result.context))
}

pub fn edit(
    owner: HWND,
    anchor: Anchor,
    palette: Palette,
    title: &str,
    fields: &[(&str, &str, usize)],
) -> io::Result<Option<Vec<String>>> {
    let items = [
        Item::action(1, "Salvar").shortcut("Enter"),
        Item::action(2, "Cancelar").shortcut("Esc"),
    ];
    let result = run(
        owner,
        anchor,
        palette,
        &items,
        Content::Editor { title, fields },
    )?;
    Ok((result.id == 1).then_some(result.fields))
}

pub fn confirm_download(
    owner: HWND,
    anchor: Anchor,
    palette: Palette,
    filename: &str,
    host: &str,
    private: bool,
) -> io::Result<bool> {
    let items = [Item::action(2, "Cancelar"), Item::action(1, "Baixar")];
    let content = Content::Download {
        filename,
        host,
        private,
    };
    Ok(run(owner, anchor, palette, &items, content)?.id == 1)
}

enum Content<'a> {
    Menu {
        context: bool,
    },
    Editor {
        title: &'a str,
        fields: &'a [(&'a str, &'a str, usize)],
    },
    Download {
        filename: &'a str,
        host: &'a str,
        private: bool,
    },
}

const DOWNLOAD_DETAIL: u16 = 30;

struct PaintResources {
    background: HBRUSH,
    input: HBRUSH,
    font: HFONT,
    heading: HFONT,
}

impl PaintResources {
    fn new(palette: Palette, dpi: i32) -> io::Result<Self> {
        let family = chrome::wide("Segoe UI Variable Text");
        // SAFETY: The font descriptor is read immediately and the handles are owned below.
        let resources = unsafe {
            Self {
                background: CreateSolidBrush(palette.surface),
                input: CreateSolidBrush(palette.address),
                font: CreateFontW(
                    -scaled(14, dpi),
                    0,
                    0,
                    0,
                    450,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    DEFAULT_PITCH as u32,
                    family.as_ptr(),
                ),
                heading: CreateFontW(
                    -scaled(17, dpi),
                    0,
                    0,
                    0,
                    650,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    DEFAULT_PITCH as u32,
                    family.as_ptr(),
                ),
            }
        };
        if resources.background.is_null()
            || resources.input.is_null()
            || resources.font.is_null()
            || resources.heading.is_null()
        {
            return Err(io::Error::last_os_error());
        }
        Ok(resources)
    }
}

impl Drop for PaintResources {
    fn drop(&mut self) {
        // SAFETY: The popup has been destroyed before its GDI objects are dropped.
        unsafe {
            DeleteObject(self.background);
            DeleteObject(self.input);
            DeleteObject(self.font);
            DeleteObject(self.heading);
        }
    }
}

struct MenuWindow(HWND);

impl Drop for MenuWindow {
    fn drop(&mut self) {
        // SAFETY: This window was created exclusively by this scope on the UI thread.
        unsafe { DestroyWindow(self.0) };
    }
}

struct State<'a> {
    items: &'a [Item],
    palette: Palette,
    paint: &'a PaintResources,
    answer: Cell<Option<u16>>,
    hovered: Cell<HWND>,
    dpi: i32,
    download: bool,
}

#[derive(Default)]
struct Choice {
    id: u16,
    fields: Vec<String>,
    context: Option<POINT>,
}

fn scaled(value: i32, dpi: i32) -> i32 {
    (value * dpi + 48) / 96
}

fn popup_rect(owner: HWND, anchor: Anchor, width: i32, height: i32) -> RECT {
    let (rect, align_left) = match anchor {
        Anchor::Point(point) => (
            RECT {
                left: point.x,
                top: point.y,
                right: point.x + 1,
                bottom: point.y + 1,
            },
            true,
        ),
        Anchor::Control(control, left) => {
            let mut rect = RECT::default();
            // SAFETY: The caller supplies a live toolbar control or the owner.
            unsafe { GetWindowRect(control, &mut rect) };
            (rect, left)
        }
    };
    // SAFETY: The rectangle is initialized and MonitorFromRect retains no pointer.
    let monitor = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: The monitor and owner's window are both acquired on this UI thread.
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        unsafe { GetWindowRect(owner, &mut info.rcWork) };
    }
    let area = info.rcWork;
    let x = if align_left {
        rect.left
    } else {
        rect.right - width
    }
    .clamp(area.left, (area.right - width).max(area.left));
    let y = if rect.bottom + height > area.bottom {
        rect.top - height
    } else {
        rect.bottom
    }
    .clamp(area.top, (area.bottom - height).max(area.top));
    RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}

fn run(
    owner: HWND,
    anchor: Anchor,
    palette: Palette,
    items: &[Item],
    content: Content<'_>,
) -> io::Result<Choice> {
    if items.is_empty() {
        return Ok(Choice::default());
    }
    // SAFETY: The owner is the application's live window, on this thread.
    let dpi = unsafe { GetDpiForWindow(owner) }.clamp(96, 768) as i32;
    let px = |value| scaled(value, dpi);
    let paint = PaintResources::new(palette, dpi)?;
    let download = matches!(content, Content::Download { .. });
    let (title, fields) = match content {
        Content::Editor { title, fields } => (title, fields),
        Content::Download { .. } => ("Baixar arquivo", &[][..]),
        Content::Menu { .. } => ("", &[][..]),
    };
    let state = State {
        items,
        palette,
        paint: &paint,
        answer: Cell::new(None),
        hovered: Cell::new(null_mut()),
        dpi,
        download,
    };
    let class = window_class()?;
    let title_utf16 = chrome::wide(if title.is_empty() {
        "Menu do Brisa"
    } else {
        title
    });
    let width = px(if download || !fields.is_empty() {
        382
    } else {
        322
    });
    let title_height = if title.is_empty() { 0 } else { 31 };
    let rows_height: i32 = items
        .iter()
        .map(|item| if item.gap { 40 } else { 33 })
        .sum();
    let height = px(match content {
        Content::Download { private, .. } => 150 + i32::from(private) * 24,
        _ => 14 + title_height + 62 * fields.len() as i32 + rows_height,
    });
    let rect = popup_rect(owner, anchor, width, height);
    // SAFETY: The window borrows the state until the MenuWindow guard destroys it.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            title_utf16.as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN,
            rect.left,
            rect.top,
            width,
            height,
            owner,
            null_mut(),
            GetModuleHandleW(null_mut()),
            (&raw const state).cast(),
        )
    };
    if hwnd.is_null() {
        return Err(io::Error::last_os_error());
    }
    let _window = MenuWindow(hwnd);
    let mut y = 7;
    let box_at = |left, top, height| RECT {
        left: px(left),
        top: px(top),
        right: width - px(left),
        bottom: px(top + height),
    };
    if let Content::Download {
        filename,
        host,
        private,
    } = content
    {
        child(
            hwnd,
            "STATIC",
            title,
            SS_CENTERIMAGE,
            0,
            box_at(61, 13, 34),
            paint.heading,
        )?;
        child(
            hwnd,
            "STATIC",
            filename,
            SS_CENTERIMAGE | SS_ENDELLIPSIS | SS_NOPREFIX,
            0,
            box_at(16, 54, 23),
            paint.font,
        )?;
        child(
            hwnd,
            "STATIC",
            &format!("De {host}"),
            SS_CENTERIMAGE | SS_ENDELLIPSIS | SS_NOPREFIX,
            DOWNLOAD_DETAIL,
            box_at(16, 77, 22),
            paint.font,
        )?;
        if private {
            child(
                hwnd,
                "STATIC",
                "O arquivo ficará salvo no computador.",
                SS_CENTERIMAGE,
                DOWNLOAD_DETAIL,
                box_at(16, 101, 22),
                paint.font,
            )?;
        }
        y = 105 + i32::from(private) * 24;
    } else if !title.is_empty() {
        let field_rect = box_at(16, y, 29);
        child(
            hwnd,
            "STATIC",
            title,
            SS_CENTERIMAGE,
            0,
            field_rect,
            paint.font,
        )?;
        y += 31;
    }
    let mut edit_controls = Vec::with_capacity(fields.len());
    for &(label, value, maximum) in fields {
        child(
            hwnd,
            "STATIC",
            label,
            SS_CENTERIMAGE,
            0,
            box_at(16, y, 24),
            paint.font,
        )?;
        let edit = child(
            hwnd,
            "EDIT",
            value,
            WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32,
            0,
            box_at(16, y + 24, 27),
            paint.font,
        )?;
        // SAFETY: The edit remains owned by the popup and the length is an integer limit.
        unsafe { SendMessageW(edit, EM_SETLIMITTEXT, maximum, 0) };
        edit_controls.push(edit);
        y += 62;
    }
    let mut buttons = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        if item.gap {
            y += 7;
        }
        if item.id == 0 {
            child(
                hwnd,
                "STATIC",
                &item.title,
                SS_CENTERIMAGE | SS_ENDELLIPSIS | SS_NOPREFIX,
                0,
                box_at(19, y, 33),
                paint.font,
            )?;
        } else {
            let bounds = if download {
                let left = width - px(220 - 104 * index as i32);
                RECT {
                    left,
                    top: px(y),
                    right: left + px(96),
                    bottom: px(y + 34),
                }
            } else {
                box_at(7, y, 33)
            };
            let control = child(
                hwnd,
                "BUTTON",
                &item.title,
                WS_TABSTOP | BS_OWNERDRAW as u32,
                item.id,
                bounds,
                paint.font,
            )?;
            buttons.push((item.id, control));
        }
        if !download {
            y += 33;
        }
    }
    // SAFETY: A successful SetWindowRgn transfers the region to Windows.
    unsafe {
        let rounding = px(18);
        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, rounding, rounding);
        if !region.is_null() && SetWindowRgn(hwnd, region, 0) == 0 {
            DeleteObject(region);
        }
        ShowWindow(hwnd, SW_SHOWNORMAL);
        if let Some(&first) = edit_controls.first() {
            SetFocus(first);
            SendMessageW(first, EM_SETSEL, 0, -1);
        } else if download && let Some(&(_, cancel)) = buttons.first() {
            SetFocus(cancel);
        } else {
            SetFocus(hwnd);
        }
    }

    let focusable: Vec<_> = edit_controls
        .iter()
        .copied()
        .chain(buttons.iter().map(|&(_, button)| button))
        .collect();
    let mut message = MSG::default();
    let mut context = None;
    while state.answer.get().is_none() {
        // SAFETY: This is a short UI-thread popup loop. A quit message is forwarded to the outer loop.
        match unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } {
            -1 => return Err(io::Error::last_os_error()),
            0 => {
                unsafe { PostQuitMessage(message.wParam as i32) };
                break;
            }
            _ => {}
        }
        if matches!(content, Content::Menu { context: true })
            && let Some((id, point)) = context_request(hwnd, &message, &buttons)
        {
            state.answer.set(Some(id));
            context = Some(point);
            continue;
        }
        if message.message == WM_KEYDOWN
            && handle_key(&state, &focusable, &buttons, &edit_controls, message.wParam)
        {
            continue;
        }
        if message.message == WM_MOUSEMOVE {
            let hovered = buttons
                .iter()
                .find(|&&(_, button)| button == message.hwnd)
                .map_or(null_mut(), |&(_, button)| button);
            let previous = state.hovered.replace(hovered);
            if previous != hovered {
                // SAFETY: Only the popup's live row HWNDs are invalidated.
                unsafe {
                    if !previous.is_null() {
                        InvalidateRect(previous, null_mut(), 0);
                    }
                    if !hovered.is_null() {
                        InvalidateRect(hovered, null_mut(), 0);
                    }
                }
            }
        }
        // SAFETY: The message comes from this thread's queue and can be dispatched normally.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    let answer = state.answer.get().unwrap_or(0);
    let values = if answer == 1 {
        edit_controls.into_iter().map(chrome::get_text).collect()
    } else {
        Vec::new()
    };
    Ok(Choice {
        id: answer,
        fields: values,
        context,
    })
}

fn context_request(menu: HWND, message: &MSG, buttons: &[(u16, HWND)]) -> Option<(u16, POINT)> {
    // SAFETY: The queue message and button handles come from the active popup loop.
    unsafe {
        if message.message == WM_RBUTTONUP {
            if message.hwnd == menu {
                return Some((0, message.pt));
            }
            return buttons
                .iter()
                .find(|&&(_, button)| button == message.hwnd)
                .map(|&(id, _)| (id, message.pt));
        }
        if message.message == WM_KEYDOWN
            && (message.wParam == usize::from(VK_APPS)
                || message.wParam == usize::from(VK_F10) && GetKeyState(VK_SHIFT as i32) < 0)
        {
            let focused = GetFocus();
            let &(id, _) = buttons.iter().find(|&&(_, control)| control == focused)?;
            let mut bounds = RECT::default();
            if GetWindowRect(focused, &mut bounds) != 0 {
                return Some((
                    id,
                    POINT {
                        x: bounds.left,
                        y: bounds.bottom,
                    },
                ));
            }
        }
        None
    }
}

fn handle_key(
    state: &State<'_>,
    focusable: &[HWND],
    buttons: &[(u16, HWND)],
    edits: &[HWND],
    key: WPARAM,
) -> bool {
    // SAFETY: The menu owns the live controls on this thread.
    let focused = unsafe { GetFocus() };
    let editing = edits.contains(&focused);
    let current = focusable.iter().position(|&hwnd| hwnd == focused);
    let last = focusable.len().saturating_sub(1);
    let key = key as u16;
    let next = match key {
        VK_ESCAPE => {
            state.answer.set(Some(0));
            return true;
        }
        VK_RETURN => {
            let command = buttons
                .iter()
                .find(|&&(_, button)| button == focused)
                .or_else(|| buttons.first());
            state.answer.set(command.map(|&(id, _)| id));
            return true;
        }
        _ if focusable.is_empty() => return false,
        VK_TAB if unsafe { GetKeyState(VK_SHIFT as i32) } < 0 => {
            current.map_or(last, |i| if i == 0 { last } else { i - 1 })
        }
        VK_TAB | VK_DOWN => current.map_or(0, |i| (i + 1) % focusable.len()),
        VK_UP => current.map_or(last, |i| if i == 0 { last } else { i - 1 }),
        VK_HOME if !editing => 0,
        VK_END if !editing => last,
        _ => return false,
    };
    // SAFETY: The index comes from focusable's own length and the control is live.
    unsafe { SetFocus(focusable[next]) };
    true
}

fn window_class() -> io::Result<&'static [u16]> {
    static CLASS: OnceLock<Option<Vec<u16>>> = OnceLock::new();
    CLASS
        .get_or_init(|| {
            let name = chrome::wide("Brisa.ChromeMenu");
            // SAFETY: Windows copies the descriptor and name during registration.
            let registered = unsafe {
                RegisterClassW(&WNDCLASSW {
                    style: CS_DROPSHADOW,
                    lpfnWndProc: Some(window_proc),
                    hInstance: GetModuleHandleW(null_mut()),
                    hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                    lpszClassName: name.as_ptr(),
                    ..Default::default()
                })
            };
            (registered != 0).then_some(name)
        })
        .as_deref()
        .ok_or_else(|| io::Error::other("Não foi possível preparar o menu do Brisa."))
}

fn child(
    hwnd: HWND,
    class: &str,
    caption: &str,
    style: u32,
    id: u16,
    rect: RECT,
    font: HFONT,
) -> io::Result<HWND> {
    let class = chrome::wide(class);
    let caption = chrome::wide(caption);
    // SAFETY: The parent owns the new control and Win32 copies the strings.
    let control = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            caption.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            hwnd,
            id as HMENU,
            GetModuleHandleW(null_mut()),
            null_mut(),
        )
    };
    if control.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: The popup's font lives until after all of the child controls are destroyed.
    unsafe { SendMessageW(control, WM_SETFONT, font as WPARAM, 0) };
    Ok(control)
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: WM_NCCREATE supplies a CREATESTRUCTW. The MenuWindow guard
    // invalidates the HWND before the creating scope can release State.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let Some(state) = (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State<'_>).as_ref()
        else {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        };
        match message {
            WM_CLOSE => state.answer.set(Some(0)),
            WM_ACTIVATE if wparam & 0xffff == WA_INACTIVE as usize => state.answer.set(Some(0)),
            WM_COMMAND if (wparam >> 16) & 0xffff == BN_CLICKED as usize => {
                if lparam != 0
                    && let Some(item) = state
                        .items
                        .iter()
                        .find(|item| item.id != 0 && item.id == wparam as u16)
                {
                    state.answer.set(Some(item.id));
                }
            }
            WM_ERASEBKGND => return 1,
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
                let dc = wparam as HDC;
                let edit = message == WM_CTLCOLOREDIT;
                SetTextColor(
                    dc,
                    if GetDlgCtrlID(lparam as HWND) == i32::from(DOWNLOAD_DETAIL) {
                        state.palette.muted
                    } else {
                        state.palette.ink
                    },
                );
                SetBkColor(
                    dc,
                    if edit {
                        state.palette.address
                    } else {
                        state.palette.surface
                    },
                );
                return if edit {
                    state.paint.input
                } else {
                    state.paint.background
                } as LRESULT;
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                if !dc.is_null() {
                    FillRect(dc, &paint.rcPaint, state.paint.background);
                    if state.download {
                        let px = |value| scaled(value, state.dpi);
                        Canvas::with_dc(dc, |canvas| {
                            canvas.rounded(
                                RECT {
                                    left: px(16),
                                    top: px(12),
                                    right: px(52),
                                    bottom: px(48),
                                },
                                px(10) as f32,
                                state.palette.address,
                            );
                            canvas.icon(
                                Icon::Download,
                                px(24) as f32,
                                px(20) as f32,
                                px(20) as f32,
                                state.palette.accent,
                            );
                        });
                    }
                }
                EndPaint(hwnd, &paint);
            }
            WM_DRAWITEM if lparam != 0 => {
                let draw = &*(lparam as *const DRAWITEMSTRUCT);
                if let Some(item) = state
                    .items
                    .iter()
                    .find(|item| u32::from(item.id) == draw.CtlID)
                {
                    draw_row(state, draw, item);
                    return 1;
                }
            }
            _ => return DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
    0
}

fn draw_row(state: &State<'_>, draw: &DRAWITEMSTRUCT, item: &Item) {
    let dc = draw.hDC;
    let rect = draw.rcItem;
    let theme = state.palette;
    let px = |value| scaled(value, state.dpi);
    let active =
        state.hovered.get() == draw.hwndItem || draw.itemState & (ODS_FOCUS | ODS_SELECTED) != 0;
    // SAFETY: The owner-draw DC and borrowed resources stay live through this paint.
    unsafe { FillRect(dc, &rect, state.paint.background) };
    if state.download {
        Canvas::with_dc(dc, |canvas| {
            let shape = RECT {
                left: rect.left + px(1),
                top: rect.top + px(1),
                right: rect.right - px(1),
                bottom: rect.bottom - px(1),
            };
            canvas.rounded(
                shape,
                px(8) as f32,
                if item.id == 1 {
                    theme.accent
                } else if active {
                    theme.hover
                } else {
                    theme.address
                },
            );
        });
        let color = if item.id == 1 {
            theme.surface
        } else {
            theme.ink
        };
        chrome::draw_text(dc, state.paint.font, rect, &item.title, color, DT_CENTER);
        if draw.itemState & ODS_FOCUS != 0 {
            let focus = RECT {
                left: rect.left + px(4),
                top: rect.top + px(4),
                right: rect.right - px(4),
                bottom: rect.bottom - px(4),
            };
            // SAFETY: The focus bounds and paint DC belong to this button.
            unsafe { DrawFocusRect(dc, &focus) };
        }
        return;
    }
    Canvas::with_dc(dc, |canvas| {
        if active {
            canvas.rounded(rect, px(7) as f32, theme.hover);
        }
        if let Some(icon) = item.icon.filter(|_| item.favicon.is_none()) {
            canvas.icon(
                icon,
                (rect.left + px(10)) as f32,
                (rect.top + (rect.bottom - rect.top - px(17)) / 2) as f32,
                px(17) as f32,
                theme.accent,
            );
        }
    });
    if let Some(icon) = &item.favicon {
        let area = RECT {
            left: rect.left + px(10),
            right: rect.left + px(27),
            ..rect
        };
        icon.draw(dc, area, px(17));
    }
    let font = state.paint.font;
    // SAFETY: Restore the original font before returning the borrowed DC.
    unsafe {
        let old = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        let text = chrome::wide(&item.title);
        let mut label = rect;
        label.left += px(if item.icon.is_some() || item.favicon.is_some() {
            36
        } else {
            12
        });
        label.right -= px(if item.hint.is_empty() { 12 } else { 108 });
        SetTextColor(dc, theme.ink);
        DrawTextW(
            dc,
            text.as_ptr(),
            -1,
            &mut label,
            DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        if !item.hint.is_empty() {
            let hint = chrome::wide(&item.hint);
            let mut bounds = rect;
            bounds.left = label.right + px(4);
            bounds.right -= px(12);
            SetTextColor(dc, theme.muted);
            DrawTextW(
                dc,
                hint.as_ptr(),
                -1,
                &mut bounds,
                DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
        SelectObject(dc, old);
    }
}
