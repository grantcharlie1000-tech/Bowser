//! Anchored composers: New workspace, Rename workspace, Add web app and Edit web app.
//!
//! Each opens as a page (`{t:"composer"}`) on the menu popup host, below-left of the control that
//! asked for it, instead of a centred name dialog. Nothing is created until the user confirms: the
//! page sends `compose` (name and, for web apps, address), Rust validates both again, and either
//! makes the change and answers `done`, or answers the same page with an error under the failing
//! field. Escape and click-away create nothing (docs/design/ntp-menus-motion.md, section 3).

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};

use crate::{
    app::{App, AppRef},
    chrome_host,
    win32::html_popup::{self, ActOutcome, Anchor, Page, Placement},
    workspaces::{WorkspaceError, WorkspaceId},
};

/// Longest workspace or web app name the composer accepts, in characters. The page checks it first.
pub(crate) const MAX_NAME_CHARS: usize = 40;

/// What a composer creates or edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Composer {
    WorkspaceNew,
    /// Renames the workspace with this id (`WorkspaceId`'s inner value).
    WorkspaceRename(u32),
    AppAdd,
    AppEdit(u32),
}

impl Composer {
    /// The popup kind, which also keys the reopen guard: reclicking the button that closed it
    /// does not reopen it. At most 32 UTF-16 code units on the wire.
    fn kind(&self) -> String {
        match self {
            Self::WorkspaceNew => "composer:workspace-new".to_owned(),
            Self::WorkspaceRename(id) => format!("composer:ws-rename:{id}"),
            Self::AppAdd => "composer:app-add".to_owned(),
            Self::AppEdit(id) => format!("composer:app-edit:{id}"),
        }
    }

    fn mode(&self) -> &'static str {
        match self {
            Self::WorkspaceNew => "workspace-new",
            Self::WorkspaceRename(_) => "workspace-rename",
            Self::AppAdd => "app-add",
            Self::AppEdit(_) => "app-edit",
        }
    }

    fn has_address(&self) -> bool {
        !matches!(self, Self::WorkspaceNew | Self::WorkspaceRename(_))
    }
}

/// Which field an error belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Name,
    Url,
}

impl Field {
    fn wire(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Url => "url",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Rejection {
    field: Field,
    text: String,
}

fn reject(field: Field, text: impl Into<String>) -> Rejection {
    Rejection { field, text: text.into() }
}

/// The trimmed name, or the message to show under the field. The rules every name shares.
fn check_name(text: &str) -> Result<String, String> {
    let name = text.trim();
    if name.is_empty() {
        return Err("Enter a name.".to_owned());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("Use at most {MAX_NAME_CHARS} characters."));
    }
    if name.chars().any(char::is_control) {
        return Err("Names cannot contain control characters.".to_owned());
    }
    Ok(name.to_owned())
}

/// The normalised http(s) address, or the message to show under the field.
fn check_url(text: &str) -> Result<String, String> {
    let address = text.trim();
    if address.is_empty() {
        return Err("Enter a web address.".to_owned());
    }
    let parsed = url::Url::parse(address).ok().filter(|url| matches!(url.scheme(), "http" | "https"));
    let Some(parsed) = parsed else {
        return Err("Enter an address starting with http:// or https://.".to_owned());
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Addresses cannot contain a user name or password.".to_owned());
    }
    crate::sidebar::validate_app_url(address)
        .ok_or_else(|| "Enter an address starting with http:// or https://.".to_owned())
}

/// The page the popup shows: the values as typed, the error under the failing field, or `done`
/// once the change is made (the page then fades out and dismisses itself).
fn page(composer: &Composer, name: &str, url: &str, error: Option<&Rejection>, done: bool) -> Page {
    let mode = composer.mode();
    let address = composer.has_address();
    let name = name.to_owned();
    let url = url.to_owned();
    let error = error.cloned();
    Page::new("composer", move |out| {
        out.string("mode", mode);
        out.uint("max", MAX_NAME_CHARS as u64);
        out.string("name", &name);
        if address {
            out.string("url", &url);
        }
        if let Some(error) = &error {
            out.object("error", |item| {
                item.string("field", error.field.wire());
                item.string("text", &error.text);
            });
        }
        if done {
            out.bool("done", true);
        }
    })
}

/// What the submit does. `Ok` means the change is made.
fn apply(owner: HWND, app: &AppRef, composer: &Composer, name: &str, url: Option<&str>) -> Result<(), Rejection> {
    let name = check_name(name).map_err(|text| reject(Field::Name, text))?;
    let address = if composer.has_address() {
        Some(check_url(url.unwrap_or("")).map_err(|text| reject(Field::Url, text))?)
    } else {
        None
    };
    match composer {
        Composer::WorkspaceNew => App::workspace_create(app, &name).map(|_| ()).map_err(|error| {
            reject(
                Field::Name,
                match error {
                    WorkspaceError::DuplicateName => format!("A workspace named “{name}” already exists."),
                    WorkspaceError::TooMany => "No more workspaces can be added.".to_owned(),
                    WorkspaceError::InvalidName => "Enter a name.".to_owned(),
                    _ => "The workspace could not be created.".to_owned(),
                },
            )
        }),
        Composer::WorkspaceRename(id) => App::rename_workspace(app, WorkspaceId(*id), &name).map_err(|error| {
            reject(
                Field::Name,
                match error {
                    WorkspaceError::DuplicateName => format!("A workspace named “{name}” already exists."),
                    WorkspaceError::TooMany => "No more workspaces can be added.".to_owned(),
                    WorkspaceError::InvalidName => "Enter a name.".to_owned(),
                    _ => "The workspace could not be renamed.".to_owned(),
                },
            )
        }),
        Composer::AppAdd | Composer::AppEdit(_) => {
            let address = address.unwrap_or_default();
            let profile = crate::profile::active().map_err(|_| reject(Field::Name, "The profile could not be opened."))?;
            let root = profile.root();
            let mut sidebar = crate::sidebar::SidebarConfig::load(root).unwrap_or_default();
            let saved_app = match composer {
                Composer::AppEdit(id) => {
                    let id = *id;
                    // The app may have been removed while the composer was open.
                    if !sidebar.apps.iter().any(|item| item.id == id) {
                        return Err(reject(Field::Name, "This web app was removed."));
                    }
                    sidebar.rename_app(id, &name).map_err(sidebar_rejection)?;
                    sidebar.edit_url(id, &address).map_err(sidebar_rejection)?;
                    id
                }
                _ => sidebar.add_app(&name, &address).map_err(sidebar_rejection)?,
            };
            sidebar
                .save(root)
                .map_err(|_| reject(Field::Name, "The change could not be saved."))?;
            if matches!(composer, Composer::AppEdit(_)) {
                super::panel_webapp::forget_app(saved_app);
            }
            if let Some(web_app) = sidebar.apps.iter().find(|item| item.id == saved_app).cloned() {
                super::panel_webapp::register_app(app, web_app);
            }
            crate::win32::post(owner, crate::win32::WM_RAIL_CHANGED, WPARAM(0), LPARAM(0));
            Ok(())
        }
    }
}

fn sidebar_rejection(error: crate::sidebar::SidebarError) -> Rejection {
    use crate::sidebar::SidebarError;
    match error {
        SidebarError::InvalidName => reject(Field::Name, "Enter a name."),
        SidebarError::InvalidUrl => reject(Field::Url, "Enter an address starting with http:// or https://."),
        SidebarError::DuplicateUrl => reject(Field::Url, "That web app is already pinned."),
        SidebarError::TooMany => reject(Field::Name, "No more web apps can be pinned."),
        SidebarError::NotFound => reject(Field::Name, "This web app was removed."),
        SidebarError::Io(_) => reject(Field::Name, "The change could not be saved."),
    }
}

/// The values a composer opens with. `prefill` is a preset (name, address) for Add web app.
fn initial_values(app: &AppRef, composer: &Composer, prefill: Option<(String, String)>) -> Option<(String, String)> {
    match composer {
        Composer::WorkspaceNew => Some((format!("Workspace {}", App::workspace_entries(app).len() + 1), String::new())),
        Composer::WorkspaceRename(id) => App::workspace_entries(app)
            .into_iter()
            .find(|(entry_id, ..)| entry_id.0 == *id)
            .map(|(_, name, ..)| (name, String::new())),
        Composer::AppAdd => Some(prefill.unwrap_or_default()),
        Composer::AppEdit(id) => {
            let profile = crate::profile::active().ok()?;
            let sidebar = crate::sidebar::SidebarConfig::load(profile.root()).ok()?;
            sidebar.apps.iter().find(|item| item.id == *id).map(|item| (item.name.clone(), item.url.clone()))
        }
    }
}

/// A composer requested from the Customize flyout, waiting for the deferred queue.
pub(crate) struct Request {
    pub composer: Composer,
    pub prefill: Option<(String, String)>,
    /// Screen pixels: the bottom-left of the button that asked.
    pub at: POINT,
}

thread_local! { static REQUESTS: RefCell<HashMap<isize, Request>> = RefCell::new(HashMap::new()); }

/// Keeps the newest request for `owner`; the deferred command that follows takes it.
pub(crate) fn stash(owner: HWND, request: Request) {
    REQUESTS.with(|requests| requests.borrow_mut().insert(owner.0 as isize, request));
}

pub(crate) fn take(owner: HWND) -> Option<Request> {
    REQUESTS.with(|requests| requests.borrow_mut().remove(&(owner.0 as isize)))
}

/// Opens `composer` below-left of `at` (screen pixels). Any open menu is closed first. False only
/// when the HTML chrome is off, so the caller runs its native fallback; a composer that could not
/// show right now (popup page still loading, or the one just dismissed) returns true.
pub(crate) fn open(owner: HWND, app: &AppRef, composer: Composer, prefill: Option<(String, String)>, at: POINT) -> bool {
    if !chrome_host::html_ui_enabled() {
        return false;
    }
    html_popup::cancel(owner, app);
    let Some((name, url)) = initial_values(app, &composer, prefill) else {
        return true;
    };
    let handler_composer = composer.clone();
    let mut first = page(&composer, &name, &url, None, false);
    first.compose(Rc::new(move |app, name, url| {
        match apply(owner, app, &handler_composer, name, url) {
            Ok(()) => ActOutcome::Refresh(page(&handler_composer, name, url.unwrap_or(""), None, true)),
            Err(error) => ActOutcome::Refresh(page(&handler_composer, name, url.unwrap_or(""), Some(&error), false)),
        }
    }));
    // A refusal hands the callback back; there is nothing to report either way: the popup page may
    // still be loading (its prewarm is requested), or this is the click that just dismissed it.
    let _ = html_popup::show_page(
        owner,
        Anchor::Point(at, Placement::BelowLeft),
        &composer.kind(),
        None,
        first,
        Box::new(|_, _| {}),
        None,
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_bounded_like_the_page_checks_them() {
        assert_eq!(check_name("  Work  "), Ok("Work".to_owned()));
        assert_eq!(check_name("   "), Err("Enter a name.".to_owned()));
        assert_eq!(check_name(&"a".repeat(MAX_NAME_CHARS)), Ok("a".repeat(MAX_NAME_CHARS)));
        assert_eq!(check_name(&"a".repeat(MAX_NAME_CHARS + 1)), Err("Use at most 40 characters.".to_owned()));
        assert_eq!(check_name(&"é".repeat(MAX_NAME_CHARS)), Ok("é".repeat(MAX_NAME_CHARS)), "the limit counts characters, not bytes");
        assert!(check_name("a\u{0007}b").is_err());
        assert!(check_name("a\nb").is_err());
    }

    #[test]
    fn addresses_must_be_plain_http_or_https() {
        assert_eq!(check_url(" https://example.com/ "), Ok("https://example.com/".to_owned()));
        assert_eq!(check_url("http://example.com/a?b=1"), Ok("http://example.com/a?b=1".to_owned()));
        for bad in ["", "example.com", "ftp://example.com/", "javascript:alert(1)", "file:///c:/x", "https://user:pw@example.com/", "https://"] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn each_composer_has_its_own_popup_kind_within_the_wire_limit() {
        assert_eq!(Composer::WorkspaceNew.kind(), "composer:workspace-new");
        assert_eq!(Composer::AppAdd.kind(), "composer:app-add");
        assert_eq!(Composer::AppEdit(7).kind(), "composer:app-edit:7");
        assert_eq!(Composer::WorkspaceRename(7).kind(), "composer:ws-rename:7");
        assert!(Composer::AppEdit(u32::MAX).kind().encode_utf16().count() <= 32);
        assert!(Composer::WorkspaceRename(u32::MAX).kind().encode_utf16().count() <= 32);
    }

    #[test]
    fn workspace_composers_are_name_only() {
        assert_eq!(Composer::WorkspaceRename(3).mode(), "workspace-rename");
        assert!(!Composer::WorkspaceNew.has_address());
        assert!(!Composer::WorkspaceRename(3).has_address());
        assert!(Composer::AppAdd.has_address());
        assert!(Composer::AppEdit(3).has_address());
        let json = html_popup::Page::debug_json(page(&Composer::WorkspaceRename(3), "Work", "", None, false), 4);
        assert_eq!(json, "{\"v\":1,\"t\":\"composer\",\"tok\":4,\"kind\":\"composer\",\"mode\":\"workspace-rename\",\"max\":40,\"name\":\"Work\"}");
    }

    #[test]
    fn the_page_carries_values_the_error_under_its_field_and_done() {
        let json = |page| html_popup::Page::debug_json(page, 4);
        let new = json(page(&Composer::WorkspaceNew, "Workspace 3", "", None, false));
        assert_eq!(new, "{\"v\":1,\"t\":\"composer\",\"tok\":4,\"kind\":\"composer\",\"mode\":\"workspace-new\",\"max\":40,\"name\":\"Workspace 3\"}");
        let error = reject(Field::Url, "Enter a web address.");
        let app = json(page(&Composer::AppAdd, "Mail", "", Some(&error), false));
        assert!(app.contains("\"url\":\"\""), "{app}");
        assert!(app.contains("\"error\":{\"field\":\"url\",\"text\":\"Enter a web address.\"}"), "{app}");
        assert!(!app.contains("done"));
        let done = json(page(&Composer::AppEdit(2), "Mail", "https://mail.example/", None, true));
        assert!(done.contains("\"mode\":\"app-edit\""));
        assert!(done.ends_with("\"done\":true}"), "{done}");
    }
}
