use super::*;

impl App {
    /// Custom engine data folders, benchmarks and guests must never read or write the installed
    /// profile's cache. The worker captures its directory once, rather than consulting global state.
    fn ensure_favicon_service(app: &AppRef) {
        let mut state = app.borrow_mut();
        if state.closing || state.favicon_service.is_some() || state.benchmark_url.is_some() {
            return;
        }
        let Ok(profile) = crate::profile::active() else {
            return;
        };
        if profile.is_guest() || profile.webview2_dir() != state.user_data_dir {
            return;
        }
        let window = state.hwnd.0 as isize;
        state.favicon_service =
            crate::favicon::restore::Service::start(profile.favicons_dir(), move || {
                win32::post(
                    HWND(window as *mut _),
                    win32::WM_RESTORED_FAVICONS,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                );
            });
    }

    pub(super) fn restore_tab_icons(app: &AppRef) {
        Self::ensure_favicon_service(app);
        let state = app.borrow();
        if let Some(service) = &state.favicon_service {
            service.restore_with(
                state
                    .tabs
                    .iter()
                    .filter(|tab| !tab.private && tab.restored_icon_origin.is_some())
                    .filter_map(|tab| tab.url.clone()),
                |page| {
                    state.blocker.as_ref().is_none_or(|blocker| {
                        let blocker = blocker.borrow();
                        crate::favicon::favicon_url_candidates(page.as_str())
                            .iter()
                            .all(|candidate| {
                                !blocker
                                    .should_block(candidate, page.host_str().unwrap_or_default())
                            })
                    })
                },
            );
        }
    }

    /// Called only on the window thread; the worker posts a wake-up without owning an App/COM view.
    pub fn restored_favicons_ready(app: &AppRef) {
        let answers = {
            let state = app.borrow();
            if state.closing {
                return;
            }
            state
                .favicon_service
                .as_ref()
                .map(|service| service.drain())
                .unwrap_or_default()
        };
        for answer in answers {
            Self::apply_restored_favicon(app, answer);
        }
    }

    pub(super) fn apply_restored_favicon(app: &AppRef, answer: crate::favicon::restore::Answer) {
        let Some(png) = answer
            .png
            .filter(|png| crate::chrome_host::favicon_sendable(png))
        else {
            return;
        };
        let key = crate::favicon::key_for(&png);
        let (hwnd, changed) = {
            let mut state = app.borrow_mut();
            if state.closing {
                return;
            }
            let mut changed = Vec::new();
            for tab in &mut state.tabs {
                if !tab.private
                    && tab.discarded
                    && !tab.controller_created
                    && tab.restored_icon_origin.as_deref() == Some(&answer.origin)
                    && tab
                        .url
                        .as_ref()
                        .and_then(crate::favicon::restore::origin)
                        .as_deref()
                        == Some(&answer.origin)
                    && tab.favicon.as_ref() != Some(&png)
                {
                    tab.favicon = Some(png.clone());
                    tab.favicon_key = Some(key);
                    changed.push(tab.id);
                }
            }
            for id in &changed {
                state.tab_model.set_favicon(*id, Some(key));
            }
            (state.hwnd, changed)
        };
        for id in changed {
            Self::post_tab_changed_kind(hwnd, id, false, false);
        }
    }

    pub(super) fn page_favicon_changed(app: &AppRef, id: TabId, page: &url::Url, bytes: Vec<u8>) {
        let origin = {
            let mut state = app.borrow_mut();
            if state.closing {
                return;
            }
            let Some(tab) = state
                .tabs
                .iter_mut()
                .find(|tab| tab.id == id && tab.url.as_ref() == Some(page))
            else {
                return;
            };
            tab.restored_icon_origin = None;
            tab.favicon_key = Some(crate::favicon::key_for(&bytes));
            tab.favicon = Some(bytes.clone());
            (!tab.private)
                .then(|| crate::favicon::restore::origin(page))
                .flatten()
        };
        if let Some(origin) = origin {
            Self::ensure_favicon_service(app);
            if let Some(service) = &app.borrow().favicon_service {
                service.save(origin, bytes);
            }
        }
        Self::post_tab_changed_kind(app.borrow().hwnd, id, false, false);
        Self::note_favicon_colour(app, id);
    }
}
