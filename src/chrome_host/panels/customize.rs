use super::super::{json::Json, protocol::{self, PanelFields}};
use crate::{sidebar::{SidebarConfig, SidebarKind}, workspaces::{WorkspaceColour, WorkspaceIcon}};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Visibility(SidebarKind, bool),
    // New and edited workspaces and web apps come from the anchored composer (win32/composer.rs).
    AppRemove(u32), AppMove(u32, Option<u32>),
    WorkspaceRename(u32, String), WorkspaceDelete(u32), WorkspaceMove(u32, Option<u32>),
    WorkspaceIcon(u32, WorkspaceIcon), WorkspaceColour(u32, Option<WorkspaceColour>),
    SwitchProfile, ClearAppData,
}

pub(crate) type WorkspaceSnapshot = (u32, String, WorkspaceIcon, Option<WorkspaceColour>, usize, bool);

pub(crate) fn snapshot(sidebar: &SidebarConfig, workspaces: &[WorkspaceSnapshot]) -> String {
    protocol::snap_json("customize", |out| {
        out.object("visible", |visible| {
            visible.bool("workspaces", sidebar.is_visible(SidebarKind::Workspaces));
            visible.bool("apps", sidebar.is_visible(SidebarKind::Apps));
            visible.bool("bookmarks", sidebar.is_visible(SidebarKind::Bookmarks));
            visible.bool("history", sidebar.is_visible(SidebarKind::History));
        });
        out.array("workspaces", |items| for (id, name, icon, colour, tabs, active) in workspaces {
            items.object(|item| {
                item.uint("id", u64::from(*id)); item.string("name", name);
                item.string("icon", icon_name(*icon));
                if let Some(colour) = colour { item.string("colour", colour_name(*colour)); } else { item.null("colour"); }
                item.bool("active", *active); item.uint("tabs", *tabs as u64);
            });
        });
        out.array("apps", |items| for app in &sidebar.apps {
            items.object(|item| { item.uint("id", u64::from(app.id)); item.string("name", &app.name); item.string("url", &app.url); item.uint("position", app.position as u64); });
        });
    }).unwrap_or_else(|| Json::object(|_| {}))
}

pub(crate) fn map_command(f: PanelFields) -> Option<Action> {
    let exact = |id: bool, before: bool, n: bool, on: bool, text: bool, url: bool| {
        f.id.is_some() == id && f.parent.is_none() && f.before.is_some() == before
            && f.n.is_some() == n && f.on.is_some() == on && f.text.is_some() == text && f.url.is_some() == url
    };
    let id = || u32::try_from(f.id?).ok();
    let before = || match f.before { None => Some(None), Some(0) => Some(None), Some(value) => Some(Some(u32::try_from(value).ok()?)) };
    match f.a.as_str() {
        "visibility" if exact(false,false,false,true,true,false) => Some(Action::Visibility(sidebar_kind(f.text.as_deref()?)?, f.on?)),
        "app-remove" if exact(true,false,false,false,false,false) => Some(Action::AppRemove(id()?)),
        "app-move" if exact(true,true,false,false,false,false) => Some(Action::AppMove(id()?, before()?)),
        "workspace-rename" if exact(true,false,false,false,true,false) => Some(Action::WorkspaceRename(id()?, f.text?)),
        "workspace-delete" if exact(true,false,false,false,false,false) => Some(Action::WorkspaceDelete(id()?)),
        "workspace-move" if exact(true,true,false,false,false,false) => Some(Action::WorkspaceMove(id()?, before()?)),
        "workspace-icon" if exact(true,false,false,false,true,false) => Some(Action::WorkspaceIcon(id()?, parse_icon(f.text.as_deref()?)?)),
        "workspace-colour" if exact(true,false,true,false,false,false) => Some(Action::WorkspaceColour(id()?, parse_colour(f.n?)?)),
        "switch-profile" if exact(false,false,false,false,false,false) => Some(Action::SwitchProfile),
        "clear-app-data" if exact(false,false,false,false,false,false) => Some(Action::ClearAppData),
        _ => None,
    }
}

fn sidebar_kind(value: &str) -> Option<SidebarKind> { Some(match value { "workspaces" => SidebarKind::Workspaces, "apps" => SidebarKind::Apps, "bookmarks" => SidebarKind::Bookmarks, "history" => SidebarKind::History, _ => return None }) }
fn parse_icon(value: &str) -> Option<WorkspaceIcon> { Some(match value { "briefcase" => WorkspaceIcon::Briefcase, "home" => WorkspaceIcon::Home, "code" => WorkspaceIcon::Code, "book" => WorkspaceIcon::Book, "cart" => WorkspaceIcon::Cart, "game" => WorkspaceIcon::Game, "music" => WorkspaceIcon::Music, "heart" => WorkspaceIcon::Heart, "star" => WorkspaceIcon::Star, "bolt" => WorkspaceIcon::Bolt, "globe" => WorkspaceIcon::Globe, "leaf" => WorkspaceIcon::Leaf, _ => return None }) }
fn icon_name(value: WorkspaceIcon) -> &'static str { match value { WorkspaceIcon::Briefcase => "briefcase", WorkspaceIcon::Home => "home", WorkspaceIcon::Code => "code", WorkspaceIcon::Book => "book", WorkspaceIcon::Cart => "cart", WorkspaceIcon::Game => "game", WorkspaceIcon::Music => "music", WorkspaceIcon::Heart => "heart", WorkspaceIcon::Star => "star", WorkspaceIcon::Bolt => "bolt", WorkspaceIcon::Globe => "globe", WorkspaceIcon::Leaf => "leaf" } }
fn parse_colour(value: i32) -> Option<Option<WorkspaceColour>> { Some(match value { -1 => None, 0 => Some(WorkspaceColour::Blue), 1 => Some(WorkspaceColour::Violet), 2 => Some(WorkspaceColour::Teal), 3 => Some(WorkspaceColour::Amber), 4 => Some(WorkspaceColour::Rose), 5 => Some(WorkspaceColour::Red), 6 => Some(WorkspaceColour::Green), 7 => Some(WorkspaceColour::Slate), _ => return None }) }
fn colour_name(value: WorkspaceColour) -> &'static str { match value { WorkspaceColour::Blue => "blue", WorkspaceColour::Violet => "violet", WorkspaceColour::Teal => "teal", WorkspaceColour::Amber => "amber", WorkspaceColour::Rose => "rose", WorkspaceColour::Red => "red", WorkspaceColour::Green => "green", WorkspaceColour::Slate => "slate" } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_and_command_mapping_preserve_default_workspace_and_flat_fields() {
        let snap = snapshot(&SidebarConfig::default(), &[(0, "Default".into(), WorkspaceIcon::Briefcase, None, 1, true)]);
        assert!(snap.contains(r#""id":0,"name":"Default""#));
        assert!(snap.contains(r#""colour":null"#));
        let fields = PanelFields { a: "workspace-delete".into(), id: Some(0), ..PanelFields::default() };
        assert_eq!(map_command(fields), Some(Action::WorkspaceDelete(0)));
    }

    #[test]
    fn hyphenated_wire_actions_reach_the_handler_from_raw_messages() {
        use crate::chrome_host::protocol::{parse_command, Command, PanelType};
        // The page must be able to send these verbatim: `workspace-icon` has a hyphen.
        for (raw, expected) in [
            (r#"{"v":1,"t":"cust","a":"workspace-icon","id":1,"text":"code"}"#, Action::WorkspaceIcon(1, WorkspaceIcon::Code)),
            (r#"{"v":1,"t":"cust","a":"app-remove","id":4}"#, Action::AppRemove(4)),
        ] {
            let Ok(Command::Panel { kind: PanelType::Customize, f }) = parse_command(raw) else { panic!("{raw}") };
            assert_eq!(map_command(f), Some(expected), "{raw}");
        }
    }

    #[test]
    fn creating_and_editing_go_through_the_composer_not_the_customize_bridge() {
        use crate::chrome_host::protocol::{parse_command, Command, PanelType};
        for raw in [
            r#"{"v":1,"t":"cust","a":"workspace-create","text":"Work"}"#,
            r#"{"v":1,"t":"cust","a":"app-add","text":"Mail","url":"https://mail.example/"}"#,
            r#"{"v":1,"t":"cust","a":"app-update","id":4,"text":"Mail","url":"https://mail.example/"}"#,
        ] {
            let Ok(Command::Panel { kind: PanelType::Customize, f }) = parse_command(raw) else { panic!("{raw}") };
            assert_eq!(map_command(f), None, "{raw}");
        }
    }

    #[test]
    fn actions_reject_unknown_fields_values_and_invalid_palette_entries() {
        let mut fields = PanelFields { a: "visibility".into(), text: Some("menu".into()), on: Some(false), ..PanelFields::default() };
        assert_eq!(map_command(fields.clone()), None);
        fields.a = "workspace-colour".into(); fields.id = Some(0); fields.text = None; fields.on = None; fields.n = Some(8);
        assert_eq!(map_command(fields), None);
    }
}
