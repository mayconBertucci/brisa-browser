use serde::{Deserialize, Serialize};
use windows_webview::WebResourceResponse;

use crate::profile::{Accent, BookmarkKey, BookmarkPlacement, Language, Theme, VisitKey};
use crate::protection::SearchEngine;

const HTML: &str = include_str!("center.html");
const JS: &str = include_str!("center.js");
const CSS: &str = include_str!("center.css");
pub const URL: &str = "https://brisa.invalid/center";

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Page {
    #[serde(rename = "marcadores")]
    Bookmarks,
    #[serde(rename = "historico")]
    History,
    #[serde(rename = "configuracoes")]
    Settings,
    #[serde(rename = "protecao")]
    Protection,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Self::Bookmarks => "Marcadores",
            Self::History => "Histórico",
            Self::Settings => "Configurações",
            Self::Protection => "Bloqueios",
        }
    }

    pub fn address(self) -> &'static str {
        match self {
            Self::Bookmarks => "brisa://marcadores",
            Self::History => "brisa://historico",
            Self::Settings => "brisa://configuracoes",
            Self::Protection => "brisa://protecao",
        }
    }

    pub fn from_address(address: &str) -> Option<Self> {
        [
            Self::Bookmarks,
            Self::History,
            Self::Settings,
            Self::Protection,
        ]
        .into_iter()
        .find(|page| address.trim().trim_end_matches('/') == page.address())
    }
}

pub fn response(uri: &str, method: &str) -> WebResourceResponse {
    if method != "GET" {
        return denied();
    }
    let (body, mime) = match uri {
        URL => (HTML, "text/html; charset=utf-8"),
        "https://brisa.invalid/center.js" => (JS, "text/javascript; charset=utf-8"),
        "https://brisa.invalid/center.css" => (CSS, "text/css; charset=utf-8"),
        _ => return denied(),
    };
    WebResourceResponse::new(body.as_bytes())
        .content_type(mime)
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Security-Policy", "default-src 'none'; style-src 'self'; script-src 'self'; img-src data:; base-uri 'none'; frame-ancestors 'none'; form-action 'none'")
}

#[derive(Deserialize)]
pub struct Message {
    pub section: Option<Page>,
    #[serde(flatten)]
    pub command: Command,
}

impl Message {
    pub fn parse(json: &str) -> Option<Self> {
        // A history selection can include up to 200 URLs of 2 KiB each.
        if json.len() > 1024 * 1024 {
            return None;
        }
        let message: Self = serde_json::from_str(json).ok()?;
        (json.len() <= 8192 || message.section == Some(Page::History)).then_some(message)
    }
}

fn denied() -> WebResourceResponse {
    WebResourceResponse::new(Vec::new()).status(403, "Blocked by Brisa")
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum Command {
    Ready,
    Show {
        page: Page,
    },
    Open {
        url: String,
    },
    AddFolder {
        parent: u64,
        title: String,
    },
    RenameFolder {
        id: u64,
        title: String,
    },
    RemoveFolder {
        id: u64,
    },
    EditBookmark {
        url: String,
        title: String,
        folder: u64,
    },
    MoveBookmark {
        item: BookmarkKey,
        placement: BookmarkPlacement,
    },
    RemoveBookmark {
        url: String,
    },
    RemoveVisits {
        visits: Vec<VisitKey>,
    },
    ClearHistory,
    SetPreference {
        name: String,
        checked: bool,
    },
    SetAppearance {
        theme: Theme,
        accent: Accent,
    },
    SetSearchEngine {
        engine: SearchEngine,
    },
    SetLanguages {
        website: Language,
        translation: Language,
    },
    ClearSites,
    ClearCache,
    ChooseDownloadFolder,
}

impl Command {
    pub fn allowed(&self, page: Page) -> bool {
        use Command::*;
        match self {
            Ready | Show { .. } => true,
            Open { .. } => matches!(page, Page::Bookmarks | Page::History),
            AddFolder { .. }
            | RenameFolder { .. }
            | RemoveFolder { .. }
            | EditBookmark { .. }
            | MoveBookmark { .. }
            | RemoveBookmark { .. } => page == Page::Bookmarks,
            RemoveVisits { .. } => page == Page::History,
            ClearHistory => matches!(page, Page::History | Page::Settings),
            SetPreference { .. }
            | SetAppearance { .. }
            | SetSearchEngine { .. }
            | SetLanguages { .. }
            | ClearSites
            | ClearCache
            | ChooseDownloadFolder => page == Page::Settings,
        }
    }
}
