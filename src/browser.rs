use std::{
    cell::{Cell, RefCell},
    collections::{HashSet, VecDeque},
    error::Error,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use adblock::Engine;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT},
    System::Com::CoTaskMemFree,
    UI::{
        Controls::DRAWITEMSTRUCT,
        Shell::{
            BFFM_INITIALIZED, BFFM_SETSELECTIONW, BIF_EDITBOX, BIF_NEWDIALOGSTYLE,
            BIF_RETURNONLYFSDIRS, BROWSEINFOW, GPFIDL_DEFAULT, SHBrowseForFolderW,
            SHGetPathFromIDListEx, ShellExecuteW,
        },
        WindowsAndMessaging::*,
    },
};
use windows_webview::{
    Controller, ControllerOptions, Deferral, DownloadOperation, DownloadStartingArgs,
    DownloadState, Environment, EnvironmentOptions, EventRegistration, KeyEventKind,
    MemoryUsageTargetLevel, MoveFocusReason, NewWindowRequestedArgs, PermissionState,
    PreferredColorScheme, WebResourceResponse, WebView,
};
use windows_window::Window;

use crate::{
    blocked::{Kind as BlockKind, Log as BlockLog},
    center::{self, Command as CenterCommand, Message as CenterMessage, Page as CenterPage},
    chrome::{self, *},
    chrome_draw::Icon,
    chrome_menu::{self, Anchor as MenuAnchor, Item as MenuItem},
    favicon::Favicon,
    instance::ProfileWindow,
    omnibox,
    profile::{
        self, BookmarkItem, BookmarkPlacement, Link, Preferences, Store, Theme, display_title,
    },
    protection, translation, updates,
};

const REFRESH_LAYOUT: u32 = WM_APP + 3;
const NAVIGATION_STATE: u32 = WM_APP + 14;
const POLL_TIMER: usize = 1;
const UPDATE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_EXTRA_RULES_BYTES: u64 = 8 * 1024 * 1024;
const TRANSLATE: u16 = 201;
const PARK: u16 = 208;
const SHELF: u16 = 209;
const ZOOM_IN: u16 = 211;
const ZOOM_OUT: u16 = 212;
const TOGGLE_SITE_FILTER: u16 = 214;
const PROTECTION_DETAILS: u16 = 215;
const TOGGLE_SITE_POPUPS: u16 = 216;
const FIRST_LINK: u16 = 1000;
const RECENT_FIRST: u16 = 1500;
const MENU_BACK: u16 = 250;
const MENU_HISTORY: u16 = 251;
const MENU_BOOKMARKS: u16 = 252;
const MENU_TOOLS: u16 = 253;
const CHECK_UPDATES: u16 = 254;
const MAX_DOWNLOADS: usize = 64;

const HOME_PAGE: &str = include_str!("home.html");
const GOOGLE_HOME: &str = "https://www.google.com/";
const HOME_URL: &str = "https://brisa.invalid/";
type AppResult<T> = Result<T, Box<dyn Error>>;

#[derive(Default)]
struct Options {
    url: Option<String>,
    profile: Option<PathBuf>,
    rules: Option<PathBuf>,
}

impl Options {
    fn read() -> AppResult<Self> {
        let mut options = Self::default();
        let mut args = std::env::args_os().skip(1);
        while let Some(arg) = args.next() {
            let value = args.next().ok_or("Faltou o valor de um argumento.")?;
            match arg.to_str() {
                Some("--url") => {
                    options.url = Some(
                        value
                            .into_string()
                            .map_err(|_| "Endereço inicial inválido.")?,
                    )
                }
                Some("--profile") => options.profile = Some(value.into()),
                Some("--rules") => options.rules = Some(value.into()),
                _ => {
                    return Err(format!("Argumento desconhecido: {}", arg.to_string_lossy()).into());
                }
            }
        }
        Ok(options)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Connection {
    #[default]
    Internal,
    Connecting,
    Https,
    Http,
    LocalHttp,
    BlockedHttp,
    BlockedLocal,
    BlockedRedirect,
    BlockedPopup,
    BlockedScheme,
    Failed,
}

impl Connection {
    fn needs_attention(self) -> bool {
        matches!(
            self,
            Self::Http
                | Self::BlockedHttp
                | Self::BlockedLocal
                | Self::BlockedRedirect
                | Self::BlockedPopup
                | Self::BlockedScheme
                | Self::Failed
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::Internal => "Proteção",
            Self::Connecting => "Conectando",
            Self::Https => "HTTPS",
            Self::Http => "HTTP em claro",
            Self::LocalHttp => "HTTP local",
            Self::BlockedHttp => "HTTP bloqueado",
            Self::BlockedLocal => "Acesso local bloqueado",
            Self::BlockedRedirect => "Redireção barrada",
            Self::BlockedPopup => "Pop-up bloqueado",
            Self::BlockedScheme => "Link bloqueado",
            Self::Failed => "Falha na página",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            Self::Internal => "Esta é uma página interna do Brisa.",
            Self::Connecting => "A navegação ainda está em andamento.",
            Self::Https => {
                "A conexão atual usa HTTPS. A criptografia protege a conexão com o domínio. A confiabilidade do conteúdo depende do site."
            }
            Self::Http => {
                "Esta conexão usa HTTP sem criptografia. Dados enviados pela página podem ser lidos ou alterados por terceiros na rede."
            }
            Self::LocalHttp => {
                "Esta página HTTP aponta para um endereço de loopback neste computador."
            }
            Self::BlockedHttp => {
                "O Brisa impediu a navegação por HTTP sem criptografia. Tente acessar o mesmo endereço por HTTPS."
            }
            Self::BlockedLocal => {
                "Um site tentou abrir um endereço neste computador ou na rede local. Para acessá-lo diretamente, digite o endereço na barra de navegação."
            }
            Self::BlockedRedirect => {
                "O filtro impediu a abertura de um endereço que corresponde às regras de bloqueio."
            }
            Self::BlockedPopup => "A regra de pop-ups do Brisa bloqueou este destino.",
            Self::BlockedScheme => {
                "O Brisa aceita navegação web por HTTP e HTTPS e bloqueou este tipo de link."
            }
            Self::Failed => {
                "O WebView2 não concluiu a navegação. Se o motor exibiu um alerta de certificado ou reputação, siga a orientação exibida na página."
            }
        }
    }
}

#[derive(Default)]
struct Page {
    nav_id: u64,
    navigation_target: String,
    download_navigation: Option<u64>,
    new_tab_home: bool,
    address: String,
    committed_address: String,
    committed_connection: Connection,
    title: String,
    source: String,
    connection: Connection,
    requests: u64,
    popups: u64,
    record: bool,
    update_pending: bool,
    blocks: BlockLog,
    popup_source: Option<String>,
    local_navigation: Option<url::Url>,
    favicon: Option<Rc<Favicon>>,
    favicon_request: u64,
    favicon_changed: bool,
}

struct Tab {
    id: usize,
    private: bool,
    pinned: bool,
    pinned_url: Option<String>,
    center: Option<CenterPage>,
    protection_source: Option<usize>,
    web: WebView,
    controller: Controller,
    host: crate::frame::ViewHost,
    visible: Cell<bool>,
    bounds: Cell<Option<(i32, i32, i32, i32)>>,
    page: Rc<RefCell<Page>>,
    translated: Cell<Option<(u64, i64)>>,
    _events: Vec<EventRegistration>,
}

impl Tab {
    fn show(&self, visible: bool) -> AppResult<()> {
        if self.visible.get() == visible {
            return Ok(());
        }
        self.controller.set_visible(visible)?;
        if let Err(error) = self.host.show(visible) {
            let _ = self.controller.set_visible(!visible);
            return Err(error.into());
        }
        self.visible.set(visible);
        // Older runtime versions can ignore the memory hint and still hide the tab.
        let _ = self.web.set_memory_usage_target_level(if visible {
            MemoryUsageTargetLevel::Normal
        } else {
            MemoryUsageTargetLevel::Low
        });
        Ok(())
    }

    fn resize(&self, area: RECT) -> AppResult<()> {
        let width = (area.right - area.left).max(0);
        let height = (area.bottom - area.top).max(0);
        let bounds = (area.left, area.top, width, height);
        if self.bounds.get() != Some(bounds) {
            self.controller.set_bounds(0, 0, width, height)?;
            self.host.position(area.left, area.top, width, height)?;
            let _ = self.controller.notify_parent_window_position_changed();
            self.bounds.set(Some(bounds));
        }
        Ok(())
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        self._events.clear();
        let _ = self.controller.close();
    }
}

enum Pending {
    Popup {
        tab: usize,
        source: String,
        args: NewWindowRequestedArgs,
        deferral: Deferral,
    },
    ClosePopup(usize),
    Download {
        tab: usize,
        args: DownloadStartingArgs,
        host: String,
        deferral: Deferral,
    },
    DataCleared(bool),
    CacheCleared {
        tab: usize,
        ok: bool,
    },
    Center {
        tab: usize,
        message: CenterMessage,
    },
}

struct Runtime {
    hwnd: HWND,
    preferences: Cell<Preferences>,
    engine: Engine,
    popup_engine: protection::PopupEngine,
    site_exceptions: RefCell<HashSet<(bool, String)>>,
    popup_exceptions: RefCell<HashSet<(bool, String)>>,
    pending: RefCell<VecDeque<Pending>>,
    download_confirmation: Cell<bool>,
}

impl Runtime {
    fn filter_enabled(&self, source: &str, private: bool) -> bool {
        if !self.preferences.get().filter_network {
            return false;
        }
        let exceptions = self.site_exceptions.borrow();
        exceptions.is_empty()
            || site_key(source).is_none_or(|site| !exceptions.contains(&(private, site)))
    }

    fn popup_block(&self, target: &url::Url, source: &str, private: bool) -> Option<Connection> {
        let prefs = self.preferences.get();
        if target.host_str() == Some("brisa.invalid") {
            return Some(Connection::BlockedScheme);
        }
        if !protection::transport_allowed(target, prefs.https_only) {
            return Some(Connection::BlockedHttp);
        }
        let cross_site = prefs.cross_site_popups
            || site_key(source)
                .is_some_and(|site| self.popup_exceptions.borrow().contains(&(private, site)));
        if !protection::popup_allowed(target, source, true, cross_site, prefs.https_only) {
            return Some(Connection::BlockedPopup);
        }
        if self.filter_enabled(source, private)
            && protection::popup_blocked(&self.engine, &self.popup_engine, target.as_str(), source)
        {
            return Some(Connection::BlockedRedirect);
        }
        None
    }

    fn enqueue(&self, action: Pending) {
        let mut queue = self.pending.borrow_mut();
        if queue.len() < 8 {
            queue.push_back(action);
        }
    }
}

struct Browser {
    suggestions: Option<omnibox::Popup>,
    google_suggestions: omnibox::Remote,
    toolbar: Toolbar,
    environment: Environment,
    runtime: Rc<Runtime>,
    store: Store,
    profile_window: ProfileWindow,
    tabs: Vec<Tab>,
    active: usize,
    next_tab: usize,
    closed: VecDeque<String>,
    downloads: VecDeque<(bool, DownloadOperation)>,
    update: Option<updates::Update>,
    update_check: Option<updates::Check>,
    update_download: Option<updates::Download>,
    next_update_check: Option<Instant>,
    inspector: Option<crate::inspector::Panel>,
    dock: crate::inspector::Side,
    translation: Option<PageTranslation>,
}

struct PageTranslation {
    tab: usize,
    navigation: u64,
    target: &'static str,
    phase: TranslationPhase,
}

enum TranslationPhase {
    Reading {
        response: Receiver<Result<translation::Capture, &'static str>>,
        deadline: Instant,
    },
    Translating {
        context: i64,
        worker: translation::Worker,
    },
    Applying {
        context: i64,
        response: Receiver<bool>,
        deadline: Instant,
    },
}

impl BookmarkItem<'_> {
    fn label(&self) -> String {
        match self {
            Self::Folder(folder) => format!("▸ {}", display_title(&folder.title, 30)),
            Self::Link(link) => link_label(link, 40),
        }
    }
}

impl Browser {
    fn current(&self) -> AppResult<&Tab> {
        self.tabs
            .get(self.active)
            .ok_or_else(|| "Nenhuma aba aberta.".into())
    }

    fn new_tab(&mut self, target: Option<&str>, private: bool) -> AppResult<()> {
        self.create_tab(target, private, None)
    }

    fn open_center(&mut self, page: CenterPage) -> AppResult<()> {
        if page == CenterPage::Protection {
            return self.protection_details();
        }
        let source = self
            .tabs
            .get(self.active)
            .filter(|tab| !tab.private && tab.center.is_none())
            .map(|tab| tab.id);
        self.open_center_section(page, false, source)
    }

    fn open_center_section(
        &mut self,
        page: CenterPage,
        private: bool,
        source: Option<usize>,
    ) -> AppResult<()> {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.center.is_some() && tab.private == private)
        {
            let tab = &mut self.tabs[index];
            let changed = tab.center != Some(page);
            tab.center = Some(page);
            if source.is_some() {
                tab.protection_source = source;
            }
            if changed {
                tab.page.borrow_mut().address = page.address().into();
                self.toolbar.title(index, page.title());
            }
            if self.active != index {
                self.select(index)?;
            } else if changed {
                self.refresh(true);
            }
        } else {
            self.create_tab(None, private, Some(page))?;
            self.tabs[self.active].protection_source = source;
        }
        self.send_center(self.current()?.id, page, "")
    }

    fn create_tab(
        &mut self,
        target: Option<&str>,
        private: bool,
        center: Option<CenterPage>,
    ) -> AppResult<()> {
        let mut tab = self.prepare_new_tab(private, center, None)?;
        tab.page.borrow_mut().new_tab_home = target.is_none() && !private && center.is_none();
        if center.is_none() {
            register_resource_filter(&mut tab, &self.runtime)?;
        }
        self.add_tab(tab)?;
        if center.is_some() {
            self.current()?.web.navigate(center::URL)?;
        } else {
            self.navigate(target)?;
            if target.is_none() {
                self.toolbar.focus_address();
            }
        }
        Ok(())
    }

    fn prepare_new_tab(
        &self,
        private: bool,
        center: Option<CenterPage>,
        popup_source: Option<String>,
    ) -> AppResult<Tab> {
        if self.tabs.len() >= MAX_TABS {
            return Err("Feche ou guarde uma aba na estante para abrir outra.".into());
        }
        let next_id = self.next_tab;
        let palette = self.toolbar.palette();
        let host = crate::frame::ViewHost::new(self.runtime.hwnd, palette.surface)?;
        let controller = if private {
            let options = ControllerOptions::new()
                .profile_name("Privado")
                .in_private_mode(true);
            // SAFETY: The tab host is live on this UI thread and outlives its controller.
            unsafe {
                self.environment
                    .create_controller_with_options_for_hwnd(host.hwnd(), &options)?
            }
        } else {
            // SAFETY: The tab host is live on this UI thread and outlives its controller.
            unsafe { self.environment.create_controller_for_hwnd(host.hwnd())? }
        };
        controller.set_visible(false)?;
        let prefs = self.runtime.preferences.get();
        let web = controller.webview()?;
        apply_theme(&web, prefs.theme);
        if let Some(folder) = self.store.data.download_folder.as_deref()
            && let Ok(profile) = web.profile()
            && profile.default_download_folder_path() != folder
        {
            // A saved folder may be on an external drive that is currently unavailable.
            let _ = profile.set_default_download_folder_path(folder);
        }
        let _ = controller.set_default_background_color(web_background(palette, center.is_some()));
        if let Some(page) = center {
            prepare_center(
                host,
                controller,
                Rc::clone(&self.runtime),
                next_id,
                private,
                page,
            )
        } else {
            prepare_tab(
                host,
                controller,
                Rc::clone(&self.runtime),
                next_id,
                private,
                popup_source,
            )
        }
    }

    fn add_tab(&mut self, tab: Tab) -> AppResult<()> {
        let index = self.tabs.len();
        self.toolbar.add_tab(tab.center.map_or(
            if tab.private {
                "Aba privada"
            } else {
                "Nova aba"
            },
            CenterPage::title,
        ))?;
        self.tabs.push(tab);
        self.next_tab = self.next_tab.wrapping_add(1);
        self.select(index)
    }

    fn select(&mut self, index: usize) -> AppResult<()> {
        self.dismiss_suggestions();
        let Some(next) = self.tabs.get(index) else {
            return Ok(());
        };
        self.present(next)?;
        self.active = index;
        self.toolbar.select(index);
        self.refresh(true);
        self.layout();
        Ok(())
    }

    fn move_tab_to_pins(&mut self, index: usize, pin: bool, address: Option<String>) {
        let selected = self.tabs[self.active].id;
        let pinned_count = self.tabs.iter().take_while(|tab| tab.pinned).count();
        let position = if pin { pinned_count } else { pinned_count - 1 };
        let mut tab = self.tabs.remove(index);
        tab.pinned = pin;
        tab.pinned_url = address;
        self.tabs.insert(position, tab);
        self.toolbar.move_tab(index, position, pin);
        if let Some(active) = self.tabs.iter().position(|tab| tab.id == selected) {
            self.active = active;
            self.toolbar.select(active);
        }
        self.layout();
    }

    fn reorder_tab(&mut self, from: usize, to: usize) -> AppResult<()> {
        let selected = self.current()?.id;
        let tab = self.tabs.remove(from);
        self.toolbar.move_tab(from, to, tab.pinned);
        self.tabs.insert(to, tab);
        if let Some(active) = self.tabs.iter().position(|tab| tab.id == selected) {
            self.active = active;
            self.toolbar.select(active);
        }
        self.layout();
        if self.update_saved_pins() {
            self.store.save()?;
        }
        Ok(())
    }

    fn update_saved_pins(&mut self) -> bool {
        let urls = self.tabs.iter().filter_map(|tab| tab.pinned_url.as_ref());
        if urls.clone().eq(&self.store.data.pinned_tabs) {
            return false;
        }
        self.store.data.pinned_tabs = urls.cloned().collect();
        true
    }

    fn tab_context_menu(&mut self, index: usize, point: POINT) -> AppResult<()> {
        let Some(tab) = self.tabs.get(index) else {
            return Ok(());
        };
        let pin = !tab.pinned;
        let items = [
            MenuItem::action(1, if pin { "Fixar aba" } else { "Desafixar aba" }).icon(Icon::Pin),
            MenuItem::action(2, "Fechar aba")
                .icon(Icon::Close)
                .shortcut("Ctrl+W"),
        ];
        match chrome_menu::pick(
            self.runtime.hwnd,
            MenuAnchor::Point(point),
            self.toolbar.palette(),
            &items,
        )? {
            1 => {
                if pin && self.tabs.iter().take_while(|tab| tab.pinned).count() >= MAX_TABS - 1 {
                    return Err("Desafixe uma aba antes de fixar outra.".into());
                }
                let url = tab.web.source();
                let address =
                    (pin && !tab.private && tab.center.is_none() && profile::pinned_address(&url))
                        .then_some(url);
                let mut urls: Vec<_> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .filter_map(|(_, tab)| tab.pinned_url.clone())
                    .collect();
                urls.extend(address.iter().cloned());
                if urls != self.store.data.pinned_tabs {
                    let previous = std::mem::replace(&mut self.store.data.pinned_tabs, urls);
                    if let Err(error) = self.store.save() {
                        self.store.data.pinned_tabs = previous;
                        return Err(error.into());
                    }
                }
                self.move_tab_to_pins(index, pin, address);
            }
            2 => self.close_tab(index, true)?,
            _ => {}
        }
        Ok(())
    }

    fn present(&self, next: &Tab) -> AppResult<()> {
        let show_bookmarks = self.store.data.preferences.show_bookmarks;
        let area = self.client_area();
        if area.right > 0 && area.bottom > 0 {
            let area = RECT {
                top: self.toolbar.height_with_bookmarks(show_bookmarks),
                ..area
            };
            next.resize(self.pane_rects(next.id, area).0)?;
        }
        // Reveal the correctly sized view before hiding the outgoing tab.
        if !next.visible.get() {
            let _ = next.controller.notify_parent_window_position_changed();
        }
        next.show(true)?;
        if let Some(current) = self.tabs.get(self.active)
            && current.id != next.id
            && let Err(error) = current.show(false)
        {
            let _ = next.show(false);
            return Err(error);
        }
        Ok(())
    }

    fn close_current(&mut self, remember: bool) -> AppResult<()> {
        self.close_tab(self.active, remember)
    }

    fn close_tab(&mut self, item: usize, remember: bool) -> AppResult<()> {
        if self.tabs.get(item).is_none() {
            return Ok(());
        }
        if self.tabs.len() == 1 {
            // Closing the final private controller ends the private session.
            let start = self.current()?.private.then_some(HOME_URL);
            self.new_tab(start, false)?;
        } else if self.active == item {
            // Reveal the successor while the current page still covers the
            // host. Only then remove the closing tab and its controller.
            let next = if item + 1 < self.tabs.len() {
                item + 1
            } else {
                item - 1
            };
            self.present(&self.tabs[next])?;
            self.active = next;
        }
        if remember {
            self.remember_closed(item);
        }
        let closed = self.tabs.remove(item);
        if self
            .inspector
            .as_ref()
            .is_some_and(|panel| panel.tab == closed.id)
        {
            self.inspector = None;
        }
        if closed.private && self.tabs.iter().all(|tab| !tab.private) {
            self.downloads.retain(|(private, _)| !private);
            self.runtime
                .site_exceptions
                .borrow_mut()
                .retain(|(private, _)| !private);
            self.runtime
                .popup_exceptions
                .borrow_mut()
                .retain(|(private, _)| !private);
        }
        drop(closed);
        self.toolbar.delete_tab(item);
        let index =
            (self.active - usize::from(item < self.active)).min(self.tabs.len().saturating_sub(1));
        self.active = index;
        self.select(index)?;
        if self.update_saved_pins() {
            self.store.save()?;
        }
        Ok(())
    }

    fn remember_closed(&mut self, index: usize) {
        if let Some(tab) = self
            .tabs
            .get(index)
            .filter(|tab| !tab.private && tab.center.is_none())
        {
            let url = tab.web.source();
            if url != HOME_URL && protection::web_url(&url).is_some() {
                self.closed.push_front(url);
                self.closed.truncate(15);
            }
        }
    }

    fn navigate(&self, target: Option<&str>) -> AppResult<()> {
        let tab = self.current()?;
        let start = if tab.private { HOME_URL } else { GOOGLE_HOME };
        let explicit = target.is_some();
        let target = target.unwrap_or(start);
        {
            let mut page = tab.page.borrow_mut();
            if explicit {
                page.new_tab_home = false;
            }
            page.popup_source = None;
            page.local_navigation =
                protection::web_url(target).filter(protection::is_local_network);
        }
        if let Err(error) = tab.web.navigate(target) {
            tab.page.borrow_mut().local_navigation = None;
            return Err(error.into());
        }
        Ok(())
    }

    fn open_public(&mut self, target: &str) -> AppResult<()> {
        if self.current()?.center.is_some() {
            self.new_tab(Some(target), false)
        } else {
            self.navigate(Some(target))
        }
    }

    fn update_page(&mut self, id: usize) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        let tab = &mut self.tabs[index];
        let (entry, bookmark_icon) = {
            let mut page = tab.page.borrow_mut();
            page.update_pending = false;
            self.toolbar.title(
                index,
                &tab.center.map_or_else(
                    || tab_caption(tab.private, &page),
                    |center| center.title().to_owned(),
                ),
            );
            self.toolbar.tab_icon(index, page.favicon.clone());
            self.toolbar
                .tab_loading(index, page.connection == Connection::Connecting);
            let loaded = matches!(
                page.connection,
                Connection::Https | Connection::Http | Connection::LocalHttp
            );
            if tab.pinned
                && !tab.private
                && loaded
                && profile::pinned_address(&page.committed_address)
                && tab.pinned_url.as_deref() != Some(page.committed_address.as_str())
            {
                tab.pinned_url = Some(page.committed_address.clone());
            }
            let bookmark_icon =
                if std::mem::take(&mut page.favicon_changed) && !tab.private && loaded {
                    page.favicon
                        .as_ref()
                        .and_then(|icon| icon.stored())
                        .map(|icon| (page.committed_address.clone(), icon))
                } else {
                    None
                };
            let entry = if std::mem::take(&mut page.record)
                && !tab.private
                && self.store.data.preferences.remember_history
            {
                Some((page.title.clone(), page.source.clone()))
            } else {
                None
            };
            (entry, bookmark_icon)
        };
        if index == self.active {
            self.refresh(false);
        }
        let mut changed = false;
        if let Some((title, url)) = entry {
            profile::remember(&mut self.store.data.history, &title, &url, true);
            changed = true;
        }
        if let Some((url, icon)) = bookmark_icon
            && let Some(link) = self
                .store
                .data
                .bookmarks
                .iter_mut()
                .find(|link| link.url == url)
            && link.favicon.as_ref() != Some(&icon)
        {
            link.favicon = Some(icon);
            self.layout();
            changed = true;
        }
        changed
    }

    fn poll(&mut self) -> AppResult<()> {
        if self.google_suggestions.poll() {
            self.show_suggestions();
        }
        self.poll_updates()?;
        // Modal Win32 dialogs and controller creation pump messages. Out-of-band
        // changes stay dirty until the main handler has control again.
        let changed: Vec<_> = self
            .tabs
            .iter()
            .filter(|tab| tab.page.borrow().update_pending)
            .map(|tab| tab.id)
            .collect();
        let mut history_changed = false;
        for id in changed {
            history_changed |= self.update_page(id);
        }
        if self.update_saved_pins() || history_changed {
            self.store.save()?;
        }
        if self
            .inspector
            .as_mut()
            .is_some_and(|panel| panel.poll() || panel.finished())
        {
            if self
                .inspector
                .as_ref()
                .is_some_and(crate::inspector::Panel::finished)
            {
                self.inspector = None;
            }
            self.layout();
        }
        if let Ok(tab) = self.current() {
            self.toolbar.update_zoom(tab.controller.zoom_factor());
        }
        self.poll_translation()?;
        self.pending()
    }

    fn refresh(&self, address: bool) {
        let Ok(tab) = self.current() else { return };
        self.toolbar.update_zoom(tab.controller.zoom_factor());
        let page = tab.page.borrow();
        // SAFETY: GetFocus reads this UI thread's current control without retaining it.
        let editing = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus() }
            == self.toolbar.address;
        if address || !editing || chrome::get_text(self.toolbar.address).is_empty() {
            chrome::set_text(
                self.toolbar.address,
                if page.new_tab_home && page.address == GOOGLE_HOME {
                    ""
                } else {
                    &page.address
                },
            );
        }
        let marked = self
            .store
            .data
            .bookmarks
            .iter()
            .any(|link| link.url == page.source);
        let filtered = self.runtime.filter_enabled(&page.source, tab.private);
        self.toolbar.protection_tooltip(&format!(
            "{}. {}{}",
            page.connection.label(),
            page.connection.explanation(),
            if filtered {
                ""
            } else {
                " O filtro de anúncios e rastreadores está pausado."
            },
        ));
        self.toolbar.update(
            page.connection.needs_attention() || !filtered,
            page.requests,
            page.popups,
            tab.private,
            marked,
            filtered,
        );
    }

    fn client_area(&self) -> RECT {
        let mut area = RECT::default();
        // SAFETY: The browser owns a live top-level window. The rectangle is valid for this call.
        unsafe { GetClientRect(self.runtime.hwnd, &mut area) };
        area
    }

    fn layout(&self) {
        let area = self.client_area();
        if area.right <= 0 || area.bottom <= 0 {
            return;
        }
        let items = self.bar_items(0);
        let titles: Vec<_> = items.iter().take(3).map(BookmarkItem::label).collect();
        for (index, item) in items.iter().take(3).enumerate() {
            match item {
                BookmarkItem::Folder(_) => self.toolbar.bookmark_icon(index, true, None),
                BookmarkItem::Link(link) => {
                    self.toolbar
                        .bookmark_icon(index, false, link.favicon.as_deref())
                }
            }
        }
        self.toolbar.layout(
            area.right,
            self.store.data.preferences.show_bookmarks,
            &titles,
        );
        if let Ok(tab) = self.current() {
            let area = RECT {
                top: self.toolbar.height(),
                ..area
            };
            let (page, panel) = self.pane_rects(tab.id, area);
            let _ = tab.resize(page);
            let panel = self.toolbar.inspector_header(panel, self.dock.command());
            if let Some(inspector) = &self.inspector {
                let _ = inspector.layout(panel);
            }
        }
    }

    fn pane_rects(&self, tab: usize, area: RECT) -> (RECT, Option<RECT>) {
        if self
            .inspector
            .as_ref()
            .is_some_and(|panel| panel.tab == tab && panel.docked())
        {
            let (page, panel) = self.dock.split(area);
            (page, Some(panel))
        } else {
            (area, None)
        }
    }

    fn open_inspector(&mut self, dock: bool) -> AppResult<()> {
        let id = self.current()?.id;
        if let Some(panel) = self.inspector.as_mut().filter(|panel| panel.tab == id) {
            if !dock {
                panel.detach()?;
            } else if panel.docked() {
                self.inspector = None;
            } else {
                panel.attach()?;
            }
        } else {
            let tab = self.current()?;
            self.inspector = Some(crate::inspector::Panel::open(
                self.runtime.hwnd,
                tab.host.hwnd(),
                self.toolbar.palette(),
                id,
                &tab.web,
                dock,
            )?);
        }
        self.layout();
        Ok(())
    }

    fn zoom_command(&self, command: u16) -> AppResult<()> {
        let controller = &self.current()?.controller;
        let factor = controller.zoom_factor();
        let levels = [
            0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0,
            5.0,
        ];
        let next = match command {
            ZOOM_IN => levels
                .into_iter()
                .find(|&level| level > factor + 0.005)
                .unwrap_or(factor),
            ZOOM_OUT => levels
                .into_iter()
                .rev()
                .find(|&level| level < factor - 0.005)
                .unwrap_or(factor),
            ZOOM_RESET => 1.0,
            _ => return Ok(()),
        };
        controller.set_zoom_factor(next)?;
        self.toolbar.update_zoom(controller.zoom_factor());
        Ok(())
    }

    fn choose_zoom(&self) -> AppResult<()> {
        let zoom = self.current()?.controller.zoom_factor() * 100.0;
        let items = [
            MenuItem::info(format!("Zoom da página   {zoom:.0}%")).icon(Icon::Zoom),
            MenuItem::action(ZOOM_OUT, "Diminuir zoom").icon(Icon::Minimize),
            MenuItem::action(ZOOM_IN, "Aumentar zoom").icon(Icon::Plus),
            MenuItem::action(ZOOM_RESET, "Restaurar 100%")
                .shortcut("Ctrl+0")
                .icon(Icon::Reload),
        ];
        let picked = chrome_menu::pick(
            self.runtime.hwnd,
            MenuAnchor::Control(self.toolbar.zoom, false),
            self.toolbar.palette(),
            &items,
        )?;
        self.zoom_command(picked)
    }

    fn change_dpi(&mut self, dpi: i32, suggested: RECT) -> AppResult<()> {
        self.toolbar.set_dpi(dpi.max(96))?;
        // SAFETY: The rectangle is copied from WM_DPICHANGED and the root HWND
        // belongs to this UI thread.
        if unsafe {
            SetWindowPos(
                self.runtime.hwnd,
                std::ptr::null_mut(),
                suggested.left,
                suggested.top,
                suggested.right - suggested.left,
                suggested.bottom - suggested.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        self.layout();
        Ok(())
    }

    fn command(&mut self, command: u16) -> AppResult<()> {
        let picked = if command == OPEN {
            self.suggestions
                .as_ref()
                .and_then(|popup| popup.target(None))
        } else {
            None
        };
        self.dismiss_suggestions();
        if let Some(action) = self.toolbar.tab_action(command) {
            return match action {
                TabAction::Select(index) => self.select(index),
                TabAction::Close(index) => self.close_tab(index, true),
            };
        }
        match command {
            BACK => self.current()?.web.go_back()?,
            FORWARD => self.current()?.web.go_forward()?,
            RELOAD => self.current()?.web.reload()?,
            GOTO_HOME => self.open_public(GOOGLE_HOME)?,
            OPEN => {
                if let Some(target) = picked {
                    return self.open_address_suggestion(&target);
                }
                let input = chrome::get_text(self.toolbar.address);
                if input.trim().is_empty() {
                    return Ok(());
                }
                if let Some(page) = CenterPage::from_address(&input) {
                    return self.open_center(page);
                }
                let url = protection::address_or_search(
                    &input,
                    self.store.data.preferences.search_engine,
                )
                .ok_or("Digite um endereço web válido ou um termo de pesquisa.")?;
                self.open_public(url.as_str())?;
                self.current()?
                    .controller
                    .move_focus(MoveFocusReason::Programmatic)?;
            }
            FOCUS_ADDRESS => self.toolbar.focus_address(),
            MINIMIZE_WINDOW | MAXIMIZE_WINDOW | CLOSE_WINDOW => {
                let hwnd = self.runtime.hwnd;
                // SAFETY: Commands target the application's own live top-level window.
                unsafe {
                    match command {
                        MINIMIZE_WINDOW => {
                            ShowWindow(hwnd, SW_MINIMIZE);
                        }
                        MAXIMIZE_WINDOW => {
                            ShowWindow(
                                hwnd,
                                if IsZoomed(hwnd) != 0 {
                                    SW_RESTORE
                                } else {
                                    SW_MAXIMIZE
                                },
                            );
                        }
                        _ => {
                            PostMessageW(hwnd, WM_CLOSE, 0, 0);
                        }
                    }
                }
                if command == MAXIMIZE_WINDOW {
                    self.layout();
                }
            }
            NEW_TAB => self.new_tab(None, self.current()?.private)?,
            NEW_PRIVATE => self.new_tab(None, true)?,
            CLOSE_TAB => self.close_current(true)?,
            NEXT_TAB | PREVIOUS_TAB if !self.tabs.is_empty() => {
                let step = if command == NEXT_TAB {
                    1
                } else {
                    self.tabs.len() - 1
                };
                self.select((self.active + step) % self.tabs.len())?;
            }
            TAB_LIST => self.tab_list()?,
            TOGGLE_BOOKMARKS => {
                let mut preferences = self.store.data.preferences;
                preferences.show_bookmarks = !preferences.show_bookmarks;
                self.save_preferences(preferences)?;
            }
            REOPEN => {
                if let Some(target) = self.closed.front().cloned() {
                    self.new_tab(Some(&target), false)?;
                    self.closed.pop_front();
                }
            }
            INSPECT if self.current()?.center.is_none() => self.open_inspector(true)?,
            INSPECT_UNDOCK | crate::inspector::DETACHED if self.current()?.center.is_none() => {
                self.open_inspector(false)?;
            }
            INSPECT_LEFT | INSPECT_BOTTOM | INSPECT_RIGHT => {
                self.dock = match command {
                    INSPECT_LEFT => crate::inspector::Side::Left,
                    INSPECT_BOTTOM => crate::inspector::Side::Bottom,
                    _ => crate::inspector::Side::Right,
                };
                self.layout();
            }
            INSPECT_CLOSE => {
                self.inspector = None;
                self.layout();
            }
            ZOOM_LEVEL => self.choose_zoom()?,
            ZOOM_IN | ZOOM_OUT | ZOOM_RESET => self.zoom_command(command)?,
            MARK | PARK => self.save_current(command == PARK)?,
            BOOKMARKS => self.bookmarks_menu()?,
            HISTORY => self.open_center(CenterPage::History)?,
            BOOKMARK_MANAGER => self.open_center(CenterPage::Bookmarks)?,
            SHELF => self.links(command)?,
            DOWNLOADS => self.downloads_menu()?,
            CHECK_UPDATES => self.check_updates()?,
            SETTINGS => self.open_center(CenterPage::Settings)?,
            TRANSLATE => self.translate_page()?,
            MENU => self.main_menu()?,
            PROTECTION => self.protection_menu()?,
            BLOCK_REPORT => self.protection_details()?,
            id if (PIN_FIRST..PIN_FIRST + 3).contains(&id) => {
                self.open_bar_item(usize::from(id - PIN_FIRST))?;
            }
            id if (RECENT_FIRST..RECENT_FIRST + 6).contains(&id) => {
                let index = usize::from(id - RECENT_FIRST);
                if let Some(url) = self.closed.get(index).cloned() {
                    self.new_tab(Some(&url), false)?;
                    self.closed.remove(index);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn update_suggestions(&mut self) {
        // SAFETY: GetFocus reads the current UI-thread focus without retaining an HWND.
        if unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus() }
            != self.toolbar.address
        {
            return;
        }
        let input = chrome::get_text(self.toolbar.address);
        let preferences = self.store.data.preferences;
        let enabled = preferences.google_suggestions
            && preferences.search_engine == protection::SearchEngine::Google
            && self.current().is_ok_and(|tab| !tab.private);
        self.google_suggestions
            .queue(&input, preferences.website_language.tag(), enabled);
        self.show_suggestions();
    }

    fn show_suggestions(&mut self) {
        // SAFETY: The edit and the polling window are both owned by this UI thread.
        if unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus() }
            != self.toolbar.address
        {
            return;
        }
        let Ok(tab) = self.current() else { return };
        let input = chrome::get_text(self.toolbar.address);
        let data = &self.store.data;
        let mut rows = omnibox::choices(
            &input,
            data.preferences.search_engine,
            &data.bookmarks,
            if tab.private { &[] } else { &data.history },
        );
        omnibox::append_google(&mut rows, &input, self.google_suggestions.terms());
        if rows.is_empty() {
            self.dismiss_suggestions();
            return;
        }
        let palette = self.toolbar.palette();
        if self.suggestions.is_none() {
            self.suggestions = omnibox::Popup::new(self.runtime.hwnd, palette).ok();
        }
        if let Some(popup) = &self.suggestions {
            popup.show(self.toolbar.address, palette, rows);
        }
    }

    fn dismiss_suggestions(&mut self) {
        self.google_suggestions.clear();
        if let Some(popup) = &self.suggestions {
            popup.hide();
        }
    }

    fn pick_suggestion(&mut self, row: usize) -> AppResult<()> {
        let target = self
            .suggestions
            .as_ref()
            .and_then(|popup| popup.target(Some(row)));
        self.dismiss_suggestions();
        if let Some(target) = target {
            self.open_address_suggestion(&target)?;
        }
        Ok(())
    }

    fn open_address_suggestion(&mut self, target: &str) -> AppResult<()> {
        self.open_public(target)?;
        self.current()?
            .controller
            .move_focus(MoveFocusReason::Programmatic)?;
        Ok(())
    }

    fn save_current(&mut self, shelf: bool) -> AppResult<()> {
        let tab = self.current()?;
        if shelf && tab.private {
            return Err(
                "A estante guarda endereços no perfil. Feche a aba privada para descartá-la."
                    .into(),
            );
        }
        let url = tab.web.source();
        if url == HOME_URL || protection::web_url(&url).is_none() {
            return Ok(());
        }
        let current_title = tab.web.document_title();
        let title = if shelf {
            &current_title
        } else {
            profile::bookmark_title(&current_title, &url)
        };
        let favicon = if !shelf && !tab.private {
            tab.page
                .borrow()
                .favicon
                .as_ref()
                .and_then(|icon| icon.stored())
        } else {
            None
        };
        let links = if shelf {
            &mut self.store.data.shelf
        } else {
            &mut self.store.data.bookmarks
        };
        if !shelf && links.iter().any(|link| link.url == url) {
            links.retain(|link| link.url != url);
        } else {
            profile::remember(links, title, &url, false);
            let saved = links
                .first_mut()
                .filter(|link| link.url == url)
                .ok_or("O endereço é longo demais para salvar.")?;
            saved.favicon = favicon;
        }
        self.store.save()?;
        if shelf {
            self.close_current(false)?;
        }
        self.refresh(false);
        self.layout();
        Ok(())
    }

    fn menu(&self, menu: &PopupMenu, anchor: u16) -> u16 {
        menu.pick(
            self.runtime.hwnd,
            self.toolbar.menu_anchor(anchor),
            matches!(anchor, BOOKMARKS | PROTECTION),
        )
    }

    fn bar_items(&self, folder: u64) -> Vec<BookmarkItem<'_>> {
        self.store.data.bookmark_items(folder)
    }

    fn open_bar_item(&mut self, index: usize) -> AppResult<()> {
        let items = self.bar_items(0);
        match items.get(index) {
            Some(BookmarkItem::Folder(folder)) => self.bookmark_folder_menu(folder.id),
            Some(BookmarkItem::Link(link)) => self.open_public(&link.url.clone()),
            None => Ok(()),
        }
    }

    fn move_bar_item(&mut self, index: usize, target: BookmarkDrop) -> AppResult<()> {
        let items = self.bar_items(0);
        let Some(item) = items.get(index).copied() else {
            return Ok(());
        };
        let destination = |index: usize| items.get(index).copied().map(BookmarkItem::key);
        let placement = match target {
            BookmarkDrop::Before(index) => {
                destination(index).map(|item| BookmarkPlacement::Before { item })
            }
            BookmarkDrop::After(index) => {
                destination(index).map(|item| BookmarkPlacement::After { item })
            }
            BookmarkDrop::Into(index) => match items.get(index) {
                Some(BookmarkItem::Folder(folder)) => {
                    Some(BookmarkPlacement::Into { folder: folder.id })
                }
                _ => None,
            },
            BookmarkDrop::End => Some(BookmarkPlacement::Into { folder: 0 }),
        };
        if let Some(placement) = placement {
            let mut updated = self.store.data.clone();
            updated.move_bookmark(&item.key(), &placement)?;
            self.save_bookmark_changes(updated)?;
        }
        Ok(())
    }

    fn bookmark_context_menu(
        &mut self,
        position: POINT,
        folder: u64,
        index: Option<usize>,
    ) -> AppResult<()> {
        const ADD_FOLDER: u16 = 1;
        const EDIT: u16 = 2;
        const REMOVE: u16 = 3;
        const MANAGE: u16 = 4;

        let items = self.bar_items(folder);
        let target = index.and_then(|index| items.get(index));
        let mut menu = Vec::new();
        if let Some(item) = target {
            match item {
                BookmarkItem::Link(_) => menu.extend([
                    MenuItem::action(EDIT, "Editar marcador"),
                    MenuItem::action(REMOVE, "Excluir marcador").icon(Icon::Close),
                ]),
                BookmarkItem::Folder(_) => menu.extend([
                    MenuItem::action(EDIT, "Renomear pasta"),
                    MenuItem::action(REMOVE, "Excluir pasta").icon(Icon::Close),
                ]),
            }
        }
        let is_folder = matches!(target, Some(BookmarkItem::Folder(_)));
        menu.push(
            MenuItem::action(
                ADD_FOLDER,
                if is_folder {
                    "Nova subpasta"
                } else {
                    "Nova pasta"
                },
            )
            .icon(Icon::Folder),
        );
        menu.push(
            MenuItem::action(MANAGE, "Gerenciar marcadores")
                .shortcut("Ctrl+Shift+O")
                .section(),
        );
        let owner = self.runtime.hwnd;
        let palette = self.toolbar.palette();
        let anchor = MenuAnchor::Point(position);
        let command = chrome_menu::pick(owner, anchor, palette, &menu)?;
        if command == MANAGE {
            return self.open_center(CenterPage::Bookmarks);
        }
        let mut updated = self.store.data.clone();
        match (command, target) {
            (ADD_FOLDER, target) => {
                let parent = match target {
                    Some(BookmarkItem::Folder(folder)) => folder.id,
                    _ => folder,
                };
                let title = if parent == 0 {
                    "Nova pasta"
                } else {
                    "Nova subpasta"
                };
                let Some(values) =
                    chrome_menu::edit(owner, anchor, palette, title, &[("Nome", "", 120)])?
                else {
                    return Ok(());
                };
                updated.add_folder(parent, &values[0])?;
            }
            (EDIT, Some(BookmarkItem::Link(link))) => {
                let Some(values) = chrome_menu::edit(
                    owner,
                    anchor,
                    palette,
                    "Editar marcador",
                    &[("Nome", &link.title, 120), ("Endereço", &link.url, 2048)],
                )?
                else {
                    return Ok(());
                };
                updated.edit_bar_bookmark(&link.url, &values[0], &values[1])?;
            }
            (EDIT, Some(BookmarkItem::Folder(folder))) => {
                let Some(values) = chrome_menu::edit(
                    owner,
                    anchor,
                    palette,
                    "Renomear pasta",
                    &[("Nome", &folder.title, 120)],
                )?
                else {
                    return Ok(());
                };
                updated.rename_folder(folder.id, &values[0])?;
            }
            (REMOVE, Some(BookmarkItem::Link(link))) => {
                updated.bookmarks.retain(|item| item.url != link.url)
            }
            (REMOVE, Some(BookmarkItem::Folder(folder))) => {
                if !confirmed(
                    owner,
                    "Excluir a pasta e mover seu conteúdo para a pasta superior?",
                ) {
                    return Ok(());
                }
                updated.remove_folder(folder.id)?;
            }
            _ => return Ok(()),
        }
        self.save_bookmark_changes(updated)
    }

    fn save_bookmark_changes(&mut self, updated: profile::Data) -> AppResult<()> {
        self.save_profile_data(updated)?;
        if let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| tab.center == Some(CenterPage::Bookmarks))
        {
            self.send_center(tab.id, CenterPage::Bookmarks, "Alteração salva.")?;
        }
        Ok(())
    }

    fn save_profile_data(&mut self, updated: profile::Data) -> AppResult<()> {
        let previous = std::mem::replace(&mut self.store.data, updated);
        if let Err(error) = self.store.save() {
            self.store.data = previous;
            return Err(error.into());
        }
        self.refresh(false);
        self.layout();
        Ok(())
    }

    fn bookmarks_menu(&mut self) -> AppResult<()> {
        self.bookmark_folder_menu(0)
    }

    fn bookmark_folder_menu(&mut self, mut folder: u64) -> AppResult<()> {
        const FOLDER_UP: u16 = 1;
        const EARLIER: u16 = 2;
        const LATER: u16 = 3;
        const PER_PAGE: usize = 7;
        let mut start = 0;
        loop {
            let mut menu = Vec::new();
            let current = self
                .store
                .data
                .folders
                .iter()
                .find(|entry| entry.id == folder);
            if let Some(current) = current {
                menu.push(MenuItem::action(FOLDER_UP, &current.title).icon(Icon::Back));
            }
            let items = self.bar_items(folder);
            if items.is_empty() {
                menu.push(MenuItem::info("Esta pasta está vazia"));
            }
            if start != 0 {
                menu.push(MenuItem::action(EARLIER, "Marcadores anteriores").icon(Icon::Back));
            }
            for (index, item) in items.iter().enumerate().skip(start).take(PER_PAGE) {
                let command = FIRST_LINK + index as u16;
                menu.push(match item {
                    BookmarkItem::Folder(folder) => {
                        MenuItem::action(command, display_title(&folder.title, 48))
                            .icon(Icon::Folder)
                    }
                    BookmarkItem::Link(link) => MenuItem::action(command, link_label(link, 48))
                        .icon(Icon::Globe)
                        .favicon(link.favicon.as_deref()),
                });
            }
            if start + PER_PAGE < items.len() {
                menu.push(MenuItem::action(LATER, "Mais marcadores").icon(Icon::Forward));
            }
            menu.push(
                MenuItem::action(BOOKMARK_MANAGER, "Gerenciar marcadores")
                    .shortcut("Ctrl+Shift+O")
                    .section(),
            );
            let on_bar = self.store.data.preferences.show_bookmarks;
            let anchor = self
                .toolbar
                .menu_anchor(if on_bar { BOOKMARKS } else { MENU });
            let (command, context) = chrome_menu::pick_context(
                self.runtime.hwnd,
                MenuAnchor::Control(anchor, on_bar),
                self.toolbar.palette(),
                &menu,
            )?;
            let index = command.checked_sub(FIRST_LINK).map(usize::from);
            if let Some(position) = context {
                return self.bookmark_context_menu(position, folder, index);
            }
            if command == BOOKMARK_MANAGER {
                return self.open_center(CenterPage::Bookmarks);
            }
            match command {
                FOLDER_UP => {
                    folder = current.map_or(0, |entry| entry.parent);
                    start = 0;
                    continue;
                }
                EARLIER => {
                    start = start.saturating_sub(PER_PAGE);
                    continue;
                }
                LATER => {
                    start += PER_PAGE;
                    continue;
                }
                _ => {}
            }
            match index.and_then(|index| items.get(index)) {
                Some(BookmarkItem::Folder(next)) => {
                    folder = next.id;
                    start = 0;
                }
                Some(BookmarkItem::Link(link)) => return self.open_public(&link.url.clone()),
                None => return Ok(()),
            }
        }
    }

    fn translate_page(&mut self) -> AppResult<()> {
        let tab = self.current()?;
        let (connection, navigation) = {
            let page = tab.page.borrow();
            (page.connection, page.nav_id)
        };
        if tab.center.is_some()
            || !matches!(
                connection,
                Connection::Https | Connection::LocalHttp | Connection::Http
            )
        {
            return Err("Abra uma página web carregada para traduzir.".into());
        }
        if let Some((_, context)) = tab.translated.get().filter(|(id, _)| *id == navigation) {
            translation::evaluate(
                &tab.web,
                context,
                "globalThis.brisaPageTranslation?.restore()",
                |_| {},
            )?;
            tab.translated.set(None);
            return Ok(());
        }
        if self.translation.is_some() {
            return Err("Uma tradução já está em andamento.".into());
        }
        let response = translation::capture(&tab.web)?;
        let target = self.store.data.preferences.translation_language.tag();
        self.translation = Some(PageTranslation {
            tab: tab.id,
            navigation,
            target: target.split('-').next().unwrap_or(target),
            phase: TranslationPhase::Reading {
                response,
                deadline: Instant::now() + Duration::from_secs(5),
            },
        });
        Ok(())
    }

    fn poll_translation(&mut self) -> AppResult<()> {
        let Some(mut job) = self.translation.take() else {
            return Ok(());
        };
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| tab.id == job.tab && tab.page.borrow().nav_id == job.navigation)
        else {
            return Ok(());
        };
        let phase = match job.phase {
            TranslationPhase::Reading { response, deadline } => match response.try_recv() {
                Ok(Ok(capture)) => {
                    if !matches!(
                        capture.text.language.as_str(),
                        "pt" | "en" | "es" | "fr" | "it" | "de"
                    ) {
                        return Err("O idioma desta página não foi identificado entre os idiomas disponíveis.".into());
                    }
                    if capture.text.language == job.target {
                        return Err("A página já está no idioma de destino.".into());
                    }
                    if capture.text.text.is_empty() {
                        return Err("Nenhum texto visível foi encontrado para traduzir.".into());
                    }
                    TranslationPhase::Translating {
                        context: capture.context,
                        worker: translation::Worker::start(
                            &capture.text.language,
                            job.target,
                            capture.text.text,
                        )?,
                    }
                }
                Ok(Err(error)) => return Err(error.into()),
                Err(TryRecvError::Empty) if Instant::now() < deadline => {
                    TranslationPhase::Reading { response, deadline }
                }
                Err(_) => return Err("A página não respondeu ao pedido de tradução.".into()),
            },
            TranslationPhase::Translating { context, worker } => match worker.poll() {
                Some(result) => {
                    let translated = serde_json::to_string(&result?)?;
                    let (send, response) = std::sync::mpsc::channel();
                    let apply = format!("globalThis.brisaPageTranslation?.apply({translated})");
                    translation::evaluate(&tab.web, context, &apply, move |result| {
                        let success = result
                            .ok()
                            .and_then(|data| serde_json::from_str::<serde_json::Value>(&data).ok())
                            .is_some_and(|data| {
                                data.pointer("/result/value")
                                    .and_then(serde_json::Value::as_bool)
                                    == Some(true)
                            });
                        let _ = send.send(success);
                    })?;
                    TranslationPhase::Applying {
                        context,
                        response,
                        deadline: Instant::now() + Duration::from_secs(5),
                    }
                }
                None => TranslationPhase::Translating { context, worker },
            },
            TranslationPhase::Applying {
                context,
                response,
                deadline,
            } => match response.try_recv() {
                Ok(true) => {
                    tab.translated.set(Some((job.navigation, context)));
                    return Ok(());
                }
                Err(TryRecvError::Empty) if Instant::now() < deadline => {
                    TranslationPhase::Applying {
                        context,
                        response,
                        deadline,
                    }
                }
                _ => {
                    return Err(
                        "A página mudou antes que o Brisa pudesse aplicar a tradução.".into(),
                    );
                }
            },
        };
        job.phase = phase;
        self.translation = Some(job);
        Ok(())
    }

    fn send_center(&self, id: usize, page: CenterPage, message: &str) -> AppResult<()> {
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| tab.id == id && tab.center == Some(page))
        else {
            return Ok(());
        };
        if tab.web.source() != center::URL {
            return Ok(());
        }
        let data = match page {
            CenterPage::Bookmarks => {
                serde_json::json!({ "folders": self.store.data.folders, "bookmarks": self.store.data.bookmarks })
            }
            CenterPage::History => serde_json::json!({ "history": self.store.data.history }),
            CenterPage::Settings => {
                let folder = tab.web.profile().ok().map(|profile| profile.default_download_folder_path());
                serde_json::json!({ "preferences": self.store.data.preferences, "download_folder": folder })
            }
            CenterPage::Protection => tab.protection_source
                .and_then(|id| self.tabs.iter().find(|candidate| candidate.id == id && candidate.private == tab.private))
                .map_or_else(|| serde_json::json!({ "entries": [] }), |origin| {
                    let state = origin.page.borrow();
                    let entries: Vec<_> = state.blocks.entries.iter().map(|entry| serde_json::json!({
                        "kind": entry.kind.label(), "target": entry.target, "count": entry.count
                    })).collect();
                    serde_json::json!({
                        "entries": entries, "host": protection::web_url(&state.source).and_then(|url| url.host_str().map(str::to_owned)),
                        "connection": state.connection.explanation(), "requests": state.requests, "popups": state.popups,
                        "unlisted": state.blocks.unlisted,
                    })
                }),
        };
        tab.web.post_web_message_as_json(
            &serde_json::json!({
                "data": data, "message": message,
                "section": page,
                "private": tab.private,
                "theme": self.store.data.preferences.theme,
                "accent": self.store.data.preferences.accent,
            })
            .to_string(),
        )?;
        Ok(())
    }

    fn center_command(&mut self, tab: usize, message: CenterMessage) -> AppResult<()> {
        let Some(entry) = self.tabs.iter().find(|entry| entry.id == tab) else {
            return Ok(());
        };
        let Some(page) = entry.center else {
            return Ok(());
        };
        let CenterMessage { section, command } = message;
        if entry.web.source() != center::URL
            || !command.allowed(page)
            || !matches!(command, CenterCommand::Ready) && section != Some(page)
        {
            return Ok(());
        }
        use CenterCommand::*;
        match command {
            Ready => self.send_center(tab, page, ""),
            Show { page } => {
                if !entry.private {
                    self.open_center(page)?;
                }
                Ok(())
            }
            Open { url } => {
                let saved = match page {
                    CenterPage::Bookmarks => &self.store.data.bookmarks,
                    _ => &self.store.data.history,
                };
                if let Some(link) = saved.iter().find(|link| link.url == url)
                    && let Some(url) = protection::web_url(&link.url)
                    && url.host_str() != Some("brisa.invalid")
                {
                    self.new_tab(Some(url.as_str()), false)?;
                }
                Ok(())
            }
            ClearSites => self.clear_web_data(),
            ChooseDownloadFolder => {
                if self.choose_download_folder()? {
                    self.send_center(tab, page, "Pasta de downloads salva.")?;
                }
                Ok(())
            }
            ClearCache => {
                let runtime = Rc::clone(&self.runtime);
                if let Some(settings) = self.tabs.iter().find(|entry| entry.id == tab) {
                    settings.web.call_dev_tools_protocol_method(
                        "Network.clearBrowserCache",
                        "{}",
                        move |result| {
                            runtime.enqueue(Pending::CacheCleared {
                                tab,
                                ok: result.is_ok(),
                            })
                        },
                    )?;
                }
                Ok(())
            }
            SetSearchEngine { engine } => {
                let updated = Preferences {
                    search_engine: engine,
                    search_engine_selected: true,
                    ..self.store.data.preferences
                };
                self.save_preferences(updated)?;
                self.send_center(tab, page, "Buscador salvo.")
            }
            SetLanguages {
                website,
                translation,
            } => {
                let current = self.store.data.preferences;
                let updated = Preferences {
                    website_language: website,
                    translation_language: translation,
                    ..current
                };
                self.save_preferences(updated)?;
                self.send_center(
                    tab,
                    page,
                    if current.website_language == website {
                        "Idioma da tradução salvo."
                    } else {
                        "Idiomas salvos. Reinicie o Brisa para aplicar o idioma dos sites."
                    },
                )
            }
            SetAppearance { theme, accent } => {
                let updated = Preferences {
                    theme,
                    accent,
                    ..self.store.data.preferences
                };
                self.save_preferences(updated)?;
                self.send_center(tab, page, "Aparência salva.")
            }
            SetPreference { name, checked } => {
                let mut updated = self.store.data.preferences;
                match name.as_str() {
                    "https_only" => updated.https_only = checked,
                    "filter_network" => updated.filter_network = checked,
                    "cross_site_popups" => updated.cross_site_popups = checked,
                    "show_bookmarks" => updated.show_bookmarks = checked,
                    "remember_history" => updated.remember_history = checked,
                    "google_suggestions" => updated.google_suggestions = checked,
                    _ => return Ok(()),
                }
                self.save_preferences(updated)?;
                self.send_center(tab, page, "Configuração salva.")
            }
            operation => {
                let mut updated = self.store.data.clone();
                let outcome = match operation {
                    AddFolder { parent, title } => updated.add_folder(parent, &title),
                    MoveBookmark { item, placement } => updated.move_bookmark(&item, &placement),
                    RenameFolder { id, title } => updated.rename_folder(id, &title),
                    RemoveFolder { id } => updated.remove_folder(id),
                    EditBookmark { url, title, folder } => {
                        updated.edit_bookmark(&url, &title, folder)
                    }
                    RemoveBookmark { url } => {
                        updated.bookmarks.retain(|link| link.url != url);
                        Ok(())
                    }
                    RemoveVisits { visits } => updated.remove_visits(&visits),
                    ClearHistory => {
                        updated.history.clear();
                        Ok(())
                    }
                    _ => return Ok(()),
                };
                match outcome {
                    Ok(()) => {
                        self.save_profile_data(updated)?;
                        self.send_center(tab, page, "Alteração salva.")
                    }
                    Err(message) => self.send_center(tab, page, message),
                }
            }
        }
    }

    fn tab_list(&mut self) -> AppResult<()> {
        let menu = PopupMenu::new()?;
        for (index, tab) in self.tabs.iter().enumerate() {
            let title = tab
                .center
                .map_or_else(
                    || tab_caption(tab.private, &tab.page.borrow()),
                    |page| page.title().into(),
                )
                .replace('&', "&&");
            menu.item(FIRST_LINK + index as u16, &title, index == self.active)?;
        }
        if let Some(index) = self.menu(&menu, TAB_LIST).checked_sub(FIRST_LINK) {
            self.select(usize::from(index))?;
        }
        Ok(())
    }

    fn main_menu(&mut self) -> AppResult<()> {
        let mut page = MENU;
        loop {
            let back = |label: &str| MenuItem::action(MENU_BACK, label).icon(Icon::Back);
            let items = match page {
                MENU_HISTORY => {
                    let mut items = vec![
                        back("Histórico"),
                        MenuItem::action(HISTORY, "Abrir histórico")
                            .shortcut("Ctrl+H")
                            .section(),
                    ];
                    if self.closed.is_empty() {
                        items.push(MenuItem::info("Nenhuma aba fechada nesta sessão"));
                    } else {
                        items.push(
                            MenuItem::action(REOPEN, "Reabrir última aba").shortcut("Ctrl+Shift+T"),
                        );
                    }
                    items.extend(self.closed.iter().take(6).enumerate().map(|(index, url)| {
                        MenuItem::action(RECENT_FIRST + index as u16, display_title(url, 74))
                    }));
                    items
                }
                MENU_BOOKMARKS => vec![
                    back("Marcadores"),
                    MenuItem::action(BOOKMARK_MANAGER, "Gerenciar marcadores")
                        .shortcut("Ctrl+Shift+O")
                        .section(),
                    MenuItem::action(MARK, "Salvar ou remover marcador").shortcut("Ctrl+D"),
                    MenuItem::action(
                        TOGGLE_BOOKMARKS,
                        if self.store.data.preferences.show_bookmarks {
                            "Ocultar barra de marcadores"
                        } else {
                            "Mostrar barra de marcadores"
                        },
                    )
                    .shortcut("Ctrl+Shift+B"),
                ],
                MENU_TOOLS => {
                    let current = self.current()?;
                    let zoom = current.controller.zoom_factor() * 100.0;
                    let translation = if current
                        .translated
                        .get()
                        .is_some_and(|(id, _)| id == current.page.borrow().nav_id)
                    {
                        "Mostrar texto original"
                    } else if self.translation.is_some() {
                        "Traduzindo página…"
                    } else {
                        "Traduzir página"
                    };
                    vec![
                        back("Ferramentas"),
                        MenuItem::action(ZOOM_RESET, "Restaurar zoom")
                            .shortcut(format!("{zoom:.0}%"))
                            .section(),
                        MenuItem::action(TRANSLATE, translation),
                        MenuItem::action(INSPECT, "Inspetor").shortcut("F12"),
                        MenuItem::action(crate::inspector::DETACHED, "DevTools em janela separada"),
                        MenuItem::action(PARK, "Guardar aba na estante").section(),
                        MenuItem::action(SHELF, "Abrir estante"),
                    ]
                }
                _ => vec![
                    MenuItem::action(NEW_TAB, "Nova aba")
                        .shortcut("Ctrl+T")
                        .icon(Icon::Plus),
                    MenuItem::action(NEW_PRIVATE, "Nova aba privada")
                        .shortcut("Ctrl+Shift+N")
                        .icon(Icon::Shield),
                    MenuItem::action(GOTO_HOME, "Página inicial").icon(Icon::Home),
                    MenuItem::action(MENU_HISTORY, "Histórico")
                        .shortcut("›")
                        .section(),
                    MenuItem::action(MENU_BOOKMARKS, "Marcadores")
                        .shortcut("›")
                        .icon(Icon::Star(false)),
                    MenuItem::action(DOWNLOADS, "Downloads")
                        .icon(Icon::Download)
                        .shortcut("Ctrl+J"),
                    MenuItem::action(MENU_TOOLS, "Ferramentas").shortcut("›"),
                    MenuItem::action(SETTINGS, "Configurações").section(),
                    MenuItem::action(
                        CHECK_UPDATES,
                        if self.update_download.is_some() {
                            "Baixando atualização…".into()
                        } else if let Some(version) = &self.update {
                            format!("Atualização disponível ({version})")
                        } else if self.update_check.is_some() {
                            "Verificando atualizações…".into()
                        } else {
                            "Verificar atualizações".into()
                        },
                    ),
                ],
            };
            let command = chrome_menu::pick(
                self.runtime.hwnd,
                MenuAnchor::Control(self.toolbar.menu_anchor(MENU), false),
                self.toolbar.palette(),
                &items,
            )?;
            match command {
                MENU_BACK => page = MENU,
                MENU_HISTORY | MENU_BOOKMARKS | MENU_TOOLS => page = command,
                _ => return self.command(command),
            }
        }
    }

    fn links(&self, command: u16) -> AppResult<()> {
        let links = match command {
            HISTORY => &self.store.data.history,
            SHELF => &self.store.data.shelf,
            _ => &self.store.data.bookmarks,
        };
        if links.is_empty() {
            let message = match command {
                HISTORY => "O histórico está vazio.",
                SHELF => "Use Menu › Guardar aba na estante para guardar o endereço atual.",
                _ => "Use Salvar marcador para adicionar a página atual.",
            };
            dialog(self.runtime.hwnd, "Brisa", message, MB_ICONINFORMATION);
            return Ok(());
        }
        let menu = PopupMenu::new()?;
        for (index, link) in links.iter().enumerate() {
            menu.item(
                FIRST_LINK + index as u16,
                &link_label(link, 68).replace('&', "&&"),
                false,
            )?;
        }
        let selected = self.menu(
            &menu,
            if command == BOOKMARKS && self.store.data.preferences.show_bookmarks {
                BOOKMARKS
            } else {
                MENU
            },
        );
        if let Some(link) = selected
            .checked_sub(FIRST_LINK)
            .and_then(|index| links.get(usize::from(index)))
        {
            self.navigate(Some(&link.url))?;
        }
        Ok(())
    }

    fn save_preferences(&mut self, updated: Preferences) -> AppResult<()> {
        let current = self.store.data.preferences;
        self.store.data.preferences = updated;
        if let Err(error) = self.store.save() {
            self.store.data.preferences = current;
            return Err(error.into());
        }
        self.runtime.preferences.set(updated);
        if current.search_engine != updated.search_engine
            || current.google_suggestions != updated.google_suggestions
        {
            self.dismiss_suggestions();
        }
        if current.search_engine != updated.search_engine {
            self.toolbar.search_hint(updated.search_engine);
        }
        if updated.theme != current.theme || updated.accent != current.accent {
            self.toolbar
                .set_palette(Palette::new(updated.theme, updated.accent))?;
            if updated.theme != current.theme {
                for private in [false, true] {
                    if let Some(tab) = self.tabs.iter().find(|tab| tab.private == private) {
                        apply_theme(&tab.web, updated.theme);
                    }
                }
                let palette = self.toolbar.palette();
                for tab in &self.tabs {
                    tab.host.set_color(palette.surface);
                    let _ = tab.controller.set_default_background_color(web_background(
                        palette,
                        tab.center.is_some(),
                    ));
                }
            }
            for tab in &self.tabs {
                if let Some(page) = tab.center.filter(|page| *page != CenterPage::Settings) {
                    self.send_center(tab.id, page, "")?;
                }
            }
        }
        self.refresh(false);
        if current.show_bookmarks != updated.show_bookmarks {
            self.layout();
        }
        Ok(())
    }

    fn clear_web_data(&self) -> AppResult<()> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| !tab.private)
            .ok_or("Abra uma aba normal para limpar os dados do perfil.")?;
        if confirmed(
            self.runtime.hwnd,
            "Limpar cookies, cache e armazenamento dos sites do perfil?",
        ) {
            let runtime = Rc::clone(&self.runtime);
            tab.web.profile()?.clear_browsing_data_all(move |result| {
                runtime.enqueue(Pending::DataCleared(result.is_ok()));
            })?;
        }
        Ok(())
    }

    fn protection_menu(&mut self) -> AppResult<()> {
        let tab = self.current()?;
        let (source, requests, popups, connection) = {
            let page = tab.page.borrow();
            (
                page.source.clone(),
                page.requests,
                page.popups,
                page.connection,
            )
        };
        let private = tab.private;
        let mut items = vec![
            MenuItem::info(if self.runtime.filter_enabled(&source, private) {
                connection.label().to_owned()
            } else {
                format!("{} · Filtro pausado", connection.label())
            }),
            MenuItem::info(format!("{requests} requisições · {popups} pop-ups")),
            MenuItem::action(PROTECTION_DETAILS, "Ver bloqueios e conexão").section(),
        ];
        let site = site_key(&source);
        if site.is_some() && self.runtime.preferences.get().filter_network {
            let label = if self.runtime.filter_enabled(&source, private) {
                "Pausar filtro neste site durante a sessão"
            } else {
                "Retomar filtro neste site"
            };
            items.push(MenuItem::action(TOGGLE_SITE_FILTER, label));
        }
        if let Some(site) = site
            .as_ref()
            .filter(|_| !self.runtime.preferences.get().cross_site_popups)
        {
            let permitted = self
                .runtime
                .popup_exceptions
                .borrow()
                .contains(&(private, site.clone()));
            let label = if permitted {
                "Remover permissão para pop-ups externos"
            } else {
                "Permitir pop-ups externos por clique"
            };
            items.push(MenuItem::action(TOGGLE_SITE_POPUPS, label));
        }
        match crate::chrome_menu::pick(
            self.runtime.hwnd,
            MenuAnchor::Control(self.toolbar.menu_anchor(PROTECTION), true),
            self.toolbar.palette(),
            &items,
        )? {
            PROTECTION_DETAILS => self.protection_details()?,
            TOGGLE_SITE_FILTER => {
                if let Some(site) = site.map(|site| (private, site)) {
                    let mut exceptions = self.runtime.site_exceptions.borrow_mut();
                    if !exceptions.remove(&site) && exceptions.len() < 128 {
                        exceptions.insert(site);
                    }
                    drop(exceptions);
                    self.refresh(false);
                    self.current()?.web.reload()?;
                }
            }
            TOGGLE_SITE_POPUPS => {
                if let Some(site) = site.map(|site| (private, site)) {
                    let mut exceptions = self.runtime.popup_exceptions.borrow_mut();
                    if !exceptions.remove(&site) && exceptions.len() < 128 {
                        exceptions.insert(site);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn downloads_menu(&mut self) -> AppResult<()> {
        let tab = self.current()?;
        let recent: Vec<_> = self
            .downloads
            .iter()
            .filter(|(private, _)| *private == tab.private)
            .take(8)
            .map(|(_, download)| download)
            .collect();
        const OPEN_FOLDER: u16 = 1;
        const CHOOSE_FOLDER: u16 = 2;
        const FIRST: u16 = 3;
        let mut items = vec![MenuItem::info("Downloads")];
        if recent.is_empty() {
            items.push(MenuItem::info("Nenhum download nesta sessão"));
        }
        for (index, download) in recent.iter().enumerate() {
            let path = download.result_file_path();
            let name = download_filename(&path);
            let row = match download.state() {
                DownloadState::Completed => {
                    MenuItem::action(FIRST + index as u16, name).shortcut("Abrir")
                }
                DownloadState::Interrupted => MenuItem::info(name).shortcut("Interrompido"),
                DownloadState::InProgress => {
                    let total = download.total_bytes_to_receive();
                    let status = if total > 0 {
                        format!(
                            "{}%",
                            download
                                .bytes_received()
                                .saturating_mul(100)
                                .div_euclid(total)
                                .clamp(0, 100)
                        )
                    } else {
                        "Baixando…".into()
                    };
                    MenuItem::info(name).shortcut(status)
                }
            };
            items.push(row.icon(Icon::Download));
        }
        items.push(
            MenuItem::action(OPEN_FOLDER, "Abrir pasta de downloads")
                .icon(Icon::Folder)
                .section(),
        );
        items.push(MenuItem::action(CHOOSE_FOLDER, "Alterar pasta…"));
        let action = chrome_menu::pick(
            self.runtime.hwnd,
            MenuAnchor::Control(self.toolbar.menu_anchor(DOWNLOADS), false),
            self.toolbar.palette(),
            &items,
        )?;
        if action == OPEN_FOLDER {
            let path = tab.web.profile()?.default_download_folder_path();
            if !Path::new(&path).is_absolute() || !Path::new(&path).is_dir() {
                return Err("A pasta de downloads não está disponível.".into());
            }
            self.open_download_path(&path)?;
        } else if action == CHOOSE_FOLDER {
            if self.choose_download_folder()?
                && let Ok(tab) = self.current()
                && tab.center == Some(CenterPage::Settings)
            {
                self.send_center(tab.id, CenterPage::Settings, "Pasta de downloads salva.")?;
            }
        } else if let Some(download) = action
            .checked_sub(FIRST)
            .and_then(|index| recent.get(usize::from(index)))
            .filter(|download| download.state() == DownloadState::Completed)
        {
            let path = download.result_file_path();
            let file = Path::new(&path);
            if !file.is_absolute() || !file.is_file() {
                return Err("O arquivo foi movido ou removido.".into());
            }
            self.open_download_path(&path)?;
        }
        Ok(())
    }

    fn open_download_path(&self, path: &str) -> AppResult<()> {
        let filename = chrome::wide(path);
        // SAFETY: The path is a local directory or finished WebView2 download chosen by the user.
        // ShellExecute opens it through Windows, retaining the file's security zone.
        let opened = unsafe {
            ShellExecuteW(
                self.runtime.hwnd,
                std::ptr::null(),
                filename.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if opened as isize <= 32 {
            return Err("O Windows não conseguiu abrir o caminho selecionado.".into());
        }
        Ok(())
    }

    fn choose_download_folder(&mut self) -> AppResult<bool> {
        let current = self
            .current()?
            .web
            .profile()?
            .default_download_folder_path();
        let Some(folder) = pick_download_folder(self.runtime.hwnd, &current)? else {
            return Ok(false);
        };
        let mut profiles = Vec::new();
        for private in [false, true] {
            if let Some(tab) = self.tabs.iter().find(|tab| tab.private == private) {
                let profile = tab.web.profile()?;
                let previous = profile.default_download_folder_path();
                profiles.push((profile, previous));
            }
        }
        let mut changed = 0;
        let result = (|| -> AppResult<()> {
            for (profile, _) in &profiles {
                profile.set_default_download_folder_path(&folder)?;
                changed += 1;
            }
            let mut updated = self.store.data.clone();
            updated.download_folder = Some(folder);
            self.save_profile_data(updated)
        })();
        if result.is_err() {
            for (profile, previous) in &profiles[..changed] {
                let _ = profile.set_default_download_folder_path(previous);
            }
        }
        result.map(|()| true)
    }

    fn protection_details(&mut self) -> AppResult<()> {
        let active = self.tabs.get(self.active);
        let private = active.is_some_and(|tab| tab.private);
        let source = active.and_then(|tab| {
            if tab.center.is_some() {
                tab.protection_source
            } else {
                Some(tab.id)
            }
        });
        self.open_center_section(CenterPage::Protection, private, source)
    }

    fn pending(&mut self) -> AppResult<()> {
        let pending = self.runtime.pending.borrow_mut().pop_front();
        match pending {
            Some(Pending::Popup {
                tab,
                source,
                args,
                deferral,
            }) => {
                if let Some(private) = self
                    .tabs
                    .iter()
                    .find(|candidate| candidate.id == tab)
                    .map(|tab| tab.private)
                {
                    let mut child = self.prepare_new_tab(private, None, Some(source))?;
                    args.set_new_window(&child.web)?;
                    // WebView2 replaces the underlying web contents when it
                    // adopts a popup. Register its request filter afterwards.
                    register_resource_filter(&mut child, &self.runtime)?;
                    self.add_tab(child)?;
                }
                deferral.complete()?;
            }
            Some(Pending::ClosePopup(tab)) => {
                if let Some(index) = self.tabs.iter().position(|candidate| candidate.id == tab) {
                    self.close_tab(index, false)?;
                }
            }
            Some(Pending::Download {
                tab,
                args,
                host,
                deferral,
            }) => {
                if let Some(tab) = self.tabs.iter().find(|candidate| candidate.id == tab) {
                    let path = args.result_file_path();
                    let filename = download_filename(&path);
                    let private = tab.private;
                    self.runtime.download_confirmation.set(true);
                    let allow = chrome_menu::confirm_download(
                        self.runtime.hwnd,
                        MenuAnchor::Control(self.toolbar.menu_anchor(DOWNLOADS), false),
                        self.toolbar.palette(),
                        &filename,
                        &host,
                        private,
                    );
                    self.runtime.download_confirmation.set(false);
                    if allow? {
                        let operation = args.download_operation()?;
                        args.set_handled(true)?;
                        args.set_cancel(false)?;
                        self.downloads.push_front((private, operation));
                        self.downloads.truncate(MAX_DOWNLOADS);
                    }
                }
                deferral.complete()?;
            }
            Some(Pending::CacheCleared { tab, ok }) => {
                let message = if ok {
                    "Cache esvaziado."
                } else {
                    "O motor não conseguiu esvaziar o cache."
                };
                self.send_center(tab, CenterPage::Settings, message)?;
            }
            Some(Pending::DataCleared(ok)) => {
                dialog(
                    self.runtime.hwnd,
                    "Dados de sites",
                    if ok {
                        "Dados do perfil limpos."
                    } else {
                        "O WebView2 não conseguiu limpar os dados."
                    },
                    MB_ICONINFORMATION,
                );
            }
            Some(Pending::Center { tab, message }) => self.center_command(tab, message)?,
            None => {}
        }
        Ok(())
    }

    fn check_updates(&mut self) -> AppResult<()> {
        if self.update_download.is_some() {
            return Ok(());
        }
        if let Some(update) = &self.update {
            if chrome_menu::confirm_update(
                self.runtime.hwnd,
                MenuAnchor::Control(self.toolbar.menu_anchor(MENU), false),
                self.toolbar.palette(),
                &update.to_string(),
            )? {
                self.update_download = Some(updates::Download::start(update.clone())?);
            }
            return Ok(());
        }
        if let Some(check) = &mut self.update_check {
            check.notify = true;
        } else {
            self.update_check = Some(updates::Check::start(true)?);
        }
        Ok(())
    }

    fn poll_updates(&mut self) -> AppResult<()> {
        if let Some(result) = self
            .update_download
            .as_ref()
            .and_then(updates::Download::poll)
        {
            self.update_download = None;
            match result.and_then(|installer| installer.launch(self.runtime.hwnd)) {
                Ok(()) => {
                    post(self.runtime.hwnd, WM_CLOSE, 0);
                    return Ok(());
                }
                Err(error) => {
                    dialog(
                        self.runtime.hwnd,
                        "Atualização do Brisa",
                        &format!("Não foi possível instalar a atualização.\n\n{error}"),
                        MB_ICONWARNING,
                    );
                }
            }
        }
        if self
            .next_update_check
            .is_some_and(|next| self.profile_window.take_update_request() || Instant::now() >= next)
            && self.update_check.is_none()
        {
            self.update_check = updates::Check::start(false).ok();
            self.next_update_check = Some(Instant::now() + UPDATE_INTERVAL);
        }
        let Some((notify, result)) = self
            .update_check
            .as_ref()
            .and_then(|check| check.poll().map(|result| (check.notify, result)))
        else {
            return Ok(());
        };
        self.update_check = None;
        let message = match result {
            Ok(Some(version)) => {
                self.toolbar.set_update_available(true);
                self.update = Some(version);
                return if notify { self.check_updates() } else { Ok(()) };
            }
            Ok(None) => {
                self.update = None;
                self.toolbar.set_update_available(false);
                "Nenhuma atualização disponível."
            }
            Err(_) => {
                "Não foi possível consultar as atualizações. Tente novamente em alguns minutos."
            }
        };
        if notify {
            dialog(
                self.runtime.hwnd,
                "Atualizações do Brisa",
                message,
                MB_ICONINFORMATION,
            );
        }
        Ok(())
    }
}

fn pick_download_folder(owner: HWND, current: &str) -> io::Result<Option<String>> {
    unsafe extern "system" fn select_initial(
        hwnd: HWND,
        message: u32,
        _: LPARAM,
        path: LPARAM,
    ) -> i32 {
        if message == BFFM_INITIALIZED {
            // SAFETY: The synchronous folder dialog borrows the initial null-terminated path.
            unsafe { SendMessageW(hwnd, BFFM_SETSELECTIONW, 1, path) };
        }
        0
    }
    let start = chrome::wide(current);
    let mut display_name = [0u16; 260];
    let options = BROWSEINFOW {
        hwndOwner: owner,
        pszDisplayName: display_name.as_mut_ptr(),
        lpszTitle: windows_sys::w!("Escolha onde salvar os downloads do Brisa"),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX,
        lpfn: Some(select_initial),
        lParam: start.as_ptr() as LPARAM,
        ..Default::default()
    };
    // SAFETY: The UI thread is initialized as an STA. All options and callback data live through this call.
    let selected = unsafe { SHBrowseForFolderW(&options) };
    if selected.is_null() {
        return Ok(None);
    }
    let mut path = [0u16; 32768];
    // SAFETY: The returned shell item must be freed by CoTaskMemFree. The path buffer is valid for the supplied length.
    let resolved = unsafe {
        let result = SHGetPathFromIDListEx(
            selected,
            path.as_mut_ptr(),
            path.len() as u32,
            GPFIDL_DEFAULT,
        );
        CoTaskMemFree(selected.cast());
        result != 0
    };
    let end = path
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(path.len());
    let path = String::from_utf16(&path[..end]).map_err(io::Error::other)?;
    if resolved && Path::new(&path).is_absolute() && Path::new(&path).is_dir() {
        Ok(Some(path))
    } else {
        Err(io::Error::other("A pasta selecionada não está disponível."))
    }
}

fn apply_theme(web: &WebView, theme: Theme) {
    if let Ok(profile) = web.profile() {
        let scheme = match theme {
            Theme::Light => PreferredColorScheme::Light,
            Theme::Dark => PreferredColorScheme::Dark,
        };
        if profile.preferred_color_scheme() != scheme {
            let _ = profile.set_preferred_color_scheme(scheme);
        }
    }
}

fn web_background(palette: Palette, internal: bool) -> windows_webview::Color {
    // Ordinary documents can rely on a light fallback and supply their own
    // dark canvas when their stylesheet supports it.
    let color = if internal {
        palette.surface
    } else {
        0x00ff_ffff
    };
    let [r, g, b, _] = color.to_le_bytes();
    windows_webview::Color { r, g, b, a: 255 }
}

fn read_favicon(web: &WebView, page: &Rc<RefCell<Page>>) {
    let Some(origin) = site_key(&web.source()) else {
        return;
    };
    let (navigation, request) = {
        let mut state = page.borrow_mut();
        if site_key(&state.source).as_ref() != Some(&origin) {
            return;
        }
        state.favicon_request = state.favicon_request.wrapping_add(1);
        (state.nav_id, state.favicon_request)
    };
    let page = Rc::clone(page);
    let _ = web.favicon_png(move |result| {
        let mut state = page.borrow_mut();
        if state.nav_id != navigation || state.favicon_request != request {
            return;
        }
        if let Some(icon) = result.ok().and_then(Favicon::from_png)
            && !state
                .favicon
                .as_ref()
                .is_some_and(|previous| previous.same_image(&icon))
        {
            state.favicon = Some(Rc::new(icon));
            state.favicon_changed = true;
            state.update_pending = true;
        }
    });
}

fn download_filename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| "Arquivo".to_owned(), |name| display_title(name, 110))
}

fn prepare_tab(
    host: crate::frame::ViewHost,
    controller: Controller,
    runtime: Rc<Runtime>,
    id: usize,
    private: bool,
    popup_source: Option<String>,
) -> AppResult<Tab> {
    controller.set_allow_external_drop(false)?;
    let web = controller.webview()?;
    let settings = web.settings()?;
    if let Ok(agent) = settings.user_agent()
        && let Some((browser, _)) = agent.rsplit_once(" Edg/")
    {
        let _ = settings.set_user_agent(&format!("{browser} Brisa/{}", env!("CARGO_PKG_VERSION")));
    }
    settings.set_web_message_enabled(false)?;
    settings.set_host_objects_allowed(false)?;
    settings.set_password_autosave_enabled(false)?;
    settings.set_general_autofill_enabled(false)?;
    settings.set_dev_tools_enabled(true)?;
    settings.set_zoom_control_enabled(true)?;
    settings.set_status_bar_enabled(false)?;
    settings.set_default_script_dialogs_enabled(false)?;
    let is_popup = popup_source.is_some();
    let page = Rc::new(RefCell::new(Page {
        popup_source,
        ..Page::default()
    }));
    let mut events = vec![
        web.on_permission_requested(|args| {
            let _ = args.set_state(PermissionState::Deny);
        })?,
        {
            let runtime = Rc::clone(&runtime);
            let page = Rc::clone(&page);
            web.on_navigation_starting(move |args| {
                let uri = args.uri();
                let mut state = page.borrow_mut();
                state.nav_id = args.navigation_id();
                state.navigation_target.clone_from(&uri);
                state.download_navigation = None;
                state.record = false;
                let allowed = protection::web_url(&uri);
                let prefs = runtime.preferences.get();
                let redirected = args.is_redirected();
                let local_navigation = state.local_navigation.take().is_some_and(|requested| {
                    !redirected && args.is_user_initiated() && allowed.as_ref() == Some(&requested)
                });
                let previous = if redirected {
                    &state.source
                } else {
                    &state.committed_address
                };
                let reason = match &allowed {
                    Some(url) if let Some(source) = state.popup_source.as_deref() => {
                        runtime.popup_block(url, source, private)
                    }
                    Some(url)
                        if url.host_str() == Some("brisa.invalid") && url.as_str() != HOME_URL =>
                    {
                        Some(Connection::BlockedScheme)
                    }
                    Some(url) if !protection::transport_allowed(url, prefs.https_only) => {
                        Some(Connection::BlockedHttp)
                    }
                    Some(url)
                        if !local_navigation
                            && !protection::local_network_allowed(url, previous) =>
                    {
                        Some(Connection::BlockedLocal)
                    }
                    Some(url)
                        if redirected
                            && runtime.filter_enabled(&state.source, private)
                            && protection::web_url(&state.source)
                                .is_some_and(|source| source.origin() != url.origin())
                            && protection::request_blocked(
                                &runtime.engine,
                                url.as_str(),
                                &state.source,
                                "document",
                            ) =>
                    {
                        Some(Connection::BlockedRedirect)
                    }
                    None if uri != "about:blank" => Some(Connection::BlockedScheme),
                    _ => None,
                };
                if let Some(reason) = reason {
                    let _ = args.set_cancel(true);
                    state.connection = reason;
                    let kind = match reason {
                        Connection::BlockedRedirect if state.popup_source.is_some() => {
                            BlockKind::AdvertisingPopup
                        }
                        Connection::BlockedRedirect => BlockKind::Redirect,
                        _ => BlockKind::Connection,
                    };
                    state.blocks.record(kind, allowed.as_ref());
                    state.address = state.committed_address.clone();
                    state.source = state.committed_address.clone();
                    state.update_pending = true;
                    post(runtime.hwnd, NAVIGATION_STATE, id);
                    return;
                }
                if site_key(&state.source)
                    != allowed.as_ref().and_then(|url| site_key(url.as_str()))
                {
                    state.favicon = None;
                    state.favicon_changed = false;
                }
                state.address = allowed
                    .filter(|url| url.as_str() != HOME_URL)
                    .map(String::from)
                    .unwrap_or_default();
                state.source = state.address.clone();
                if !state.address.is_empty() {
                    state.new_tab_home &= state.address == GOOGLE_HOME;
                }
                state.connection = if state.address.is_empty() {
                    Connection::Internal
                } else {
                    Connection::Connecting
                };
                if !redirected {
                    state.requests = 0;
                    state.popups = 0;
                    state.blocks.clear();
                }
                state.update_pending = true;
                post(runtime.hwnd, NAVIGATION_STATE, id);
            })?
        },
        {
            let page = Rc::clone(&page);
            let web_for_completed = web.clone();
            let runtime = Rc::clone(&runtime);
            web.on_navigation_completed(move |args| {
                let mut state = page.borrow_mut();
                if args.navigation_id() != state.nav_id
                    || matches!(
                        state.connection,
                        Connection::BlockedHttp
                            | Connection::BlockedLocal
                            | Connection::BlockedRedirect
                            | Connection::BlockedPopup
                            | Connection::BlockedScheme
                    )
                {
                    return;
                }
                if !args.is_success()
                    && (args.was_cancelled() || state.download_navigation == Some(state.nav_id))
                {
                    state.address = state.committed_address.clone();
                    state.source = state.address.clone();
                    state.connection = state.committed_connection;
                    state.update_pending = true;
                    post(runtime.hwnd, NAVIGATION_STATE, id);
                    drop(state);
                    read_favicon(&web_for_completed, &page);
                    return;
                }
                if let Some(url) = protection::web_url(&web_for_completed.source())
                    .filter(|url| url.as_str() != HOME_URL)
                {
                    state.source = url.as_str().to_owned();
                    state.address = state.source.clone();
                    state.connection = if !args.is_success() {
                        Connection::Failed
                    } else if url.scheme() == "https" {
                        Connection::Https
                    } else if protection::is_loopback(&url) {
                        Connection::LocalHttp
                    } else {
                        Connection::Http
                    };
                    state.record = args.is_success();
                    if state.record {
                        state.committed_address = state.address.clone();
                        state.popup_source = None;
                    }
                } else {
                    if args.is_success() {
                        state.committed_address.clear();
                    } else {
                        state.connection = Connection::Failed;
                    }
                }
                state.title = display_title(&web_for_completed.document_title(), 120);
                if args.is_success() {
                    state.committed_connection = state.connection;
                }
                state.favicon_changed |= state.favicon.is_some() && args.is_success();
                state.update_pending = true;
                post(runtime.hwnd, NAVIGATION_STATE, id);
                drop(state);
                if args.is_success() {
                    read_favicon(&web_for_completed, &page);
                }
            })?
        },
        {
            let page = Rc::clone(&page);
            web.on_document_title_changed(move |title| {
                let mut state = page.borrow_mut();
                state.title = display_title(&title, 120);
                state.update_pending = true;
            })?
        },
        {
            let runtime = Rc::clone(&runtime);
            let page = Rc::clone(&page);
            web.on_new_window_requested(move |args| {
                if args.set_handled(true).is_err() {
                    return;
                }
                let mut state = page.borrow_mut();
                let uri = args.uri();
                let target = protection::web_url(&uri);
                let user_initiated = args.is_user_initiated();
                let source = user_initiated
                    .then(|| site_key(&state.committed_address))
                    .flatten();
                let reason = target
                    .as_ref()
                    .zip(source.as_deref())
                    .and_then(|(target, source)| runtime.popup_block(target, source, private));
                if let Some(source) = source
                    && (uri == "about:blank" || target.is_some() && reason.is_none())
                    && let Ok(deferral) = args.defer()
                {
                    drop(state);
                    runtime.enqueue(Pending::Popup {
                        tab: id,
                        source,
                        args,
                        deferral,
                    });
                } else {
                    state.popups = state.popups.saturating_add(1);
                    let kind = if !user_initiated {
                        BlockKind::AutomaticPopup
                    } else if reason == Some(Connection::BlockedRedirect) {
                        BlockKind::AdvertisingPopup
                    } else {
                        BlockKind::RestrictedPopup
                    };
                    state.blocks.record(kind, target.as_ref());
                    state.update_pending = true;
                }
            })?
        },
        {
            let runtime = Rc::clone(&runtime);
            let page = Rc::clone(&page);
            let current_web = web.clone();
            web.on_download_starting(move |args| {
                if args.set_cancel(true).is_err() {
                    return;
                }
                if runtime.download_confirmation.get()
                    || runtime
                        .pending
                        .borrow()
                        .iter()
                        .any(|item| matches!(item, Pending::Download { .. }))
                {
                    return;
                }
                let mut state = page.borrow_mut();
                let Ok(operation) = args.download_operation() else {
                    return;
                };
                let download_url = operation.uri();
                let source = if state.committed_address.is_empty() {
                    &state.source
                } else {
                    &state.committed_address
                };
                let origin = protection::download_origin(
                    &download_url,
                    source,
                    runtime.preferences.get().https_only,
                );
                let was_navigation = matches!(
                    state.connection,
                    Connection::Connecting | Connection::Failed
                ) && state.navigation_target == download_url;
                if was_navigation {
                    state.download_navigation = Some(state.nav_id);
                    state.address = state.committed_address.clone();
                    state.source = state.address.clone();
                    state.connection = state.committed_connection;
                    state.update_pending = true;
                    post(runtime.hwnd, NAVIGATION_STATE, id);
                }
                drop(state);
                if was_navigation {
                    read_favicon(&current_web, &page);
                }
                let Some(origin) = origin else { return };
                let Ok(deferral) = args.defer() else { return };
                runtime.enqueue(Pending::Download {
                    tab: id,
                    args,
                    host: origin.host_str().unwrap_or("este site").to_owned(),
                    deferral,
                });
            })?
        },
        {
            let runtime = Rc::clone(&runtime);
            controller.on_accelerator_key_pressed(move |args| {
                if matches!(
                    args.key_event_kind(),
                    KeyEventKind::KeyDown | KeyEventKind::SystemKeyDown
                ) && let Some(command) = chrome::shortcut(args.virtual_key())
                    && post(runtime.hwnd, WM_COMMAND, command as usize)
                {
                    let _ = args.set_handled(true);
                }
            })?
        },
    ];
    {
        let page = Rc::clone(&page);
        if let Ok(event) = web.on_favicon_changed(move |web| read_favicon(&web, &page)) {
            events.push(event);
        }
    }
    if is_popup {
        events.push(web.on_window_close_requested(move || {
            runtime.enqueue(Pending::ClosePopup(id));
        })?);
    }
    Ok(Tab {
        id,
        private,
        pinned: false,
        pinned_url: None,
        center: None,
        protection_source: None,
        web,
        controller,
        host,
        visible: Cell::new(false),
        bounds: Cell::new(None),
        page,
        translated: Cell::new(None),
        _events: events,
    })
}

fn register_resource_filter(tab: &mut Tab, runtime: &Rc<Runtime>) -> AppResult<()> {
    let runtime = Rc::clone(runtime);
    let page = Rc::clone(&tab.page);
    let private = tab.private;
    tab._events
        .push(tab.web.on_web_resource_requested("*", move |request| {
            let uri = request.uri();
            let target = protection::web_url(&uri)?;
            if target.host_str() == Some("brisa.invalid") {
                let response = if target.as_str() == HOME_URL && request.method() == "GET" {
                    WebResourceResponse::new(HOME_PAGE.as_bytes())
                        .content_type("text/html; charset=utf-8")
                } else {
                    WebResourceResponse::new(Vec::new()).status(404, "Not Found")
                };
                return Some(response.header("Cache-Control", "no-store"));
            }
            let prefs = runtime.preferences.get();
            let mut state = page.borrow_mut();
            let insecure = !protection::transport_allowed(&target, prefs.https_only);
            let private_destination = !protection::local_network_allowed(&target, &state.source);
            if insecure
                || private_destination
                || runtime.filter_enabled(&state.source, private)
                    && protection::should_block(
                        &runtime.engine,
                        &uri,
                        &state.source,
                        &request.headers(),
                    )
            {
                state.requests = state.requests.saturating_add(1);
                let kind = if insecure || private_destination {
                    BlockKind::Connection
                } else {
                    BlockKind::Network
                };
                state.blocks.record(kind, Some(&target));
                state.update_pending = true;
                return Some(
                    WebResourceResponse::new(Vec::new())
                        .status(403, "Blocked by Brisa")
                        .header("Cache-Control", "no-store"),
                );
            }
            None
        })?);
    Ok(())
}

fn prepare_center(
    host: crate::frame::ViewHost,
    controller: Controller,
    runtime: Rc<Runtime>,
    id: usize,
    private: bool,
    center: CenterPage,
) -> AppResult<Tab> {
    controller.set_allow_external_drop(false)?;
    let web = controller.webview()?;
    let settings = web.settings()?;
    settings.set_host_objects_allowed(false)?;
    settings.set_web_message_enabled(true)?;
    settings.set_password_autosave_enabled(false)?;
    settings.set_general_autofill_enabled(false)?;
    settings.set_dev_tools_enabled(false)?;
    settings.set_default_context_menus_enabled(false)?;
    let state = Rc::new(RefCell::new(Page {
        address: center.address().into(),
        title: center.title().into(),
        ..Page::default()
    }));
    let events = vec![
        web.on_navigation_starting(|args| {
            if args.uri() != center::URL {
                let _ = args.set_cancel(true);
            }
        })?,
        web.on_new_window_requested(|args| {
            let _ = args.set_handled(true);
        })?,
        web.on_permission_requested(|args| {
            let _ = args.set_state(PermissionState::Deny);
        })?,
        web.on_download_starting(|args| {
            let _ = args.set_cancel(true);
        })?,
        web.on_web_resource_requested("*", |request| {
            Some(center::response(&request.uri(), &request.method()))
        })?,
        {
            let runtime = Rc::clone(&runtime);
            web.on_web_message_received(move |args| {
                if args.source() != center::URL {
                    return;
                }
                if let Some(message) = CenterMessage::parse(&args.web_message_as_json()) {
                    runtime.enqueue(Pending::Center { tab: id, message });
                }
            })?
        },
        controller.on_accelerator_key_pressed(move |args| {
            if matches!(
                args.key_event_kind(),
                KeyEventKind::KeyDown | KeyEventKind::SystemKeyDown
            ) && let Some(command) = chrome::shortcut(args.virtual_key())
                && post(runtime.hwnd, WM_COMMAND, command as usize)
            {
                let _ = args.set_handled(true);
            }
        })?,
    ];
    Ok(Tab {
        id,
        private,
        pinned: false,
        pinned_url: None,
        center: Some(center),
        protection_source: None,
        web,
        controller,
        host,
        visible: Cell::new(false),
        bounds: Cell::new(None),
        page: state,
        translated: Cell::new(None),
        _events: events,
    })
}

fn tab_caption(private: bool, page: &Page) -> String {
    let title = display_title(&page.title, 26);
    if private {
        if title.is_empty() || page.source.is_empty() {
            "Aba privada".into()
        } else {
            format!("Privada  {title}")
        }
    } else if page.source.is_empty() {
        "Nova aba".into()
    } else if title.is_empty() {
        protection::web_url(&page.source)
            .and_then(|url| url.host_str().map(|host| display_title(host, 26)))
            .unwrap_or_else(|| "Página".into())
    } else {
        title
    }
}

fn link_label(link: &Link, limit: usize) -> String {
    if link.title.is_empty() {
        display_title(&link.url, limit)
    } else {
        display_title(&link.title, limit)
    }
}

fn site_key(location: &str) -> Option<String> {
    protection::web_url(location)
        .filter(|url| url.as_str() != HOME_URL)
        .map(|url| url.origin().ascii_serialization())
}

fn read_extra_rules(path: &Path) -> io::Result<String> {
    let mut rules = Vec::new();
    fs::File::open(path)?
        .take(MAX_EXTRA_RULES_BYTES + 1)
        .read_to_end(&mut rules)?;
    if rules.len() as u64 > MAX_EXTRA_RULES_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "O arquivo de regras do Brisa excede 8 MiB.",
        ));
    }
    String::from_utf8(rules).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn run() -> AppResult<()> {
    let options = Options::read()?;
    let default_profile = options.profile.is_none();
    let path = match options.profile {
        Some(path) => path,
        None => {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA indisponível.")?)
                .join("BrisaBrowser")
                .join("WebView2")
        }
    };
    fs::create_dir_all(&path)?;
    let path = std::path::absolute(path)?;
    let mut profile_window = ProfileWindow::new(&path)?;
    let store = match Store::open(&path) {
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            for _ in 0..10 {
                if profile_window.activate(default_profile) {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(120));
            }
            Store::open(&path)?
        }
        result => result?,
    };
    let extra_rules = options.rules.as_deref().map(read_extra_rules).transpose()?;
    let engine = protection::engine(extra_rules.as_deref());
    let popup_engine = protection::popup_engine(extra_rules);
    let app = Rc::new(RefCell::new(None::<Browser>));
    let state = Rc::clone(&app);
    let preferences = store.data.preferences;
    let colors = Rc::new(ControlBrushes::new(Palette::new(
        preferences.theme,
        preferences.accent,
    ))?);
    let window = Window::new(concat!("Brisa ", env!("CARGO_PKG_VERSION")))
        .size(1120, 790)
        .style(crate::frame::STYLE)
        .on_message(move |hwnd, message, wparam, lparam| {
            let mut borrow = state.try_borrow_mut().ok()?;
            let browser = borrow.as_mut()?;
            let result = match message {
                WM_PAINT => {
                    let mut paint = PAINTSTRUCT::default();
                    // SAFETY: Windows sent WM_PAINT for this live root window.
                    // The borrowed DC is used only until the paired EndPaint.
                    unsafe {
                        let dc = BeginPaint(hwnd, &mut paint);
                        if !dc.is_null() {
                            browser.toolbar.paint_background(dc);
                            EndPaint(hwnd, &paint);
                        }
                    }
                    return Some(0);
                }
                WM_DRAWITEM if lparam != 0 => {
                    // SAFETY: WM_DRAWITEM supplies an initialized descriptor for the duration of this message.
                    browser
                        .toolbar
                        .draw(unsafe { &*(lparam as *const DRAWITEMSTRUCT) });
                    return Some(1);
                }
                WM_COMMAND
                    if lparam as HWND == browser.toolbar.address
                        && wparam >> 16 == EN_CHANGE as usize =>
                {
                    browser.update_suggestions();
                    Ok(())
                }
                WM_COMMAND
                    if lparam as HWND == browser.toolbar.address
                        && wparam >> 16 == EN_KILLFOCUS as usize =>
                {
                    browser.dismiss_suggestions();
                    Ok(())
                }
                omnibox::PICK => browser.pick_suggestion(wparam),
                omnibox::STEP => {
                    if let Some(popup) = &browser.suggestions {
                        popup.step(wparam != 0);
                    }
                    Ok(())
                }
                omnibox::DISMISS | WM_ACTIVATEAPP if message == omnibox::DISMISS || wparam == 0 => {
                    browser.dismiss_suggestions();
                    Ok(())
                }
                WM_COMMAND if wparam >> 16 == 0 || wparam >> 16 == 1 && lparam == 0 => {
                    browser.command(wparam as u16)
                }
                WM_CONTEXTMENU => {
                    if let Some((index, point)) =
                        browser.toolbar.tab_context(wparam as HWND, lparam)
                    {
                        browser.tab_context_menu(index, point)
                    } else {
                        let (position, index) =
                            browser.toolbar.bookmark_context(wparam as HWND, lparam)?;
                        browser.bookmark_context_menu(position, 0, index)
                    }
                }
                BOOKMARK_DRAG => {
                    let target = browser.toolbar.bookmark_drop_at(lparam, wparam);
                    browser.toolbar.show_bookmark_drop(target);
                    Ok(())
                }
                BOOKMARK_DRAG_CANCEL => {
                    browser.toolbar.show_bookmark_drop(None);
                    Ok(())
                }
                BOOKMARK_DROP => {
                    let target = browser.toolbar.bookmark_drop_at(lparam, wparam);
                    browser.toolbar.show_bookmark_drop(None);
                    target.map_or(Ok(()), |target| browser.move_bar_item(wparam, target))
                }
                TAB_DRAG => {
                    let target = browser.toolbar.tab_drop_at(wparam, lparam);
                    browser
                        .toolbar
                        .show_tab_drop(target.map(|(_, _, after, hwnd)| (hwnd, after)));
                    Ok(())
                }
                TAB_DRAG_CANCEL => {
                    browser.toolbar.show_tab_drop(None);
                    Ok(())
                }
                TAB_DROP => {
                    let target = browser.toolbar.tab_drop_at(wparam, lparam);
                    browser.toolbar.show_tab_drop(None);
                    target.map_or(Ok(()), |(from, to, _, _)| browser.reorder_tab(from, to))
                }
                NAVIGATION_STATE => {
                    if let Some(index) = browser.tabs.iter().position(|tab| tab.id == wparam) {
                        let loading =
                            browser.tabs[index].page.borrow().connection == Connection::Connecting;
                        browser.toolbar.tab_loading(index, loading);
                    }
                    Ok(())
                }
                WM_TIMER if wparam == POLL_TIMER => browser.poll(),
                WM_TIMER if wparam == LOADING_TIMER => {
                    browser.toolbar.animate_loading();
                    Ok(())
                }
                WM_SIZE | REFRESH_LAYOUT => {
                    browser.dismiss_suggestions();
                    browser.layout();
                    Ok(())
                }
                WM_MOVE => {
                    browser.dismiss_suggestions();
                    if let Ok(tab) = browser.current() {
                        let _ = tab.controller.notify_parent_window_position_changed();
                    }
                    Ok(())
                }
                WM_DPICHANGED if lparam != 0 => {
                    // SAFETY: The operating system supplies a readable RECT
                    // for the lifetime of this message.
                    let suggested = unsafe { *(lparam as *const RECT) };
                    browser.change_dpi(wparam as u16 as i32, suggested)
                }
                FOCUS_CYCLE => {
                    if browser.toolbar.tab_focus(lparam as HWND, wparam != 0) {
                        Ok(())
                    } else {
                        let reason = if wparam == 0 {
                            MoveFocusReason::Next
                        } else {
                            MoveFocusReason::Previous
                        };
                        browser
                            .current()
                            .and_then(|tab| Ok(tab.controller.move_focus(reason)?))
                    }
                }
                _ => return None,
            };
            if let Err(error) = result {
                dialog(hwnd, "Brisa", &error.to_string(), MB_ICONWARNING);
            }
            Some(0)
        })
        .create()?;
    let hwnd = window.hwnd();
    crate::frame::integrate(hwnd)?;
    colors.attach(hwnd)?;
    profile_window.register(hwnd);
    let toolbar = Toolbar::new(hwnd, colors)?;
    toolbar.search_hint(preferences.search_engine);
    let environment = Environment::with_options(
        &EnvironmentOptions::new()
            .user_data_folder(path.to_string_lossy())
            .language(preferences.website_language.tag()),
    )?;
    let runtime = Rc::new(Runtime {
        hwnd,
        preferences: Cell::new(store.data.preferences),
        engine,
        popup_engine,
        site_exceptions: RefCell::default(),
        popup_exceptions: RefCell::default(),
        pending: RefCell::default(),
        download_confirmation: Cell::new(false),
    });
    let mut browser = Browser {
        suggestions: None,
        google_suggestions: omnibox::Remote::default(),
        toolbar,
        environment,
        runtime,
        store,
        profile_window,
        tabs: Vec::new(),
        active: 0,
        next_tab: 1,
        closed: VecDeque::new(),
        downloads: VecDeque::new(),
        update: None,
        update_download: None,
        update_check: default_profile
            .then(|| updates::Check::start(false).ok())
            .flatten(),
        next_update_check: default_profile.then(|| Instant::now() + UPDATE_INTERVAL),
        inspector: None,
        dock: crate::inspector::Side::Right,
        translation: None,
    };
    let restored = browser.store.data.pinned_tabs.clone();
    for url in restored {
        browser.new_tab(Some(&url), false)?;
        browser.move_tab_to_pins(browser.active, true, Some(url));
    }
    if let Some(center) = options.url.as_deref().and_then(CenterPage::from_address) {
        browser.open_center(center)?;
    } else {
        let initial = options
            .url
            .as_deref()
            .map(|input| protection::address(input).ok_or("Endereço inicial inválido."))
            .transpose()?;
        browser.new_tab(initial.as_ref().map(url::Url::as_str), false)?;
    }
    // WebView initialization can pump past the initial toolbar invalidation
    // while the main message handler is still waiting for the browser.
    browser.toolbar.repaint_all();
    app.replace(Some(browser));
    // SAFETY: Register a timer on the live window with no foreign callback.
    if unsafe { SetTimer(hwnd, POLL_TIMER, 150, None) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    post(hwnd, REFRESH_LAYOUT, 0);
    let result = chrome::run(hwnd);
    app.take();
    result?;
    Ok(())
}

fn post(hwnd: HWND, message: u32, wparam: usize) -> bool {
    // SAFETY: Posts only an integer payload. Windows discards messages for destroyed windows.
    unsafe { PostMessageW(hwnd, message, wparam, 0) != 0 }
}

fn dialog(hwnd: HWND, title: &str, body: &str, flags: u32) -> i32 {
    // SAFETY: The nul-terminated text is valid throughout this modal call.
    unsafe {
        MessageBoxW(
            hwnd,
            chrome::wide(body).as_ptr(),
            chrome::wide(title).as_ptr(),
            flags,
        )
    }
}

fn confirmed(hwnd: HWND, body: &str) -> bool {
    dialog(
        hwnd,
        "Brisa",
        body,
        MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
    ) == IDYES
}

pub fn show_error(message: &str) {
    dialog(std::ptr::null_mut(), "Brisa", message, MB_ICONERROR);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_settings_actions_require_the_settings_page() {
        assert!(!CenterCommand::ClearSites.allowed(CenterPage::Bookmarks));
        let languages = CenterCommand::SetLanguages {
            website: profile::Language::Italian,
            translation: profile::Language::Portuguese,
        };
        assert!(languages.allowed(CenterPage::Settings));
        assert!(!languages.allowed(CenterPage::History));
    }

    #[test]
    fn temporary_filter_exceptions_do_not_follow_subdomains_ports_or_private_tabs() {
        let runtime = Runtime {
            hwnd: std::ptr::null_mut(),
            preferences: Cell::new(Preferences::default()),
            engine: protection::engine(None),
            popup_engine: protection::popup_engine(None),
            site_exceptions: RefCell::new(HashSet::from([(false, "https://example.org".into())])),
            popup_exceptions: RefCell::default(),
            pending: RefCell::default(),
            download_confirmation: Cell::new(false),
        };
        assert!(!runtime.filter_enabled("https://example.org/article", false));
        assert!(runtime.filter_enabled("https://example.org/article", true));
        assert!(runtime.filter_enabled("https://other.example.org/article", false));
        assert!(runtime.filter_enabled("https://example.org:444/article", false));
        assert!(runtime.filter_enabled("http://example.org/article", false));
        assert!(site_key(HOME_URL).is_none());
    }

    #[test]
    fn popup_permission_does_not_bypass_network_filtering_or_follow_private_tabs() {
        let runtime = Runtime {
            hwnd: std::ptr::null_mut(),
            preferences: Cell::new(Preferences {
                cross_site_popups: false,
                ..Preferences::default()
            }),
            engine: protection::engine(Some("||generic-ad.example^")),
            popup_engine: protection::popup_engine(Some(
                "||ads.example^$popup\n@@||ads.example/session$popup".into(),
            )),
            site_exceptions: RefCell::default(),
            popup_exceptions: RefCell::new(HashSet::from([(false, "https://example.org".into())])),
            pending: RefCell::default(),
            download_confirmation: Cell::new(false),
        };
        let source = "https://example.org";
        let login = protection::web_url("https://login.example/session").unwrap();
        let ad = protection::web_url("https://ads.example/banner").unwrap();
        let allowed_ad_origin = protection::web_url("https://ads.example/session").unwrap();
        let generic_ad = protection::web_url("https://generic-ad.example/banner").unwrap();
        let insecure = protection::web_url("http://login.example/session").unwrap();
        assert!(runtime.popup_block(&login, source, false).is_none());
        assert!(runtime.popup_block(&login, source, true) == Some(Connection::BlockedPopup));
        assert!(
            runtime.popup_block(&login, "https://other.example.org", false)
                == Some(Connection::BlockedPopup)
        );
        assert!(runtime.popup_block(&ad, source, false) == Some(Connection::BlockedRedirect));
        assert!(
            runtime
                .popup_block(&allowed_ad_origin, source, false)
                .is_none()
        );
        assert!(
            runtime.popup_block(&generic_ad, source, false) == Some(Connection::BlockedRedirect)
        );
        assert!(!protection::request_blocked(
            &runtime.engine,
            ad.as_str(),
            source,
            "document"
        ));
        assert!(runtime.popup_block(&insecure, source, false) == Some(Connection::BlockedHttp));
    }
}
