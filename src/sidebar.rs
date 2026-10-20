//! Ordered, profile-local configuration for the future sidebar rail.
use std::{fs, io, path::Path};
const MAX_BYTES: usize = 256 * 1024;
const MAX_APPS: usize = 128;
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SidebarKind {
    Customize,
    Workspaces,
    Apps,
    Bookmarks,
    History,
    Menu,
}
impl SidebarKind {
    fn key(self) -> &'static str {
        match self {
            Self::Customize => "customize",
            Self::Workspaces => "workspaces",
            Self::Apps => "apps",
            Self::Bookmarks => "bookmarks",
            Self::History => "history",
            Self::Menu => "menu",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "customize" => Self::Customize,
            "workspaces" => Self::Workspaces,
            "apps" => Self::Apps,
            "bookmarks" => Self::Bookmarks,
            "history" => Self::History,
            "menu" => Self::Menu,
            _ => return None,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidebarItem {
    pub id: String,
    pub kind: SidebarKind,
    pub visible: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebApp {
    pub id: u32,
    pub name: String,
    pub url: String,
    pub favicon_file: Option<String>,
    pub position: usize,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidebarConfig {
    pub items: Vec<SidebarItem>,
    pub apps: Vec<WebApp>,
    next_id: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SidebarError {
    InvalidUrl,
    InvalidName,
    DuplicateUrl,
    TooMany,
    NotFound,
    Io(io::ErrorKind),
}
impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            items: vec![
                item(SidebarKind::Customize, true),
                item(SidebarKind::Workspaces, true),
                item(SidebarKind::Apps, true),
                item(SidebarKind::Bookmarks, true),
                item(SidebarKind::History, true),
                item(SidebarKind::Menu, true),
            ],
            apps: Vec::new(),
            next_id: 1,
        }
    }
}
fn item(kind: SidebarKind, visible: bool) -> SidebarItem {
    SidebarItem {
        id: kind.key().into(),
        kind,
        visible,
    }
}
impl SidebarConfig {
    pub fn load(profile_dir: &Path) -> io::Result<Self> {
        let path = profile_dir.join("sidebar.txt");
        match fs::read(&path) {
            Ok(bytes) => {
                if bytes.len() > MAX_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "sidebar config too large",
                    ));
                }
                let text = String::from_utf8_lossy(&bytes);
                Ok(Self::parse(&text).unwrap_or_default())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }
    /// Writes sidebar.txt. A file that is there but does not read as a sidebar (a header this
    /// build does not know, a hand edit, too large) is never replaced: [`Self::load`] gave the
    /// defaults for it, and saving those would drop every pinned web app it holds.
    pub fn save(&self, profile_dir: &Path) -> io::Result<()> {
        let path = profile_dir.join("sidebar.txt");
        match fs::read(&path) {
            Ok(bytes)
                if bytes.len() > MAX_BYTES
                    || Self::parse(&String::from_utf8_lossy(&bytes)).is_err() =>
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "sidebar.txt could not be read; it is left as it is",
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        atomic_write(&path, self.serialize().as_bytes())
    }
    pub fn add_app(&mut self, name: &str, url: &str) -> Result<u32, SidebarError> {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err(SidebarError::InvalidName);
        }
        let url = valid_url(url).ok_or(SidebarError::InvalidUrl)?;
        if self.apps.iter().any(|a| a.url.eq_ignore_ascii_case(&url)) {
            return Err(SidebarError::DuplicateUrl);
        }
        if self.apps.len() >= MAX_APPS {
            return Err(SidebarError::TooMany);
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(SidebarError::TooMany)?;
        let position = self.apps.len();
        self.apps.push(WebApp {
            id,
            name: name.into(),
            url,
            favicon_file: None,
            position,
        });
        Ok(id)
    }
    /// `remove_app`, remembering the app (name and address) so the rail's "Add web app" menu can
    /// restore it (thread T9, `panel_prefs.txt`), and dropping its per-panel options.
    pub fn remove_app_remembered(&mut self, id: u32) -> bool {
        let gone = self.apps.iter().find(|app| app.id == id).map(|app| (app.name.clone(), app.url.clone()));
        let removed = self.remove_app(id);
        if let (true, Some((name, url))) = (removed, gone) {
            let _ = crate::panel_prefs::update(|file| {
                file.forget(id);
                file.remember_removed(&name, &url);
            });
        }
        removed
    }

    pub fn remove_app(&mut self, id: u32) -> bool {
        let n = self.apps.len();
        self.apps.retain(|a| a.id != id);
        self.reindex();
        n != self.apps.len()
    }
    pub fn rename_app(&mut self, id: u32, name: &str) -> Result<bool, SidebarError> {
        let n = name.trim();
        if n.is_empty() || n.len() > 128 || n.chars().any(char::is_control) {
            return Err(SidebarError::InvalidName);
        }
        let Some(a) = self.apps.iter_mut().find(|a| a.id == id) else {
            return Ok(false);
        };
        a.name = n.into();
        Ok(true)
    }
    pub fn edit_url(&mut self, id: u32, url: &str) -> Result<bool, SidebarError> {
        let url = valid_url(url).ok_or(SidebarError::InvalidUrl)?;
        if self
            .apps
            .iter()
            .any(|a| a.id != id && a.url.eq_ignore_ascii_case(&url))
        {
            return Err(SidebarError::DuplicateUrl);
        }
        let Some(a) = self.apps.iter_mut().find(|a| a.id == id) else {
            return Ok(false);
        };
        a.url = url;
        Ok(true)
    }
    pub fn reorder_app(&mut self, id: u32, index: usize) -> Result<bool, SidebarError> {
        let Some(pos) = self.apps.iter().position(|a| a.id == id) else {
            return Ok(false);
        };
        let a = self.apps.remove(pos);
        self.apps.insert(index.min(self.apps.len()), a);
        self.reindex();
        Ok(true)
    }
    fn reindex(&mut self) {
        for (i, a) in self.apps.iter_mut().enumerate() {
            a.position = i
        }
    }
    pub fn set_visible(&mut self, kind: SidebarKind, visible: bool) -> Result<(), SidebarError> {
        if kind == SidebarKind::Menu && !visible {
            return Err(SidebarError::InvalidName);
        }
        let Some(i) = self.items.iter_mut().find(|i| i.kind == kind) else {
            return Err(SidebarError::NotFound);
        };
        i.visible = visible;
        Ok(())
    }
    /// Missing section records keep their default-visible behavior.
    pub fn is_visible(&self, kind: SidebarKind) -> bool {
        self.items
            .iter()
            .find(|item| item.kind == kind)
            .is_none_or(|item| item.visible)
    }
    pub fn reorder_section(&mut self, kind: SidebarKind, index: usize) -> Result<(), SidebarError> {
        if !matches!(kind, SidebarKind::Bookmarks | SidebarKind::History) {
            return Err(SidebarError::NotFound);
        }
        let Some(pos) = self.items.iter().position(|i| i.kind == kind) else {
            return Err(SidebarError::NotFound);
        };
        let i = self.items.remove(pos);
        let start = self
            .items
            .iter()
            .position(|i| matches!(i.kind, SidebarKind::Bookmarks | SidebarKind::History))
            .unwrap_or(self.items.len());
        let end = self
            .items
            .iter()
            .rposition(|i| matches!(i.kind, SidebarKind::Bookmarks | SidebarKind::History))
            .map_or(start, |i| i + 1);
        self.items.insert((start + index).min(end), i);
        Ok(())
    }
    pub fn serialize(&self) -> String {
        let mut o = String::from("bowser-sidebar-v1\n");
        for i in &self.items {
            o.push_str(&format!(
                "item\t{}\t{}\t{}\n",
                escape(&i.id),
                i.kind.key(),
                u8::from(i.visible)
            ));
        }
        for a in &self.apps {
            o.push_str(&format!(
                "app\t{}\t{}\t{}\t{}\t{}\n",
                a.id,
                escape(&a.name),
                escape(&a.url),
                a.favicon_file
                    .as_deref()
                    .map(escape)
                    .unwrap_or_else(|| "-".into()),
                a.position
            ));
        }
        o
    }
    pub fn parse(text: &str) -> io::Result<Self> {
        if text.len() > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "sidebar config too large",
            ));
        }
        let mut lines = text.strip_prefix('\u{FEFF}').unwrap_or(text).lines();
        if lines.next() != Some("bowser-sidebar-v1") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bad sidebar header",
            ));
        }
        let mut s = Self::default();
        s.items.clear();
        for line in lines {
            let mut f = line.splitn(6, '\t');
            match f.next() {
                Some("item") => {
                    let id = unescape(f.next().unwrap_or(""));
                    let Some(kind) = f.next().and_then(SidebarKind::parse) else {
                        continue;
                    };
                    let visible = f.next() == Some("1");
                    if !s.items.iter().any(|i| i.kind == kind) {
                        s.items.push(SidebarItem {
                            id,
                            kind,
                            visible: visible || kind == SidebarKind::Menu,
                        })
                    }
                }
                Some("app") => {
                    let Some(id) = f.next().and_then(|x| x.parse().ok()) else {
                        continue;
                    };
                    let name = unescape(f.next().unwrap_or(""));
                    let Some(url) = valid_url(&unescape(f.next().unwrap_or(""))) else {
                        continue;
                    };
                    let favicon = f
                        .next()
                        .filter(|x| *x != "-")
                        .map(unescape)
                        .filter(|x| safe_favicon_path(x));
                    let position = f
                        .next()
                        .and_then(|x| x.parse().ok())
                        .unwrap_or(s.apps.len());
                    if s.apps.len() < MAX_APPS
                        && !s.apps.iter().any(|a: &WebApp| a.id == id || a.url == url)
                    {
                        s.next_id = s.next_id.max(id.saturating_add(1));
                        s.apps.push(WebApp {
                            id,
                            name,
                            url,
                            favicon_file: favicon,
                            position,
                        })
                    }
                }
                _ => {}
            }
        }
        s.items.sort_by_key(|i| match i.kind {
            SidebarKind::Customize => 0,
            SidebarKind::Workspaces => 1,
            SidebarKind::Apps => 2,
            SidebarKind::Bookmarks => 3,
            SidebarKind::History => 4,
            SidebarKind::Menu => 5,
        });
        for k in [
            SidebarKind::Customize,
            SidebarKind::Workspaces,
            SidebarKind::Apps,
            SidebarKind::Bookmarks,
            SidebarKind::History,
            SidebarKind::Menu,
        ] {
            if !s.items.iter().any(|i| i.kind == k) {
                s.items.push(item(
                    k,
                    matches!(
                        k,
                        SidebarKind::Bookmarks | SidebarKind::History | SidebarKind::Menu
                    ),
                ))
            }
        }
        s.apps.sort_by_key(|a| a.position);
        s.reindex();
        Ok(s)
    }
}

/// Whether changing sidebar visibility should close the currently open rail panel.
pub fn should_close_hidden_panel(active: Option<SidebarKind>, config: &SidebarConfig) -> bool {
    matches!(active, Some(SidebarKind::Bookmarks | SidebarKind::History))
        && active.is_some_and(|kind| !config.is_visible(kind))
}
pub fn validate_app_url(s: &str) -> Option<String> {
    valid_url(s)
}
fn valid_url(s: &str) -> Option<String> {
    if s.len() > 2048 {
        return None;
    }
    let u = url::Url::parse(s).ok()?;
    if !matches!(u.scheme(), "http" | "https") || !u.username().is_empty() || u.password().is_some()
    {
        None
    } else {
        Some(u.to_string())
    }
}
fn safe_favicon_path(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 128
        && !s.contains(['/', '\\', ':'])
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
fn unescape(s: &str) -> String {
    let mut o = String::new();
    let mut c = s.chars();
    while let Some(x) = c.next() {
        if x != '\\' {
            o.push(x)
        } else {
            match c.next() {
                Some('t') => o.push('\t'),
                Some('n') => o.push('\n'),
                Some('r') => o.push('\r'),
                Some('\\') => o.push('\\'),
                Some(x) => {
                    o.push('\\');
                    o.push(x)
                }
                None => o.push('\\'),
            }
        }
    }
    o
}
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let a: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                windows::core::PCWSTR(a.as_ptr()),
                windows::core::PCWSTR(b.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )?;
        }
    }
    #[cfg(not(windows))]
    fs::rename(temp, path)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_validation_dedupe_and_roundtrip() {
        let mut c = SidebarConfig::default();
        assert!(c.add_app("bad", "javascript:alert(1)").is_err());
        assert!(c.add_app("bad", "file:///x").is_err());
        assert!(c.add_app("bad", "data:text/plain,x").is_err());
        assert!(c.add_app("bad", "https://user:pass@example.com").is_err());
        let id = c.add_app("Mail", "https://mail.example/").unwrap();
        assert_eq!(
            c.add_app("Again", "https://mail.example/").unwrap_err(),
            SidebarError::DuplicateUrl
        );
        assert_eq!(SidebarConfig::parse(&c.serialize()).unwrap(), c);
        assert!(c.remove_app(id));
    }
    #[test]
    fn corrupt_defaults_and_menu_cannot_hide() {
        assert_eq!(
            SidebarConfig::parse("broken").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            SidebarConfig::default().set_visible(SidebarKind::Menu, false),
            Err(SidebarError::InvalidName)
        );
    }

    #[test]
    fn a_sidebar_file_that_does_not_read_is_never_saved_over() {
        let dir = std::env::temp_dir().join(format!("bowser-sidebar-bom-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut pinned = SidebarConfig::default();
        pinned.add_app("Mail", "https://mail.example/").unwrap();
        // A BOM (Notepad's UTF-8) is not a different file: the pinned apps are read.
        fs::write(dir.join("sidebar.txt"), format!("\u{FEFF}{}", pinned.serialize())).unwrap();
        assert_eq!(SidebarConfig::load(&dir).unwrap().apps, pinned.apps);
        // A header this build does not know loads as defaults, and the next change must not
        // replace the file (and every pinned web app) with them.
        let unknown = pinned.serialize().replacen("bowser-sidebar-v1", "bowser-sidebar-v2", 1);
        fs::write(dir.join("sidebar.txt"), &unknown).unwrap();
        let mut loaded = SidebarConfig::load(&dir).unwrap_or_default();
        loaded.set_visible(SidebarKind::History, false).unwrap();
        assert!(loaded.save(&dir).is_err(), "saving over an unreadable sidebar.txt is refused");
        assert_eq!(fs::read_to_string(dir.join("sidebar.txt")).unwrap(), unknown);
        // A file that reads is saved as usual, and a missing one is created.
        fs::write(dir.join("sidebar.txt"), pinned.serialize()).unwrap();
        pinned.set_visible(SidebarKind::History, false).unwrap();
        pinned.save(&dir).unwrap();
        assert!(!SidebarConfig::load(&dir).unwrap().is_visible(SidebarKind::History));
        fs::remove_file(dir.join("sidebar.txt")).unwrap();
        SidebarConfig::default().save(&dir).unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn hiding_bookmarks_or_history_closes_only_that_open_panel() {
        let mut config = SidebarConfig::default();
        assert!(config.is_visible(SidebarKind::Bookmarks));
        assert!(!should_close_hidden_panel(Some(SidebarKind::Bookmarks), &config));
        config.set_visible(SidebarKind::Bookmarks, false).unwrap();
        config.set_visible(SidebarKind::History, false).unwrap();
        assert!(!config.is_visible(SidebarKind::Bookmarks));
        assert!(!config.is_visible(SidebarKind::History));
        assert!(config.is_visible(SidebarKind::Customize));
        assert!(should_close_hidden_panel(Some(SidebarKind::Bookmarks), &config));
        assert!(should_close_hidden_panel(Some(SidebarKind::History), &config));
        assert!(!should_close_hidden_panel(Some(SidebarKind::Customize), &config));
    }

    #[test]
    fn reorder_targets_move_apps_and_bottom_panels_within_their_sections() {
        let mut config = SidebarConfig::default();
        let mail = config.add_app("Mail", "https://mail.example/").unwrap();
        let docs = config.add_app("Docs", "https://docs.example/").unwrap();
        config.reorder_app(docs, 0).unwrap();
        assert_eq!(config.apps.iter().map(|app| app.id).collect::<Vec<_>>(), [docs, mail]);

        config.reorder_section(SidebarKind::History, 0).unwrap();
        let bottom: Vec<_> = config.items.iter()
            .filter(|item| matches!(item.kind, SidebarKind::Bookmarks | SidebarKind::History))
            .map(|item| item.kind)
            .collect();
        assert_eq!(bottom, [SidebarKind::History, SidebarKind::Bookmarks]);
    }
}
