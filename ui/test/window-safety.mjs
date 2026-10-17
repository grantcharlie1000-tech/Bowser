// Static Windows security/integration contracts run even when native Rust/Edge is unavailable.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = (file) => readFileSync(new URL(`../../${file}`, import.meta.url), 'utf8');
const window = read('src/win32/window.rs');
const mod = read('src/win32/mod.rs');
const auto = read('src/win32/window/auto_hide.rs');
const notice = read('src/win32/window/fullscreen_notice.rs');
const app = read('src/app.rs');
const engine = read('src/engine/webview2.rs');
const constant = (source, name) => {
  const match = new RegExp(`const ${name}: usize = (0x[\\da-fA-F_]+|[\\d_]+);`).exec(source);
  assert.ok(match, `${name} exists`);
  return Number(match[1].replaceAll('_', ''));
};
test('feature timers cannot collide in the main window dispatcher', () => {
  const pairs = [[auto, 'TIMER_ID'], [window, 'WM_THEME_SCHEDULE_TIMER'],
    [mod, 'WM_PRIVACY_STATS_TIMER'], [mod, 'WM_SPLIT_BOUNDS_TIMER'],
    [mod, 'WM_READER_PROBE_TIMER'], [read('src/win32/panel_webapp.rs'), 'RELOAD_TIMER']];
  const values = pairs.map(([source, name]) => constant(source, name));
  assert.equal(new Set(values).size, values.length);
  assert.match(window, /fn the_main_window_timer_ids_are_unique_and_outside_hover_ranges/);
});
test('fullscreen identity is browser-owned, persistent, ASCII-origin only and fail-closed', () => {
  assert.match(notice, /url\.origin\(\)\.ascii_serialization\(\)/);
  assert.match(notice, /Press Esc to exit full screen/);
  assert.match(notice, /CreateWindowExW\(/);
  assert.match(notice, /WS_EX_NOACTIVATE \| WS_EX_TOOLWINDOW/);
  assert.doesNotMatch(notice, /ExecuteScript|execute_script|innerHTML|SetTimer\(/);
  assert.match(auto, /if !super::fullscreen_notice::show\(hwnd, app, source\) \{\s*page_fullscreen\(hwnd, app, false, None\)/);
  assert.match(auto, /super::fullscreen_notice::hide\(hwnd\)/);
  assert.match(auto, /if on == \(bits & 2 != 0\) \{ return; \}/);
});
test('Escape, navigation and tab switching cannot leave stale fullscreen identity', () => {
  assert.match(app, /pub fn reader_escape[\s\S]*?win32::page_fullscreen\(hwnd, app, false, None\)/);
  assert.match(app, /new document must never inherit[\s\S]*?win32::page_fullscreen\(progress_hwnd, &app, false, None\)/);
  assert.match(window, /WM_KEYDOWN \| WM_SYSKEYDOWN =>[\s\S]*?Fullscreen::Page[\s\S]*?auto_hide::page_fullscreen\(hwnd, app, false, None\)/);
  assert.match(window, /WM_TABS_CHANGED =>[\s\S]*?fullscreen_notice::belongs_to_active_tab[\s\S]*?auto_hide::page_fullscreen\(hwnd, app, false, None\)/);
});

test('fullscreen event captures native WebView2 Source, not optimistic app URL or page script', () => {
  assert.match(engine, /fullscreen_changed\(tab_id, on.as_bool\(\), current_web_url\(&sender\)\)/);
  assert.match(engine, /fn current_web_url[\s\S]*?webview.Source\(&mut source\)/);
  assert.doesNotMatch(notice, /App::active_site|tab_committed_url|tab\.url/);
  assert.match(notice, /source.and_then\(notice_text\)/);
  assert.match(notice, /chunks\(columns\)/);
  assert.match(notice, /bounds.bottom - bounds.top < height/);
});

test('foreground fading refreshes page and panel state without resetting dirty drafts', () => {
  const sync = window.slice(window.indexOf('fn sync_foreground('), window.indexOf('unsafe extern "system" fn foreground_changed'));
  assert.match(sync, /if faded && changed/);
  assert.match(sync, /mark_chrome_dirty\(hwnd\)/);
  assert.match(sync, /html_panel::settings_changed\(hwnd\)/);
  assert.doesNotMatch(sync, /settings_page::refresh|history_page::refresh|downloads_page::refresh/);
});
