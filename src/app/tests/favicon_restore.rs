use super::*;
use crate::favicon::restore::{Answer, Service};

const PNG: &[u8] = include_bytes!("../../../icons/history-favicon.png");
const OTHER: &[u8] = include_bytes!("../../../icons/settings-favicon.png");

fn profile(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bowser-tab-icons-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn worker(dir: &std::path::Path) -> Service {
    Service::start_with(dir.to_owned(), || {}, |_, _| None).unwrap()
}

fn wait_for(mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("favicon worker did not finish");
}

fn set_url(app: &AppRef, id: TabId, text: &str) -> url::Url {
    let url = url::Url::parse(text).unwrap();
    let mut state = app.borrow_mut();
    state.tab_model.set_url(id, Some(text));
    state.tabs.iter_mut().find(|tab| tab.id == id).unwrap().url = Some(url.clone());
    url
}

#[test]
fn restart_restores_site_icons_without_activating_or_creating_sleeping_tabs() {
    let dir = profile("restart");
    let first = app_with_pending(rect(800, 600));
    first.borrow_mut().favicon_service = Some(worker(&dir));
    let active = App::active_tab_id(&first);
    let sleeping = App::new_tab(&first);
    let first_url = set_url(&first, active, "https://active.example/");
    let sleeping_url = set_url(
        &first,
        sleeping,
        "https://sleeping.example/private?q=secret#fragment",
    );
    App::page_favicon_changed(&first, active, &first_url, OTHER.to_vec());
    App::page_favicon_changed(&first, sleeping, &sleeping_url, PNG.to_vec());
    first.borrow_mut().tab_model.select(active);
    let text = App::session_snapshot(&first).unwrap().serialize();
    assert!(
        !text.contains("data:image"),
        "icons live in the bounded profile cache, not session text"
    );
    wait_for(|| {
        std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|file| file.path().extension().is_some_and(|ext| ext == "ico"))
            .count()
            == 2
    });
    drop(first);

    let restored = app_with_pending(rect(800, 600));
    restored.borrow_mut().favicon_service = Some(worker(&dir));
    restored.borrow_mut().pending_restore = crate::session::Session::parse(&text);
    App::restore_pending_session(&restored);
    let ids = restored
        .borrow()
        .tab_model
        .iter()
        .map(|tab| tab.id)
        .collect::<Vec<_>>();
    let active = App::active_tab_id(&restored);
    wait_for(|| {
        App::restored_favicons_ready(&restored);
        App::tab_favicon(&restored, ids[1]).as_deref() == Some(PNG)
    });
    assert_eq!(App::active_tab_id(&restored), active);
    let state = restored.borrow();
    let sleep = state.tabs.iter().find(|tab| tab.id == ids[1]).unwrap();
    assert!(sleep.discarded && !sleep.controller_created);
    assert!(!sleep.loading && !sleep.page_usable);
    assert!(state.engine.is_none() && state.tab_state.eager.is_empty());
    assert_eq!(
        state.tab_model.get(ids[1]).unwrap().favicon,
        Some(crate::favicon::key_for(PNG))
    );
    let wire = crate::chrome_host::protocol::fav_json(ids[1].0, PNG);
    assert!(
        wire.is_some(),
        "restored icon passes the same gate as HTML chrome"
    );
}

#[test]
fn failed_or_late_results_preserve_newer_icons_and_ignore_navigation_private_closed_and_closing_tabs()
 {
    let app = app_with_pending(rect(800, 600));
    let id = App::active_tab_id(&app);
    let url = set_url(&app, id, "https://saved.example/");
    {
        let mut state = app.borrow_mut();
        let tab = state.tabs.iter_mut().find(|tab| tab.id == id).unwrap();
        tab.discarded = true;
        tab.restored_icon_origin = Some("https://saved.example".into());
    }
    let answer = |png| Answer {
        origin: "https://saved.example".into(),
        png,
    };
    App::apply_restored_favicon(&app, answer(Some(PNG.to_vec())));
    App::apply_restored_favicon(&app, answer(None));
    assert_eq!(App::tab_favicon(&app, id).as_deref(), Some(PNG));
    App::page_favicon_changed(&app, id, &url, OTHER.to_vec());
    App::apply_restored_favicon(&app, answer(Some(PNG.to_vec())));
    assert_eq!(
        App::tab_favicon(&app, id).as_deref(),
        Some(OTHER),
        "a restored answer cannot replace the live favicon"
    );

    for guard in ["navigation", "private", "controller", "closed", "closing"] {
        let app = app_with_pending(rect(800, 600));
        set_url(&app, id, "https://saved.example/");
        {
            let mut state = app.borrow_mut();
            let tab = &mut state.tabs[0];
            tab.discarded = true;
            tab.restored_icon_origin = Some("https://saved.example".into());
            match guard {
                "navigation" => tab.url = Some(url::Url::parse("https://new.example/").unwrap()),
                "private" => tab.private = true,
                "controller" => tab.controller_created = true,
                "closed" => state.tabs.clear(),
                "closing" => state.closing = true,
                _ => unreachable!(),
            }
        }
        App::apply_restored_favicon(&app, answer(Some(PNG.to_vec())));
        assert_eq!(App::tab_favicon(&app, id), None, "{guard}");
    }
}

#[test]
fn private_tabs_and_outdated_webview_callbacks_do_not_persist() {
    let dir = profile("private");
    let app = app_with_pending(rect(800, 600));
    app.borrow_mut().favicon_service = Some(worker(&dir));
    let id = App::active_tab_id(&app);
    let page = set_url(&app, id, "https://private.example/");
    app.borrow_mut().tabs[0].private = true;
    App::page_favicon_changed(&app, id, &page, PNG.to_vec());
    assert_eq!(App::tab_favicon(&app, id).as_deref(), Some(PNG));
    app.borrow_mut().tabs[0].private = false;
    set_url(&app, id, "https://different.example/");
    App::page_favicon_changed(&app, id, &page, OTHER.to_vec());
    assert_eq!(App::tab_favicon(&app, id).as_deref(), Some(PNG));
    // Enqueue a normal icon behind the refused ones, then wait for its write as a barrier.
    let normal = set_url(&app, id, "https://normal.example/");
    App::page_favicon_changed(&app, id, &normal, OTHER.to_vec());
    wait_for(|| {
        std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .any(|file| file.path().extension().is_some_and(|ext| ext == "ico"))
    });
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().flatten().count(),
        1,
        "only the normal current page was persisted"
    );
}
