use std::fmt;
use std::path::Path;
use std::{fs, net::IpAddr};

use url::{Host, Url};

const DEFAULT_SEARCH_TEMPLATE: &str = "https://html.duckduckgo.com/html/?q={query}&k1=-1";

/// The engines Settings can choose between. Each is a fixed HTTPS template, so a saved choice can
/// never send searches anywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchEngine {
    Optimised,
    Google,
    DuckDuckGo,
}

impl SearchEngine {
    /// Settings window order.
    pub const ALL: [SearchEngine; 3] = [
        SearchEngine::Optimised,
        SearchEngine::Google,
        SearchEngine::DuckDuckGo,
    ];

    /// The `search_engine` value stored in settings.txt.
    pub fn key(self) -> &'static str {
        match self {
            Self::Optimised => "optimised",
            Self::Google => "google",
            Self::DuckDuckGo => "duckduckgo",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Optimised => "Optimised",
            Self::Google => "Google",
            Self::DuckDuckGo => "DuckDuckGo",
        }
    }

    pub fn template(self) -> &'static str {
        match self {
            Self::Optimised => DEFAULT_SEARCH_TEMPLATE,
            Self::Google => "https://www.google.com/search?q={query}",
            Self::DuckDuckGo => "https://duckduckgo.com/?q={query}",
        }
    }

    pub fn image_template(self) -> &'static str {
        match self {
            Self::Optimised | Self::Google => "https://www.google.com/search?tbm=isch&q={query}",
            Self::DuckDuckGo => "https://duckduckgo.com/?q={query}&iax=images&ia=images",
        }
    }

    /// Matches the settings.txt value, ignoring case and surrounding spaces. Anything else is `None`.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        Self::ALL
            .into_iter()
            .find(|engine| engine.key().eq_ignore_ascii_case(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationTarget {
    Empty,
    Web(Url),
    Search { query: String, url: Url },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationError {
    InvalidWebUrl,
    UnsupportedScheme(String),
    InvalidSearchProvider,
}

impl fmt::Display for NavigationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWebUrl => write!(f, "The web address is invalid or unsafe."),
            Self::UnsupportedScheme(scheme) => {
                write!(f, "The URL scheme '{scheme}' is not supported.")
            }
            Self::InvalidSearchProvider => {
                write!(f, "The search provider configuration is invalid.")
            }
        }
    }
}

impl std::error::Error for NavigationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchProvider {
    template: String,
    image_template: String,
}

impl SearchProvider {
    /// A saved engine choice wins. With no choice, a valid `search-provider.txt` template is used,
    /// and otherwise the Optimised default.
    pub fn from_config(engine: Option<SearchEngine>, path: &Path) -> Self {
        let template = match engine {
            Some(engine) => engine.template().to_owned(),
            None => fs::read_to_string(path)
                .ok()
                .map(|contents| contents.trim().to_owned())
                .filter(|template| valid_template(template))
                .unwrap_or_else(|| DEFAULT_SEARCH_TEMPLATE.to_owned()),
        };
        let image_template = engine
            .unwrap_or(SearchEngine::Optimised)
            .image_template()
            .to_owned();
        Self {
            template,
            image_template,
        }
    }

    /// A provider with these templates (each `{query}` once, https only), or `None` when either is
    /// not valid. Used by the address bar, whose engine list holds `%s` templates.
    pub fn from_templates(template: &str, image_template: &str) -> Option<Self> {
        (valid_template(template) && valid_template(image_template)).then(|| Self {
            template: template.to_owned(),
            image_template: image_template.to_owned(),
        })
    }

    /// True when searches go to the built-in Optimised template.
    pub fn is_default(&self) -> bool {
        self.template == DEFAULT_SEARCH_TEMPLATE
    }

    pub fn search_url(&self, query: &str) -> Result<Url, NavigationError> {
        self.url_from_template(&self.template, query)
    }

    pub fn image_search_url(&self, query: &str) -> Result<Url, NavigationError> {
        self.url_from_template(&self.image_template, query)
    }

    fn url_from_template(&self, template: &str, query: &str) -> Result<Url, NavigationError> {
        if query.chars().any(char::is_control) || !valid_template(template) {
            return Err(NavigationError::InvalidSearchProvider);
        }
        let encoded = percent_encode_query(query);
        Url::parse(&template.replacen("{query}", &encoded, 1))
            .map_err(|_| NavigationError::InvalidSearchProvider)
    }
}

pub fn resolve_input(
    input: &str,
    provider: &SearchProvider,
) -> Result<NavigationTarget, NavigationError> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(NavigationTarget::Empty);
    }
    if input.chars().any(char::is_control) {
        return Err(NavigationError::InvalidWebUrl);
    }

    // A colon after a hostname can denote a port. Recognize only authorities
    // whose host is independently a sensible bare host; unsupported schemes
    // such as javascript: and file: remain rejected below.
    if looks_like_bare_host_port(input) {
        return parse_bare_host(input)
            .map(NavigationTarget::Web)
            .ok_or(NavigationError::InvalidWebUrl);
    }

    if let Some(scheme) = explicit_scheme(input) {
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return Err(NavigationError::UnsupportedScheme(
                scheme.to_ascii_lowercase(),
            ));
        }
        let remainder = input
            .split_once(':')
            .map(|(_, rest)| rest)
            .unwrap_or_default();
        if !remainder.starts_with("//") {
            return Err(NavigationError::InvalidWebUrl);
        }
        return parse_web_url(input, false).map(NavigationTarget::Web);
    }

    if let Some(url) = parse_bare_host(input) {
        return Ok(NavigationTarget::Web(url));
    }

    let (query, images) = parse_image_prefix(input);
    let url = if images {
        provider.image_search_url(&query)?
    } else {
        provider.search_url(&query)?
    };
    Ok(NavigationTarget::Search { query, url })
}

/// Image search is explicit: one recognized token must be followed by a space and non-empty text.
pub fn parse_image_prefix(input: &str) -> (String, bool) {
    for prefix in ["!img ", "images ", "!i ", "img "] {
        if input
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        {
            let query = input[prefix.len()..].trim();
            if !query.is_empty() {
                return (query.to_owned(), true);
            }
        }
    }
    (input.to_owned(), false)
}

fn explicit_scheme(input: &str) -> Option<&str> {
    let colon = input.find(':')?;
    let scheme = &input[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic()
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '.' | '-'))
    {
        return None;
    }
    Some(scheme)
}

fn looks_like_bare_host_port(input: &str) -> bool {
    let authority = input.split(['/', '?', '#']).next().unwrap_or_default();
    let Some((host, _port)) = authority.split_once(':') else {
        return false;
    };
    // A bracketed IPv6 host contains colons of its own and is handled by the
    // ordinary bare-URL parser instead.
    if host.is_empty() || host.contains(']') {
        return false;
    }
    let host_url = Url::parse(&format!("https://{host}/"));
    host_url.is_ok_and(|url| {
        url.username().is_empty() && url.password().is_none() && sensible_bare_host(&url)
    })
}

fn parse_web_url(input: &str, bare: bool) -> Result<Url, NavigationError> {
    let url = Url::parse(input).map_err(|_| NavigationError::InvalidWebUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(NavigationError::InvalidWebUrl);
    }
    if bare && !sensible_bare_host(&url) {
        return Err(NavigationError::InvalidWebUrl);
    }
    Ok(url)
}

fn parse_bare_host(input: &str) -> Option<Url> {
    // Whitespace is query text, not part of a guessed URL. Explicit HTTP(S) inputs
    // have already been parsed strictly and therefore cannot fall through here.
    if input.chars().any(char::is_whitespace) {
        return None;
    }
    let candidate = format!("https://{input}");
    parse_web_url(&candidate, true).ok()
}

fn sensible_bare_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(_)) | Some(Host::Ipv6(_)) => true,
        Some(Host::Domain(host)) => {
            host.eq_ignore_ascii_case("localhost")
                || (host.contains('.') && valid_domain_labels(host))
                || host.parse::<IpAddr>().is_ok()
        }
        None => false,
    }
}

fn valid_domain_labels(host: &str) -> bool {
    host.split('.').all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

fn valid_template(template: &str) -> bool {
    if template.matches("{query}").count() != 1 || template.chars().any(char::is_control) {
        return false;
    }
    let marker = "BOWSERQUERYMARKER";
    let candidate = template.replacen("{query}", marker, 1);
    let Ok(url) = Url::parse(&candidate) else {
        return false;
    };
    url.scheme() == "https"
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && (url.path().contains(marker) || url.query().is_some_and(|query| query.contains(marker)))
}

fn percent_encode_query(query: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(query.len());
    for byte in query.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    fn temp_config(contents: Option<&str>) -> PathBuf {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("bowser-navigation-{}-{id}.txt", std::process::id()));
        if let Some(contents) = contents {
            fs::write(&path, contents).unwrap();
        } else {
            let _ = fs::remove_file(&path);
        }
        path
    }

    fn default_provider() -> SearchProvider {
        let path = temp_config(None);
        let provider = SearchProvider::from_config(None, &path);
        let _ = fs::remove_file(path);
        provider
    }

    fn engine_provider(engine: SearchEngine) -> SearchProvider {
        SearchProvider::from_config(Some(engine), &temp_config(None))
    }

    #[test]
    fn blank_and_whitespace_input_is_empty() {
        assert_eq!(
            resolve_input("  \t\n", &default_provider()).unwrap(),
            NavigationTarget::Empty
        );
    }

    #[test]
    fn explicit_http_and_https_addresses_are_web_targets() {
        let provider = default_provider();
        let http = resolve_input("http://example.com/a?b=c", &provider).unwrap();
        let https = resolve_input("https://example.com", &provider).unwrap();
        assert_eq!(
            http,
            NavigationTarget::Web(Url::parse("http://example.com/a?b=c").unwrap())
        );
        assert_eq!(
            https,
            NavigationTarget::Web(Url::parse("https://example.com/").unwrap())
        );
    }

    #[test]
    fn sensible_bare_hosts_are_normalized_to_https() {
        let provider = default_provider();
        for (input, expected) in [
            ("example.com", "https://example.com/"),
            ("example.com:8080/path", "https://example.com:8080/path"),
            ("münich.example", "https://xn--mnich-kva.example/"),
            ("localhost:8080/path", "https://localhost:8080/path"),
            ("127.0.0.1:8080", "https://127.0.0.1:8080/"),
            ("[::1]:8080", "https://[::1]:8080/"),
        ] {
            assert_eq!(
                resolve_input(input, &provider).unwrap(),
                NavigationTarget::Web(Url::parse(expected).unwrap()),
                "input: {input}"
            );
        }
    }

    #[test]
    fn ordinary_single_words_are_searches() {
        let provider = default_provider();
        assert!(
            matches!(resolve_input("intranet", &provider).unwrap(), NavigationTarget::Search { query, .. } if query == "intranet")
        );
    }

    #[test]
    fn unicode_search_query_is_utf8_encoded_without_query_injection() {
        let path = temp_config(Some("https://search.example/search?q={query}"));
        let provider = SearchProvider::from_config(None, &path);
        let target = resolve_input("café & tea #morning", &provider).unwrap();
        let NavigationTarget::Search { query, url } = target else {
            panic!("expected search")
        };
        assert_eq!(query, "café & tea #morning");
        assert!(url.as_str().contains("%C3%A9"), "{url}");
        assert!(url.as_str().contains("%26"), "{url}");
        assert!(url.as_str().contains("%23"), "{url}");
        assert!(url.fragment().is_none(), "{url}");
        assert_eq!(url.query_pairs().count(), 1, "{url}");
        assert_eq!(
            url.query_pairs().next().unwrap(),
            ("q".into(), query.into())
        );
    }

    #[test]
    fn unsafe_or_external_schemes_are_rejected() {
        let provider = default_provider();
        for input in [
            "javascript:alert(1)",
            "file:///C:/secret.txt",
            "data:text/html,hi",
            "mailto:test@example.com",
            "tel:+441234567890",
            "custom:payload",
        ] {
            assert!(
                matches!(
                    resolve_input(input, &provider),
                    Err(NavigationError::UnsupportedScheme(_))
                ),
                "input: {input}"
            );
        }
    }

    #[test]
    fn malformed_http_addresses_and_credentials_are_rejected_not_searched() {
        let provider = default_provider();
        for input in [
            "http://",
            "https://",
            "http:example.com",
            "https://exa mple.com",
            "https://user:pass@example.com/",
            "http://example.com:99999/",
        ] {
            assert_eq!(
                resolve_input(input, &provider),
                Err(NavigationError::InvalidWebUrl),
                "input: {input}"
            );
        }
    }

    #[test]
    fn malformed_bare_domain_ports_are_invalid_web_urls() {
        let provider = default_provider();
        for input in ["example.com:99999", "example.com:not-a-port/path"] {
            assert_eq!(
                resolve_input(input, &provider),
                Err(NavigationError::InvalidWebUrl),
                "input: {input}"
            );
        }
    }

    #[test]
    fn controls_are_rejected_but_internal_query_whitespace_is_preserved() {
        let provider = default_provider();
        assert!(matches!(
            resolve_input("find\u{7}me", &provider),
            Err(NavigationError::InvalidWebUrl)
        ));
        let NavigationTarget::Search { query, .. } = resolve_input("two words", &provider).unwrap()
        else {
            panic!("expected search")
        };
        assert_eq!(query, "two words");
    }

    #[test]
    fn invalid_and_missing_configuration_fall_back_to_optimised() {
        let missing = SearchProvider::from_config(None, &temp_config(None));
        let invalid = SearchProvider::from_config(
            None,
            &temp_config(Some("http://search.example/?q={query}")),
        );
        let malformed = SearchProvider::from_config(
            None,
            &temp_config(Some("https://search.example/?q={query}&again={query}")),
        );
        for provider in [missing, invalid, malformed] {
            assert_eq!(
                provider.search_url("hello world").unwrap().as_str(),
                "https://html.duckduckgo.com/html/?q=hello%20world&k1=-1"
            );
        }
    }

    #[test]
    fn a_valid_configured_search_template_is_used() {
        let path = temp_config(Some("https://search.example/?query={query}"));
        let provider = SearchProvider::from_config(None, &path);
        assert_eq!(
            provider.search_url("rust").unwrap().as_str(),
            "https://search.example/?query=rust"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn each_engine_routes_searches_to_its_own_site() {
        assert_eq!(
            engine_provider(SearchEngine::Google)
                .search_url("rust lang")
                .unwrap()
                .as_str(),
            "https://www.google.com/search?q=rust%20lang"
        );
        assert_eq!(
            engine_provider(SearchEngine::DuckDuckGo)
                .search_url("rust lang")
                .unwrap()
                .as_str(),
            "https://duckduckgo.com/?q=rust%20lang"
        );
    }

    #[test]
    fn optimised_search_uses_ad_free_html_and_google_images() {
        let provider = engine_provider(SearchEngine::Optimised);
        assert_eq!(
            provider.search_url("cats").unwrap().as_str(),
            "https://html.duckduckgo.com/html/?q=cats&k1=-1"
        );
        assert_eq!(
            provider.image_search_url("cats").unwrap().as_str(),
            "https://www.google.com/search?tbm=isch&q=cats"
        );
    }

    #[test]
    fn image_prefix_requires_a_token_and_following_space() {
        let provider = engine_provider(SearchEngine::Optimised);
        for (input, expected_query, expected_host) in [
            ("img", "img", "html.duckduckgo.com"),
            ("image of a cat", "image of a cat", "html.duckduckgo.com"),
            ("img cats", "cats", "www.google.com"),
            ("images cats", "cats", "www.google.com"),
            ("!i cats", "cats", "www.google.com"),
            ("!img cats", "cats", "www.google.com"),
        ] {
            let NavigationTarget::Search { query, url } = resolve_input(input, &provider).unwrap()
            else {
                panic!("expected search for {input:?}")
            };
            assert_eq!(query, expected_query, "input: {input}");
            assert_eq!(url.host_str(), Some(expected_host), "input: {input}");
        }
    }

    #[test]
    fn every_engine_has_an_image_search_target_and_encodes_the_query() {
        let query = "a+b & c # café";
        let expected = [
            (SearchEngine::Optimised, "www.google.com", "tbm=isch"),
            (SearchEngine::Google, "www.google.com", "tbm=isch"),
            (SearchEngine::DuckDuckGo, "duckduckgo.com", "iax=images"),
        ];
        for (engine, host, mode) in expected {
            let url = engine_provider(engine).image_search_url(query).unwrap();
            assert_eq!(url.host_str(), Some(host));
            assert!(url.as_str().contains(mode), "{url}");
            assert_eq!(url.query_pairs().find(|(key, _)| key == "q").unwrap().1, query);
            assert!(url.fragment().is_none(), "{url}");
        }
    }

    #[test]
    fn search_text_stays_one_query_value_for_every_engine() {
        let query = "a+b & c # d%20e? é/ü=1 \"q\" 日本";
        for engine in SearchEngine::ALL {
            let url = engine_provider(engine).search_url(query).unwrap();
            let text = url.as_str();
            assert!(!text.contains(' '), "{text}");
            for escaped in [
                "%2B",
                "%26",
                "%23",
                "%25",
                "%3F",
                "%3D",
                "%C3%A9",
                "%E6%97%A5",
            ] {
                assert!(text.contains(escaped), "{escaped} missing from {text}");
            }
            assert!(url.fragment().is_none(), "{text}");
            // Parsing the query back gives the typed text exactly, so nothing splits or injects.
            let pairs: Vec<(String, String)> = url
                .query_pairs()
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
            assert_eq!(
                pairs.iter().filter(|(key, _)| key == "q").count(),
                1,
                "{text}"
            );
            assert_eq!(
                pairs.iter().find(|(key, _)| key == "q").unwrap().1,
                query,
                "{text}"
            );
            if engine == SearchEngine::Optimised {
                assert!(pairs.contains(&("k1".to_owned(), "-1".to_owned())), "{text}");
            }
        }
    }

    #[test]
    fn engine_choice_changes_only_search_targets_not_addresses() {
        let bing = default_provider();
        for engine in SearchEngine::ALL {
            let provider = engine_provider(engine);
            for input in [
                "example.com",
                "example.com:8080/path",
                "localhost:8080/path",
                "127.0.0.1:8080",
                "[::1]:8080",
                "http://example.com/a?b=c",
                "https://example.com/?q=a",
                "münich.example",
            ] {
                assert_eq!(
                    resolve_input(input, &provider).unwrap(),
                    resolve_input(input, &bing).unwrap(),
                    "{engine:?} input: {input}"
                );
            }
            let NavigationTarget::Search { query, url } =
                resolve_input("two words & more", &provider).unwrap()
            else {
                panic!("expected search")
            };
            assert_eq!(query, "two words & more");
            let host = match engine {
                SearchEngine::Optimised => "html.duckduckgo.com",
                SearchEngine::Google => "www.google.com",
                SearchEngine::DuckDuckGo => "duckduckgo.com",
            };
            assert_eq!(url.host_str(), Some(host));
        }
    }

    #[test]
    fn engine_keys_parse_case_insensitively_and_reject_anything_else() {
        assert_eq!(SearchEngine::parse("google"), Some(SearchEngine::Google));
        assert_eq!(
            SearchEngine::parse("  DuckDuckGo "),
            Some(SearchEngine::DuckDuckGo)
        );
        for value in [
            "",
            "bing",
            "ddg",
            "google.com",
            "https://www.google.com/search?q={query}",
        ] {
            assert_eq!(SearchEngine::parse(value), None, "value: {value}");
        }
    }

    #[test]
    fn a_saved_engine_takes_precedence_over_the_override_file() {
        let path = temp_config(Some("https://search.example/?q={query}"));
        let provider = SearchProvider::from_config(Some(SearchEngine::Google), &path);
        assert_eq!(
            provider.search_url("rust").unwrap().as_str(),
            "https://www.google.com/search?q=rust"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn without_a_saved_engine_the_override_file_and_then_optimised_are_used() {
        let path = temp_config(Some("https://search.example/?q={query}"));
        let overridden = SearchProvider::from_config(None, &path);
        assert!(!overridden.is_default());
        assert_eq!(
            overridden.search_url("rust").unwrap().as_str(),
            "https://search.example/?q=rust"
        );
        let _ = fs::remove_file(path);

        let optimised = SearchProvider::from_config(None, &temp_config(None));
        assert!(optimised.is_default());
        assert_eq!(
            optimised.search_url("rust").unwrap().as_str(),
            "https://html.duckduckgo.com/html/?q=rust&k1=-1"
        );
    }
}
