//! The profile's search engines: the list the address field, the search field and the context menu
//! search with, their nicknames, the default, the private-window default and the image-search engine.
//!
//! Stored in `search-engines.txt` beside `settings.txt`. A missing file means the built-in list, with
//! the default taken from the older `search_engine` setting (and `search-provider.txt`) so nothing
//! changes for a profile that never opens the editor. Every URL is HTTPS and every `%s` is replaced
//! with percent-encoded text, so an engine can never send a search anywhere but the host it names.

use std::{fs, io, path::Path};

use url::Url;

use crate::navigation::SearchEngine;

pub const FILE_NAME: &str = "search-engines.txt";
pub const MAX_ENGINES: usize = 40;
const MAX_FILE_BYTES: u64 = 64 * 1024;
pub const MAX_NAME_CHARS: usize = 40;
pub const MAX_NICKNAME_CHARS: usize = 16;
pub const MAX_URL_BYTES: usize = 1024;
const MAX_POST_BYTES: usize = 512;
/// The placeholder for the search text (or, in an image URL, the image address).
pub const PLACEHOLDER: &str = "%s";

/// One search engine.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Engine {
    pub id: String,
    pub name: String,
    /// The word typed before a query to use this engine, lower case; may be empty.
    pub nickname: String,
    /// The search address. With no `post`, it holds one `%s`.
    pub url: String,
    /// Form fields to POST to `url` (`q=%s&hl=en`); empty for a plain GET search.
    pub post: String,
    /// Address that answers suggestions for `%s`; empty when the engine has none.
    pub suggest: String,
    /// Address that searches for the image at `%s`; empty when the engine cannot.
    pub image: String,
    /// POST fields for the image search, when it is a POST.
    pub image_post: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineError {
    Name,
    Nickname,
    Taken(String),
    Url(&'static str),
    Missing,
    Limit,
    IsDefault,
    LastOne,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name => write!(f, "Give the engine a name of up to {MAX_NAME_CHARS} characters."),
            Self::Nickname => write!(f, "A nickname is up to {MAX_NICKNAME_CHARS} letters, digits, dots, dashes or underscores."),
            Self::Taken(other) => write!(f, "The nickname is already used by {other}."),
            Self::Url(why) => write!(f, "{why}"),
            Self::Missing => write!(f, "That engine no longer exists."),
            Self::Limit => write!(f, "The list holds at most {MAX_ENGINES} engines."),
            Self::IsDefault => write!(f, "Choose another default engine before removing this one."),
            Self::LastOne => write!(f, "At least one engine must stay."),
        }
    }
}

/// How a search is sent: a GET address, or a POST of `body` to the address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchRequest {
    pub url: Url,
    pub post: Option<String>,
}

fn builtin(id: &str, name: &str, nickname: &str, url: &str, suggest: &str, image: &str) -> Engine {
    Engine {
        id: id.to_owned(),
        name: name.to_owned(),
        nickname: nickname.to_owned(),
        url: url.to_owned(),
        post: String::new(),
        suggest: suggest.to_owned(),
        image: image.to_owned(),
        image_post: String::new(),
    }
}

/// The built-in engines, in list order. The first three keep the ids of the older `search_engine`
/// setting.
pub fn builtin_engines() -> Vec<Engine> {
    vec![
        builtin(
            "optimised",
            "Optimised",
            "opt",
            "https://html.duckduckgo.com/html/?q=%s&k1=-1",
            "https://duckduckgo.com/ac/?q=%s&type=list",
            "https://lens.google.com/uploadbyurl?url=%s",
        ),
        builtin(
            "google",
            "Google",
            "g",
            "https://www.google.com/search?q=%s",
            "https://suggestqueries.google.com/complete/search?client=firefox&q=%s",
            "https://lens.google.com/uploadbyurl?url=%s",
        ),
        builtin("duckduckgo", "DuckDuckGo", "ddg", "https://duckduckgo.com/?q=%s", "https://duckduckgo.com/ac/?q=%s&type=list", ""),
        builtin(
            "bing",
            "Bing",
            "b",
            "https://www.bing.com/search?q=%s",
            "https://api.bing.com/osjson.aspx?query=%s",
            "https://www.bing.com/images/search?view=detailv2&iss=sbi&q=imgurl:%s",
        ),
        builtin("brave", "Brave", "br", "https://search.brave.com/search?q=%s", "https://search.brave.com/api/suggest?q=%s", ""),
        builtin(
            "wikipedia",
            "Wikipedia",
            "w",
            "https://en.wikipedia.org/w/index.php?search=%s",
            "https://en.wikipedia.org/w/api.php?action=opensearch&search=%s",
            "",
        ),
        builtin(
            "youtube",
            "YouTube",
            "yt",
            "https://www.youtube.com/results?search_query=%s",
            "https://suggestqueries.google.com/complete/search?client=firefox&ds=yt&q=%s",
            "",
        ),
    ]
}

/// The id of the engine the older `search_engine` setting names.
pub fn id_of_legacy(engine: SearchEngine) -> &'static str {
    engine.key()
}

/// The older setting's engine for `id`, when `id` is one of the three it knows.
pub fn legacy_of_id(id: &str) -> Option<SearchEngine> {
    SearchEngine::parse(id)
}

fn percent_encode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

fn clean_text(text: &str) -> String {
    text.trim().to_owned()
}

fn has_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

fn placeholder_count(text: &str) -> usize {
    text.matches(PLACEHOLDER).count()
}

/// An https address (with at most one `%s` once the placeholder is swapped for a marker) that names a host.
fn https_template(text: &str) -> bool {
    if text.len() > MAX_URL_BYTES || has_control(text) || text.contains(char::is_whitespace) {
        return false;
    }
    Url::parse(&text.replace(PLACEHOLDER, "bowserquerymarker")).is_ok_and(|url| {
        url.scheme() == "https" && url.host().is_some() && url.username().is_empty() && url.password().is_none()
    })
}

/// A fully checked engine, or why it is not allowed. `id` is not checked here.
pub fn validate(engine: &Engine) -> Result<(), EngineError> {
    let name_chars = engine.name.chars().count();
    if name_chars == 0 || name_chars > MAX_NAME_CHARS || has_control(&engine.name) {
        return Err(EngineError::Name);
    }
    if engine.nickname.chars().count() > MAX_NICKNAME_CHARS
        || !engine.nickname.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_'))
    {
        return Err(EngineError::Nickname);
    }
    if !https_template(&engine.url) {
        return Err(EngineError::Url("The search address must be an https:// address."));
    }
    if engine.post.is_empty() {
        if placeholder_count(&engine.url) != 1 {
            return Err(EngineError::Url("The search address needs %s once, where the search text goes."));
        }
    } else if engine.post.len() > MAX_POST_BYTES || has_control(&engine.post) || placeholder_count(&engine.post) != 1 {
        return Err(EngineError::Url("POST parameters need %s once, where the search text goes."));
    }
    if !engine.suggest.is_empty() && (!https_template(&engine.suggest) || placeholder_count(&engine.suggest) != 1) {
        return Err(EngineError::Url("The suggestions address must be https:// and hold %s once."));
    }
    if !engine.image.is_empty() && !https_template(&engine.image) {
        return Err(EngineError::Url("The image search address must be an https:// address."));
    }
    if engine.image.is_empty() {
        if !engine.image_post.is_empty() {
            return Err(EngineError::Url("Image POST parameters need an image search address."));
        }
    } else if engine.image_post.is_empty() {
        if placeholder_count(&engine.image) != 1 {
            return Err(EngineError::Url("The image search address needs %s once, where the image address goes."));
        }
    } else if engine.image_post.len() > MAX_POST_BYTES || has_control(&engine.image_post) || placeholder_count(&engine.image_post) != 1 {
        return Err(EngineError::Url("Image POST parameters need %s once, where the image address goes."));
    }
    Ok(())
}

fn fill(template: &str, text: &str) -> String {
    template.replacen(PLACEHOLDER, &percent_encode(text), 1)
}

impl Engine {
    /// The request that searches for `query`. `None` when `query` has a control character or the
    /// engine's address does not parse.
    pub fn request(&self, query: &str) -> Option<SearchRequest> {
        if query.chars().any(char::is_control) {
            return None;
        }
        let url = Url::parse(&if self.post.is_empty() { fill(&self.url, query) } else { self.url.clone() }).ok()?;
        (url.scheme() == "https").then_some(())?;
        let post = (!self.post.is_empty()).then(|| fill(&self.post, query));
        Some(SearchRequest { url, post })
    }

    /// The request that searches for the image at `image_url`. `None` when the engine cannot.
    pub fn image_request(&self, image_url: &str) -> Option<SearchRequest> {
        if self.image.is_empty() || image_url.chars().any(char::is_control) {
            return None;
        }
        let url = Url::parse(&if self.image_post.is_empty() { fill(&self.image, image_url) } else { self.image.clone() }).ok()?;
        (url.scheme() == "https").then_some(())?;
        let post = (!self.image_post.is_empty()).then(|| fill(&self.image_post, image_url));
        Some(SearchRequest { url, post })
    }

    /// The address that answers suggestions for `query`.
    pub fn suggest_url(&self, query: &str) -> Option<Url> {
        if self.suggest.is_empty() || query.chars().any(char::is_control) {
            return None;
        }
        Url::parse(&fill(&self.suggest, query)).ok().filter(|url| url.scheme() == "https")
    }

    /// The host the engine searches on, for its icon.
    pub fn host(&self) -> Option<String> {
        Url::parse(&self.url.replace(PLACEHOLDER, "x")).ok()?.host_str().map(str::to_owned)
    }
}

/// The engine list and the three choices made from it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineStore {
    engines: Vec<Engine>,
    default: String,
    private: String,
    image: String,
}

impl Default for EngineStore {
    fn default() -> Self {
        Self { engines: builtin_engines(), default: "optimised".to_owned(), private: String::new(), image: "google".to_owned() }
    }
}

impl EngineStore {
    pub fn engines(&self) -> &[Engine] {
        &self.engines
    }
    pub fn find(&self, id: &str) -> Option<&Engine> {
        self.engines.iter().find(|engine| engine.id == id)
    }
    pub fn default_id(&self) -> &str {
        &self.default
    }
    /// The private-window engine's id; empty means the default engine.
    pub fn private_id(&self) -> &str {
        &self.private
    }
    pub fn image_id(&self) -> &str {
        &self.image
    }

    /// The engine searches use: the private-window one in a private window when one is chosen,
    /// else the default. Never fails: the first engine stands in when a choice is stale.
    pub fn effective(&self, private: bool) -> &Engine {
        let chosen = if private && !self.private.is_empty() { self.find(&self.private) } else { None };
        chosen.or_else(|| self.find(&self.default)).unwrap_or(&self.engines[0])
    }

    /// The engine that searches for images (the image-search engine, or the first that can).
    pub fn image_engine(&self) -> Option<&Engine> {
        self.find(&self.image).filter(|engine| !engine.image.is_empty()).or_else(|| self.engines.iter().find(|engine| !engine.image.is_empty()))
    }

    /// The engine whose nickname is `word` (ASCII case aside).
    pub fn by_nickname(&self, word: &str) -> Option<&Engine> {
        (!word.is_empty()).then(|| self.engines.iter().find(|engine| !engine.nickname.is_empty() && engine.nickname.eq_ignore_ascii_case(word))).flatten()
    }

    /// `nickname query` split into the engine and its query, when the first word is a nickname and a
    /// query follows.
    pub fn split_nickname<'a>(&self, input: &'a str) -> Option<(&Engine, &'a str)> {
        let text = input.trim_start();
        let (word, rest) = text.split_once(char::is_whitespace)?;
        let rest = rest.trim();
        if rest.is_empty() {
            return None;
        }
        Some((self.by_nickname(word)?, rest))
    }

    fn unique_id(&self, name: &str) -> String {
        let mut slug: String = name
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        if slug.is_empty() {
            slug = "engine".to_owned();
        }
        slug.truncate(24);
        let mut id = slug.clone();
        let mut counter = 2;
        while self.find(&id).is_some() {
            id = format!("{slug}-{counter}");
            counter += 1;
        }
        id
    }

    fn nickname_taken(&self, nickname: &str, except: &str) -> Option<&Engine> {
        (!nickname.is_empty()).then(|| self.engines.iter().find(|engine| engine.id != except && engine.nickname == nickname)).flatten()
    }

    fn normalised(mut engine: Engine) -> Engine {
        engine.name = clean_text(&engine.name);
        engine.nickname = engine.nickname.trim().to_ascii_lowercase();
        engine.url = clean_text(&engine.url);
        engine.post = clean_text(&engine.post);
        engine.suggest = clean_text(&engine.suggest);
        engine.image = clean_text(&engine.image);
        engine.image_post = clean_text(&engine.image_post);
        engine
    }

    /// Appends a new engine; its id is made from its name. Returns the id.
    pub fn add(&mut self, engine: Engine) -> Result<String, EngineError> {
        if self.engines.len() >= MAX_ENGINES {
            return Err(EngineError::Limit);
        }
        let mut engine = Self::normalised(engine);
        validate(&engine)?;
        if let Some(other) = self.nickname_taken(&engine.nickname, "") {
            return Err(EngineError::Taken(other.name.clone()));
        }
        engine.id = self.unique_id(&engine.name);
        let id = engine.id.clone();
        self.engines.push(engine);
        Ok(id)
    }

    /// Replaces the fields of engine `id` (its id and place stay).
    pub fn update(&mut self, id: &str, engine: Engine) -> Result<(), EngineError> {
        let mut engine = Self::normalised(engine);
        validate(&engine)?;
        if let Some(other) = self.nickname_taken(&engine.nickname, id) {
            return Err(EngineError::Taken(other.name.clone()));
        }
        let slot = self.engines.iter_mut().find(|slot| slot.id == id).ok_or(EngineError::Missing)?;
        engine.id = slot.id.clone();
        *slot = engine;
        Ok(())
    }

    /// Removes engine `id`. The default engine cannot go; a private or image choice that named it
    /// falls back to the default.
    pub fn remove(&mut self, id: &str) -> Result<(), EngineError> {
        if self.find(id).is_none() {
            return Err(EngineError::Missing);
        }
        if self.default == id {
            return Err(EngineError::IsDefault);
        }
        if self.engines.len() <= 1 {
            return Err(EngineError::LastOne);
        }
        self.engines.retain(|engine| engine.id != id);
        if self.private == id {
            self.private.clear();
        }
        if self.image == id {
            self.image = self.engines.iter().find(|engine| !engine.image.is_empty()).map(|engine| engine.id.clone()).unwrap_or_default();
        }
        Ok(())
    }

    /// Moves engine `id` to list position `index` (clamped).
    pub fn move_to(&mut self, id: &str, index: usize) -> Result<(), EngineError> {
        let from = self.engines.iter().position(|engine| engine.id == id).ok_or(EngineError::Missing)?;
        let engine = self.engines.remove(from);
        self.engines.insert(index.min(self.engines.len()), engine);
        Ok(())
    }

    pub fn set_default(&mut self, id: &str) -> Result<(), EngineError> {
        self.find(id).ok_or(EngineError::Missing)?;
        self.default = id.to_owned();
        Ok(())
    }

    /// `id` empty makes private windows use the default engine.
    pub fn set_private(&mut self, id: &str) -> Result<(), EngineError> {
        if !id.is_empty() {
            self.find(id).ok_or(EngineError::Missing)?;
        }
        self.private = id.to_owned();
        Ok(())
    }

    pub fn set_image(&mut self, id: &str) -> Result<(), EngineError> {
        let engine = self.find(id).ok_or(EngineError::Missing)?;
        if engine.image.is_empty() {
            return Err(EngineError::Url("That engine has no image search address."));
        }
        self.image = id.to_owned();
        Ok(())
    }

    /// Puts every built-in engine back as shipped (re-adding any that were removed, in their
    /// place); engines the user made stay, and so do the three choices when they still exist.
    pub fn restore_defaults(&mut self) {
        let shipped = builtin_engines();
        let custom: Vec<Engine> = self.engines.iter().filter(|engine| !shipped.iter().any(|b| b.id == engine.id)).cloned().collect();
        let mut engines = shipped;
        // A custom engine that took a shipped nickname gives it back.
        let custom: Vec<Engine> = custom
            .into_iter()
            .map(|mut engine| {
                if engines.iter().any(|b| !b.nickname.is_empty() && b.nickname == engine.nickname) {
                    engine.nickname.clear();
                }
                engine
            })
            .collect();
        engines.extend(custom);
        self.engines = engines;
        if self.find(&self.default).is_none() {
            self.default = "optimised".to_owned();
        }
        if !self.private.is_empty() && self.find(&self.private).is_none() {
            self.private.clear();
        }
        if self.find(&self.image).is_none_or(|engine| engine.image.is_empty()) {
            self.image = "google".to_owned();
        }
    }

    // ----- the file -----

    pub fn serialize(&self) -> String {
        let mut out = String::from("# Bowser search engines. Edit them in Settings.\n");
        out.push_str(&format!("default={}\nprivate={}\nimage={}\n", self.default, self.private, self.image));
        for engine in &self.engines {
            out.push_str(&format!(
                "engine={}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                engine.id, engine.name, engine.nickname, engine.url, engine.post, engine.suggest, engine.image, engine.image_post
            ));
        }
        out
    }

    /// Parses a file. Lines that do not parse or validate are dropped; an empty result is the
    /// built-in list.
    pub fn parse(text: &str) -> Self {
        let mut store = Self { engines: Vec::new(), default: String::new(), private: String::new(), image: String::new() };
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            match key.trim() {
                "default" => store.default = value.trim().to_owned(),
                "private" => store.private = value.trim().to_owned(),
                "image" => store.image = value.trim().to_owned(),
                "engine" => {
                    let fields: Vec<&str> = value.split('\t').collect();
                    if fields.len() != 8 || store.engines.len() >= MAX_ENGINES {
                        continue;
                    }
                    let id = fields[0].trim();
                    if id.is_empty()
                        || id.len() > 32
                        || !id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
                        || store.find(id).is_some()
                    {
                        continue;
                    }
                    let engine = Engine {
                        id: id.to_owned(),
                        name: fields[1].to_owned(),
                        nickname: fields[2].to_owned(),
                        url: fields[3].to_owned(),
                        post: fields[4].to_owned(),
                        suggest: fields[5].to_owned(),
                        image: fields[6].to_owned(),
                        image_post: fields[7].to_owned(),
                    };
                    if validate(&engine).is_ok() && store.nickname_taken(&engine.nickname, "").is_none() {
                        store.engines.push(engine);
                    }
                }
                _ => {}
            }
        }
        if store.engines.is_empty() {
            return Self::default();
        }
        if store.find(&store.default).is_none() {
            store.default = store.engines[0].id.clone();
        }
        if !store.private.is_empty() && store.find(&store.private).is_none() {
            store.private.clear();
        }
        if store.find(&store.image).is_none_or(|engine| engine.image.is_empty()) {
            store.image = store.engines.iter().find(|engine| !engine.image.is_empty()).map(|engine| engine.id.clone()).unwrap_or_default();
        }
        store
    }

    /// Reads the profile's list. With no file, the built-in list whose default is `legacy` (the older
    /// `search_engine` setting) or, when that is unset and `provider_template` (the older
    /// `search-provider.txt`, with `{query}`) is valid, a "Custom" engine made from it.
    pub fn load(root: &Path, legacy: Option<SearchEngine>, provider_template: Option<&str>) -> Self {
        match fs::read(root.join(FILE_NAME)) {
            Ok(bytes) if bytes.len() as u64 <= MAX_FILE_BYTES => {
                String::from_utf8(bytes).map(|text| Self::parse(&text)).unwrap_or_default()
            }
            Ok(_) => Self::default(),
            Err(_) => Self::from_legacy(legacy, provider_template),
        }
    }

    fn from_legacy(legacy: Option<SearchEngine>, provider_template: Option<&str>) -> Self {
        let mut store = Self::default();
        if let Some(engine) = legacy {
            store.default = id_of_legacy(engine).to_owned();
        } else if let Some(template) = provider_template {
            let custom = Engine {
                id: "custom".to_owned(),
                name: "Custom".to_owned(),
                url: template.replace("{query}", PLACEHOLDER),
                ..Engine::default()
            };
            if validate(&custom).is_ok() && store.add(custom).is_ok() {
                store.default = "custom".to_owned();
            }
        }
        store
    }

    /// Writes the list atomically.
    pub fn save(&self, root: &Path) -> io::Result<()> {
        let path = root.join(FILE_NAME);
        fs::create_dir_all(root)?;
        let temporary = path.with_extension(format!("txt.{}.tmp", std::process::id()));
        let written = fs::write(&temporary, self.serialize().as_bytes());
        if let Err(error) = written.and_then(|()| fs::rename(&temporary, &path)) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(())
    }
}

/// The percent-encoded form of `text` that `%s` is replaced with (exposed for the suggestion path
/// and tests).
pub fn encode_query(text: &str) -> String {
    percent_encode(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(name: &str, nickname: &str, url: &str) -> Engine {
        Engine { name: name.into(), nickname: nickname.into(), url: url.into(), ..Engine::default() }
    }

    #[test]
    fn every_builtin_engine_validates_and_the_first_three_keep_the_legacy_ids() {
        let engines = builtin_engines();
        for engine in &engines {
            assert_eq!(validate(engine), Ok(()), "{}", engine.id);
            assert!(engine.request("rust lang").is_some(), "{}", engine.id);
        }
        for (index, legacy) in SearchEngine::ALL.iter().enumerate() {
            assert_eq!(engines[index].id, id_of_legacy(*legacy));
            assert_eq!(legacy_of_id(&engines[index].id), Some(*legacy));
        }
        let mut nicknames: Vec<&str> = engines.iter().map(|e| e.nickname.as_str()).collect();
        nicknames.sort_unstable();
        nicknames.dedup();
        assert_eq!(nicknames.len(), engines.len(), "built-in nicknames are unique");
        assert_eq!(legacy_of_id("bing"), None);
    }

    #[test]
    fn requests_encode_the_query_and_only_ever_use_https() {
        let google = builtin_engines().remove(1);
        let request = google.request("café & tea #1").unwrap();
        assert_eq!(request.url.as_str(), "https://www.google.com/search?q=caf%C3%A9%20%26%20tea%20%231");
        assert_eq!(request.post, None);
        assert_eq!(google.request("a\u{7}b"), None, "control characters are refused");
        let hostile = Engine { url: "http://example.com/?q=%s".into(), ..custom("x", "", "") };
        assert_eq!(hostile.request("a"), None, "a plain http engine never searches");
        assert_eq!(
            google.suggest_url("a b").unwrap().as_str(),
            "https://suggestqueries.google.com/complete/search?client=firefox&q=a%20b"
        );
        assert_eq!(builtin_engines().remove(2).image_request("https://x.test/a.png"), None, "no image address");
        assert_eq!(
            google.image_request("https://x.test/a b.png").unwrap().url.as_str(),
            "https://lens.google.com/uploadbyurl?url=https%3A%2F%2Fx.test%2Fa%20b.png"
        );
    }

    #[test]
    fn post_engines_send_form_fields_and_leave_the_address_alone() {
        let engine = Engine {
            post: "q=%s&lang=en".into(),
            image: "https://img.example/search".into(),
            image_post: "u=%s".into(),
            ..custom("Posty", "po", "https://posty.example/find")
        };
        assert_eq!(validate(&engine), Ok(()));
        let request = engine.request("a b&c").unwrap();
        assert_eq!(request.url.as_str(), "https://posty.example/find");
        assert_eq!(request.post.as_deref(), Some("q=a%20b%26c&lang=en"));
        let image = engine.image_request("https://x.test/i.png").unwrap();
        assert_eq!(image.url.as_str(), "https://img.example/search");
        assert_eq!(image.post.as_deref(), Some("u=https%3A%2F%2Fx.test%2Fi.png"));
        // POST fields without a placeholder, or a stray placeholder in the address, are refused.
        assert!(validate(&Engine { post: "q=1".into(), ..engine.clone() }).is_err());
        assert!(validate(&Engine { image_post: "u=1".into(), ..engine.clone() }).is_err());
        assert!(validate(&Engine { image: String::new(), ..engine.clone() }).is_err(), "image POST needs an address");
    }

    #[test]
    fn validation_refuses_unsafe_or_malformed_engines() {
        let ok = custom("Ok", "ok", "https://ok.example/?q=%s");
        assert_eq!(validate(&ok), Ok(()));
        for (engine, why) in [
            (Engine { name: String::new(), ..ok.clone() }, "empty name"),
            (Engine { name: "n".repeat(41), ..ok.clone() }, "long name"),
            (Engine { name: "a\tb".into(), ..ok.clone() }, "tab in name"),
            (Engine { nickname: "UP".into(), ..ok.clone() }, "upper nickname"),
            (Engine { nickname: "a b".into(), ..ok.clone() }, "space in nickname"),
            (Engine { nickname: "n".repeat(17), ..ok.clone() }, "long nickname"),
            (Engine { url: "http://ok.example/?q=%s".into(), ..ok.clone() }, "http"),
            (Engine { url: "https://ok.example/".into(), ..ok.clone() }, "no placeholder"),
            (Engine { url: "https://ok.example/?q=%s&r=%s".into(), ..ok.clone() }, "two placeholders"),
            (Engine { url: "https://user:pw@ok.example/?q=%s".into(), ..ok.clone() }, "credentials"),
            (Engine { url: "https://ok.example/?q=%s x".into(), ..ok.clone() }, "space"),
            (Engine { url: "javascript:alert(%s)".into(), ..ok.clone() }, "script"),
            (Engine { suggest: "http://s.example/%s".into(), ..ok.clone() }, "http suggest"),
            (Engine { suggest: "https://s.example/".into(), ..ok.clone() }, "suggest without placeholder"),
            (Engine { image: "ftp://i.example/%s".into(), ..ok.clone() }, "ftp image"),
            (Engine { url: format!("https://ok.example/?q=%s&{}", "a".repeat(1100)), ..ok.clone() }, "long url"),
        ] {
            assert!(validate(&engine).is_err(), "{why}");
        }
    }

    #[test]
    fn nicknames_split_the_first_word_off() {
        let store = EngineStore::default();
        let (engine, query) = store.split_nickname("g  rust lang ").unwrap();
        assert_eq!((engine.id.as_str(), query), ("google", "rust lang"));
        assert_eq!(store.split_nickname("G rust").map(|(e, _)| e.id.as_str()), Some("google"));
        assert_eq!(store.split_nickname("g"), None, "a nickname alone is not a search");
        assert_eq!(store.split_nickname("g   "), None);
        assert_eq!(store.split_nickname("gx rust"), None);
        assert_eq!(store.split_nickname("rust g"), None);
        assert_eq!(store.by_nickname(""), None);
    }

    #[test]
    fn add_update_remove_move_and_the_three_choices() {
        let mut store = EngineStore::default();
        let id = store.add(custom("My Site!", "ms", "https://my.example/?q=%s")).unwrap();
        assert_eq!(id, "my-site");
        let again = store.add(custom("My Site!", "ms2", "https://my.example/?q=%s")).unwrap();
        assert_eq!(again, "my-site-2", "ids stay unique");
        assert_eq!(store.add(custom("Dup", "ms", "https://d.example/?q=%s")), Err(EngineError::Taken("My Site!".into())));
        assert_eq!(store.add(custom("Dup", "G", "https://d.example/?q=%s")), Err(EngineError::Taken("Google".into())), "nicknames fold case");
        store.update("my-site", custom("Mine", "ms", "https://mine.example/?q=%s")).unwrap();
        assert_eq!(store.find("my-site").unwrap().name, "Mine");
        assert_eq!(store.update("nope", custom("a", "", "https://a.example/?q=%s")), Err(EngineError::Missing));
        // A nickname may stay with its own engine on update.
        store.update("my-site", custom("Mine", "ms", "https://mine.example/?q=%s&x=1")).unwrap();

        store.move_to("my-site", 0).unwrap();
        assert_eq!(store.engines()[0].id, "my-site");
        store.move_to("my-site", 999).unwrap();
        assert_eq!(store.engines().last().unwrap().id, "my-site");

        store.set_default("my-site-2").unwrap();
        assert_eq!(store.remove("my-site-2"), Err(EngineError::IsDefault));
        store.set_private("bing").unwrap();
        assert_eq!(store.effective(true).id, "bing");
        assert_eq!(store.effective(false).id, "my-site-2");
        store.remove("bing").unwrap();
        assert_eq!(store.private_id(), "", "a removed private engine falls back to the default");
        assert_eq!(store.effective(true).id, "my-site-2");
        assert_eq!(store.set_image("my-site"), Err(EngineError::Url("That engine has no image search address.")));
        store.set_image("optimised").unwrap();
        assert_eq!(store.image_engine().unwrap().id, "optimised");
        store.remove("optimised").unwrap();
        assert_eq!(store.image_engine().unwrap().id, "google", "the image engine falls back to one that can");
        assert_eq!(store.set_default("zzz"), Err(EngineError::Missing));
    }

    #[test]
    fn the_list_has_a_limit_and_never_empties() {
        let mut store = EngineStore::default();
        for n in 0..MAX_ENGINES {
            let result = store.add(custom(&format!("E{n}"), "", &format!("https://e{n}.example/?q=%s")));
            if store.engines().len() == MAX_ENGINES {
                break;
            }
            assert!(result.is_ok());
        }
        assert_eq!(store.add(custom("One more", "", "https://more.example/?q=%s")), Err(EngineError::Limit));
        let mut single = EngineStore { engines: vec![builtin_engines().remove(0)], ..EngineStore::default() };
        single.default = "other".into();
        assert_eq!(single.remove("optimised"), Err(EngineError::LastOne));
    }

    #[test]
    fn restore_defaults_brings_back_built_ins_and_keeps_custom_engines() {
        let mut store = EngineStore::default();
        store.add(custom("Mine", "bing2", "https://mine.example/?q=%s")).unwrap();
        store.set_default("google").unwrap();
        store.remove("brave").unwrap();
        let mut edited = store.find("bing").unwrap().clone();
        edited.url = "https://evil.example/?q=%s".into();
        store.update("bing", edited).unwrap();
        store.restore_defaults();
        assert!(store.find("brave").is_some(), "a removed built-in returns");
        assert_eq!(store.find("bing").unwrap().url, "https://www.bing.com/search?q=%s", "an edited built-in is put back");
        assert_eq!(store.engines().len(), builtin_engines().len() + 1);
        assert_eq!(store.engines().last().unwrap().id, "mine", "custom engines stay, after the built-ins");
        assert_eq!(store.default_id(), "google", "the default choice survives");
        let mut clash = EngineStore::default();
        clash.remove("youtube").unwrap();
        clash.add(custom("Tube", "yt", "https://tube.example/?q=%s")).unwrap();
        clash.restore_defaults();
        assert_eq!(clash.find("tube").unwrap().nickname, "", "a custom engine gives a shipped nickname back");
        assert_eq!(clash.by_nickname("yt").unwrap().id, "youtube");
    }

    #[test]
    fn the_file_round_trips_and_bad_lines_are_dropped() {
        let mut store = EngineStore::default();
        store.add(Engine { post: "q=%s".into(), ..custom("Posty", "po", "https://posty.example/find") }).unwrap();
        store.set_default("google").unwrap();
        store.set_private("duckduckgo").unwrap();
        let text = store.serialize();
        assert_eq!(EngineStore::parse(&text), store);
        let dirty = format!(
            "{text}engine=bad id\tn\t\thttps://a.example/?q=%s\t\t\t\t\nengine=x\tonly\tthree\nengine=http\tHttp\t\thttp://a.example/?q=%s\t\t\t\t\nengine=google\tDup\t\thttps://dup.example/?q=%s\t\t\t\t\n"
        );
        assert_eq!(EngineStore::parse(&dirty), store, "invalid, short, insecure and duplicate lines change nothing");
        assert_eq!(EngineStore::parse(""), EngineStore::default());
        assert_eq!(EngineStore::parse("garbage\n# x\n"), EngineStore::default());
        let stale = "default=gone\nprivate=gone\nimage=gone\nengine=a\tA\t\thttps://a.example/?q=%s\t\t\t\t\n";
        let parsed = EngineStore::parse(stale);
        assert_eq!((parsed.default_id(), parsed.private_id(), parsed.image_id()), ("a", "", ""));
    }

    #[test]
    fn load_prefers_the_file_then_the_legacy_choice_then_a_custom_provider() {
        let dir = std::env::temp_dir().join(format!("bowser-engines-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(EngineStore::load(&dir, None, None).default_id(), "optimised");
        assert_eq!(EngineStore::load(&dir, Some(SearchEngine::Google), None).default_id(), "google");
        let from_provider = EngineStore::load(&dir, None, Some("https://p.example/s?q={query}"));
        assert_eq!(from_provider.default_id(), "custom");
        assert_eq!(from_provider.find("custom").unwrap().url, "https://p.example/s?q=%s");
        assert_eq!(EngineStore::load(&dir, None, Some("http://p.example/{query}")).default_id(), "optimised", "an insecure provider is ignored");
        let mut saved = EngineStore::default();
        saved.set_default("bing").unwrap();
        saved.save(&dir).unwrap();
        assert_eq!(EngineStore::load(&dir, Some(SearchEngine::Google), None).default_id(), "bing", "the file wins over the legacy setting");
        fs::write(dir.join(FILE_NAME), vec![b'#'; 70 * 1024]).unwrap();
        assert_eq!(EngineStore::load(&dir, None, None), EngineStore::default(), "an oversized file is ignored");
        let _ = fs::remove_dir_all(&dir);
    }
}
