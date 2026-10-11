//! The Passwords & Passkeys panel: a native, owned window that drives `passwords_import::Flow`.
//!
//! The panel is laid out once per paint and per pointer event from the same `build` function, so
//! what is drawn and what responds to a click cannot drift apart. Only counts and fixed messages are
//! ever shown; adapter payloads never reach a window.

use std::{
    cell::Cell,
    ffi::c_void,
    path::{Path, PathBuf},
};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
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
                RegisterClassW, SW_SHOW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
                WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE,
                WM_NCDESTROY, WM_PAINT, WNDCLASSW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    passwords::ImportPlan,
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
const CLIENT_HEIGHT: i32 = 460;
const PAD: i32 = 24;
const LINE: i32 = 24;
const ROW: i32 = 40;
const BUTTON_HEIGHT: i32 = 36;
const RADIUS: i32 = 10;
const NOTICE: &str = "Passkey transfer isn't available here. Passkeys already saved with your provider keep working through Windows and WebView2.";
const INTRO: &str = "Import saved logins from another browser or password manager. Everything stays on this device.";
const REMINDER: &str = "Keep the original passwords until you can verify a sign-in. Bowser cannot autofill imported entries yet. Then delete this plaintext export yourself; Bowser never deletes it for you.";

thread_local! {
    /// The open panel, so reopening the shortcut focuses it rather than stacking another.
    static OPEN: Cell<isize> = const { Cell::new(0) };
    static CLASS_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

struct Panel {
    flow: Flow,
    /// Parsed credentials awaiting confirmation. Held here, not in `Flow`, so counts stay plain data.
    plan: Option<ImportPlan>,
    hover: Target,
    pressed: Target,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    None,
    Provider(usize),
    ChooseFile,
    Back,
    Import,
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
                "Nothing has been imported yet. Confirm to write these passwords to this device. Imported passwords cannot autofill yet.",
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
        Stage::Choose => scene.control(
            Target::ChooseFile,
            rect(x1 - s(180), by, x1, by + s(BUTTON_HEIGHT)),
            "Choose file…",
            Role::Primary,
            flow.can_choose_file(),
        ),
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
            Target::Close,
            rect(x1 - s(120), by, x1, by + s(BUTTON_HEIGHT)),
            "Close",
            Role::Primary,
            true,
        ),
    }
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

/// Rounded control with an antialiased fill and a one-pixel border.
unsafe fn round_rect(dc: HDC, bounds: RECT, fill: Rgba, border: Rgba) {
    unsafe {
        smooth::fill_round_rect(dc, bounds, RADIUS, fill);
        smooth::stroke_round_rect(dc, bounds, RADIUS, border, 1);
    }
}

unsafe fn draw_text(dc: HDC, bounds: RECT, text: &str, colour: Rgba, format: DRAW_TEXT_FORMAT) {
    unsafe {
        let mut wide = to_wide(text);
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
            let scene = build(&panel.flow, client.right, client.bottom, dpi);
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
    build(&panel.flow, client.right, client.bottom, dpi_of(hwnd))
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

        let panel = Box::into_raw(Box::new(Panel {
            flow: Flow::new(passwords_import::profiles()),
            plan: None,
            hover: Target::None,
            pressed: Target::None,
        }));
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
                        let _ = DestroyWindow(hwnd);
                    }
                    0x0D => {
                        let target = match panel.flow.stage {
                            Stage::Choose => Target::ChooseFile,
                            Stage::Preview { .. } => Target::Import,
                            Stage::Done { .. } => Target::Close,
                        };
                        if scene_for(hwnd, panel).enabled(target) {
                            activate(hwnd, target);
                        }
                    }
                    0x26 if panel.flow.stage == Stage::Choose => {
                        panel.flow.select_previous();
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    0x28 if panel.flow.stage == Stage::Choose => {
                        panel.flow.select_next();
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    _ => {}
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
                panel.flow.back();
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
