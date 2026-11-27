use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::protection;

const MAX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BOOKMARKS: usize = 150;
const MAX_HISTORY: usize = 200;
const MAX_SAVED_URL: usize = 2048;

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    #[default]
    Jade,
    Blue,
    Violet,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum Language {
    #[default]
    #[serde(rename = "pt-BR")]
    Portuguese,
    #[serde(rename = "en-US")]
    English,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "it")]
    Italian,
    #[serde(rename = "de")]
    German,
}

impl Language {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Portuguese => "pt-BR",
            Self::English => "en-US",
            Self::Spanish => "es",
            Self::French => "fr",
            Self::Italian => "it",
            Self::German => "de",
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    pub https_only: bool,
    pub filter_network: bool,
    pub cross_site_popups: bool,
    pub show_bookmarks: bool,
    pub remember_history: bool,
    pub theme: Theme,
    pub accent: Accent,
    pub search_engine: protection::SearchEngine,
    pub search_engine_selected: bool,
    pub google_suggestions: bool,
    pub website_language: Language,
    pub translation_language: Language,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            https_only: true,
            filter_network: true,
            cross_site_popups: true,
            show_bookmarks: true,
            remember_history: true,
            theme: Theme::Light,
            accent: Accent::Jade,
            search_engine: protection::SearchEngine::default(),
            search_engine_selected: false,
            google_suggestions: true,
            website_language: Language::default(),
            translation_language: Language::default(),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Link {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub folder: u64,
    #[serde(default)]
    pub visited_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) order: Option<u16>,
}

#[derive(Deserialize)]
pub struct VisitKey {
    url: String,
    time: u64,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Folder {
    pub id: u64,
    pub parent: u64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    order: Option<u16>,
}

#[derive(PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BookmarkKey {
    Folder { id: u64 },
    Link { url: String },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BookmarkPlacement {
    Before { item: BookmarkKey },
    After { item: BookmarkKey },
    Into { folder: u64 },
}

#[derive(Clone, Copy)]
pub enum BookmarkItem<'a> {
    Folder(&'a Folder),
    Link(&'a Link),
}

impl BookmarkItem<'_> {
    pub fn key(self) -> BookmarkKey {
        match self {
            Self::Folder(folder) => BookmarkKey::Folder { id: folder.id },
            Self::Link(link) => BookmarkKey::Link {
                url: link.url.clone(),
            },
        }
    }

    fn parent(self) -> u64 {
        match self {
            Self::Folder(folder) => folder.parent,
            Self::Link(link) => link.folder,
        }
    }

    fn order(self) -> u16 {
        match self {
            Self::Folder(folder) => folder.order,
            Self::Link(link) => link.order,
        }
        .unwrap_or(0)
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Data {
    pub preferences: Preferences,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_folder: Option<String>,
    pub folders: Vec<Folder>,
    pub bookmarks: Vec<Link>,
    pub history: Vec<Link>,
    pub shelf: Vec<Link>,
    pub pinned_tabs: Vec<String>,
}

impl Data {
    pub fn bookmark_items(&self, folder: u64) -> Vec<BookmarkItem<'_>> {
        let mut items: Vec<_> = self
            .folders
            .iter()
            .filter(|item| item.parent == folder)
            .map(BookmarkItem::Folder)
            .chain(
                self.bookmarks
                    .iter()
                    .filter(|item| item.folder == folder)
                    .map(BookmarkItem::Link),
            )
            .collect();
        items.sort_by_key(|item| item.order());
        items
    }

    fn bookmark_item(&self, key: &BookmarkKey) -> Option<BookmarkItem<'_>> {
        match key {
            BookmarkKey::Folder { id } => self
                .folders
                .iter()
                .find(|folder| folder.id == *id)
                .map(BookmarkItem::Folder),
            BookmarkKey::Link { url } => self
                .bookmarks
                .iter()
                .find(|link| link.url == *url)
                .map(BookmarkItem::Link),
        }
    }

    pub fn move_bookmark(
        &mut self,
        item: &BookmarkKey,
        placement: &BookmarkPlacement,
    ) -> Result<(), &'static str> {
        self.bookmark_item(item).ok_or("Item não encontrado.")?;
        let (destination, anchor, after) = match placement {
            BookmarkPlacement::Into { folder } => (*folder, None, false),
            BookmarkPlacement::Before { item } | BookmarkPlacement::After { item } => (
                self.bookmark_item(item)
                    .ok_or("O marcador de destino não existe.")?
                    .parent(),
                Some(item),
                matches!(placement, BookmarkPlacement::After { .. }),
            ),
        };
        if anchor == Some(item) {
            return Ok(());
        }
        if !self.folder_exists(destination) {
            return Err("A pasta de destino não existe.");
        }
        if let BookmarkKey::Folder { id } = item
            && !valid_ancestry(destination, *id, self.folders.len(), |parent| {
                self.folders
                    .iter()
                    .find(|folder| folder.id == parent)
                    .map(|folder| folder.parent)
            })
        {
            return Err("Uma pasta não pode ser movida para dentro de si mesma.");
        }
        let items: Vec<_> = self
            .bookmark_items(destination)
            .into_iter()
            .map(BookmarkItem::key)
            .filter(|key| key != item)
            .collect();
        let position = anchor
            .and_then(|anchor| items.iter().position(|key| key == anchor))
            .map_or(items.len(), |index| index + usize::from(after));
        let (before, after) = items.split_at(position);
        let order = before.iter().chain([item]).chain(after);
        for (index, key) in order.enumerate() {
            let order = Some(index as u16 + 1);
            match key {
                BookmarkKey::Folder { id } => {
                    if let Some(folder) = self.folders.iter_mut().find(|folder| folder.id == *id) {
                        folder.parent = destination;
                        folder.order = order;
                    }
                }
                BookmarkKey::Link { url } => {
                    if let Some(link) = self.bookmarks.iter_mut().find(|link| link.url == *url) {
                        link.folder = destination;
                        link.order = order;
                    }
                }
            }
        }
        Ok(())
    }
    pub fn remove_visits(&mut self, visits: &[VisitKey]) -> Result<(), &'static str> {
        if visits.len() > MAX_HISTORY {
            return Err("O histórico mudou. Confira as visitas selecionadas.");
        }
        for visit in visits {
            if let Some(index) = self
                .history
                .iter()
                .position(|link| link.url == visit.url && link.visited_at == visit.time)
            {
                self.history.remove(index);
            }
        }
        Ok(())
    }

    pub fn folder_exists(&self, id: u64) -> bool {
        id == 0 || self.folders.iter().any(|folder| folder.id == id)
    }

    pub fn add_folder(&mut self, parent: u64, title: &str) -> Result<(), &'static str> {
        if !self.folder_exists(parent) {
            return Err("A pasta superior não existe.");
        }
        if self.folders.len() >= 128 {
            return Err("Limite de pastas atingido.");
        }
        let title = name(title)?;
        let id = self
            .folders
            .iter()
            .map(|folder| folder.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|id| *id <= u32::MAX as u64)
            .ok_or("Limite de pastas atingido.")?;
        self.folders.push(Folder {
            id,
            parent,
            title,
            order: None,
        });
        Ok(())
    }

    pub fn rename_folder(&mut self, id: u64, title: &str) -> Result<(), &'static str> {
        let folder = self
            .folders
            .iter_mut()
            .find(|folder| folder.id == id)
            .ok_or("Pasta não encontrada.")?;
        folder.title = name(title)?;
        Ok(())
    }

    pub fn remove_folder(&mut self, id: u64) -> Result<(), &'static str> {
        let parent = self
            .folders
            .iter()
            .find(|folder| folder.id == id)
            .ok_or("Pasta não encontrada.")?
            .parent;
        self.folders.retain(|folder| folder.id != id);
        for folder in &mut self.folders {
            if folder.parent == id {
                folder.parent = parent;
            }
        }
        for bookmark in &mut self.bookmarks {
            if bookmark.folder == id {
                bookmark.folder = parent;
            }
        }
        Ok(())
    }

    pub fn edit_bookmark(
        &mut self,
        url: &str,
        title: &str,
        folder: u64,
    ) -> Result<(), &'static str> {
        if !self.folder_exists(folder) {
            return Err("A pasta de destino não existe.");
        }
        let bookmark = self
            .bookmarks
            .iter_mut()
            .find(|link| link.url == url)
            .ok_or("Marcador não encontrado.")?;
        bookmark.title = name(title)?;
        bookmark.folder = folder;
        Ok(())
    }

    pub fn edit_bar_bookmark(
        &mut self,
        previous: &str,
        title: &str,
        address: &str,
    ) -> Result<(), &'static str> {
        let target = protection::web_url(address.trim())
            .filter(|url| {
                url.host_str() != Some("brisa.invalid") && url.as_str().len() <= MAX_SAVED_URL
            })
            .ok_or("Digite um endereço HTTP ou HTTPS válido.")?;
        if self
            .bookmarks
            .iter()
            .any(|link| link.url == target.as_str() && link.url != previous)
        {
            return Err("Este endereço já está entre os marcadores.");
        }
        let title = name(title)?;
        let bookmark = self
            .bookmarks
            .iter_mut()
            .find(|link| link.url == previous)
            .ok_or("Marcador não encontrado.")?;
        bookmark.title = title;
        if bookmark.url != target.as_str() {
            bookmark.favicon = None;
        }
        bookmark.url = target.into();
        Ok(())
    }
}

fn name(title: &str) -> Result<String, &'static str> {
    let title = display_title(title.trim(), 120);
    if title.is_empty() {
        Err("Digite um nome.")
    } else {
        Ok(title)
    }
}

pub struct Store {
    pub data: Data,
    path: PathBuf,
    _lock: File,
}

impl Store {
    pub fn open(directory: &Path) -> io::Result<Self> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("brisa.lock"))?;
        lock.try_lock().map_err(|error| match error {
            fs::TryLockError::WouldBlock => io::Error::new(
                io::ErrorKind::WouldBlock,
                "Este perfil do Brisa já está aberto. Feche a janela anterior do Brisa e tente de novo.",
            ),
            fs::TryLockError::Error(error) => io::Error::new(
                error.kind(),
                format!("Não foi possível abrir o perfil do Brisa. {error}"),
            ),
        })?;
        let path = directory.join("brisa.json");
        let mut data: Data = match File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Perfil do Brisa excede o limite.",
                    ));
                }
                serde_json::from_slice(&bytes)?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Data::default(),
            Err(error) => return Err(error),
        };
        if !data.preferences.search_engine_selected
            && matches!(
                data.preferences.search_engine,
                protection::SearchEngine::DuckDuckGo
            )
        {
            data.preferences.search_engine = protection::SearchEngine::Google;
        }
        sanitize_links(&mut data.bookmarks, MAX_BOOKMARKS);
        sanitize_links(&mut data.history, MAX_HISTORY);
        sanitize_links(&mut data.shelf, MAX_BOOKMARKS);
        data.pinned_tabs.retain(|address| pinned_address(address));
        data.pinned_tabs.truncate(crate::chrome::MAX_TABS - 1);
        data.folders.truncate(128);
        let mut ids = HashSet::new();
        data.folders.retain(|folder| {
            folder.id > 0 && folder.id <= u32::MAX as u64 && ids.insert(folder.id)
        });
        let parents: HashMap<_, _> = data
            .folders
            .iter()
            .map(|folder| (folder.id, folder.parent))
            .collect();
        for folder in &mut data.folders {
            folder.title = name(&folder.title).unwrap_or_else(|_| "Pasta".to_owned());
            if !valid_ancestry(folder.parent, folder.id, parents.len(), |id| {
                parents.get(&id).copied()
            }) {
                folder.parent = 0;
            }
        }
        for link in &mut data.bookmarks {
            if link.folder != 0 && !ids.contains(&link.folder) {
                link.folder = 0;
            }
        }
        Ok(Self {
            data,
            path,
            _lock: lock,
        })
    }

    pub fn save(&self) -> io::Result<()> {
        let bytes = serde_json::to_vec(&self.data)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Perfil do Brisa excede o limite.",
            ));
        }
        let temporary = self.path.with_extension("json.tmp");
        let mut file = File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary, &self.path)
    }
}

fn valid_ancestry(
    mut parent: u64,
    child: u64,
    limit: usize,
    parent_of: impl Fn(u64) -> Option<u64>,
) -> bool {
    for _ in 0..=limit {
        if parent == 0 {
            return true;
        }
        if parent == child {
            return false;
        }
        let Some(next) = parent_of(parent) else {
            return false;
        };
        parent = next;
    }
    false
}

fn sanitize_links(links: &mut Vec<Link>, limit: usize) {
    links
        .retain(|link| link.url.len() <= MAX_SAVED_URL && protection::web_url(&link.url).is_some());
    links.truncate(limit);
    for link in links {
        link.title = display_title(&link.title, 120);
        if link
            .favicon
            .as_deref()
            .is_some_and(|icon| crate::favicon::decode(icon).is_none())
        {
            link.favicon = None;
        }
    }
}

pub fn pinned_address(address: &str) -> bool {
    address.len() <= MAX_SAVED_URL
        && protection::web_url(address).is_some_and(|url| url.host_str() != Some("brisa.invalid"))
}

pub fn bookmark_title<'a>(title: &'a str, address: &str) -> &'a str {
    if protection::web_url(address).is_some_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("mail.google.com")
            && url.path().starts_with("/mail/")
            && url.fragment().is_none_or(|fragment| fragment == "inbox")
    }) {
        "Gmail"
    } else {
        title
    }
}

pub fn display_title(title: &str, limit: usize) -> String {
    title
        .chars()
        .filter(|character| {
            !character.is_control()
                && !matches!(character, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(limit)
        .collect()
}

pub fn remember(links: &mut Vec<Link>, title: &str, url: &str, history: bool) {
    if url.len() > MAX_SAVED_URL || protection::web_url(url).is_none() {
        return;
    }
    if history {
        if links.first().is_some_and(|link| link.url == url) {
            links.remove(0);
        }
    } else {
        links.retain(|link| link.url != url);
    }
    links.insert(
        0,
        Link {
            title: display_title(title, 120),
            url: url.to_owned(),
            folder: 0,
            favicon: None,
            order: None,
            visited_at: if history {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_millis() as u64)
            } else {
                0
            },
        },
    );
    links.truncate(if history { MAX_HISTORY } else { MAX_BOOKMARKS });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_bookmarks_across_folders_and_rejects_folder_cycles() {
        let mut data = Data::default();
        data.add_folder(0, "Primeira").unwrap();
        data.add_folder(0, "Segunda").unwrap();
        let first = BookmarkKey::Folder { id: 1 };
        let second = BookmarkKey::Folder { id: 2 };
        let url = "https://example.org/";
        remember(&mut data.bookmarks, "Exemplo", url, false);
        let link = BookmarkKey::Link { url: url.into() };

        data.move_bookmark(&link, &BookmarkPlacement::Before { item: first })
            .unwrap();
        assert!(data.bookmark_items(0)[0].key() == link);

        data.move_bookmark(
            &BookmarkKey::Folder { id: 1 },
            &BookmarkPlacement::Into { folder: 2 },
        )
        .unwrap();
        assert_eq!(data.folders[0].parent, 2);
        assert!(
            data.move_bookmark(&second, &BookmarkPlacement::Into { folder: 1 })
                .is_err()
        );
        assert_eq!(data.folders[1].parent, 0);

        data.move_bookmark(&link, &BookmarkPlacement::Into { folder: 2 })
            .unwrap();
        assert!(data.bookmark_items(2)[1].key() == link);
        data.move_bookmark(&link, &BookmarkPlacement::Into { folder: 0 })
            .unwrap();
        assert!(data.bookmark_items(0)[1].key() == link);
    }

    #[test]
    fn bookmark_name_uses_gmail_only_for_the_inbox() {
        assert_eq!(
            bookmark_title(
                "Caixa de entrada",
                "https://mail.google.com/mail/u/0/#inbox"
            ),
            "Gmail"
        );
        assert_eq!(
            bookmark_title(
                "Uma mensagem",
                "https://mail.google.com/mail/u/0/#inbox/test"
            ),
            "Uma mensagem"
        );
        assert_eq!(
            bookmark_title("Página", "https://mail.google.com.example/mail/u/0/#inbox"),
            "Página"
        );
    }

    #[test]
    fn legacy_links_remain_available_and_removing_a_folder_preserves_content() {
        let mut data: Data = serde_json::from_str(
            r#"{"bookmarks":[{"title":"Exemplo","url":"https://example.org/"}]}"#,
        )
        .expect("legacy profile parses");
        assert_eq!(data.bookmarks[0].folder, 0);
        data.add_folder(0, "Estudos").expect("root folder");
        data.add_folder(1, "Referências").expect("nested folder");
        data.edit_bookmark("https://example.org/", "Exemplo editado", 1)
            .expect("move to folder");
        data.remove_folder(1).expect("remove first folder");
        assert_eq!(data.folders[0].parent, 0);
        assert_eq!(data.bookmarks[0].folder, 0);
        assert_eq!(data.bookmarks[0].title, "Exemplo editado");
    }

    #[test]
    fn replaces_profile_atomically_and_keeps_saved_links() -> io::Result<()> {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::current_dir()?
            .join("target")
            .join(format!("brisa-profile-{}-{suffix}", std::process::id()));
        fs::create_dir(&directory)?;
        let result = (|| {
            fs::write(
                directory.join("brisa.json"),
                r#"{"preferences":{"search_engine":"duck_duck_go"}}"#,
            )?;
            let mut store = Store::open(&directory)?;
            assert!(matches!(
                store.data.preferences.search_engine,
                protection::SearchEngine::Google
            ));
            assert!(Store::open(&directory).is_err());
            remember(
                &mut store.data.bookmarks,
                "Exemplo",
                "https://example.org/",
                false,
            );
            store.save()?;
            store.data.preferences.show_bookmarks = false;
            store.data.preferences.search_engine = protection::SearchEngine::DuckDuckGo;
            store.data.preferences.search_engine_selected = true;
            store.save()?;
            drop(store);
            let restored = Store::open(&directory)?;
            assert!(!restored.data.preferences.show_bookmarks);
            assert!(matches!(
                restored.data.preferences.search_engine,
                protection::SearchEngine::DuckDuckGo
            ));
            assert_eq!(
                restored
                    .data
                    .bookmarks
                    .first()
                    .map(|link| link.url.as_str()),
                Some("https://example.org/")
            );
            Ok(())
        })();
        for name in ["brisa.json", "brisa.json.tmp", "brisa.lock"] {
            match fs::remove_file(directory.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        fs::remove_dir(directory)?;
        result
    }
}
