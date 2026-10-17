//! What the theme options do to the running app (src/app/theme_features.rs): each option reaches the
//! palettes, the look or the settings the chrome reads, driven with a scripted engine and no window.

use super::*;
use crate::{
    settings::{Settings, Theme},
    theme::{self, Appearance, Rgba},
    theme_custom::{self, Colouring, Schedule},
    win32::glass,
};

/// Answers every reader script with `answer` and records the scripts it was asked to run.
struct ScriptedEngine {
    answer: Rc<RefCell<Option<String>>>,
    scripts: Rc<RefCell<Vec<String>>>,
}

impl EngineAdapter for ScriptedEngine {
    fn navigate(&mut self, _url: &url::Url) -> windows::core::Result<()> {
        Ok(())
    }
    fn can_go_back(&self) -> bool {
        false
    }
    fn go_back(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn can_go_forward(&self) -> bool {
        false
    }
    fn go_forward(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn resize(&mut self, _bounds: windows::Win32::Foundation::RECT) -> windows::core::Result<()> {
        Ok(())
    }
    fn set_memory_usage_target(&mut self, _target: MemoryUsageTarget) -> windows::core::Result<MemoryTargetApply> {
        Ok(MemoryTargetApply::Applied)
    }
    fn close(&mut self) -> windows::core::Result<()> {
        Ok(())
    }
    fn run_reader_script(&mut self, _tab: TabId, script: &str, done: Box<dyn FnOnce(Option<String>)>) {
        self.scripts.borrow_mut().push(script.to_owned());
        done(self.answer.borrow().clone());
    }
}

type Fixture = (AppRef, Rc<RefCell<Option<String>>>, Rc<RefCell<Vec<String>>>);

fn fixture() -> Fixture {
    let app = app_with_pending(rect(800, 600));
    let answer = Rc::new(RefCell::new(None));
    let scripts = Rc::new(RefCell::new(Vec::new()));
    app.borrow_mut().engine = Some(Box::new(ScriptedEngine { answer: answer.clone(), scripts: scripts.clone() }));
    (app, answer, scripts)
}

fn option(app: &AppRef, text: &str) {
    assert!(App::set_theme_option(app, text), "{text}");
}

#[test]
fn a_colour_option_reaches_the_palette_every_surface_reads() {
    let app = app_with_pending(rect(800, 600));
    let base = theme::palette_base(Appearance::Dark, true).clone();
    assert_eq!(theme::palette(Appearance::Dark, true), &base, "nothing chosen: Bowser's own look");

    option(&app, "theme_background=#102030");
    assert_eq!(App::settings(&app).themes.background, Some(Rgba::hex(0x102030, 0xFF)));
    assert_eq!(theme::palette(Appearance::Dark, true).window, Rgba::hex(0x102030, 0xFF), "menus, dialogs and the GDI chrome follow");
    assert_eq!(App::themed_palette(&app, Appearance::Dark, true).0.window, Rgba::hex(0x102030, 0xFF), "and the HTML hosts");
    assert_eq!(glass::appearance(), Appearance::Dark, "a dark background brings the dark family");

    option(&app, "theme_background=");
    assert_eq!(theme::palette(Appearance::Dark, true), &base, "clearing it puts Bowser's look back");

    option(&app, "theme_foreground=#aabbcc");
    assert_eq!(theme::palette(Appearance::Dark, true).tab_text_active, Rgba::hex(0xAABBCC, 0xFF));
    option(&app, "theme_highlight=#ffcc00");
    let selection = theme::palette(Appearance::Dark, false).omnibox_selection;
    assert_eq!((selection.r, selection.g, selection.b), (0xFF, 0xCC, 0x00));
    let flat = theme::palette(Appearance::Dark, true).omnibox_selection;
    assert_eq!(flat.a, 255, "solid palettes flatten it over the field");
    assert!(flat.r > flat.b, "and it is still the highlight's colour: {flat:?}");
}

#[test]
fn a_refused_value_changes_nothing() {
    let app = app_with_pending(rect(800, 600));
    option(&app, "theme_colouring=tab_bar");
    let before = App::settings(&app).themes;
    for bad in ["theme_colouring=sideways", "theme_background=red", "theme_saturation=400", "theme_nothing=1", "theme_dark_from=7:00", "no equals sign"] {
        assert!(!App::set_theme_option(&app, bad), "{bad}");
    }
    assert_eq!(App::settings(&app).themes, before);
    // The two schedule times may not meet.
    assert!(!App::set_theme_option(&app, "theme_dark_from=07:00"), "the light time is 07:00");
    option(&app, "theme_dark_from=21:30");
    assert_eq!(App::settings(&app).themes.dark_from, 21 * 60 + 30);
}

#[test]
fn the_colouring_mode_tints_the_bar_it_names() {
    let app = app_with_pending(rect(800, 600));
    option(&app, "theme_accent_page=false");
    app.borrow_mut().settings.theme_accent = Some("#d03030".to_owned());
    let base = theme::palette_base(Appearance::Light, true).clone();
    let with = |mode: &str| {
        option(&app, &format!("theme_colouring={mode}"));
        App::themed_palette(&app, Appearance::Light, true).0
    };
    let tab = with("tab_bar");
    assert_ne!(tab.window, base.window);
    assert_eq!(tab.omnibox_fill, base.omnibox_fill);
    let address = with("address_bar");
    assert_eq!(address.window, base.window);
    assert_ne!(address.omnibox_fill, base.omnibox_fill);
    let both = with("unified");
    assert_eq!((both.window, both.omnibox_fill), (tab.window, address.omnibox_fill));
    let off = with("off");
    assert_eq!((off.window, off.omnibox_fill), (base.window, base.omnibox_fill));
    // The native surfaces follow the mode too.
    option(&app, "theme_colouring=tab_bar");
    assert_ne!(theme::palette(Appearance::Light, true).window, base.window);
}

#[test]
fn the_accent_saturation_limit_reaches_the_accent_the_chrome_uses() {
    let app = app_with_pending(rect(800, 600));
    app.borrow_mut().settings.theme_accent = Some("#e02020".to_owned());
    let full = App::user_theme(&app).accent.expect("accent");
    assert_eq!(full, Rgba::hex(0xE02020, 0xFF));
    option(&app, "theme_saturation=30");
    let limited = App::user_theme(&app).accent.expect("accent");
    assert_ne!(limited, full);
    let spread = |c: Rgba| c.r.max(c.g).max(c.b) - c.r.min(c.g).min(c.b);
    assert!(spread(limited) < spread(full) / 2, "{limited:?} is greyer than {full:?}");
    assert_eq!(
        App::themed_palette(&app, Appearance::Light, true).0.progress_fill.r.max(1) > 0,
        true
    );
    option(&app, "theme_saturation=100");
    assert_eq!(App::user_theme(&app).accent, Some(full));
}

#[test]
fn the_contrast_option_raises_the_text_contrast_the_chrome_keeps() {
    let app = app_with_pending(rect(800, 600));
    option(&app, "theme_background=#ffffff");
    option(&app, "theme_foreground=#6b6b6b");
    let text = |p: &theme::Palette| theme::worst_contrast(p.tab_text_active, &[vec![p.window, p.tab_active]]);
    let standard = App::themed_palette(&app, Appearance::Light, true).0;
    assert!(text(&standard) >= 4.5 && text(&standard) < 7.0);
    option(&app, "theme_contrast=high");
    let high = App::themed_palette(&app, Appearance::Light, true).0;
    assert!(text(&high) >= 7.0, "{:.2}", text(&high));
}

#[test]
fn a_private_tabs_own_look_keeps_its_family_when_a_background_is_chosen() {
    let app = app_with_pending(rect(800, 600));
    option(&app, "theme_background=#ffffff");
    option(&app, "theme_highlight=#ffcc00");
    let (dark, look) = App::themed_palette(&app, Appearance::Dark, false);
    assert_eq!(look, Appearance::Dark);
    assert_ne!(dark.window, Rgba::hex(0xFFFFFF, 0xFF), "a light background does not apply to the dark family");
    assert_eq!(dark.window, theme::palette_base(Appearance::Dark, false).window);
    assert_eq!((dark.omnibox_selection.r, dark.omnibox_selection.g), (0xFF, 0xCC), "but the rest of the colours do");
}

#[test]
fn the_schedule_sets_the_look_the_window_uses() {
    let app = app_with_pending(rect(800, 600));
    option(&app, "theme_schedule=none");
    option(&app, "theme_look=dark");
    assert_eq!(glass::appearance(), Appearance::Dark, "no schedule keeps the chosen look, whatever Windows says");
    option(&app, "theme_look=light");
    assert_eq!(glass::appearance(), Appearance::Light);
    option(&app, "theme_schedule=system");
    assert_eq!(App::settings(&app).themes.schedule, Schedule::System);
    assert_eq!(glass::appearance(), crate::theme::system_appearance(), "Windows' own choice again");

    // The time schedule: the answer for any minute is what `scheduled_look` says, and the window uses it.
    option(&app, "theme_light_from=00:00");
    option(&app, "theme_dark_from=23:59");
    option(&app, "theme_schedule=time");
    let now = glass::local_minutes();
    let expected = App::settings(&app).themes.scheduled_look(now).expect("a schedule decides");
    assert_eq!(glass::appearance(), expected);
    // A schedule that is always dark right now: dark from the minute after `now`'s light start.
    let mut dark_now = App::settings(&app).themes;
    dark_now.light_from = (now + 1) % (24 * 60);
    dark_now.dark_from = (now + 2) % (24 * 60);
    assert_eq!(dark_now.scheduled_look(now), Some(Appearance::Dark));
    app.borrow_mut().settings.themes = dark_now;
    App::theme_tick(&app);
    assert_eq!(glass::appearance(), Appearance::Dark, "the timer switches the window to what the time says");
    let mut light_now = App::settings(&app).themes;
    light_now.light_from = now;
    light_now.dark_from = (now + 1) % (24 * 60);
    app.borrow_mut().settings.themes = light_now;
    App::theme_tick(&app);
    assert_eq!(glass::appearance(), Appearance::Light);
}

#[test]
fn saving_the_page_s_settings_takes_the_theme_options_into_memory() {
    let app = app_with_pending(rect(800, 600));
    let mut file = Settings::default();
    file.theme = Theme::System;
    file.themes.set("theme_background", "#203040");
    file.themes.set("theme_colouring", "address_bar");
    App::apply_live_settings(&app, &file);
    let now = App::settings(&app);
    assert_eq!(now.themes, file.themes);
    assert_eq!(theme::palette(Appearance::Dark, true).window, Rgba::hex(0x203040, 0xFF), "and into the installed palettes");
    // Reset settings puts them back.
    let reset = crate::settings::reset_defaults(&now);
    App::apply_live_settings(&app, &reset);
    assert_eq!(App::settings(&app).themes, theme_custom::ThemeOptions::default());
    assert_eq!(theme::palette(Appearance::Dark, true), theme::palette_base(Appearance::Dark, true));
}

#[test]
fn reset_theme_settings_puts_the_whole_theme_back_and_only_it() {
    let app = app_with_pending(rect(800, 600));
    option(&app, "theme_background=#102030");
    option(&app, "theme_schedule=none");
    option(&app, "theme_look=dark");
    option(&app, "theme_saturation=20");
    {
        let mut state = app.borrow_mut();
        state.settings.theme = Theme::System;
        state.settings.theme_accent = Some("#112233".to_owned());
        state.settings.website_appearance = crate::settings::WebsiteAppearance::Dark;
    }
    App::reset_theme(&app);
    let now = App::settings(&app);
    assert_eq!(now.theme, Theme::Helium);
    assert_eq!(now.theme_accent, None);
    assert_eq!(now.themes, theme_custom::ThemeOptions::default());
    assert_eq!(now.website_appearance, crate::settings::WebsiteAppearance::Dark, "website appearance is not part of it");
    assert_eq!(theme::palette(Appearance::Dark, true), theme::palette_base(Appearance::Dark, true));
    assert_eq!(glass::appearance(), crate::theme::system_appearance(), "the schedule is back to Windows");
}

#[test]
fn an_exported_theme_imports_back_and_a_bad_file_changes_nothing() {
    let (from, ..) = fixture();
    option(&from, "theme_background=#202830");
    option(&from, "theme_foreground=#eeeeee");
    option(&from, "theme_colouring=address_bar");
    option(&from, "theme_saturation=60");
    from.borrow_mut().settings.theme_accent = Some("#aa5500".to_owned());
    let text = App::export_theme_text(&from);

    let to = app_with_pending(rect(800, 600));
    option(&to, "theme_schedule=none");
    App::import_theme(&to, text.as_bytes()).expect("a file Bowser wrote");
    let now = App::settings(&to);
    assert_eq!(now.themes.background, Some(Rgba::hex(0x202830, 0xFF)));
    assert_eq!(now.themes.colouring, Colouring::AddressBar);
    assert_eq!(now.themes.saturation, 60);
    assert_eq!(now.theme_accent.as_deref(), Some("#aa5500"));
    assert_eq!(now.themes.schedule, Schedule::None, "the importing profile keeps its own schedule");
    assert_eq!(theme::palette(Appearance::Dark, true).window, Rgba::hex(0x202830, 0xFF), "the imported colours are live");

    let before = App::settings(&to);
    for bad in [&b"hello"[..], b"bowser_theme=1\ntheme_background=url(http://x)\n", b"bowser_theme=1\ndownload_dir=C:\\x\n", &[0xFF, 0xFE, 0x00], &vec![b'#'; 4000]] {
        assert!(App::import_theme(&to, bad).is_err());
    }
    assert_eq!(App::settings(&to), before, "a refused file changes nothing");
}

#[test]
fn the_accent_follows_the_page_only_while_the_option_is_on() {
    let (app, answer, scripts) = fixture();
    let tab = App::active_tab_id(&app);
    *answer.borrow_mut() = Some("\"#1a73e8\"".to_owned());

    App::sample_page_accent(&app, tab);
    assert!(scripts.borrow().is_empty(), "off: the page is not asked at all");
    assert_eq!(App::user_theme(&app).accent, None);

    option(&app, "theme_accent_page=true");
    app.borrow_mut().tabs.iter_mut().find(|t| t.id == tab).unwrap().url = Some(url::Url::parse("https://example.test/").unwrap());
    App::sample_page_accent(&app, tab);
    assert_eq!(scripts.borrow().len(), 1);
    assert_eq!(scripts.borrow()[0], theme_custom::PAGE_COLOUR_SCRIPT);
    assert_eq!(App::page_accent(tab), Some(Rgba::hex(0x1a73e8, 0xFF)));
    assert_eq!(App::user_theme(&app).accent, Some(Rgba::hex(0x1a73e8, 0xFF)), "the page's colour is the accent");
    assert_eq!(App::themed_palette(&app, Appearance::Light, true).0.progress_fill, Rgba::hex(0x1a73e8, 0xFF));

    // The accent is the active tab's: another tab's colour does not leak into this one.
    let other = App::new_tab(&app);
    let _ = App::select_tab(&app, tab);
    App::set_page_accent(&app, other, |accent| {
        accent.meta = Some(Rgba::hex(0xFF0000, 0xFF));
    });
    assert_eq!(App::user_theme(&app).accent, Some(Rgba::hex(0x1a73e8, 0xFF)));
    let _ = App::select_tab(&app, other);
    assert_eq!(App::user_theme(&app).accent, Some(Rgba::hex(0xFF0000, 0xFF)));

    // A page without one hands back nothing: the usual accent returns. The favicon fills in meanwhile.
    let _ = App::select_tab(&app, tab);
    *answer.borrow_mut() = Some("\"\"".to_owned());
    App::set_page_accent(&app, tab, |accent| accent.icon = Some(Rgba::hex(0x00AA00, 0xFF)));
    App::sample_page_accent(&app, tab);
    assert_eq!(App::user_theme(&app).accent, Some(Rgba::hex(0x00AA00, 0xFF)), "no theme-color: the favicon's colour");
    App::set_page_accent(&app, tab, |accent| accent.icon = None);
    assert_eq!(App::user_theme(&app).accent, None);

    // Turning it off puts the settings' accent back at once.
    *answer.borrow_mut() = Some("\"#1a73e8\"".to_owned());
    App::sample_page_accent(&app, tab);
    assert_eq!(App::user_theme(&app).accent, Some(Rgba::hex(0x1a73e8, 0xFF)));
    option(&app, "theme_accent_page=false");
    assert_eq!(App::user_theme(&app).accent, None);
}

#[test]
fn the_page_accent_tints_the_bar_the_colouring_mode_names() {
    let (app, answer, _) = fixture();
    let tab = App::active_tab_id(&app);
    app.borrow_mut().tabs.iter_mut().find(|t| t.id == tab).unwrap().url = Some(url::Url::parse("https://example.test/").unwrap());
    option(&app, "theme_accent_page=true");
    option(&app, "theme_colouring=tab_bar");
    *answer.borrow_mut() = Some("\"#cc2222\"".to_owned());
    App::sample_page_accent(&app, tab);
    let red = App::themed_palette(&app, Appearance::Light, true).0;
    *answer.borrow_mut() = Some("\"#2222cc\"".to_owned());
    App::sample_page_accent(&app, tab);
    let blue = App::themed_palette(&app, Appearance::Light, true).0;
    assert_ne!(red.window, blue.window, "each page tints the tab bar its own colour");
    assert!(red.window.r > red.window.b && blue.window.b > blue.window.r);
}
