//! The main ⋮ menu: a borderless popup drawn in the chrome palette, run as a synchronous modal loop
//! that returns the chosen command. Layout, hit testing, keyboard stepping and placement are pure
//! functions so they can be tested without a window.
//!
//! The popup is owned by the main window and takes capture while open. Any press outside it, Escape,
//! losing activation or losing capture dismisses it. Focus and capture are restored on every exit.
//!
//! A row with a flyout opens it beside the row while the popup stays on screen. A press on the popup
//! it came from, or Escape or Left in the flyout, closes only the flyout; a choice or any other press
//! closes the whole chain.

use std::{cell::Cell, time::{Duration, Instant}};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
            BLENDFUNCTION, BeginPaint, BitBlt, CLEARTYPE_QUALITY, CreateCompatibleBitmap,
            CreateCompatibleDC, CreateDIBSection, CreateFontIndirectW, CreateRoundRectRgn,
            CreateSolidBrush, DIB_RGB_COLORS, DRAW_TEXT_FORMAT, DT_CENTER, DT_END_ELLIPSIS,
            DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject,
            DrawTextW, EndPaint, FillRect, GetMonitorInfoW, GetObjectW, HBITMAP, HDC, HFONT,
            HGDIOBJ, LOGFONTW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
            PAINTSTRUCT, SRCCOPY, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                GetCapture, GetFocus, ReleaseCapture, SetCapture, SetFocus, VK_DOWN, VK_END,
                VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SPACE, VK_UP,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GWLP_USERDATA, GetClientRect, GetForegroundWindow, GetMessageW, GetWindowLongPtrW,
                GetWindowRect, IDC_ARROW, IsWindow, LoadCursorW, MSG, PostQuitMessage,
                RegisterClassW, SW_SHOW, SetForegroundWindow, SetWindowLongPtrW, SetTimer,
                KillTimer, ShowWindow, SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_TIMER,
                WA_INACTIVE, WM_ACTIVATE, WM_CAPTURECHANGED, WM_ERASEBKGND, WM_KEYDOWN,
                WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_PAINT,
                WM_RBUTTONDOWN, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{BOOL, PCWSTR, w},
};

use crate::{
    app::AppRef,
    theme::{self, Rgba},
    win32::{glass, smooth},
};

const CLASS_NAME: PCWSTR = w!("BowserMainMenu");
/// Logical (96 DPI) measurements. Every size is scaled from these by the owner's DPI.
const WIDTH: i32 = 320;
/// Right-hand column for shortcuts and the submenu chevron. Wide enough for "Ctrl+Shift+B".
const HINT_WIDTH: i32 = 100;
const HEADER_HEIGHT: i32 = 72;
const ROW_HEIGHT: i32 = 34;
const RICH_ROW_HEIGHT: i32 = 44;
const SEPARATOR_HEIGHT: i32 = 9;
const FOOTER_HEIGHT: i32 = 32;
const LIST_GAP: i32 = 4;
const PAD: i32 = 8;
const RADIUS: i32 = 12;
const ROW_RADIUS: i32 = 6;
const AVATAR: i32 = 36;
const ICON_FRAME_TIMER: usize = 0xB047;
const ICON_FRAME_MS: u32 = 16;
const ICON_EASE: Duration = Duration::from_millis(150);

/// One scalar per menu-row channel. Sampling is allocation-free and retargets from the value
/// currently on screen; pressing snaps immediately, while hover and release ease out.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IconMotion {
    from: f32,
    to: f32,
    started: Instant,
}

impl IconMotion {
    pub(crate) fn new(value: f32, now: Instant) -> Self { Self { from: value, to: value, started: now } }

    fn value(self, now: Instant) -> f32 {
        let t = now.saturating_duration_since(self.started).as_secs_f32()
            / ICON_EASE.as_secs_f32();
        let t = t.clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to - self.from) * eased
    }

    pub(crate) fn sample(&mut self, target: f32, now: Instant, snap: bool) -> f32 {
        if target != self.to {
            self.from = self.value(now);
            self.to = target;
            self.started = now;
        }
        if snap { self.from = self.to; }
        self.value(now)
    }

    pub(crate) fn running(self, now: Instant) -> bool {
        self.from != self.to && now.saturating_duration_since(self.started) < ICON_EASE
    }
}

#[derive(Clone, Copy)]
struct RowMotion { hover: IconMotion, press: IconMotion, active: IconMotion }

pub(crate) fn animations_enabled() -> bool {
    let mut enabled = BOOL(1);
    let read = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0,
        Some((&mut enabled as *mut BOOL).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    read.is_err() || enabled.as_bool()
}

thread_local! {
    static CLASS_REGISTERED: Cell<bool> = const { Cell::new(false) };
    static TRACKING_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn is_tracking() -> bool {
    TRACKING_DEPTH.with(|depth| depth.get() > 0)
}

struct TrackingGuard;

impl TrackingGuard {
    fn new() -> Self {
        TRACKING_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
        Self
    }
}

impl Drop for TrackingGuard {
    fn drop(&mut self) {
        TRACKING_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Monochrome glyph drawn with pen strokes, so it follows the palette and the DPI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    None,
    Plus,
    Person,
    Grid,
    Key,
    Shield,
    Gear,
    Close,
    /// Marks the selected choice in a list, such as the search engine picker.
    Check,
    /// Site information: a padlock for a secure connection, and its open variant for plain HTTP.
    Lock,
    Unlock,
    /// A certificate problem.
    Warning,
    /// Local, internal or unknown pages, and the default for the connection row.
    Info,
    Mic,
    Camera,
    Location,
    Bell,
    Cookie,
    /// Circular arrow for resetting stored choices.
    Reset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub id: usize,
    pub label: String,
    /// Muted hint drawn right-aligned, such as "Ctrl+T".
    pub shortcut: Option<&'static str>,
    pub icon: Icon,
    pub checked: bool,
    pub enabled: bool,
    /// Opens a flyout rather than running a command, so it shows a chevron instead of a shortcut.
    pub submenu: bool,
    pub submenu_entries: Option<Vec<Entry>>,
    /// Full wording shown in the footer while the row is highlighted, for labels kept short.
    pub description: Option<&'static str>,
    pub rich_row: Option<RichRow>,
    /// Profile avatar glyph and accent fill, drawn in the normal icon slot.
    pub badge: Option<(String, Rgba)>,
}

/// Optional list presentation for a page or other row that has a title and favicon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RichRow {
    pub title: String,
    pub favicon: Option<crate::favicon::FaviconBitmap>,
}

impl Item {
    pub fn new(id: usize, label: impl Into<String>, icon: Icon) -> Self {
        Self {
            id,
            label: label.into(),
            shortcut: None,
            icon,
            checked: false,
            enabled: true,
            submenu: false,
            submenu_entries: None,
            description: None,
            rich_row: None,
            badge: None,
        }
    }

    pub fn shortcut(mut self, shortcut: &'static str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn described(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn disabled_if(mut self, disabled: bool) -> Self {
        self.enabled = !disabled;
        self
    }

    pub fn submenu_with(mut self, entries: Vec<Entry>) -> Self {
        self.submenu = true;
        self.submenu_entries = Some(entries);
        self
    }

    pub fn rich_row(
        mut self,
        title: impl Into<String>,
        favicon: Option<crate::favicon::FaviconBitmap>,
    ) -> Self {
        self.rich_row = Some(RichRow {
            title: title.into(),
            favicon,
        });
        self
    }

    pub fn profile_badge(mut self, glyph: impl Into<String>, accent: Rgba) -> Self {
        self.badge = Some((glyph.into(), accent));
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    Item(Item),
    Separator,
    SectionHeader(String),
}

/// Profile card drawn above the list: avatar initial, profile name and a muted caption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProfileHeader {
    pub profile: String,
    pub caption: String,
    pub avatar: String,
    pub accent: Rgba,
}

/// Everything the popup shows. Only the main ⋮ menu sets `header`; every other popup leaves it
/// `None`, so it has no header block and its list starts at the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Menu {
    pub header: Option<ProfileHeader>,
    pub entries: Vec<Entry>,
    /// Non-interactive text at the bottom, such as the version.
    pub footer: String,
}

/// The command the user picked and the row it came from, in screen coordinates, so a flyout can
/// open beside that row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Chosen {
    pub id: usize,
    pub row: RECT,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MenuResult {
    Chosen(Chosen),
    Back,
    Dismissed,
}

fn selectable(entry: &Entry) -> bool {
    matches!(entry, Entry::Item(item) if item.enabled)
}

fn initial_item_index(entries: &[Entry], item_id: Option<usize>) -> Option<usize> {
    item_id.and_then(|id| {
        entries
            .iter()
            .position(|entry| matches!(entry, Entry::Item(item) if item.id == id && item.enabled))
    })
}

/// Next selectable entry after `from` in the given direction, wrapping at the ends. With no
/// selection, Down starts at the first entry and Up at the last. `None` when nothing can be chosen.
pub(crate) fn step(entries: &[Entry], from: Option<usize>, forward: bool) -> Option<usize> {
    let count = entries.len() as isize;
    if count == 0 {
        return None;
    }
    let start = match from {
        Some(index) => index as isize,
        None if forward => -1,
        None => count,
    };
    (1..=count).find_map(|k| {
        let offset = if forward { k } else { -k };
        let index = (start + offset).rem_euclid(count) as usize;
        selectable(&entries[index]).then_some(index)
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Row {
    rect: RECT,
    entry: usize,
    selectable: bool,
}

/// Pixel geometry for one DPI, relative to the popup's client area.
#[derive(Clone, Debug, PartialEq)]
struct Layout {
    width: i32,
    height: i32,
    header: RECT,
    rows: Vec<Row>,
    footer: RECT,
    list_top: i32,
    list_bottom: i32,
    max_scroll: i32,
}

fn scaled(logical: i32, dpi: u32) -> i32 {
    (i64::from(logical) * i64::from(dpi) / 96) as i32
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

impl Layout {
    fn new(menu: &Menu, dpi: u32, available_height: i32) -> Self {
        let s = |v: i32| scaled(v, dpi);
        let width = s(WIDTH);
        let header_height = if menu.header.is_some() {
            s(HEADER_HEIGHT)
        } else {
            0
        };
        let header = rect(0, 0, width, header_height);
        let mut y = header.bottom + s(LIST_GAP);
        let mut rows = Vec::with_capacity(menu.entries.len());
        for (entry, kind) in menu.entries.iter().enumerate() {
            let height = match kind {
                Entry::Item(item) => s(if item.rich_row.is_some() {
                    RICH_ROW_HEIGHT
                } else {
                    ROW_HEIGHT
                }),
                Entry::Separator => s(SEPARATOR_HEIGHT),
                Entry::SectionHeader(_) => s(SEPARATOR_HEIGHT + 13),
            };
            rows.push(Row {
                rect: rect(0, y, width, y + height),
                entry,
                selectable: selectable(kind),
            });
            y += height;
        }
        let full_height = y + s(LIST_GAP) + s(FOOTER_HEIGHT);
        let height = full_height.min(available_height.max(header_height + s(FOOTER_HEIGHT + 10)));
        let footer = rect(0, height - s(FOOTER_HEIGHT), width, height);
        let list_top = header.bottom + s(LIST_GAP);
        let list_bottom = (footer.top - s(LIST_GAP)).max(list_top);
        let content_bottom = rows.last().map_or(list_top, |row| row.rect.bottom);
        Self {
            width,
            height,
            header,
            rows,
            footer,
            list_top,
            list_bottom,
            max_scroll: (content_bottom - list_bottom).max(0),
        }
    }

    /// Entry index of the selectable row under the point, if any.
    fn entry_at(&self, x: i32, y: i32, scroll: i32) -> Option<usize> {
        if y < self.list_top || y >= self.list_bottom {
            return None;
        }
        self.rows
            .iter()
            .find(|row| row.selectable && contains(self.row_rect(row, scroll), x, y))
            .map(|row| row.entry)
    }

    fn row_rect(&self, row: &Row, scroll: i32) -> RECT {
        rect(
            row.rect.left,
            row.rect.top - scroll,
            row.rect.right,
            row.rect.bottom - scroll,
        )
    }

    fn ensure_visible(&self, entry: usize, scroll: i32) -> i32 {
        let Some(row) = self.rows.iter().find(|row| row.entry == entry) else {
            return scroll;
        };
        let top = row.rect.top - scroll;
        let bottom = row.rect.bottom - scroll;
        if top < self.list_top {
            (row.rect.top - self.list_top).clamp(0, self.max_scroll)
        } else if bottom > self.list_bottom {
            (row.rect.bottom - self.list_bottom).clamp(0, self.max_scroll)
        } else {
            scroll
        }
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width && y < self.height
    }
}

/// Top-left corner for a popup whose top-right corner is `anchor`, moved to keep it inside `work`.
pub(crate) fn place(anchor: POINT, width: i32, height: i32, work: RECT) -> POINT {
    let x = (anchor.x - width).clamp(work.left, (work.right - width).max(work.left));
    let y = if anchor.y + height <= work.bottom {
        anchor.y
    } else {
        (work.bottom - height).max(work.top)
    };
    POINT { x, y }
}

/// Positions a flyout at the row's right edge, flipping to its left when needed and clamping it
/// into the nearest monitor work area in both axes.
pub(crate) fn place_submenu(anchor: POINT, width: i32, height: i32, work: RECT) -> POINT {
    let right = anchor.x + width <= work.right;
    let preferred_x = if right { anchor.x } else { anchor.x - width };
    let x = preferred_x.clamp(work.left, (work.right - width).max(work.left));
    let y = anchor
        .y
        .clamp(work.top, (work.bottom - height).max(work.top));
    POINT { x, y }
}

/// Places a context/list popup below its anchor, flipping above or to the other side at an edge.
pub(crate) fn place_below(anchor: POINT, width: i32, height: i32, work: RECT) -> POINT {
    let x = if anchor.x + width <= work.right {
        anchor.x
    } else {
        anchor.x - width
    }
    .clamp(work.left, (work.right - width).max(work.left));
    let y = if anchor.y + height <= work.bottom {
        anchor.y
    } else {
        anchor.y - height
    }
    .clamp(work.top, (work.bottom - height).max(work.top));
    POINT { x, y }
}

fn offset(r: RECT, origin: POINT) -> RECT {
    rect(
        r.left + origin.x,
        r.top + origin.y,
        r.right + origin.x,
        r.bottom + origin.y,
    )
}

/// Everything the window procedure needs. Lives in a `Box` from creation until the modal loop ends.
/// Interaction state is in `Cell`s so the loop and the procedure can both read it through `&`.
struct State {
    menu: Menu,
    layout: Layout,
    body: HFONT,
    heading: HFONT,
    rich_bitmaps: Vec<RichBitmap>,
    icon_motion: Vec<Cell<RowMotion>>,
    /// Entry under the pointer or chosen with the arrow keys.
    current: Cell<Option<usize>>,
    scroll: Cell<i32>,
    /// Entry the left button went down on, so only a matching release activates.
    pressed: Cell<Option<usize>>,
    chosen: Cell<Option<usize>>,
    done: Cell<bool>,
    is_submenu: bool,
    /// Set when this flyout should close and return to its parent, not the whole chain.
    back: Cell<bool>,
    /// A flyout opened from this popup is running. Its focus and capture changes do not dismiss this
    /// popup, which stays on screen beside it.
    child_open: Cell<bool>,
    /// Screen rectangles of the popups this one was opened from, nearest last.
    ancestors: Vec<RECT>,
}

impl State {
    /// Back on this popup after a flyout returned: the choice and press that opened it are spent.
    fn resume(&self) {
        self.done.set(false);
        self.back.set(false);
        self.chosen.set(None);
        self.pressed.set(None);
    }
}

struct RichBitmap {
    entry: usize,
    dc: HDC,
    bitmap: HBITMAP,
    original: HGDIOBJ,
}

// --- async menu API (lane M) ---
/// Where `show` puts the menu relative to `anchor` (screen coordinates): `AboveLeft` puts its
/// top-right corner there (as `track` does), `BelowLeft` its top-left corner (as `track_below`).
pub(crate) use super::html_popup::Placement;

/// Shows `menu` without blocking and calls `callback` with the pick, or `None` when dismissed.
///
/// With the HTML chrome on and the window's popup host ready, the menu renders in the HTML popup
/// and the callback runs later from the deferred chrome queue (never inside WebView2's message
/// callback), after the pick's token and enabled id are checked. Otherwise the native menu runs
/// right away and the callback runs before this returns. Either way the callback must re-check the
/// ids it captured against the live model: tabs, workspaces and files may change meanwhile.
///
/// `kind` names the menu for the 300 ms reopen guard (a click that dismissed a menu does not reopen
/// the same kind); include the target id when menus for different targets should not guard each
/// other, such as `tab:12`. At most 32 bytes.
pub(crate) fn show(
    owner: HWND,
    anchor: POINT,
    placement: Placement,
    menu: Menu,
    kind: &str,
    callback: impl FnOnce(&AppRef, Option<Chosen>) + 'static,
) {
    let Some(app) = super::window::popup_app(owner) else {
        return;
    };
    // One menu at a time: a new one replaces any that is open, which reports a dismissal.
    if super::html_popup::is_active(owner) {
        super::html_popup::cancel(owner, &app);
    }
    let context = super::window::popup_context(owner);
    let callback = match super::html_popup::show_menu(
        owner,
        anchor,
        placement,
        &menu,
        kind,
        context,
        Box::new(callback),
    ) {
        Ok(()) => return,
        Err(super::html_popup::ShowRefused::Guarded(callback)) => {
            callback(&app, None);
            return;
        }
        Err(super::html_popup::ShowRefused::Unavailable(callback)) => callback,
    };
    let chosen = match placement {
        Placement::AboveLeft => track(owner, anchor, menu),
        Placement::BelowLeft => track_below(owner, anchor, menu),
    };
    callback(&app, chosen);
}

/// The entry at `index` of the list a menu was built from, if the live list still has it. Menu ids
/// that encode a list position use this so a pick never acts on whatever moved into that slot.
pub(crate) fn still_listed<'a, T: PartialEq>(built: &'a [T], live: &[T], index: usize) -> Option<&'a T> {
    built.get(index).filter(|item| live.contains(item))
}
// --- end async menu API ---

/// Shows the menu with its top-right corner at `anchor` (screen coordinates) and returns the item
/// the user activated, or `None` when it was dismissed. Blocks until then.
pub(crate) fn track(owner: HWND, anchor: POINT, menu: Menu) -> Option<Chosen> {
    track_with_default(owner, anchor, menu, None)
}

/// Shows a popup with an initial keyboard selection for the requested item id.
pub(crate) fn track_with_default(
    owner: HWND,
    anchor: POINT,
    menu: Menu,
    default_item_id: Option<usize>,
) -> Option<Chosen> {
    loop {
        match track_placed(
            owner,
            anchor,
            menu.clone(),
            false,
            false,
            &[],
            default_item_id,
        ) {
            MenuResult::Chosen(chosen) => return Some(chosen),
            MenuResult::Back => continue,
            MenuResult::Dismissed => return None,
        }
    }
}

/// Shows a context or list popup at its top-left anchor, flipping when the work area requires it.
pub(crate) fn track_below(owner: HWND, anchor: POINT, menu: Menu) -> Option<Chosen> {
    loop {
        match track_placed(owner, anchor, menu.clone(), false, true, &[], None) {
            MenuResult::Chosen(chosen) => return Some(chosen),
            MenuResult::Back => continue,
            MenuResult::Dismissed => return None,
        }
    }
}

/// Shows one popup and runs it until it is chosen, dismissed, or backed out of. `ancestors` are the
/// screen rectangles of the popups it was opened from. A flyout opened from this popup runs inside
/// this call, so this popup stays on screen beside it: a choice or dismissal in the flyout closes the
/// whole chain, and Back from it resumes this popup.
fn track_placed(
    owner: HWND,
    anchor: POINT,
    menu: Menu,
    submenu: bool,
    below: bool,
    ancestors: &[RECT],
    default_item_id: Option<usize>,
) -> MenuResult {
    let _tracking = TrackingGuard::new();
    let dpi = dpi_of(owner);
    let work = work_area(anchor);
    let layout = Layout::new(&menu, dpi, work.bottom - work.top);
    let origin = if submenu {
        place_submenu(anchor, layout.width, layout.height, work)
    } else if below {
        place_below(anchor, layout.width, layout.height, work)
    } else {
        place(anchor, layout.width, layout.height, work)
    };
    let screen = offset(rect(0, 0, layout.width, layout.height), origin);

    let body = crate::chrome::paint::create_message_font(dpi, CLEARTYPE_QUALITY);
    let heading = unsafe { create_heading_font(body) };
    let rich_bitmaps = unsafe { create_rich_bitmaps(&menu) };
    let initial_item = initial_item_index(&menu.entries, default_item_id);
    let now = Instant::now();
    let icon_motion = menu.entries.iter().map(|_| {
        Cell::new(RowMotion { hover: IconMotion::new(0.0, now),
            press: IconMotion::new(0.0, now), active: IconMotion::new(0.0, now) })
    }).collect();
    let state = Box::into_raw(Box::new(State {
        menu,
        layout,
        body,
        heading,
        rich_bitmaps,
        icon_motion,
        current: Cell::new(initial_item),
        scroll: Cell::new(0),
        pressed: Cell::new(None),
        chosen: Cell::new(None),
        done: Cell::new(false),
        is_submenu: submenu,
        back: Cell::new(false),
        child_open: Cell::new(false),
        ancestors: ancestors.to_vec(),
    }));
    let prior_focus = unsafe { GetFocus() };
    let owner_rect = unsafe { window_rect(owner) };
    let window = unsafe { create_window(owner, origin, &*state, state.cast_const().cast()) };
    let Some(window) = window else {
        unsafe { free_state(state) };
        return MenuResult::Dismissed;
    };

    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
    }
    take_input(window);

    let state_ref = unsafe { &*state };
    let outcome = loop {
        if !pump_until_done(owner, owner_rect, state_ref) {
            break MenuResult::Dismissed;
        }
        if state_ref.back.get() {
            break MenuResult::Back;
        }
        if let Some((flyout, row)) = flyout_request(state_ref, origin) {
            // This popup stays up. The flyout's own loop pumps the messages until it closes.
            let chain: Vec<RECT> = ancestors.iter().copied().chain([screen]).collect();
            state_ref.child_open.set(true);
            let child = track_placed(
                owner,
                POINT {
                    x: row.right,
                    y: row.top,
                },
                flyout,
                true,
                false,
                &chain,
                None,
            );
            state_ref.child_open.set(false);
            match after_flyout(child) {
                Some(result) => break result,
                None => {
                    state_ref.resume();
                    invalidate(window);
                    take_input(window);
                }
            }
            continue;
        }
        break chosen_result(state_ref, origin);
    };

    // Focus only follows the menu while Bowser is still in front. After a dismissal by another
    // app taking activation, focus stays where that app put it: no foreground stealing.
    let foreground = unsafe { GetForegroundWindow() };
    let app_in_front = foreground == window || foreground == owner;
    unsafe {
        if GetCapture() == window {
            let _ = ReleaseCapture();
        }
        if IsWindow(Some(window)).as_bool() {
            let _ = DestroyWindow(window);
        }
        free_state(state);
        // A flyout leaves focus to the popup it came from, which restores it when it closes.
        if ancestors.is_empty() && app_in_front && IsWindow(Some(owner)).as_bool() {
            if foreground == window {
                let _ = SetForegroundWindow(owner);
            }
            if !prior_focus.is_invalid() && IsWindow(Some(prior_focus)).as_bool() {
                let _ = SetFocus(Some(prior_focus));
            }
        }
    }
    outcome
}

/// Gives the popup the keyboard and the mouse. Also run when a flyout backs out to this popup.
fn take_input(window: HWND) {
    unsafe {
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(window));
        let _ = SetCapture(window);
    }
}

/// Dispatches messages until the popup is done. Returns false when the thread is quitting.
fn pump_until_done(owner: HWND, owner_rect: RECT, state: &State) -> bool {
    // Messages for other windows are dispatched during the popup's modal loop.
    while !state.done.get() {
        let mut msg = MSG::default();
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            // WM_QUIT (0) or failure (-1): leave the loop and let the caller's loop see the quit.
            if got.0 == 0 {
                unsafe { PostQuitMessage(msg.wParam.0 as i32) };
            }
            return false;
        }
        unsafe {
            DispatchMessageW(&msg);
        }
        // The owner was destroyed, moved or resized (its messages ran through the loop above):
        // the menu no longer sits under its anchor, so it is dismissed.
        if unsafe { owner_changed(owner, owner_rect) } {
            state.done.set(true);
        }
    }
    true
}

/// The submenu row that was chosen, with its entries and the row's screen rectangle. The flyout opens
/// beside the row as it is drawn, so a scrolled list places it correctly.
fn flyout_request(state: &State, origin: POINT) -> Option<(Menu, RECT)> {
    let index = state.chosen.get()?;
    let Entry::Item(item) = state.menu.entries.get(index)? else {
        return None;
    };
    let entries = item.submenu_entries.clone()?;
    let row = state.layout.rows.iter().find(|row| row.entry == index)?;
    Some((
        Menu {
            header: None,
            entries,
            footer: state.menu.footer.clone(),
        },
        offset(state.layout.row_rect(row, state.scroll.get()), origin),
    ))
}

/// The result of a popup whose loop has ended: the chosen command, or dismissal when none was chosen.
fn chosen_result(state: &State, origin: POINT) -> MenuResult {
    let chosen = state.chosen.get().and_then(|index| {
        let row = state.layout.rows.iter().find(|row| row.entry == index)?;
        match &state.menu.entries[index] {
            Entry::Item(item) => Some(Chosen {
                id: item.id,
                row: offset(state.layout.row_rect(row, state.scroll.get()), origin),
            }),
            Entry::Separator | Entry::SectionHeader(_) => None,
        }
    });
    chosen.map_or(MenuResult::Dismissed, MenuResult::Chosen)
}

/// What a press outside a popup does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutsidePress {
    /// On one of the popups this flyout was opened from: only the flyout closes, and the press is
    /// spent on returning to that popup.
    CloseFlyout,
    /// Anywhere else: the whole chain closes.
    CloseChain,
}

/// Classifies a press outside the popup (screen coordinates) against the popups it was opened from.
pub(crate) fn outside_press(ancestors: &[RECT], point: POINT) -> OutsidePress {
    if ancestors
        .iter()
        .any(|ancestor| contains(*ancestor, point.x, point.y))
    {
        OutsidePress::CloseFlyout
    } else {
        OutsidePress::CloseChain
    }
}

/// What a popup does when its flyout returns: Back keeps it open, while a choice or a dismissal
/// closes it too.
pub(crate) fn after_flyout(result: MenuResult) -> Option<MenuResult> {
    match result {
        MenuResult::Back => None,
        other => Some(other),
    }
}

/// Losing activation or capture dismisses a popup, except while the flyout it opened holds them.
pub(crate) fn dismisses_on_focus_loss(child_open: bool) -> bool {
    !child_open
}

/// A press outside the popup. Over one of its ancestors it closes only this flyout; anywhere else it
/// closes the whole chain.
unsafe fn dismiss_outside(hwnd: HWND, state: &State, x: i32, y: i32) {
    unsafe {
        let window = window_rect(hwnd);
        let point = POINT {
            x: window.left + x,
            y: window.top + y,
        };
        if outside_press(&state.ancestors, point) == OutsidePress::CloseFlyout {
            state.back.set(true);
        }
        state.done.set(true);
    }
}

unsafe fn window_rect(hwnd: HWND) -> RECT {
    unsafe {
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        rect
    }
}

/// True when the owner is gone or has moved or resized since the menu opened.
unsafe fn owner_changed(owner: HWND, before: RECT) -> bool {
    unsafe { !IsWindow(Some(owner)).as_bool() || window_rect(owner) != before }
}

/// Frees the popup state and its fonts. The window must already be destroyed.
unsafe fn free_state(state: *mut State) {
    unsafe {
        let state = Box::from_raw(state);
        for image in &state.rich_bitmaps {
            let _ = SelectObject(image.dc, image.original);
            let _ = DeleteObject(image.bitmap.into());
            let _ = DeleteDC(image.dc);
        }
        let _ = DeleteObject(state.body.into());
        let _ = DeleteObject(state.heading.into());
    }
}

unsafe fn create_rich_bitmaps(menu: &Menu) -> Vec<RichBitmap> {
    unsafe {
        let mut result = Vec::new();
        for (entry, entry_value) in menu.entries.iter().enumerate() {
            let Entry::Item(item) = entry_value else {
                continue;
            };
            let Some(favicon) = item.rich_row.as_ref().and_then(|row| row.favicon.as_ref()) else {
                continue;
            };
            let (width, height) = (favicon.width as i32, favicon.height as i32);
            if width <= 0
                || height <= 0
                || favicon.bgra_premultiplied.len() != (width * height * 4) as usize
            {
                continue;
            }
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                continue;
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                let _ = DeleteDC(dc);
                continue;
            };
            if bitmap.is_invalid() || bits.is_null() {
                let _ = DeleteDC(dc);
                continue;
            }
            std::ptr::copy_nonoverlapping(
                favicon.bgra_premultiplied.as_ptr(),
                bits.cast::<u8>(),
                favicon.bgra_premultiplied.len(),
            );
            let original = SelectObject(dc, bitmap.into());
            result.push(RichBitmap {
                entry,
                dc,
                bitmap,
                original,
            });
        }
        result
    }
}

unsafe fn create_window(
    owner: HWND,
    origin: POINT,
    state: &State,
    params: *const std::ffi::c_void,
) -> Option<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
        if !CLASS_REGISTERED.with(Cell::get) {
            let class = WNDCLASSW {
                lpfnWndProc: Some(menu_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return None;
            }
            CLASS_REGISTERED.with(|registered| registered.set(true));
        }
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!("Bowser menu"),
            WS_POPUP,
            origin.x,
            origin.y,
            state.layout.width,
            state.layout.height,
            Some(owner),
            None,
            Some(instance),
            Some(params),
        )
        .ok()?;
        // The region takes ownership of the handle.
        let radius = scaled(RADIUS, dpi_of(owner)) * 2;
        let region = CreateRoundRectRgn(
            0,
            0,
            state.layout.width + 1,
            state.layout.height + 1,
            radius,
            radius,
        );
        let _ = SetWindowRgn(window, Some(region), false);
        Some(window)
    }
}

/// The body font made larger and semibold, for the profile name and avatar initial.
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

fn dpi_of(hwnd: HWND) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 96 } else { dpi }
}

/// Work area of the monitor nearest the point, so the menu stays clear of the taskbar.
fn work_area(anchor: POINT) -> RECT {
    unsafe {
        let monitor = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            rect(0, 0, 1920, 1080)
        }
    }
}

/// The popup state, valid from `WM_NCCREATE` until the modal loop frees it.
fn state<'a>(hwnd: HWND) -> Option<&'a State> {
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref() }
}

/// Client-space point from `WM_*MOUSE*` parameters, with the signed coordinates capture can produce.
fn point(lparam: LPARAM) -> (i32, i32) {
    (
        (lparam.0 & 0xFFFF) as i16 as i32,
        ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    )
}

unsafe extern "system" fn menu_proc(
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
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == ICON_FRAME_TIMER => {
                invalidate(hwnd);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if let Some(state) = state(hwnd) {
                    let (x, y) = point(lparam);
                    let hover = state.layout.entry_at(x, y, state.scroll.get());
                    if hover != state.current.get() {
                        state.current.set(hover);
                        invalidate(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                if let Some(state) = state(hwnd) {
                    let (x, y) = point(lparam);
                    if state.layout.inside(x, y) {
                        state
                            .pressed
                            .set(state.layout.entry_at(x, y, state.scroll.get()));
                        invalidate(hwnd);
                    } else {
                        dismiss_outside(hwnd, state, x, y);
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if let Some(state) = state(hwnd) {
                    let (x, y) = point(lparam);
                    let released = state.layout.entry_at(x, y, state.scroll.get());
                    if state.layout.inside(x, y)
                        && released.is_some()
                        && released == state.pressed.get()
                    {
                        state.chosen.set(released);
                        state.done.set(true);
                    }
                    state.pressed.set(None);
                    invalidate(hwnd);
                }
                LRESULT(0)
            }
            WM_RBUTTONDOWN => {
                if let Some(state) = state(hwnd) {
                    let (x, y) = point(lparam);
                    if !state.layout.inside(x, y) {
                        dismiss_outside(hwnd, state, x, y);
                    }
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                if let Some(state) = state(hwnd) {
                    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                    let amount = scaled(ROW_HEIGHT * 3, dpi_of(hwnd));
                    let next = (state.scroll.get() - amount * i32::from(delta.signum()))
                        .clamp(0, state.layout.max_scroll);
                    if next != state.scroll.get() {
                        state.scroll.set(next);
                        state.current.set(None);
                        invalidate(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                if let Some(state) = state(hwnd) {
                    let key = wparam.0;
                    let is = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| {
                        key == vk.0 as usize
                    };
                    if is(VK_ESCAPE) {
                        // Escape in a flyout returns to its parent; at the top it dismisses the menu.
                        state.back.set(state.is_submenu);
                        state.done.set(true);
                    } else if is(VK_DOWN) || is(VK_UP) {
                        let next = step(&state.menu.entries, state.current.get(), is(VK_DOWN));
                        if next != state.current.get() {
                            state.current.set(next);
                            if let Some(index) = next {
                                state
                                    .scroll
                                    .set(state.layout.ensure_visible(index, state.scroll.get()));
                            }
                            invalidate(hwnd);
                        }
                    } else if is(VK_HOME) || is(VK_END) {
                        let next = step(&state.menu.entries, None, is(VK_HOME));
                        if next != state.current.get() {
                            state.current.set(next);
                            if let Some(index) = next {
                                state
                                    .scroll
                                    .set(state.layout.ensure_visible(index, state.scroll.get()));
                            }
                            invalidate(hwnd);
                        }
                    } else if is(VK_RETURN) || is(VK_SPACE) {
                        if let Some(index) = state.current.get() {
                            state.chosen.set(Some(index));
                            state.done.set(true);
                        }
                    } else if is(VK_RIGHT) {
                        // Right opens a flyout row, as in native menus; other rows ignore it.
                        if let Some(index) = state.current.get() {
                            if matches!(&state.menu.entries[index], Entry::Item(item) if item.submenu)
                            {
                                state.chosen.set(Some(index));
                                state.done.set(true);
                            }
                        }
                    } else if is(VK_LEFT) && state.is_submenu {
                        state.back.set(true);
                        state.done.set(true);
                    }
                }
                LRESULT(0)
            }
            WM_ACTIVATE => {
                // Low word of WPARAM is the activation state; any other window taking focus dismisses.
                if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                    if let Some(state) = state(hwnd) {
                        if dismisses_on_focus_loss(state.child_open.get()) {
                            state.done.set(true);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                // A flyout takes capture from this popup while it is open; that is not a dismissal.
                if let Some(state) = state(hwnd) {
                    if dismisses_on_focus_loss(state.child_open.get()) {
                        state.done.set(true);
                    }
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false);
    }
}

fn colorref(colour: Rgba) -> windows::Win32::Foundation::COLORREF {
    windows::Win32::Foundation::COLORREF(
        u32::from(colour.r) | (u32::from(colour.g) << 8) | (u32::from(colour.b) << 16),
    )
}

/// Draws into a memory bitmap and copies it once, so hover changes do not flicker.
unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        if let Some(state) = state(hwnd) {
            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let memory = CreateCompatibleDC(Some(dc));
            let bitmap = CreateCompatibleBitmap(dc, client.right, client.bottom);
            let old = SelectObject(memory, bitmap.into());
            draw(memory, hwnd, &client, dpi_of(hwnd), state);
            let _ = BitBlt(
                dc,
                0,
                0,
                client.right,
                client.bottom,
                Some(memory),
                0,
                0,
                SRCCOPY,
            );
            SelectObject(memory, old);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe fn draw_text(dc: HDC, bounds: RECT, text: &str, colour: Rgba, format: DRAW_TEXT_FORMAT) {
    unsafe {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        if wide.is_empty() {
            return;
        }
        let mut bounds = bounds;
        SetTextColor(dc, colorref(colour));
        DrawTextW(dc, &mut wide, &mut bounds, format);
    }
}

unsafe fn draw(dc: HDC, hwnd: HWND, client: &RECT, dpi: u32, state: &State) {
    unsafe {
        let palette = theme::palette(glass::appearance(), true);
        let s = |v: i32| scaled(v, dpi);
        SetBkMode(dc, TRANSPARENT);
        SelectObject(dc, state.body.into());

        let background = CreateSolidBrush(colorref(palette.window));
        FillRect(dc, client, background);
        let _ = DeleteObject(background.into());
        smooth::stroke_round_rect(
            dc,
            rect(0, 0, client.right - 1, client.bottom - 1),
            s(RADIUS),
            palette.hairline,
            1,
        );

        // Profile card: avatar with the profile's initial, the name and a muted caption. Only the
        // main menu sets it.
        if let Some(card) = &state.menu.header {
            let header = state.layout.header;
            let avatar_top = (header.top + header.bottom - s(AVATAR)) / 2;
            let avatar = rect(
                header.left + s(PAD + 8),
                avatar_top,
                header.left + s(PAD + 8) + s(AVATAR),
                avatar_top + s(AVATAR),
            );
            smooth::fill_ellipse(dc, avatar, card.accent);
            SelectObject(dc, state.heading.into());
            draw_text(
                dc,
                avatar,
                &card.avatar,
                theme::Rgba::hex(0xFFFFFF, 255),
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            let text_left = avatar.right + s(12);
            draw_text(
                dc,
                rect(
                    text_left,
                    header.top + s(16),
                    header.right - s(PAD),
                    header.top + s(40),
                ),
                &card.profile,
                palette.tab_text_active,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
            );
            SelectObject(dc, state.body.into());
            draw_text(
                dc,
                rect(
                    text_left,
                    header.top + s(40),
                    header.right - s(PAD),
                    header.top + s(60),
                ),
                &card.caption,
                palette.omnibox_placeholder,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }

        let current = state.current.get();
        let pressed = state.pressed.get();
        let now = Instant::now();
        let snap = !animations_enabled();
        let mut moving = false;
        for row in &state.layout.rows {
            let row_rect = state.layout.row_rect(row, state.scroll.get());
            match &state.menu.entries[row.entry] {
                Entry::Separator => {
                    let y = (row_rect.top + row_rect.bottom) / 2;
                    smooth::stroke_line(
                        dc,
                        POINT {
                            x: row_rect.left + s(PAD + 8),
                            y,
                        },
                        POINT {
                            x: row_rect.right - s(PAD + 8),
                            y,
                        },
                        palette.hairline,
                        1,
                    );
                }
                Entry::SectionHeader(label) => {
                    draw_text(
                        dc,
                        rect(
                            row_rect.left + s(PAD + 8),
                            row_rect.top,
                            row_rect.right - s(PAD + 8),
                            row_rect.bottom,
                        ),
                        label,
                        palette.omnibox_placeholder,
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                    );
                }
                Entry::Item(item) => {
                    let highlighted = row.selectable && current == Some(row.entry);
                    let held = row.selectable && pressed == Some(row.entry);
                    let cell = &state.icon_motion[row.entry];
                    let mut motion = cell.get();
                    let hover = motion.hover.sample(highlighted as u8 as f32, now, snap);
                    let press = motion.press.sample(held as u8 as f32, now, snap || held);
                    let active = motion.active.sample((item.checked || item.icon == Icon::Check)
                        as u8 as f32, now, snap);
                    moving |= motion.hover.running(now) || motion.press.running(now)
                        || motion.active.running(now);
                    cell.set(motion);
                    let favicon_dc = state
                        .rich_bitmaps
                        .iter()
                        .find(|image| image.entry == row.entry)
                        .map(|image| image.dc);
                    draw_item(
                        dc,
                        row_rect,
                        item,
                        (highlighted, held),
                        (hover, press, active),
                        palette,
                        dpi,
                        favicon_dc,
                    );
                }
            }
        }
        if moving {
            let _ = SetTimer(Some(hwnd), ICON_FRAME_TIMER, ICON_FRAME_MS, None);
        } else {
            let _ = KillTimer(Some(hwnd), ICON_FRAME_TIMER);
        }

        // The footer explains the highlighted row in full, and otherwise shows the version.
        let description = state
            .current
            .get()
            .and_then(|index| match &state.menu.entries[index] {
                Entry::Item(item) => item.description,
                Entry::Separator | Entry::SectionHeader(_) => None,
            });
        draw_text(
            dc,
            rect(
                state.layout.footer.left + s(PAD + 8),
                state.layout.footer.top,
                state.layout.footer.right - s(PAD),
                state.layout.footer.bottom,
            ),
            description.unwrap_or(&state.menu.footer),
            palette.omnibox_placeholder,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
    }
}

/// One row: optional hover or pressed fill, the icon or check colour, the label, and the shortcut
/// or chevron on the right.
unsafe fn draw_item(
    dc: HDC,
    row: RECT,
    item: &Item,
    (highlighted, held): (bool, bool),
    (hover, press, active): (f32, f32, f32),
    palette: &theme::Palette,
    dpi: u32,
    favicon_dc: Option<HDC>,
) {
    unsafe {
        let s = |v: i32| scaled(v, dpi);
        let inner = rect(row.left + s(PAD), row.top, row.right - s(PAD), row.bottom);
        let fill = if !item.enabled {
            None
        } else if held {
            Some(palette.tab_active)
        } else if highlighted {
            Some(palette.tab_hover)
        } else {
            None
        };
        if let Some(fill) = fill {
            smooth::fill_round_rect(dc, inner, s(ROW_RADIUS), fill);
        }
        let (text, muted) = if item.enabled {
            (palette.tab_text_inactive, false)
        } else {
            (palette.omnibox_placeholder, true)
        };
        let text = if highlighted && item.enabled {
            palette.tab_text_active
        } else {
            text
        };
        let glyph_colour = if item.checked || item.icon == Icon::Check {
            palette.progress_fill
        } else {
            text
        };
        if let Some(favicon_dc) = favicon_dc {
            if let Some(favicon) = item.rich_row.as_ref().and_then(|row| row.favicon.as_ref()) {
                let size = favicon.width as i32;
                let centre = (inner.top + inner.bottom) / 2;
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let _ = AlphaBlend(
                    dc,
                    inner.left + s(10),
                    centre - size / 2,
                    favicon.width as i32,
                    favicon.height as i32,
                    favicon_dc,
                    0,
                    0,
                    favicon.width as i32,
                    favicon.height as i32,
                    blend,
                );
            }
        } else if let Some((glyph, accent)) = &item.badge {
            let centre = (inner.top + inner.bottom) / 2;
            let size = s(20);
            let badge = rect(
                inner.left + s(8),
                centre - size / 2,
                inner.left + s(8) + size,
                centre + size / 2,
            );
            smooth::fill_ellipse(dc, badge, *accent);
            draw_text(
                dc,
                badge,
                glyph,
                theme::Rgba::hex(0xFFFFFF, 255),
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        } else if item.icon != Icon::None {
            let centre = (inner.top + inner.bottom) / 2;
            let size = s(16);
            let glyph = rect(
                inner.left + s(10),
                centre - size / 2,
                inner.left + s(10) + size,
                centre - size / 2 + size,
            );
            draw_icon(dc, item.icon, glyph, glyph_colour, hover, press, active);
        }
        let has_hint = item.submenu || item.shortcut.is_some();
        let label_right = if has_hint {
            inner.right - s(HINT_WIDTH + 8)
        } else {
            inner.right - s(10)
        };
        draw_text(
            dc,
            rect(
                inner.left + s(if item.rich_row.is_some() { 40 } else { 36 }),
                inner.top,
                label_right,
                inner.bottom,
            ),
            item.rich_row.as_ref().map_or(&item.label, |row| &row.title),
            text,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        let hint_bounds = rect(
            inner.right - s(HINT_WIDTH),
            inner.top,
            inner.right - s(10),
            inner.bottom,
        );
        if item.submenu {
            draw_text(
                dc,
                hint_bounds,
                "›",
                text,
                DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        } else if let Some(shortcut) = item.shortcut {
            let colour = if muted {
                palette.omnibox_placeholder
            } else {
                palette.omnibox_completion
            };
            draw_text(
                dc,
                hint_bounds,
                shortcut,
                colour,
                DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
    }
}

/// Draws the Clean icon set on its shared 24-unit grid. GDI+ still antialiases each stroke.
unsafe fn draw_icon(dc: HDC, icon: Icon, r: RECT, colour: Rgba,
    hover: f32, press: f32, active: f32) {
    unsafe {
        let width = (r.right - r.left).max(1);
        let height = (r.bottom - r.top).max(1);
        let stroke = ((width as f32 * 1.5 / 24.0).round() as i32).max(1);
        let resting_scale = 1.0 + 0.06 * hover + 0.02 * active;
        let scale = resting_scale + (0.96 - resting_scale) * press;
        let angle = if icon == Icon::Reset { press * 0.61 } else { 0.0_f32 };
        let grid = |px: f32, py: f32| {
            let x = (px - 12.0) * scale;
            let y = (py - 12.0) * scale;
            POINT {
                x: r.left + ((12.0 + x * angle.cos() - y * angle.sin())
                    * width as f32 / 24.0).round() as i32,
                y: r.top + ((12.0 + x * angle.sin() + y * angle.cos())
                    * height as f32 / 24.0).round() as i32,
            }
        };
        let boxed = |x0: f32, y0: f32, x1: f32, y1: f32| {
            let a = grid(x0, y0);
            let b = grid(x1, y1);
            rect(a.x, a.y, b.x, b.y)
        };
        let line = |x0, y0, x1, y1| {
            rounded_icon_line(dc, grid(x0, y0), grid(x1, y1), colour, stroke);
        };
        let path = |coords: &[(f32, f32)]| {
            for pair in coords.windows(2) {
                rounded_icon_line(dc, grid(pair[0].0, pair[0].1),
                    grid(pair[1].0, pair[1].1), colour, stroke);
            }
        };
        let circle = |cx: f32, cy: f32, radius: f32| {
            smooth::stroke_ellipse(dc, boxed(cx-radius, cy-radius, cx+radius, cy+radius), colour, stroke);
        };
        let round_rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
            smooth::stroke_round_rect(dc, boxed(x0, y0, x1, y1),
                ((width as f32 * 4.0 / 24.0).round() as i32).max(2), colour, stroke);
        };
        match icon {
            Icon::None => {}
            Icon::Plus => { line(12.0, 4.0, 12.0, 20.0); line(4.0, 12.0, 20.0, 12.0); }
            Icon::Person => {
                circle(12.0, 7.0, 3.5);
                path(&[(4.5, 20.0), (4.5, 18.0)]);
                let shoulders: [(f32, f32); 13] = std::array::from_fn(|i| {
                    let t = std::f32::consts::PI * (1.0 - i as f32 / 12.0);
                    (12.0 + 7.5 * t.cos(), 18.0 - 7.5 * t.sin())
                });
                path(&shoulders);
                line(19.5, 18.0, 19.5, 20.0);
            }
            Icon::Grid => {
                for (x, y) in [(3.0, 3.0), (13.0, 3.0), (3.0, 13.0), (13.0, 13.0)] {
                    round_rect(x, y, x + 8.0, y + 8.0);
                }
            }
            Icon::Key => {
                circle(7.5, 11.0, 4.0);
                path(&[(11.5, 11.0), (21.0, 11.0), (21.0, 14.0)]);
                line(17.0, 11.0, 17.0, 14.0);
            }
            Icon::Shield => {
                let points = [(12.0, 2.5), (20.0, 6.0), (20.0, 11.0), (18.0, 16.5), (12.0, 21.5), (6.0, 16.5), (4.0, 11.0), (4.0, 6.0), (12.0, 2.5)]
                    .map(|(x,y)| grid(x,y));
                polyline(dc, &points, colour, stroke);
            }
            Icon::Gear => {
                let points = [(10.0,2.5),(14.0,2.5),(14.5,5.0),(17.0,6.0),(19.0,4.5),(21.5,7.5),(19.5,9.5),(20.0,12.0),(21.5,14.5),(19.0,17.5),(17.0,16.5),(14.5,18.0),(14.0,21.5),(10.0,21.5),(9.5,18.0),(7.0,16.5),(5.0,17.5),(2.5,14.5),(4.5,12.0),(4.5,9.5),(2.5,7.5),(5.0,4.5),(7.0,6.0),(9.5,5.0),(10.0,2.5)]
                    .map(|(x,y)| grid(x,y));
                polyline(dc, &points, colour, stroke);
                circle(12.0, 12.0, 3.0);
            }
            Icon::Close => { line(6.0, 6.0, 18.0, 18.0); line(18.0, 6.0, 6.0, 18.0); }
            Icon::Check => {
                let reveal = active;
                if reveal <= 0.0 { return; }
                let first = (reveal * 2.0).min(1.0);
                let second = (reveal * 2.0 - 1.0).clamp(0.0, 1.0);
                line(4.0, 12.0, 4.0 + 5.5 * first, 12.0 + 5.5 * first);
                if second > 0.0 { line(9.5, 17.5,
                    9.5 + 10.5 * second, 17.5 - 11.0 * second); }
            }
            Icon::Lock | Icon::Unlock => {
                round_rect(4.0, 10.0, 20.0, 21.0);
                let open = icon == Icon::Unlock;
                line(7.0, 10.0, 7.0, 7.5);
                let mut prior = (7.0, 7.5);
                for next in arc(12.0, 7.5, 5.0, std::f32::consts::PI,
                    if open { std::f32::consts::PI / 3.0 } else { 0.0 }) {
                    line(prior.0, prior.1, next.0, next.1);
                    prior = next;
                }
                if !open { line(prior.0, prior.1, 17.0, 10.0); }
            }
            Icon::Warning => {
                let triangle = [grid(12.0, 3.0), grid(21.0, 20.0), grid(3.0, 20.0), grid(12.0, 3.0)];
                polyline(dc, &triangle, colour, stroke);
                line(12.0, 9.0, 12.0, 14.0);
                smooth::fill_ellipse(dc, boxed(11.0, 16.5, 13.0, 18.5), colour);
            }
            Icon::Info => {
                circle(12.0, 12.0, 9.0);
                smooth::fill_ellipse(dc, boxed(11.0, 6.0, 13.0, 8.0), colour);
                line(12.0, 11.0, 12.0, 17.0);
            }
            Icon::Mic => {
                round_rect(9.0, 3.0, 15.0, 15.0);
                let mut prior = (19.0, 12.0);
                for next in arc(12.0, 12.0, 7.0, 0.0, -std::f32::consts::PI) {
                    line(prior.0, prior.1, next.0, next.1); prior = next;
                }
                line(12.0, 19.0, 12.0, 21.0);
                line(8.0, 21.0, 16.0, 21.0);
            }
            Icon::Camera => {
                round_rect(3.0, 6.0, 16.0, 18.0);
                path(&[(16.0, 10.0), (21.0, 7.0), (21.0, 17.0), (16.0, 14.0)]);
            }
            Icon::Location => {
                let mut prior = (5.0, 10.0);
                for next in arc(12.0, 10.0, 7.0, std::f32::consts::PI, 0.0) {
                    line(prior.0, prior.1, next.0, next.1); prior = next;
                }
                line(prior.0, prior.1, 12.0, 21.0);
                line(12.0, 21.0, 5.0, 10.0);
                circle(12.0, 10.0, 2.5);
            }
            Icon::Bell => {
                path(&[(5.0,17.0),(6.5,15.0),(6.5,10.0)]);
                let mut prior = (6.5, 10.0);
                for next in arc(12.0, 10.0, 5.5, std::f32::consts::PI, 0.0) {
                    line(prior.0, prior.1, next.0, next.1); prior = next;
                }
                path(&[(17.5,10.0),(17.5,15.0),(19.0,17.0),(5.0,17.0)]);
                path(&[(10.0,20.0),(11.0,21.0),(13.0,21.0),(14.0,20.0)]);
            }
            Icon::Cookie => {
                circle(12.0, 12.0, 9.0);
                for (x, y) in [(8.0, 9.0), (15.5, 10.0), (10.0, 16.0)] {
                    smooth::fill_ellipse(dc, boxed(x-1.0,y-1.0,x+1.0,y+1.0), colour);
                }
            }
            Icon::Reset => {
                let mut prior = (20.0, 12.0);
                for next in arc(12.0, 12.0, 8.0, 0.0, -5.75) {
                    line(prior.0, prior.1, next.0, next.1); prior = next;
                }
                path(&[(19.0,3.0),(19.0,8.0),(14.0,8.0)]);
            }
        }
    }
}

/// Points along the arc of a circle centred at (`cx`, `cy`) in 24-unit grid space, from angle
/// `from` to `to` (radians, y downwards). Used to build the glyph outlines.
fn arc(cx: f32, cy: f32, radius: f32, from: f32, to: f32) -> impl Iterator<Item = (f32, f32)> {
    const STEPS: usize = 12;
    (0..=STEPS).map(move |step| {
        let t = from + (to - from) * step as f32 / STEPS as f32;
        (cx + radius * t.cos(), cy - radius * t.sin())
    })
}

/// Strokes consecutive segments of an open path.
unsafe fn polyline(dc: HDC, points: &[POINT], colour: Rgba, stroke: i32) {
    unsafe {
        for pair in points.windows(2) {
            rounded_icon_line(dc, pair[0], pair[1], colour, stroke);
        }
    }
}

/// Add circular caps at the rasterised endpoints; GDI+'s default pen uses flat caps.
unsafe fn rounded_icon_line(dc: HDC, from: POINT, to: POINT, colour: Rgba, stroke: i32) {
    unsafe {
        smooth::stroke_line(dc, from, to, colour, stroke);
        for end in [from, to] {
            let left = end.x - stroke / 2;
            let top = end.y - stroke / 2;
            smooth::fill_ellipse(
                dc,
                rect(left, top, left + stroke, top + stroke),
                colour,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_list_positions_are_rejected() {
        let built = [3u32, 5, 8];
        assert_eq!(still_listed(&built, &[3, 5, 8], 1), Some(&5));
        // The workspace or tab at that position went away while the menu was open.
        assert_eq!(still_listed(&built, &[3, 8], 1), None);
        // Something else moved into the slot: the built entry is still what the pick means.
        assert_eq!(still_listed(&built, &[8, 3], 0), Some(&3));
        assert_eq!(still_listed(&built, &[3, 5, 8], 3), None);
    }

    #[test]
    fn icon_motion_reaches_target_and_stops() {
        let start = Instant::now();
        let mut motion = IconMotion::new(0.0, start);
        assert_eq!(motion.sample(1.0, start, false), 0.0);
        let middle = start + Duration::from_millis(75);
        assert!(motion.sample(1.0, middle, false) > 0.5);
        assert!(motion.running(middle));
        let end = start + ICON_EASE;
        assert_eq!(motion.sample(1.0, end, false), 1.0);
        assert!(!motion.running(end));
    }

    #[test]
    fn icon_motion_reduced_motion_snaps_to_end() {
        let start = Instant::now();
        let mut motion = IconMotion::new(0.0, start);
        assert_eq!(motion.sample(1.0, start, true), 1.0);
        assert!(!motion.running(start));
        assert_eq!(motion.sample(0.0, start, true), 0.0);
    }

    fn item(id: usize, enabled: bool) -> Entry {
        let item = Item::new(id, format!("Item {id}"), Icon::None);
        Entry::Item(if enabled { item } else { item.disabled() })
    }

    /// A popup as every caller other than the main menu builds it: no profile header.
    fn menu(entries: Vec<Entry>) -> Menu {
        Menu {
            header: None,
            entries,
            footer: "Bowser test".to_owned(),
        }
    }

    /// The main ⋮ menu's shape, with the profile card above the list.
    fn main_menu(entries: Vec<Entry>) -> Menu {
        Menu {
            header: Some(ProfileHeader {
                profile: "Default".to_owned(),
                caption: "Current profile".to_owned(),
                avatar: "D".to_owned(),
                accent: theme::Rgba::hex(0x4C83D5, 255),
            }),
            entries,
            footer: "Bowser test".to_owned(),
        }
    }

    fn sample() -> Vec<Entry> {
        vec![
            item(1, true),
            Entry::Separator,
            item(2, false),
            item(3, true),
        ]
    }

    #[test]
    fn initial_selection_uses_only_the_requested_enabled_item() {
        let entries = sample();
        assert_eq!(initial_item_index(&entries, Some(3)), Some(3));
        assert_eq!(initial_item_index(&entries, Some(2)), None);
        assert_eq!(initial_item_index(&entries, None), None);
    }

    #[test]
    fn down_skips_separators_and_disabled_rows_and_wraps() {
        let entries = sample();
        assert_eq!(step(&entries, None, true), Some(0));
        assert_eq!(step(&entries, Some(0), true), Some(3));
        assert_eq!(step(&entries, Some(3), true), Some(0));
    }

    #[test]
    fn up_skips_disabled_rows_and_wraps() {
        let entries = sample();
        assert_eq!(step(&entries, None, false), Some(3));
        assert_eq!(step(&entries, Some(3), false), Some(0));
        assert_eq!(step(&entries, Some(0), false), Some(3));
    }

    #[test]
    fn nothing_is_selectable_when_every_row_is_disabled_or_empty() {
        assert_eq!(step(&[], None, true), None);
        let entries = vec![
            Entry::SectionHeader("Profiles".to_owned()),
            item(1, false),
            Entry::Separator,
        ];
        assert_eq!(step(&entries, None, true), None);
        assert_eq!(step(&entries, Some(0), false), None);
    }

    #[test]
    fn submenu_flips_left_and_clamps_vertically_at_screen_edges() {
        let work = rect(0, 0, 1000, 700);
        let right = place_submenu(POINT { x: 300, y: 100 }, 280, 300, work);
        assert_eq!((right.x, right.y), (300, 100));

        let flipped = place_submenu(POINT { x: 900, y: 600 }, 280, 300, work);
        assert_eq!((flipped.x, flipped.y), (620, 400));
    }

    #[test]
    fn context_menu_flips_above_and_left_at_work_area_edges() {
        let work = rect(0, 0, 1000, 700);
        let below = place_below(POINT { x: 100, y: 40 }, 280, 300, work);
        assert_eq!((below.x, below.y), (100, 40));

        let flipped = place_below(POINT { x: 900, y: 600 }, 280, 300, work);
        assert_eq!((flipped.x, flipped.y), (620, 300));
    }

    #[test]
    fn section_headers_are_never_selectable() {
        let entries = vec![
            Entry::SectionHeader("Profiles".to_owned()),
            item(1, false),
            item(2, true),
        ];
        assert_eq!(step(&entries, None, true), Some(2));
        assert_eq!(step(&entries, Some(2), true), Some(2));
    }

    #[test]
    fn profile_header_is_absent_by_default_and_only_the_main_menu_shows_it() {
        let plain = Layout::new(&menu(sample()), 96, 700);
        assert_eq!(plain.header.top, plain.header.bottom);
        assert_eq!(plain.rows[0].rect.top, LIST_GAP);

        let main = Layout::new(&main_menu(sample()), 96, 700);
        assert_eq!(main.header.bottom - main.header.top, HEADER_HEIGHT);
        assert_eq!(main.rows[0].rect.top, HEADER_HEIGHT + LIST_GAP);
        assert!(main.height > plain.height);
    }

    #[test]
    fn hit_testing_finds_only_enabled_rows() {
        let menu = main_menu(sample());
        let layout = Layout::new(&menu, 96, 700);
        let first = layout.rows[0].rect;
        let separator = layout.rows[1].rect;
        let disabled = layout.rows[2].rect;
        assert_eq!(layout.entry_at(first.left + 5, first.top + 5, 0), Some(0));
        assert_eq!(layout.entry_at(separator.left + 5, separator.top, 0), None);
        assert_eq!(
            layout.entry_at(disabled.left + 5, disabled.top + 5, 0),
            None
        );
        assert_eq!(layout.entry_at(-1, -1, 0), None);
        assert_eq!(
            layout.entry_at(layout.header.left + 5, layout.header.top + 5, 0),
            None
        );
    }

    #[test]
    fn layout_scales_rows_and_keeps_the_footer_below_the_list() {
        let menu = menu(sample());
        let at_96 = Layout::new(&menu, 96, 700);
        let at_144 = Layout::new(&menu, 144, 700);
        assert_eq!(at_96.width, WIDTH);
        assert_eq!(at_144.width, WIDTH * 3 / 2);
        assert_eq!(
            at_144.rows[0].rect.bottom - at_144.rows[0].rect.top,
            ROW_HEIGHT * 3 / 2
        );
        let last = at_96.rows.last().expect("rows").rect;
        assert!(at_96.footer.top > last.bottom);
        assert_eq!(at_96.height, at_96.footer.bottom);
    }

    #[test]
    fn long_menu_layout_caps_to_work_area_and_scrolls_selection_into_view() {
        let entries = (0..20).map(|id| item(id, true)).collect();
        let menu = menu(entries);
        let layout = Layout::new(&menu, 96, 180);
        assert_eq!(layout.height, 180);
        assert!(layout.max_scroll > 0);
        let scroll = layout.ensure_visible(19, 0);
        let last = layout.row_rect(layout.rows.last().unwrap(), scroll);
        assert_eq!(last.bottom, layout.list_bottom);
        assert!(layout.footer.top > last.bottom);
    }

    #[test]
    fn placement_opens_below_the_anchor_and_clamps_to_the_work_area() {
        let work = rect(0, 0, 1000, 700);
        let inside = place(POINT { x: 900, y: 40 }, 280, 300, work);
        assert_eq!((inside.x, inside.y), (620, 40));
        let past_bottom = place(POINT { x: 900, y: 600 }, 280, 300, work);
        assert_eq!(past_bottom.y, 400);
        let past_left = place(POINT { x: 100, y: 40 }, 280, 300, work);
        assert_eq!(past_left.x, 0);
    }

    #[test]
    fn a_selection_moves_only_between_selectable_rows_when_the_menu_is_rebuilt() {
        let before = menu(sample());
        let after = menu(vec![item(9, true), Entry::Separator, item(8, true)]);
        assert_eq!(step(&before.entries, Some(3), true), Some(0));
        assert_eq!(step(&after.entries, Some(0), true), Some(2));
    }

    #[test]
    fn a_press_on_a_parent_popup_closes_only_the_flyout() {
        let parent = [rect(0, 0, 320, 400)];
        let inside_parent = POINT { x: 10, y: 10 };
        let outside_all = POINT { x: 400, y: 10 };
        assert_eq!(
            outside_press(&parent, inside_parent),
            OutsidePress::CloseFlyout
        );
        assert_eq!(
            outside_press(&parent, outside_all),
            OutsidePress::CloseChain
        );
    }

    #[test]
    fn a_press_at_the_top_level_closes_the_chain() {
        assert_eq!(
            outside_press(&[], POINT { x: 10, y: 10 }),
            OutsidePress::CloseChain
        );
    }

    #[test]
    fn back_from_a_flyout_resumes_the_parent_and_anything_else_closes_the_chain() {
        assert_eq!(after_flyout(MenuResult::Back), None);
        assert_eq!(
            after_flyout(MenuResult::Dismissed),
            Some(MenuResult::Dismissed)
        );
        let chosen = Chosen {
            id: 4,
            row: rect(0, 0, 10, 10),
        };
        assert_eq!(
            after_flyout(MenuResult::Chosen(chosen)),
            Some(MenuResult::Chosen(chosen))
        );
    }

    #[test]
    fn a_flyout_holding_focus_or_capture_does_not_dismiss_its_parent() {
        assert!(!dismisses_on_focus_loss(true));
        assert!(dismisses_on_focus_loss(false));
    }

    #[test]
    fn tracking_state_covers_nested_popups_and_clears_on_drop() {
        assert!(!is_tracking());
        let outer = TrackingGuard::new();
        assert!(is_tracking());
        let inner = TrackingGuard::new();
        drop(outer);
        assert!(is_tracking());
        drop(inner);
        assert!(!is_tracking());
    }
}
