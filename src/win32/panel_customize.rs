//! Sidebar customisation panel: rail item visibility, pinned web apps (add, rename, change address,
//! reorder, remove), and workspace management (rename, reorder, icon, colour, delete).
//!
//! Everything here is pure. [`layout`] places each control in content coordinates. The same layout
//! drives painting and hit-testing, so the two cannot drift apart. Input becomes an [`Action`], and
//! the window layer applies it to `SidebarConfig` or `WorkspaceSet`.

use windows::Win32::Foundation::RECT;

use super::panel_widgets::{self as w, Key};
use crate::{
    sidebar::{SidebarKind, WebApp, validate_app_url},
    theme::Rgba,
    workspaces::{Workspace, WorkspaceColour, WorkspaceIcon},
};

pub(crate) const MAX_NAME: usize = 128;
pub(crate) const MAX_APPS: usize = 128;

/// Ready-made web apps. Choosing one fills the form, and the user still presses Add.
pub(crate) const PRESETS: [(&str, &str); 4] = [
    ("Spotify", "https://open.spotify.com/"),
    ("ChatGPT", "https://chatgpt.com/"),
    ("WhatsApp", "https://web.whatsapp.com/"),
    ("Notion", "https://www.notion.so/"),
];

pub(crate) const ICONS: [WorkspaceIcon; 12] = [
    WorkspaceIcon::Briefcase,
    WorkspaceIcon::Home,
    WorkspaceIcon::Code,
    WorkspaceIcon::Book,
    WorkspaceIcon::Cart,
    WorkspaceIcon::Game,
    WorkspaceIcon::Music,
    WorkspaceIcon::Heart,
    WorkspaceIcon::Star,
    WorkspaceIcon::Bolt,
    WorkspaceIcon::Globe,
    WorkspaceIcon::Leaf,
];

pub(crate) const COLOURS: [WorkspaceColour; 8] = [
    WorkspaceColour::Blue,
    WorkspaceColour::Violet,
    WorkspaceColour::Teal,
    WorkspaceColour::Amber,
    WorkspaceColour::Rose,
    WorkspaceColour::Red,
    WorkspaceColour::Green,
    WorkspaceColour::Slate,
];

pub(crate) fn icon_label(icon: WorkspaceIcon) -> &'static str {
    match icon {
        WorkspaceIcon::Briefcase => "Briefcase",
        WorkspaceIcon::Home => "Home",
        WorkspaceIcon::Code => "Code",
        WorkspaceIcon::Book => "Book",
        WorkspaceIcon::Cart => "Shopping",
        WorkspaceIcon::Game => "Games",
        WorkspaceIcon::Music => "Music",
        WorkspaceIcon::Heart => "Heart",
        WorkspaceIcon::Star => "Star",
        WorkspaceIcon::Bolt => "Bolt",
        WorkspaceIcon::Globe => "Globe",
        WorkspaceIcon::Leaf => "Leaf",
    }
}

pub(crate) fn colour_label(colour: WorkspaceColour) -> &'static str {
    match colour {
        WorkspaceColour::Blue => "Blue",
        WorkspaceColour::Violet => "Violet",
        WorkspaceColour::Teal => "Teal",
        WorkspaceColour::Amber => "Amber",
        WorkspaceColour::Rose => "Rose",
        WorkspaceColour::Red => "Red",
        WorkspaceColour::Green => "Green",
        WorkspaceColour::Slate => "Slate",
    }
}

/// Swatch colour for a workspace colour. Chosen to stay distinct on light and dark surfaces.
pub(crate) fn colour_rgba(colour: WorkspaceColour) -> Rgba {
    match colour {
        WorkspaceColour::Blue => Rgba::hex(0x3B7BE0, 0xFF),
        WorkspaceColour::Violet => Rgba::hex(0x7B5CD6, 0xFF),
        WorkspaceColour::Teal => Rgba::hex(0x14A3A0, 0xFF),
        WorkspaceColour::Amber => Rgba::hex(0xE9A23B, 0xFF),
        WorkspaceColour::Rose => Rgba::hex(0xE0457B, 0xFF),
        WorkspaceColour::Red => Rgba::hex(0xE74F5F, 0xFF),
        WorkspaceColour::Green => Rgba::hex(0x2E9E5B, 0xFF),
        WorkspaceColour::Slate => Rgba::hex(0x64748B, 0xFF),
    }
}

/// Label for a rail section in the visibility list.
pub(crate) fn section_label(kind: SidebarKind) -> &'static str {
    match kind {
        SidebarKind::Customize => "Customize",
        SidebarKind::Workspaces => "Workspaces",
        SidebarKind::Apps => "Web apps",
        SidebarKind::Bookmarks => "Bookmarks",
        SidebarKind::History => "History",
        SidebarKind::Menu => "Menu",
    }
}

/// Customize and Menu stay visible: they are how the user gets back to this panel.
pub(crate) fn is_locked(kind: SidebarKind) -> bool {
    matches!(kind, SidebarKind::Customize | SidebarKind::Menu)
}

/// The rail sections the user can hide, in rail order.
pub(crate) const TOGGLEABLE: [SidebarKind; 4] = [
    SidebarKind::Workspaces,
    SidebarKind::Apps,
    SidebarKind::Bookmarks,
    SidebarKind::History,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Plain,
    Hint,
    Problem,
    Ok,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FormStatus {
    pub tone: Tone,
    pub text: String,
}

/// Which app the add form is editing. `Add` means the form creates a new app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormMode {
    Add,
    Edit(u32),
}

/// What a press on a form control does. Stored so the window can focus the matching EDIT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Name,
    Url,
    WorkspaceName,
    NewWorkspace,
}

/// The icon or colour picker open under a workspace row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Picker {
    Icon(u32),
    Colour(u32),
}

/// Every control the layout can place. Button and row hits are what [`Model::press`] receives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hit {
    Toggle(SidebarKind),
    AppSelect(u32),
    AppRename(u32),
    AppAddress(u32),
    AppUp(u32),
    AppDown(u32),
    AppRemove(u32),
    Preset(usize),
    FormSubmit,
    FormCancel,
    WsRename(u32),
    WsIcon(u32),
    WsColour(u32),
    WsUp(u32),
    WsDown(u32),
    WsDelete(u32),
    WsRenameSave(u32),
    /// One tile of the open icon picker.
    WsSetIcon(u32, WorkspaceIcon),
    /// One tile of the open colour picker. `None` clears the colour.
    WsSetColour(u32, Option<WorkspaceColour>),
    WsCreate,
    SwitchProfile,
    ClearWebAppData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    Heading(&'static str),
    Note(FormStatus),
    Toggle {
        kind: SidebarKind,
        label: &'static str,
        on: bool,
        locked: bool,
    },
    AppRow {
        id: u32,
        title: String,
        detail: String,
        selected: bool,
    },
    Button {
        label: String,
        tone: Tone,
        enabled: bool,
    },
    /// Position of a native EDIT. The window places the control on this rectangle.
    Field(Field),
    WorkspaceRow {
        id: u32,
        name: String,
        icon: WorkspaceIcon,
        colour: Option<WorkspaceColour>,
        active: bool,
        tabs: usize,
    },
    /// A tile in the icon picker. `selected` marks the workspace's current icon.
    IconChoice {
        icon: WorkspaceIcon,
        selected: bool,
    },
    /// A tile in the colour picker. `None` is the uncoloured choice.
    ColourChoice {
        colour: Option<WorkspaceColour>,
        selected: bool,
    },
    Empty(&'static str),
}

/// A laid-out control in content coordinates. `hit` is what a press on its rectangle means.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placed {
    pub item: Item,
    pub rect: RECT,
    pub hit: Option<Hit>,
}

/// Metrics in device pixels, supplied by the window at the current DPI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Dims {
    pub width: i32,
    /// Viewport height. Scrolling stops when the last control reaches its bottom edge.
    pub height: i32,
    pub pad: i32,
    pub gap: i32,
    pub line: i32,
    pub row: i32,
    pub button: i32,
    pub field: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceEntry {
    pub id: u32,
    pub name: String,
    pub icon: WorkspaceIcon,
    pub colour: Option<WorkspaceColour>,
    pub active: bool,
    pub tabs: usize,
}

/// Snapshots the live workspace set into the rows the panel shows, in tab-strip order.
pub(crate) fn workspace_entries(all: &[Workspace], active: u32) -> Vec<WorkspaceEntry> {
    all.iter()
        .map(|ws| WorkspaceEntry {
            id: ws.id().0,
            name: ws.name().to_owned(),
            icon: ws.icon(),
            colour: ws.colour(),
            active: ws.id().0 == active,
            tabs: ws.tabs().len(),
        })
        .collect()
}

/// What the window must do with the stores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Nothing,
    Redraw,
    /// Edits the field and focuses it. The window moves focus to the EDIT.
    Focus(Field),
    SetVisible { kind: SidebarKind, visible: bool },
    AddApp { name: String, url: String },
    UpdateApp { id: u32, name: String, url: String },
    MoveApp { id: u32, index: usize },
    /// The window confirms before removing.
    RemoveApp { id: u32 },
    RenameWorkspace { id: u32, name: String },
    MoveWorkspace { id: u32, index: usize },
    /// The window confirms, then moves the workspace's tabs to a neighbour.
    DeleteWorkspace { id: u32 },
    SetIcon { id: u32, icon: WorkspaceIcon },
    SetColour { id: u32, colour: Option<WorkspaceColour> },
    CreateWorkspace { name: String },
    /// Asks the main window for its profile switcher, the same one Ctrl+Shift+P opens.
    SwitchProfile,
    /// The window confirms, then clears every pinned app's site data.
    ClearWebAppData,
    Dismiss,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Model {
    pub visible: Vec<(SidebarKind, bool)>,
    pub apps: Vec<WebApp>,
    pub workspaces: Vec<WorkspaceEntry>,
    pub selected_app: Option<u32>,
    pub mode: Option<FormMode>,
    pub name: String,
    pub url: String,
    /// Workspace whose name is being edited inline.
    pub renaming: Option<u32>,
    /// Text of the inline workspace-name EDIT, kept in step by the window.
    pub ws_name: String,
    /// Text of the new-workspace EDIT, kept in step by the window.
    pub new_workspace: String,
    /// Which workspace has its icon or colour picker open.
    pub picker: Option<Picker>,
    /// The profile this window belongs to, for the Profile row.
    pub profile: String,
    /// The last change's outcome, shown at the top. A failure stays until the next change.
    pub message: Option<String>,
    pub scroll: i32,
}

/// The message a successful web app data clear leaves behind. It is the one outcome shown as Ok.
const WEB_APP_CLEARED: &str = "Web app data cleared.";

/// Validates a web app name and address. Returns the trimmed name and normalised address.
pub(crate) fn check_app(
    name: &str,
    url: &str,
    apps: &[WebApp],
    editing: Option<u32>,
) -> Result<(String, String), (Tone, &'static str)> {
    let name = name.trim();
    if name.is_empty() {
        return Err((Tone::Problem, "Name it so you can find it in the rail."));
    }
    if name.len() > MAX_NAME || name.chars().any(char::is_control) {
        return Err((Tone::Problem, "Keep the name under 128 characters."));
    }
    if url.trim().is_empty() {
        return Err((Tone::Hint, "Paste the full address, such as https://example.com."));
    }
    let Some(url) = validate_app_url(url.trim()) else {
        return Err((Tone::Problem, "Enter a full https:// or http:// address."));
    };
    if apps
        .iter()
        .any(|a| Some(a.id) != editing && a.url.eq_ignore_ascii_case(&url))
    {
        return Err((Tone::Problem, "That address is already pinned."));
    }
    if editing.is_none() && apps.len() >= MAX_APPS {
        return Err((Tone::Problem, "The rail holds up to 128 web apps."));
    }
    Ok((name.to_owned(), url))
}

/// The live message under the form. It reports the first thing that needs fixing, and a ready
/// message once the form is valid.
pub(crate) fn form_status(name: &str, url: &str, apps: &[WebApp], editing: Option<u32>) -> FormStatus {
    if name.trim().is_empty() && url.trim().is_empty() {
        return FormStatus {
            tone: Tone::Hint,
            text: "Give the web app a name and its address.".into(),
        };
    }
    match check_app(name, url, apps, editing) {
        Ok(_) => FormStatus {
            tone: Tone::Ok,
            text: "Ready. Its icon is saved the first time the app loads.".into(),
        },
        Err((tone, text)) => FormStatus {
            tone,
            text: text.into(),
        },
    }
}

/// Icon status for an existing app, shown on its row.
pub(crate) fn icon_status(app: &WebApp) -> &'static str {
    if app.favicon_file.is_some() {
        "Icon saved"
    } else {
        "Icon not fetched yet"
    }
}

/// Position an app moves to for a step up (`-1`) or down (`+1`), or `None` at the edge.
pub(crate) fn step_index(len: usize, current: usize, delta: isize) -> Option<usize> {
    let next = current as isize + delta;
    (next >= 0 && (next as usize) < len).then_some(next as usize)
}

impl Model {
    pub(crate) fn sorted_apps(&self) -> Vec<WebApp> {
        let mut apps = self.apps.clone();
        apps.sort_by_key(|a| a.position);
        apps
    }

    fn visible_of(&self, kind: SidebarKind) -> bool {
        self.visible
            .iter()
            .find(|(k, _)| *k == kind)
            .is_none_or(|(_, v)| *v)
    }

    /// The form's current EDIT contents, validated for the mode.
    fn editing_id(&self) -> Option<u32> {
        match self.mode {
            Some(FormMode::Edit(id)) => Some(id),
            _ => None,
        }
    }

    pub(crate) fn status(&self) -> FormStatus {
        form_status(&self.name, &self.url, &self.apps, self.editing_id())
    }

    /// Fills the form from a preset. The user still presses Add.
    pub(crate) fn apply_preset(&mut self, index: usize) -> Action {
        let Some((name, url)) = PRESETS.get(index) else {
            return Action::Nothing;
        };
        self.mode = Some(FormMode::Add);
        self.name = (*name).into();
        self.url = (*url).into();
        Action::Redraw
    }

    /// Starts editing an app's name and address. The form switches to Save mode.
    pub(crate) fn edit_app(&mut self, id: u32) -> Action {
        let Some(app) = self.apps.iter().find(|a| a.id == id) else {
            return Action::Nothing;
        };
        self.selected_app = Some(id);
        self.mode = Some(FormMode::Edit(id));
        self.name = app.name.clone();
        self.url = app.url.clone();
        Action::Focus(Field::Name)
    }

    pub(crate) fn cancel_form(&mut self) {
        self.mode = None;
        self.name.clear();
        self.url.clear();
    }

    /// Submits the form. A valid add or edit returns its action and clears the form. An invalid one
    /// keeps the text so the user can fix it, and its message shows in the status line.
    pub(crate) fn submit(&mut self) -> Action {
        let editing = self.editing_id();
        match check_app(&self.name, &self.url, &self.apps, editing) {
            Ok((name, url)) => {
                let action = match editing {
                    Some(id) => Action::UpdateApp { id, name, url },
                    None => Action::AddApp { name, url },
                };
                self.cancel_form();
                action
            }
            Err(_) => Action::Redraw,
        }
    }

    /// Escape: leave the form or inline workspace rename, then the panel.
    pub(crate) fn key(&mut self, key: Key) -> Action {
        match key {
            Key::Escape => {
                if self.picker.take().is_some() {
                    Action::Redraw
                } else if self.renaming.take().is_some() || self.mode.is_some() {
                    self.cancel_form();
                    Action::Redraw
                } else {
                    Action::Dismiss
                }
            }
            Key::Up | Key::Down => {
                let apps = self.sorted_apps();
                let current = self
                    .selected_app
                    .and_then(|id| apps.iter().position(|a| a.id == id));
                match w::moved_selection(key, current, apps.len(), 1) {
                    Some(index) => {
                        self.selected_app = Some(apps[index].id);
                        Action::Redraw
                    }
                    None => Action::Nothing,
                }
            }
            Key::Delete => match self.selected_app {
                Some(id) => Action::RemoveApp { id },
                None => Action::Nothing,
            },
            Key::F2 => match self.selected_app {
                Some(id) => self.edit_app(id),
                None => Action::Nothing,
            },
            _ => Action::Nothing,
        }
    }

    /// Maps a press on a laid-out control to its action.
    pub(crate) fn press(&mut self, hit: Hit) -> Action {
        let apps = self.sorted_apps();
        match hit {
            Hit::Toggle(kind) => {
                if is_locked(kind) {
                    Action::Nothing
                } else {
                    Action::SetVisible {
                        kind,
                        visible: !self.visible_of(kind),
                    }
                }
            }
            Hit::AppSelect(id) => {
                self.selected_app = Some(id);
                Action::Redraw
            }
            Hit::AppRename(id) | Hit::AppAddress(id) => self.edit_app(id),
            Hit::AppUp(id) | Hit::AppDown(id) => {
                let Some(current) = apps.iter().position(|a| a.id == id) else {
                    return Action::Nothing;
                };
                let delta = if matches!(hit, Hit::AppUp(_)) { -1 } else { 1 };
                match step_index(apps.len(), current, delta) {
                    Some(index) => Action::MoveApp { id, index },
                    None => Action::Nothing,
                }
            }
            Hit::AppRemove(id) => Action::RemoveApp { id },
            Hit::Preset(index) => self.apply_preset(index),
            Hit::FormSubmit => self.submit(),
            Hit::FormCancel => {
                self.cancel_form();
                Action::Redraw
            }
            Hit::WsRename(id) => {
                self.renaming = Some(id);
                Action::Focus(Field::WorkspaceName)
            }
            Hit::WsRenameSave(id) => {
                self.renaming = None;
                Action::RenameWorkspace {
                    id,
                    name: self.ws_name.trim().to_owned(),
                }
            }
            Hit::WsIcon(id) => self.toggle_picker(Picker::Icon(id)),
            Hit::WsColour(id) => self.toggle_picker(Picker::Colour(id)),
            Hit::WsSetIcon(id, icon) => {
                self.picker = None;
                Action::SetIcon { id, icon }
            }
            Hit::WsSetColour(id, colour) => {
                self.picker = None;
                Action::SetColour { id, colour }
            }
            Hit::WsCreate => {
                let name = self.new_workspace.trim();
                if name.is_empty() {
                    Action::Nothing
                } else {
                    Action::CreateWorkspace { name: name.to_owned() }
                }
            }
            Hit::SwitchProfile => Action::SwitchProfile,
            Hit::ClearWebAppData => Action::ClearWebAppData,
            Hit::WsUp(id) | Hit::WsDown(id) => {
                let Some(current) = self.workspaces.iter().position(|w| w.id == id) else {
                    return Action::Nothing;
                };
                let delta = if matches!(hit, Hit::WsUp(_)) { -1 } else { 1 };
                match step_index(self.workspaces.len(), current, delta) {
                    Some(index) => Action::MoveWorkspace { id, index },
                    None => Action::Nothing,
                }
            }
            Hit::WsDelete(id) => Action::DeleteWorkspace { id },
        }
    }

    /// Opens a picker, or closes it when its button is pressed again.
    fn toggle_picker(&mut self, picker: Picker) -> Action {
        self.picker = if self.picker == Some(picker) { None } else { Some(picker) };
        Action::Redraw
    }

    /// Lays out every control top to bottom. Heights and offsets are content coordinates.
    pub(crate) fn layout(&self, d: Dims) -> Vec<Placed> {
        let mut out = Vec::new();
        let apps = self.sorted_apps();
        let inner = d.width - 2 * d.pad;
        let mut y = d.pad;
        let push = |out: &mut Vec<Placed>, item: Item, top: i32, height: i32, hit: Option<Hit>| {
            out.push(Placed {
                item,
                rect: RECT {
                    left: d.pad,
                    top,
                    right: d.pad + inner,
                    bottom: top + height,
                },
                hit,
            });
        };
        let heading = |out: &mut Vec<Placed>, text: &'static str, y: &mut i32| {
            push(out, Item::Heading(text), *y, d.line, None);
            *y += d.line + d.gap;
        };

        if let Some(text) = &self.message {
            let tone = if text == WEB_APP_CLEARED { Tone::Ok } else { Tone::Problem };
            push(&mut out, Item::Note(FormStatus { tone, text: text.clone() }), y, d.line, None);
            y += d.line + d.gap;
        }

        heading(&mut out, "Profile", &mut y);
        push(
            &mut out,
            Item::Note(FormStatus {
                tone: Tone::Plain,
                text: format!("Current profile: {}", self.profile),
            }),
            y,
            d.line,
            None,
        );
        y += d.line + d.gap;
        place_row(
            &mut out,
            &d,
            y,
            &[("Switch profile\u{2026}".into(), Some(Hit::SwitchProfile), Tone::Plain, true)],
        );
        y += d.button + d.gap * 2;

        heading(&mut out, "Rail items", &mut y);
        for kind in [SidebarKind::Customize]
            .into_iter()
            .chain(TOGGLEABLE)
            .chain([SidebarKind::Menu])
        {
            let locked = is_locked(kind);
            push(
                &mut out,
                Item::Toggle {
                    kind,
                    label: section_label(kind),
                    on: self.visible_of(kind),
                    locked,
                },
                y,
                d.row,
                (!locked).then_some(Hit::Toggle(kind)),
            );
            y += d.row;
        }
        y += d.gap;

        heading(&mut out, "Web apps", &mut y);
        if self.apps.is_empty() {
            push(&mut out, Item::Empty("No web apps yet. Add one below."), y, d.row, None);
            y += d.row;
        }
        let buttons = |selected: u32| -> Vec<(&'static str, Hit, Tone)> {
            vec![
                ("Rename", Hit::AppRename(selected), Tone::Plain),
                ("Address", Hit::AppAddress(selected), Tone::Plain),
                ("Up", Hit::AppUp(selected), Tone::Plain),
                ("Down", Hit::AppDown(selected), Tone::Plain),
                ("Remove", Hit::AppRemove(selected), Tone::Problem),
            ]
        };
        for app in &apps {
            let selected = self.selected_app == Some(app.id);
            push(
                &mut out,
                Item::AppRow {
                    id: app.id,
                    title: app.name.clone(),
                    detail: format!("{} · {}", w::host_label(&app.url), icon_status(app)),
                    selected,
                },
                y,
                d.row,
                Some(Hit::AppSelect(app.id)),
            );
            y += d.row;
            if selected {
                place_buttons(&mut out, &d, y, &buttons(app.id));
                y += d.button + d.gap;
            }
        }
        if !apps.is_empty() {
            place_row(
                &mut out,
                &d,
                y,
                &[("Clear web app data\u{2026}".into(), Some(Hit::ClearWebAppData), Tone::Problem, true)],
            );
            y += d.button + d.gap;
        }
        y += d.gap;

        heading(&mut out, "Add web app", &mut y);
        push(&mut out, Item::Field(Field::Name), y, d.field, None);
        y += d.field + d.gap;
        push(&mut out, Item::Field(Field::Url), y, d.field, None);
        y += d.field + d.gap;
        let status = self.status();
        push(&mut out, Item::Note(status), y, d.line, None);
        y += d.line + d.gap;
        let half = (inner - d.gap) / 2;
        for (index, (label, _)) in PRESETS.iter().enumerate() {
            let column = (index % 2) as i32;
            let row_top = y + (index / 2) as i32 * (d.button + d.gap);
            out.push(Placed {
                item: Item::Button {
                    label: (*label).into(),
                    tone: Tone::Plain,
                    enabled: true,
                },
                rect: RECT {
                    left: d.pad + column * (half + d.gap),
                    top: row_top,
                    right: d.pad + column * (half + d.gap) + half,
                    bottom: row_top + d.button,
                },
                hit: Some(Hit::Preset(index)),
            });
        }
        y += 2 * (d.button + d.gap);
        let editing = self.editing_id().is_some();
        let submit_label = if editing { "Save changes" } else { "Add web app" };
        let valid = self.status().tone == Tone::Ok;
        let action_row: Vec<(String, Option<Hit>, Tone, bool)> = {
            let mut row = vec![(submit_label.to_owned(), Some(Hit::FormSubmit), Tone::Ok, valid)];
            if editing {
                row.push(("Cancel".to_owned(), Some(Hit::FormCancel), Tone::Plain, true));
            }
            row
        };
        place_row(&mut out, &d, y, &action_row);
        y += d.button + d.gap * 2;

        heading(&mut out, "Workspaces", &mut y);
        if self.workspaces.is_empty() {
            push(&mut out, Item::Empty("No workspaces."), y, d.row, None);
            y += d.row;
        }
        for (index, ws) in self.workspaces.iter().enumerate() {
            push(
                &mut out,
                Item::WorkspaceRow {
                    id: ws.id,
                    name: ws.name.clone(),
                    icon: ws.icon,
                    colour: ws.colour,
                    active: ws.active,
                    tabs: ws.tabs,
                },
                y,
                d.row,
                Some(Hit::WsRename(ws.id)),
            );
            y += d.row;
            // The store keeps at least one workspace, so the last one cannot be deleted. Up and Down
            // are disabled at the ends of the list rather than doing nothing.
            let last = self.workspaces.len().saturating_sub(1);
            let row: Vec<(String, Option<Hit>, Tone, bool)> = vec![
                ("Rename".into(), Some(Hit::WsRename(ws.id)), Tone::Plain, true),
                ("Icon".into(), Some(Hit::WsIcon(ws.id)), Tone::Plain, true),
                ("Colour".into(), Some(Hit::WsColour(ws.id)), Tone::Plain, true),
                ("Up".into(), Some(Hit::WsUp(ws.id)), Tone::Plain, index > 0),
                ("Down".into(), Some(Hit::WsDown(ws.id)), Tone::Plain, index < last),
                ("Delete".into(), Some(Hit::WsDelete(ws.id)), Tone::Problem, self.workspaces.len() > 1),
            ];
            place_row(&mut out, &d, y, &row);
            y += d.button + d.gap;
            // Each open picker names the current choice first, so the tiles are not the only cue.
            match self.picker {
                Some(Picker::Icon(id)) if id == ws.id => {
                    let note = format!("Icon: {}", icon_label(ws.icon));
                    push(&mut out, Item::Note(FormStatus { tone: Tone::Hint, text: note }), y, d.line, None);
                    y = place_icon_choices(&mut out, &d, y + d.line + d.gap, ws.id, ws.icon);
                }
                Some(Picker::Colour(id)) if id == ws.id => {
                    let note = format!("Colour: {}", ws.colour.map_or("None", colour_label));
                    push(&mut out, Item::Note(FormStatus { tone: Tone::Hint, text: note }), y, d.line, None);
                    y = place_colour_choices(&mut out, &d, y + d.line + d.gap, ws.id, ws.colour);
                }
                _ => {}
            }
        }
        y += d.gap;

        let create_ready = !self.new_workspace.trim().is_empty();
        push(
            &mut out,
            Item::Note(FormStatus {
                tone: Tone::Hint,
                text: "New workspace".into(),
            }),
            y,
            d.line,
            None,
        );
        y += d.line + d.gap;
        push(&mut out, Item::Field(Field::NewWorkspace), y, d.field, None);
        y += d.field + d.gap;
        place_row(
            &mut out,
            &d,
            y,
            &[("Create workspace".into(), Some(Hit::WsCreate), Tone::Ok, create_ready)],
        );
        y += d.button + d.gap * 2;

        push(
            &mut out,
            Item::Note(FormStatus {
                tone: Tone::Hint,
                text: "Changes apply immediately. Deleting a workspace closes its tabs after you confirm."
                    .into(),
            }),
            y,
            d.line * 2,
            None,
        );
        out
    }

    pub(crate) fn content_height(&self, d: Dims) -> i32 {
        self.layout(d).iter().map(|p| p.rect.bottom).max().unwrap_or(0) + d.pad
    }

    /// The control under a window point, with the scroll applied. Later controls win, so buttons
    /// that sit on top of a row are found before the row.
    pub(crate) fn hit_at(&self, d: Dims, x: i32, y: i32) -> Option<Hit> {
        let y = y + self.scroll;
        self.layout(d)
            .iter()
            .rev()
            .find(|p| p.hit.is_some() && w::contains(p.rect, x, y))
            .and_then(|p| p.hit)
    }

    pub(crate) fn scroll_by(&mut self, delta: i32, d: Dims) {
        let max = (self.content_height(d) - d.height.max(0)).max(0);
        self.scroll = (self.scroll + delta).clamp(0, max);
    }
}

fn place_buttons(out: &mut Vec<Placed>, d: &Dims, top: i32, buttons: &[(&'static str, Hit, Tone)]) {
    let row: Vec<(String, Option<Hit>, Tone, bool)> = buttons
        .iter()
        .map(|(label, hit, tone)| ((*label).to_owned(), Some(*hit), *tone, true))
        .collect();
    place_row(out, d, top, &row);
}

/// Lays buttons out in one equal-width row across the content width.
fn place_row(out: &mut Vec<Placed>, d: &Dims, top: i32, buttons: &[(String, Option<Hit>, Tone, bool)]) {
    if buttons.is_empty() {
        return;
    }
    let inner = d.width - 2 * d.pad;
    let count = buttons.len() as i32;
    let each = (inner - (count - 1) * d.gap) / count;
    for (index, (label, hit, tone, enabled)) in buttons.iter().enumerate() {
        let left = d.pad + index as i32 * (each + d.gap);
        out.push(Placed {
            item: Item::Button {
                label: label.clone(),
                tone: *tone,
                enabled: *enabled,
            },
            rect: RECT {
                left,
                top,
                right: left + each,
                bottom: top + d.button,
            },
            hit: if *enabled { *hit } else { None },
        });
    }
}

/// Lays the icon picker out as rows of six tiles under a workspace row. Returns the y below it.
fn place_icon_choices(
    out: &mut Vec<Placed>,
    d: &Dims,
    top: i32,
    id: u32,
    current: WorkspaceIcon,
) -> i32 {
    const PER_ROW: usize = 6;
    let inner = d.width - 2 * d.pad;
    let each = (inner - (PER_ROW as i32 - 1) * d.gap) / PER_ROW as i32;
    for (index, icon) in ICONS.iter().enumerate() {
        let left = d.pad + (index % PER_ROW) as i32 * (each + d.gap);
        let row_top = top + (index / PER_ROW) as i32 * (d.button + d.gap);
        out.push(Placed {
            item: Item::IconChoice {
                icon: *icon,
                selected: *icon == current,
            },
            rect: RECT {
                left,
                top: row_top,
                right: left + each,
                bottom: row_top + d.button,
            },
            hit: Some(Hit::WsSetIcon(id, *icon)),
        });
    }
    top + ICONS.len().div_ceil(PER_ROW) as i32 * (d.button + d.gap)
}

/// Lays the colour picker out as one row: the uncoloured choice, then the palette. Returns the y below.
fn place_colour_choices(
    out: &mut Vec<Placed>,
    d: &Dims,
    top: i32,
    id: u32,
    current: Option<WorkspaceColour>,
) -> i32 {
    let choices: Vec<Option<WorkspaceColour>> = std::iter::once(None)
        .chain(COLOURS.iter().copied().map(Some))
        .collect();
    let inner = d.width - 2 * d.pad;
    let count = choices.len() as i32;
    let each = (inner - (count - 1) * d.gap) / count;
    for (index, colour) in choices.into_iter().enumerate() {
        let left = d.pad + index as i32 * (each + d.gap);
        out.push(Placed {
            item: Item::ColourChoice {
                colour,
                selected: colour == current,
            },
            rect: RECT {
                left,
                top,
                right: left + each,
                bottom: top + d.button,
            },
            hit: Some(Hit::WsSetColour(id, colour)),
        });
    }
    top + d.button + d.gap
}

// ---- Window layer ---------------------------------------------------------------------------

use std::path::PathBuf;

use windows::Win32::{
    Foundation::{LPARAM, POINT, WPARAM},
    Graphics::Gdi::{DeleteObject, HDC, HFONT, SelectObject, SetBkMode, TRANSPARENT},
    UI::Input::KeyboardAndMouse::{ReleaseCapture, VK_P},
};

use super::{
    panel_widgets::{EditEvent, EditField, EditSink, Surface},
    smooth,
};
use crate::{
    app::{App, AppRef},
    chrome::{
        rail::{self, RailIcon, WorkspaceGlyph},
        rail_icons,
    },
    profile::{self, Profile},
    sidebar::{SidebarConfig, SidebarError},
    theme::Palette,
    win32::{ask_yes_no_owned, side_panel::{self, PanelContent, PanelId}},
    workspaces::WorkspaceId,
};

const NAME: usize = 0;
const URL: usize = 1;
const WS_NAME: usize = 2;
const NEW_WS: usize = 3;

/// Modifier bits `WM_SHORTCUT_KEY` carries in its LPARAM: Ctrl, Shift, Alt.
const SHORTCUT_CTRL: isize = 1;
const SHORTCUT_SHIFT: isize = 2;

thread_local! {
    /// Guest sidebar settings are kept for the session only and never written to disk.
    static GUEST_CONFIG: std::cell::RefCell<Option<SidebarConfig>> = const { std::cell::RefCell::new(None) };
    static PENDING_EDIT_APP: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// Selects an app edit form when the Customize panel is next opened.
pub(crate) fn focus_app(id: u32) {
    PENDING_EDIT_APP.with(|pending| pending.set(Some(id)));
}

/// Registers the customisation panel with the host. Call once at startup, on the UI thread.
pub(crate) fn register(app: &AppRef) {
    let app = app.clone();
    side_panel::register_content(PanelId::Customize, move || Box::new(Content::new(app.clone())));
}

fn sidebar_message(error: SidebarError) -> &'static str {
    match error {
        SidebarError::InvalidUrl => "Enter a full https:// or http:// address.",
        SidebarError::InvalidName => "Enter a name for it.",
        SidebarError::DuplicateUrl => "That address is already pinned.",
        SidebarError::TooMany => "The rail holds up to 128 web apps.",
        SidebarError::NotFound => "That web app no longer exists.",
        SidebarError::Io(_) => "The rail settings could not be saved.",
    }
}

fn workspace_message(error: crate::workspaces::WorkspaceError) -> &'static str {
    use crate::workspaces::WorkspaceError as E;
    match error {
        E::InvalidName => "Use a name that is not empty.",
        E::DuplicateName => "Another workspace already has that name.",
        E::LastWorkspace => "Keep at least one workspace.",
        E::NotFound => "That workspace no longer exists.",
        _ => "The workspace could not be changed.",
    }
}

/// The name the Profile row shows: the listed name, `Default` for the default profile, and a plain
/// word for a guest window.
fn profile_label(profile: Option<&Profile>) -> String {
    match profile {
        Some(p) if p.is_guest() => "Guest (not saved)".to_owned(),
        Some(p) if p.listed_name() == profile::DEFAULT_NAME => "Default".to_owned(),
        Some(p) => p.listed_name().to_owned(),
        None => "Unknown".to_owned(),
    }
}

/// The glyph a workspace icon draws, the same one the rail uses.
fn glyph_for(icon: WorkspaceIcon) -> WorkspaceGlyph {
    match icon {
        WorkspaceIcon::Briefcase => WorkspaceGlyph::Briefcase,
        WorkspaceIcon::Home => WorkspaceGlyph::Home,
        WorkspaceIcon::Code => WorkspaceGlyph::Code,
        WorkspaceIcon::Book => WorkspaceGlyph::Book,
        WorkspaceIcon::Cart => WorkspaceGlyph::Cart,
        WorkspaceIcon::Game => WorkspaceGlyph::Game,
        WorkspaceIcon::Music => WorkspaceGlyph::Music,
        WorkspaceIcon::Heart => WorkspaceGlyph::Heart,
        WorkspaceIcon::Star => WorkspaceGlyph::Star,
        WorkspaceIcon::Bolt => WorkspaceGlyph::Bolt,
        WorkspaceIcon::Globe => WorkspaceGlyph::Globe,
        WorkspaceIcon::Leaf => WorkspaceGlyph::Leaf,
    }
}

pub(crate) struct Content {
    app: AppRef,
    profile: Option<Profile>,
    dir: Option<PathBuf>,
    config: SidebarConfig,
    read_only: bool,
    model: Model,
    surface: Surface,
    font: HFONT,
    font_dpi: u32,
    fields: Option<Fields>,
    /// Workspace whose name the inline EDIT was last filled for, so repaints do not reset it.
    renaming_shown: Option<u32>,
    focus_app_on_paint: Option<u32>,
}

struct Fields {
    name: EditField,
    url: EditField,
    workspace: EditField,
    new_workspace: EditField,
}

impl Drop for Content {
    fn drop(&mut self) {
        self.fields = None;
        if !self.font.is_invalid() {
            unsafe {
                let _ = DeleteObject(self.font.into());
            }
        }
    }
}

impl Content {
    fn new(app: AppRef) -> Self {
        let profile = profile::active().ok();
        let (config, dir, read_only) = match &profile {
            Some(p) if p.is_guest() => (
                GUEST_CONFIG
                    .with(|c| c.borrow().clone())
                    .unwrap_or_default(),
                None,
                false,
            ),
            Some(p) => {
                let dir = p.sidebar_file().parent().map(PathBuf::from);
                match dir.as_deref().map(SidebarConfig::load) {
                    Some(Ok(config)) => (config, dir, false),
                    Some(Err(error)) => {
                        crate::win32::show_error_message(&format!(
                            "Sidebar settings could not be read: {error}. Changes will not be saved."
                        ));
                        (SidebarConfig::default(), dir, true)
                    }
                    None => (SidebarConfig::default(), None, true),
                }
            }
            None => (SidebarConfig::default(), None, true),
        };
        let mut content = Self {
            app,
            profile,
            dir,
            config,
            read_only,
            model: Model::default(),
            surface: Surface::default(),
            font: HFONT::default(),
            font_dpi: 0,
            fields: None,
            renaming_shown: None,
            focus_app_on_paint: None,
        };
        content.reload();
        if let Some(id) = PENDING_EDIT_APP.with(std::cell::Cell::take) {
            if matches!(content.model.edit_app(id), Action::Focus(_)) {
                content.focus_app_on_paint = Some(id);
            }
        }
        content
    }

    fn px(&self, dip: i32) -> i32 {
        self.surface.px(dip)
    }

    fn dims(&self) -> Dims {
        let viewport = (self.surface.origin.bottom - self.surface.origin.top).max(0);
        Dims {
            width: (self.surface.origin.right - self.surface.origin.left).max(0),
            height: viewport,
            pad: self.px(12),
            gap: self.px(6),
            line: self.px(18),
            row: self.px(36),
            button: self.px(26),
            field: self.px(28),
        }
    }

    fn ensure_controls(&mut self) {
        if self.surface.host.is_invalid() {
            return;
        }
        if self.font.is_invalid() || self.font_dpi != self.surface.dpi {
            let old_font = self.font;
            let new_font = w::panel_font(self.surface.dpi);
            self.font = new_font;
            self.font_dpi = self.surface.dpi;
            if let Some(fields) = &self.fields {
                fields.name.set_font(new_font);
                fields.url.set_font(new_font);
                fields.workspace.set_font(new_font);
                fields.new_workspace.set_font(new_font);
            }
            if !old_font.is_invalid() { unsafe { let _ = DeleteObject(old_font.into()); } }
        }
        if self.fields.is_none() {
            let sink: *mut dyn EditSink = self as *mut Content;
            self.fields = Some(Fields {
                name: EditField::new(self.surface.host, sink, NAME, self.font),
                url: EditField::new(self.surface.host, sink, URL, self.font),
                workspace: EditField::new(self.surface.host, sink, WS_NAME, self.font),
                new_workspace: EditField::new(self.surface.host, sink, NEW_WS, self.font),
            });
            if let Some(fields) = &self.fields {
                fields.name.set_cue("Name");
                fields.url.set_cue("Address, such as https://example.com");
                fields.new_workspace.set_cue("New workspace name");
            }
        }
    }

    /// Pulls the sidebar settings and the live workspace list into the view model.
    fn reload(&mut self) {
        self.model.visible = self
            .config
            .items
            .iter()
            .map(|item| (item.kind, item.visible))
            .collect();
        self.model.apps = self.config.apps.clone();
        let set = App::workspaces(&self.app);
        self.model.workspaces = workspace_entries(set.all(), set.active_id().0);
        self.model.profile = profile_label(self.profile.as_ref());
        self.surface.invalidate();
    }

    /// Saves a sidebar change, or reports why it failed. Guest changes stay in memory.
    fn finish_sidebar(&mut self, result: Result<(), SidebarError>) {
        match result {
            Ok(()) => {
                self.model.message =None;
                self.persist();
            }
            Err(error) => self.model.message =Some(sidebar_message(error).into()),
        }
        self.reload();
    }

    fn persist(&mut self) {
        if self.read_only {
            return;
        }
        match &self.profile {
            Some(p) if p.is_guest() => {
                GUEST_CONFIG.with(|c| *c.borrow_mut() = Some(self.config.clone()));
            }
            _ => {
                if let Some(dir) = &self.dir
                    && let Err(error) = self.config.save(dir)
                {
                    self.model.message =Some(format!("The rail settings could not be saved: {error}"));
                }
            }
        }
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Nothing => return,
            Action::Redraw => {}
            Action::Focus(field) => self.focus(field),
            Action::SetVisible { kind, visible } => {
                let result = self.config.set_visible(kind, visible);
                self.finish_sidebar(result);
            }
            Action::AddApp { name, url } => {
                let result = self.config.add_app(&name, &url).map(|_| ());
                let ok = result.is_ok();
                self.finish_sidebar(result);
                if ok {
                    self.model.cancel_form();
                }
            }
            Action::UpdateApp { id, name, url } => {
                let result = self
                    .config
                    .rename_app(id, &name)
                    .and_then(|_| self.config.edit_url(id, &url))
                    .map(|_| ());
                let ok = result.is_ok();
                self.finish_sidebar(result);
                if ok {
                    self.model.cancel_form();
                }
            }
            Action::MoveApp { id, index } => {
                let result = self.config.reorder_app(id, index).map(|_| ());
                self.finish_sidebar(result);
            }
            Action::RemoveApp { id } => {
                let Some(app) = self.config.apps.iter().find(|a| a.id == id).cloned() else {
                    return;
                };
                if ask_yes_no_owned(self.surface.owner(), &format!("Remove \"{}\" from the rail? Its saved data stays in its own window.", app.name)) {
                    self.config.remove_app(id);
                    self.model.selected_app = None;
                    self.finish_sidebar(Ok(()));
                }
            }
            Action::RenameWorkspace { id, name } => {
                let result = App::rename_workspace(&self.app, WorkspaceId(id), &name);
                self.model.renaming = None;
                self.renaming_shown = None;
                self.model.message =result.err().map(|e| workspace_message(e).into());
                self.reload();
            }
            Action::DeleteWorkspace { id } => self.delete_workspace(id),
            Action::MoveWorkspace { id, index } => {
                let result = App::workspace_move(&self.app, WorkspaceId(id), index);
                self.model.message =result.err().map(|e| workspace_message(e).into());
                self.reload();
            }
            Action::SetIcon { id, icon } => {
                let result = App::workspace_set_icon(&self.app, WorkspaceId(id), icon);
                self.model.message =result.err().map(|e| workspace_message(e).into());
                self.reload();
            }
            Action::SetColour { id, colour } => {
                let result = App::workspace_set_colour(&self.app, WorkspaceId(id), colour);
                self.model.message =result.err().map(|e| workspace_message(e).into());
                self.reload();
            }
            Action::CreateWorkspace { name } => {
                match App::workspace_create(&self.app, &name) {
                    Ok(_) => {
                        self.model.message =None;
                        self.model.new_workspace.clear();
                        self.push_form_text();
                    }
                    Err(error) => self.model.message =Some(workspace_message(error).into()),
                }
                self.reload();
            }
            Action::ClearWebAppData => {
                if ask_yes_no_owned(self.surface.owner(),
                    "Clear the cookies, storage and cache of every pinned web app? Each one will ask you to sign in again.",
                ) {
                    self.model.message =match super::panel_webapp::clear_all_data() {
                        Ok(()) => Some("Web app data cleared.".into()),
                        Err(error) => Some(format!("The web app data could not be cleared: {error}")),
                    };
                }
            }
            Action::SwitchProfile => {
                // The profile switcher lives in the main window's menu code. Ctrl+Shift+P opens it
                // through the same shortcut dispatch, so the panel asks for it the same way.
                // Shortcut dispatch and profile switching are handled by the main-window menu.
                crate::win32::post(
                    self.surface.owner(),
                    crate::win32::WM_SHORTCUT_KEY,
                    WPARAM(VK_P.0 as usize),
                    LPARAM(SHORTCUT_CTRL | SHORTCUT_SHIFT),
                );
                return;
            }
            Action::Dismiss => {
                side_panel::close(self.surface.owner());
                return;
            }
        }
        self.surface.invalidate();
    }

    /// Deletes a workspace after confirmation. The app closes its tabs, so the prompt says how many.
    fn delete_workspace(&mut self, id: u32) {
        let Some(entry) = self.model.workspaces.iter().find(|w| w.id == id).cloned() else {
            return;
        };
        let message = match entry.tabs {
            0 => format!("Delete the workspace \"{}\"?", entry.name),
            1 => format!("Delete \"{}\" and close its 1 tab?", entry.name),
            n => format!("Delete \"{}\" and close its {n} tabs?", entry.name),
        };
        if !ask_yes_no_owned(self.surface.owner(), &message) {
            return;
        }
        let result = App::delete_workspace(&self.app, WorkspaceId(id));
        self.model.message =result.err().map(|e| workspace_message(e).into());
        self.reload();
    }

    fn focus(&self, field: Field) {
        let Some(fields) = &self.fields else {
            return;
        };
        match field {
            Field::Name => fields.name.focus(),
            Field::Url => fields.url.focus(),
            Field::WorkspaceName => fields.workspace.focus(),
            Field::NewWorkspace => fields.new_workspace.focus(),
        }
    }

    /// Enter in the form submits. Enter in the workspace EDIT saves the rename.
    fn submit_form(&mut self) {
        let action = self.model.submit();
        self.run(action);
    }

    /// Writes the model's form text into the EDITs when the model changed it, for example from a preset.
    fn push_form_text(&mut self) {
        let Some(fields) = &self.fields else {
            return;
        };
        if fields.name.text() != self.model.name {
            fields.name.set_text(&self.model.name);
        }
        if fields.url.text() != self.model.url {
            fields.url.set_text(&self.model.url);
        }
        if fields.new_workspace.text() != self.model.new_workspace {
            fields.new_workspace.set_text(&self.model.new_workspace);
        }
        match self.model.renaming {
            Some(id) if self.renaming_shown != Some(id) => {
                if let Some(entry) = self.model.workspaces.iter().find(|w| w.id == id) {
                    fields.workspace.set_text(&entry.name);
                    self.model.ws_name = entry.name.clone();
                }
                self.renaming_shown = Some(id);
            }
            Some(_) => {}
            None => self.renaming_shown = None,
        }
    }
}

impl EditSink for Content {
    fn edit_event(&mut self, field: usize, event: EditEvent) -> bool {
        match (field, event) {
            (NAME | URL, EditEvent::Changed) => {
                if let Some(fields) = &self.fields {
                    self.model.name = fields.name.text();
                    self.model.url = fields.url.text();
                }
                self.surface.invalidate();
                false
            }
            (WS_NAME, EditEvent::Changed) => {
                if let Some(fields) = &self.fields {
                    self.model.ws_name = fields.workspace.text();
                }
                false
            }
            (NAME | URL, EditEvent::Key(Key::Enter)) => {
                self.submit_form();
                true
            }
            (NAME | URL, EditEvent::Key(Key::Escape)) => {
                let action = self.model.key(Key::Escape);
                self.push_form_text();
                self.run(action);
                true
            }
            (WS_NAME, EditEvent::Key(Key::Enter)) => {
                if let Some(id) = self.model.renaming {
                    let action = self.model.press(Hit::WsRenameSave(id));
                    self.run(action);
                }
                true
            }
            (NEW_WS, EditEvent::Changed) => {
                if let Some(fields) = &self.fields {
                    self.model.new_workspace = fields.new_workspace.text();
                }
                // The Create button enables and disables with the text.
                self.surface.invalidate();
                false
            }
            (NEW_WS, EditEvent::Key(Key::Enter)) => {
                let action = self.model.press(Hit::WsCreate);
                self.run(action);
                true
            }
            (NEW_WS, EditEvent::Key(Key::Escape)) => {
                self.model.new_workspace.clear();
                self.push_form_text();
                let action = self.model.key(Key::Escape);
                self.run(action);
                true
            }
            (WS_NAME, EditEvent::Key(Key::Escape)) => {
                self.model.renaming = None;
                self.renaming_shown = None;
                self.surface.focus_host();
                self.surface.invalidate();
                true
            }
            (_, EditEvent::Tab { .. }) => {
                self.surface.focus_host();
                true
            }
            (NAME | URL, EditEvent::Key(key @ (Key::Up | Key::Down))) => {
                let action = self.model.key(key);
                self.run(action);
                true
            }
            _ => false,
        }
    }
}

impl PanelContent for Content {
    fn on_host_created(&mut self, host: windows::Win32::Foundation::HWND) {
        self.surface.attach(host);
    }

    fn paint(&mut self, hdc: HDC, rect: RECT, palette: &Palette) {
        let palette = &w::opaque(palette);
        self.surface.learn(hdc, rect, palette);
        if w::too_narrow(&self.surface) {
            return;
        }
        self.ensure_controls();
        let d = self.dims();
        let mut placed = self.model.layout(d);
        let old = unsafe { SelectObject(hdc, self.font.into()) };
        unsafe {
            SetBkMode(hdc, TRANSPARENT);
        }
        self.push_form_text();
        let mut focus_name = false;
        if let Some(id) = self.focus_app_on_paint.take() {
            if self.model.selected_app == Some(id) {
                if let Some(form) = placed.iter().find(|item| item.hit == Some(Hit::FormSubmit)) {
                    self.model.scroll = form.rect.top.saturating_sub(d.height / 2).max(0);
                    placed = self.model.layout(d);
                }
                focus_name = true;
            }
        }
        self.place_fields(&placed, d);
        if focus_name { self.focus(Field::Name); }
        let scroll = self.model.scroll;
        for item in &placed {
            // Skip rows that are scrolled out of the viewport.
            if item.rect.bottom - scroll < 0 || item.rect.top - scroll > d.height {
                continue;
            }
            let view = self.view(item.rect);
            unsafe { self.draw_item(hdc, item, view, palette) };
        }
        unsafe {
            SelectObject(hdc, old);
        }
    }

    fn on_mouse_move(&mut self, _x: i32, _y: i32) {}

    fn on_mouse_down(&mut self, x: i32, y: i32) {
        self.surface.focus_host();
        let (cx, cy) = self.surface.to_content(x, y);
        // No drag here, so the panel does not capture the mouse. A capture held across an action
        // that opens a menu or a confirmation dialog makes the modal loop fight the capture.
        let d = self.dims();
        if let Some(hit) = self.model.hit_at(d, cx, cy) {
            let action = self.model.press(hit);
            self.run(action);
        }
    }

    fn on_mouse_up(&mut self, _x: i32, _y: i32) {
        unsafe {
            let _ = ReleaseCapture();
        }
    }

    fn on_mouse_wheel(&mut self, delta: i16) {
        let d = self.dims();
        self.model.scroll_by(-(delta.signum() as i32) * d.row * 2, d);
        self.surface.invalidate();
    }

    fn on_char(&mut self, _ch: char) {}

    fn on_key(&mut self, key: u32, _shift: bool, _ctrl: bool) {
        if key == windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB.0 as u32 {
            if let Some(fields) = &self.fields {
                fields.name.focus();
            }
            return;
        }
        if let Some(k) = w::key_from_vk(key as u16) {
            let action = self.model.key(k);
            self.run(action);
        }
    }

    fn on_focus(&mut self, focused: bool) {
        if focused {
            self.reload();
        }
    }

    fn title(&self) -> &str {
        "Customize"
    }

    fn preferred_width(&self) -> i32 {
        360
    }
}

impl Content {
    /// Content rectangle moved into the viewport, relative to the content origin.
    fn view(&self, rect: RECT) -> RECT {
        let scroll = self.model.scroll;
        self.surface.to_host(RECT {
            left: rect.left,
            top: rect.top - scroll,
            right: rect.right,
            bottom: rect.bottom - scroll,
        })
    }

    /// Positions the three EDITs on their laid-out fields, or hides them when they are scrolled out
    /// of view. Each rectangle is taken to viewport space before the visibility check.
    fn place_fields(&mut self, placed: &[Placed], d: Dims) {
        let scroll = self.model.scroll;
        let mut name = None;
        let mut url = None;
        let mut workspace = None;
        let mut new_workspace = None;
        for item in placed {
            match &item.item {
                Item::Field(Field::Name) => name = Some(item.rect),
                Item::Field(Field::Url) => url = Some(item.rect),
                Item::Field(Field::NewWorkspace) => new_workspace = Some(item.rect),
                Item::WorkspaceRow { id, .. } if self.model.renaming == Some(*id) => {
                    workspace = Some(RECT {
                        left: item.rect.left + self.px(28),
                        top: item.rect.top,
                        right: item.rect.right - self.px(96),
                        bottom: item.rect.bottom,
                    })
                }
                _ => {}
            }
        }
        let place = |r: Option<RECT>| -> Option<RECT> {
            let r = r?;
            let top = r.top - scroll;
            let bottom = r.bottom - scroll;
            (bottom > 0 && top < d.height).then(|| {
                self.surface.to_host(RECT {
                    left: r.left,
                    top,
                    right: r.right,
                    bottom,
                })
            })
        };
        let (name, url, workspace, new_workspace) =
            (place(name), place(url), place(workspace), place(new_workspace));
        if let Some(fields) = self.fields.as_mut() {
            fields.name.place(name);
            fields.url.place(url);
            fields.workspace.place(workspace);
            fields.new_workspace.place(new_workspace);
        }
    }

    /// A square of `side` device pixels centred in `rect`.
    fn centred_square(&self, rect: RECT, side: i32) -> RECT {
        let left = rect.left + (rect.right - rect.left - side) / 2;
        let top = rect.top + (rect.bottom - rect.top - side) / 2;
        RECT { left, top, right: left + side, bottom: top + side }
    }

    /// Strokes a workspace glyph into `rect`, scaled from the rail's 24-unit grid with the rail's pen.
    unsafe fn draw_glyph(&self, dc: HDC, rect: RECT, icon: WorkspaceIcon, colour: Rgba) {
        let scale = (rect.right - rect.left) as f32 / rail_icons::GRID;
        let at = |(x, y): (f32, f32)| POINT {
            x: rect.left + (x * scale).round() as i32,
            y: rect.top + (y * scale).round() as i32,
        };
        let pen = (rail::STROKE * self.surface.dpi as f32 / 96.0).round().max(1.0) as i32;
        for primitive in rail_icons::primitives(RailIcon::Glyph(glyph_for(icon))) {
            unsafe {
                match primitive {
                    rail_icons::Primitive::Segment { from, to } => {
                        smooth::stroke_line(dc, at(from), at(to), colour, pen);
                    }
                    rail_icons::Primitive::Dot { centre, radius } => {
                        let c = at(centre);
                        let r = (radius * scale).round() as i32;
                        smooth::fill_ellipse(
                            dc,
                            RECT { left: c.x - r, top: c.y - r, right: c.x + r, bottom: c.y + r },
                            colour,
                        );
                    }
                }
            }
        }
    }

    unsafe fn draw_item(&self, dc: HDC, item: &Placed, view: RECT, palette: &Palette) {
        let text = palette.tab_text_active;
        let muted = palette.tab_text_inactive;
        match &item.item {
            Item::Heading(label) => unsafe {
                w::draw_text(dc, view, label, muted, false, true);
                smooth::fill_round_rect(dc, RECT { left: view.left, top: view.bottom - 1, right: view.right, bottom: view.bottom }, 2, palette.hairline);
            },
            Item::Note(status) => {
                let colour = match status.tone {
                    Tone::Problem => w::danger(palette),
                    Tone::Ok => palette.progress_fill,
                    Tone::Hint => muted,
                    Tone::Plain => text,
                };
                unsafe { w::draw_text(dc, view, &status.text, colour, false, true) };
            }
            Item::Toggle { label, on, locked, .. } => unsafe {
                let label_rect = RECT { right: view.right - self.px(44), ..view };
                w::draw_text(dc, label_rect, label, if *locked { muted } else { text }, false, true);
                if *locked {
                    w::draw_text(dc, RECT { left: view.right - self.px(96), ..view }, "Always shown", muted, false, true);
                    return;
                }
                let track = RECT {
                    left: view.right - self.px(34),
                    top: view.top + self.px(9),
                    right: view.right,
                    bottom: view.top + self.px(27),
                };
                let fill = if *on { palette.progress_fill } else { palette.tab_inactive };
                smooth::fill_round_rect(dc, track, self.px(18), fill);
                let knob = self.px(14);
                let x = if *on { track.right - knob - self.px(2) } else { track.left + self.px(2) };
                let knob_rect = RECT { left: x, top: track.top + self.px(2), right: x + knob, bottom: track.top + self.px(2) + knob };
                smooth::fill_ellipse(dc, knob_rect, Rgba::hex(0xFFFFFF, 0xFF));
            },
            Item::AppRow { title, detail, selected, .. } => unsafe {
                if *selected {
                    smooth::fill_round_rect(dc, view, self.px(10), palette.tab_active);
                }
                let top = RECT { left: view.left + self.px(8), top: view.top + self.px(2), right: view.right - self.px(8), bottom: view.top + self.px(19) };
                w::draw_text(dc, top, title, text, false, true);
                let bottom = RECT { top: view.top + self.px(18), ..top };
                w::draw_text(dc, bottom, detail, muted, false, true);
            },
            Item::Button { label, tone, enabled } => unsafe {
                let (fill, label_colour) = match (tone, enabled) {
                    (_, false) => (palette.tab_inactive, muted),
                    (Tone::Ok, true) => (palette.progress_fill, Rgba::hex(0xFFFFFF, 0xFF)),
                    (Tone::Problem, true) => (palette.tab_inactive, w::danger(palette)),
                    _ => (palette.tab_inactive, text),
                };
                smooth::fill_round_rect(dc, view, self.px(16), fill);
                w::draw_text(dc, view, label, label_colour, true, true);
            },
            Item::Field(field) => unsafe {
                let _ = field;
                smooth::stroke_round_rect(dc, view, self.px(8), palette.hairline, 1);
            },
            Item::WorkspaceRow { name, colour, active, tabs, id, icon } => unsafe {
                let dot = RECT { left: view.left + self.px(4), top: view.top + self.px(9), right: view.left + self.px(16), bottom: view.top + self.px(21) };
                let swatch = colour.map_or(palette.hairline, colour_rgba);
                smooth::fill_ellipse(dc, dot, swatch);
                // The glyph is the same one the rail draws for this workspace. The active one takes the accent.
                let glyph = RECT { left: view.left + self.px(20), top: view.top + self.px(4), right: view.left + self.px(36), bottom: view.top + self.px(20) };
                self.draw_glyph(dc, glyph, *icon, if *active { palette.progress_fill } else { text });
                let name_rect = RECT { left: view.left + self.px(42), top: view.top, right: view.right - self.px(96), bottom: view.top + self.px(22) };
                // While renaming, the inline EDIT covers the name, so nothing is drawn under it.
                if self.model.renaming != Some(*id) {
                    w::draw_text(dc, name_rect, name, text, false, true);
                }
                let note = match (*active, *tabs) {
                    (true, n) => format!("Current \u{00B7} {n} tab{}", if n == 1 { "" } else { "s" }),
                    (false, n) => format!("{n} tab{}", if n == 1 { "" } else { "s" }),
                };
                let note_rect = RECT { left: view.right - self.px(96), top: view.top, right: view.right, bottom: view.top + self.px(22) };
                w::draw_text(dc, note_rect, &note, muted, true, true);
            },
            Item::IconChoice { icon, selected } => unsafe {
                smooth::fill_round_rect(dc, view, self.px(8), if *selected { palette.tab_active } else { palette.tab_inactive });
                if *selected {
                    smooth::stroke_round_rect(dc, view, self.px(8), palette.progress_fill, self.px(2));
                }
                let glyph = self.centred_square(view, self.px(16));
                self.draw_glyph(dc, glyph, *icon, if *selected { palette.progress_fill } else { text });
            },
            Item::ColourChoice { colour, selected } => unsafe {
                smooth::fill_round_rect(dc, view, self.px(8), palette.tab_inactive);
                let dot = self.centred_square(view, self.px(14));
                match colour {
                    Some(colour) => smooth::fill_ellipse(dc, dot, colour_rgba(*colour)),
                    // No colour is a hollow ring, so it reads as "none" next to the filled swatches.
                    None => smooth::stroke_ellipse(dc, dot, muted, self.px(1).max(1)),
                }
                if *selected {
                    smooth::stroke_round_rect(dc, view, self.px(8), palette.progress_fill, self.px(2));
                }
            },
            Item::Empty(label) => unsafe {
                w::draw_text(dc, view, label, muted, false, true);
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidebar::{SidebarConfig, SidebarKind};

    fn app(id: u32, name: &str, url: &str, position: usize) -> WebApp {
        WebApp {
            id,
            name: name.into(),
            url: url.into(),
            favicon_file: None,
            position,
        }
    }

    fn model() -> Model {
        Model {
            visible: vec![
                (SidebarKind::Workspaces, true),
                (SidebarKind::Apps, false),
                (SidebarKind::Bookmarks, true),
                (SidebarKind::History, true),
            ],
            apps: vec![
                app(1, "Mail", "https://mail.example.com/", 0),
                app(2, "Docs", "https://docs.example.com/", 1),
            ],
            workspaces: vec![
                WorkspaceEntry {
                    id: 10,
                    name: "Work".into(),
                    icon: WorkspaceIcon::Briefcase,
                    colour: Some(WorkspaceColour::Blue),
                    active: true,
                    tabs: 3,
                },
                WorkspaceEntry {
                    id: 11,
                    name: "Home".into(),
                    icon: WorkspaceIcon::Home,
                    colour: None,
                    active: false,
                    tabs: 1,
                },
            ],
            ..Model::default()
        }
    }

    fn dims() -> Dims {
        Dims {
            width: 360,
            height: 500,
            pad: 12,
            gap: 6,
            line: 18,
            row: 36,
            button: 26,
            field: 30,
        }
    }

    #[test]
    fn toggles_flip_visibility_and_locked_sections_do_nothing() {
        let mut m = model();
        assert_eq!(
            m.press(Hit::Toggle(SidebarKind::Apps)),
            Action::SetVisible {
                kind: SidebarKind::Apps,
                visible: true
            }
        );
        assert_eq!(
            m.press(Hit::Toggle(SidebarKind::Bookmarks)),
            Action::SetVisible {
                kind: SidebarKind::Bookmarks,
                visible: false
            }
        );
        // Locked sections have no toggle hit, and pressing one directly still changes nothing.
        assert_eq!(m.press(Hit::Toggle(SidebarKind::Menu)), Action::Nothing);
        assert_eq!(m.press(Hit::Toggle(SidebarKind::Customize)), Action::Nothing);
    }

    #[test]
    fn toggle_rows_are_laid_out_with_locked_rows_unclickable() {
        let m = model();
        let placed = m.layout(dims());
        let toggles: Vec<_> = placed
            .iter()
            .filter_map(|p| match &p.item {
                Item::Toggle { kind, locked, .. } => Some((*kind, *locked, p.hit)),
                _ => None,
            })
            .collect();
        assert_eq!(toggles.len(), 6);
        assert_eq!(toggles[0], (SidebarKind::Customize, true, None));
        assert_eq!(toggles[5], (SidebarKind::Menu, true, None));
        assert_eq!(toggles[1].2, Some(Hit::Toggle(SidebarKind::Workspaces)));
    }

    #[test]
    fn form_validation_explains_each_problem() {
        let apps = model().apps;
        assert_eq!(form_status("", "", &apps, None).tone, Tone::Hint);
        assert_eq!(form_status("  ", "https://a.example/", &apps, None).tone, Tone::Problem);
        assert_eq!(form_status("A", "", &apps, None).tone, Tone::Hint);
        assert_eq!(
            form_status("A", "ftp://a.example/", &apps, None).text,
            "Enter a full https:// or http:// address."
        );
        assert_eq!(
            form_status("Mail again", "https://MAIL.example.com", &apps, None).text,
            "That address is already pinned."
        );
        assert_eq!(form_status("Mail", "https://mail.example.com", &apps, Some(1)).tone, Tone::Ok);
        let long = "x".repeat(129);
        assert_eq!(form_status(&long, "https://a.example/", &apps, None).tone, Tone::Problem);
    }

    #[test]
    fn valid_forms_submit_trimmed_values_and_clear_on_success() {
        let mut m = model();
        m.name = "  Notes  ".into();
        m.url = " https://notes.example.com ".into();
        assert_eq!(
            m.submit(),
            Action::AddApp {
                name: "Notes".into(),
                url: "https://notes.example.com/".into()
            }
        );
        assert!(m.name.is_empty() && m.url.is_empty());
    }

    #[test]
    fn invalid_forms_keep_the_text_for_correction() {
        let mut m = model();
        m.name = "Bad".into();
        m.url = "javascript:alert(1)".into();
        assert_eq!(m.submit(), Action::Redraw);
        assert_eq!(m.url, "javascript:alert(1)");
    }

    #[test]
    fn presets_fill_name_and_address_without_adding() {
        let mut m = model();
        assert_eq!(m.press(Hit::Preset(0)), Action::Redraw);
        assert_eq!(m.name, "Spotify");
        assert_eq!(m.url, "https://open.spotify.com/");
        assert_eq!(m.mode, Some(FormMode::Add));
        assert_eq!(m.apps.len(), 2);
        assert_eq!(m.press(Hit::Preset(99)), Action::Nothing);
        assert_eq!(PRESETS.len(), 4);
    }

    #[test]
    fn editing_an_app_switches_the_form_to_save_mode() {
        let mut m = model();
        assert_eq!(m.press(Hit::AppRename(2)), Action::Focus(Field::Name));
        assert_eq!(m.mode, Some(FormMode::Edit(2)));
        assert_eq!(m.name, "Docs");
        m.name = "Documents".into();
        assert_eq!(
            m.submit(),
            Action::UpdateApp {
                id: 2,
                name: "Documents".into(),
                url: "https://docs.example.com/".into()
            }
        );
        assert_eq!(m.mode, None);
    }

    #[test]
    fn escape_leaves_the_form_before_dismissing_the_panel() {
        let mut m = model();
        m.edit_app(1);
        assert_eq!(m.key(Key::Escape), Action::Redraw);
        assert_eq!(m.mode, None);
        assert_eq!(m.key(Key::Escape), Action::Dismiss);
    }

    #[test]
    fn up_and_down_move_the_app_selection_and_step_moves_reorder() {
        let mut m = model();
        assert_eq!(m.key(Key::Down), Action::Redraw);
        assert_eq!(m.selected_app, Some(1));
        m.key(Key::Down);
        assert_eq!(m.selected_app, Some(2));
        assert_eq!(m.key(Key::Down), Action::Redraw);
        assert_eq!(m.selected_app, Some(2));
        assert_eq!(m.press(Hit::AppUp(2)), Action::MoveApp { id: 2, index: 0 });
        assert_eq!(m.press(Hit::AppDown(2)), Action::Nothing);
        assert_eq!(m.key(Key::Delete), Action::RemoveApp { id: 2 });
    }

    #[test]
    fn step_index_stays_inside_the_list() {
        assert_eq!(step_index(3, 0, -1), None);
        assert_eq!(step_index(3, 0, 1), Some(1));
        assert_eq!(step_index(3, 2, 1), None);
        assert_eq!(step_index(0, 0, 1), None);
    }

    #[test]
    fn workspace_actions_carry_the_row_id() {
        let mut m = model();
        assert_eq!(m.press(Hit::WsDown(10)), Action::MoveWorkspace { id: 10, index: 1 });
        assert_eq!(m.press(Hit::WsUp(10)), Action::Nothing);
        assert_eq!(m.press(Hit::WsDelete(11)), Action::DeleteWorkspace { id: 11 });
        assert_eq!(m.press(Hit::WsRename(10)), Action::Focus(Field::WorkspaceName));
        assert_eq!(m.renaming, Some(10));
    }

    #[test]
    fn icon_and_colour_buttons_open_pickers_that_apply_and_close() {
        let mut m = model();
        assert_eq!(m.press(Hit::WsIcon(10)), Action::Redraw);
        assert_eq!(m.picker, Some(Picker::Icon(10)));
        // Pressing the same button again closes the picker without changing anything.
        assert_eq!(m.press(Hit::WsIcon(10)), Action::Redraw);
        assert_eq!(m.picker, None);
        m.press(Hit::WsColour(11));
        assert_eq!(m.picker, Some(Picker::Colour(11)));
        assert_eq!(
            m.press(Hit::WsSetColour(11, Some(WorkspaceColour::Teal))),
            Action::SetColour { id: 11, colour: Some(WorkspaceColour::Teal) }
        );
        assert_eq!(m.picker, None);
        m.press(Hit::WsIcon(10));
        assert_eq!(
            m.press(Hit::WsSetIcon(10, WorkspaceIcon::Star)),
            Action::SetIcon { id: 10, icon: WorkspaceIcon::Star }
        );
        assert_eq!(m.picker, None);
    }

    #[test]
    fn up_and_down_are_disabled_at_the_ends_of_the_workspace_list() {
        // The first workspace's Up and the last workspace's Down are the disabled ones.
        let buttons: Vec<(String, bool)> = model()
            .layout(dims())
            .into_iter()
            .filter_map(|p| match p.item {
                Item::Button { label, enabled, .. } => Some((label, enabled)),
                _ => None,
            })
            .collect();
        let state = |wanted: &str| -> Vec<bool> {
            buttons.iter().filter(|(l, _)| l == wanted).map(|(_, e)| *e).collect()
        };
        assert_eq!(state("Up"), vec![false, true]);
        assert_eq!(state("Down"), vec![true, false]);
    }

    #[test]
    fn icon_picker_lays_out_twelve_tiles_with_the_current_one_selected() {
        let mut m = model();
        m.picker = Some(Picker::Icon(11));
        let tiles: Vec<(WorkspaceIcon, bool)> = m
            .layout(dims())
            .into_iter()
            .filter_map(|p| match p.item {
                Item::IconChoice { icon, selected } => Some((icon, selected)),
                _ => None,
            })
            .collect();
        assert_eq!(tiles.len(), 12);
        assert_eq!(tiles.iter().filter(|(_, s)| *s).count(), 1);
        assert!(tiles.contains(&(WorkspaceIcon::Home, true)));
        // Only the open picker lays tiles out.
        m.picker = None;
        assert!(m.layout(dims()).iter().all(|p| !matches!(p.item, Item::IconChoice { .. })));
    }

    #[test]
    fn colour_picker_offers_none_then_the_palette() {
        let mut m = model();
        m.picker = Some(Picker::Colour(10));
        let hits: Vec<Option<Hit>> = m
            .layout(dims())
            .into_iter()
            .filter(|p| matches!(p.item, Item::ColourChoice { .. }))
            .map(|p| p.hit)
            .collect();
        assert_eq!(hits.len(), 9);
        assert_eq!(hits[0], Some(Hit::WsSetColour(10, None)));
        assert_eq!(hits[1], Some(Hit::WsSetColour(10, Some(COLOURS[0]))));
    }

    #[test]
    fn escape_closes_an_open_picker_before_the_panel() {
        let mut m = model();
        m.picker = Some(Picker::Icon(10));
        assert_eq!(m.key(Key::Escape), Action::Redraw);
        assert_eq!(m.picker, None);
        assert_eq!(m.key(Key::Escape), Action::Dismiss);
    }

    #[test]
    fn create_needs_a_name_and_trims_it() {
        let mut m = model();
        assert_eq!(m.press(Hit::WsCreate), Action::Nothing);
        m.new_workspace = "   ".into();
        assert_eq!(m.press(Hit::WsCreate), Action::Nothing);
        m.new_workspace = "  Research  ".into();
        assert_eq!(
            m.press(Hit::WsCreate),
            Action::CreateWorkspace { name: "Research".into() }
        );
    }

    #[test]
    fn profile_row_names_the_profile_and_asks_for_the_switcher() {
        let mut m = model();
        m.profile = "Default".into();
        assert_eq!(m.press(Hit::SwitchProfile), Action::SwitchProfile);
        let texts: Vec<String> = m
            .layout(dims())
            .into_iter()
            .filter_map(|p| match p.item {
                Item::Note(status) => Some(status.text),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|t| t == "Current profile: Default"));
        assert!(m.layout(dims()).iter().any(|p| p.hit == Some(Hit::SwitchProfile)));
    }

    #[test]
    fn clear_web_app_data_is_offered_only_when_an_app_is_pinned() {
        let has_clear = |m: &Model| {
            m.layout(dims())
                .iter()
                .any(|p| p.hit == Some(Hit::ClearWebAppData))
        };
        let mut m = model();
        assert!(has_clear(&m));
        m.apps.clear();
        assert!(!has_clear(&m));
    }

    #[test]
    fn profile_label_says_unknown_when_no_profile_is_active() {
        assert_eq!(profile_label(None), "Unknown");
    }

    #[test]
    fn hit_testing_finds_buttons_on_top_of_selected_rows() {
        let mut m = model();
        m.selected_app = Some(1);
        let d = dims();
        let placed = m.layout(d);
        let remove = placed
            .iter()
            .find(|p| p.hit == Some(Hit::AppRemove(1)))
            .expect("selected app has a remove button");
        let (x, y) = (remove.rect.left + 2, remove.rect.top + 2);
        assert_eq!(m.hit_at(d, x, y), Some(Hit::AppRemove(1)));
        // Clicking the app's row text selects it.
        let row = placed
            .iter()
            .find(|p| p.hit == Some(Hit::AppSelect(2)))
            .unwrap();
        assert_eq!(m.hit_at(d, row.rect.left + 1, row.rect.top + 1), Some(Hit::AppSelect(2)));
    }

    #[test]
    fn layout_grows_with_content_and_scroll_is_clamped() {
        let mut m = model();
        let d = dims();
        let short = m.content_height(d);
        m.selected_app = Some(1);
        let expanded = m.content_height(d);
        assert!(expanded > short);
        m.scroll_by(10_000, d);
        assert!(m.scroll <= expanded);
        m.scroll_by(-10_000, d);
        assert_eq!(m.scroll, 0);
    }

    #[test]
    fn save_button_is_disabled_until_the_form_is_valid() {
        let mut m = model();
        let d = dims();
        let submit = |m: &Model| {
            m.layout(d)
                .into_iter()
                .find(|p| matches!(&p.item, Item::Button { label, .. } if label.as_str() == "Add web app"))
                .unwrap()
        };
        assert_eq!(submit(&m).hit, None);
        m.name = "Notes".into();
        m.url = "https://notes.example.com/".into();
        assert_eq!(submit(&m).hit, Some(Hit::FormSubmit));
    }

    #[test]
    fn icon_status_reports_whether_the_favicon_is_saved() {
        let mut a = app(1, "Mail", "https://mail.example.com/", 0);
        assert_eq!(icon_status(&a), "Icon not fetched yet");
        a.favicon_file = Some("abc.png".into());
        assert_eq!(icon_status(&a), "Icon saved");
    }

    #[test]
    fn labels_cover_every_icon_colour_and_section() {
        assert_eq!(ICONS.len(), 12);
        assert_eq!(COLOURS.len(), 8);
        assert_eq!(icon_label(WorkspaceIcon::Cart), "Shopping");
        assert_eq!(colour_label(WorkspaceColour::Slate), "Slate");
        assert_eq!(section_label(SidebarKind::Apps), "Web apps");
        assert!(is_locked(SidebarKind::Menu));
        assert!(!is_locked(SidebarKind::History));
    }

    #[test]
    fn workspace_rows_are_built_from_a_real_set() {
        let set = crate::workspaces::WorkspaceSet::new("Main", crate::tabs::TabId(1));
        let rows = workspace_entries(set.all(), set.active_id().0);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].active);
        assert_eq!(rows[0].tabs, 1);
        assert_eq!(rows[0].name, "Main");
    }

    #[test]
    fn default_rail_has_every_section_in_toggle_list() {
        let config = SidebarConfig::default();
        let kinds: Vec<SidebarKind> = config.items.iter().map(|i| i.kind).collect();
        for kind in TOGGLEABLE {
            assert!(kinds.contains(&kind));
        }
    }
}
