//! Vertical sidebar row projection and hit targets.

use std::collections::HashSet;

use crate::{
    tab_groups::{self, DomainKey, GroupPreferences, TabGroupInput, TabGroupKey},
    tabs::TabId,
    vertical_tabs::{self, SidebarLayout, SidebarHit},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SidebarRow {
    Group {
        representative: TabId,
        key: DomainKey,
        name: String,
        colour: Option<u8>,
        collapsed: bool,
    },
    Tab {
        id: TabId,
        title: String,
        pinned: bool,
        loading: bool,
    },
}

/// Projects tabs into group headers and contiguous member rows without changing strip order.
pub fn rows(
    tabs: &[(TabId, String, bool, bool)],
    inputs: &[TabGroupInput],
    preferences: &GroupPreferences,
) -> Vec<SidebarRow> {
    let keys = tab_groups::group_keys(inputs, preferences);
    let mut emitted = HashSet::<TabGroupKey>::new();
    let mut rows = Vec::with_capacity(tabs.len());
    for (index, (id, title, pinned, loading)) in tabs.iter().enumerate() {
        let key = keys.get(index).and_then(Option::as_ref);
        if let Some(TabGroupKey::Domain(domain)) = key {
            if emitted.insert(TabGroupKey::Domain(domain.clone())) {
                let collapsed = preferences.is_collapsed(domain);
                rows.push(SidebarRow::Group {
                    representative: *id,
                    key: domain.clone(),
                    name: preferences.name(domain).unwrap_or(domain.as_str()).to_owned(),
                    colour: preferences.colour(domain),
                    collapsed,
                });
                if !collapsed {
                    for (member_index, (member_id, member_title, member_pinned, member_loading))
                        in tabs.iter().enumerate().skip(index)
                    {
                        if keys.get(member_index).and_then(Option::as_ref) == key {
                            rows.push(SidebarRow::Tab {
                                id: *member_id,
                                title: member_title.clone(),
                                pinned: *member_pinned,
                                loading: *member_loading,
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
            });
        }
    }
    rows
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
        SidebarHit::Tab(index) if index < rows.len() && matches!(rows[index], SidebarRow::Tab { .. }) => {
            Some(SidebarHit::Tab(index))
        }
        SidebarHit::TabClose(index) if index < rows.len() && matches!(rows[index], SidebarRow::Tab { .. }) => {
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
