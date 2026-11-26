use std::{
    cell::{Cell, RefCell},
    io,
    ptr::null_mut,
    rc::Rc,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DwmSetWindowAttribute},
        Gdi::*,
    },
    UI::{
        Controls::*,
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{
            GetCapture, GetKeyState, ReleaseCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT,
            TrackMouseEvent, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_F12, VK_HOME, VK_LEFT, VK_MENU,
            VK_RETURN, VK_RIGHT, VK_SHIFT, VK_TAB, VK_UP,
        },
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

use crate::{
    chrome_draw::{Canvas, DrawingSession, Icon},
    favicon::Favicon,
    profile::{Accent, Theme},
};

pub const BACK: u16 = 101;
pub const FORWARD: u16 = 102;
pub const RELOAD: u16 = 103;
pub const OPEN: u16 = 104;
pub const NEW_TAB: u16 = 105;
pub const CLOSE_TAB: u16 = 106;
pub const MARK: u16 = 107;
pub const BOOKMARKS: u16 = 108;
pub const MENU: u16 = 109;
pub const PROTECTION: u16 = 110;
pub const FOCUS_ADDRESS: u16 = 111;
pub const INSPECT: u16 = 112;
pub const REOPEN: u16 = 113;
pub const HISTORY: u16 = 114;
pub const SETTINGS: u16 = 115;
pub const TAB_LIST: u16 = 116;
pub const NEW_PRIVATE: u16 = 117;
pub const NEXT_TAB: u16 = 118;
pub const PREVIOUS_TAB: u16 = 119;
pub const PIN_FIRST: u16 = 120;
pub const BOOKMARK_DRAG: u32 = WM_APP + 11;
pub const BOOKMARK_DROP: u32 = WM_APP + 12;
pub const BOOKMARK_DRAG_CANCEL: u32 = WM_APP + 13;
pub const TAB_DROP: u32 = WM_APP + 15;
pub const TAB_DRAG: u32 = WM_APP + 16;
pub const TAB_DRAG_CANCEL: u32 = WM_APP + 17;
pub const LOADING_TIMER: usize = 2;
const SPINNER_DELAY: Duration = Duration::from_millis(100);
const ADDRESS: u16 = 130;
pub const GOTO_HOME: u16 = 131;
pub const TOGGLE_BOOKMARKS: u16 = 132;
pub const MINIMIZE_WINDOW: u16 = 133;
pub const MAXIMIZE_WINDOW: u16 = 134;
pub const CLOSE_WINDOW: u16 = 135;
pub const BLOCK_REPORT: u16 = 136;
pub const ZOOM_LEVEL: u16 = 137;
pub const INSPECT_LEFT: u16 = 138;
pub const INSPECT_BOTTOM: u16 = 139;
pub const INSPECT_RIGHT: u16 = 140;
pub const INSPECT_UNDOCK: u16 = 141;
pub const INSPECT_CLOSE: u16 = 142;
pub const DOWNLOADS: u16 = 143;
const INSPECT_BUTTONS: [(&str, u16); 5] = [
    ("Encaixar DevTools à esquerda", INSPECT_LEFT),
    ("Encaixar DevTools embaixo", INSPECT_BOTTOM),
    ("Encaixar DevTools à direita", INSPECT_RIGHT),
    ("Abrir DevTools em janela separada", INSPECT_UNDOCK),
    ("Fechar DevTools", INSPECT_CLOSE),
];
pub const BOOKMARK_MANAGER: u16 = 200;
pub const MAX_TABS: usize = 32;
const TAB_LABEL_FIRST: u16 = 500;
const TAB_CLOSE_FIRST: u16 = 600;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BookmarkDrop {
    Before(usize),
    After(usize),
    Into(usize),
    End,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub surface: u32,
    pub address: u32,
    pub ink: u32,
    pub muted: u32,
    pub band: u32,
    pub active: u32,
    pub tab_ink: u32,
    pub hover: u32,
    pub accent: u32,
    divider: u32,
    warning: u32,
}

impl Palette {
    pub fn new(theme: Theme, accent: Accent) -> Self {
        let mut palette = match theme {
            Theme::Light => Self {
                surface: rgb(248, 250, 249),
                address: rgb(233, 240, 238),
                ink: rgb(26, 45, 44),
                muted: rgb(77, 96, 93),
                band: rgb(223, 234, 231),
                active: rgb(248, 250, 249),
                tab_ink: rgb(38, 64, 61),
                hover: rgb(214, 230, 225),
                divider: rgb(208, 221, 217),
                warning: rgb(151, 68, 10),
                accent: 0,
            },
            Theme::Dark => Self {
                surface: rgb(29, 42, 42),
                address: rgb(19, 31, 32),
                ink: rgb(238, 246, 244),
                muted: rgb(174, 198, 192),
                band: rgb(17, 28, 30),
                active: rgb(40, 56, 56),
                tab_ink: rgb(222, 235, 232),
                hover: rgb(54, 75, 73),
                divider: rgb(55, 76, 73),
                warning: rgb(255, 176, 125),
                accent: 0,
            },
        };
        palette.accent = match (theme, accent) {
            (Theme::Light, Accent::Jade) => rgb(0, 105, 87),
            (Theme::Light, Accent::Blue) => rgb(21, 86, 166),
            (Theme::Light, Accent::Violet) => rgb(101, 63, 161),
            (Theme::Dark, Accent::Jade) => rgb(147, 230, 204),
            (Theme::Dark, Accent::Blue) => rgb(153, 204, 255),
            (Theme::Dark, Accent::Violet) => rgb(212, 185, 255),
        };
        palette
    }
}
const TAB_HEIGHT: i32 = 44;
const NAV_HEIGHT: i32 = 97;
const BOOKMARK_HEIGHT: i32 = 33;

#[derive(Clone, Copy)]
pub enum TabAction {
    Select(usize),
    Close(usize),
}

struct TabButton {
    slot: usize,
    title: HWND,
    close: HWND,
    pinned: bool,
    loading: Cell<Option<Instant>>,
    spinner: Cell<bool>,
    icon: RefCell<Option<Rc<Favicon>>>,
    tip: RefCell<Vec<u16>>,
}

#[derive(Default)]
struct BookmarkGlyph {
    folder: bool,
    stored: Option<String>,
    icon: Option<Favicon>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Geometry {
    width: i32,
    first_tab: usize,
    visible_tabs: usize,
    first_pinned: usize,
    visible_pinned: usize,
    bookmarks: bool,
    visible_pins: usize,
    pin_widths: [i32; 3],
}

struct ControlPlacement {
    hwnd: HWND,
    bounds: Option<(i32, i32, i32, i32)>,
}

pub struct Toolbar {
    parent: HWND,
    pub address: HWND,
    back: HWND,
    forward: HWND,
    reload: HWND,
    downloads: HWND,
    home: HWND,
    open: HWND,
    new_tab: HWND,
    tab_list: HWND,
    mark: HWND,
    pub zoom: HWND,
    zoom_percent: Cell<u16>,
    inspect_controls: [HWND; 5],
    inspect_area: Cell<Option<(i32, i32, i32, u16)>>,
    bookmarks: HWND,
    pins: [HWND; 3],
    bookmark_glyphs: [RefCell<BookmarkGlyph>; 3],
    bookmark_drop: Cell<Option<BookmarkDrop>>,
    menu: HWND,
    protection: HWND,
    status: HWND,
    caption: [HWND; 3],
    app_icon: HICON,
    tabs: Vec<TabButton>,
    tab_drop: Cell<Option<(HWND, bool)>>,
    selected: Cell<usize>,
    loading_timer: Cell<bool>,
    animate_motion: bool,
    update_available: Cell<bool>,
    geometry: Cell<Option<Geometry>>,
    bookmark_bar: Cell<bool>,
    marked: Cell<bool>,
    warning: Cell<bool>,
    tooltip: HWND,
    protection_tip: RefCell<Vec<u16>>,
    tooltip_text: Vec<Vec<u16>>,
    colors: Rc<ControlBrushes>,
    palette: Palette,
    font: HFONT,
    dpi: i32,
    _drawing: DrawingSession,
}

impl Toolbar {
    pub fn new(parent: HWND, colors: Rc<ControlBrushes>) -> io::Result<Self> {
        let palette = colors.palette.get();
        let control_classes = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_WIN95_CLASSES,
        };
        // SAFETY: The descriptor remains valid through common controls initialization.
        if unsafe { InitCommonControlsEx(&control_classes) } == 0 {
            return Err(ui_error("Inicializar controles comuns"));
        }
        let drawing = DrawingSession::start()?;
        // SAFETY: parent is the live top-level window and the system returns its monitor DPI.
        let dpi = unsafe { GetDpiForWindow(parent) }.max(96) as i32;
        let font = create_font(dpi);
        let mut resources = FontGuard(font);
        if font.is_null() {
            return Err(ui_error("Criar fonte"));
        }
        // SAFETY: Both handles belong to this live window and the OS never retains the data pointer.
        unsafe {
            SetWindowLongPtrW(
                parent,
                GWL_STYLE,
                GetWindowLongPtrW(parent, GWL_STYLE) | WS_CLIPCHILDREN as isize,
            );
            for (field, color) in [
                (DWMWA_CAPTION_COLOR, palette.band),
                (DWMWA_TEXT_COLOR, palette.tab_ink),
                (DWMWA_BORDER_COLOR, palette.band),
            ] {
                let _ = DwmSetWindowAttribute(
                    parent,
                    field as u32,
                    (&raw const color).cast(),
                    std::mem::size_of::<u32>() as u32,
                );
            }
        }
        let button = |caption, id| button(parent, caption, id, font);
        let mut toolbar = Self {
            parent,
            address: control(
                parent,
                "EDIT",
                "",
                WS_TABSTOP | ES_AUTOHSCROLL as u32,
                ADDRESS,
                font,
            )?,
            back: button("Voltar", BACK)?,
            forward: button("Avançar", FORWARD)?,
            reload: button("Recarregar", RELOAD)?,
            downloads: button("Downloads", DOWNLOADS)?,
            home: button("Página inicial", GOTO_HOME)?,
            open: button("Abrir endereço", OPEN)?,
            new_tab: button("Nova aba", NEW_TAB)?,
            tab_list: button("Listar abas", TAB_LIST)?,
            mark: button("Salvar marcador", MARK)?,
            zoom: button("Zoom da página: 100%", ZOOM_LEVEL)?,
            zoom_percent: Cell::new(100),
            inspect_controls: [
                button(INSPECT_BUTTONS[0].0, INSPECT_BUTTONS[0].1)?,
                button(INSPECT_BUTTONS[1].0, INSPECT_BUTTONS[1].1)?,
                button(INSPECT_BUTTONS[2].0, INSPECT_BUTTONS[2].1)?,
                button(INSPECT_BUTTONS[3].0, INSPECT_BUTTONS[3].1)?,
                button(INSPECT_BUTTONS[4].0, INSPECT_BUTTONS[4].1)?,
            ],
            inspect_area: Cell::new(None),
            bookmarks: button("Marcadores", BOOKMARKS)?,
            pins: [
                button("", PIN_FIRST)?,
                button("", PIN_FIRST + 1)?,
                button("", PIN_FIRST + 2)?,
            ],
            bookmark_glyphs: Default::default(),
            bookmark_drop: Cell::new(None),
            menu: button("Menu do Brisa", MENU)?,
            protection: button("Proteção da página", PROTECTION)?,
            status: button("Ver bloqueios", BLOCK_REPORT)?,
            caption: [
                button("Minimizar", MINIMIZE_WINDOW)?,
                button("Maximizar ou restaurar", MAXIMIZE_WINDOW)?,
                button("Fechar janela", CLOSE_WINDOW)?,
            ],
            app_icon: crate::frame::app_icon(),
            tabs: Vec::new(),
            tab_drop: Cell::new(None),
            selected: Cell::new(0),
            loading_timer: Cell::new(false),
            animate_motion: client_animations(),
            update_available: Cell::new(false),
            geometry: Cell::new(None),
            bookmark_bar: Cell::new(true),
            marked: Cell::new(false),
            warning: Cell::new(false),
            tooltip: null_mut(),
            protection_tip: RefCell::new(wide("Proteção desta página")),
            tooltip_text: Vec::new(),
            colors,
            palette,
            font,
            dpi,
            _drawing: drawing,
        };
        resources.0 = null_mut();
        for hwnd in toolbar.keyboard_controls() {
            subclass(hwnd, parent)?;
        }
        for hwnd in toolbar.inspect_controls {
            // SAFETY: These controls are live children and stay owned by this toolbar.
            unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
        toolbar.install_tooltips();
        Ok(toolbar)
    }

    pub fn palette(&self) -> Palette {
        self.palette
    }

    pub fn search_hint(&self, engine: crate::protection::SearchEngine) {
        let hint = wide(&format!(
            "Pesquise no {} ou digite um endereço",
            engine.name()
        ));
        // SAFETY: The edit control copies the cue string synchronously.
        unsafe { SendMessageW(self.address, EM_SETCUEBANNER, 0, hint.as_ptr() as LPARAM) };
    }

    pub fn set_palette(&mut self, palette: Palette) -> io::Result<()> {
        if self.palette == palette {
            return Ok(());
        }
        self.colors.set_palette(palette)?;
        if self.palette.surface != palette.surface {
            // SAFETY: The live HWND belongs to this UI thread and DWM copies the color.
            unsafe {
                let _ = DwmSetWindowAttribute(
                    self.parent,
                    DWMWA_BORDER_COLOR as u32,
                    (&raw const palette.band).cast(),
                    std::mem::size_of::<u32>() as u32,
                );
            }
        }
        self.palette = palette;
        self.repaint_all();
        Ok(())
    }
    fn install_tooltips(&mut self) {
        // SAFETY: The popup tooltip is owned by parent and remains on this UI thread.
        self.tooltip = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                TOOLTIPS_CLASSW,
                null_mut(),
                WS_POPUP | TTS_ALWAYSTIP,
                0,
                0,
                0,
                0,
                self.parent,
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if self.tooltip.is_null() {
            return;
        }
        for (hwnd, message) in [
            (self.back, "Voltar (Alt+←)"),
            (self.forward, "Avançar (Alt+→)"),
            (self.reload, "Recarregar (Ctrl+R)"),
            (self.downloads, "Downloads (Ctrl+J)"),
            (self.home, "Página inicial (Alt+Home)"),
            (self.open, "Abrir endereço (Enter)"),
            (self.new_tab, "Nova aba (Ctrl+T)"),
            (self.tab_list, "Ver todas as abas"),
            (self.mark, "Salvar ou remover marcador (Ctrl+D)"),
            (self.menu, "Menu do Brisa"),
            (self.status, "Ver os itens bloqueados nesta aba"),
            (self.zoom, "Zoom da página"),
        ]
        .into_iter()
        .chain(
            self.inspect_controls
                .into_iter()
                .zip(INSPECT_BUTTONS.map(|(title, _)| title)),
        ) {
            let mut text = wide(message);
            self.control_tip(hwnd, &mut text, TTM_ADDTOOLW);
            self.tooltip_text.push(text);
        }
        self.control_tip(
            self.protection,
            &mut self.protection_tip.borrow_mut(),
            TTM_ADDTOOLW,
        );
        // SAFETY: The tooltip is live and setting its wrap width requires no pointer.
        unsafe {
            SendMessageW(
                self.tooltip,
                TTM_SETMAXTIPWIDTH,
                0,
                scale(340, self.dpi) as LPARAM,
            )
        };
    }

    fn keyboard_controls(&self) -> Vec<HWND> {
        self.tabs
            .iter()
            .flat_map(|tab| [tab.title, tab.close])
            .chain([
                self.new_tab,
                self.tab_list,
                self.back,
                self.forward,
                self.reload,
                self.home,
                self.protection,
                self.address,
                self.zoom,
                self.mark,
                self.open,
                self.downloads,
                self.menu,
                self.bookmarks,
                self.status,
                self.pins[0],
                self.pins[1],
                self.pins[2],
                self.caption[0],
                self.caption[1],
                self.caption[2],
            ])
            .chain(self.inspect_controls)
            .collect()
    }

    pub fn tab_focus(&self, current: HWND, backwards: bool) -> bool {
        let order = self.keyboard_controls();
        let Some(index) = order.iter().position(|handle| *handle == current) else {
            return false;
        };
        let next = if backwards {
            order[..index].iter().rev().find(|hwnd| visible(**hwnd))
        } else {
            order[index + 1..].iter().find(|hwnd| visible(**hwnd))
        };
        if let Some(hwnd) = next {
            // SAFETY: A visible control from this window can receive UI-thread focus.
            unsafe { SetFocus(*hwnd) };
            return true;
        }
        false
    }

    pub fn layout(&self, width: i32, show_bookmarks: bool, titles: &[String]) {
        self.bookmark_bar.set(show_bookmarks);
        let d = self.dpi;
        let width = (width.saturating_mul(96) / d).max(480);
        let right = width - 12;
        let available = right - 250;
        let pinned = self.tabs.iter().take_while(|tab| tab.pinned).count();
        let ordinary = self.tabs.len() - pinned;
        let pinned_width = 43;
        let reserved = if ordinary > 0 { 132 } else { 0 };
        let pinned_visible = (((available - reserved) / pinned_width).max(0) as usize).min(pinned);
        let pinned_first = self
            .selected
            .get()
            .min(pinned.saturating_sub(1))
            .saturating_sub(pinned_visible.saturating_sub(1))
            .min(pinned.saturating_sub(pinned_visible));
        let pinned_extent = pinned_visible as i32 * pinned_width;
        let available = available - pinned_extent;
        let count = ((available / 132).max(1) as usize).min(ordinary);
        let start = self
            .selected
            .get()
            .saturating_sub(count.saturating_sub(1))
            .clamp(pinned, self.tabs.len().saturating_sub(count));
        let report_width = 212;
        let pin_area = right - report_width - 147;
        let pin_widths = self.bookmark_widths(titles, if show_bookmarks { pin_area } else { 0 });
        let visible_pins = pin_widths.iter().take_while(|&&width| width > 0).count();
        for (&pin, title) in self.pins.iter().zip(titles).take(visible_pins) {
            set_text(pin, title);
        }
        let geometry = Geometry {
            width,
            first_tab: start,
            visible_tabs: count,
            first_pinned: pinned_first,
            visible_pinned: pinned_visible,
            bookmarks: show_bookmarks,
            visible_pins,
            pin_widths,
        };
        if self.geometry.get() == Some(geometry) {
            return;
        }
        let tab_width = (available / count.max(1) as i32).clamp(124, 220);
        let mut controls = Vec::with_capacity(self.tabs.len() * 2 + 19);
        let mut place = |hwnd, x, y, w, h, shown: bool| {
            controls.push(ControlPlacement {
                hwnd,
                bounds: shown.then(|| (scale(x, d), scale(y, d), scale(w, d), scale(h, d))),
            });
        };
        for (index, tab) in self.tabs.iter().enumerate() {
            if tab.pinned {
                let shown = (pinned_first..pinned_first + pinned_visible).contains(&index);
                let x = 44 + index.saturating_sub(pinned_first) as i32 * pinned_width;
                place(tab.title, x, 5, pinned_width - 3, 35, shown);
                place(tab.close, x, 5, 0, 0, false);
            } else {
                let shown = (start..start + count).contains(&index);
                let x = 44 + pinned_extent + index.saturating_sub(start) as i32 * tab_width;
                place(tab.title, x, 5, tab_width - 34, 35, shown);
                place(tab.close, x + tab_width - 34, 5, 29, 35, shown);
            }
        }
        let new_x = 46 + pinned_extent + count as i32 * tab_width;
        place(self.new_tab, new_x, 5, 35, 35, true);
        place(self.tab_list, right - 159, 5, 34, 35, true);
        for (index, &caption) in self.caption.iter().enumerate() {
            place(caption, width - 123 + index as i32 * 40, 6, 38, 32, true);
        }

        place(self.back, 10, 53, 34, 34, true);
        place(self.forward, 46, 53, 34, 34, true);
        place(self.reload, 82, 53, 34, 34, true);
        place(self.home, 118, 53, 34, 34, true);
        place(self.protection, 166, 55, 31, 31, true);
        place(self.address, 202, 61, right - 435, 22, true);
        place(self.zoom, right - 229, 55, 76, 31, true);
        place(self.mark, right - 147, 55, 32, 31, true);
        place(self.open, right - 112, 55, 30, 31, true);
        place(self.downloads, right - 76, 55, 34, 31, true);
        place(self.menu, right - 36, 53, 36, 34, true);

        place(
            self.status,
            right - report_width,
            99,
            report_width,
            28,
            show_bookmarks,
        );
        place(self.bookmarks, 12, 99, 121, 28, show_bookmarks);
        let mut pin_x = 137;
        for (&pin, pin_width) in self.pins.iter().zip(pin_widths) {
            place(pin, pin_x, 99, pin_width, 28, pin_width > 0);
            pin_x += pin_width + 4;
        }
        if position_controls(&controls) {
            self.geometry.set(Some(geometry));
        }
        self.repaint_all();
    }

    fn bookmark_widths(&self, titles: &[String], mut remaining: i32) -> [i32; 3] {
        let mut widths = [0; 3];
        if remaining < 42 || titles.is_empty() {
            return widths;
        }
        // SAFETY: The DC and font belong to this toolbar. The original GDI object
        // is restored before releasing the DC.
        unsafe {
            let dc = GetDC(self.parent);
            if dc.is_null() {
                return widths;
            }
            let previous = SelectObject(dc, self.font);
            for (width, title) in widths.iter_mut().zip(titles) {
                if remaining < 42 {
                    break;
                }
                let text = wide(title);
                let mut extent = windows_sys::Win32::Foundation::SIZE::default();
                GetTextExtentPoint32W(dc, text.as_ptr(), (text.len() - 1) as i32, &mut extent);
                *width = (extent.cx * 96 / self.dpi + 34)
                    .clamp(42, 170)
                    .min(remaining);
                remaining -= *width + 4;
            }
            SelectObject(dc, previous);
            ReleaseDC(self.parent, dc);
        }
        widths
    }

    pub fn update_zoom(&self, factor: f64) {
        let percent = (factor * 100.0).round() as u16;
        if self.zoom_percent.replace(percent) != percent {
            set_text(self.zoom, &format!("Zoom da página: {percent}%"));
            repaint(self.zoom);
        }
    }

    pub fn bookmark_icon(&self, index: usize, folder: bool, stored: Option<&str>) {
        let Some(glyph) = self.bookmark_glyphs.get(index) else {
            return;
        };
        let changed = {
            let mut glyph = glyph.borrow_mut();
            let mut changed = glyph.folder != folder;
            glyph.folder = folder;
            if glyph.stored.as_deref() != stored {
                glyph.icon = stored.and_then(Favicon::from_stored);
                glyph.stored = stored.map(str::to_owned);
                changed = true;
            }
            changed
        };
        if changed {
            repaint(self.pins[index]);
        }
    }

    pub fn inspector_header(&self, area: Option<RECT>, side: u16) -> Option<RECT> {
        let bounds = area.map(|area| (area.right, area.left, area.top, side));
        let height = scale(32, self.dpi);
        let extent = scale(30, self.dpi);
        if bounds != self.inspect_area.get() {
            let controls = self
                .inspect_controls
                .iter()
                .enumerate()
                .map(|(index, &hwnd)| ControlPlacement {
                    hwnd,
                    bounds: area.map(|area| {
                        (
                            area.right - scale(5, self.dpi) - extent * (5 - index as i32),
                            area.top + scale(2, self.dpi),
                            extent,
                            height - scale(4, self.dpi),
                        )
                    }),
                })
                .collect::<Vec<_>>();
            if position_controls(&controls) {
                self.inspect_area.set(bounds);
                for control in self.inspect_controls {
                    repaint(control);
                }
            }
        }
        area.map(|area| RECT {
            top: (area.top + height).min(area.bottom),
            ..area
        })
    }

    pub fn height(&self) -> i32 {
        self.height_with_bookmarks(self.bookmark_bar.get())
    }

    pub fn height_with_bookmarks(&self, visible: bool) -> i32 {
        let bookmarks = if visible { BOOKMARK_HEIGHT } else { 0 };
        scale(NAV_HEIGHT + bookmarks, self.dpi)
    }

    pub fn set_dpi(&mut self, dpi: i32) -> io::Result<()> {
        if self.dpi == dpi {
            return Ok(());
        }
        let font = create_font(dpi);
        if font.is_null() {
            return Err(ui_error("Ajustar fonte para a tela"));
        }
        let previous = std::mem::replace(&mut self.font, font);
        self.dpi = dpi;
        self.geometry.set(None);
        self.inspect_area.set(None);
        // SAFETY: These handles are live toolbar controls. The new font
        // remains owned by the toolbar and every control releases the old one.
        unsafe {
            for hwnd in self.keyboard_controls() {
                SendMessageW(hwnd, WM_SETFONT, font as WPARAM, 0);
            }
            DeleteObject(previous);
        }
        Ok(())
    }

    pub fn repaint_all(&self) {
        let mut rect = RECT::default();
        // SAFETY: The toolbar owns the live parent window and the rectangle is valid through each call.
        unsafe {
            GetClientRect(self.parent, &mut rect);
            rect.bottom = rect.bottom.min(self.height());
            RedrawWindow(
                self.parent,
                &rect,
                null_mut(),
                RDW_INVALIDATE | RDW_ALLCHILDREN,
            );
        }
    }

    pub fn paint_background(&self, dc: HDC) {
        let mut rect = RECT::default();
        // SAFETY: The erase-background message lends a live window and a DC.
        unsafe { GetClientRect(self.parent, &mut rect) };
        let content_top = rect.bottom.min(self.height());
        // Child windows are clipped out. Fill the theme color wherever the
        // old or new renderer temporarily leaves the parent exposed.
        fill(dc, rect, self.palette.surface);
        rect.bottom = content_top;
        rect.bottom = rect.bottom.min(scale(TAB_HEIGHT, self.dpi));
        fill(dc, rect, self.palette.band);
        let s = |value| scale(value, self.dpi);
        if !self.app_icon.is_null() {
            // SAFETY: The icon is shared from the executable and the paint DC is live.
            unsafe {
                DrawIconEx(
                    dc,
                    s(11),
                    s(11),
                    self.app_icon,
                    s(22),
                    s(22),
                    0,
                    null_mut(),
                    DI_NORMAL,
                )
            };
        }
        Canvas::with_dc(dc, |canvas| {
            canvas.rounded(
                RECT {
                    left: s(160),
                    top: s(51),
                    right: rect.right - s(54),
                    bottom: s(89),
                },
                s(19) as f32,
                self.palette.address,
            )
        });
        if self.bookmark_bar.get() {
            fill(
                dc,
                RECT {
                    left: s(12),
                    top: s(96),
                    right: rect.right - s(12),
                    bottom: s(97),
                },
                self.palette.divider,
            );
        }
        fill(
            dc,
            RECT {
                left: 0,
                top: content_top - 1,
                right: rect.right,
                bottom: content_top,
            },
            self.palette.divider,
        );
    }

    pub fn draw(&self, item: &DRAWITEMSTRUCT) {
        if item.CtlType != ODT_BUTTON {
            return;
        }
        let id = item.CtlID as u16;
        let rect = item.rcItem;
        let s = |value| scale(value, self.dpi);
        let hot = cursor_over(item.hwndItem) || item.itemState & ODS_SELECTED != 0;
        let focused = item.itemState & ODS_FOCUS != 0;
        if let Some(action) = self.tab_action(id) {
            let (index, is_close) = match action {
                TabAction::Close(i) => (i, true),
                TabAction::Select(i) => (i, false),
            };
            if let Some(tab) = self.tabs.get(index) {
                let active = self.selected.get() == index;
                buffered(item.hDC, rect, |dc, rect| {
                    fill(dc, rect, self.palette.band);
                    let mut body = rect;
                    if is_close {
                        body.left -= client_width(tab.title);
                    } else if !tab.pinned {
                        body.right += client_width(tab.close);
                    }
                    let tile = if tab.pinned {
                        rect
                    } else {
                        RECT {
                            left: rect.left + s(9),
                            right: rect.left + s(34),
                            top: rect.top + s(5),
                            bottom: rect.bottom - s(5),
                        }
                    };
                    let favicon = tab.icon.borrow();
                    Canvas::with_dc(dc, |canvas| {
                        if active {
                            canvas.rounded(body, s(10) as f32, self.palette.active);
                        } else if hot && !is_close {
                            canvas.rounded(inset(rect, s(2)), s(8) as f32, self.palette.active);
                        }
                        if is_close {
                            if hot {
                                canvas.rounded(inset(rect, s(5)), s(8) as f32, self.palette.hover);
                            }
                            centered_icon(canvas, rect, s(17), Icon::Close, self.palette.tab_ink);
                        } else if let Some(start) = tab.loading.get().filter(|_| tab.spinner.get())
                        {
                            let angle = if self.animate_motion {
                                (start.elapsed().as_secs_f32() % 0.68) / 0.68 * 360.0
                            } else {
                                -90.0
                            };
                            centered_icon(
                                canvas,
                                tile,
                                s(18),
                                Icon::Loading(angle),
                                self.palette.accent,
                            );
                        } else if favicon.is_none() {
                            centered_icon(canvas, tile, s(17), Icon::Globe, self.palette.accent);
                        }
                    });
                    if !is_close {
                        if !tab.spinner.get()
                            && let Some(icon) = favicon.as_ref()
                        {
                            icon.draw(dc, tile, s(17));
                        }
                        if !tab.pinned {
                            let text = RECT {
                                left: rect.left + s(39),
                                right: rect.right - s(4),
                                ..rect
                            };
                            draw_text(
                                dc,
                                self.font,
                                text,
                                &get_text(item.hwndItem),
                                self.palette.tab_ink,
                                DT_LEFT,
                            );
                        }
                    }
                    if let Some((target, after)) = self.tab_drop.get()
                        && target == item.hwndItem
                    {
                        let x = if after { rect.right - s(2) } else { rect.left };
                        fill(
                            dc,
                            RECT {
                                left: x,
                                right: x + s(2),
                                top: rect.top + s(4),
                                bottom: rect.bottom - s(4),
                            },
                            self.palette.accent,
                        );
                    }
                    if focused {
                        focus_box(dc, inset(rect, s(4)), self.palette.tab_ink);
                    }
                });
            }
            return;
        }

        let icon = match id {
            BACK => Some(Icon::Back),
            FORWARD | OPEN => Some(Icon::Forward),
            RELOAD => Some(Icon::Reload),
            DOWNLOADS => Some(Icon::Download),
            GOTO_HOME => Some(Icon::Home),
            NEW_TAB => Some(Icon::Plus),
            TAB_LIST => Some(Icon::Chevron),
            PROTECTION => Some(Icon::Shield),
            MARK => Some(Icon::Star(self.marked.get())),
            MENU => Some(Icon::Menu),
            BOOKMARKS => Some(Icon::Folder),
            MINIMIZE_WINDOW => Some(Icon::Minimize),
            // SAFETY: The root browser window is live.
            MAXIMIZE_WINDOW => Some(Icon::Maximize(unsafe { IsZoomed(self.parent) != 0 })),
            CLOSE_WINDOW => Some(Icon::Close),
            INSPECT_LEFT => Some(Icon::DockLeft),
            INSPECT_BOTTOM => Some(Icon::DockBottom),
            INSPECT_RIGHT => Some(Icon::DockRight),
            INSPECT_UNDOCK => Some(Icon::Undock),
            INSPECT_CLOSE => Some(Icon::Close),
            _ => None,
        };
        let dark = matches!(
            id,
            NEW_TAB | TAB_LIST | MINIMIZE_WINDOW | MAXIMIZE_WINDOW | CLOSE_WINDOW
        );
        let in_address = matches!(id, PROTECTION | MARK | OPEN | ZOOM_LEVEL);
        let base = if dark {
            self.palette.band
        } else if in_address {
            self.palette.address
        } else {
            self.palette.surface
        };
        let ink = if dark {
            self.palette.tab_ink
        } else if id == PROTECTION && self.warning.get() {
            self.palette.warning
        } else if id == PROTECTION || id == MARK && self.marked.get() {
            self.palette.accent
        } else {
            self.palette.ink
        };
        fill(item.hDC, rect, base);
        if id == ZOOM_LEVEL {
            let percent = self.zoom_percent.get();
            let active = percent != 100;
            let foreground = if active {
                self.palette.accent
            } else {
                self.palette.muted
            };
            Canvas::with_dc(item.hDC, |canvas| {
                if active || hot {
                    canvas.rounded(inset(rect, s(2)), s(9) as f32, self.palette.hover);
                }
                let icon_rect = RECT {
                    left: rect.left + s(4),
                    right: rect.left + s(25),
                    ..rect
                };
                centered_icon(canvas, icon_rect, s(15), Icon::Zoom, foreground);
            });
            let label = RECT {
                left: rect.left + s(26),
                right: rect.right - s(6),
                ..rect
            };
            draw_text(
                item.hDC,
                self.font,
                label,
                &format!("{percent}%"),
                foreground,
                DT_LEFT,
            );
            if focused {
                focus_box(item.hDC, inset(rect, s(4)), self.palette.accent);
            }
            return;
        }
        if id == BLOCK_REPORT {
            let body = inset(rect, s(2));
            Canvas::with_dc(item.hDC, |canvas| {
                canvas.rounded(
                    body,
                    s(8) as f32,
                    if hot {
                        self.palette.hover
                    } else {
                        self.palette.address
                    },
                );
                let icon_rect = RECT {
                    left: rect.right - s(23),
                    ..rect
                };
                centered_icon(canvas, icon_rect, s(14), Icon::Chevron, self.palette.muted);
            });
            let text_area = RECT {
                right: rect.right - s(25),
                ..inset(rect, s(7))
            };
            draw_text(
                item.hDC,
                self.font,
                text_area,
                &get_text(item.hwndItem),
                self.palette.ink,
                DT_RIGHT,
            );
            if focused {
                focus_box(item.hDC, inset(rect, s(4)), self.palette.accent);
            }
            return;
        }
        let bookmark_glyph = id
            .checked_sub(PIN_FIRST)
            .and_then(|index| self.bookmark_glyphs.get(index as usize))
            .map(RefCell::borrow);
        let bookmark_icon_rect = RECT {
            left: rect.left + s(5),
            right: rect.left + s(26),
            ..rect
        };
        Canvas::with_dc(item.hDC, |canvas| {
            let dock_selected = self
                .inspect_area
                .get()
                .is_some_and(|(_, _, _, side)| id == side);
            if hot || dock_selected {
                canvas.rounded(
                    inset(rect, s(2)),
                    s(9) as f32,
                    if id == CLOSE_WINDOW {
                        rgb(160, 49, 57)
                    } else if dark {
                        self.palette.active
                    } else {
                        self.palette.hover
                    },
                );
            }
            if id == BOOKMARKS {
                let pos = RECT {
                    left: rect.left + s(5),
                    right: rect.left + s(28),
                    ..rect
                };
                centered_icon(canvas, pos, s(17), Icon::Folder, self.palette.accent);
            } else if let Some(icon) = icon {
                centered_icon(
                    canvas,
                    rect,
                    s(20),
                    icon,
                    if dock_selected {
                        self.palette.accent
                    } else {
                        ink
                    },
                );
                if id == MENU && self.update_available.get() {
                    canvas.dot(
                        (rect.right - s(10)) as f32,
                        (rect.top + s(4)) as f32,
                        s(6) as f32,
                        self.palette.accent,
                    );
                }
            } else if let Some(bookmark) = &bookmark_glyph
                && bookmark.icon.is_none()
            {
                let icon = if bookmark.folder {
                    Icon::Folder
                } else {
                    Icon::Globe
                };
                centered_icon(canvas, bookmark_icon_rect, s(16), icon, self.palette.accent);
            }
        });
        if let Some(icon) = bookmark_glyph
            .as_ref()
            .and_then(|bookmark| bookmark.icon.as_ref())
        {
            icon.draw(item.hDC, bookmark_icon_rect, s(16));
        }
        if id == BOOKMARKS || (PIN_FIRST..PIN_FIRST + 3).contains(&id) {
            let bounds = RECT {
                left: rect.left + s(28),
                right: rect.right - s(5),
                ..rect
            };
            draw_text(
                item.hDC,
                self.font,
                bounds,
                &get_text(item.hwndItem),
                ink,
                DT_LEFT,
            );
            self.paint_bookmark_drop(id, item.hDC, rect);
        }
        if focused {
            focus_box(
                item.hDC,
                inset(rect, s(4)),
                if dark {
                    self.palette.tab_ink
                } else {
                    self.palette.accent
                },
            );
        }
    }

    pub fn update(
        &self,
        warning: bool,
        requests: u64,
        popups: u64,
        private: bool,
        marked: bool,
        filtered: bool,
    ) {
        if self.warning.replace(warning) != warning {
            repaint(self.protection);
        }
        if self.marked.replace(marked) != marked {
            set_text(
                self.mark,
                if marked {
                    "Remover marcador"
                } else {
                    "Salvar marcador"
                },
            );
            repaint(self.mark);
        }
        let status = match (filtered, requests, popups) {
            (false, 0, 0) => "Filtro pausado · Ver bloqueios".to_owned(),
            (_, 0, 0) if private => "Aba privada · Ver bloqueios".to_owned(),
            (_, 0, 0) => "Ver bloqueios".to_owned(),
            _ => {
                let requests_label = if requests == 1 {
                    "bloqueio"
                } else {
                    "bloqueios"
                };
                let popups_label = if popups == 1 { "pop-up" } else { "pop-ups" };
                format!("{requests} {requests_label} · {popups} {popups_label}")
            }
        };
        set_text(self.status, &status);
    }

    pub fn protection_tooltip(&self, text: &str) {
        if get_text(self.protection) == text {
            return;
        }
        set_text(self.protection, text);
        let _old = self.protection_tip.replace(wide(text));
        self.control_tip(
            self.protection,
            &mut self.protection_tip.borrow_mut(),
            TTM_UPDATETIPTEXTW,
        );
    }

    pub fn add_tab(&mut self, title: &str) -> io::Result<()> {
        let slot = (0..MAX_TABS)
            .find(|slot| self.tabs.iter().all(|tab| tab.slot != *slot))
            .ok_or_else(|| io::Error::other("O limite de abas foi atingido."))?;
        let title_button = button(self.parent, title, TAB_LABEL_FIRST + slot as u16, self.font)?;
        let id = TAB_CLOSE_FIRST + slot as u16;
        let close = match button(self.parent, "Fechar aba", id, self.font) {
            Ok(close) => close,
            Err(error) => {
                destroy(title_button);
                return Err(error);
            }
        };
        if let Err(error) =
            subclass(title_button, self.parent).and_then(|_| subclass(close, self.parent))
        {
            destroy(title_button);
            destroy(close);
            return Err(error);
        }
        self.tabs.push(TabButton {
            slot,
            title: title_button,
            close,
            pinned: false,
            loading: Cell::new(None),
            spinner: Cell::new(false),
            icon: RefCell::new(None),
            tip: RefCell::new(wide(title)),
        });
        if let Some(tab) = self.tabs.last() {
            self.tab_tip(tab, TTM_ADDTOOLW);
        }
        self.geometry.set(None);
        Ok(())
    }

    pub fn title(&self, index: usize, title: &str) {
        if let Some(tab) = self.tabs.get(index)
            && get_text(tab.title) != title
        {
            set_text(tab.title, title);
            set_text(tab.close, &format!("Fechar aba: {title}"));
            let _old_tip = tab.tip.replace(wide(title));
            self.tab_tip(tab, TTM_UPDATETIPTEXTW);
            repaint(tab.title);
            repaint(tab.close);
        }
    }

    fn tab_tip(&self, tab: &TabButton, message: u32) {
        self.control_tip(tab.title, &mut tab.tip.borrow_mut(), message);
    }

    fn control_tip(&self, target: HWND, text: &mut [u16], message: u32) {
        if self.tooltip.is_null() {
            return;
        }
        let info = TTTOOLINFOW {
            cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
            hwnd: self.parent,
            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
            uId: target as usize,
            lpszText: text.as_mut_ptr(),
            ..TTTOOLINFOW::default()
        };
        // SAFETY: The tooltip can borrow the tab's heap buffer while registered.
        // On updates the former buffer stays alive through the call.
        unsafe { SendMessageW(self.tooltip, message, 0, (&raw const info) as LPARAM) };
    }

    pub fn tab_icon(&self, index: usize, icon: Option<Rc<Favicon>>) {
        if let Some(tab) = self.tabs.get(index) {
            let changed = {
                let mut previous = tab.icon.borrow_mut();
                let equal = match (previous.as_ref(), icon.as_ref()) {
                    (Some(previous), Some(next)) => Rc::ptr_eq(previous, next),
                    (None, None) => true,
                    _ => false,
                };
                if !equal {
                    *previous = icon;
                }
                !equal
            };
            if changed {
                repaint(tab.title);
            }
        }
    }

    pub fn tab_loading(&self, index: usize, loading: bool) {
        if let Some(tab) = self.tabs.get(index)
            && tab.loading.get().is_some() != loading
        {
            tab.loading.set(loading.then(Instant::now));
            if !loading && tab.spinner.replace(false) {
                repaint(tab.title);
            }
            self.sync_loading_timer();
        }
    }

    pub fn animate_loading(&self) {
        for tab in &self.tabs {
            if tab
                .loading
                .get()
                .is_some_and(|start| start.elapsed() >= SPINNER_DELAY)
            {
                let showing = tab.spinner.replace(true);
                if (!showing || self.animate_motion) && visible(tab.title) {
                    repaint(tab.title);
                }
            }
        }
        self.sync_loading_timer();
    }

    fn sync_loading_timer(&self) {
        let needed = self
            .tabs
            .iter()
            .any(|tab| tab.loading.get().is_some() && (self.animate_motion || !tab.spinner.get()));
        if needed != self.loading_timer.get() {
            // SAFETY: The timer is owned by the live browser HWND and is dispatched on this thread.
            unsafe {
                if needed {
                    self.loading_timer
                        .set(SetTimer(self.parent, LOADING_TIMER, 25, None) != 0);
                } else {
                    KillTimer(self.parent, LOADING_TIMER);
                    self.loading_timer.set(false);
                }
            }
        }
    }

    pub fn set_update_available(&self, available: bool) {
        if self.update_available.replace(available) != available {
            set_text(
                self.menu,
                if available {
                    "Menu do Brisa — atualização disponível"
                } else {
                    "Menu do Brisa"
                },
            );
            repaint(self.menu);
        }
    }

    pub fn move_tab(&mut self, from: usize, to: usize, pinned: bool) {
        let mut tab = self.tabs.remove(from);
        tab.pinned = pinned;
        self.tabs.insert(to, tab);
        self.geometry.set(None);
    }

    pub fn tab_drop_at(
        &self,
        source: usize,
        position: LPARAM,
    ) -> Option<(usize, usize, bool, HWND)> {
        let from = self.tabs.iter().position(|tab| tab.slot == source)?;
        let x = i32::from(position as i16);
        let y = i32::from((position >> 16) as i16);
        self.tabs.iter().enumerate().find_map(|(index, tab)| {
            if tab.pinned != self.tabs[from].pinned || !visible(tab.title) {
                return None;
            }
            let mut bounds = RECT::default();
            let mut close = RECT::default();
            // SAFETY: Both buttons are owned by this toolbar. The output rectangles are writable.
            unsafe {
                if GetWindowRect(tab.title, &mut bounds) == 0 {
                    return None;
                }
                if !tab.pinned && GetWindowRect(tab.close, &mut close) != 0 {
                    bounds.right = bounds.right.max(close.right);
                }
            }
            if !(bounds.left..bounds.right + scale(5, self.dpi)).contains(&x)
                || !(bounds.top..bounds.bottom).contains(&y)
            {
                return None;
            }
            let after = x >= (bounds.left + bounds.right) / 2;
            let insert = index + usize::from(after);
            let to = insert - usize::from(insert > from);
            (from != to).then_some((
                from,
                to,
                after,
                if after && !tab.pinned {
                    tab.close
                } else {
                    tab.title
                },
            ))
        })
    }

    pub fn show_tab_drop(&self, target: Option<(HWND, bool)>) {
        let old = self.tab_drop.replace(target);
        if old != target {
            for (hwnd, _) in [old, target].into_iter().flatten() {
                repaint(hwnd);
            }
        }
    }

    pub fn tab_context(&self, source: HWND, position: LPARAM) -> Option<(usize, POINT)> {
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.title == source || tab.close == source)?;
        let point = if position == -1 {
            let mut bounds = RECT::default();
            // SAFETY: source is a live control owned by this toolbar and the rectangle is writable.
            if unsafe { GetWindowRect(source, &mut bounds) } == 0 {
                return None;
            }
            POINT {
                x: bounds.left,
                y: bounds.bottom,
            }
        } else {
            POINT {
                x: i32::from(position as i16),
                y: i32::from((position >> 16) as i16),
            }
        };
        Some((index, point))
    }

    pub fn delete_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.show_tab_drop(None);
            let tab = self.tabs.remove(index);
            self.tab_tip(&tab, TTM_DELTOOLW);
            destroy(tab.title);
            destroy(tab.close);
            self.geometry.set(None);
            self.sync_loading_timer();
        }
    }

    pub fn select(&self, index: usize) {
        let previous = self.selected.replace(index);
        if previous != index {
            for tab in [self.tabs.get(previous), self.tabs.get(index)]
                .into_iter()
                .flatten()
            {
                repaint(tab.title);
                repaint(tab.close);
            }
        }
    }

    pub fn tab_action(&self, id: u16) -> Option<TabAction> {
        self.tabs.iter().enumerate().find_map(|(index, tab)| {
            if id == TAB_LABEL_FIRST + tab.slot as u16 {
                Some(TabAction::Select(index))
            } else if id == TAB_CLOSE_FIRST + tab.slot as u16 {
                Some(TabAction::Close(index))
            } else {
                None
            }
        })
    }

    pub fn focus_address(&self) {
        // SAFETY: The native edit control can receive focus and selection.
        unsafe {
            SetFocus(self.address);
            SendMessageW(self.address, EM_SETSEL, 0, -1);
        }
    }

    pub fn menu_anchor(&self, command: u16) -> HWND {
        match command {
            BOOKMARKS => self.bookmarks,
            PROTECTION => self.protection,
            DOWNLOADS => self.downloads,
            TAB_LIST => self.tab_list,
            _ => self.menu,
        }
    }

    pub fn bookmark_drop_at(&self, position: LPARAM, source: usize) -> Option<BookmarkDrop> {
        let (point, index) = self.bookmark_context(null_mut(), position)?;
        if index == Some(source) {
            return None;
        }
        // SAFETY: The live toolbar owns both controls, and the bounds are writable.
        unsafe {
            let mut bounds = RECT::default();
            if let Some(index) = index {
                if GetWindowRect(self.pins[index], &mut bounds) == 0 {
                    return None;
                }
                let offset = point.x - bounds.left;
                let width = bounds.right - bounds.left;
                return Some(
                    if self.bookmark_glyphs[index].borrow().folder
                        && (width / 4..width * 3 / 4).contains(&offset)
                    {
                        BookmarkDrop::Into(index)
                    } else if offset < width / 2 {
                        BookmarkDrop::Before(index)
                    } else {
                        BookmarkDrop::After(index)
                    },
                );
            }
            (GetWindowRect(self.status, &mut bounds) != 0 && point.x < bounds.left)
                .then_some(BookmarkDrop::End)
        }
    }

    pub fn show_bookmark_drop(&self, drop: Option<BookmarkDrop>) {
        if self.bookmark_drop.replace(drop) != drop {
            for hwnd in self.pins.into_iter().chain([self.bookmarks]) {
                repaint(hwnd);
            }
        }
    }

    fn paint_bookmark_drop(&self, id: u16, dc: HDC, rect: RECT) {
        let line = scale(2, self.dpi);
        let index = |index| id == PIN_FIRST + index as u16;
        let bounds = match self.bookmark_drop.get() {
            Some(BookmarkDrop::Before(slot)) if index(slot) => RECT {
                right: rect.left + line,
                ..rect
            },
            Some(BookmarkDrop::After(slot)) if index(slot) => RECT {
                left: rect.right - line,
                ..rect
            },
            Some(BookmarkDrop::Into(slot)) if index(slot) => {
                focus_box(dc, inset(rect, line), self.palette.accent);
                return;
            }
            Some(BookmarkDrop::End) if id == BOOKMARKS => RECT {
                top: rect.bottom - line,
                ..rect
            },
            _ => return,
        };
        Canvas::with_dc(dc, |canvas| {
            canvas.rounded(bounds, 1.0, self.palette.accent)
        });
    }

    pub fn bookmark_context(
        &self,
        source: HWND,
        position: LPARAM,
    ) -> Option<(POINT, Option<usize>)> {
        if !self.bookmark_bar.get() {
            return None;
        }
        // SAFETY: Each handle belongs to this live toolbar and the rect's storage is writable.
        unsafe {
            if position == -1 {
                let index = self.pins.iter().position(|&pin| pin == source);
                if source != self.bookmarks && index.is_none() {
                    return None;
                }
                let mut bounds = RECT::default();
                if IsWindowVisible(source) == 0 || GetWindowRect(source, &mut bounds) == 0 {
                    return None;
                }
                return Some((
                    POINT {
                        x: bounds.left,
                        y: bounds.bottom,
                    },
                    index,
                ));
            }
            let point = POINT {
                x: (position as i16) as i32,
                y: ((position >> 16) as i16) as i32,
            };
            let mut client = point;
            if ScreenToClient(self.parent, &mut client) == 0
                || client.y < scale(NAV_HEIGHT, self.dpi)
                || client.y >= self.height()
                || !(0..client_width(self.parent)).contains(&client.x)
            {
                return None;
            }
            let pin = self.pins.iter().position(|&pin| {
                let mut bounds = RECT::default();
                IsWindowVisible(pin) != 0
                    && GetWindowRect(pin, &mut bounds) != 0
                    && PtInRect(&bounds, point) != 0
            });
            Some((point, pin))
        }
    }
}

impl Drop for Toolbar {
    fn drop(&mut self) {
        destroy(self.tooltip);
        // SAFETY: The toolbar owns this font. The window closes before normal drop.
        unsafe { DeleteObject(self.font) };
    }
}

struct FontGuard(HFONT);

impl Drop for FontGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: This guard owns the font until it is transferred to the toolbar.
            unsafe { DeleteObject(self.0) };
        }
    }
}

/// Shared with the window procedure because opening a popup can synchronously
/// repaint the address control while the browser is already mutably borrowed.
pub struct ControlBrushes {
    palette: Cell<Palette>,
    brushes: RefCell<BrushPair>,
}

impl ControlBrushes {
    pub fn new(palette: Palette) -> io::Result<Self> {
        Ok(Self {
            palette: Cell::new(palette),
            brushes: RefCell::new(BrushPair::new(palette)?),
        })
    }

    pub fn attach(self: &Rc<Self>, hwnd: HWND) -> io::Result<()> {
        let colors = Rc::into_raw(Rc::clone(self));
        // SAFETY: The live HWND belongs to this thread. The subclass owns the
        // raw reference until WM_NCDESTROY, or until registration fails.
        unsafe {
            if SetWindowSubclass(hwnd, Some(control_colors), 1, colors as usize) == 0 {
                drop(Rc::from_raw(colors));
                return Err(ui_error("Aplicar cores aos controles"));
            }
        }
        Ok(())
    }

    fn set_palette(&self, palette: Palette) -> io::Result<()> {
        let previous = self.palette.get();
        if previous.surface != palette.surface || previous.address != palette.address {
            self.brushes.replace(BrushPair::new(palette)?);
        }
        self.palette.set(palette);
        Ok(())
    }

    fn control_brush(&self, dc: HDC, edit: bool) -> LRESULT {
        let palette = self.palette.get();
        let (background, foreground) = if edit {
            (palette.address, palette.ink)
        } else {
            (palette.surface, palette.muted)
        };
        // SAFETY: Windows lends the control's drawing context for this message.
        unsafe {
            SetBkColor(dc, background);
            SetTextColor(dc, foreground);
        }
        self.brushes.borrow().0[usize::from(edit)] as LRESULT
    }
}

unsafe extern "system" fn control_colors(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    let colors = data as *const ControlBrushes;
    // SAFETY: Registration retains exactly one Rc until this subclass is removed.
    unsafe {
        match message {
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
                return (*colors).control_brush(wparam as HDC, message == WM_CTLCOLOREDIT);
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(control_colors), id);
                drop(Rc::from_raw(colors));
            }
            _ => {}
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

struct BrushPair([HBRUSH; 2]);

impl BrushPair {
    fn new(palette: Palette) -> io::Result<Self> {
        // SAFETY: GDI returns owned brushes. This guard also cleans up partial allocation.
        let brushes = Self(unsafe {
            [
                CreateSolidBrush(palette.surface),
                CreateSolidBrush(palette.address),
            ]
        });
        if brushes.0.iter().any(|brush| brush.is_null()) {
            return Err(ui_error("Criar cores do navegador"));
        }
        Ok(brushes)
    }
}

impl Drop for BrushPair {
    fn drop(&mut self) {
        for brush in self.0 {
            if !brush.is_null() {
                // SAFETY: The pair owns these brushes.
                unsafe { DeleteObject(brush) };
            }
        }
    }
}

fn centered_icon(canvas: &Canvas, rect: RECT, size: i32, icon: Icon, color: u32) {
    let x = rect.left + (rect.right - rect.left - size) / 2;
    let y = rect.top + (rect.bottom - rect.top - size) / 2;
    canvas.icon(icon, x as f32, y as f32, size as f32, color);
}

pub(crate) fn draw_text(dc: HDC, font: HFONT, mut rect: RECT, text: &str, color: u32, align: u32) {
    let text = wide(text);
    // SAFETY: The paint DC, toolbar font, text and rectangle remain valid for the draw call.
    unsafe {
        let previous = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        DrawTextW(
            dc,
            text.as_ptr(),
            -1,
            &mut rect,
            align | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(dc, previous);
    }
}

fn focus_box(dc: HDC, rect: RECT, color: u32) {
    // SAFETY: The current draw call lends this DC. The brush is owned here.
    unsafe {
        let brush = CreateSolidBrush(color);
        if !brush.is_null() {
            FrameRect(dc, &rect, brush);
            DeleteObject(brush);
        }
    }
}

fn inset(rect: RECT, amount: i32) -> RECT {
    RECT {
        left: rect.left + amount,
        top: rect.top + amount,
        right: rect.right - amount,
        bottom: rect.bottom - amount,
    }
}

fn cursor_over(hwnd: HWND) -> bool {
    let mut point = POINT::default();
    let mut bounds = RECT::default();
    // SAFETY: Both output structs are writable and live for the Win32 calls.
    unsafe {
        GetCursorPos(&mut point) != 0
            && GetWindowRect(hwnd, &mut bounds) != 0
            && point.x >= bounds.left
            && point.x < bounds.right
            && point.y >= bounds.top
            && point.y < bounds.bottom
    }
}

fn client_width(hwnd: HWND) -> i32 {
    let mut rect = RECT::default();
    // SAFETY: The control is live and the rectangle is writable.
    unsafe { GetClientRect(hwnd, &mut rect) };
    rect.right
}

fn repaint(hwnd: HWND) {
    // SAFETY: Invalidation affects only the contents of the passed window.
    unsafe { InvalidateRect(hwnd, null_mut(), 0) };
}

fn buffered(target: HDC, bounds: RECT, draw: impl FnOnce(HDC, RECT)) {
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    if width <= 0 || height <= 0 {
        return;
    }
    // SAFETY: The passed DC is valid for owner draw. The compatible bitmap
    // stays selected only during drawing and is restored before destruction.
    unsafe {
        let memory = CreateCompatibleDC(target);
        if !memory.is_null() {
            let bitmap = CreateCompatibleBitmap(target, width, height);
            if !bitmap.is_null() {
                let previous = SelectObject(memory, bitmap);
                if !previous.is_null() {
                    draw(
                        memory,
                        RECT {
                            right: width,
                            bottom: height,
                            ..RECT::default()
                        },
                    );
                    BitBlt(
                        target,
                        bounds.left,
                        bounds.top,
                        width,
                        height,
                        memory,
                        0,
                        0,
                        SRCCOPY,
                    );
                    SelectObject(memory, previous);
                    DeleteObject(bitmap);
                    DeleteDC(memory);
                    return;
                }
                DeleteObject(bitmap);
            }
            DeleteDC(memory);
        }
    }
    draw(target, bounds);
}

fn destroy(hwnd: HWND) {
    // SAFETY: This UI thread creates and owns all handles passed here.
    unsafe {
        if IsWindow(hwnd) != 0 {
            DestroyWindow(hwnd);
        }
    }
}

fn button(parent: HWND, caption: &str, id: u16, font: HFONT) -> io::Result<HWND> {
    control(
        parent,
        "BUTTON",
        caption,
        WS_TABSTOP | BS_OWNERDRAW as u32,
        id,
        font,
    )
}

fn subclass(hwnd: HWND, parent: HWND) -> io::Result<()> {
    // SAFETY: The child and parent remain alive for the registration.
    if unsafe { SetWindowSubclass(hwnd, Some(child_keys), 1, parent as usize) } == 0 {
        return Err(ui_error("Preparar atalhos do controle"));
    }
    Ok(())
}

pub struct PopupMenu(HMENU);

impl PopupMenu {
    pub fn new() -> io::Result<Self> {
        // SAFETY: CreatePopupMenu allocates a standalone menu owned by this wrapper.
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(menu))
        }
    }

    pub fn item(&self, id: u16, caption: &str, checked: bool) -> io::Result<()> {
        let label = wide(caption);
        let flags = if checked {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        // SAFETY: AppendMenuW copies the text synchronously into the owned menu.
        if unsafe { AppendMenuW(self.0, flags, id as usize, label.as_ptr()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn pick(&self, window: HWND, anchor: HWND, left: bool) -> u16 {
        let mut rect = RECT::default();
        // SAFETY: The anchor and owner are live windows, and TrackPopupMenu retains neither pointer.
        unsafe {
            GetWindowRect(anchor, &mut rect);
            TrackPopupMenu(
                self.0,
                TPM_RETURNCMD | if left { TPM_LEFTALIGN } else { TPM_RIGHTALIGN },
                if left { rect.left } else { rect.right },
                rect.bottom,
                0,
                window,
                null_mut(),
            ) as u16
        }
    }
}

impl Drop for PopupMenu {
    fn drop(&mut self) {
        // SAFETY: The wrapper owns the standalone menu and has not attached it to a window.
        if !self.0.is_null() {
            unsafe { DestroyMenu(self.0) };
        }
    }
}

pub const ZOOM_RESET: u16 = 213;

const SHORTCUTS: [(u16, u8, u16); 20] = [
    (0x46, FALT, MENU),
    (VK_F12, 0, INSPECT),
    (0x49, FCONTROL | FSHIFT, INSPECT),
    (0x30, FCONTROL, ZOOM_RESET),
    (0x4c, FCONTROL, FOCUS_ADDRESS),
    (0x54, FCONTROL, NEW_TAB),
    (0x54, FCONTROL | FSHIFT, REOPEN),
    (0x57, FCONTROL, CLOSE_TAB),
    (0x44, FCONTROL, MARK),
    (0x48, FCONTROL, HISTORY),
    (0x4a, FCONTROL, DOWNLOADS),
    (0x4e, FCONTROL | FSHIFT, NEW_PRIVATE),
    (0x52, FCONTROL, RELOAD),
    (VK_HOME, FALT, GOTO_HOME),
    (VK_LEFT, FALT, BACK),
    (VK_RIGHT, FALT, FORWARD),
    (VK_TAB, FCONTROL, NEXT_TAB),
    (VK_TAB, FCONTROL | FSHIFT, PREVIOUS_TAB),
    (0x42, FCONTROL | FSHIFT, TOGGLE_BOOKMARKS),
    (0x4f, FCONTROL | FSHIFT, BOOKMARK_MANAGER),
];

pub fn shortcut(key: u32) -> Option<u16> {
    // SAFETY: GetKeyState reads the current UI-thread keyboard state for these keys.
    let (control, shift, alt) = unsafe {
        (
            GetKeyState(VK_CONTROL as i32) < 0,
            GetKeyState(VK_SHIFT as i32) < 0,
            GetKeyState(VK_MENU as i32) < 0,
        )
    };
    let modifiers = u8::from(control) * FCONTROL + u8::from(shift) * FSHIFT + u8::from(alt) * FALT;
    SHORTCUTS.iter().find_map(|&(virtual_key, flags, command)| {
        (u32::from(virtual_key) == key && flags == modifiers).then_some(command)
    })
}

pub fn run(hwnd: HWND) -> io::Result<()> {
    loop {
        let mut message = MSG::default();
        // SAFETY: MSG storage remains writable throughout the call.
        match unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } {
            0 => return Ok(()),
            -1 => return Err(io::Error::last_os_error()),
            _ => {}
        }
        if matches!(message.message, WM_KEYDOWN | WM_SYSKEYDOWN)
            // SAFETY: The handle came from GetMessageW and is checked against the browser window.
            && unsafe { GetAncestor(message.hwnd, GA_ROOT) } == hwnd
            && let Some(command) = shortcut(message.wParam as u32)
        {
            // SAFETY: The integer command is handled synchronously by the browser's UI thread.
            unsafe { SendMessageW(hwnd, WM_COMMAND, command as usize, 0) };
            continue;
        }
        // SAFETY: Dispatch uses only the initialized message received from Windows.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

pub const FOCUS_CYCLE: u32 = WM_APP + 4;

unsafe extern "system" fn child_keys(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    parent: usize,
) -> LRESULT {
    // SAFETY: The subclass is active only for the live controls registered in Toolbar::new.
    unsafe {
        let id = GetDlgCtrlID(hwnd);
        if id == i32::from(ADDRESS) && msg == WM_LBUTTONDBLCLK {
            SetFocus(hwnd);
            SendMessageW(hwnd, EM_SETSEL, 0, -1);
            return 0;
        }
        let pin = (i32::from(PIN_FIRST)..i32::from(PIN_FIRST) + 3).contains(&id);
        if pin && msg == WM_SETTEXT {
            let result = DefSubclassProc(hwnd, msg, wparam, lparam);
            repaint(hwnd);
            return result;
        }
        let tab_label = (i32::from(TAB_LABEL_FIRST)..i32::from(TAB_LABEL_FIRST) + MAX_TABS as i32)
            .contains(&id);
        if (pin || tab_label) && drag_input(hwnd, parent as HWND, id as u16, msg, wparam, lparam) {
            return 0;
        }
        let bookmark = id == i32::from(BOOKMARKS) || pin;
        let tab = [TAB_LABEL_FIRST, TAB_CLOSE_FIRST]
            .into_iter()
            .any(|first| (i32::from(first)..i32::from(first) + MAX_TABS as i32).contains(&id));
        if bookmark || tab {
            let screen_position = match msg {
                WM_CONTEXTMENU => Some(lparam),
                WM_RBUTTONUP => {
                    let mut point = POINT {
                        x: i32::from(lparam as i16),
                        y: i32::from((lparam >> 16) as i16),
                    };
                    (ClientToScreen(hwnd, &mut point) != 0)
                        .then_some(point.x as u16 as LPARAM | ((point.y as u16 as LPARAM) << 16))
                }
                _ => None,
            };
            if let Some(position) = screen_position {
                PostMessageW(parent as HWND, WM_CONTEXTMENU, hwnd as WPARAM, position);
                return 0;
            }
        }
        let hover_marker = windows_sys::w!("Brisa.HoveredControl");
        if msg == WM_MOUSEMOVE && id != i32::from(ADDRESS) && GetPropW(hwnd, hover_marker).is_null()
        {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            if TrackMouseEvent(&mut tracking) != 0 {
                SetPropW(hwnd, hover_marker, std::ptr::without_provenance_mut(1));
            }
            repaint(hwnd);
        } else if msg == WM_MOUSELEAVE {
            RemovePropW(hwnd, hover_marker);
            repaint(hwnd);
        } else if msg == WM_NCDESTROY {
            RemovePropW(hwnd, hover_marker);
        } else if msg == WM_ERASEBKGND && tab {
            return 1;
        }
        if msg == WM_KEYDOWN {
            if wparam == VK_TAB as usize {
                let backwards = GetKeyState(VK_SHIFT as i32) < 0;
                PostMessageW(
                    parent as HWND,
                    FOCUS_CYCLE,
                    usize::from(backwards),
                    hwnd as LPARAM,
                );
                return 0;
            }
            let is_address = GetDlgCtrlID(hwnd) == ADDRESS as i32;
            if is_address {
                let virtual_key = wparam as u16;
                match virtual_key {
                    0x41 if GetKeyState(VK_CONTROL as i32) < 0
                        && GetKeyState(VK_MENU as i32) >= 0 =>
                    {
                        SendMessageW(hwnd, EM_SETSEL, 0, -1);
                        return 0;
                    }
                    VK_RETURN => PostMessageW(parent as HWND, WM_COMMAND, OPEN as usize, 0),
                    VK_UP | VK_DOWN => PostMessageW(
                        parent as HWND,
                        crate::omnibox::STEP,
                        usize::from(virtual_key == VK_UP),
                        0,
                    ),
                    VK_ESCAPE => PostMessageW(parent as HWND, crate::omnibox::DISMISS, 0, 0),
                    _ => return DefSubclassProc(hwnd, msg, wparam, lparam),
                };
                return 0;
            }
        }
        if msg == WM_CHAR
            && (wparam == VK_TAB as usize
                || matches!(wparam as u16, 0x01 | VK_RETURN | VK_ESCAPE)
                    && GetDlgCtrlID(hwnd) == ADDRESS as i32)
        {
            return 0;
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

fn drag_input(hwnd: HWND, parent: HWND, id: u16, msg: u32, key: WPARAM, pos: LPARAM) -> bool {
    let press = windows_sys::w!("Brisa.DragPress");
    let dragging = windows_sys::w!("Brisa.Dragging");
    let (slot, moving, drop, cancel) = if id >= TAB_LABEL_FIRST {
        (id - TAB_LABEL_FIRST, TAB_DRAG, TAB_DROP, TAB_DRAG_CANCEL)
    } else {
        (
            id - PIN_FIRST,
            BOOKMARK_DRAG,
            BOOKMARK_DROP,
            BOOKMARK_DRAG_CANCEL,
        )
    };
    let point = |pos: LPARAM| POINT {
        x: i32::from(pos as i16),
        y: i32::from((pos >> 16) as i16),
    };
    // SAFETY: hwnd is a live button of parent. Properties store integers without
    // dereferencing them, including during capture change and teardown.
    unsafe {
        match msg {
            WM_LBUTTONDOWN => {
                SetPropW(
                    hwnd,
                    press,
                    std::ptr::without_provenance_mut(pos as u32 as usize + 1),
                );
            }
            WM_MOUSEMOVE if GetCapture() == hwnd => {
                let start = GetPropW(hwnd, press).addr();
                if start == 0 {
                    return false;
                }
                let mut destination = point(pos);
                if GetPropW(hwnd, dragging).is_null() {
                    let start = point((start - 1) as LPARAM);
                    if (destination.x - start.x).abs() < GetSystemMetrics(SM_CXDRAG).max(4)
                        && (destination.y - start.y).abs() < GetSystemMetrics(SM_CYDRAG).max(4)
                    {
                        return false;
                    }
                    if SetPropW(hwnd, dragging, std::ptr::without_provenance_mut(1)) == 0 {
                        return false;
                    }
                    SendMessageW(hwnd, BM_SETSTATE, 0, 0);
                }
                SetCursor(LoadCursorW(null_mut(), IDC_SIZEALL));
                if ClientToScreen(hwnd, &mut destination) != 0 {
                    PostMessageW(parent, moving, usize::from(slot), packed_point(destination));
                }
                return true;
            }
            WM_LBUTTONUP => {
                RemovePropW(hwnd, press);
                if !RemovePropW(hwnd, dragging).is_null() {
                    ReleaseCapture();
                    let mut destination = point(pos);
                    if ClientToScreen(hwnd, &mut destination) != 0 {
                        PostMessageW(parent, drop, usize::from(slot), packed_point(destination));
                    } else {
                        PostMessageW(parent, cancel, 0, 0);
                    }
                    return true;
                }
            }
            WM_CAPTURECHANGED | WM_CANCELMODE | WM_NCDESTROY | WM_KEYDOWN => {
                if msg == WM_KEYDOWN && key != VK_ESCAPE as usize {
                    return false;
                }
                RemovePropW(hwnd, press);
                if !RemovePropW(hwnd, dragging).is_null() {
                    PostMessageW(parent, cancel, 0, 0);
                    if msg == WM_KEYDOWN {
                        ReleaseCapture();
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn packed_point(point: POINT) -> LPARAM {
    point.x as u16 as LPARAM | ((point.y as u16 as LPARAM) << 16)
}

fn client_animations() -> bool {
    let mut enabled = 1i32;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes a BOOL into this live variable.
    unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut enabled).cast(), 0) };
    enabled != 0
}

fn control(
    parent: HWND,
    class: &str,
    label: &str,
    style: u32,
    id: u16,
    font: HFONT,
) -> io::Result<HWND> {
    let class_name = wide(class);
    let label = wide(label);
    // SAFETY: The class and label are nul-terminated, and parent owns the new control.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            label.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            0,
            0,
            parent,
            id as HMENU,
            null_mut(),
            null_mut(),
        )
    };
    if hwnd.is_null() {
        return Err(ui_error(&format!("Criar controle {class}")));
    }
    if !font.is_null() {
        // SAFETY: The toolbar owns the font for the lifetime of the child controls.
        unsafe { SendMessageW(hwnd, WM_SETFONT, font as WPARAM, 1) };
    }
    Ok(hwnd)
}

fn ui_error(action: &str) -> io::Error {
    io::Error::other(format!("{action}. {}", io::Error::last_os_error()))
}

fn fill(dc: HDC, rect: RECT, color: u32) {
    // SAFETY: The call uses the borrowed DC and destroys only its own brush.
    unsafe {
        let brush = CreateSolidBrush(color);
        if !brush.is_null() {
            FillRect(dc, &rect, brush);
            DeleteObject(brush);
        }
    }
}

pub(crate) fn create_font(dpi: i32) -> HFONT {
    let font_name = wide("Segoe UI Variable Text");
    // SAFETY: GDI copies the nul-terminated name. The caller owns the font.
    unsafe {
        CreateFontW(
            -scale(14, dpi),
            0,
            0,
            0,
            440,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            font_name.as_ptr(),
        )
    }
}

fn position_controls(controls: &[ControlPlacement]) -> bool {
    let coordinates = |control: &ControlPlacement| {
        let flags = SWP_NOREDRAW | SWP_NOZORDER | SWP_NOACTIVATE;
        match control.bounds {
            Some((x, y, width, height)) => (x, y, width, height, flags | SWP_SHOWWINDOW),
            None => (0, 0, 0, 0, flags | SWP_HIDEWINDOW | SWP_NOSIZE | SWP_NOMOVE),
        }
    };
    // SAFETY: Each HWND is a live toolbar child on this thread. The deferred
    // handle is updated by every successful call and consumed only once.
    unsafe {
        let mut batch = BeginDeferWindowPos(controls.len() as i32);
        for control in controls {
            if batch.is_null() {
                break;
            }
            let (x, y, width, height, flags) = coordinates(control);
            batch = DeferWindowPos(batch, control.hwnd, null_mut(), x, y, width, height, flags);
        }
        if !batch.is_null() && EndDeferWindowPos(batch) != 0 {
            return true;
        }
        let mut positioned = true;
        for control in controls {
            let (x, y, width, height, flags) = coordinates(control);
            positioned &= SetWindowPos(control.hwnd, null_mut(), x, y, width, height, flags) != 0;
        }
        positioned
    }
}

fn visible(hwnd: HWND) -> bool {
    // SAFETY: This function only examines a toolbar child.
    unsafe { IsWindowVisible(hwnd) != 0 }
}

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn set_text(hwnd: HWND, text: &str) {
    if get_text(hwnd) == text {
        return;
    }
    // SAFETY: SetWindowTextW copies the nul-terminated text during the call.
    unsafe { SetWindowTextW(hwnd, wide(text).as_ptr()) };
}

pub fn get_text(hwnd: HWND) -> String {
    // SAFETY: The allocated buffer is passed with its capacity in UTF-16 code units.
    let length = unsafe { GetWindowTextLengthW(hwnd) }.clamp(0, 16_384);
    let mut buffer = vec![0; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) }.max(0);
    String::from_utf16_lossy(&buffer[..copied as usize])
}

const fn rgb(red: u32, green: u32, blue: u32) -> u32 {
    red | (green << 8) | (blue << 16)
}

fn scale(value: i32, dpi: i32) -> i32 {
    value.saturating_mul(dpi) / 96
}
