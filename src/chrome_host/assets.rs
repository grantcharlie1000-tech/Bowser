//! Embedded HTML chrome assets, served to the chrome WebView2 through the `bowser-ui` scheme.
//!
//! Everything is compiled in with `include_bytes!` from `ui/dist/`; nothing is read from disk at
//! runtime. The table is closed: any path not listed here is a 404 for the caller.

pub const SCHEME: &str = "bowser-ui";
/// Start URL of the toolbar host. The page reads `view` from `location.search`.
pub const START_URL: &str = "bowser-ui://app/index.html?view=toolbar";
/// Start URL of the tab strip host. Same bundle as `START_URL`, different view.
pub const TABS_START_URL: &str = "bowser-ui://app/index.html?view=tabs";
/// Start URL of the left rail host. Same bundle as `START_URL`, different view.
pub const RAIL_START_URL: &str = "bowser-ui://app/index.html?view=rail";
pub const PANEL_START_URL: &str = "bowser-ui://app/index.html?view=panel";
pub const POPUP_START_URL: &str = "bowser-ui://app/index.html?view=popup";
pub const DIALOG_START_URL: &str = "bowser-ui://app/index.html?view=dialog";
pub const NOTES_DIALOG_START_URL: &str = "bowser-ui://app/index.html?view=dialog&p=notes";
/// The settings page over a `bowser://settings` tab (docs/design/settings-page.md).
pub const SETTINGS_START_URL: &str = "bowser-ui://app/index.html?view=settings";
/// The history page over a `bowser://history` tab (docs/design/history-page.md).
pub const HISTORY_PAGE_START_URL: &str = "bowser-ui://app/index.html?view=history";
/// The downloads page over a `bowser://downloads` tab (docs/design/downloads.md).
pub const DOWNLOADS_PAGE_START_URL: &str = "bowser-ui://app/index.html?view=downloads";
pub const PRIVACY_DIALOG_START_URL: &str = "bowser-ui://app/index.html?view=dialog&p=privacy";
pub const SESSIONS_DIALOG_START_URL: &str = "bowser-ui://app/index.html?view=dialog&p=sessions";
pub const RENAME_DIALOG_START_URL: &str = "bowser-ui://app/index.html?view=dialog&p=rename";
/// Start URL of the no-activate hover layer (tooltips, tab hover card, island menu).
pub const HOVER_START_URL: &str = "bowser-ui://app/index.html?view=hover";
/// The split view's empty side and drop-zone preview (`View::Split`).
pub const SPLIT_START_URL: &str = "bowser-ui://app/index.html?view=split";

const HTML: &str = "text/html;charset=utf-8";
const JS: &str = "text/javascript;charset=utf-8";
const CSS: &str = "text/css;charset=utf-8";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asset {
    pub bytes: &'static [u8],
    pub mime: &'static str,
}

/// Headers sent with every asset, CRLF-separated for `CreateWebResourceResponse`. The caller adds
/// `Content-Type` from `Asset::mime`.
pub const RESPONSE_HEADERS: &str = concat!(
    "Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; require-trusted-types-for 'script'; trusted-types 'none'\r\n",
    "X-Content-Type-Options: nosniff\r\n",
    "Cache-Control: no-store",
);

/// Maps a request path (as seen after the scheme and host) to an embedded asset.
///
/// A leading slash is optional, and any query or fragment is ignored. The empty path and
/// `index.html` both return the page. Anything containing `..` or a backslash is rejected before
/// the path is interpreted.
pub fn lookup(path: &str) -> Option<Asset> {
    if path.contains("..") || path.contains('\\') {
        return None;
    }
    let path = path.split(['?', '#']).next()?;
    let name = path.strip_prefix('/').unwrap_or(path);
    match name {
        "" | "index.html" => Some(Asset {
            bytes: include_bytes!("../../ui/dist/index.html"),
            mime: HTML,
        }),
        "app.js" => Some(Asset {
            bytes: include_bytes!("../../ui/dist/app.js"),
            mime: JS,
        }),
        "app.css" => Some(Asset {
            bytes: include_bytes!("../../ui/dist/app.css"),
            mime: CSS,
        }),
        // ES modules app.js imports only when the settings or history view opens (ui/build.mjs).
        "settings-page.js" => Some(Asset {
            bytes: include_bytes!("../../ui/dist/settings-page.js"),
            mime: JS,
        }),
        "history-page.js" => Some(Asset {
            bytes: include_bytes!("../../ui/dist/history-page.js"),
            mime: JS,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_dist_matches_manifest() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest = std::fs::read_to_string(root.join("ui/dist.manifest")).expect("manifest");
        let expected = super::super::fingerprint::manifest_value(&manifest, "dist").expect("dist entry");
        let actual = super::super::fingerprint::tree_fingerprint(&root.join("ui/dist")).expect("dist fingerprint");
        assert_eq!(actual, expected, "run npm run build in ui after changing built assets");
    }

    #[test]
    fn root_and_index_serve_the_page() {
        for path in ["/", "/index.html", "index.html", ""] {
            let asset = lookup(path).unwrap_or_else(|| panic!("{path:?} not found"));
            assert_eq!(asset.mime, HTML, "{path:?}");
            assert!(!asset.bytes.is_empty(), "{path:?}");
            assert_eq!(asset.bytes, include_bytes!("../../ui/dist/index.html"));
        }
    }

    #[test]
    fn scripts_and_styles_have_their_mime_types() {
        let js = lookup("/app.js").expect("app.js");
        assert_eq!(js.mime, JS);
        assert_eq!(js.bytes, include_bytes!("../../ui/dist/app.js"));
        assert_eq!(lookup("app.js"), Some(js));

        let css = lookup("/app.css").expect("app.css");
        assert_eq!(css.mime, CSS);
        assert_eq!(css.bytes, include_bytes!("../../ui/dist/app.css"));
        assert_eq!(lookup("app.css"), Some(css));
    }

    #[test]
    fn page_modules_are_scripts_apart_from_app_js() {
        let settings = lookup("/settings-page.js").expect("settings-page.js");
        assert_eq!(settings.mime, JS);
        assert_eq!(settings.bytes, include_bytes!("../../ui/dist/settings-page.js"));
        assert_eq!(lookup("settings-page.js?v=2"), Some(settings));

        let history = lookup("/history-page.js").expect("history-page.js");
        assert_eq!(history.mime, JS);
        assert_eq!(history.bytes, include_bytes!("../../ui/dist/history-page.js"));
        assert_eq!(lookup("history-page.js#x"), Some(history));

        let app = lookup("/app.js").expect("app.js");
        assert_ne!(settings.bytes, app.bytes);
        assert_ne!(history.bytes, app.bytes);
    }

    #[test]
    fn query_and_fragment_are_ignored() {
        assert_eq!(lookup("/index.html?x=1#frag"), lookup("/index.html"));
        assert_eq!(lookup("/app.js?v=2"), lookup("/app.js"));
        assert_eq!(lookup("/?cache=no"), lookup("/"));
        assert_eq!(lookup("/#top"), lookup("/"));
    }

    #[test]
    fn unknown_paths_are_not_found() {
        for path in [
            "/missing.html",
            "/app.js.map",
            "/APP.JS",
            "/index",
            "/index.html/",
            "//index.html",
            "/ui/dist/app.js",
            "/app.js ",
            "/settings-page.js/",
            "/SETTINGS-PAGE.JS",
            "/history-page",
            "/history-page.js.map",
            "/ui/dist/settings-page.js",
            "/settings-page.ts",
        ] {
            assert_eq!(lookup(path), None, "{path:?}");
        }
    }

    #[test]
    fn traversal_is_rejected() {
        for path in [
            "/../app.js",
            "../app.js",
            "/..%2fapp.js",
            "/a/../app.js",
            "/app.js/..",
            "/..",
            "..",
            "/app.js?../x",
            "/app.js#..",
            "\\app.js",
            "/..\\app.js",
            "/app.js\\..",
            "/../settings-page.js",
            "/a/../history-page.js",
            "/settings-page.js/..",
            "/history-page.js?../x",
            "/..\\settings-page.js",
        ] {
            assert_eq!(lookup(path), None, "{path:?}");
        }
    }

    #[test]
    fn response_headers_are_crlf_joined_without_bare_newlines() {
        let lines: Vec<&str> = RESPONSE_HEADERS.split("\r\n").collect();
        assert_eq!(lines.len(), 3);
        assert!(!RESPONSE_HEADERS.replace("\r\n", "").contains('\n'));
        assert_eq!(lines[1], "X-Content-Type-Options: nosniff");
        assert_eq!(lines[2], "Cache-Control: no-store");
        assert!(lines[0].starts_with("Content-Security-Policy: default-src 'none';"));
        assert!(lines[0].ends_with("trusted-types 'none'"));
        assert!(lines.iter().all(|line| !line.starts_with("Content-Type:")));
    }

    #[test]
    fn scheme_and_start_url_agree() {
        assert!(START_URL.starts_with(&format!("{SCHEME}://app/")));
        assert!(TABS_START_URL.starts_with(&format!("{SCHEME}://app/")));
        assert!(RAIL_START_URL.starts_with(&format!("{SCHEME}://app/")));
        assert!(lookup("/index.html").is_some());
    }

    #[test]
    fn all_view_start_urls_serve_the_same_page() {
        let page = lookup("/index.html").expect("page");
        for url in [START_URL, TABS_START_URL, RAIL_START_URL, PANEL_START_URL, POPUP_START_URL, DIALOG_START_URL, NOTES_DIALOG_START_URL, SETTINGS_START_URL, HISTORY_PAGE_START_URL, DOWNLOADS_PAGE_START_URL, PRIVACY_DIALOG_START_URL, SESSIONS_DIALOG_START_URL, RENAME_DIALOG_START_URL] {
            let path = url
                .strip_prefix(&format!("{SCHEME}://app"))
                .expect("app host");
            assert_eq!(lookup(path), Some(page), "{url}");
        }
        assert_eq!(lookup("/index.html?view=tabs"), Some(page));
        assert_eq!(lookup("/index.html?view=toolbar"), Some(page));
        assert_eq!(lookup("/index.html?view=rail"), Some(page));
    }

    #[test]
    fn rail_start_url_is_the_rail_view_only() {
        assert_eq!(RAIL_START_URL, "bowser-ui://app/index.html?view=rail");
        assert_ne!(RAIL_START_URL, START_URL);
        assert_ne!(RAIL_START_URL, TABS_START_URL);
    }
}
