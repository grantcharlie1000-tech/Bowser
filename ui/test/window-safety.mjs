// Static Windows security/integration contracts run even when native Rust/Edge is unavailable.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { runInNewContext } from 'node:vm';
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

test('theme-flow tests use isolated real persistence and cannot block in error dialogs', () => {
  const flow = read('src/app/tests/theme_flow.rs');
  assert.equal((flow.match(/#\[test\]/g) ?? []).length,
    (flow.match(/let _profile = crate::profile::TestProfileGuard::isolated\(\);/g) ?? []).length);
  assert.match(flow, /path\.is_file\(\)/);
  assert.match(flow, /settings::read_strict/);
  const profile = read('src/profile.rs');
  assert.match(profile, /thread_local! \{\s*static TEST_ACTIVE:/);
  assert.match(profile, /parallel_app_profiles_keep_real_settings_writes_separate/);
  assert.match(profile, /isolated_app_profiles_restore_scope_and_remove_only_their_owned_root/);
  assert.match(mod, /#\[cfg\(test\)\]\s*pub fn show_error_message\(message: &str\) \{\s*panic!/);
  assert.match(mod, /#\[cfg\(not\(test\)\)\]\s*pub fn show_error_message/);
});
test('lint repairs preserve callback-driven modal completion and integer icon identity', () => {
  for (const file of ['command_palette.rs', 'profile_rename.rs']) {
    const source = read(`src/win32/${file}`);
    assert.match(source, /loop \{[\s\S]*?if state\.done \{ break; \}[\s\S]*?GetMessageW[\s\S]*?DispatchMessageW/);
  }
  assert.match(window, /PCWSTR\(std::ptr::without_provenance\(1\)\)/);
  const engineSource = read('src/engine/webview2.rs');
  assert.match(engineSource, /Zeroizing::new\(raw_message\.owned_text\(\)\)/);
  assert.doesNotMatch(engineSource, /impl[^\n]*Display for ZeroizingWebMessage/);
});

test('destructive sleeping always resumes and probes instead of trusting suspension hints', () => {
  const method = engine.slice(engine.indexOf('    fn check_sleep_blocked('), engine.indexOf('    fn suspend_hidden_tabs('));
  assert.doesNotMatch(method, /is_suspended|suspended\.contains|try_suspend\(/);
  assert.match(method, /let Some\(webview\) = webview else \{\s*done\(true\)/);
  assert.match(method, /resume_then_sleep_probe\([\s\S]*?api\.Resume\(\)[\s\S]*?webview\.ExecuteScript/);
  assert.match(method, /sleep_probe_blocks\(status\.is_ok\(\), &result\)/);
  assert.match(engine, /if resume\(\)\.is_err\(\) \{\s*done\(true\);/);
  assert.match(read('src/engine/mod.rs'), /fn check_sleep_blocked[^\{]+\{\s*done\(true\)/);
});

test('sleep script conservatively preserves blurred edits and unreadable frame state', () => {
  const script = /const SLEEP_BLOCK_SCRIPT: &str = r#"([\s\S]*?)"#;/.exec(engine)?.[1];
  assert.ok(script);
  const page = (fields = []) => ({ document: { querySelectorAll: (selector) => selector === 'video,audio' ? [] : fields }, frames: [] });
  const blocked = (window) => runInNewContext(script, { window });
  assert.equal(blocked(page()), false, 'a readable empty page can be checked');
  const edited = { tagName: 'DIV', isContentEditable: true, textContent: 'unsaved draft' };
  assert.equal(blocked(page([edited])), true, 'blurred contenteditable is still protected');
  assert.equal(blocked(page([{ ...edited, contentEditable: 'plaintext-only' }])), true, 'plaintext-only edits are protected');
  assert.equal(blocked(page([{ ...edited, isContentEditable: false }])), false, 'contenteditable=false is not editable');
  assert.equal(blocked(page([{ tagName: 'INPUT', type: 'file', files: { length: 1 } }])), true, 'selected file is unsaved form state');
  assert.equal(blocked(page([{ tagName: 'INPUT', type: 'file', files: { length: 0 } }])), false, 'empty file input is readable');
  assert.equal(blocked(page([{ tagName: 'SELECT', options: [{ selected: true, defaultSelected: false }] }])), true, 'changed select stays awake');
  assert.equal(blocked(page([{ tagName: 'SELECT', options: [{ selected: true, defaultSelected: true }] }])), false, 'unchanged selection can be checked');
  const media = page(); media.document.querySelectorAll = (selector) => selector === 'video,audio'
    ? [{ paused: false, ended: false, readyState: 4 }] : [];
  assert.equal(blocked(media), true, 'muted but playing media stays awake');
  const inaccessible = {}; Object.defineProperty(inaccessible, 'document', { get() { throw Error('cross-origin'); } });
  const framed = page(); framed.frames = [inaccessible];
  assert.equal(blocked(framed), true, 'unreadable child frame is unknown, not safe');
  const broken = page(); broken.document.querySelectorAll = () => { throw Error('page exception'); };
  assert.equal(blocked(broken), true, 'probe exceptions fail closed');
  let deep = page(); for (let depth = 0; depth < 10; depth++) { const parent = page(); parent.frames = [deep]; deep = parent; }
  assert.equal(blocked(deep), true, 'unvisited frame depth is unknown, not safe');
});
