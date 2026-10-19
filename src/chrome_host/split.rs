//! Split view messages (Rust to JS) and the pure checks behind the split view's commands.
//! See docs/design/split-view.md and the "Split view" section of docs/html-chrome-protocol.md.
//!
//! - `pair` goes to the tab strip and the toolbar: which two tabs form the window's pair, which side
//!   is focused, whether the pair is shown and whether it is side by side or stacked.
//! - `splitpick` fills the empty side (`View::Split`): the open tabs it can take and recently closed
//!   pages; `splitdrop` turns the same view into the drop-zone card while a tab is dragged;
//!   `splithide` clears it.
//! - `privatepage` turns the same view into the private landing over a blank private tab
//!   (docs/design/private-tabs.md): the tab's wire id and how many private tabs are open.

use super::json::{capped, Json};
use super::protocol::{favicon_data_uri, MAX_TAB_TITLE_CHARS};
use crate::split_view::{DropZone, Orientation, Pane};

/// Most open tabs listed on the empty side.
pub const MAX_PICK_TABS: usize = 100;
/// Most recently closed pages listed on the empty side (the `reopen` index range).
pub const MAX_PICK_CLOSED: usize = super::protocol::MAX_SPLIT_CLOSED as usize;
/// Longest host shown under a row, in characters.
pub const MAX_HOST_CHARS: usize = 253;
/// Total base64 favicon text one `splitpick` message may carry; rows past it get no favicon.
pub const PICK_FAVICON_BUDGET: usize = 192 * 1024;

/// The pair half a pane belongs to: `l` (first) or `r` (second).
fn side(pane: Pane) -> &'static str {
    match pane.half() {
        Pane::Left => "l",
        _ => "r",
    }
}

/// The wire name of a page edge (`l`, `r`, `t`, `b`) or corner (`tl`, `tr`, `bl`, `br`). The same
/// names say where a pane of a larger layout sits.
pub fn edge(zone: DropZone) -> &'static str {
    match zone {
        DropZone::Left => "l",
        DropZone::Right => "r",
        DropZone::Top => "t",
        DropZone::Bottom => "b",
        DropZone::TopLeft => "tl",
        DropZone::TopRight => "tr",
        DropZone::BottomLeft => "bl",
        DropZone::BottomRight => "br",
    }
}

/// `{"v":1,"t":"pair","l":..,"r":..,"f":"l"|"r","shown":..,"o":"h"|"v"}` with wire ids
/// (`TabId + 1`), or `l` and `r` both `0` when the window has no pair. `l` and `r` are the first
/// and second tab: left and right side by side (`o` `h`), top and bottom stacked (`o` `v`).
pub fn pair_json(pair: Option<(u32, u32, Pane, bool, Orientation)>) -> String {
    let (l, r, focused, shown, orientation) = pair.unwrap_or((0, 0, Pane::Left, false, Orientation::SideBySide));
    Json::object(|o| {
        o.int("v", 1);
        o.string("t", "pair");
        o.uint("l", u64::from(l));
        o.uint("r", u64::from(r));
        o.string("f", side(focused));
        o.bool("shown", shown);
        o.string("o", if orientation.is_stacked() { "v" } else { "h" });
    })
}

/// The `pair` message of a layout of three or four panes: the pair fields as [`pair_json`] (`l` and
/// `r` the halves' lead tabs, `f` the focused half, `o` the primary split) plus `panes`, every pane
/// in tree order as `{"id":..,"at":..}` with `at` an edge (`l`, `r`, `t`, `b`: a whole half) or a
/// corner (`tl`, `tr`, `bl`, `br`: a quarter), and `fp`, the focused pane's `at`. A pair never
/// carries `panes`, so its message is unchanged.
pub fn layout_json(pair: (u32, u32, Pane, bool, Orientation), panes: &[(u32, DropZone)], focused: DropZone) -> String {
    let (l, r, f, shown, orientation) = pair;
    Json::object(|o| {
        o.int("v", 1);
        o.string("t", "pair");
        o.uint("l", u64::from(l));
        o.uint("r", u64::from(r));
        o.string("f", side(f));
        o.bool("shown", shown);
        o.string("o", if orientation.is_stacked() { "v" } else { "h" });
        o.array("panes", |a| {
            for (id, at) in panes.iter().take(4) {
                a.object(|o| {
                    o.uint("id", u64::from(*id));
                    o.string("at", edge(*at));
                });
            }
        });
        o.string("fp", edge(focused));
    })
}

/// An open tab the empty side offers.
pub struct PickTab<'a> {
    /// Wire id (`TabId + 1`).
    pub id: u32,
    pub title: &'a str,
    pub host: &'a str,
    pub png: Option<&'a [u8]>,
    pub private: bool,
}

/// A recently closed page the empty side offers (its index is the `reopen` `n`).
pub struct PickClosed<'a> {
    pub title: &'a str,
    pub host: &'a str,
}

/// `{"v":1,"t":"splitpick","side":..,"tabs":[{id,title,host,fav,private}],"closed":[{n,title,host}]}`.
/// `side` is the page edge the empty pane sits against (`l`, `r`, `t` or `b`). Lists are capped,
/// text is capped on character boundaries and favicons pass the PNG gate and the message budget (a
/// row without one gets `""`).
pub fn pick_json(side: DropZone, tabs: &[PickTab<'_>], closed: &[PickClosed<'_>]) -> String {
    let mut budget = PICK_FAVICON_BUDGET;
    Json::object(|o| {
        o.int("v", 1);
        o.string("t", "splitpick");
        o.string("side", edge(side));
        o.array("tabs", |a| {
            for tab in tabs.iter().take(MAX_PICK_TABS) {
                let fav = tab
                    .png
                    .and_then(favicon_data_uri)
                    .filter(|uri| uri.len() <= budget)
                    .unwrap_or_default();
                budget -= fav.len();
                a.object(|o| {
                    o.uint("id", u64::from(tab.id));
                    o.string("title", capped(tab.title, MAX_TAB_TITLE_CHARS));
                    o.string("host", capped(tab.host, MAX_HOST_CHARS));
                    o.string("fav", &fav);
                    o.bool("private", tab.private);
                });
            }
        });
        o.array("closed", |a| {
            for (n, page) in closed.iter().take(MAX_PICK_CLOSED).enumerate() {
                a.object(|o| {
                    o.uint("n", n as u64);
                    o.string("title", capped(page.title, MAX_TAB_TITLE_CHARS));
                    o.string("host", capped(page.host, MAX_HOST_CHARS));
                });
            }
        });
    })
}

/// `{"v":1,"t":"splitdrop","side":..,"title":..}`: the drop-zone card for a dragged tab over the
/// `side` half of the page (`l`, `r`, `t` or `b`).
pub fn drop_json(side: DropZone, title: &str) -> String {
    Json::object(|o| {
        o.int("v", 1);
        o.string("t", "splitdrop");
        o.string("side", edge(side));
        o.string("title", capped(title, MAX_TAB_TITLE_CHARS));
    })
}

/// `{"v":1,"t":"splithide"}`: the view is about to be hidden; it drops its content.
pub fn hide_json() -> String {
    r#"{"v":1,"t":"splithide"}"#.to_owned()
}

/// Most private tabs a `privatepage` reports (shared with `ui/src/split-protocol.ts`).
pub const MAX_PRIVATE_TABS: usize = 10_000;

/// `{"v":1,"t":"privatepage","id":..,"n":..}`: the private landing over blank private tab `id`
/// (wire id), with `n` private tabs open in the window (clamped to 1..=[`MAX_PRIVATE_TABS`]).
pub fn private_json(id: u32, n: usize) -> String {
    Json::object(|o| {
        o.int("v", 1);
        o.string("t", "privatepage");
        o.uint("id", u64::from(id));
        o.uint("n", n.clamp(1, MAX_PRIVATE_TABS) as u64);
    })
}

/// The host shown under a row: the URL's host without a leading `www.`, or `""`.
pub fn display_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.strip_prefix("www.").unwrap_or(host).to_owned()))
        .unwrap_or_default()
}

/// Converts a point in CSS pixels of a view whose top-left corner is at `origin` (screen pixels)
/// to screen pixels at `scale` (device pixels per CSS pixel).
pub fn css_to_screen(origin: (i32, i32), scale: f32, x: i32, y: i32) -> (i32, i32) {
    let to_px = |value: i32| (value as f32 * scale).round() as i32;
    (origin.0.saturating_add(to_px(x)), origin.1.saturating_add(to_px(y)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome_host::protocol::{parse_command, Command, SplitCommand};

    fn png(size: usize) -> Vec<u8> {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&16u32.to_be_bytes());
        png.extend_from_slice(&16u32.to_be_bytes());
        png.resize(size, 0);
        png
    }

    #[test]
    fn pair_message_names_both_halves_and_the_focused_side() {
        assert_eq!(
            pair_json(Some((3, 8, Pane::Right, true, Orientation::SideBySide))),
            r#"{"v":1,"t":"pair","l":3,"r":8,"f":"r","shown":true,"o":"h"}"#
        );
        assert_eq!(
            pair_json(Some((3, 8, Pane::Left, false, Orientation::Stacked))),
            r#"{"v":1,"t":"pair","l":3,"r":8,"f":"l","shown":false,"o":"v"}"#
        );
        assert_eq!(pair_json(None), r#"{"v":1,"t":"pair","l":0,"r":0,"f":"l","shown":false,"o":"h"}"#);
    }

    #[test]
    fn layout_message_lists_every_pane_and_the_focused_one() {
        let panes = [(3, DropZone::TopLeft), (5, DropZone::BottomLeft), (8, DropZone::Right)];
        assert_eq!(
            layout_json((3, 8, Pane::LeftTrail, true, Orientation::SideBySide), &panes, DropZone::BottomLeft),
            r#"{"v":1,"t":"pair","l":3,"r":8,"f":"l","shown":true,"o":"h","panes":[{"id":3,"at":"tl"},{"id":5,"at":"bl"},{"id":8,"at":"r"}],"fp":"bl"}"#
        );
        assert_eq!(DropZone::CORNERS.map(edge), ["tl", "tr", "bl", "br"]);
        assert_eq!(drop_json(DropZone::TopRight, "x"), r#"{"v":1,"t":"splitdrop","side":"tr","title":"x"}"#);
    }

    #[test]
    fn split_edges_name_all_four_sides() {
        assert_eq!(DropZone::ALL.map(edge), ["l", "r", "t", "b"]);
        assert_eq!(drop_json(DropZone::Bottom, "x"), r#"{"v":1,"t":"splitdrop","side":"b","title":"x"}"#);
        assert!(pick_json(DropZone::Top, &[], &[]).contains(r#""side":"t""#));
    }

    #[test]
    fn pick_message_caps_lists_text_and_favicons() {
        let long = "x".repeat(MAX_TAB_TITLE_CHARS + 20);
        let icon = png(64);
        let tabs: Vec<PickTab<'_>> = (0..MAX_PICK_TABS as u32 + 5)
            .map(|id| PickTab { id: id + 1, title: &long, host: "example.com", png: Some(&icon), private: false })
            .collect();
        let closed: Vec<PickClosed<'_>> = (0..12).map(|_| PickClosed { title: "Old", host: "old.test" }).collect();
        let json = pick_json(DropZone::Left, &tabs, &closed);
        assert_eq!(json.matches("\"id\":").count(), MAX_PICK_TABS);
        assert_eq!(json.matches("\"n\":").count(), MAX_PICK_CLOSED);
        assert!(!json.contains(&"x".repeat(MAX_TAB_TITLE_CHARS + 1)));
        assert!(json.contains("data:image/png;base64,"));
        let bad = [0u8; 40];
        let one = [PickTab { id: 1, title: "<b>", host: "h", png: Some(&bad), private: true }];
        let json = pick_json(DropZone::Right, &one, &[]);
        assert_eq!(
            json,
            r#"{"v":1,"t":"splitpick","side":"r","tabs":[{"id":1,"title":"\u003cb>","host":"h","fav":"","private":true}],"closed":[]}"#
        );
    }

    #[test]
    fn favicons_stop_at_the_message_budget() {
        let icon = png(crate::chrome_host::protocol::MAX_FAVICON_BYTES);
        let tabs: Vec<PickTab<'_>> = (0..40)
            .map(|id| PickTab { id: id + 1, title: "t", host: "h", png: Some(&icon), private: false })
            .collect();
        let json = pick_json(DropZone::Left, &tabs, &[]);
        let favs = json.matches("data:image/png;base64,").count();
        assert!(favs > 0 && favs < 40, "{favs} favicons");
        assert!(json.len() < PICK_FAVICON_BUDGET + 40 * 200);
    }

    #[test]
    fn private_page_message_carries_the_tab_and_a_clamped_count() {
        assert_eq!(private_json(4, 2), r#"{"v":1,"t":"privatepage","id":4,"n":2}"#);
        assert_eq!(private_json(1, 0), r#"{"v":1,"t":"privatepage","id":1,"n":1}"#);
        assert_eq!(private_json(1, usize::MAX), format!(r#"{{"v":1,"t":"privatepage","id":1,"n":{MAX_PRIVATE_TABS}}}"#));
    }

    #[test]
    fn hosts_and_coordinates() {
        assert_eq!(display_host("https://www.example.com/a?b"), "example.com");
        assert_eq!(display_host("https://docs.rs/x"), "docs.rs");
        assert_eq!(display_host("not a url"), "");
        assert_eq!(css_to_screen((100, 50), 1.5, 10, 20), (115, 80));
        assert_eq!(css_to_screen((i32::MAX - 1, 0), 2.0, 16_384, 0), (i32::MAX, 0));
        // A sidebar at the right: the page is left of the view.
        assert_eq!(css_to_screen((1000, 50), 1.5, -100, 20), (850, 80));
    }

    #[test]
    fn split_commands_take_exactly_their_fields() {
        let ok = |raw: &str| match parse_command(raw) {
            Ok(Command::Split(command)) => command,
            other => panic!("{raw}: {other:?}"),
        };
        assert_eq!(ok(r#"{"v":1,"t":"split","a":"pick","id":4}"#), SplitCommand::Pick { id: 4 });
        assert_eq!(ok(r#"{"v":1,"t":"split","a":"reopen","n":7}"#), SplitCommand::Reopen { n: 7 });
        assert_eq!(ok(r#"{"v":1,"t":"split","a":"go","text":"example.com"}"#), SplitCommand::Go("example.com".into()));
        assert_eq!(ok(r#"{"v":1,"t":"split","a":"close"}"#), SplitCommand::Close);
        assert_eq!(ok(r#"{"v":1,"t":"split","a":"focus"}"#), SplitCommand::Focus);
        assert_eq!(ok(r#"{"v":1,"t":"split","a":"closeprivate"}"#), SplitCommand::ClosePrivate);
        assert_eq!(
            ok(r#"{"v":1,"t":"split","a":"over","id":2,"x":0,"y":16384}"#),
            SplitCommand::DragOver { id: 2, x: 0, y: 16384 }
        );
        assert_eq!(
            ok(r#"{"v":1,"t":"split","a":"over","id":2,"x":-300,"y":-16384}"#),
            SplitCommand::DragOver { id: 2, x: -300, y: -16384 }
        );
        assert_eq!(ok(r#"{"v":1,"t":"split","a":"drop","id":2}"#), SplitCommand::Drop { id: 2 });
        assert_eq!(ok(r#"{"v":1,"t":"split","a":"end","id":2}"#), SplitCommand::DragEnd { id: 2 });
        for raw in [
            r#"{"v":1,"t":"split","a":"pick"}"#,
            r#"{"v":1,"t":"split","a":"pick","id":-1}"#,
            r#"{"v":1,"t":"split","a":"pick","id":"4"}"#,
            r#"{"v":1,"t":"split","a":"pick","id":4,"n":1}"#,
            r#"{"v":1,"t":"split","a":"reopen","n":8}"#,
            r#"{"v":1,"t":"split","a":"go","text":"   "}"#,
            r#"{"v":1,"t":"split","a":"close","id":1}"#,
            r#"{"v":1,"t":"split","a":"closeprivate","n":1}"#,
            r#"{"v":1,"t":"split","a":"closePrivate"}"#,
            r#"{"v":1,"t":"split","a":"over","id":2,"x":16385,"y":0}"#,
            r#"{"v":1,"t":"split","a":"over","id":2,"x":-16385,"y":0}"#,
            r#"{"v":1,"t":"split","a":"over","id":2,"x":1.5,"y":0}"#,
            r#"{"v":1,"t":"split","a":"over","id":2,"x":1}"#,
            r#"{"v":1,"t":"split","a":"Pick","id":4}"#,
            r#"{"v":1,"t":"split","a":"swap"}"#,
            r#"{"v":1,"t":"split","a":"pick","id":4,"extra":1}"#,
            r#"{"v":1,"t":"split","a":"over","id":2,"x":1,"y":2,"n":3}"#,
        ] {
            assert!(parse_command(raw).is_err(), "{raw}");
        }
        let long = format!(r#"{{"v":1,"t":"split","a":"go","text":"{}"}}"#, "x".repeat(crate::chrome_host::protocol::MAX_TEXT_CHARS + 1));
        assert!(parse_command(&long).is_err());
    }

    #[test]
    fn split_messages_match_committed_fixture() {
        let icon = png(40);
        let tabs = [
            PickTab { id: 3, title: "Docs <home>", host: "docs.example", png: Some(&icon), private: false },
            PickTab { id: 9, title: "", host: "", png: None, private: true },
        ];
        let closed = [PickClosed { title: "Old page 😀", host: "old.example" }];
        let content = format!(
            "{{\"pair\":{},\"pairStacked\":{},\"pairNone\":{},\"quad\":{},\"tee\":{},\"pick\":{},\"pickTop\":{},\"pickCorner\":{},\"drop\":{},\"dropBottom\":{},\"dropCorner\":{},\"hide\":{},\"private\":{}}}\n",
            pair_json(Some((3, 8, Pane::Right, true, Orientation::SideBySide))),
            pair_json(Some((3, 8, Pane::Left, true, Orientation::Stacked))),
            pair_json(None),
            layout_json(
                (3, 8, Pane::RightTrail, true, Orientation::SideBySide),
                &[(3, DropZone::TopLeft), (5, DropZone::BottomLeft), (8, DropZone::TopRight), (9, DropZone::BottomRight)],
                DropZone::BottomRight
            ),
            layout_json((3, 8, Pane::Left, false, Orientation::Stacked), &[(3, DropZone::TopLeft), (5, DropZone::TopRight), (8, DropZone::Bottom)], DropZone::TopLeft),
            pick_json(DropZone::Right, &tabs, &closed),
            pick_json(DropZone::Top, &[], &[]),
            pick_json(DropZone::BottomLeft, &[], &[]),
            drop_json(DropZone::Left, "Example"),
            drop_json(DropZone::Bottom, "Below"),
            drop_json(DropZone::TopRight, "Corner"),
            hide_json(),
            private_json(4, 2),
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/split-messages.json");
        if std::env::var_os("BOWSER_WRITE_FIXTURES").is_some_and(|value| value == "1") {
            std::fs::create_dir_all(path.parent().expect("fixtures directory")).expect("create fixtures directory");
            std::fs::write(&path, content).expect("write split messages fixture");
        } else {
            assert_eq!(std::fs::read_to_string(path).expect("committed split messages fixture"), content);
        }
    }
}
