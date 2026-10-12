//! Vertical sidebar row projection and hit targets.

use std::collections::HashSet;

use crate::{
    chrome::{
        group_ui::{self, GroupFrame, GroupGeometry},
        layout::{Metrics, Rect},
    },
    tab_groups::{self, DomainKey, GroupPreferences, TabGroupInput, TabGroupKey},
    tabs::TabId,
    vertical_tabs::{self, SidebarHit, SidebarLayout},
};

/// Logical inset of a grouped tab row, so the tabs of an expanded group sit under its header.
pub const GROUP_MEMBER_INDENT: i32 = 16;
/// Logical distance from a row's left edge to a group header's leading ball.
const HEADER_BALL_INSET: i32 = 8;
/// Logical gap between a header's ball stack and its name.
const HEADER_LABEL_GAP: i32 = 7;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SidebarRow {
    Group {
        representative: TabId,
        key: DomainKey,
        name: String,
        colour: Option<u8>,
        collapsed: bool,
        /// Every tab in the group, hidden or not. A collapsed header fans one ball per tab.
        members: usize,
        /// Whether the active tab is in this group, which keeps a collapsed header's fan.
        selected: bool,
    },
    Tab {
        id: TabId,
        title: String,
        pinned: bool,
        loading: bool,
        /// Whether this row sits under an expanded group header and is indented beneath it.
        grouped: bool,
    },
}

/// Projects tabs into group headers and contiguous member rows without changing strip order.
pub fn rows(
    tabs: &[(TabId, String, bool, bool)],
    inputs: &[TabGroupInput],
    preferences: &GroupPreferences,
    active: TabId,
) -> Vec<SidebarRow> {
    let keys = tab_groups::group_keys(inputs, preferences);
    let mut emitted = HashSet::<TabGroupKey>::new();
    let mut rows = Vec::with_capacity(tabs.len());
    for (index, (id, title, pinned, loading)) in tabs.iter().enumerate() {
        let key = keys.get(index).and_then(Option::as_ref);
        if let Some(TabGroupKey::Domain(domain)) = key {
            if emitted.insert(TabGroupKey::Domain(domain.clone())) {
                let collapsed = preferences.is_collapsed(domain);
                let members = keys.iter().filter(|other| other.as_ref() == key).count();
                let selected = tabs
                    .iter()
                    .zip(&keys)
                    .any(|((tab, ..), other)| *tab == active && other.as_ref() == key);
                rows.push(SidebarRow::Group {
                    representative: *id,
                    key: domain.clone(),
                    name: preferences
                        .name(domain)
                        .unwrap_or(domain.as_str())
                        .to_owned(),
                    colour: preferences.colour(domain),
                    collapsed,
                    members,
                    selected,
                });
                if !collapsed {
                    for (member_index, (member_id, member_title, member_pinned, member_loading)) in
                        tabs.iter().enumerate().skip(index)
                    {
                        if keys.get(member_index).and_then(Option::as_ref) == key {
                            rows.push(SidebarRow::Tab {
                                id: *member_id,
                                title: member_title.clone(),
                                pinned: *member_pinned,
                                loading: *member_loading,
                                grouped: true,
                            });
                        }
                    }
                }
            }
        } else {
            rows.push(SidebarRow::Tab {
                id: *id,
                title: title.clone(),
                pinned: *pinned,
                loading: *loading,
                grouped: false,
            });
        }
    }
    rows
}

/// Row rectangle for `index` after `scroll`, inset from the sidebar edges and the row gap.
pub fn row_bounds(layout: &SidebarLayout, index: usize, metrics: Metrics, scroll: i32) -> Rect {
    let row_height = metrics.px(vertical_tabs::TAB_ROW_HEIGHT).max(1);
    let top = layout.rows.top + index as i32 * row_height - scroll.max(0);
    Rect::new(
        layout.rows.left + metrics.px(7),
        top + metrics.px(2),
        layout.handle.left - metrics.px(7),
        top + row_height - metrics.px(2),
    )
}

/// Row rectangle of a tab that belongs to an expanded group, indented beneath its header.
pub fn member_bounds(bounds: Rect, metrics: Metrics) -> Rect {
    Rect::new(
        bounds.left + metrics.px(GROUP_MEMBER_INDENT),
        bounds.top,
        bounds.right,
        bounds.bottom,
    )
}

/// Ball geometry for a group header row, from the same helper the horizontal chrome uses. The
/// shell is one island-height square, centred on the row, so the ball matches a leading island
/// ball in size. A collapsed header fans one ball per tab, capped by `group_ui`. `None` for tabs.
pub fn header_ball(
    layout: &SidebarLayout,
    rows: &[SidebarRow],
    index: usize,
    scroll: i32,
    metrics: Metrics,
) -> Option<GroupGeometry> {
    let SidebarRow::Group {
        collapsed,
        members,
        selected,
        ..
    } = rows.get(index)?
    else {
        return None;
    };
    let bounds = row_bounds(layout, index, metrics, scroll);
    let height = metrics.px(Metrics::ISLAND_HEIGHT);
    let centre = (bounds.top + bounds.bottom) / 2;
    let left = bounds.left + metrics.px(HEADER_BALL_INSET);
    let top = centre - height / 2;
    let shell = Rect::new(left, top, left + height, top + height);
    let state = group_ui::state_from_model(*collapsed, *selected);
    Some(group_ui::group_geometry(
        GroupFrame::resting(shell, state, *members),
        metrics,
    ))
}

/// Left edge of a header's name: just right of its ball stack, which is the fan's last ball when
/// collapsed.
pub fn header_label_left(geometry: &GroupGeometry, metrics: Metrics) -> i32 {
    let stack_right = geometry
        .ball_rects
        .iter()
        .map(|ball| ball.right)
        .max()
        .unwrap_or(geometry.shell.right);
    stack_right + metrics.px(HEADER_LABEL_GAP)
}

pub fn hit_test(
    layout: &SidebarLayout,
    rows: &[SidebarRow],
    x: i32,
    y: i32,
    scroll: i32,
) -> Option<SidebarHit> {
    let hit = vertical_tabs::hit_test(layout, x, y, scroll, rows.len())?;
    match hit {
        SidebarHit::Tab(index)
            if index < rows.len() && matches!(rows[index], SidebarRow::Tab { .. }) =>
        {
            Some(SidebarHit::Tab(index))
        }
        SidebarHit::TabClose(index)
            if index < rows.len() && matches!(rows[index], SidebarRow::Tab { .. }) =>
        {
            Some(SidebarHit::TabClose(index))
        }
        SidebarHit::Tab(index) | SidebarHit::TabClose(index)
            if index < rows.len() && matches!(rows[index], SidebarRow::Group { .. }) =>
        {
            Some(SidebarHit::Group(index))
        }
        other => Some(other),
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;
    use crate::{
        chrome::group_ui::{
            GroupState, MAX_VISIBLE_STACK_BALLS, MIN_HIT_GAP_LOGICAL, MIN_HIT_LOGICAL,
        },
        tab_groups::{GroupPreferences, TabGroupInput},
        tabs::TabId,
    };

    const DPIS: [u32; 4] = [96, 120, 144, 192];

    fn header(members: usize, collapsed: bool, selected: bool) -> SidebarRow {
        SidebarRow::Group {
            representative: TabId(1),
            key: DomainKey::from_url(&Url::parse("https://example.com/").unwrap()).unwrap(),
            name: "example.com".to_owned(),
            colour: None,
            collapsed,
            members,
            selected,
        }
    }

    fn tab() -> SidebarRow {
        SidebarRow::Tab {
            id: TabId(2),
            title: "tab".to_owned(),
            pinned: false,
            loading: false,
            grouped: true,
        }
    }

    fn layout(dpi: u32) -> SidebarLayout {
        SidebarLayout::compute(900, 700, dpi, 260, true).with_tab_count(8, dpi)
    }

    fn logical(physical: i32, dpi: u32) -> i32 {
        physical * 96 / dpi as i32
    }

    #[test]
    fn expanded_header_shows_the_island_ball_centred_in_its_row() {
        for dpi in DPIS {
            let metrics = Metrics::new(dpi);
            let layout = layout(dpi);
            let rows = [header(2, false, false), tab(), tab()];
            let geometry = header_ball(&layout, &rows, 0, 0, metrics).unwrap();
            let bounds = row_bounds(&layout, 0, metrics, 0);
            let ball = geometry.ball_rects[0];

            assert_eq!(
                geometry.ball_rects.len(),
                1,
                "expanded shows one ball at {dpi} dpi"
            );
            assert_eq!(geometry.overflow_count, None);
            assert!(
                bounds.contains_rect(ball),
                "ball inside its row at {dpi} dpi"
            );
            assert!(
                ((ball.top + ball.bottom) - (bounds.top + bounds.bottom)).abs() <= 1,
                "ball vertically centred at {dpi} dpi"
            );
            let inset = ball.left - (bounds.left + metrics.px(HEADER_BALL_INSET));
            assert!(
                (0..=1).contains(&inset),
                "leading inset {inset} at {dpi} dpi"
            );

            // The same ball the horizontal island draws: one island-height shell.
            let island = Rect::new(
                0,
                0,
                metrics.px(Metrics::ISLAND_HEIGHT) * 3,
                metrics.px(Metrics::ISLAND_HEIGHT),
            );
            let horizontal = group_ui::group_geometry(
                GroupFrame::resting(island, GroupState::Expanded, 2),
                metrics,
            );
            assert_eq!(
                ball.width(),
                horizontal.ball_rects[0].width(),
                "ball size at {dpi} dpi"
            );
            assert_eq!(
                ball.height(),
                horizontal.ball_rects[0].height(),
                "ball size at {dpi} dpi"
            );
        }
    }

    #[test]
    fn collapsed_selected_header_fans_one_ball_per_hidden_tab_up_to_the_cap() {
        for dpi in DPIS {
            let metrics = Metrics::new(dpi);
            let layout = layout(dpi);
            let rows = [header(9, true, true), tab()];
            let geometry = header_ball(&layout, &rows, 0, 0, metrics).unwrap();
            let balls = &geometry.ball_rects;

            assert_eq!(balls.len(), MAX_VISIBLE_STACK_BALLS, "cap at {dpi} dpi");
            assert_eq!(geometry.overflow_count, Some(9 - MAX_VISIBLE_STACK_BALLS));
            for (index, ball) in balls.iter().enumerate() {
                assert_eq!(
                    ball.left - balls[0].left,
                    index as i32 * metrics.px(6),
                    "fan step {index} at {dpi} dpi"
                );
            }
            assert_ne!(
                balls[1].top, balls[0].top,
                "the fan alternates above and below"
            );
            assert!(
                balls.last().unwrap().right < layout.handle.left,
                "the fan stays clear of the resize handle at {dpi} dpi"
            );

            let two = header_ball(&layout, &[header(2, true, true), tab()], 0, 0, metrics).unwrap();
            assert_eq!(two.ball_rects.len(), 2);
            assert_eq!(two.overflow_count, None);
        }
    }

    #[test]
    fn collapsed_unselected_header_is_a_single_ball_and_expanded_ignores_fan_count() {
        let metrics = Metrics::new(144);
        let layout = layout(144);
        let unselected =
            header_ball(&layout, &[header(6, true, false), tab()], 0, 0, metrics).unwrap();
        assert_eq!(unselected.ball_rects.len(), 1);
        assert_eq!(unselected.overflow_count, None);

        let expanded =
            header_ball(&layout, &[header(6, false, true), tab()], 0, 0, metrics).unwrap();
        assert_eq!(expanded.ball_rects.len(), 1);
        assert_eq!(expanded.overflow_count, None);
    }

    #[test]
    fn header_name_starts_right_of_the_whole_stack() {
        for dpi in DPIS {
            let metrics = Metrics::new(dpi);
            let layout = layout(dpi);
            for rows in [
                [header(1, false, false), tab()],
                [header(9, true, true), tab()],
                [header(9, true, false), tab()],
            ] {
                let geometry = header_ball(&layout, &rows, 0, 0, metrics).unwrap();
                let bounds = row_bounds(&layout, 0, metrics, 0);
                let stack_right = geometry
                    .ball_rects
                    .iter()
                    .map(|ball| ball.right)
                    .max()
                    .unwrap();
                let label = header_label_left(&geometry, metrics);
                assert!(label > stack_right, "name clears the stack at {dpi} dpi");
                assert!(label < bounds.right, "name fits the row at {dpi} dpi");
            }
        }
    }

    #[test]
    fn header_hit_target_is_at_least_twenty_logical_pixels_with_a_gap_to_neighbours() {
        for dpi in DPIS {
            let metrics = Metrics::new(dpi);
            let layout = layout(dpi);
            for header_row in [
                header(1, false, false),
                header(9, true, true),
                header(9, true, false),
            ] {
                let rows = [tab(), header_row, tab()];
                let geometry = header_ball(&layout, &rows, 1, 0, metrics).unwrap();
                let hit = geometry.hit_rect;
                let above = row_bounds(&layout, 0, metrics, 0);
                let below = row_bounds(&layout, 2, metrics, 0);

                assert!(
                    logical(hit.width(), dpi) >= MIN_HIT_LOGICAL,
                    "width at {dpi} dpi"
                );
                assert!(
                    logical(hit.height(), dpi) >= MIN_HIT_LOGICAL,
                    "height at {dpi} dpi"
                );
                let gap_above = logical(hit.top - above.bottom, dpi);
                let gap_below = logical(below.top - hit.bottom, dpi);
                assert!(
                    gap_above >= MIN_HIT_GAP_LOGICAL,
                    "gap above {gap_above} at {dpi} dpi"
                );
                assert!(
                    gap_below >= MIN_HIT_GAP_LOGICAL,
                    "gap below {gap_below} at {dpi} dpi"
                );
            }
        }
    }

    #[test]
    fn member_rows_are_indented_beneath_their_header() {
        for dpi in DPIS {
            let metrics = Metrics::new(dpi);
            let bounds = row_bounds(&layout(dpi), 1, metrics, 0);
            let member = member_bounds(bounds, metrics);
            assert_eq!(member.left - bounds.left, metrics.px(GROUP_MEMBER_INDENT));
            assert_eq!(
                (member.top, member.right, member.bottom),
                (bounds.top, bounds.right, bounds.bottom)
            );
        }
    }

    #[test]
    fn projection_counts_members_and_marks_the_active_group() {
        let member = |id: u32, host: Option<&str>| TabGroupInput {
            id: TabId(id),
            domain: host.map(|host| {
                DomainKey::from_url(&Url::parse(&format!("https://{host}/")).unwrap()).unwrap()
            }),
            pinned: false,
            opener: None,
            manual: true,
        };
        let inputs = [
            member(1, Some("alpha.example.com")),
            member(2, None),
            member(3, Some("beta.example.com")),
        ];
        let tabs = [
            (TabId(1), "alpha".to_owned(), false, false),
            (TabId(2), "loose".to_owned(), false, false),
            (TabId(3), "beta".to_owned(), false, false),
        ];
        let key = DomainKey::from_url(&Url::parse("https://example.com/").unwrap()).unwrap();
        let mut preferences = GroupPreferences::default();

        let expanded = rows(&tabs, &inputs, &preferences, TabId(3));
        assert_eq!(expanded.len(), 4, "header, two members, loose tab");
        assert!(matches!(
            expanded[0],
            SidebarRow::Group {
                members: 2,
                selected: true,
                collapsed: false,
                ..
            }
        ));
        assert!(matches!(
            expanded[1],
            SidebarRow::Tab {
                id: TabId(1),
                grouped: true,
                ..
            }
        ));
        assert!(matches!(
            expanded[2],
            SidebarRow::Tab {
                id: TabId(3),
                grouped: true,
                ..
            }
        ));
        assert!(matches!(
            expanded[3],
            SidebarRow::Tab {
                id: TabId(2),
                grouped: false,
                ..
            }
        ));

        let unselected = rows(&tabs, &inputs, &preferences, TabId(2));
        assert!(matches!(
            unselected[0],
            SidebarRow::Group {
                selected: false,
                ..
            }
        ));

        preferences.set_collapsed(&key, true).unwrap();
        let collapsed = rows(&tabs, &inputs, &preferences, TabId(3));
        assert_eq!(collapsed.len(), 2, "collapsed members are hidden");
        assert!(matches!(
            collapsed[0],
            SidebarRow::Group {
                members: 2,
                selected: true,
                collapsed: true,
                ..
            }
        ));
    }
}
