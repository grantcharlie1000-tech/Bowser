//! History rail panel: day-grouped entries with search, per-entry removal, per-site removal and a
//! ranged clear. The view-model is pure. It turns `productivity` history entries into rows and turns
//! input into [`Action`]s. The window layer runs each action against the history file.

use super::panel_widgets::{self as w, Key, Metrics};
use crate::productivity::HistoryEntry;

/// Gap, in local seconds, that Today's bucket and the clear ranges are measured from.
const DAY: u64 = 86_400;

/// One history entry, shaped for display. `index` is its position in [`Model::lines`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    pub timestamp: u64,
    pub url: String,
    pub title: String,
    pub host: String,
    /// `HH:MM` in local time.
    pub clock: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    /// Day heading with its entry count: `Today`, `Yesterday` or a `YYYY-MM-DD` date.
    Header { label: String, count: usize },
    /// Index into [`Model::lines`].
    Entry(usize),
}

/// How far back a clear reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Range {
    LastHour,
    Today,
    All,
}

impl Range {
    pub(crate) const ALL: [Range; 3] = [Range::LastHour, Range::Today, Range::All];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::LastHour => "Last hour",
            Self::Today => "Today",
            Self::All => "All time",
        }
    }

    /// Inclusive `(start, end)` Unix seconds the range covers, or `None` for everything.
    pub(crate) fn bounds(self, now: u64, offset_secs: i64) -> Option<(u64, u64)> {
        match self {
            Self::LastHour => Some((now.saturating_sub(3600), now)),
            Self::Today => Some((start_of_local_day(now, offset_secs), u64::MAX)),
            Self::All => None,
        }
    }
}

/// The first second of the local day containing `now`, in Unix seconds.
pub(crate) fn start_of_local_day(now: u64, offset_secs: i64) -> u64 {
    let local = (now as i64 + offset_secs).max(0) as u64;
    let midnight_local = local / DAY * DAY;
    (midnight_local as i64 - offset_secs).max(0) as u64
}

/// What the window must do. The model never touches the history file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Nothing,
    Redraw,
    Open { url: String, new_tab: bool },
    /// Remove every visit to one page, by URL. History keeps no per-visit id, so this is the finest
    /// removal it supports.
    RemoveEntry { url: String },
    /// Remove every visit to a site, by host. The window confirms first.
    RemoveSite { host: String },
    Dismiss,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Model {
    pub query: String,
    pub lines: Vec<Line>,
    pub rows: Vec<Row>,
    /// Selected entry, as an index into `lines`.
    pub selected: Option<usize>,
    /// Row under the pointer, for the remove button.
    pub hover: Option<usize>,
    pub scroll: i32,
}

/// Entries shown when there is no query, so the list stays responsive.
pub(crate) const LIMIT: usize = 500;

/// Builds the view from the entries the window fetched (`search_history` with the current query).
/// Day buckets use local time: `offset_secs` shifts each timestamp before grouping, so a visit
/// at 23:30 local is filed under the local day it happened on.
pub(crate) fn build_lines(entries: &[HistoryEntry], offset_secs: i64) -> Vec<Line> {
    let mut lines: Vec<Line> = entries
        .iter()
        .map(|(timestamp, url, title)| Line {
            timestamp: *timestamp,
            url: url.clone(),
            title: title.clone(),
            host: w::host_label(url),
            clock: w::clock_label(*timestamp, offset_secs),
        })
        .collect();
    lines.sort_by_key(|l| std::cmp::Reverse(l.timestamp));
    lines
}

/// Day-grouped rows. Each line appears once, under the local day of its timestamp. Days run newest
/// first and so do the lines inside them. `Line` order is kept, so line index and display order
/// agree.
pub(crate) fn group_rows(lines: &[Line], now: u64, offset_secs: i64) -> Vec<Row> {
    let today = days_of(now, offset_secs);
    let mut rows = Vec::with_capacity(lines.len() + 8);
    let mut current: Option<(i64, usize)> = None;
    for (index, line) in lines.iter().enumerate() {
        let day = days_of(line.timestamp, offset_secs);
        match current {
            Some((open_day, header)) if open_day == day => {
                if let Row::Header { count, .. } = &mut rows[header] {
                    *count += 1;
                }
            }
            _ => {
                rows.push(Row::Header {
                    label: day_label(day, today),
                    count: 1,
                });
                current = Some((day, rows.len() - 1));
            }
        }
        rows.push(Row::Entry(index));
    }
    rows
}

fn days_of(timestamp: u64, offset_secs: i64) -> i64 {
    w::shift_to_local(timestamp, offset_secs) as i64 / DAY as i64
}

fn day_label(day: i64, today: i64) -> String {
    match today - day {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ => civil_date(day),
    }
}

/// `YYYY-MM-DD` for a count of days since 1970-01-01.
pub(crate) fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

impl Model {
    pub(crate) fn rebuild(&mut self, entries: &[HistoryEntry], now: u64, offset_secs: i64) {
        self.lines = build_lines(entries, offset_secs);
        self.rows = group_rows(&self.lines, now, offset_secs);
        self.hover = None;
        if self.selected.is_some_and(|i| i >= self.lines.len()) {
            self.selected = None;
        }
    }

    pub(crate) fn selected_row(&self) -> Option<usize> {
        let line = self.selected?;
        self.rows.iter().position(|r| *r == Row::Entry(line))
    }

    pub(crate) fn empty_text(&self) -> Option<&'static str> {
        if !self.lines.is_empty() {
            None
        } else if self.query.trim().is_empty() {
            Some("Nothing in your history yet. Pages you visit appear here.")
        } else {
            Some("No matches. Try a shorter word or check the spelling.")
        }
    }

    pub(crate) fn reveal_selection(&mut self, m: Metrics) {
        if let Some(row) = self.selected_row() {
            self.scroll = w::scroll_to_show(self.scroll, row, self.rows.len(), m);
        }
    }

    pub(crate) fn set_query(&mut self, query: &str) {
        self.query = query.to_owned();
        self.scroll = 0;
    }

    /// Movement walks entries only, so headers are skipped. Scrolling follows the chosen entry.
    pub(crate) fn key(&mut self, key: Key, new_tab: bool, m: Metrics) -> Action {
        match key {
            Key::Escape => {
                if self.query.is_empty() {
                    Action::Dismiss
                } else {
                    self.set_query("");
                    Action::Redraw
                }
            }
            Key::Up | Key::Down | Key::Home | Key::End | Key::PageUp | Key::PageDown => {
                let page = (m.height / m.row.max(1)).max(1) as usize;
                let current = self.selected;
                match w::moved_selection(key, current, self.lines.len(), page) {
                    Some(line) => {
                        self.selected = Some(line);
                        self.reveal_selection(m);
                        Action::Redraw
                    }
                    None => Action::Nothing,
                }
            }
            Key::Enter => match self.selected.and_then(|i| self.lines.get(i)) {
                Some(line) => Action::Open {
                    url: line.url.clone(),
                    new_tab,
                },
                None => Action::Nothing,
            },
            Key::Delete => match self.selected.and_then(|i| self.lines.get(i)) {
                Some(line) => Action::RemoveEntry {
                    url: line.url.clone(),
                },
                None => Action::Nothing,
            },
            Key::F2 | Key::Left | Key::Right => Action::Nothing,
        }
    }

    /// Pointer click on a row. Headers do nothing. Entries are selected and opened.
    pub(crate) fn click(&mut self, row: usize, new_tab: bool, m: Metrics) -> Action {
        match self.rows.get(row) {
            Some(Row::Entry(line)) => {
                let line = *line;
                self.selected = Some(line);
                self.reveal_selection(m);
                Action::Open {
                    url: self.lines[line].url.clone(),
                    new_tab,
                }
            }
            _ => Action::Nothing,
        }
    }

    /// Drops the entry on `row` from the list (window applies it to the file).
    pub(crate) fn remove_at(&self, row: usize) -> Action {
        match self.rows.get(row) {
            Some(Row::Entry(line)) => Action::RemoveEntry {
                url: self.lines[*line].url.clone(),
            },
            _ => Action::Nothing,
        }
    }

    /// Host of the entry on `row`, for the "remove everything from this site" choice.
    pub(crate) fn site_at(&self, row: usize) -> Option<String> {
        match self.rows.get(row) {
            Some(Row::Entry(line)) if !self.lines[*line].host.is_empty() => {
                Some(self.lines[*line].host.clone())
            }
            _ => None,
        }
    }

    /// Where a row's remove button sits in this panel. Headers have none.
    pub(crate) fn has_remove_button(&self, row: usize) -> bool {
        matches!(self.rows.get(row), Some(Row::Entry(_)))
    }

    pub(crate) fn scroll_by(&mut self, delta: i32, m: Metrics) {
        self.scroll = w::clamp_scroll(self.scroll + delta, self.rows.len(), m);
    }

}

// ---- Window layer ---------------------------------------------------------------------------

use std::{path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

use windows::Win32::{
    Foundation::RECT,
    Graphics::Gdi::{DeleteObject, HDC, HFONT, SelectObject, SetBkMode, TRANSPARENT},
    System::SystemInformation::{GetLocalTime, GetSystemTime},
    UI::Input::KeyboardAndMouse::{GetKeyState, ReleaseCapture, SetCapture, VK_APPS, VK_CONTROL, VK_F10},
};

use super::{
    menu::{self, Icon},
    panel_widgets::{EditEvent, EditField, EditSink, IconCache, PointerSink, Surface},
    smooth,
};
use crate::{
    app::{App, AppRef},
    productivity,
    profile::{self, Profile},
    theme::{Palette, Rgba},
    win32::{ask_yes_no_owned, show_error_message, side_panel::{self, PanelContent, PanelId}},
};

const SEARCH: usize = 0;
const ROW_DIP: i32 = 40;
const SEARCH_DIP: i32 = 30;
const TOOLBAR_DIP: i32 = 26;
const PAD_DIP: i32 = 12;
const GAP_DIP: i32 = 6;
const ACTION_DIP: i32 = 22;
const LINE_DIP: i32 = 18;
const DRAG_DIP: i32 = 4;

thread_local! {
    static LAST_OFFSET: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

/// Registers the history panel with the host. Call once at startup, on the UI thread.
pub(crate) fn register(app: &AppRef) {
    let app = app.clone();
    side_panel::register_content(PanelId::History, move || Box::new(Content::new(app.clone())));
}

/// Local time offset from UTC in seconds, from the two system clocks. Daylight saving follows the
/// clock at the moment of the reading, so a window open across a change may group a day off by an hour.
fn local_offset() -> i64 {
    let local = unsafe { GetLocalTime() };
    let utc = unsafe { GetSystemTime() };
    let fields = |t: windows::Win32::Foundation::SYSTEMTIME| {
        [
            i64::from(t.wYear),
            i64::from(t.wMonth),
            i64::from(t.wDay),
            i64::from(t.wHour),
            i64::from(t.wMinute),
            i64::from(t.wSecond),
        ]
    };
    w::local_offset_secs(fields(local), fields(utc))
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Btn {
    Clear,
    Remove(usize),
}

pub(crate) struct Content {
    app: AppRef,
    profile: Option<Profile>,
    history_file: Option<PathBuf>,
    model: Model,
    surface: Surface,
    font: HFONT,
    font_dpi: u32,
    search: Option<EditField>,
    icons: IconCache,
    viewport: i32,
    press: Option<(usize, i32, i32)>,
    message: Option<String>,
}

impl Drop for Content {
    fn drop(&mut self) {
        self.search = None;
        if !self.surface.host.is_invalid() {
            w::unregister_right_click(self.surface.host);
        }
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
        let history_file = profile
            .as_ref()
            .filter(|p| !p.is_guest() && p.history_enabled())
            .map(|p| p.history_file());
        let mut content = Self {
            app,
            profile,
            history_file,
            model: Model::default(),
            surface: Surface::default(),
            font: HFONT::default(),
            font_dpi: 0,
            search: None,
            icons: IconCache::default(),
            viewport: 0,
            press: None,
            message: None,
        };
        content.message = content.off_reason();
        content.reload();
        content
    }

    /// Why nothing is recorded, when that is the case.
    fn off_reason(&self) -> Option<String> {
        match &self.profile {
            Some(p) if p.is_guest() => Some("History is not kept in guest windows.".into()),
            Some(p) if !p.history_enabled() => Some("History is turned off for this profile.".into()),
            Some(_) => None,
            None => Some("History is unavailable right now.".into()),
        }
    }

    fn px(&self, dip: i32) -> i32 {
        self.surface.px(dip)
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
            if let Some(search) = &self.search { search.set_font(new_font); }
            if !old_font.is_invalid() { unsafe { let _ = DeleteObject(old_font.into()); } }
        }
        if self.search.is_none() {
            let sink: *mut dyn EditSink = self as *mut Content;
            let search = EditField::new(self.surface.host, sink, SEARCH, self.font);
            search.set_cue("Search history");
            self.search = Some(search);
            let pointer: *mut dyn PointerSink = self as *mut Content;
            w::register_right_click(self.surface.host, pointer);
        }
    }

    fn width(&self) -> i32 {
        (self.surface.origin.right - self.surface.origin.left).max(0)
    }

    fn geometry(&self) -> Geometry {
        let p = |v| self.px(v);
        let pad = p(PAD_DIP);
        let gap = p(GAP_DIP);
        let width = self.width();
        let search = RECT { left: pad, top: pad, right: width - pad, bottom: pad + p(SEARCH_DIP) };
        let toolbar_top = search.bottom + gap;
        let clear = RECT {
            left: width - pad - p(190),
            top: toolbar_top,
            right: width - pad,
            bottom: toolbar_top + p(TOOLBAR_DIP),
        };
        let mut y = toolbar_top + p(TOOLBAR_DIP) + gap;
        let status = self.message.as_ref().map(|_| {
            let rect = RECT { left: pad, top: y, right: width - pad, bottom: y + p(LINE_DIP) };
            y += p(LINE_DIP) + gap;
            rect
        });
        Geometry { search, clear, status, list_top: y + gap, pad, row: p(ROW_DIP) }
    }

    fn list_metrics(&self, g: &Geometry) -> w::Metrics {
        w::Metrics { row: g.row, height: (self.viewport - g.list_top).max(0) }
    }

    fn row_rect(&self, g: &Geometry, index: usize) -> RECT {
        let m = self.list_metrics(g);
        let top = g.list_top + w::row_top(index, self.model.scroll, m);
        RECT { left: g.pad, top, right: self.width() - g.pad, bottom: top + g.row }
    }

    fn action_rects(&self, row: RECT) -> [RECT; 1] {
        let size = self.px(ACTION_DIP);
        let right = row.right - self.px(6);
        let top = row.top + (row.bottom - row.top - size) / 2;
        [RECT { left: right - size, top, right, bottom: top + size }]
    }

    fn hit(&self, x: i32, y: i32) -> Option<Target> {
        let g = self.geometry();
        if w::contains(g.clear, x, y) {
            return Some(Target::Button(Btn::Clear));
        }
        if y < g.list_top {
            return None;
        }
        let m = self.list_metrics(&g);
        let index = w::row_at(y - g.list_top, self.model.scroll, self.model.rows.len(), m)?;
        if self.model.has_remove_button(index) {
            let row = self.row_rect(&g, index);
            let [remove] = self.action_rects(row);
            if w::contains(remove, x, y) {
                return Some(Target::Button(Btn::Remove(index)));
            }
        }
        Some(Target::Row(index))
    }

    fn reload(&mut self) {
        let now = now_secs();
        let offset = local_offset();
        LAST_OFFSET.with(|c| c.set(offset));
        let entries = match &self.history_file {
            Some(path) => productivity::search_history(path, &self.model.query, LIMIT),
            None => Vec::new(),
        };
        self.model.rebuild(&entries, now, offset);
        self.surface.invalidate();
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Nothing => return,
            Action::Redraw => {}
            Action::Open { url, new_tab } => self.open(&url, new_tab),
            Action::RemoveEntry { url } => {
                if let Some(path) = &self.history_file {
                    let result = productivity::delete_history_entry(path, &url);
                    self.finish(result.map(|_| ()));
                }
            }
            Action::RemoveSite { host } => {
                if ask_yes_no_owned(self.surface.owner(), &format!("Remove every visit to {host} from history?")) {
                    if let Some(path) = &self.history_file {
                        let result = productivity::delete_history_for_host(path, &host);
                        self.finish(result.map(|_| ()));
                    }
                }
            }
            Action::Dismiss => {
                side_panel::close(self.surface.owner());
                return;
            }
        }
        self.surface.invalidate();
    }

    fn open(&mut self, url: &str, new_tab: bool) {
        let Ok(url) = url::Url::parse(url) else {
            return;
        };
        if new_tab {
            App::new_tab(&self.app);
        }
        App::navigate_current(&self.app, &url);
    }

    /// Clears a range after confirmation. `All` clears the file, and the others delete a time span.
    fn clear(&mut self, range: Range) {
        let Some(path) = self.history_file.clone() else {
            return;
        };
        if !ask_yes_no_owned(self.surface.owner(), &format!("Clear {} of browsing history? This cannot be undone.", range.label().to_lowercase())) {
            return;
        }
        let now = now_secs();
        let offset = LAST_OFFSET.with(std::cell::Cell::get);
        let result = match range.bounds(now, offset) {
            None => productivity::clear_history(&path),
            Some((start, end)) => productivity::delete_history_range(&path, start, end).map(|_| ()),
        };
        self.finish(result);
    }

    fn finish(&mut self, result: std::io::Result<()>) {
        match result {
            Ok(()) => self.message = self.off_reason(),
            Err(error) => {
                show_error_message(&format!("History could not be changed: {error}"));
                self.message = Some("History could not be changed.".into());
            }
        }
        self.reload();
    }

    fn open_range_menu(&mut self) {
        let entries: Vec<menu::Entry> = Range::ALL
            .iter()
            .enumerate()
            .map(|(k, r)| w::item(100 + k, r.label(), Icon::None))
            .collect();
        let owner = self.surface.owner();
        let cursor = self.surface.cursor();
        let Some(id) = w::context_menu(owner, cursor, entries) else {
            return;
        };
        if let Some(range) = Range::ALL.get(id.wrapping_sub(100)).copied() {
            self.clear(range);
        }
    }

    /// Context menu for the entry on `row`, or the clear menu alone when the pointer is not on an entry.
    fn show_menu(&mut self, row: Option<usize>, x: i32, y: i32) {
        let owner = self.surface.owner();
        let line = row.and_then(|i| match self.model.rows.get(i) {
            Some(Row::Entry(l)) => Some(*l),
            _ => None,
        });
        let site = row.and_then(|i| self.model.site_at(i));
        let mut entries = Vec::new();
        if line.is_some() {
            entries.push(w::item(1, "Open", Icon::None));
            entries.push(w::item(2, "Open in new tab", Icon::None));
            entries.push(w::divider());
            // History stores one line per visit and has no per-visit delete, so this removes every
            // visit to the page. The label says so.
            entries.push(w::item(3, "Remove all visits to this page", Icon::Close));
            if let Some(host) = &site {
                entries.push(w::item(4, format!("Remove all visits to {host}"), Icon::Close));
            }
            entries.push(w::divider());
        }
        entries.push(w::item(5, "Clear browsing history\u{2026}", Icon::Reset));
        let Some(chosen) = w::context_menu(owner, windows::Win32::Foundation::POINT { x, y }, entries) else {
            return;
        };
        match (chosen, line) {
            (1, Some(l)) => {
                let url = self.model.lines[l].url.clone();
                self.run(Action::Open { url, new_tab: false });
            }
            (2, Some(l)) => {
                let url = self.model.lines[l].url.clone();
                self.run(Action::Open { url, new_tab: true });
            }
            (3, Some(l)) => {
                let url = self.model.lines[l].url.clone();
                self.run(Action::RemoveEntry { url });
            }
            (4, Some(_)) => {
                if let Some(host) = site {
                    self.run(Action::RemoveSite { host });
                }
            }
            (5, _) => self.open_range_menu(),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Button(Btn),
    Row(usize),
}

struct Geometry {
    search: RECT,
    clear: RECT,
    status: Option<RECT>,
    list_top: i32,
    pad: i32,
    row: i32,
}

impl EditSink for Content {
    fn edit_event(&mut self, field: usize, event: EditEvent) -> bool {
        if field != SEARCH {
            return false;
        }
        match event {
            EditEvent::Changed => {
                let text = self.search.as_ref().map(EditField::text).unwrap_or_default();
                if text != self.model.query {
                    self.model.set_query(&text);
                    self.reload();
                }
                false
            }
            EditEvent::Key(Key::Escape) => {
                if self.model.query.is_empty() {
                    side_panel::close(self.surface.owner());
                } else {
                    if let Some(search) = &self.search {
                        search.set_text("");
                    }
                    self.model.set_query("");
                    self.reload();
                }
                true
            }
            EditEvent::Key(key @ (Key::Up | Key::Down | Key::PageUp | Key::PageDown | Key::Enter)) => {
                let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
                let m = self.list_metrics(&self.geometry());
                let action = self.model.key(key, ctrl, m);
                self.run(action);
                true
            }
            EditEvent::Tab { .. } => {
                self.surface.focus_host();
                true
            }
            _ => false,
        }
    }
}

impl PointerSink for Content {
    fn right_click(&mut self, x: i32, y: i32) {
        let (cx, cy) = self.surface.to_content(x, y);
        let row = match self.hit(cx, cy) {
            Some(Target::Row(i)) | Some(Target::Button(Btn::Remove(i))) => Some(i),
            _ => None,
        };
        let screen = self.surface.cursor();
        self.show_menu(row, screen.x, screen.y);
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
        self.viewport = rect.bottom - rect.top;
        self.ensure_controls();
        let g = self.geometry();
        let old = unsafe { SelectObject(hdc, self.font.into()) };
        unsafe {
            SetBkMode(hdc, TRANSPARENT);
        }
        let surface = self.surface;
        if let Some(search) = self.search.as_mut() {
            search.place(Some(surface.to_host(g.search)));
        }
        let clear_host = surface.to_host(g.clear);
        unsafe { draw_button(hdc, clear_host, "Clear browsing history\u{2026}", palette.tab_inactive, palette.tab_text_active, self.px(8)) };
        if let (Some(text), Some(rect)) = (self.message.as_deref(), g.status) {
            unsafe { w::draw_text(hdc, surface.to_host(rect), text, palette.tab_text_inactive, false, true) };
        }
        self.draw_list(hdc, palette, &g);
        unsafe {
            SelectObject(hdc, old);
        }
    }

    fn on_mouse_move(&mut self, x: i32, y: i32) {
        let (_, cy) = self.surface.to_content(x, y);
        let g = self.geometry();
        let hover = if cy >= g.list_top {
            let m = self.list_metrics(&g);
            w::row_at(cy - g.list_top, self.model.scroll, self.model.rows.len(), m)
        } else {
            None
        };
        if hover != self.model.hover {
            self.model.hover = hover;
            self.surface.invalidate();
        }
    }

    fn on_mouse_down(&mut self, x: i32, y: i32) {
        self.surface.focus_host();
        let (cx, cy) = self.surface.to_content(x, y);
        // Capture is held only for a row, where a drag can follow. Buttons open a menu or a dialog,
        // and a capture held across that modal loop fights the mouse.
        match self.hit(cx, cy) {
            Some(Target::Button(Btn::Clear)) => {
                self.press = None;
                self.open_range_menu();
            }
            Some(Target::Button(Btn::Remove(index))) => {
                self.press = None;
                let action = self.model.remove_at(index);
                self.run(action);
            }
            Some(Target::Row(index)) => {
                unsafe {
                    let _ = SetCapture(self.surface.host);
                }
                self.press = Some((index, cx, cy));
            }
            None => self.press = None,
        }
    }

    fn on_mouse_up(&mut self, x: i32, y: i32) {
        unsafe {
            let _ = ReleaseCapture();
        }
        let Some((index, px_x, px_y)) = self.press.take() else {
            return;
        };
        let (cx, cy) = self.surface.to_content(x, y);
        let slop = self.px(DRAG_DIP);
        if (cx - px_x).abs() > slop || (cy - px_y).abs() > slop {
            return;
        }
        let new_tab = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
        let m = self.list_metrics(&self.geometry());
        let action = self.model.click(index, new_tab, m);
        self.run(action);
    }

    fn on_mouse_wheel(&mut self, delta: i16) {
        let g = self.geometry();
        let m = self.list_metrics(&g);
        self.model.scroll_by(-(delta.signum() as i32) * g.row * 3, m);
        self.surface.invalidate();
    }

    fn on_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        if let Some(search) = &self.search {
            search.focus();
            w::type_into(search.hwnd, ch);
        }
    }

    fn on_key(&mut self, key: u32, shift: bool, ctrl: bool) {
        if key == windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB.0 as u32 {
            if let Some(search) = &self.search {
                search.focus();
            }
            return;
        }
        if key == VK_APPS.0 as u32 || (key == VK_F10.0 as u32 && shift) {
            let g = self.geometry();
            let (x, y) = match self.model.selected_row() {
                Some(i) => {
                    let row = self.row_rect(&g, i);
                    (row.left + self.px(24), row.bottom)
                }
                None => (g.pad, g.list_top),
            };
            let mut point = windows::Win32::Foundation::POINT { x: x + self.surface.origin.left, y: y + self.surface.origin.top };
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ClientToScreen(self.surface.host, &mut point);
            }
            let row = self.model.selected_row();
            self.show_menu(row, point.x, point.y);
            return;
        }
        if let Some(k) = w::key_from_vk(key as u16) {
            let m = self.list_metrics(&self.geometry());
            let action = self.model.key(k, ctrl, m);
            self.run(action);
        }
    }

    fn on_focus(&mut self, focused: bool) {
        if focused {
            self.reload();
        }
    }

    fn title(&self) -> &str {
        "History"
    }

    fn preferred_width(&self) -> i32 {
        340
    }
}

impl Content {
    fn draw_list(&mut self, dc: HDC, palette: &Palette, g: &Geometry) {
        if let Some(text) = self.model.empty_text() {
            let rect = RECT { left: g.pad, top: g.list_top + g.pad, right: self.width() - g.pad, bottom: g.list_top + self.px(64) };
            unsafe { w::draw_text(dc, self.surface.to_host(rect), text, palette.tab_text_inactive, false, false) };
            return;
        }
        let m = self.list_metrics(g);
        let icon = self.px(16);
        for index in w::visible_range(self.model.scroll, self.model.rows.len(), m) {
            let rect = self.row_rect(g, index);
            let body = self.surface.to_host(rect);
            let hovered = self.model.hover == Some(index);
            match self.model.rows[index].clone() {
                Row::Header { label, count } => {
                    let text = format!("{label}  \u{00B7}  {count}");
                    let title = RECT { left: rect.left + 4, top: rect.top + self.px(10), right: rect.right, bottom: rect.bottom };
                    unsafe { w::draw_text(dc, self.surface.to_host(title), &text, palette.tab_text_inactive, false, true) };
                }
                Row::Entry(line) => {
                    let entry = self.model.lines[line].clone();
                    let selected = self.model.selected == Some(line);
                    if selected {
                        unsafe { smooth::fill_round_rect(dc, body, self.px(10), palette.tab_active) };
                    } else if hovered {
                        unsafe { smooth::fill_round_rect(dc, body, self.px(10), palette.tab_hover) };
                    }
                    let tile = RECT { left: rect.left + 6, top: rect.top + 11, right: rect.left + 6 + icon, bottom: rect.top + 11 + icon };
                    match self.icons.get(&entry.host, icon as u32) {
                        Some(bitmap) => unsafe { w::blit_favicon(dc, self.surface.to_host(tile), &bitmap) },
                        None => unsafe { w::letter_tile_at(dc, self.surface.to_host(tile), &entry.title, &entry.host) },
                    }
                    let [remove] = self.action_rects(rect);
                    let right = if hovered || selected { remove.left - self.px(6) } else { rect.right - self.px(54) };
                    let text_left = rect.left + 6 + icon + self.px(8);
                    let title_text = if entry.title.trim().is_empty() { entry.url.clone() } else { entry.title.clone() };
                    let title_rect = RECT { left: text_left, top: rect.top + self.px(4), right, bottom: rect.top + self.px(21) };
                    unsafe { w::draw_text(dc, self.surface.to_host(title_rect), &title_text, palette.tab_text_active, false, true) };
                    let detail_rect = RECT { left: text_left, top: rect.top + self.px(21), right, bottom: rect.top + self.px(37) };
                    unsafe { w::draw_text(dc, self.surface.to_host(detail_rect), &entry.host, palette.tab_text_inactive, false, true) };
                    let clock = RECT { left: rect.right - self.px(52), top: rect.top + self.px(4), right: rect.right - self.px(6), bottom: rect.top + self.px(21) };
                    if !(hovered || selected) {
                        unsafe { w::draw_text(dc, self.surface.to_host(clock), &entry.clock, palette.tab_text_inactive, true, false) };
                    }
                    if hovered || selected {
                        unsafe { draw_button(dc, self.surface.to_host(remove), "\u{00D7}", palette.tab_inactive, w::danger(palette), self.px(6)) };
                    }
                }
            }
        }
    }
}

unsafe fn draw_button(dc: HDC, rect: RECT, label: &str, fill: Rgba, text: Rgba, radius: i32) {
    unsafe {
        smooth::fill_round_rect(dc, rect, radius * 2, fill);
        w::draw_text(dc, rect, label, text, true, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY_S: u64 = 86_400;
    // 2026-10-12 12:00 UTC.
    const NOON: u64 = 20738 * DAY_S + 12 * 3600;

    fn entry(t: u64, url: &str, title: &str) -> HistoryEntry {
        (t, url.into(), title.into())
    }

    fn m() -> Metrics {
        Metrics { row: 20, height: 60 }
    }

    fn sample() -> Vec<HistoryEntry> {
        vec![
            entry(NOON - 600, "https://www.rust-lang.org/", "Rust"),
            entry(NOON - 3600 * 3, "https://docs.rs/serde", "serde"),
            entry(NOON - DAY_S - 60, "https://example.com/yesterday", "Yesterday page"),
            entry(NOON - 3 * DAY_S, "https://old.example.org/", "Old"),
        ]
    }

    fn built(offset: i64) -> Model {
        let mut model = Model::default();
        model.rebuild(&sample(), NOON, offset);
        model
    }

    #[test]
    fn entries_group_under_today_yesterday_and_dates_newest_first() {
        let model = built(0);
        let headers: Vec<String> = model
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Header { label, .. } => Some(label.clone()),
                Row::Entry(_) => None,
            })
            .collect();
        assert_eq!(headers, vec!["Today", "Yesterday", "2026-10-09"]);
        assert_eq!(model.rows[0], Row::Header { label: "Today".into(), count: 2 });
        assert_eq!(model.rows[1], Row::Entry(0));
        assert_eq!(model.rows[2], Row::Entry(1));
        assert_eq!(model.rows[3], Row::Header { label: "Yesterday".into(), count: 1 });
        assert_eq!(model.rows[4], Row::Entry(2));
    }

    #[test]
    fn local_offset_moves_late_visits_into_the_local_day() {
        // 23:30 UTC on the 11th is the 12th at UTC+1, so it is "Today" for that user.
        let late = 20737 * DAY_S + 23 * 3600 + 1800;
        let mut model = Model::default();
        model.rebuild(&[entry(late, "https://a.example/", "A")], 20738 * DAY_S + 3600, 3600);
        assert_eq!(model.rows[0], Row::Header { label: "Today".into(), count: 1 });
        assert_eq!(model.lines[0].clock, "00:30");
        let mut utc = Model::default();
        utc.rebuild(&[entry(late, "https://a.example/", "A")], 20738 * DAY_S + 3600, 0);
        assert_eq!(utc.rows[0], Row::Header { label: "Yesterday".into(), count: 1 });
    }

    #[test]
    fn arrow_keys_skip_headers_and_follow_entries() {
        let mut model = built(0);
        assert_eq!(model.key(Key::Down, false, m()), Action::Redraw);
        assert_eq!(model.selected, Some(0));
        model.key(Key::Down, false, m());
        assert_eq!(model.selected, Some(1));
        model.key(Key::Down, false, m());
        assert_eq!(model.selected, Some(2));
        model.key(Key::Down, false, m());
        assert_eq!(model.selected, Some(3));
        model.key(Key::Down, false, m());
        assert_eq!(model.selected, Some(3));
        assert_eq!(model.selected_row(), Some(6));
    }

    #[test]
    fn enter_opens_in_the_chosen_tab_and_delete_removes_the_entry() {
        let mut model = built(0);
        assert_eq!(model.key(Key::Enter, false, m()), Action::Nothing);
        model.selected = Some(1);
        assert_eq!(
            model.key(Key::Enter, true, m()),
            Action::Open {
                url: "https://docs.rs/serde".into(),
                new_tab: true
            }
        );
        assert_eq!(
            model.key(Key::Delete, false, m()),
            Action::RemoveEntry {
                url: "https://docs.rs/serde".into()
            }
        );
    }

    #[test]
    fn clicks_on_headers_do_nothing_and_clicks_on_entries_open() {
        let mut model = built(0);
        assert_eq!(model.click(0, false, m()), Action::Nothing);
        assert_eq!(
            model.click(1, false, m()),
            Action::Open {
                url: "https://www.rust-lang.org/".into(),
                new_tab: false
            }
        );
        assert_eq!(model.selected, Some(0));
    }

    #[test]
    fn remove_and_site_actions_use_the_row_under_the_pointer() {
        let model = built(0);
        // Row 0 is the Today heading, row 1 is the Rust visit, row 2 is the docs.rs visit.
        assert_eq!(model.remove_at(0), Action::Nothing);
        assert!(!model.has_remove_button(0));
        assert!(model.has_remove_button(2));
        assert_eq!(model.remove_at(2), Action::RemoveEntry { url: "https://docs.rs/serde".into() });
        assert_eq!(model.site_at(2), Some("docs.rs".into()));
        assert_eq!(model.site_at(0), None);
    }

    #[test]
    fn escape_clears_the_search_before_dismissing() {
        let mut model = built(0);
        model.set_query("rust");
        assert_eq!(model.key(Key::Escape, false, m()), Action::Redraw);
        assert!(model.query.is_empty());
        assert_eq!(model.key(Key::Escape, false, m()), Action::Dismiss);
    }

    #[test]
    fn empty_text_depends_on_query() {
        let mut model = Model::default();
        model.rebuild(&[], NOON, 0);
        assert!(model.empty_text().unwrap().starts_with("Nothing"));
        model.set_query("zzz");
        assert!(model.empty_text().unwrap().starts_with("No matches"));
        model.rebuild(&sample(), NOON, 0);
        assert_eq!(model.empty_text(), None);
    }

    #[test]
    fn clear_ranges_cover_the_last_hour_today_or_everything() {
        assert_eq!(Range::LastHour.bounds(NOON, 0), Some((NOON - 3600, NOON)));
        assert_eq!(Range::LastHour.bounds(100, 0), Some((0, 100)));
        assert_eq!(Range::All.bounds(NOON, 0), None);
        assert_eq!(Range::Today.bounds(NOON, 0), Some((20738 * DAY_S, u64::MAX)));
        // At UTC+2 the local day began at 22:00 UTC on the 11th.
        assert_eq!(
            start_of_local_day(NOON, 7200),
            20738 * DAY_S - 7200
        );
        assert_eq!(start_of_local_day(NOON, -5 * 3600), 20738 * DAY_S + 5 * 3600);
    }

    #[test]
    fn civil_dates_format_day_numbers_as_iso_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(11017), "2000-03-01");
        assert_eq!(civil_date(20735), "2026-10-09");
        assert_eq!(civil_date(20738), "2026-10-12");
    }

    #[test]
    fn ranges_are_labelled_for_the_menu() {
        let labels: Vec<&str> = Range::ALL.iter().map(|r| r.label()).collect();
        assert_eq!(labels, vec!["Last hour", "Today", "All time"]);
    }

    #[test]
    fn scroll_follows_the_selected_entry_and_clamps() {
        let mut model = built(0);
        model.selected = Some(3);
        model.reveal_selection(Metrics { row: 20, height: 40 });
        // The entry is row 6 of 7, so the viewport must end at its bottom edge, 140.
        assert_eq!(model.selected_row(), Some(6));
        assert_eq!(model.scroll, 140 - 40);
        model.scroll_by(-1000, Metrics { row: 20, height: 40 });
        assert_eq!(model.scroll, 0);
    }
}
