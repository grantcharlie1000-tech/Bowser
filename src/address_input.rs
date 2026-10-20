//! What Enter does with text typed into the address field or the search field: an address, a search
//! with the right engine, a scoped nickname, a Ctrl+Enter domain expansion, an HTTPS upgrade, or a
//! refusal. Pure: the caller opens the page.

use url::Url;

use crate::{
    address_settings::{AddressSettings, Scope},
    navigation::{self, NavigationError, NavigationTarget, SearchProvider},
    search_engines::{Engine, EngineStore, SearchRequest},
};

/// How the text was submitted.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options<'a> {
    /// Alt+Enter or Shift+Enter.
    pub new_tab: bool,
    /// Ctrl+Enter.
    pub expand: bool,
    /// The text came from the search field, so searching is always allowed there.
    pub from_search_field: bool,
    /// A private window: its own default engine.
    pub private: bool,
    /// The engine chosen in the search field, used instead of the default.
    pub engine: Option<&'a str>,
    /// OB-8: the text is not something the user typed (a bookmark or group chip clicked, "Open
    /// all", a reopened tab, text selected on a page), so it is not remembered as typed.
    pub not_typed: bool,
}

impl Options<'_> {
    /// An address the user picked rather than typed (a bookmark, a group chip, "Open all", a
    /// closed tab offered again), opened in a new tab when `new_tab`.
    pub fn picked(new_tab: bool) -> Self {
        Self { new_tab, not_typed: true, ..Self::default() }
    }

    /// The text to remember as typed for a submit of `input`: none when it was not typed.
    pub fn typed_text<'t>(&self, input: &'t str) -> Option<&'t str> {
        (!self.not_typed).then_some(input)
    }
}

/// What to open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Destination {
    /// An address, with the plain-http address it was upgraded from (to fall back to when the secure
    /// page cannot load).
    Navigate { url: Url, upgraded_from: Option<Url> },
    /// A search with `engine` (its id): GET address or POST body.
    Search { request: SearchRequest, query: String, engine: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Nothing to do (empty text).
    Empty,
    /// Open `destination`, in a new tab when `new_tab`.
    Open { destination: Destination, new_tab: bool },
    /// A scoped nickname: the caller picks the first matching row of that scope.
    Scoped { scope: Scope, rest: String },
    /// Do nothing and tell the user why.
    Refused(String),
}

/// A single word with nothing in it that makes it an address already: `example`, not `a.b`, `a/b`,
/// `a b`, `a:80` or `https://a`.
pub fn is_expandable(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 63
        && !text.starts_with('-')
        && !text.ends_with('-')
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// `www.<text>.<suffix>` over HTTPS for Ctrl+Enter, or `None` when `text` is not expandable or the
/// suffix is empty (the shortcut is off).
pub fn expand_domain(text: &str, suffix: &str) -> Option<Url> {
    let text = text.trim();
    if suffix.is_empty() || !is_expandable(text) {
        return None;
    }
    Url::parse(&format!("https://www.{}.{}/", text.to_ascii_lowercase(), suffix)).ok()
}

/// Whether `url` is plain http to a host that may stay on it: a local name, a private or loopback
/// address, or a `.local` / `.internal` / `.test` / `.localhost` name that never has a certificate.
fn http_allowed(url: &Url) -> bool {
    let Some(host) = url.host_str() else { return true };
    let host = host.to_ascii_lowercase();
    if !host.contains('.') && !host.contains(':') {
        return true; // `localhost`, `intranet`
    }
    if matches!(host.rsplit('.').next(), Some("local" | "internal" | "test" | "localhost" | "lan" | "home" | "invalid")) {
        return true;
    }
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00,
        _ => false,
    }
}

/// `url` upgraded to https when "always use HTTPS" asks for it, and the http address it replaced.
pub fn upgrade(url: Url, always_https: bool) -> (Url, Option<Url>) {
    if !always_https || url.scheme() != "http" || http_allowed(&url) {
        return (url, None);
    }
    let mut secure = url.clone();
    if secure.set_scheme("https").is_err() {
        return (url, None);
    }
    // An explicit :80 would be wrong over https; the default port follows the scheme.
    if secure.port() == Some(80) {
        let _ = secure.set_port(None);
    }
    (secure, Some(url))
}

/// The provider that classifies typed text as an address or a search. The default engine's own GET
/// address is used when it has one; the engine list decides what is actually sent.
pub fn provider_for(engines: &EngineStore, private: bool) -> SearchProvider {
    let engine = engines.effective(private);
    let template = if engine.post.is_empty() { engine.url.replace("%s", "{query}") } else { String::new() };
    let image = navigation::SearchEngine::parse(&engine.id).map_or("https://www.google.com/search?tbm=isch&q={query}", |legacy| legacy.image_template());
    SearchProvider::from_templates(&template, image)
        .or_else(|| SearchProvider::from_templates("https://www.google.com/search?q={query}", image))
        .unwrap_or_else(|| SearchProvider::from_config(None, std::path::Path::new("missing-search-provider.txt")))
}

fn search_destination(engine: &Engine, query: &str) -> Result<Destination, String> {
    let request = engine.request(query).ok_or_else(|| format!("{} could not search for that text.", engine.name))?;
    Ok(Destination::Search { request, query: query.to_owned(), engine: engine.id.clone() })
}

/// Decides what submitting `input` does.
pub fn resolve(input: &str, settings: &AddressSettings, engines: &EngineStore, options: Options<'_>) -> Outcome {
    let text = input.trim();
    if text.is_empty() {
        return Outcome::Empty;
    }
    let new_tab = options.new_tab;
    let open = |destination: Destination, search: bool| Outcome::Open { destination, new_tab: new_tab || (search && settings.search_new_tab) };
    if !options.from_search_field {
        if let Some((scope, rest)) = settings.parse_scope(text) {
            return Outcome::Scoped { scope, rest: rest.to_owned() };
        }
    }
    if options.expand {
        if let Some(url) = expand_domain(text, &settings.expansion_suffix) {
            return open(Destination::Navigate { url, upgraded_from: None }, false);
        }
    }
    // `nickname query` always uses that engine, from either field.
    if let Some((engine, query)) = engines.split_nickname(text) {
        return match search_destination(engine, query) {
            Ok(destination) => open(destination, true),
            Err(message) => Outcome::Refused(message),
        };
    }
    // The search field searches for what is typed, address-shaped or not.
    if options.from_search_field {
        let engine = options.engine.and_then(|id| engines.find(id)).unwrap_or_else(|| engines.effective(options.private));
        return match search_destination(engine, text) {
            Ok(destination) => open(destination, true),
            Err(message) => Outcome::Refused(message),
        };
    }
    let provider = provider_for(engines, options.private);
    match navigation::resolve_input(text, &provider) {
        Ok(NavigationTarget::Empty) => Outcome::Empty,
        Ok(NavigationTarget::Web(url)) => {
            let (url, upgraded_from) = upgrade(url, settings.always_https);
            open(Destination::Navigate { url, upgraded_from }, false)
        }
        Ok(NavigationTarget::Search { query, url }) => {
            if !settings.search_in_address {
                return Outcome::Refused("Searching from the address field is off. Type a web address, or search with the search field.".to_owned());
            }
            let (_, image) = navigation::parse_image_prefix(text);
            if image {
                // Text image search is always the engine-independent image address from the provider.
                return open(Destination::Search { request: SearchRequest { url, post: None }, query, engine: engines.effective(options.private).id.clone() }, true);
            }
            match search_destination(engines.effective(options.private), &query) {
                Ok(destination) => open(destination, true),
                Err(message) => Outcome::Refused(message),
            }
        }
        Err(NavigationError::InvalidWebUrl) => Outcome::Refused("That address is invalid or unsafe.".to_owned()),
        Err(NavigationError::UnsupportedScheme(scheme)) => Outcome::Refused(format!("The address scheme '{scheme}' is not supported.")),
        Err(NavigationError::InvalidSearchProvider) => Outcome::Refused("The search engine is not set up correctly.".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str, settings: &AddressSettings, options: Options<'_>) -> Outcome {
        resolve(input, settings, &EngineStore::default(), options)
    }

    fn navigate_url(outcome: &Outcome) -> (&str, bool) {
        match outcome {
            Outcome::Open { destination: Destination::Navigate { url, .. }, new_tab } => (url.as_str(), *new_tab),
            other => panic!("not a navigation: {other:?}"),
        }
    }

    fn searched(outcome: &Outcome) -> (&str, &str, bool) {
        match outcome {
            Outcome::Open { destination: Destination::Search { request, engine, .. }, new_tab } => (request.url.as_str(), engine.as_str(), *new_tab),
            other => panic!("not a search: {other:?}"),
        }
    }

    #[test]
    fn plain_input_is_an_address_or_a_default_engine_search() {
        let settings = AddressSettings::default();
        assert_eq!(run("   ", &settings, Options::default()), Outcome::Empty);
        assert_eq!(navigate_url(&run("example.com/a", &settings, Options::default())), ("https://example.com/a", false));
        let (url, engine, new_tab) = match run("rust lang", &settings, Options::default()) {
            Outcome::Open { destination: Destination::Search { request, engine, .. }, new_tab } => (request.url.to_string(), engine, new_tab),
            other => panic!("{other:?}"),
        };
        assert_eq!(url, "https://html.duckduckgo.com/html/?q=rust%20lang&k1=-1");
        assert_eq!((engine.as_str(), new_tab), ("optimised", false));
        assert!(matches!(run("javascript:alert(1)", &settings, Options::default()), Outcome::Refused(_)));
        assert!(matches!(run("ftp://x.example", &settings, Options::default()), Outcome::Refused(_)));
    }

    /// OB-8: a bookmark, a group chip, "Open all", a reopened tab or page text opens as before, but
    /// only what the user typed is handed on to be remembered as typed.
    #[test]
    fn only_typed_text_is_remembered_as_typed() {
        let settings = AddressSettings::default();
        assert_eq!(Options::default().typed_text("rust"), Some("rust"), "the address field");
        for new_tab in [false, true] {
            let picked = Options::picked(new_tab);
            assert_eq!(picked.typed_text("https://docs.example/"), None);
            assert_eq!(navigate_url(&run("https://docs.example/", &settings, picked)), ("https://docs.example/", new_tab), "it still opens");
        }
        let selection = Options { new_tab: true, from_search_field: true, not_typed: true, ..Options::default() };
        assert_eq!(selection.typed_text("text on a page"), None);
        assert!(searched(&run("text on a page", &settings, selection)).2, "still searched, in a new tab");
    }

    #[test]
    fn alt_or_shift_enter_opens_in_a_new_tab() {
        let settings = AddressSettings::default();
        let options = Options { new_tab: true, ..Options::default() };
        assert_eq!(navigate_url(&run("example.com", &settings, options)), ("https://example.com/", true));
        assert!(searched(&run("rust", &settings, options)).2);
    }

    #[test]
    fn ctrl_enter_expands_a_single_word_and_the_suffix_is_configurable() {
        let mut settings = AddressSettings::default();
        let expand = Options { expand: true, ..Options::default() };
        assert_eq!(navigate_url(&run("example", &settings, expand)), ("https://www.example.com/", false));
        settings.set("domain_expansion_suffix", "co.uk");
        assert_eq!(navigate_url(&run("Example", &settings, expand)), ("https://www.example.co.uk/", false));
        // Anything that is already more than a word is left to the normal rules.
        assert_eq!(navigate_url(&run("example.org", &settings, expand)), ("https://example.org/", false));
        assert!(searched(&run("two words", &settings, expand)).0.contains("two%20words"));
        // Without the key the same text is a search, and an empty suffix turns the shortcut off.
        assert!(searched(&run("example", &settings, Options::default())).0.contains("q=example"));
        settings.set("domain_expansion_suffix", "");
        assert!(searched(&run("example", &settings, expand)).0.contains("q=example"));
        assert!(is_expandable("my-site") && !is_expandable("-x") && !is_expandable("a.b") && !is_expandable("a b") && !is_expandable(""));
        assert_eq!(expand_domain("x_y", "com"), None);
    }

    #[test]
    fn always_https_upgrades_public_http_and_leaves_local_hosts_alone() {
        let mut settings = AddressSettings::default();
        let off = run("http://example.com/a?b=1", &settings, Options::default());
        assert_eq!(navigate_url(&off).0, "http://example.com/a?b=1");
        settings.set("sec_always_https", "true");
        match run("http://example.com:80/a?b=1", &settings, Options::default()) {
            Outcome::Open { destination: Destination::Navigate { url, upgraded_from }, .. } => {
                assert_eq!(url.as_str(), "https://example.com/a?b=1");
                assert_eq!(upgraded_from.unwrap().as_str(), "http://example.com/a?b=1");
            }
            other => panic!("{other:?}"),
        }
        for local in ["http://localhost:3000/", "http://192.168.1.5/", "http://127.0.0.1/", "http://intranet/", "http://printer.local/", "http://[::1]/", "http://10.0.0.2:8080/x"] {
            assert_eq!(navigate_url(&run(local, &settings, Options::default())).0, local, "{local}");
        }
        assert_eq!(navigate_url(&run("https://example.com", &settings, Options::default())).0, "https://example.com/");
        assert_eq!(upgrade(Url::parse("http://8.8.8.8/").unwrap(), true).0.as_str(), "https://8.8.8.8/");
        assert_eq!(upgrade(Url::parse("http://example.com/").unwrap(), false).1, None);
    }

    #[test]
    fn engine_nicknames_search_with_that_engine_from_both_fields() {
        let settings = AddressSettings::default();
        let first = run("g rust lang", &settings, Options::default());
        let (url, engine, _) = searched(&first);
        assert_eq!((url, engine), ("https://www.google.com/search?q=rust%20lang", "google"));
        let second = run("w rust", &settings, Options { from_search_field: true, ..Options::default() });
        let (url, engine, _) = searched(&second);
        assert_eq!((url, engine), ("https://en.wikipedia.org/w/index.php?search=rust", "wikipedia"));
        // A nickname with nothing after it is just text.
        assert_eq!(searched(&run("g", &settings, Options::default())).1, "optimised");
    }

    #[test]
    fn the_search_field_uses_the_chosen_engine_and_searches_address_shaped_text() {
        let settings = AddressSettings::default();
        let field = Options { from_search_field: true, engine: Some("bing"), ..Options::default() };
        assert_eq!(searched(&run("example.com", &settings, field)), ("https://www.bing.com/search?q=example.com", "bing", false));
        let stale = Options { engine: Some("gone"), ..field };
        assert_eq!(searched(&run("x", &settings, stale)).1, "optimised", "an unknown engine falls back to the default");
    }

    #[test]
    fn search_in_the_address_field_can_be_turned_off_but_nicknames_and_addresses_still_work() {
        let mut settings = AddressSettings::default();
        settings.set("search_in_address", "false");
        assert!(matches!(run("rust lang", &settings, Options::default()), Outcome::Refused(_)));
        assert_eq!(navigate_url(&run("example.com", &settings, Options::default())).0, "https://example.com/");
        assert_eq!(searched(&run("g rust", &settings, Options::default())).1, "google");
        assert_eq!(searched(&run("rust", &settings, Options { from_search_field: true, ..Options::default() })).1, "optimised");
    }

    #[test]
    fn always_search_in_a_new_tab_applies_to_searches_only() {
        let mut settings = AddressSettings::default();
        settings.set("search_new_tab", "true");
        assert!(searched(&run("rust", &settings, Options::default())).2);
        assert!(searched(&run("g rust", &settings, Options::default())).2);
        assert!(!navigate_url(&run("example.com", &settings, Options::default())).1);
    }

    #[test]
    fn private_windows_search_with_their_own_engine() {
        let settings = AddressSettings::default();
        let mut engines = EngineStore::default();
        engines.set_default("google").unwrap();
        engines.set_private("duckduckgo").unwrap();
        let normal = resolve("rust", &settings, &engines, Options::default());
        let private = resolve("rust", &settings, &engines, Options { private: true, ..Options::default() });
        assert_eq!(searched(&normal).1, "google");
        assert_eq!(searched(&private).1, "duckduckgo");
        assert_eq!(searched(&private).0, "https://duckduckgo.com/?q=rust");
    }

    #[test]
    fn a_post_default_engine_posts_its_fields() {
        let settings = AddressSettings::default();
        let mut engines = EngineStore::default();
        let id = engines
            .add(Engine { name: "Posty".into(), nickname: "po".into(), url: "https://posty.example/find".into(), post: "q=%s".into(), ..Engine::default() })
            .unwrap();
        engines.set_default(&id).unwrap();
        match resolve("a b", &settings, &engines, Options::default()) {
            Outcome::Open { destination: Destination::Search { request, .. }, .. } => {
                assert_eq!(request.url.as_str(), "https://posty.example/find");
                assert_eq!(request.post.as_deref(), Some("q=a%20b"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn image_prefix_still_searches_images_and_scopes_are_handed_back() {
        let settings = AddressSettings::default();
        let images = run("img cats", &settings, Options::default());
        let (url, _, _) = searched(&images);
        assert!(url.contains("tbm=isch"), "{url}");
        assert_eq!(run("@history rust", &settings, Options::default()), Outcome::Scoped { scope: Scope::History, rest: "rust".into() });
        assert!(matches!(
            run("@history rust", &settings, Options { from_search_field: true, ..Options::default() }),
            Outcome::Open { .. }
        ), "the search field never reads scopes");
    }
}
