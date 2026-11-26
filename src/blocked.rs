use url::Url;

const LIMIT: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Network,
    AdvertisingPopup,
    AutomaticPopup,
    RestrictedPopup,
    Redirect,
    Connection,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Network => "Anúncio ou rastreador",
            Self::AdvertisingPopup => "Pop-up publicitário",
            Self::AutomaticPopup => "Pop-up automático",
            Self::RestrictedPopup => "Pop-up restrito",
            Self::Redirect => "Redirecionamento filtrado",
            Self::Connection => "Conexão ou destino restrito",
        }
    }
}

pub struct Entry {
    pub kind: Kind,
    pub target: String,
    pub count: u64,
}

#[derive(Default)]
pub struct Log {
    pub entries: Vec<Entry>,
    pub unlisted: u64,
}

impl Log {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.unlisted = 0;
    }

    pub fn record(&mut self, kind: Kind, target: Option<&Url>) {
        let target = target.map_or_else(
            || "Sem endereço web".to_owned(),
            |url| {
                crate::profile::display_title(
                    &format!("{}{}", url.host_str().unwrap_or_default(), url.path()),
                    90,
                )
            },
        );
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.kind == kind && entry.target == target)
        {
            entry.count = entry.count.saturating_add(1);
        } else if self.entries.len() < LIMIT {
            self.entries.push(Entry {
                kind,
                target,
                count: 1,
            });
        } else {
            self.unlisted = self.unlisted.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_distinct_block_types_and_keeps_queries_off_the_display() {
        let mut log = Log::default();
        let url = Url::parse("https://ads.example/collect?secret=1#more").unwrap();
        log.record(Kind::Network, Some(&url));
        log.record(Kind::Network, Some(&url));
        log.record(Kind::AdvertisingPopup, Some(&url));
        assert_eq!(log.entries.len(), 2);
        assert_eq!(log.entries[0].count, 2);
        assert_eq!(log.entries[0].target, "ads.example/collect");
        for n in 0..LIMIT {
            let url = Url::parse(&format!("https://ads.example/{n}")).unwrap();
            log.record(Kind::Network, Some(&url));
        }
        assert_eq!(log.entries.len(), LIMIT);
        assert_eq!(log.unlisted, 2);
        log.clear();
        assert!(log.entries.is_empty());
    }
}
