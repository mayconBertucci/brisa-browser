use std::cell::LazyCell;

use adblock::{
    Engine,
    lists::{FilterSet, ParseOptions, RuleTypes},
    request::Request,
};
use url::{Host, Url};

#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchEngine {
    DuckDuckGo,
    #[default]
    Google,
    Brave,
    Bing,
}

impl SearchEngine {
    pub fn name(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Google => "Google",
            Self::Brave => "Brave Search",
            Self::Bing => "Bing",
        }
    }

    fn base(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "https://duckduckgo.com/",
            Self::Google => "https://www.google.com/search",
            Self::Brave => "https://search.brave.com/search",
            Self::Bing => "https://www.bing.com/search",
        }
    }
}

const EASY_LIST: &str = include_str!("../assets/easylist.txt");
const EASY_PRIVACY: &str = include_str!("../assets/easyprivacy.txt");

pub fn engine(extra_rules: Option<&str>) -> Engine {
    let options = ParseOptions {
        rule_types: RuleTypes::NetworkOnly,
        ..ParseOptions::default()
    };
    let mut filters = FilterSet::new(false);
    filters.add_filter_list(EASY_LIST, options);
    filters.add_filter_list(EASY_PRIVACY, options);
    if let Some(rules) = extra_rules {
        filters.add_filter_list(rules, options);
    }
    Engine::from_filter_set(filters, true)
}

pub type PopupEngine = LazyCell<Engine, Box<dyn FnOnce() -> Engine>>;

pub fn popup_engine(extra_rules: Option<String>) -> PopupEngine {
    LazyCell::new(Box::new(move || build_popup_engine(extra_rules.as_deref())))
}

fn build_popup_engine(extra_rules: Option<&str>) -> Engine {
    let options = ParseOptions {
        rule_types: RuleTypes::NetworkOnly,
        ..ParseOptions::default()
    };
    let mut filters = FilterSet::new(false);
    for list in [EASY_LIST, EASY_PRIVACY].into_iter().chain(extra_rules) {
        for (pattern, modifiers) in list.lines().filter_map(|line| line.rsplit_once('$')) {
            if !modifiers.split(',').any(|modifier| modifier == "popup") {
                continue;
            }
            // adblock-rust does not expose a popup request type. Match these
            // rules as documents in an engine used only for popup navigation.
            let mut rule = format!("{pattern}$document");
            for modifier in modifiers
                .split(',')
                .filter(|modifier| !matches!(*modifier, "popup" | "document" | "~document"))
            {
                rule.push(',');
                rule.push_str(modifier);
            }
            let _ = filters.add_filter(&rule, options);
        }
    }
    Engine::from_filter_set(filters, true)
}

pub fn popup_blocked(network: &Engine, popup: &PopupEngine, url: &str, source: &str) -> bool {
    let (Ok(document), Ok(other)) = (
        Request::new(url, source, "document"),
        Request::new(url, source, "other"),
    ) else {
        return false;
    };
    let document_match = network.check_network_request(&document);
    if document_match.important {
        return true;
    }
    let network_match = network.check_network_request_subset(&other, document_match.matched, false);
    network_match.important
        || popup
            .check_network_request_subset(&document, network_match.matched, false)
            .matched
}

pub fn web_url(input: &str) -> Option<Url> {
    if input.len() > 16_384 || input.chars().any(char::is_control) {
        return None;
    }
    Url::parse(input)
        .ok()
        .filter(|url| matches!(url.scheme(), "https" | "http"))
        .filter(|url| url.has_host() && url.username().is_empty() && url.password().is_none())
}

pub fn address(input: &str) -> Option<Url> {
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_whitespace) {
        return None;
    }
    if input.contains("://") {
        return web_url(input);
    }
    web_url(&format!("https://{input}"))
}

pub fn address_or_search(input: &str, engine: SearchEngine) -> Option<Url> {
    let input = input.trim();
    if input.is_empty() || input.len() > 4096 || input.chars().any(char::is_control) {
        return None;
    }
    if looks_like_address(input) {
        return address(input);
    }
    let mut search = Url::parse(engine.base()).ok()?;
    search.query_pairs_mut().append_pair("q", input);
    Some(search)
}

pub(crate) fn looks_like_address(input: &str) -> bool {
    input.contains("://")
        || input.contains(':')
        || input.contains('@')
        || !input.contains(char::is_whitespace) && (input.contains('.') || input == "localhost")
}

pub fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => {
            let host = host.trim_end_matches('.');
            host == "localhost" || host.ends_with(".localhost")
        }
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
        }
        None => false,
    }
}

pub fn is_local_network(url: &Url) -> bool {
    let local_ipv4 = |ip: std::net::Ipv4Addr| {
        ip.is_unspecified() || ip.is_loopback() || ip.is_private() || ip.is_link_local()
    };
    match url.host() {
        Some(Host::Domain(_)) => is_loopback(url),
        Some(Host::Ipv4(ip)) => local_ipv4(ip),
        Some(Host::Ipv6(ip)) => {
            ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.to_ipv4_mapped().is_some_and(local_ipv4)
        }
        None => false,
    }
}

pub fn local_network_allowed(target: &Url, source: &str) -> bool {
    !is_local_network(target) || web_url(source).is_some_and(|source| is_local_network(&source))
}

pub fn transport_allowed(url: &Url, https_only: bool) -> bool {
    url.scheme() == "https" || (url.scheme() == "http" && (!https_only || is_loopback(url)))
}

pub fn download_origin(uri: &str, source: &str, https_only: bool) -> Option<Url> {
    let target = if let Some(blob) = uri.strip_prefix("blob:") {
        let target = web_url(blob)?;
        let source = web_url(source)?;
        if target.origin() != source.origin() {
            return None;
        }
        target
    } else {
        web_url(uri)?
    };
    (target.host_str() != Some("brisa.invalid")
        && transport_allowed(&target, https_only)
        && local_network_allowed(&target, source))
    .then_some(target)
}

pub fn popup_allowed(
    target: &Url,
    source: &str,
    user_initiated: bool,
    cross_site_allowed: bool,
    https_only: bool,
) -> bool {
    let Some(source) = web_url(source) else {
        return false;
    };
    user_initiated
        && transport_allowed(target, https_only)
        && local_network_allowed(target, source.as_str())
        && (cross_site_allowed || target.origin() == source.origin())
}

pub fn request_blocked(engine: &Engine, url: &str, source: &str, kind: &str) -> bool {
    Request::new(url, source, kind)
        .is_ok_and(|request| engine.check_network_request(&request).matched)
}

pub fn should_block(
    engine: &Engine,
    target: &str,
    document: &str,
    headers: &[(String, String)],
) -> bool {
    let header = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    let destination = header("Sec-Fetch-Dest");
    if target == document && destination.is_none() {
        return false;
    }
    let kind = match destination {
        Some("document") => return false,
        Some("iframe" | "frame") => "subdocument",
        Some("script" | "worker" | "sharedworker" | "serviceworker") => "script",
        Some("image") => "image",
        Some("style") => "stylesheet",
        Some("font") => "font",
        Some("audio" | "video" | "track") => "media",
        Some("object" | "embed") => "object",
        Some("empty") if header("Sec-Fetch-Mode") != Some("no-cors") => "xmlhttprequest",
        None => infer_resource_type(target, header("Accept")),
        _ => "other",
    };
    let source = header("Referer")
        .or_else(|| header("Origin").filter(|origin| *origin != "null"))
        .unwrap_or(document);
    request_blocked(engine, target, source, kind)
}

fn infer_resource_type(target: &str, accept: Option<&str>) -> &'static str {
    // Some WebView2 requests are surfaced before Fetch Metadata headers are added.
    if let Some(accept) = accept {
        if accept.starts_with("image/") {
            return "image";
        }
        if accept.starts_with("text/css") {
            return "stylesheet";
        }
    }
    let Ok(url) = Url::parse(target) else {
        return "other";
    };
    match url.path().rsplit_once('.').map(|(_, extension)| extension) {
        Some("js" | "mjs") => "script",
        Some("png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "avif" | "ico") => "image",
        Some("css") => "stylesheet",
        Some("woff" | "woff2" | "ttf" | "otf") => "font",
        Some("mp4" | "webm" | "mp3" | "ogg") => "media",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_downloads_from_permitted_sources() {
        let site = "https://example.org/";
        assert!(download_origin("https://files.example.org/report.pdf", site, true).is_some());
        assert!(download_origin("blob:https://example.org/id", site, true).is_some());
        assert!(download_origin("blob:https://example.net/id", site, true).is_none());
        assert!(download_origin("http://example.org/report.pdf", site, true).is_none());
        assert!(download_origin("https://192.168.0.1/report.pdf", site, true).is_none());
        assert!(
            download_origin("http://127.0.0.1/report.pdf", "http://127.0.0.1/", true).is_some()
        );
    }

    #[test]
    fn protects_local_addresses_without_blocking_public_sites_or_local_sessions() {
        let public = "https://example.org/";
        let local = "http://localhost:4000/";
        for address in [
            "https://0.0.0.0/",
            "https://192.168.1.1/",
            "https://169.254.169.254/",
            "https://[::ffff:10.0.0.1]/",
            "https://[fd00::1]/",
            "https://lab.localhost/",
        ] {
            let target = web_url(address).unwrap();
            assert!(!local_network_allowed(&target, public));
            assert!(local_network_allowed(&target, local));
            assert!(!popup_allowed(&target, public, true, true, true));
        }
        assert!(local_network_allowed(&web_url(public).unwrap(), public));
        assert!(transport_allowed(&web_url(local).unwrap(), true));
        assert!(!transport_allowed(
            &web_url("http://192.168.1.1/").unwrap(),
            true
        ));
    }

    #[test]
    fn recognizes_web_addresses_and_rejects_other_schemes() {
        assert_eq!(
            address("example.org").unwrap().as_str(),
            "https://example.org/"
        );
        assert!(address("http://127.0.0.1:8100/").is_some());
        assert!(address("javascript:alert(1)").is_none());
        assert!(address("file:///C:/secret.txt").is_none());
        assert!(address("https://user:password@example.org").is_none());
    }

    #[test]
    fn protects_transport_and_blocks_unrequested_popups() {
        let secure = web_url("https://example.org/open").unwrap();
        let insecure = web_url("http://example.org/open").unwrap();
        let local = web_url("http://127.0.0.1:9000/").unwrap();
        assert!(transport_allowed(&secure, true));
        assert!(!transport_allowed(&insecure, true));
        assert!(transport_allowed(&insecure, false));
        assert!(transport_allowed(&local, true));
        for alias in [
            "http://localhost./",
            "http://dev.localhost/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
        ] {
            let alias = web_url(alias).unwrap();
            assert!(is_loopback(&alias));
            assert!(transport_allowed(&alias, true));
        }
        assert!(!is_loopback(&web_url("http://localhost.example/").unwrap()));
        assert!(popup_allowed(
            &secure,
            "https://example.org/",
            true,
            false,
            true
        ));
        assert!(!popup_allowed(
            &secure,
            "https://example.org/",
            false,
            true,
            true
        ));
        assert!(!popup_allowed(
            &secure,
            "https://outro.example/",
            true,
            false,
            true
        ));
        assert!(!popup_allowed(
            &local,
            "https://example.org/",
            true,
            true,
            true
        ));
        assert!(popup_allowed(
            &secure,
            "https://outro.example/",
            true,
            true,
            true
        ));
        assert!(address_or_search("javascript:alert(1)", SearchEngine::default()).is_none());
        assert!(
            address_or_search("https://user:password@example.org", SearchEngine::default())
                .is_none()
        );
        assert_eq!(
            address_or_search("navegador leve", SearchEngine::default())
                .unwrap()
                .host_str(),
            Some("www.google.com")
        );
        let google = address_or_search("navegador & leve", SearchEngine::Google).unwrap();
        assert_eq!(google.host_str(), Some("www.google.com"));
        assert_eq!(google.query_pairs().next().unwrap().1, "navegador & leve");
    }

    #[test]
    fn network_rules_block_subresources_and_leave_the_page_available() {
        let rules = engine(Some("||fixture.test/ads/banner.png$image"));
        let source = "https://fixture.test/";
        let ad = "https://fixture.test/ads/banner.png";
        let image = [("Sec-Fetch-Dest".into(), "image".into())];
        let document = [("Sec-Fetch-Dest".into(), "document".into())];
        assert!(should_block(&rules, ad, source, &image));
        assert!(!should_block(&rules, ad, source, &document));
        assert!(!should_block(
            &rules,
            "https://fixture.test/logo.png",
            source,
            &image
        ));
        assert!(should_block(
            &rules,
            "https://www.google-analytics.com/analytics.js",
            source,
            &[("Sec-Fetch-Dest".into(), "script".into())],
        ));
        let local = engine(Some("/ad.svg|$image\n/tracker.js|$script"));
        let page = "http://127.0.0.1:8100/fixture/";
        assert!(should_block(&local, &format!("{page}ad.svg"), page, &[]));
        assert!(should_block(
            &local,
            &format!("{page}tracker.js"),
            page,
            &[]
        ));
    }
}
