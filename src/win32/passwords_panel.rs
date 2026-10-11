//! The Passwords & Passkeys panel: a native, owned window that drives `passwords_import::Flow`.
//!
//! The panel is laid out once per paint and per pointer event from the same `build` function, so
//! what is drawn and what responds to a click cannot drift apart. Only counts and fixed messages are
//! ever shown; adapter payloads never reach a window.

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    ffi::c_void,
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

use windows::{
    Win32::{
        Foundation::{COLORREF, HGLOBAL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateFontIndirectW, CreateSolidBrush, DRAW_TEXT_FORMAT,
            DT_CENTER, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteObject,
            DrawTextW, EndPaint, FillRect, FrameRect, GetObjectW, HDC, HFONT,
            InvalidateRect, LOGFONTW, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor,
            TRANSPARENT,
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
            Shell::{
                FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog,
                SIGDN_FILESYSPATH,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
                GetClientRect, GetWindowLongPtrW, GetWindowRect, IDC_ARROW, IsWindow, LoadCursorW,
                GetParent,
                RegisterClassW, SW_SHOW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
                WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE,
                WM_NCDESTROY, WM_PAINT, WM_CHAR, WM_TIMER, WNDCLASSW, WS_POPUP, KillTimer,
                SetTimer,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    passwords::{self, CredentialId, CredentialSummary, ImportPlan, PasswordError, PasswordStore},
    theme::{self, Rgba},
    win32::{
        glass, smooth,
        passwords_import::{self, Counts, Flow, ImportError, Outcome, Profile, Stage},
    },
};

const CLASS_NAME: PCWSTR = w!("BowserPasswordsPanel");
const WM_MOUSELEAVE: u32 = 0x02A3;
const TITLE: &str = "Passwords & Passkeys";
/// Logical (96 DPI) client size. Every other measurement is scaled from these values.
const CLIENT_WIDTH: i32 = 520;
const CLIENT_HEIGHT: i32 = 600;
const PAD: i32 = 24;
const LINE: i32 = 24;
const ROW: i32 = 40;
const BUTTON_HEIGHT: i32 = 36;
const RADIUS: i32 = 10;
const NOTICE: &str = "Passkey use is unverified in Bowser. Manage Windows Hello passkeys in Windows Settings; Bowser cannot import or export them.";
const INTRO: &str = "DPAPI protects saved passwords at rest; reveal has no Windows re-authentication.";
const REMINDER: &str = "CSV exports contain plaintext passwords. Bowser reads the selected file locally and never deletes it.";
pub(crate) const CLIPBOARD_CLEAR_TIMER: usize = 31;
const REVEAL_CLEAR_TIMER: usize = 32;

thread_local! {
    /// The open panel, so reopening the shortcut focuses it rather than stacking another.
    static OPEN: Cell<isize> = const { Cell::new(0) };
    static CLASS_REGISTERED: Cell<bool> = const { Cell::new(false) };
    static CLIPBOARD_GUARD: RefCell<Option<Zeroizing<Vec<u16>>>> = const { RefCell::new(None) };
    static SAVE_PROMPT_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static SAVE_PROMPT_DECLINED_ORIGINS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

struct SavePromptGuard;
impl Drop for SavePromptGuard {
    fn drop(&mut self) {
        SAVE_PROMPT_ACTIVE.with(|active| active.set(false));
    }
}

struct Panel {
    flow: Flow,
    /// Parsed credentials awaiting confirmation. Held here, not in `Flow`, so counts stay plain data.
    plan: Option<ImportPlan>,
    hover: Target,
    pressed: Target,
    view: PanelView,
    entries: Vec<CredentialSummary>,
    selected: Option<CredentialId>,
    list_offset: usize,
    search: String,
    search_focused: bool,
    editor: Option<CredentialDraft>,
    revealed: Option<(CredentialId, Zeroizing<String>)>,
    notice: Option<&'static str>,
}

impl Panel {
    fn refresh_entries(&mut self) {
        match PasswordStore::open_default() {
            Ok(vault) => {
                self.entries = vault.search(&self.search);
                if self.selected.is_some_and(|id| !self.entries.iter().any(|entry| entry.id == id)) {
                    self.selected = None;
                    self.revealed = None;
                }
            }
            Err(_) => {
                self.entries.clear();
                self.selected = None;
                self.revealed = None;
                self.notice = Some("The profile password vault could not be opened.");
            }
        }
    }

    fn selected_entry(&self) -> Option<&CredentialSummary> {
        self.entries
            .iter()
            .find(|entry| Some(entry.id) == self.selected)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelView {
    Vault,
    Import,
    Edit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DraftField {
    Origin,
    Username,
    Password,
}

struct CredentialDraft {
    id: Option<CredentialId>,
    origin: String,
    username: String,
    password: Zeroizing<String>,
    visible: bool,
    focus: DraftField,
    pending_surrogate: Option<u16>,
}

impl CredentialDraft {
    fn new(id: Option<CredentialId>, origin: String, username: String, password: String) -> Self {
        Self {
            id,
            origin,
            username,
            password: Zeroizing::new(password),
            visible: false,
            focus: DraftField::Origin,
            pending_surrogate: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    None,
    Provider(usize),
    ChooseFile,
    Back,
    Import,
    OpenImport,
    Search,
    Focus(DraftField),
    Select(CredentialId),
    Add,
    Edit,
    Delete,
    Reveal,
    Copy,
    Generate,
    Save,
    Cancel,
    TogglePassword,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Heading,
    Body,
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Provider {
        selected: bool,
    },
    Secondary,
    Primary,
    /// A square glyph button; the label is the accessible name and is not drawn.
    Icon,
    Field,
}

struct Control {
    target: Target,
    rect: RECT,
    label: String,
    role: Role,
    enabled: bool,
}

struct TextRun {
    rect: RECT,
    text: String,
    style: Style,
}

impl Drop for TextRun {
    fn drop(&mut self) {
        self.text.zeroize();
    }
}

#[derive(Default)]
struct Scene {
    texts: Vec<TextRun>,
    cards: Vec<RECT>,
    controls: Vec<Control>,
}

impl Scene {
    fn text(&mut self, rect: RECT, text: impl Into<String>, style: Style) {
        self.texts.push(TextRun {
            rect,
            text: text.into(),
            style,
        });
    }

    fn control(
        &mut self,
        target: Target,
        rect: RECT,
        label: impl Into<String>,
        role: Role,
        enabled: bool,
    ) {
        self.controls.push(Control {
            target,
            rect,
            label: label.into(),
            role,
            enabled,
        });
    }

    fn target_at(&self, x: i32, y: i32) -> Target {
        self.controls
            .iter()
            .find(|c| contains(c.rect, x, y))
            .map_or(Target::None, |c| c.target)
    }

    fn enabled(&self, target: Target) -> bool {
        self.controls
            .iter()
            .any(|c| c.target == target && c.enabled)
    }
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

fn contains(r: RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// Builds the panel for the current flow. Painting and hit-testing both read this one result.
fn build(flow: &Flow, width: i32, height: i32, dpi: u32) -> Scene {
    let s = |v: i32| (v as i64 * dpi as i64 / 96) as i32;
    let (x0, x1) = (s(PAD), width - s(PAD));
    let mut scene = Scene::default();
    let mut y = s(PAD);

    scene.text(rect(x0, y, x1 - s(44), y + s(32)), TITLE, Style::Heading);
    scene.control(
        Target::Close,
        rect(x1 - s(32), y, x1, y + s(32)),
        "Close",
        Role::Icon,
        true,
    );
    y += s(44);
    scene.text(rect(x0, y, x1, y + s(40)), INTRO, Style::Muted);
    y += s(48);
    scene.cards.push(rect(x0, y, x1, y + s(64)));
    scene.text(
        rect(x0 + s(14), y + s(10), x1 - s(14), y + s(54)),
        NOTICE,
        Style::Body,
    );
    y += s(76);

    match &flow.stage {
        Stage::Choose => {
            scene.text(rect(x0, y, x1, y + s(LINE)), "Import from", Style::Muted);
            y += s(LINE + 4);
            if flow.profiles.is_empty() {
                scene.text(
                    rect(x0, y, x1, y + s(44)),
                    "No import sources are available yet. Bowser only offers sources it can read on this device.",
                    Style::Body,
                );
                y += s(48);
            }
            for (index, profile) in flow.profiles.iter().enumerate() {
                scene.control(
                    Target::Provider(index),
                    rect(x0, y, x1, y + s(ROW)),
                    profile.label,
                    Role::Provider {
                        selected: index == flow.selected,
                    },
                    true,
                );
                y += s(ROW + 8);
            }
            if let Some(error) = flow.error {
                scene.text(
                    rect(x0, y, x1, y + s(44)),
                    error_message(error),
                    Style::Body,
                );
            }
        }
        Stage::Preview { counts } => {
            let source = flow.selected_profile().map_or("", |p| p.label);
            let file = flow
                .file
                .as_deref()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .unwrap_or("");
            scene.text(
                rect(x0, y, x1, y + s(LINE)),
                format!("Source: {source}"),
                Style::Body,
            );
            y += s(LINE);
            scene.text(
                rect(x0, y, x1, y + s(LINE)),
                format!("File: {file}"),
                Style::Muted,
            );
            y += s(LINE + 12);
            for (label, value) in count_rows(counts) {
                scene.text(rect(x0, y, x1 - s(120), y + s(LINE)), label, Style::Body);
                scene.text(
                    rect(x1 - s(120), y, x1, y + s(LINE)),
                    value.to_string(),
                    Style::Body,
                );
                y += s(LINE);
            }
            y += s(12);
            scene.text(
                rect(x0, y, x1, y + s(44)),
                "Nothing has been imported yet. Confirm to write these passwords to this profile.",
                Style::Muted,
            );
        }
        Stage::Done { outcome } => {
            for (label, value) in outcome_rows(outcome) {
                scene.text(rect(x0, y, x1 - s(120), y + s(LINE)), label, Style::Body);
                scene.text(
                    rect(x1 - s(120), y, x1, y + s(LINE)),
                    value.to_string(),
                    Style::Body,
                );
                y += s(LINE);
            }
            y += s(12);
            scene.cards.push(rect(x0, y, x1, y + s(76)));
            scene.text(
                rect(x0 + s(14), y + s(10), x1 - s(14), y + s(68)),
                REMINDER,
                Style::Body,
            );
        }
    }

    let by = height - s(PAD) - s(BUTTON_HEIGHT);
    match flow.stage {
        Stage::Choose => {
            scene.control(
                Target::Back,
                rect(x0, by, x0 + s(96), by + s(BUTTON_HEIGHT)),
                "Passwords",
                Role::Secondary,
                true,
            );
            scene.control(
                Target::ChooseFile,
                rect(x1 - s(180), by, x1, by + s(BUTTON_HEIGHT)),
                "Choose file…",
                Role::Primary,
                flow.can_choose_file(),
            );
        }
        Stage::Preview { counts } => {
            scene.control(
                Target::Back,
                rect(x0, by, x0 + s(96), by + s(BUTTON_HEIGHT)),
                "Back",
                Role::Secondary,
                true,
            );
            let label = format!(
                "Import {} {}",
                counts.imported,
                if counts.imported == 1 {
                    "password"
                } else {
                    "passwords"
                }
            );
            scene.control(
                Target::Import,
                rect(x1 - s(200), by, x1, by + s(BUTTON_HEIGHT)),
                label,
                Role::Primary,
                flow.can_import(),
            );
        }
        Stage::Done { .. } => scene.control(
            Target::Back,
            rect(x1 - s(180), by, x1, by + s(BUTTON_HEIGHT)),
            "Passwords",
            Role::Primary,
            true,
        ),
    }
    scene
}

fn build_panel(panel: &Panel, width: i32, height: i32, dpi: u32) -> Scene {
    match panel.view {
        PanelView::Vault => build_vault(panel, width, height, dpi),
        PanelView::Import => build(&panel.flow, width, height, dpi),
        PanelView::Edit => build_editor(panel, width, height, dpi),
    }
}

fn list_window<T>(entries: &[T], offset: usize, count: usize) -> &[T] {
    let start = offset.min(entries.len());
    let end = start.saturating_add(count).min(entries.len());
    &entries[start..end]
}

fn build_vault(panel: &Panel, width: i32, height: i32, dpi: u32) -> Scene {
    let s = |value: i32| (value as i64 * dpi as i64 / 96) as i32;
    let x0 = s(PAD);
    let x1 = width - s(PAD);
    let mut scene = Scene::default();
    let mut y = s(PAD);
    scene.text(rect(x0, y, x1 - s(44), y + s(32)), TITLE, Style::Heading);
    scene.control(
        Target::Close,
        rect(x1 - s(32), y, x1, y + s(32)),
        "Close",
        Role::Icon,
        true,
    );
    y += s(42);
    scene.text(rect(x0, y, x1, y + s(32)), INTRO, Style::Muted);
    y += s(40);
    scene.cards.push(rect(x0, y, x1, y + s(54)));
    scene.text(
        rect(x0 + s(12), y + s(7), x1 - s(12), y + s(47)),
        NOTICE,
        Style::Body,
    );
    y += s(66);
    scene.control(
        Target::Search,
        rect(x0, y, x1, y + s(38)),
        if panel.search.is_empty() {
            "Search by site or username".to_owned()
        } else {
            panel.search.clone()
        },
        Role::Field,
        true,
    );
    y += s(48);
    if panel.entries.is_empty() {
        scene.text(
            rect(x0, y, x1, y + s(50)),
            if panel.search.is_empty() {
                "No saved passwords yet. Add one or import a CSV export."
            } else {
                "No saved passwords match this search."
            },
            Style::Muted,
        );
    } else {
        for entry in list_window(&panel.entries, panel.list_offset, 5) {
            let label = format!("{}   ·   {}", entry.host, entry.username);
            let selected = panel.selected == Some(entry.id);
            scene.control(
                Target::Select(entry.id),
                rect(x0, y, x1, y + s(38)),
                label,
                Role::Provider { selected },
                true,
            );
            y += s(44);
        }
        if panel.entries.len() > 5 {
            scene.text(
                rect(x0, y, x1, y + s(24)),
                format!("{} more entries; refine the search to find one.", panel.entries.len() - 5),
                Style::Muted,
            );
        }
    }
    if let Some((id, password)) = &panel.revealed
        && panel.selected == Some(*id)
    {
        let secret = password.as_str().to_owned();
        scene.cards.push(rect(x0, height - s(160), x1, height - s(132)));
        scene.text(
            rect(x0 + s(12), height - s(157), x1 - s(12), height - s(134)),
            secret,
            Style::Body,
        );
    }
    let second_row = height - s(PAD) - s(BUTTON_HEIGHT) * 2 - s(8);
    let action_row = height - s(PAD) - s(BUTTON_HEIGHT);
    scene.control(
        Target::Add,
        rect(x0, second_row, x0 + s(104), second_row + s(BUTTON_HEIGHT)),
        "Add password",
        Role::Primary,
        true,
    );
    scene.control(
        Target::OpenImport,
        rect(x0 + s(112), second_row, x0 + s(232), second_row + s(BUTTON_HEIGHT)),
        "Import CSV",
        Role::Secondary,
        true,
    );
    let action_width = (x1 - x0 - s(24)) / 4;
    let has_selection = panel.selected.is_some();
    for (index, (target, label)) in [
        (Target::Reveal, if panel.revealed.as_ref().is_some_and(|(id, _)| Some(*id) == panel.selected) { "Hide" } else { "Reveal" }),
        (Target::Copy, "Copy"),
        (Target::Edit, "Edit"),
        (Target::Delete, "Delete"),
    ]
    .into_iter()
    .enumerate()
    {
        let left = x0 + index as i32 * (action_width + s(8));
        scene.control(
            target,
            rect(left, action_row, left + action_width, action_row + s(BUTTON_HEIGHT)),
            label,
            if target == Target::Delete { Role::Secondary } else { Role::Secondary },
            has_selection,
        );
    }
    if let Some(notice) = panel.notice {
        scene.text(
            rect(x0, second_row - s(28), x1, second_row - s(4)),
            notice,
            Style::Muted,
        );
    }
    scene
}

fn build_editor(panel: &Panel, width: i32, height: i32, dpi: u32) -> Scene {
    let s = |value: i32| (value as i64 * dpi as i64 / 96) as i32;
    let x0 = s(PAD);
    let x1 = width - s(PAD);
    let mut scene = Scene::default();
    let Some(draft) = panel.editor.as_ref() else {
        return scene;
    };
    scene.text(
        rect(x0, s(PAD), x1 - s(44), s(PAD + 32)),
        if draft.id.is_some() { "Edit password" } else { "Add password" },
        Style::Heading,
    );
    scene.control(
        Target::Close,
        rect(x1 - s(32), s(PAD), x1, s(PAD + 32)),
        "Close",
        Role::Icon,
        true,
    );
    scene.text(
        rect(x0, s(68), x1, s(100)),
        "Use an HTTPS site address. HTTP is accepted only for localhost.",
        Style::Muted,
    );
    let fields = [
        (DraftField::Origin, "Site", draft.origin.clone()),
        (DraftField::Username, "Username", draft.username.clone()),
        (
            DraftField::Password,
            "Password",
            if draft.visible {
                draft.password.as_str().to_owned()
            } else {
                "•".repeat(draft.password.chars().count())
            },
        ),
    ];
    for (index, (field, label, value)) in fields.into_iter().enumerate() {
        let top = s(124 + index as i32 * 88);
        scene.text(rect(x0, top, x1, top + s(20)), label, Style::Muted);
        let text = if value.is_empty() {
            match field {
                DraftField::Origin => "https://example.com",
                DraftField::Username => "Username",
                DraftField::Password => "Password",
            }
            .to_owned()
        } else {
            value
        };
        scene.control(
            match field {
                _ => Target::Focus(field),
            },
            rect(x0, top + s(22), x1, top + s(58)),
            text,
            Role::Field,
            true,
        );
        if draft.focus == field {
            scene.cards.push(rect(x0 - s(1), top + s(21), x1 + s(1), top + s(59)));
        }
    }
    scene.control(
        Target::TogglePassword,
        rect(x0, s(400), x0 + s(104), s(400 + BUTTON_HEIGHT)),
        if draft.visible { "Hide" } else { "Show" },
        Role::Secondary,
        true,
    );
    scene.control(
        Target::Generate,
        rect(x0 + s(112), s(400), x0 + s(240), s(400 + BUTTON_HEIGHT)),
        "Generate",
        Role::Secondary,
        true,
    );
    scene.text(
        rect(x0, s(456), x1, s(490)),
        "Select a field and type. Passwords are held in zeroizing memory buffers.",
        Style::Muted,
    );
    if let Some(notice) = panel.notice {
        scene.text(rect(x0, s(496), x1, s(520)), notice, Style::Muted);
    }
    let by = height - s(PAD) - s(BUTTON_HEIGHT);
    scene.control(
        Target::Cancel,
        rect(x0, by, x0 + s(100), by + s(BUTTON_HEIGHT)),
        "Cancel",
        Role::Secondary,
        true,
    );
    scene.control(
        Target::Save,
        rect(x1 - s(112), by, x1, by + s(BUTTON_HEIGHT)),
        "Save",
        Role::Primary,
        true,
    );
    scene
}

fn count_rows(counts: &Counts) -> [(&'static str, usize); 4] {
    [
        ("To import", counts.imported),
        ("Duplicates", counts.duplicates),
        ("Conflicts", counts.conflicts),
        ("Unreadable entries", counts.errors),
    ]
}

fn outcome_rows(outcome: &Outcome) -> [(&'static str, usize); 3] {
    [
        ("Imported", outcome.succeeded),
        ("Skipped", outcome.skipped),
        ("Failed", outcome.failed),
    ]
}

/// Fixed wording only. Adapter error payloads are never displayed.
fn error_message(error: ImportError) -> &'static str {
    match error {
        ImportError::Unreadable => {
            "That file couldn't be read as this source. Nothing was imported."
        }
        ImportError::VaultUnavailable => {
            "Bowser's password vault couldn't be opened or saved. Nothing was imported."
        }
    }
}

/// Linear blend of two opaque palette colours, for hover and pressed fills.
fn mix_toward(from: Rgba, to: Rgba, t: f32) -> Rgba {
    let lerp = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Rgba {
        r: lerp(from.r, to.r),
        g: lerp(from.g, to.g),
        b: lerp(from.b, to.b),
        a: 0xFF,
    }
}

/// The body font made larger and semibold, for the panel title.
unsafe fn create_heading_font(body: HFONT) -> HFONT {
    unsafe {
        let mut logfont = LOGFONTW::default();
        GetObjectW(
            body.into(),
            size_of::<LOGFONTW>() as i32,
            Some((&mut logfont as *mut LOGFONTW).cast()),
        );
        logfont.lfHeight = logfont.lfHeight * 13 / 10;
        logfont.lfWeight = 600;
        CreateFontIndirectW(&logfont)
    }
}

fn colorref(colour: Rgba) -> COLORREF {
    COLORREF(u32::from(colour.r) | (u32::from(colour.g) << 8) | (u32::from(colour.b) << 16))
}

/// Opaque palette for the panel. The glass palettes are translucent and only suit the chrome overlay.
fn palette() -> &'static theme::Palette {
    theme::palette(glass::appearance(), true)
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// Called by the main window's one-shot timer, so the clipboard guard survives closing this panel.
pub(crate) fn clear_password_clipboard_timer(owner: HWND) {
    let _ = unsafe { KillTimer(Some(owner), CLIPBOARD_CLEAR_TIMER) };
    let baseline = CLIPBOARD_GUARD.with(|guard| guard.borrow_mut().take());
    let Some(baseline) = baseline else {
        return;
    };
    if clipboard_unchanged_and_clear(owner, &baseline) == ClipboardClear::Retry {
        CLIPBOARD_GUARD.with(|guard| *guard.borrow_mut() = Some(baseline));
        let _ = unsafe { SetTimer(Some(owner), CLIPBOARD_CLEAR_TIMER, 1_000, None) };
    }
}

fn copy_password(hwnd: HWND, password: &str) -> bool {
    if password.contains('\0') {
        return false;
    }
    let owner = unsafe { GetParent(hwnd) }.unwrap_or(hwnd);
    if !crate::win32::window::copy_text_to_clipboard(owner, password) {
        return false;
    }
    let baseline = Zeroizing::new(password.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
    CLIPBOARD_GUARD.with(|guard| *guard.borrow_mut() = Some(baseline));
    let _ = unsafe { KillTimer(Some(owner), CLIPBOARD_CLEAR_TIMER) };
    if unsafe { SetTimer(Some(owner), CLIPBOARD_CLEAR_TIMER, 30_000, None) } == 0 {
        clear_password_clipboard_timer(owner);
        return false;
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipboardClear {
    Cleared,
    Changed,
    Retry,
}

fn clipboard_unchanged_and_clear(owner: HWND, baseline: &[u16]) -> ClipboardClear {
    use windows::Win32::{
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard},
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        },
    };
    unsafe {
        if OpenClipboard(Some(owner)).is_err() {
            return ClipboardClear::Retry;
        }
        let result = (|| {
            let Ok(handle) = GetClipboardData(13) else {
                return ClipboardClear::Changed;
            };
            let memory = HGLOBAL(handle.0);
            if GlobalSize(memory) < baseline.len() * std::mem::size_of::<u16>() {
                return ClipboardClear::Changed;
            }
            let data = GlobalLock(memory).cast::<u16>();
            if data.is_null() {
                return ClipboardClear::Retry;
            }
            let unchanged = baseline
                .iter()
                .enumerate()
                .all(|(index, value)| *data.add(index) == *value);
            let _ = GlobalUnlock(memory);
            if unchanged {
                if EmptyClipboard().is_ok() {
                    ClipboardClear::Cleared
                } else {
                    ClipboardClear::Retry
                }
            } else {
                ClipboardClear::Changed
            }
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Rounded control with an antialiased fill and a one-pixel border.
unsafe fn round_rect(dc: HDC, bounds: RECT, fill: Rgba, border: Rgba) {
    unsafe {
        smooth::fill_round_rect(dc, bounds, RADIUS, fill);
        smooth::stroke_round_rect(dc, bounds, RADIUS, border, 1);
    }
}

unsafe fn draw_text(dc: HDC, bounds: RECT, text: &str, colour: Rgba, format: DRAW_TEXT_FORMAT) {
    unsafe {
        let mut wide = Zeroizing::new(to_wide(text));
        let mut bounds = bounds;
        SetTextColor(dc, colorref(colour));
        DrawTextW(dc, &mut wide, &mut bounds, format);
    }
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let dpi = dpi_of(hwnd);
        let palette = palette();
        let brush = CreateSolidBrush(colorref(palette.window));
        FillRect(dc, &client, brush);
        let _ = DeleteObject(brush.into());
        let edge = CreateSolidBrush(colorref(palette.omnibox_border));
        FrameRect(dc, &client, edge);
        let _ = DeleteObject(edge.into());

        if let Some(panel) = state(hwnd) {
            let font = crate::chrome::paint::create_message_font(
                dpi,
                windows::Win32::Graphics::Gdi::CLEARTYPE_QUALITY,
            );
            let heading_font = create_heading_font(font);
            let old_font = SelectObject(dc, font.into());
            SetBkMode(dc, TRANSPARENT);
            let scene = build_panel(panel, client.right, client.bottom, dpi);
            for card in &scene.cards {
                round_rect(dc, *card, palette.omnibox_fill, palette.omnibox_border);
            }
            for text in &scene.texts {
                let colour = match text.style {
                    Style::Heading => palette.tab_text_active,
                    Style::Body => palette.omnibox_text,
                    Style::Muted => palette.omnibox_placeholder,
                };
                if text.style == Style::Heading {
                    SelectObject(dc, heading_font.into());
                    draw_text(
                        dc,
                        text.rect,
                        &text.text,
                        colour,
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                    );
                    SelectObject(dc, font.into());
                    continue;
                }
                draw_text(
                    dc,
                    text.rect,
                    &text.text,
                    colour,
                    DT_LEFT | DT_WORDBREAK | DT_NOPREFIX,
                );
            }
            for control in &scene.controls {
                paint_control(dc, control, panel.hover, panel.pressed, palette);
            }
            SelectObject(dc, old_font);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(heading_font.into());
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe fn paint_control(
    dc: HDC,
    control: &Control,
    hover: Target,
    pressed: Target,
    palette: &theme::Palette,
) {
    unsafe {
        let hovered = control.enabled && control.target == hover;
        let held = hovered && control.target == pressed;
        let bounds = control.rect;
        match control.role {
            Role::Provider { selected } => {
                let (fill, border) = if selected {
                    (palette.tab_active, palette.omnibox_focus_border)
                } else if held {
                    (palette.tab_active, palette.omnibox_border)
                } else if hovered {
                    (palette.tab_hover, palette.omnibox_border)
                } else {
                    (palette.omnibox_fill, palette.omnibox_border)
                };
                round_rect(dc, bounds, fill, border);
                // A filled dot marks the chosen source, so the choice does not rest on colour alone.
                let centre_y = (bounds.top + bounds.bottom) / 2;
                let dot = rect(
                    bounds.left + 14,
                    centre_y - 5,
                    bounds.left + 24,
                    centre_y + 5,
                );
                let dot_fill = if selected {
                    palette.omnibox_focus_border
                } else {
                    palette.omnibox_fill
                };
                smooth::fill_ellipse(dc, dot, dot_fill);
                smooth::stroke_ellipse(dc, dot, palette.omnibox_focus_border, 1);
                let text_colour = if selected {
                    palette.tab_text_active
                } else {
                    palette.tab_text_inactive
                };
                let text_bounds = rect(
                    bounds.left + 34,
                    bounds.top,
                    bounds.right - 8,
                    bounds.bottom,
                );
                draw_text(
                    dc,
                    text_bounds,
                    &control.label,
                    text_colour,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            }
            Role::Icon => {
                if hovered {
                    round_rect(
                        dc,
                        bounds,
                        if held {
                            palette.tab_active
                        } else {
                            palette.tab_hover
                        },
                        if held {
                            palette.tab_active
                        } else {
                            palette.tab_hover
                        },
                    );
                }
                let (cx, cy) = (
                    (bounds.left + bounds.right) / 2,
                    (bounds.top + bounds.bottom) / 2,
                );
                let arm = (bounds.right - bounds.left) / 6;
                let stroke = ((bounds.right - bounds.left) / 20).max(1);
                let glyph = if hovered {
                    palette.tab_text_active
                } else {
                    palette.tab_text_inactive
                };
                smooth::stroke_line(
                    dc,
                    POINT { x: cx - arm, y: cy - arm },
                    POINT { x: cx + arm + 1, y: cy + arm + 1 },
                    glyph,
                    stroke,
                );
                smooth::stroke_line(
                    dc,
                    POINT { x: cx - arm, y: cy + arm },
                    POINT { x: cx + arm + 1, y: cy - arm - 1 },
                    glyph,
                    stroke,
                );
            }
            Role::Field => {
                let border = if hovered {
                    palette.omnibox_focus_border
                } else {
                    palette.omnibox_border
                };
                round_rect(dc, bounds, palette.omnibox_fill, border);
                draw_text(
                    dc,
                    rect(bounds.left + 12, bounds.top, bounds.right - 12, bounds.bottom),
                    &control.label,
                    if control.label.starts_with("Search by ") {
                        palette.omnibox_placeholder
                    } else {
                        palette.omnibox_text
                    },
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            }
            Role::Secondary | Role::Primary => {
                let primary = control.role == Role::Primary;
                let (fill, text) = match (primary, control.enabled) {
                    (true, true) if held => (
                        mix_toward(palette.progress_fill, palette.window, 0.25),
                        palette.window,
                    ),
                    (true, true) if hovered => (
                        mix_toward(palette.progress_fill, palette.tab_text_active, 0.18),
                        palette.window,
                    ),
                    (true, true) => (palette.progress_fill, palette.window),
                    (false, true) if held => (palette.tab_active, palette.omnibox_text),
                    (true, false) => (palette.omnibox_fill, palette.omnibox_placeholder),
                    (false, true) if hovered => (palette.tab_hover, palette.omnibox_text),
                    (false, true) => (palette.omnibox_fill, palette.omnibox_text),
                    (false, false) => (palette.omnibox_fill, palette.omnibox_placeholder),
                };
                round_rect(dc, bounds, fill, palette.omnibox_border);
                draw_text(
                    dc,
                    bounds,
                    &control.label,
                    text,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            }
        }
    }
}

fn dpi_of(hwnd: HWND) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 96 } else { dpi }
}

/// The panel state, valid from `WM_NCCREATE` until `WM_NCDESTROY`.
/// Only called on the UI thread from this window's own messages. Callers never hold this borrow
/// across the file dialog, whose nested message loop re-enters this window.
fn state<'a>(hwnd: HWND) -> Option<&'a mut Panel> {
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Panel).as_mut() }
}

/// Current scene for the open panel, sized from its client rectangle.
fn scene_for(hwnd: HWND, panel: &Panel) -> Scene {
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    build_panel(panel, client.right, client.bottom, dpi_of(hwnd))
}

/// Opens the Passwords & Passkeys panel over `owner`, or focuses the one already open.
pub fn open(owner: HWND) -> windows::core::Result<()> {
    if crate::profile::active().is_ok_and(|profile| profile.is_guest()) {
        return Err(windows::core::Error::new(
            windows::Win32::Foundation::E_ACCESSDENIED,
            "Password storage is unavailable in guest windows.",
        ));
    }
    unsafe {
        let existing = OPEN.with(Cell::get);
        if existing != 0 && IsWindow(Some(HWND(existing as *mut c_void))).as_bool() {
            let existing = HWND(existing as *mut c_void);
            let _ = ShowWindow(existing, SW_SHOW);
            let _ = SetForegroundWindow(existing);
            return Ok(());
        }

        let instance = windows::Win32::Foundation::HINSTANCE(
            windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?.0,
        );
        if !CLASS_REGISTERED.with(Cell::get) {
            let class = WNDCLASSW {
                lpfnWndProc: Some(panel_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return Err(windows::core::Error::from_thread());
            }
            CLASS_REGISTERED.with(|registered| registered.set(true));
        }

        let dpi = dpi_of(owner);
        let scale = |v: i32| (v as i64 * dpi as i64 / 96) as i32;
        // Borderless, so the panel uses its own palette throughout. Escape and the header button close it.
        let style = WS_POPUP;
        let (width, height) = (scale(CLIENT_WIDTH), scale(CLIENT_HEIGHT));

        let mut owner_rect = RECT::default();
        let _ = GetWindowRect(owner, &mut owner_rect);
        let x = owner_rect.left + (owner_rect.right - owner_rect.left - width) / 2;
        let y = owner_rect.top + (owner_rect.bottom - owner_rect.top - height) / 2;

        let mut panel = Box::new(Panel {
            flow: Flow::new(passwords_import::profiles()),
            plan: None,
            hover: Target::None,
            pressed: Target::None,
            view: PanelView::Vault,
            entries: Vec::new(),
            selected: None,
            list_offset: 0,
            search: String::new(),
            search_focused: false,
            editor: None,
            revealed: None,
            notice: None,
        });
        panel.refresh_entries();
        let panel = Box::into_raw(panel);
        let window = CreateWindowExW(
            Default::default(),
            CLASS_NAME,
            w!("Passwords & Passkeys"),
            style,
            x,
            y,
            width,
            height,
            Some(owner),
            None,
            Some(instance),
            Some(panel.cast()),
        );
        let window = match window {
            Ok(window) => window,
            Err(error) => {
                drop(Box::from_raw(panel));
                return Err(error);
            }
        };
        OPEN.with(|open| open.set(window.0 as isize));
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        Ok(())
    }
}

unsafe extern "system" fn panel_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_NCCREATE => {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if let Some(panel) = state(hwnd) {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut track);
                    let hover = scene_for(hwnd, panel).target_at(point_x(lparam), point_y(lparam));
                    if hover != panel.hover {
                        panel.hover = hover;
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                if let Some(panel) = state(hwnd) {
                    if panel.hover != Target::None {
                        panel.hover = Target::None;
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                if let Some(panel) = state(hwnd) {
                    let target = scene_for(hwnd, panel).target_at(point_x(lparam), point_y(lparam));
                    panel.pressed = target;
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let Some(panel) = state(hwnd) else {
                    return LRESULT(0);
                };
                let scene = scene_for(hwnd, panel);
                let target = scene.target_at(point_x(lparam), point_y(lparam));
                let pressed = std::mem::replace(&mut panel.pressed, Target::None);
                if target != Target::None && target == pressed && scene.enabled(target) {
                    activate(hwnd, target);
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let Some(panel) = state(hwnd) else {
                    return LRESULT(0);
                };
                match wparam.0 as u16 {
                    0x1B => {
                        if panel.view == PanelView::Edit {
                            panel.editor = None;
                            panel.view = PanelView::Vault;
                            panel.notice = None;
                        } else {
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                    0x0D => {
                        let target = match panel.view {
                            PanelView::Vault if panel.search_focused => {
                                panel.search_focused = false;
                                Target::None
                            }
                            PanelView::Vault => Target::None,
                            PanelView::Edit => Target::Save,
                            PanelView::Import => match panel.flow.stage {
                                Stage::Choose => Target::ChooseFile,
                                Stage::Preview { .. } => Target::Import,
                                Stage::Done { .. } => Target::Back,
                            },
                        };
                        if scene_for(hwnd, panel).enabled(target) {
                            activate(hwnd, target);
                        }
                    }
                    0x08 if panel.view == PanelView::Vault && panel.search_focused => {
                        panel.search.pop();
                        panel.refresh_entries();
                    }
                    0x08 if panel.view == PanelView::Edit => {
                        if let Some(draft) = panel.editor.as_mut() {
                            match draft.focus {
                                DraftField::Origin => { draft.origin.pop(); }
                                DraftField::Username => { draft.username.pop(); }
                                DraftField::Password => { draft.password.pop(); }
                            }
                        }
                    }
                    0x09 if panel.view == PanelView::Edit => {
                        if let Some(draft) = panel.editor.as_mut() {
                            draft.focus = match draft.focus {
                                DraftField::Origin => DraftField::Username,
                                DraftField::Username => DraftField::Password,
                                DraftField::Password => DraftField::Origin,
                            };
                        }
                    }
                    0x26 | 0x28 if panel.view == PanelView::Vault && !panel.entries.is_empty() => {
                        let selected = panel
                            .selected
                            .and_then(|id| panel.entries.iter().position(|entry| entry.id == id));
                        let next = match (wparam.0 as u16, selected) {
                            (0x28, Some(index)) => (index + 1).min(panel.entries.len() - 1),
                            (0x26, Some(index)) => index.saturating_sub(1),
                            (0x26, None) => panel.entries.len() - 1,
                            (_, None) => 0,
                            _ => unreachable!(),
                        };
                        panel.selected = Some(panel.entries[next].id);
                        if next < panel.list_offset {
                            panel.list_offset = next;
                        } else if next >= panel.list_offset + 5 {
                            panel.list_offset = next - 4;
                        }
                        panel.revealed = None;
                        let _ = KillTimer(Some(hwnd), REVEAL_CLEAR_TIMER);
                    }
                    _ => {}
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_CHAR => {
                handle_char(hwnd, wparam.0 as u16);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == REVEAL_CLEAR_TIMER => {
                let _ = KillTimer(Some(hwnd), REVEAL_CLEAR_TIMER);
                if let Some(panel) = state(hwnd) {
                    panel.revealed = None;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                OPEN.with(|open| {
                    if open.get() == hwnd.0 as isize {
                        open.set(0);
                    }
                });
                let panel = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut Panel;
                if !panel.is_null() {
                    drop(Box::from_raw(panel));
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

fn point_x(lparam: LPARAM) -> i32 {
    (lparam.0 & 0xFFFF) as i16 as i32
}

fn handle_char(hwnd: HWND, unit: u16) {
    let Some(panel) = state(hwnd) else {
        return;
    };
    if panel.view == PanelView::Vault {
        if panel.search_focused
            && let Some(character) = char::from_u32(u32::from(unit))
            && !character.is_control()
            && panel.search.len() + character.len_utf8() <= 256
        {
            panel.search.push(character);
            panel.list_offset = 0;
            panel.refresh_entries();
        }
    } else if panel.view == PanelView::Edit
        && let Some(draft) = panel.editor.as_mut()
    {
        let character = match unit {
            0xD800..=0xDBFF => {
                draft.pending_surrogate = Some(unit);
                return;
            }
            0xDC00..=0xDFFF => {
                let Some(high) = draft.pending_surrogate.take() else {
                    return;
                };
                let scalar = 0x1_0000
                    + ((u32::from(high) - 0xD800) << 10)
                    + (u32::from(unit) - 0xDC00);
                char::from_u32(scalar)
            }
            _ => {
                draft.pending_surrogate = None;
                char::from_u32(u32::from(unit))
            }
        };
        if let Some(character) = character.filter(|character| !character.is_control()) {
            let (value, limit) = match draft.focus {
                DraftField::Origin => (&mut draft.origin, 4096),
                DraftField::Username => (&mut draft.username, 4096),
                DraftField::Password => (&mut *draft.password, 16_384),
            };
            if value.len() + character.len_utf8() <= limit {
                value.push(character);
            }
        }
    }
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

fn point_y(lparam: LPARAM) -> i32 {
    ((lparam.0 >> 16) & 0xFFFF) as i16 as i32
}

/// Runs one user action. Nothing is borrowed across the file dialog or the adapter call, because
/// the dialog pumps messages back into this window.
fn activate(hwnd: HWND, target: Target) {
    match target {
        Target::Provider(index) => {
            if let Some(panel) = state(hwnd) {
                panel.flow.selected = index;
            }
        }
        Target::ChooseFile => {
            let Some(profile) = state(hwnd).and_then(|p| p.flow.selected_profile()) else {
                return;
            };
            let Some(path) = pick_file(hwnd, profile) else {
                return;
            };
            let previewed = passwords_import::preview_file(&path);
            if let Some(panel) = state(hwnd) {
                match previewed {
                    Ok(previewed) => {
                        panel.plan = Some(previewed.plan);
                        panel.flow.apply_preview(path, Ok(previewed.counts));
                    }
                    Err(error) => {
                        panel.plan = None;
                        panel.flow.apply_preview(path, Err(error));
                    }
                }
            }
        }
        Target::Back => {
            if let Some(panel) = state(hwnd) {
                panel.plan = None;
                if panel.view == PanelView::Import
                    && !matches!(panel.flow.stage, Stage::Preview { .. })
                {
                    panel.view = PanelView::Vault;
                    panel.notice = None;
                    panel.refresh_entries();
                } else {
                    panel.flow.back();
                }
            }
        }
        Target::OpenImport => {
            if let Some(panel) = state(hwnd) {
                panel.view = PanelView::Import;
                panel.notice = None;
                panel.search_focused = false;
            }
        }
        Target::Search => {
            if let Some(panel) = state(hwnd) {
                panel.search_focused = true;
                panel.notice = None;
            }
        }
        Target::Focus(field) => {
            if let Some(panel) = state(hwnd) {
                panel.search_focused = false;
                if let Some(draft) = panel.editor.as_mut() {
                    draft.focus = field;
                }
            }
        }
        Target::Select(id) => {
            if let Some(panel) = state(hwnd) {
                panel.selected = Some(id);
                panel.revealed = None;
                panel.search_focused = false;
                let _ = unsafe { KillTimer(Some(hwnd), REVEAL_CLEAR_TIMER) };
            }
        }
        Target::Add => {
            if let Some(panel) = state(hwnd) {
                panel.editor = Some(CredentialDraft::new(None, String::new(), String::new(), String::new()));
                panel.view = PanelView::Edit;
                panel.search_focused = false;
                panel.notice = None;
            }
        }
        Target::Edit => {
            let Some((id, entry)) = state(hwnd).and_then(|panel| {
                Some((panel.selected?, panel.selected_entry()?.clone()))
            }) else {
                return;
            };
            match PasswordStore::open_default().and_then(|vault| {
                vault.reveal(id).map(|password| (entry, password))
            }) {
                Ok((entry, password)) => {
                    if let Some(panel) = state(hwnd) {
                        panel.editor = Some(CredentialDraft::new(
                            Some(id),
                            entry.origin,
                            entry.username,
                            password.to_string(),
                        ));
                        if let Some(draft) = panel.editor.as_mut() {
                            draft.focus = DraftField::Password;
                        }
                        panel.view = PanelView::Edit;
                        panel.notice = None;
                    }
                }
                Err(error) => set_notice(hwnd, password_error_message(error)),
            }
        }
        Target::Delete => {
            let Some((id, entry)) = state(hwnd).and_then(|panel| {
                Some((panel.selected?, panel.selected_entry()?.clone()))
            }) else {
                return;
            };
            if confirm_delete_entry(hwnd, &entry)
                && let Some(panel) = state(hwnd)
            {
                match PasswordStore::open_default().and_then(|mut vault| vault.delete(id)) {
                    Ok(()) => {
                        panel.selected = None;
                        panel.revealed = None;
                        panel.notice = Some("Saved password deleted.");
                        panel.refresh_entries();
                    }
                    Err(error) => panel.notice = Some(password_error_message(error)),
                }
            }
        }
        Target::Reveal => {
            let Some(id) = state(hwnd).and_then(|panel| panel.selected) else {
                return;
            };
            let already_revealed = state(hwnd)
                .is_some_and(|panel| panel.revealed.as_ref().is_some_and(|(active, _)| *active == id));
            if already_revealed {
                if let Some(panel) = state(hwnd) {
                    panel.revealed = None;
                    panel.notice = None;
                }
                let _ = unsafe { KillTimer(Some(hwnd), REVEAL_CLEAR_TIMER) };
            } else {
                match PasswordStore::open_default().and_then(|vault| vault.reveal(id)) {
                    Ok(password) => {
                        if unsafe { SetTimer(Some(hwnd), REVEAL_CLEAR_TIMER, 15_000, None) } == 0 {
                            set_notice(hwnd, "Password reveal could not be timed; try again.");
                        } else if let Some(panel) = state(hwnd) {
                            panel.revealed = Some((id, Zeroizing::new(password.to_string())));
                            panel.notice = Some("Password hidden automatically after 15 seconds.");
                        }
                    }
                    Err(error) => set_notice(hwnd, password_error_message(error)),
                }
            }
        }
        Target::Copy => {
            let Some(id) = state(hwnd).and_then(|panel| panel.selected) else {
                return;
            };
            match PasswordStore::open_default().and_then(|vault| vault.reveal(id)) {
                Ok(password) if copy_password(hwnd, &password) => {
                    set_notice(hwnd, "Password copied; Bowser will clear it after 30 seconds if unchanged.");
                }
                Ok(_) => set_notice(hwnd, "Password was not copied or could not be cleared automatically."),
                Err(error) => set_notice(hwnd, password_error_message(error)),
            }
        }
        Target::Generate => {
            match passwords::generate_password(
                24,
                b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#$%^&*",
            ) {
                Ok(password) => {
                    if let Some(draft) = state(hwnd).and_then(|panel| panel.editor.as_mut()) {
                        draft.password = Zeroizing::new(password.to_string());
                        draft.focus = DraftField::Password;
                    }
                    set_notice(hwnd, "A 24-character password was generated.");
                }
                Err(error) => set_notice(hwnd, password_error_message(error)),
            }
        }
        Target::TogglePassword => {
            if let Some(draft) = state(hwnd).and_then(|panel| panel.editor.as_mut()) {
                draft.visible = !draft.visible;
                draft.focus = DraftField::Password;
            }
        }
        Target::Save => save_draft(hwnd),
        Target::Cancel => {
            if let Some(panel) = state(hwnd) {
                panel.editor = None;
                panel.view = PanelView::Vault;
                panel.notice = None;
            }
        }
        Target::Import => {
            // The plan is consumed here, so a second confirm cannot write the same rows again.
            let Some(plan) = state(hwnd).and_then(|p| p.plan.take()) else {
                return;
            };
            let result = passwords_import::commit_plan(plan);
            if let Some(panel) = state(hwnd) {
                panel.flow.apply_commit(result);
            }
        }
        Target::Close => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        Target::None => {}
    }
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn save_draft(hwnd: HWND) {
    let Some(draft) = state(hwnd).and_then(|panel| panel.editor.take()) else {
        return;
    };
    let result = PasswordStore::open_default().and_then(|mut vault| match draft.id {
        Some(id) => vault
            .update(id, &draft.origin, &draft.username, &draft.password)
            .map(|()| id),
        None => vault.add(&draft.origin, &draft.username, &draft.password),
    });
    if let Some(panel) = state(hwnd) {
        match result {
            Ok(id) => {
                panel.editor = None;
                panel.view = PanelView::Vault;
                panel.search.clear();
                panel.list_offset = 0;
                panel.selected = Some(id);
                panel.notice = Some("Password saved in this profile.");
                panel.refresh_entries();
            }
            Err(error) => {
                panel.editor = Some(draft);
                panel.notice = Some(password_error_message(error));
            }
        }
    }
}

fn set_notice(hwnd: HWND, notice: &'static str) {
    if let Some(panel) = state(hwnd) {
        panel.notice = Some(notice);
    }
}

fn password_error_message(error: PasswordError) -> &'static str {
    match error {
        PasswordError::Disabled => "Password storage is unavailable in this profile.",
        PasswordError::InsecureOrigin => "Use HTTPS, except HTTP is allowed for localhost.",
        PasswordError::Duplicate => "This exact password entry is already saved.",
        PasswordError::Conflict => "That site and username already have a different password.",
        PasswordError::MissingEntry => "That password entry is no longer available.",
        PasswordError::InvalidGenerator => "Password generator settings are invalid.",
        _ => "The password vault could not be read or saved.",
    }
}

fn confirm_delete_entry(hwnd: HWND, entry: &CredentialSummary) -> bool {
    use crate::win32::menu::{Entry, Icon, Item, Menu};
    let mut window = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut window) }.is_err() {
        return false;
    }
    crate::win32::menu::track(
        hwnd,
        POINT {
            x: window.right - 24,
            y: window.top + 72,
        },
        Menu {
            header: None,
            entries: vec![
                Entry::SectionHeader(format!("Delete password for {}?", entry.username)),
                Entry::Item(Item::new(1, "Delete saved password", Icon::Close)),
                Entry::Item(Item::new(2, "Keep password", Icon::Person)),
            ],
            footer: format!("This removes the entry for {} from this profile", entry.host),
        },
    )
    .is_some_and(|chosen| chosen.id == 1)
}

/// Offers a user-initiated fill for the active tab's exact current origin.
pub(crate) fn offer_fill(hwnd: HWND, app: &crate::app::AppRef) {
    use crate::win32::menu::{Entry, Icon, Item, Menu};

    let Some((tab_id, url)) = crate::app::App::active_site(app) else {
        return;
    };
    let Some(origin) = passwords::secure_origin(url.as_str()) else {
        return;
    };
    let Ok(vault) = PasswordStore::open_default() else {
        return;
    };
    let credentials = vault.get_for_origin(&origin);
    if credentials.is_empty() {
        return;
    }
    let Some(anchor) = menu_anchor(hwnd) else {
        return;
    };
    let mut entries = vec![Entry::SectionHeader("Choose a saved login".to_owned())];
    entries.extend(credentials.iter().enumerate().map(|(index, entry)| {
        Entry::Item(Item::new(
            index + 1,
            format!("{} — {}", entry.host, entry.username),
            Icon::Key,
        ))
    }));
    let Some(chosen) = crate::win32::menu::track(
        hwnd,
        anchor,
        Menu {
            header: None,
            entries,
            footer: "Bowser fills only this page's matching origin after your choice".to_owned(),
        },
    ) else {
        return;
    };
    let Some(entry) = credentials.get(chosen.id.saturating_sub(1)) else {
        return;
    };
    let Ok(password) = vault.reveal(entry.id) else {
        return;
    };
    let script = passwords::password_fill_script(&origin, &entry.username, &password);
    let _ = crate::app::App::execute_script_for_origin(app, tab_id, &origin, &script);
}

/// Handles only validated, top-frame submissions for the currently selected tab.
pub(crate) fn handle_login_message(
    hwnd: HWND,
    app: &crate::app::AppRef,
    tab_id: crate::tabs::TabId,
    mut message: passwords::LoginMessage,
) {
    if message.kind != passwords::LoginMessageKind::Submitted
        || crate::app::App::active_tab_id(app) != tab_id
    {
        return;
    }
    let Some(current_url) = crate::app::App::tab_committed_url(app, tab_id) else {
        return;
    };
    if passwords::secure_origin(current_url.as_str()).as_deref()
        != Some(message.origin.as_str())
    {
        return;
    }
    let (Some(username), Some(password)) = (message.username.take(), message.password.take())
    else {
        return;
    };
    let Ok(vault) = PasswordStore::open_default() else {
        return;
    };
    if vault.is_never_for_site(&message.origin) {
        return;
    }
    if SAVE_PROMPT_DECLINED_ORIGINS.with(|origins| origins.borrow().contains(&message.origin)) {
        return;
    }
    let matched = vault.match_login(&message.origin, &username, &password);
    if matches!(matched, passwords::LoginMatch::Unchanged(_)) {
        return;
    }
    if SAVE_PROMPT_ACTIVE.with(|active| active.replace(true)) {
        return;
    }
    let _prompt_guard = SavePromptGuard;
    let Some(choice) = save_prompt(hwnd, &message.origin, &username, matched) else {
        return;
    };
    match choice {
        2 => {
            let Ok(mut vault) = PasswordStore::open_default() else {
                return;
            };
            match vault.match_login(&message.origin, &username, &password) {
                passwords::LoginMatch::Missing => {
                    let _ = vault.add(&message.origin, &username, &password);
                }
                passwords::LoginMatch::Changed(id) => {
                    let _ = vault.update(id, &message.origin, &username, &password);
                }
                passwords::LoginMatch::Unchanged(_) => {}
            }
        }
        1 => {
            SAVE_PROMPT_DECLINED_ORIGINS.with(|origins| {
                origins.borrow_mut().insert(message.origin.clone());
            });
        }
        3 => {
            if let Ok(mut vault) = PasswordStore::open_default() {
                let _ = vault.mark_never_for_site(&message.origin, true);
            }
        }
        _ => {}
    }
}

fn save_prompt(
    hwnd: HWND,
    origin: &str,
    username: &str,
    matched: passwords::LoginMatch,
) -> Option<usize> {
    use crate::win32::menu::{Entry, Icon, Item, Menu};

    let anchor = menu_anchor(hwnd)?;
    let title = match matched {
        passwords::LoginMatch::Changed(_) => "Update saved password?",
        _ => "Save this password?",
    };
    let footer = match matched {
        passwords::LoginMatch::Changed(_) => "Save replaces the password for this username",
        _ => "The password stays in this profile's encrypted vault",
    };
    crate::win32::menu::track_with_default(
        hwnd,
        anchor,
        Menu {
            header: None,
            entries: vec![
                Entry::SectionHeader(format!("{title} for {username} at {origin}?")),
                Entry::Item(Item::new(1, "Not now", Icon::Close)),
                Entry::Item(Item::new(2, "Save", Icon::Key)),
                Entry::Item(Item::new(3, "Never for this site", Icon::Shield)),
            ],
            footer: footer.to_owned(),
        },
        Some(1),
    )
    .map(|chosen| chosen.id)
}

fn menu_anchor(hwnd: HWND) -> Option<POINT> {
    let mut window = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut window) }.ok()?;
    Some(POINT {
        x: window.right - 24,
        y: window.top + 72,
    })
}

/// Shows the system open dialog filtered to the chosen source's extension. Returns `None` when
/// cancelled or when the dialog cannot be created.
fn pick_file(owner: HWND, profile: Profile) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        // The extension filter needs the Win32_UI_Shell_Common feature, which is not enabled, so the
        // title names the expected file type instead and the adapter reports unreadable files.
        let title = to_wide(&format!(
            "Choose a {} export (.{})",
            profile.label, profile.extension
        ));
        dialog
            .SetOptions(FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM)
            .ok()?;
        dialog.SetTitle(PCWSTR(title.as_ptr())).ok()?;
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let display = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = display.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(display.0.cast_const().cast()));
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_entry_window_clamps_offsets_and_count() {
        let entries = [10, 20, 30, 40, 50, 60];
        assert_eq!(list_window(&entries, 3, 2), &[40, 50]);
        assert_eq!(list_window(&entries, 99, 5), &[]);
        assert_eq!(list_window(&entries, 4, 99), &[50, 60]);
    }
}
