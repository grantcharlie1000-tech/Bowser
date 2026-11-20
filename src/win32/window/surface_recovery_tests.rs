use super::*;
use crate::chrome::paint::surface_test_support as faults;
use windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE;

// All HWNDs are exclusively owned, offscreen and never shown. The real painter and GDI run;
// no window class starts Bowser or WebView2 and the app is the existing windowless fixture.
struct Fixture { hwnd: HWND, app: Box<AppRef> }
impl Fixture {
    fn new() -> Self {
        let hwnd = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""),
            WS_POPUP | WS_CLIPCHILDREN, -32000, -32000, 800, 600, None, None, None, None) }.unwrap();
        let app = Box::new(crate::app::test_app());
        app.borrow_mut().hwnd = hwnd;
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&*app as *const AppRef) as isize); }
        let overlay = unsafe { CreateWindowExW(WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            w!("STATIC"), w!(""), WS_POPUP, -32000, -32000, 800, 600, Some(hwnd), None, None, None) }.unwrap();
        let mut s = WindowChrome::new(800, 600, 96, overlay);
        // No HTML engine is started in this native fixture.
        let edit = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("EDIT"), w!(""),
            WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32), 100, 3, 480, 24, Some(hwnd), None, None, None) }.unwrap();
        unsafe { SetWindowRgn(edit, Some(CreateRectRgn(0, 0, 0, 0)), false); }
        app.borrow_mut().edit = edit;
        s.edit = edit;
        s.html_failed = [true; 3];
        s.sync_html_bounds(); s.sync_html_tabs_bounds(); s.sync_html_rail_bounds();
        WINDOWS.with(|all| all.borrow_mut().insert(hwnd.0 as isize, s));
        unsafe {
            assert!(SetWindowSubclass(edit, Some(edit_paint_probe), 2, 0).as_bool());
            let lease = with_chrome(hwnd, |s| s.surface_recovery.edit_paint_lease()).unwrap();
            assert!(install_edit_subclass(edit, hwnd, lease));
        }
        Self { hwnd, app }
    }
    fn paint(&self) {
        unsafe { let _ = InvalidateRect(Some(self.hwnd), None, false); }
        let size = with_chrome(self.hwnd, |s| s.client).unwrap();
        PAINT_DAMAGE.with(|slot| slot.set(Some(RECT { left: 0, top: 0, right: size.0, bottom: size.1 })));
        paint_chrome(self.hwnd, Some(&self.app));
    }
    fn retry(&self) {
        unsafe { window_proc(self.hwnd, WM_TIMER, WPARAM(0x5f2), LPARAM(0)); }
        self.paint();
    }
    fn resize(&self, width: i32, height: i32, tabs: crate::settings::TabLayout) {
        with_chrome(self.hwnd, |s| s.relayout(RelayoutRequest {
            w: width, h: height, dpi: 96, active: TabId(0), maximised: false,
            arrangement: layout::Arrangement { tabs, ..Default::default() },
            density: crate::layout_prefs::UiDensity::Regular, sidebar_width: 260,
            centred_address_bar: true, font_scale: 100, ui_font: "",
        }));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        faults::reset();
        OVERLAY_FAILURE.with(|slot| slot.set(false));
        TIMER_FAILURE.with(|slot| slot.set(false));
        BUTTON_FAILURES.with(|slot| slot.set(0));
        PAINT_DAMAGE.with(|slot| slot.set(None));
        crate::win32::downloads_ui::close(self.hwnd);
        crate::win32::split_ui::close(self.hwnd);
        let state = WINDOWS.with(|all| all.borrow_mut().remove(&(self.hwnd.0 as isize)));
        unsafe {
            if let Some(state) = state { let _ = DestroyWindow(state.overlay); }
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn pixel_alpha(hwnd: HWND, x: i32, y: i32) -> u8 {
    with_chrome(hwnd, |s| s.surface.pixels()[y as usize * s.surface.stride() + x as usize * 4 + 3]).unwrap()
}

#[test]
fn recovery_presentation_native_card_is_enabled_raised_and_unmasked_by_the_real_painter() {
    use windows::Win32::UI::{Input::KeyboardAndMouse::IsWindowEnabled, WindowsAndMessaging::GW_CHILD};
    let _profile = crate::profile::TestProfileGuard::isolated();
    let f = Fixture::new();
    App::tab_render_failed(&f.app, TabId(0));
    sync_recovery_buttons(f.hwnd, &f.app);
    let (button, tab) = with_chrome(f.hwnd, |s| s.recovery_buttons[0]).unwrap();
    assert_eq!(tab, Some(TabId(0)));
    assert_ne!(unsafe { GetWindowLongPtrW(button, GWL_STYLE) } as u32 & WS_VISIBLE.0, 0,
        "check the child's show style: the parent remains never shown");
    assert!(unsafe { IsWindowEnabled(button) }.as_bool());
    assert_eq!(unsafe { GetWindow(f.hwnd, GW_CHILD) }.unwrap(), button, "raise the native card above other children");
    assert_eq!(read_edit(button), App::content_failure_message(&f.app, TabId(0)).unwrap());
    let mut rect = RECT::default();
    unsafe { GetWindowRect(button, &mut rect).unwrap(); }
    let mut point = POINT { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 };
    unsafe { let _ = ScreenToClient(f.hwnd, &mut point); }
    f.paint();
    assert_eq!(pixel_alpha(f.hwnd, point.x, point.y), 0,
        "HWND_TOP cannot escape the owned layered popup; the actual DIB must expose the card");
    let viewport = split_viewport(f.hwnd).unwrap();
    assert_eq!(pixel_alpha(f.hwnd, viewport.left + 20, point.y), 255,
        "the rest of a failed pane must keep its opaque themed fallback");
}

#[test]
fn recovery_presentation_switch_to_healthy_content_clears_old_fill_and_card() {
    let _profile = crate::profile::TestProfileGuard::isolated();
    let f = Fixture::new();
    App::tab_render_failed(&f.app, TabId(0));
    sync_recovery_buttons(f.hwnd, &f.app);
    let button = with_chrome(f.hwnd, |s| s.recovery_buttons[0].0).unwrap();
    f.paint();
    let healthy = App::new_tab(&f.app);
    sync_recovery_buttons(f.hwnd, &f.app);
    f.paint();
    assert_eq!(unsafe { GetWindowLongPtrW(button, GWL_STYLE) } as u32 & WS_VISIBLE.0, 0);
    let viewport = split_viewport(f.hwnd).unwrap();
    // A blank new tab is the window's glass on a translucent palette and the opaque window colour on a
    // solid one; either way the old card's hole must be repainted to match the rest of it.
    let centre = pixel_alpha(f.hwnd, (viewport.left + viewport.right) / 2, (viewport.top + viewport.bottom) / 2);
    let edge = pixel_alpha(f.hwnd, viewport.left + 20, (viewport.top + viewport.bottom) / 2);
    let solid = with_chrome(f.hwnd, |s| !s.palette.translucent).unwrap();
    assert_eq!(edge, if solid { 255 } else { 0 }, "a healthy blank tab shows the new-tab surface");
    assert_eq!(centre, edge, "a healthy blank tab must repaint the old card hole with its new-tab surface");
    crate::app::test_show_page(&f.app, healthy, "https://example.test/replacement-document");
    f.paint();
    assert_eq!(pixel_alpha(f.hwnd, (viewport.left + viewport.right) / 2, (viewport.top + viewport.bottom) / 2), 0,
        "controller presentation must remove the retained empty-viewport fill");
}

#[test]
fn recovery_presentation_split_failure_exposes_only_its_card_and_keeps_the_healthy_pane_clear() {
    use crate::split_view::Pane;
    let _profile = crate::profile::TestProfileGuard::isolated();
    let f = Fixture::new();
    f.resize(1200, 800, crate::settings::TabLayout::ToolbarTop);
    let right = App::new_tab(&f.app);
    crate::app::test_show_page(&f.app, TabId(0), "https://example.test/failed-left");
    crate::app::test_show_page(&f.app, right, "https://example.test/healthy-right");
    assert!(App::open_split_pair(&f.app, TabId(0), right, Pane::Left, None));
    sync_split_ratio(f.hwnd, App::split_view(&f.app));
    App::tab_render_failed(&f.app, TabId(0));
    sync_recovery_buttons(f.hwnd, &f.app);
    let regions = with_chrome(f.hwnd, |s| s.recovery_regions).unwrap();
    let card = regions[Pane::Left.index()].unwrap();
    assert!(regions[Pane::Right.index()].is_none());
    f.paint();
    assert_eq!(pixel_alpha(f.hwnd, (card.left + card.right) / 2, (card.top + card.bottom) / 2), 0);
    let healthy = split_rects(f.hwnd, None).unwrap().get(Pane::Right).unwrap();
    assert_eq!(pixel_alpha(f.hwnd, (healthy.left + healthy.right) / 2, (healthy.top + healthy.bottom) / 2), 0);
}

#[test]
fn recovery_presentation_native_input_and_caption_close_survive_browser_loss() {
    let _profile = crate::profile::TestProfileGuard::isolated();
    let f = Fixture::new();
    recover_native_chrome(f.hwnd, &f.app);
    let edit = f.app.borrow().edit;
    unsafe {
        SetWindowTextW(edit, w!("recovery")).unwrap();
        let _ = SendMessageW(edit, EM_SETSEL, Some(WPARAM(8)), Some(LPARAM(8)));
        let _ = SendMessageW(edit, WM_CHAR, Some(WPARAM('!' as usize)), Some(LPARAM(0)));
    }
    assert_eq!(read_edit(edit), "recovery!");
    assert_ne!(unsafe { GetWindowLongPtrW(edit, GWL_STYLE) } as u32 & WS_VISIBLE.0, 0);
    let caption = glass::CaptionButtons::for_client_width(800, 96).close;
    let mut point = POINT { x: (caption.left + caption.right) / 2, y: (caption.top + caption.bottom) / 2 };
    unsafe { let _ = ClientToScreen(f.hwnd, &mut point); }
    let packed = LPARAM(((point.y as u16 as u32) << 16 | point.x as u16 as u32) as isize);
    assert_eq!(unsafe { window_proc(f.hwnd, WM_NCHITTEST, WPARAM(0), packed) }.0, HTCLOSE as isize);
}

#[test]
fn initial_allocation_failure_is_not_a_successful_chrome_frame() {
    faults::fail_allocation();
    let f = Fixture::new();
    f.paint();
    assert!(!with_chrome(f.hwnd, |s| s.first_paint_logged).unwrap(), "absent pixels must never log chrome_painted");
}

#[test]
fn failed_resize_retries_the_requested_size_without_another_size_message() {
    let f = Fixture::new();
    faults::fail_allocation();
    f.resize(1024, 768, crate::settings::TabLayout::Vertical);
    assert_eq!(with_chrome(f.hwnd, |s| (s.client, s.surface.width(), s.surface.height())).unwrap(), ((1024, 768), 800, 600));
    f.paint();
    f.retry();
    assert_eq!(with_chrome(f.hwnd, |s| (s.surface.width(), s.surface.height())).unwrap(), (1024, 768));
}

#[test]
fn failed_layered_upload_is_not_a_successful_chrome_frame() {
    let f = Fixture::new();
    faults::fail_upload();
    f.paint();
    assert!(!with_chrome(f.hwnd, |s| s.first_paint_logged).unwrap(), "failed upload must never log chrome_painted");
}

#[test]
fn a_late_ready_cannot_reactivate_a_failed_document() {
    let f = Fixture::new();
    html_command(f.hwnd, ChromeCommand::Ready);
    html_tabs_command(f.hwnd, ChromeCommand::Ready);
    html_rail_command(f.hwnd, ChromeCommand::Ready);
    assert_eq!(with_chrome(f.hwnd, |s| (s.html_page_ready, s.html_tabs_page_ready, s.html_rail_page_ready)).unwrap(), (false, false, false));
}

#[test]
fn native_edit_remains_visible_and_unclipped_when_the_surface_is_absent() {
    faults::fail_allocation();
    let f = Fixture::new();
    f.paint();
    let edit = f.app.borrow().edit;
    assert_ne!(unsafe { GetWindowLongPtrW(edit, GWL_STYLE) } as u32 & WS_VISIBLE.0, 0);
    let region = unsafe { CreateRectRgn(0, 0, 1, 1) };
    let kind = unsafe { windows::Win32::Graphics::Gdi::GetWindowRgn(edit, region) };
    unsafe { let _ = DeleteObject(region.into()); }
    assert_ne!(kind, windows::Win32::Graphics::Gdi::NULLREGION, "the native edit must not retain its empty painter-only region");
}

// Never-shown HWNDs can return an empty BeginPaint rectangle. Supply the damage input only;
// the real WindowChrome painter, DIBs and upload/fallback calls still execute unchanged.
thread_local! {
    static PAINT_DAMAGE: std::cell::Cell<Option<RECT>> = const { std::cell::Cell::new(None) };
    static OVERLAY_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TIMER_ARMS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static TIMER_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static BUTTON_FAILURES: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}
pub(super) fn take_paint_damage() -> Option<RECT> { PAINT_DAMAGE.with(|slot| slot.take()) }
pub(super) fn take_overlay_failure() -> bool { OVERLAY_FAILURE.with(|slot| slot.replace(false)) }
pub(super) fn take_timer_failure() -> bool { TIMER_FAILURE.with(|slot| slot.replace(false)) }
pub(super) fn take_button_failure() -> bool { BUTTON_FAILURES.with(|slot| { let left = slot.get(); slot.set(left.saturating_sub(1)); left > 0 }) }
pub(super) fn record_timer() { TIMER_ARMS.with(|slot| slot.set(slot.get() + 1)); }

#[test]
fn recovered_surface_fully_repaints_vertical_tabs_after_a_small_dirty_rect() {
    let f = Fixture::new();
    f.resize(1024, 768, crate::settings::TabLayout::Vertical);
    f.paint();
    let expected = with_chrome(f.hwnd, |s| faults::pixels(&s.surface)).unwrap();
    faults::fail_upload();
    f.paint();
    with_chrome(f.hwnd, |s| faults::damage_pixels(&mut s.surface));
    unsafe { window_proc(f.hwnd, WM_TIMER, WPARAM(surface_recovery::TIMER), LPARAM(0)); }
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ValidateRect(Some(f.hwnd), None);
        let _ = InvalidateRect(Some(f.hwnd), Some(&RECT { left: 900, top: 3, right: 910, bottom: 9 }), false);
    }
    PAINT_DAMAGE.with(|slot| slot.set(Some(RECT { left: 950, top: 2, right: 960, bottom: 7 })));
    paint_chrome(f.hwnd, Some(&f.app));
    let actual = with_chrome(f.hwnd, |s| faults::pixels(&s.surface)).unwrap();
    assert!(actual == expected, "all independent painters must restore their whole area");
    assert!(!surface_recovery::needs_emergency(f.hwnd));
}

#[test]
fn healthy_paints_and_stale_timer_messages_never_start_recovery_polling() {
    let f = Fixture::new();
    TIMER_ARMS.with(|slot| slot.set(0));
    f.paint(); f.paint(); f.retry();
    assert_eq!(TIMER_ARMS.with(std::cell::Cell::get), 0);
    assert!(!surface_recovery::needs_emergency(f.hwnd));
}

#[test]
fn upload_failure_has_three_coalesced_retries_then_stops_until_explicit_retry() {
    let f = Fixture::new();
    TIMER_ARMS.with(|slot| slot.set(0));
    faults::fail_upload(); f.paint();
    for expected in 1..=3 {
        f.paint(); f.paint();
        assert_eq!(TIMER_ARMS.with(std::cell::Cell::get), expected);
        faults::fail_upload(); f.retry();
    }
    assert_eq!(TIMER_ARMS.with(std::cell::Cell::get), 3);
    f.retry(); f.paint();
    assert_eq!(TIMER_ARMS.with(std::cell::Cell::get), 3);
    assert!(surface_recovery::needs_emergency(f.hwnd));
    surface_recovery::retry(f.hwnd);
    f.paint();
    assert!(!surface_recovery::needs_emergency(f.hwnd));
}

#[test]
fn initial_failure_recovers_at_the_same_size_and_reclips_the_native_edit() {
    faults::fail_allocation();
    let f = Fixture::new();
    f.paint(); f.retry();
    assert_eq!(with_chrome(f.hwnd, |s| (s.surface.width(), s.surface.height(), s.first_paint_logged)).unwrap(), (800, 600, true));
    let region = unsafe { CreateRectRgn(0, 0, 1, 1) };
    let kind = unsafe { windows::Win32::Graphics::Gdi::GetWindowRgn(f.app.borrow().edit, region) };
    unsafe { let _ = DeleteObject(region.into()); }
    assert_eq!(kind, windows::Win32::Graphics::Gdi::NULLREGION);
}

#[test]
fn missing_overlay_recovers_without_exposing_an_unpainted_popup() {
    let f = Fixture::new();
    with_chrome(f.hwnd, |s| {
        unsafe { let _ = DestroyWindow(s.overlay); }
        s.overlay = HWND::default();
    });
    f.paint();
    OVERLAY_FAILURE.with(|slot| slot.set(true));
    f.retry();
    assert!(surface_recovery::needs_emergency(f.hwnd));
    f.retry();
    assert!(!surface_recovery::needs_emergency(f.hwnd));
    assert!(!with_chrome(f.hwnd, |s| s.overlay.is_invalid()).unwrap());
}

#[test]
fn close_during_surface_recovery_does_not_allocate_or_present_again() {
    faults::fail_allocation();
    let f = Fixture::new();
    f.paint();
    f.app.borrow_mut().closing = true;
    f.retry();
    assert_eq!(with_chrome(f.hwnd, |s| s.surface.width()).unwrap(), 0);
    assert!(!with_chrome(f.hwnd, |s| s.first_paint_logged).unwrap());
}

#[test]
fn skipped_text_is_a_failed_frame_and_is_repainted_on_the_same_size_retry() {
    let f = Fixture::new();
    faults::fail_allocation();
    f.paint();
    assert!(with_chrome(f.hwnd, |s| s.surface.allocation_error().is_some()).unwrap());
    assert!(!with_chrome(f.hwnd, |s| s.first_paint_logged).unwrap());
    f.retry();
    assert!(with_chrome(f.hwnd, |s| s.surface.allocation_error().is_none() && s.first_paint_logged).unwrap());
}

#[test]
fn pending_html_shows_plain_chrome_until_ready_and_native_controls_after_a_failure() {
    let f = Fixture::new();
    f.resize(800, 600, crate::settings::TabLayout::Vertical);
    f.paint();
    let native = with_chrome(f.hwnd, |s| faults::pixels(&s.surface)).unwrap();
    let omnibox = with_chrome(f.hwnd, |s| s.layout.omnibox).unwrap();
    let centre = ((omnibox.left + omnibox.right) / 2, (omnibox.top + omnibox.bottom) / 2);
    // Starting hosts: the native toolbar, rail and sidebar are neither drawn nor clickable.
    with_chrome(f.hwnd, |s| {
        s.html_failed = [false; 3];
        s.chrome_lifetimes.begin(Instant::now());
        s.sync_html_bounds(); s.sync_html_tabs_bounds(); s.sync_html_rail_bounds();
    });
    f.paint();
    if crate::chrome_host::html_ui_enabled() {
        assert!(with_chrome(f.hwnd, |s| faults::pixels(&s.surface)).unwrap() != native,
            "pending hosts show plain chrome, not the native design");
        assert_eq!(hit(f.hwnd, centre.0, centre.1), Hit::None, "the hidden native address field takes no clicks");
    }
    // A failed start falls back to the functional native controls.
    with_chrome(f.hwnd, |s| {
        s.html_failed = [true; 3];
        s.sync_html_bounds(); s.sync_html_tabs_bounds(); s.sync_html_rail_bounds();
    });
    f.paint();
    assert!(with_chrome(f.hwnd, |s| faults::pixels(&s.surface)).unwrap() == native,
        "after a failure the native toolbar, rail and sidebar are back");
    assert_eq!(hit(f.hwnd, centre.0, centre.1), Hit::Omnibox);
}

#[test]
fn fallback_revokes_document_callbacks_before_replacement_creation() {
    let f = Fixture::new();
    with_chrome(f.hwnd, |s| { s.html_failed = [false; 3]; s.chrome_lifetimes.begin(Instant::now()); });
    let (old, commands, _, failure) = chrome_recovery::callbacks(f.hwnd, 0, View::Toolbar, html_command, WM_HTML_CHROME_FAILED).unwrap();
    close_html_chrome(f.hwnd);
    with_chrome(f.hwnd, |s| { s.html_failed = [false; 3]; s.chrome_lifetimes.begin(Instant::now()); });
    commands(ChromeCommand::Ready);
    failure();
    html_created(f.hwnd, old.clone(), Err(windows::core::Error::from_hresult(windows::core::HRESULT(0x80004005_u32 as i32))));
    assert!(!with_chrome(f.hwnd, |s| s.html_page_ready || s.html_failed[0]).unwrap());
    assert!(!chrome_recovery::accepts_failure(f.hwnd, 0, old.token));
    let (_, current, _, _) = chrome_recovery::callbacks(f.hwnd, 0, View::Toolbar, html_command, WM_HTML_CHROME_FAILED).unwrap();
    current(ChromeCommand::Ready);
    let token = with_chrome(f.hwnd, |s| s.chrome_lifetimes.lease(0).token).unwrap();
    chrome_recovery::dispatch(f.hwnd, 0, token);
    assert!(!with_chrome(f.hwnd, |s| s.html_page_ready).unwrap(), "generic ready never grants applied-state ownership");
}

#[test]
fn owner_removal_revokes_callbacks_even_if_an_hwnd_is_reused() {
    let f = Fixture::new();
    with_chrome(f.hwnd, |s| { s.html_failed = [false; 3]; s.chrome_lifetimes.begin(Instant::now()); });
    let (_, commands, _, _) = chrome_recovery::callbacks(f.hwnd, 0, View::Toolbar, html_command, WM_HTML_CHROME_FAILED).unwrap();
    let old = WINDOWS.with(|all| all.borrow_mut().remove(&(f.hwnd.0 as isize))).unwrap();
    let overlay = old.overlay;
    drop(old);
    let mut replacement = WindowChrome::new(800, 600, 96, overlay);
    replacement.chrome_lifetimes.begin(Instant::now());
    WINDOWS.with(|all| all.borrow_mut().insert(f.hwnd.0 as isize, replacement));
    commands(ChromeCommand::Ready);
    assert!(!with_chrome(f.hwnd, |s| s.html_page_ready).unwrap());
}

#[test]
fn creation_or_readiness_timeout_releases_native_controls_and_rejects_late_ready() {
    let f = Fixture::new();
    let now = Instant::now();
    with_chrome(f.hwnd, |s| {
        s.html_failed = [false; 3]; s.chrome_lifetimes.begin(now);
        s.html_env_waiting = true;
    });
    let (_, ready, _, _) = chrome_recovery::callbacks(f.hwnd, 0, View::Toolbar, html_command, WM_HTML_CHROME_FAILED).unwrap();
    chrome_recovery::tick(f.hwnd, now + Duration::from_secs(16));
    ready(ChromeCommand::Ready);
    assert_eq!(with_chrome(f.hwnd, |s| (s.html_failed, s.html_page_ready, s.html_env_waiting)).unwrap(), ([true; 3], false, false));
    assert_ne!(unsafe { GetWindowLongPtrW(f.app.borrow().edit, GWL_STYLE) } as u32 & WS_VISIBLE.0, 0);
    assert!(!with_chrome(f.hwnd, |s| s.html_toolbar_active).unwrap());
}

#[test]
fn emergency_caption_hit_and_native_close_button_do_not_depend_on_the_overlay() {
    use windows::Win32::UI::WindowsAndMessaging::{GetDlgItem, PeekMessageW, PM_REMOVE};
    faults::fail_allocation();
    let f = Fixture::new();
    f.paint();
    let close = unsafe { GetDlgItem(Some(f.hwnd), surface_recovery::CLOSE_ID as i32) }.unwrap();
    assert_eq!(read_edit(close), "Close");
    let mut point = POINT { x: 780, y: 15 };
    unsafe { let _ = ClientToScreen(f.hwnd, &mut point); }
    let packed = LPARAM(((point.y as u16 as u32) << 16 | point.x as u16 as u32) as isize);
    assert_eq!(unsafe { window_proc(f.hwnd, WM_NCHITTEST, WPARAM(0), packed) }.0, HTCLOSE as isize);
    unsafe { window_proc(f.hwnd, WM_COMMAND, WPARAM(surface_recovery::CLOSE_ID), LPARAM(close.0 as isize)); }
    let mut msg = MSG::default();
    assert!(unsafe { PeekMessageW(&mut msg, Some(f.hwnd), WM_CLOSE, WM_CLOSE, PM_REMOVE) }.as_bool(), "native Close routes to WM_CLOSE without a renderer");
}

#[test]
fn exhausted_upload_retries_still_paint_native_control_changes_without_uploading() {
    let f = Fixture::new();
    faults::fail_upload(); f.paint();
    for _ in 0..3 { faults::fail_upload(); f.retry(); }
    let before = with_chrome(f.hwnd, |s| faults::pixels(&s.surface)).unwrap();
    with_chrome(f.hwnd, |s| s.edit_text = "Native controls still update".into());
    f.paint();
    let after = with_chrome(f.hwnd, |s| faults::pixels(&s.surface)).unwrap();
    assert!(before != after, "a retained usable DIB must keep native controls usable after upload retries stop");
    assert!(surface_recovery::needs_emergency(f.hwnd));
    assert!(!with_chrome(f.hwnd, |s| s.first_paint_logged).unwrap());
}

#[test]
fn failure_to_create_the_retry_timer_stops_without_a_message_queue_retry_loop() {
    faults::fail_allocation();
    let f = Fixture::new();
    TIMER_FAILURE.with(|slot| slot.set(true));
    TIMER_ARMS.with(|slot| slot.set(0));
    f.paint();
    for _ in 0..6 { f.retry(); }
    assert_eq!(TIMER_ARMS.with(std::cell::Cell::get), 1);
    assert_eq!(with_chrome(f.hwnd, |s| s.surface.width()).unwrap(), 0);
    surface_recovery::retry(f.hwnd);
    f.paint();
    assert!(!surface_recovery::needs_emergency(f.hwnd));
}

#[test]
fn caption_close_hit_survives_failure_to_create_the_emergency_buttons() {
    faults::fail_allocation();
    let f = Fixture::new();
    BUTTON_FAILURES.with(|slot| slot.set(2));
    f.paint();
    f.paint();
    assert!(unsafe { windows::Win32::UI::WindowsAndMessaging::GetDlgItem(Some(f.hwnd), surface_recovery::CLOSE_ID as i32) }.is_err());
    let mut point = POINT { x: 780, y: 15 };
    unsafe { let _ = ClientToScreen(f.hwnd, &mut point); }
    let packed = LPARAM(((point.y as u16 as u32) << 16 | point.x as u16 as u32) as isize);
    assert_eq!(unsafe { window_proc(f.hwnd, WM_NCHITTEST, WPARAM(0), packed) }.0, HTCLOSE as isize);
}

#[test]
fn recovery_repaints_split_drop_and_tile_titles_outside_the_dirty_rectangle() {
    struct RestoreLook(bool, bool);
    impl Drop for RestoreLook { fn drop(&mut self) { crate::stacks::set_tile_look(self.0, self.1); } }
    let _restore = RestoreLook(crate::stacks::title_dip() > 0, crate::stacks::gap_dip() != crate::split_view::GAP_DIP);
    crate::stacks::set_tile_look(true, false);
    let f = Fixture::new();
    f.resize(1024, 768, crate::settings::TabLayout::Vertical);
    crate::app::test_show_page(&f.app, TabId(0), "https://example.test/fixture");
    with_chrome(f.hwnd, |s| {
        s.split_layout = crate::split_view::SplitView::new(TabId(0), TabId(1), crate::split_view::Pane::Left);
        s.split_drop = Some(crate::split_view::DropZone::BottomRight);
    });
    assert_eq!(with_chrome(f.hwnd, |s| tile_title_bars(f.hwnd, s).len()).unwrap(), 2);
    f.paint();
    let expected = with_chrome(f.hwnd, |s| faults::pixels(&s.surface)).unwrap();
    faults::fail_upload(); f.paint();
    with_chrome(f.hwnd, |s| faults::damage_pixels(&mut s.surface));
    unsafe {
        window_proc(f.hwnd, WM_TIMER, WPARAM(surface_recovery::TIMER), LPARAM(0));
        let _ = windows::Win32::Graphics::Gdi::ValidateRect(Some(f.hwnd), None);
        let _ = InvalidateRect(Some(f.hwnd), Some(&RECT { left: 950, top: 2, right: 960, bottom: 7 }), false);
    }
    PAINT_DAMAGE.with(|slot| slot.set(Some(RECT { left: 950, top: 2, right: 960, bottom: 7 })));
    paint_chrome(f.hwnd, Some(&f.app));
    assert!(with_chrome(f.hwnd, |s| faults::pixels(&s.surface)).unwrap() == expected);
}

#[test]
fn chrome_failure_callback_does_not_borrow_the_app_before_posting() {
    let f = Fixture::new();
    with_chrome(f.hwnd, |s| { s.html_failed = [false; 3]; s.chrome_lifetimes.begin(Instant::now()); });
    let (lease, _, _, failure) = chrome_recovery::callbacks(f.hwnd, 0, View::Toolbar, html_command, WM_HTML_CHROME_FAILED).unwrap();
    let borrowed = f.app.borrow_mut();
    failure();
    drop(borrowed);
    let mut message = MSG::default();
    assert!(unsafe { windows::Win32::UI::WindowsAndMessaging::PeekMessageW(&mut message, Some(f.hwnd),
        WM_HTML_CHROME_FAILED, WM_HTML_CHROME_FAILED, windows::Win32::UI::WindowsAndMessaging::PM_REMOVE) }.as_bool());
    assert_eq!(message.wParam.0, lease.token);
}

#[test]
fn chrome_restoration_generic_ready_does_not_acknowledge_initial_state() {
    let f = Fixture::new();
    with_chrome(f.hwnd, |s| { s.html_failed = [false; 3]; s.chrome_lifetimes.begin(Instant::now()); });
    html_command(f.hwnd, ChromeCommand::Ready);
    html_tabs_command(f.hwnd, ChromeCommand::Ready);
    html_rail_command(f.hwnd, ChromeCommand::Ready);
    assert_eq!(with_chrome(f.hwnd, |s| (s.html_page_ready, s.html_tabs_page_ready, s.html_rail_page_ready)).unwrap(), (false, false, false));
}

#[test]
fn chrome_restoration_command_callback_can_run_with_app_and_window_borrowed() {
    let f = Fixture::new();
    with_chrome(f.hwnd, |s| { s.html_failed = [false; 3]; s.chrome_lifetimes.begin(Instant::now()); });
    let (_, command, _, _) = chrome_recovery::callbacks(f.hwnd, 0, View::Toolbar, html_command, WM_HTML_CHROME_FAILED).unwrap();
    let _app = f.app.borrow_mut();
    WINDOWS.with(|all| {
        let _windows = all.borrow_mut();
        command(ChromeCommand::Ready);
    });
}

#[test]
fn chrome_restoration_environment_wakes_do_not_create_an_engine() {
    let f = Fixture::new();
    with_chrome(f.hwnd, |s| { s.html_requested = true; s.chrome_lifetimes.environments.lost(); });
    for _ in 0..3 {
        html_env_settled(f.hwnd);
        assert!(App::current_chrome_environment(&f.app).is_none());
        assert!(!with_chrome(f.hwnd, |s| s.html_env_waiting || s.html.is_some() || s.html_tabs.is_some() || s.html_rail.is_some()).unwrap());
    }
    let borrowed = f.app.borrow_mut();
    assert!(App::current_chrome_environment(&f.app).is_none());
    drop(borrowed);
}

#[test]
fn chrome_restoration_creation_callback_can_run_with_app_and_window_borrowed() {
    let f = Fixture::new();
    with_chrome(f.hwnd, |s| { s.html_failed = [false; 3]; s.chrome_lifetimes.begin(Instant::now()); });
    let lease = with_chrome(f.hwnd, |s| s.chrome_lifetimes.lease(0)).unwrap();
    let token = lease.token;
    let completion = chrome_recovery::creation(f.hwnd, 0, lease).unwrap();
    let borrowed = f.app.borrow_mut();
    WINDOWS.with(|all| {
        let _windows = all.borrow_mut();
        completion(Err(windows::core::Error::from_hresult(windows::core::HRESULT(0x80004005_u32 as i32))));
    });
    drop(borrowed);
    assert!(!with_chrome(f.hwnd, |s| s.html_failed[0]).unwrap());
    chrome_recovery::dispatch(f.hwnd, 0, token);
    assert!(with_chrome(f.hwnd, |s| s.html_failed[0]).unwrap());
}

#[test]
fn chrome_restoration_native_input_preserves_text_selection_and_composition_until_transfer() {
    let f = Fixture::new();
    let edit = f.app.borrow().edit;
    unsafe {
        SetWindowTextW(edit, w!("Typed native input")).unwrap();
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(2)), Some(LPARAM(8)));
        SendMessageW(edit, 0x010D, None, None); // WM_IME_STARTCOMPOSITION
    }
    assert!(with_chrome(f.hwnd, |s| s.native_ime_composing).unwrap());
    assert_eq!(read_edit(edit), "Typed native input");
    assert_eq!(selection(edit), (2, 8));
    unsafe { SendMessageW(edit, 0x010E, None, None); } // WM_IME_ENDCOMPOSITION
    assert!(!with_chrome(f.hwnd, |s| s.native_ime_composing).unwrap());
    with_chrome(f.hwnd, |s| s.html_input_handoff = true);
    unsafe { SendMessageW(edit, WM_KILLFOCUS, None, None); }
    assert_eq!(read_edit(edit), "Typed native input", "handoff must not replace typed text with a committed URL");
    assert_eq!(selection(edit), (2, 8));
}

#[test]
fn chrome_restoration_modal_dispatch_rechecks_document_authority_before_app_access() {
    let f = Fixture::new();
    let checked = Rc::new(std::cell::Cell::new(false));
    let called = checked.clone();
    with_chrome(f.hwnd, |s| s.html_deferred.push_back(chrome_recovery::DeferredAction {
        command: DeferredChromeCommand::Tool { bar: chrome_host::protocol::ToolBar::Address, index: 0 },
        current: Some(Rc::new(move || { called.set(true); false })),
    }));
    let _borrowed = f.app.borrow_mut();
    run_deferred_html_command(f.hwnd);
    assert!(checked.get());
    assert!(with_chrome(f.hwnd, |s| s.html_deferred.is_empty()).unwrap());
}

// Observe the real default EDIT boundary below the production subclass. These owned, hidden
// controls establish dispatch and retained data, not rendered pixels, caret or IME parity.
thread_local! { static EDIT_PAINT_CALLS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) }; }
unsafe extern "system" fn edit_paint_probe(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM, _: usize, _: usize) -> LRESULT {
    if matches!(msg, windows::Win32::UI::WindowsAndMessaging::WM_PAINT | WM_ERASEBKGND) {
        EDIT_PAINT_CALLS.with(|calls| calls.borrow_mut().push(msg));
    }
    unsafe { DefSubclassProc(hwnd, msg, w, l) }
}
fn request_edit_repaint(edit: HWND) -> Vec<u32> {
    EDIT_PAINT_CALLS.with(|calls| calls.borrow_mut().clear());
    unsafe {
        let dc = windows::Win32::Graphics::Gdi::GetDC(Some(edit));
        SendMessageW(edit, WM_ERASEBKGND, Some(WPARAM(dc.0 as usize)), None);
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(Some(edit), dc);
        let _ = InvalidateRect(Some(edit), None, false);
        SendMessageW(edit, windows::Win32::UI::WindowsAndMessaging::WM_PAINT, None, None);
    }
    EDIT_PAINT_CALLS.with(|calls| calls.borrow().clone())
}
#[test]
fn emergency_edit_uses_production_default_paint_then_restores_painter_only_chrome() {
    faults::fail_allocation();
    let f = Fixture::new();
    let edit = f.app.borrow().edit;
    unsafe {
        SetWindowTextW(edit, w!("Existing address draft")).unwrap();
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(3)), Some(LPARAM(11)));
    }
    f.paint();
    let assert_native = || {
        let calls = request_edit_repaint(edit);
        assert!(calls.contains(&WM_ERASEBKGND), "emergency erase must reach the default EDIT");
        assert!(calls.contains(&windows::Win32::UI::WindowsAndMessaging::WM_PAINT), "emergency paint must reach the default EDIT");
        assert_eq!(read_edit(edit), "Existing address draft");
        assert_eq!(selection(edit), (3, 11));
    };
    assert_native();
    faults::fail_allocation();
    f.retry();
    assert_native();
    // Native painting must not borrow the application or window table across a native call.
    let borrowed_app = f.app.borrow_mut();
    WINDOWS.with(|all| { let _borrowed_windows = all.borrow_mut(); assert_native(); });
    drop(borrowed_app);
    f.retry();
    assert!(request_edit_repaint(edit).is_empty(), "recovered DIB restores painter-only chrome");
    assert_eq!(read_edit(edit), "Existing address draft");
    assert_eq!(selection(edit), (3, 11));
}

#[test]
fn chrome_restoration_slow_environment_does_not_spend_host_creation_deadline() {
    let f = Fixture::new();
    // test_app models a pending fake engine by default; this boundary must not initiate one.
    f.app.borrow_mut().initializing = false;
    with_chrome(f.hwnd, |s| {
        s.html_failed = [false; 3];
        assert!(s.request_html_environment());
    });
    let available = Instant::now() + Duration::from_secs(60);
    chrome_recovery::tick(f.hwnd, available);
    assert_eq!(with_chrome(f.hwnd, |s| (s.html_failed, s.html_env_waiting)).unwrap(), ([false; 3], true));
    assert!(!f.app.borrow().initializing, "the boundary test must not create an engine");
    with_chrome(f.hwnd, |s| {
        assert!(s.begin_html_hosts(7, available));
        assert!(!s.begin_html_hosts(7, available + Duration::from_secs(10)), "duplicate wakes cannot extend a deadline");
        assert!(!s.begin_html_hosts(6, available + Duration::from_secs(10)));
    });
    chrome_recovery::tick(f.hwnd, available + Duration::from_secs(14));
    assert_eq!(with_chrome(f.hwnd, |s| s.html_failed).unwrap(), [false; 3]);
    chrome_recovery::tick(f.hwnd, available + Duration::from_secs(16));
    assert_eq!(with_chrome(f.hwnd, |s| s.html_failed).unwrap(), [true; 3]);
}

#[test]
fn emergency_edit_lease_rejects_closed_and_reused_window_owners() {
    for close in [true, false] {
        faults::fail_allocation();
        let f = Fixture::new();
        let edit = f.app.borrow().edit;
        f.paint();
        assert!(request_edit_repaint(edit).contains(&WM_ERASEBKGND));
        if close {
            with_chrome(f.hwnd, |s| s.surface_recovery.close());
        } else {
            let mut replacement = surface_recovery::Recovery::new();
            replacement.fail(surface_recovery::Fault::Upload);
            // Identical parent and EDIT handles now belong to a new recovery generation.
            // The old subclass cannot borrow the replacement's emergency authorization.
            with_chrome(f.hwnd, |s| s.surface_recovery = replacement);
        }
        assert!(request_edit_repaint(edit).is_empty());
    }
}

#[test]
fn recovery_presentation_unresponsive_page_gets_a_native_wait_reload_bar_that_keeps_the_page() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, IsWindowEnabled, SetFocus};
    let _profile = crate::profile::TestProfileGuard::isolated();
    let f = Fixture::new();
    crate::app::test_show_page(&f.app, TabId(0), "https://example.test/hung");
    crate::app::test_report_unresponsive(&f.app, TabId(0));
    sync_recovery_buttons(f.hwnd, &f.app);
    let prompt = with_chrome(f.hwnd, |s| s.hang_prompts[0]).unwrap();
    assert_eq!(prompt.tab, Some(TabId(0)));
    assert!(with_chrome(f.hwnd, |s| s.recovery_buttons[0].1).unwrap().is_none(), "a hang is not a failure card");
    for control in [prompt.label, prompt.wait, prompt.reload] {
        assert_ne!(unsafe { GetWindowLongPtrW(control, GWL_STYLE) } as u32 & WS_VISIBLE.0, 0);
    }
    assert!(unsafe { IsWindowEnabled(prompt.wait) }.as_bool() && unsafe { IsWindowEnabled(prompt.reload) }.as_bool());
    // Window text is each control's accessible name.
    assert_eq!(read_edit(prompt.wait), crate::app::UNRESPONSIVE_WAIT);
    assert_eq!(read_edit(prompt.reload), crate::app::UNRESPONSIVE_RELOAD);
    assert_eq!(read_edit(prompt.label).trim(), crate::app::UNRESPONSIVE_MESSAGE);
    assert_ne!(unsafe { GetFocus() }, prompt.wait, "showing the prompt never takes the keyboard");
    let region = with_chrome(f.hwnd, |s| s.hang_regions[0]).unwrap().unwrap();
    let viewport = split_viewport(f.hwnd).unwrap();
    assert!(region.top >= viewport.top && region.bottom < viewport.bottom, "a bar at the top, not a card over the page");
    f.paint();
    let mut rect = RECT::default();
    unsafe { GetWindowRect(prompt.reload, &mut rect).unwrap(); }
    let mut point = POINT { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 };
    unsafe { let _ = ScreenToClient(f.hwnd, &mut point); }
    assert_eq!(pixel_alpha(f.hwnd, point.x, point.y), 0, "the layered overlay must expose the native bar");
    // F6 towards the page reaches the prompt; Escape on a button chooses Wait and hides it.
    assert!(hang_prompt::focus(f.hwnd, &f.app));
    // The fixture's parent is never shown, so Windows may refuse it the keyboard; check the
    // move between the two choices whenever focus did arrive.
    if unsafe { GetFocus() } == prompt.wait {
        unsafe { let _ = SendMessageW(prompt.wait, WM_KEYDOWN, Some(WPARAM(0x09)), Some(LPARAM(0))); }
        assert_eq!(unsafe { GetFocus() }, prompt.reload, "Tab moves between the two choices");
    }
    unsafe { let _ = SendMessageW(prompt.reload, WM_KEYDOWN, Some(WPARAM(0x1b)), Some(LPARAM(0))); }
    assert!(!App::unresponsive_prompt(&f.app, TabId(0)));
    assert!(App::tab_has_controller(&f.app, TabId(0)), "Wait keeps the document");
    assert_eq!(unsafe { GetWindowLongPtrW(prompt.wait, GWL_STYLE) } as u32 & WS_VISIBLE.0, 0);
    assert_ne!(unsafe { GetFocus() }, prompt.reload, "focus never stays on a hidden button");
    unsafe { let _ = SetFocus(None); }
}

#[test]
fn recovery_presentation_html_omnibox_draft_and_keyboard_pass_to_the_native_field() {
    let _profile = crate::profile::TestProfileGuard::isolated();
    let f = Fixture::new();
    with_chrome(f.hwnd, |s| {
        assert_eq!(s.take_html_omnibox(), None, "an unfocused omnibox hands nothing over");
        s.html_omnibox_focused = true;
        assert_eq!(s.take_html_omnibox(), Some(None), "focus without typing keeps the shown address");
        s.html_omnibox_focused = true;
        s.omnibox_edited = true;
        s.edit_text = "half typed address".into();
    });
    recover_native_chrome(f.hwnd, &f.app);
    let edit = f.app.borrow().edit;
    assert!(!with_chrome(f.hwnd, |s| s.html_omnibox_focused).unwrap());
    assert_ne!(unsafe { GetWindowLongPtrW(edit, GWL_STYLE) } as u32 & WS_VISIBLE.0, 0);
    // The never-shown fixture is normally not the foreground window, and the draft moves only with
    // the keyboard, so it is checked only where the keyboard could move.
    if unsafe { GetForegroundWindow() } == f.hwnd {
        assert_eq!(read_edit(edit), "half typed address");
    }
}

#[test]
fn recovery_presentation_f6_reaches_a_failure_card_only_while_the_active_page_failed() {
    let _profile = crate::profile::TestProfileGuard::isolated();
    let f = Fixture::new();
    crate::app::test_show_page(&f.app, TabId(0), "https://example.test/healthy");
    sync_recovery_buttons(f.hwnd, &f.app);
    assert!(!hang_prompt::focus(f.hwnd, &f.app), "a healthy page keeps F6 for itself");
    App::tab_render_failed(&f.app, TabId(0));
    sync_recovery_buttons(f.hwnd, &f.app);
    assert!(hang_prompt::focus(f.hwnd, &f.app), "the failure card is the keyboard's way to Retry");
    let other = App::new_tab(&f.app);
    sync_recovery_buttons(f.hwnd, &f.app);
    assert_eq!(App::active_tab_id(&f.app), other);
    assert!(!hang_prompt::focus(f.hwnd, &f.app), "another tab's hidden card never takes the keyboard");
}
