//! Per-profile workspace membership and persistence, independent of browser handles.

use std::{collections::HashSet, io, path::Path};

use crate::tabs::TabId;

const MAX_WORKSPACES: usize = 128;
const MAX_NAME_BYTES: usize = 256;
const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const FILE_NAME: &str = "workspaces.txt";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WorkspaceId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WorkspaceIcon {
    #[default]
    Briefcase,
    Home,
    Code,
    Book,
    Cart,
    Game,
    Music,
    Heart,
    Star,
    Bolt,
    Globe,
    Leaf,
}
impl WorkspaceIcon {
    fn key(self) -> &'static str {
        match self {
            Self::Briefcase => "briefcase",
            Self::Home => "home",
            Self::Code => "code",
            Self::Book => "book",
            Self::Cart => "cart",
            Self::Game => "game",
            Self::Music => "music",
            Self::Heart => "heart",
            Self::Star => "star",
            Self::Bolt => "bolt",
            Self::Globe => "globe",
            Self::Leaf => "leaf",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "briefcase" => Self::Briefcase,
            "home" => Self::Home,
            "code" => Self::Code,
            "book" => Self::Book,
            "cart" => Self::Cart,
            "game" => Self::Game,
            "music" => Self::Music,
            "heart" => Self::Heart,
            "star" => Self::Star,
            "bolt" => Self::Bolt,
            "globe" => Self::Globe,
            "leaf" => Self::Leaf,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WorkspaceColour {
    Blue,
    Violet,
    Teal,
    Amber,
    Rose,
    Red,
    Green,
    Slate,
}
impl WorkspaceColour {
    fn key(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Violet => "violet",
            Self::Teal => "teal",
            Self::Amber => "amber",
            Self::Rose => "rose",
            Self::Red => "red",
            Self::Green => "green",
            Self::Slate => "slate",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "blue" => Self::Blue,
            "violet" => Self::Violet,
            "teal" => Self::Teal,
            "amber" => Self::Amber,
            "rose" => Self::Rose,
            "red" => Self::Red,
            "green" => Self::Green,
            "slate" => Self::Slate,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    id: WorkspaceId,
    name: String,
    tabs: Vec<TabId>,
    active_tab: TabId,
    icon: WorkspaceIcon,
    colour: Option<WorkspaceColour>,
}

impl Workspace {
    pub fn id(&self) -> WorkspaceId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn tabs(&self) -> &[TabId] {
        &self.tabs
    }
    pub fn active_tab(&self) -> TabId {
        self.active_tab
    }
    pub fn icon(&self) -> WorkspaceIcon {
        self.icon
    }
    pub fn colour(&self) -> Option<WorkspaceColour> {
        self.colour
    }
    pub fn set_icon(&mut self, icon: WorkspaceIcon) {
        self.icon = icon;
    }
    pub fn set_colour(&mut self, colour: Option<WorkspaceColour>) {
        self.colour = colour;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceTransition {
    pub hidden: Vec<TabId>,
    pub shown: Vec<TabId>,
    pub selected: TabId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceError {
    InvalidName,
    DuplicateName,
    DuplicateTab,
    LastTab,
    LastWorkspace,
    NotFound,
    TooMany,
    InvalidFile,
}

/// Ordered workspaces. Every workspace owns at least one tab and every tab belongs to one workspace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSet {
    workspaces: Vec<Workspace>,
    active: WorkspaceId,
    next_id: u32,
    /// Session-persisted tab IDs in the same order as `session.txt`.
    session_order: Vec<TabId>,
}

impl WorkspaceSet {
    pub fn new(name: &str, first_tab: TabId) -> Self {
        let name = clean_name(name).unwrap_or_else(|| "Default".to_owned());
        Self {
            workspaces: vec![Workspace {
                id: WorkspaceId(0),
                name,
                tabs: vec![first_tab],
                active_tab: first_tab,
                icon: WorkspaceIcon::default(),
                colour: None,
            }],
            active: WorkspaceId(0),
            next_id: 1,
            session_order: Vec::new(),
        }
    }

    pub fn all(&self) -> &[Workspace] {
        &self.workspaces
    }
    pub fn active_id(&self) -> WorkspaceId {
        self.active
    }
    pub fn workspace_for(&self, tab: TabId) -> Option<WorkspaceId> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.tabs.contains(&tab))
            .map(|workspace| workspace.id)
    }
    pub fn workspace_count(&self) -> usize {
        self.workspaces.len()
    }
    pub fn active(&self) -> &Workspace {
        self.get(self.active).expect("active workspace invariant")
    }
    pub fn get(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    fn get_mut(&mut self, id: WorkspaceId) -> Result<&mut Workspace, WorkspaceError> {
        self.workspaces
            .iter_mut()
            .find(|w| w.id == id)
            .ok_or(WorkspaceError::NotFound)
    }

    fn contains_tab(&self, tab: TabId) -> bool {
        self.workspaces.iter().any(|w| w.tabs.contains(&tab))
    }

    pub fn add_tab(&mut self, tab: TabId) -> Result<(), WorkspaceError> {
        if self.contains_tab(tab) {
            return Err(WorkspaceError::DuplicateTab);
        }
        self.get_mut(self.active)?.tabs.push(tab);
        Ok(())
    }

    pub fn select_tab(&mut self, tab: TabId) -> Result<(), WorkspaceError> {
        let workspace = self
            .workspaces
            .iter_mut()
            .find(|w| w.tabs.contains(&tab))
            .ok_or(WorkspaceError::NotFound)?;
        workspace.active_tab = tab;
        Ok(())
    }

    pub fn remove_tab(&mut self, tab: TabId) -> Result<(), WorkspaceError> {
        let workspace = self
            .workspaces
            .iter_mut()
            .find(|w| w.tabs.contains(&tab))
            .ok_or(WorkspaceError::NotFound)?;
        if workspace.tabs.len() == 1 {
            return Err(WorkspaceError::LastTab);
        }
        workspace.tabs.retain(|member| *member != tab);
        if workspace.active_tab == tab {
            workspace.active_tab = workspace.tabs[0];
        }
        Ok(())
    }

    pub fn replace_tab(&mut self, old: TabId, new: TabId) -> Result<(), WorkspaceError> {
        if self.contains_tab(new) {
            return Err(WorkspaceError::DuplicateTab);
        }
        let workspace = self
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.tabs.contains(&old))
            .ok_or(WorkspaceError::NotFound)?;
        for member in &mut workspace.tabs {
            if *member == old {
                *member = new;
            }
        }
        if workspace.active_tab == old {
            workspace.active_tab = new;
        }
        Ok(())
    }

    pub fn move_tab(&mut self, tab: TabId, new_index: usize) -> Result<(), WorkspaceError> {
        let workspace = self
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.tabs.contains(&tab))
            .ok_or(WorkspaceError::NotFound)?;
        let old_index = workspace
            .tabs
            .iter()
            .position(|member| *member == tab)
            .unwrap();
        let tab = workspace.tabs.remove(old_index);
        workspace
            .tabs
            .insert(new_index.min(workspace.tabs.len()), tab);
        Ok(())
    }

    pub fn sync_order(&mut self, ordered: &[TabId]) {
        for workspace in &mut self.workspaces {
            workspace.tabs.sort_by_key(|tab| {
                ordered
                    .iter()
                    .position(|candidate| candidate == tab)
                    .unwrap_or(usize::MAX)
            });
        }
    }

    pub fn set_session_order(&mut self, ordered: &[TabId]) {
        self.session_order = ordered.to_vec();
    }

    /// A copy for saving with the `excluded` tabs (private tabs) left out of every workspace's
    /// membership, so the file never mentions them. The file format needs every workspace to have
    /// a member, so a workspace that would be left empty keeps its tabs: only anonymous numbers
    /// remain, never an address or title.
    pub fn without_tabs(&self, excluded: &HashSet<TabId>) -> Self {
        let mut copy = self.clone();
        for workspace in &mut copy.workspaces {
            let kept: Vec<TabId> = workspace
                .tabs
                .iter()
                .copied()
                .filter(|tab| !excluded.contains(tab))
                .collect();
            if kept.is_empty() {
                continue;
            }
            if !kept.contains(&workspace.active_tab) {
                workspace.active_tab = kept[0];
            }
            workspace.tabs = kept;
        }
        copy.session_order.retain(|tab| !excluded.contains(tab));
        copy
    }

    /// Remaps persisted process-local tab IDs to IDs allocated while restoring the session.
    pub fn remap_session_order(&mut self, restored: &[TabId]) {
        if self.session_order.is_empty() {
            return;
        }
        let remap = self
            .session_order
            .iter()
            .copied()
            .zip(restored.iter().copied())
            .collect::<std::collections::HashMap<_, _>>();
        for workspace in &mut self.workspaces {
            for tab in &mut workspace.tabs {
                if let Some(mapped) = remap.get(tab) {
                    *tab = *mapped;
                }
            }
            if let Some(mapped) = remap.get(&workspace.active_tab) {
                workspace.active_tab = *mapped;
            }
        }
        self.session_order = restored.to_vec();
    }

    /// Drops stale restored ids and assigns any unclaimed session tabs to the active workspace.
    pub fn reconcile(&mut self, tabs: &[TabId], active_tab: TabId) {
        let valid = tabs.iter().copied().collect::<HashSet<_>>();
        for workspace in &mut self.workspaces {
            workspace.tabs.retain(|tab| valid.contains(tab));
            if !workspace.tabs.contains(&workspace.active_tab) {
                workspace.active_tab = workspace.tabs.first().copied().unwrap_or(active_tab);
            }
        }
        self.workspaces
            .retain(|workspace| !workspace.tabs.is_empty());
        if self.workspaces.is_empty() {
            *self = Self::new("Default", active_tab);
            return;
        }
        if !self
            .workspaces
            .iter()
            .any(|workspace| workspace.id == self.active)
        {
            self.active = self.workspaces[0].id;
        }
        let assigned = self
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.tabs.iter().copied())
            .collect::<HashSet<_>>();
        if let Ok(workspace) = self.get_mut(self.active) {
            workspace
                .tabs
                .extend(tabs.iter().copied().filter(|tab| !assigned.contains(tab)));
        }
        if valid.contains(&active_tab)
            && let Some(workspace_id) = self.workspace_for(active_tab)
        {
            self.active = workspace_id;
            if let Ok(workspace) = self.get_mut(workspace_id) {
                workspace.active_tab = active_tab;
            }
        }
        if !self.active().tabs.contains(&self.active().active_tab) {
            self.workspaces[0].active_tab = self.workspaces[0].tabs[0];
        }
    }

    pub fn create(&mut self, name: &str, first_tab: TabId) -> Result<WorkspaceId, WorkspaceError> {
        let name = clean_name(name).ok_or(WorkspaceError::InvalidName)?;
        if self.workspaces.len() >= MAX_WORKSPACES {
            return Err(WorkspaceError::TooMany);
        }
        if self
            .workspaces
            .iter()
            .any(|w| w.name.eq_ignore_ascii_case(&name))
        {
            return Err(WorkspaceError::DuplicateName);
        }
        if self.contains_tab(first_tab) {
            return Err(WorkspaceError::DuplicateTab);
        }
        let id = WorkspaceId(self.next_id);
        self.next_id = self.next_id.checked_add(1).ok_or(WorkspaceError::TooMany)?;
        self.workspaces.push(Workspace {
            id,
            name,
            tabs: vec![first_tab],
            active_tab: first_tab,
            icon: WorkspaceIcon::default(),
            colour: None,
        });
        Ok(id)
    }

    pub fn rename(&mut self, id: WorkspaceId, name: &str) -> Result<(), WorkspaceError> {
        let name = clean_name(name).ok_or(WorkspaceError::InvalidName)?;
        if self
            .workspaces
            .iter()
            .any(|w| w.id != id && w.name.eq_ignore_ascii_case(&name))
        {
            return Err(WorkspaceError::DuplicateName);
        }
        self.get_mut(id)?.name = name;
        Ok(())
    }

    pub fn set_icon(&mut self, id: WorkspaceId, icon: WorkspaceIcon) -> Result<(), WorkspaceError> {
        self.get_mut(id)?.icon = icon;
        Ok(())
    }
    pub fn set_colour(
        &mut self,
        id: WorkspaceId,
        colour: Option<WorkspaceColour>,
    ) -> Result<(), WorkspaceError> {
        self.get_mut(id)?.colour = colour;
        Ok(())
    }
    pub fn reorder(&mut self, id: WorkspaceId, new_index: usize) -> Result<(), WorkspaceError> {
        let old = self
            .workspaces
            .iter()
            .position(|workspace| workspace.id == id)
            .ok_or(WorkspaceError::NotFound)?;
        let workspace = self.workspaces.remove(old);
        self.workspaces
            .insert(new_index.min(self.workspaces.len()), workspace);
        Ok(())
    }
    pub fn move_up(&mut self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let index = self
            .workspaces
            .iter()
            .position(|workspace| workspace.id == id)
            .ok_or(WorkspaceError::NotFound)?;
        if index > 0 {
            self.workspaces.swap(index, index - 1);
        }
        Ok(())
    }
    pub fn move_down(&mut self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        let index = self
            .workspaces
            .iter()
            .position(|workspace| workspace.id == id)
            .ok_or(WorkspaceError::NotFound)?;
        if index + 1 < self.workspaces.len() {
            self.workspaces.swap(index, index + 1);
        }
        Ok(())
    }

    /// Deletes a workspace and returns its tabs so the caller can apply its confirmed move/close policy.
    pub fn remove_workspace(&mut self, id: WorkspaceId) -> Result<Vec<TabId>, WorkspaceError> {
        if self.workspaces.len() == 1 {
            return Err(WorkspaceError::LastWorkspace);
        }
        let index = self
            .workspaces
            .iter()
            .position(|w| w.id == id)
            .ok_or(WorkspaceError::NotFound)?;
        let removed = self.workspaces.remove(index);
        if self.active == id {
            self.active = self.workspaces[index.min(self.workspaces.len() - 1)].id;
        }
        Ok(removed.tabs)
    }

    pub fn switch_to(&mut self, id: WorkspaceId) -> Result<WorkspaceTransition, WorkspaceError> {
        if self.get(id).is_none() {
            return Err(WorkspaceError::NotFound);
        }
        if id == self.active {
            return Ok(WorkspaceTransition {
                hidden: vec![],
                shown: vec![],
                selected: self.active().active_tab,
            });
        }
        let hidden = self.active().tabs.clone();
        self.active = id;
        Ok(WorkspaceTransition {
            hidden,
            shown: self.active().tabs.clone(),
            selected: self.active().active_tab,
        })
    }

    pub fn serialize(&self) -> String {
        let mut out = format!("active={}\n", self.active.0);
        if !self.session_order.is_empty() {
            out.push_str("order=");
            out.push_str(
                &self
                    .session_order
                    .iter()
                    .map(|id| id.0.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push('\n');
        }
        for workspace in &self.workspaces {
            let tabs = workspace
                .tabs
                .iter()
                .map(|id| id.0.to_string())
                .collect::<Vec<_>>()
                .join(",");
            out.push_str(&format!(
                "workspace={}\t{}\t{}\t{}\t{}\t{}\n",
                workspace.id.0,
                encode(&workspace.name),
                workspace.active_tab.0,
                tabs,
                workspace.icon.key(),
                workspace.colour.map_or("-", WorkspaceColour::key),
            ));
        }
        out
    }

    pub fn parse(text: &str) -> Result<Self, WorkspaceError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(WorkspaceError::InvalidFile);
        }
        let mut active = None;
        let mut workspaces = Vec::new();
        let mut ids = HashSet::new();
        let mut tabs = HashSet::new();
        let mut next_id = 0_u32;
        let mut session_order = Vec::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            if let Some(value) = line.strip_prefix("active=") {
                active = value.parse::<u32>().ok().map(WorkspaceId);
                if active.is_none() {
                    return Err(WorkspaceError::InvalidFile);
                }
                continue;
            }
            if let Some(value) = line.strip_prefix("order=") {
                for id in value.split(',').filter(|id| !id.is_empty()) {
                    let id = TabId(id.parse::<u32>().map_err(|_| WorkspaceError::InvalidFile)?);
                    if session_order.contains(&id) {
                        return Err(WorkspaceError::InvalidFile);
                    }
                    session_order.push(id);
                }
                continue;
            }
            let Some(value) = line.strip_prefix("workspace=") else {
                return Err(WorkspaceError::InvalidFile);
            };
            let mut fields = value.split('\t');
            let id = WorkspaceId(
                fields
                    .next()
                    .and_then(|v| v.parse::<u32>().ok())
                    .ok_or(WorkspaceError::InvalidFile)?,
            );
            let name = decode(fields.next().ok_or(WorkspaceError::InvalidFile)?)?;
            let name = clean_name(&name).ok_or(WorkspaceError::InvalidFile)?;
            let active_tab = TabId(
                fields
                    .next()
                    .and_then(|v| v.parse::<u32>().ok())
                    .ok_or(WorkspaceError::InvalidFile)?,
            );
            let members = fields.next().ok_or(WorkspaceError::InvalidFile)?;
            let icon = fields
                .next()
                .and_then(WorkspaceIcon::parse)
                .unwrap_or_default();
            let colour = fields.next().and_then(WorkspaceColour::parse);
            if !ids.insert(id)
                || workspaces
                    .iter()
                    .any(|w: &Workspace| w.name.eq_ignore_ascii_case(&name))
            {
                return Err(WorkspaceError::InvalidFile);
            }
            let mut member_ids = Vec::new();
            for member in members.split(',') {
                let member = TabId(
                    member
                        .parse::<u32>()
                        .map_err(|_| WorkspaceError::InvalidFile)?,
                );
                if !tabs.insert(member) {
                    return Err(WorkspaceError::DuplicateTab);
                }
                member_ids.push(member);
            }
            if member_ids.is_empty() || !member_ids.contains(&active_tab) {
                return Err(WorkspaceError::InvalidFile);
            }
            next_id = next_id.max(id.0.checked_add(1).ok_or(WorkspaceError::InvalidFile)?);
            workspaces.push(Workspace {
                id,
                name,
                tabs: member_ids,
                active_tab,
                icon,
                colour,
            });
        }
        if workspaces.is_empty() || workspaces.len() > MAX_WORKSPACES {
            return Err(WorkspaceError::InvalidFile);
        }
        let active = active
            .filter(|id| ids.contains(id))
            .ok_or(WorkspaceError::InvalidFile)?;
        if session_order.iter().any(|tab| !tabs.contains(tab)) {
            return Err(WorkspaceError::InvalidFile);
        }
        Ok(Self {
            workspaces,
            active,
            next_id,
            session_order,
        })
    }

    pub fn load(path: &Path) -> io::Result<Option<Self>> {
        match std::fs::read(path) {
            Ok(bytes) if bytes.len() <= MAX_FILE_BYTES => {
                let text = std::str::from_utf8(&bytes).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "workspace file is not UTF-8")
                })?;
                Self::parse(text).map(Some).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "workspace file is invalid")
                })
            }
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "workspace file is too large",
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = self.serialize();
        if text.len() > MAX_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "workspace file is too large",
            ));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension(format!("txt.{}.tmp", std::process::id()));
        let result = (|| {
            std::fs::write(&temporary, text.as_bytes())?;
            std::fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
        None
    } else {
        Some(name.to_owned())
    }
}

fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b" -_.".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode(value: &str) -> Result<String, WorkspaceError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let digits = bytes
                .get(index + 1..index + 3)
                .ok_or(WorkspaceError::InvalidFile)?;
            let digits = std::str::from_utf8(digits).map_err(|_| WorkspaceError::InvalidFile)?;
            out.push(u8::from_str_radix(digits, 16).map_err(|_| WorkspaceError::InvalidFile)?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| WorkspaceError::InvalidFile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::TabId;

    #[test]
    fn starts_with_one_workspace_and_switches_without_losing_membership() {
        let mut workspaces = WorkspaceSet::new("Default", TabId(1));
        let second_tab = TabId(2);
        workspaces.add_tab(second_tab).unwrap();
        let second = workspaces.create("Research", TabId(3)).unwrap();

        let transition = workspaces.switch_to(second).unwrap();
        assert_eq!(transition.hidden, vec![TabId(1), second_tab]);
        assert_eq!(transition.shown, vec![TabId(3)]);
        assert_eq!(transition.selected, TabId(3));
        assert_eq!(workspaces.active().tabs(), &[TabId(3)]);
    }

    #[test]
    fn saving_without_private_tabs_leaves_their_ids_out_unless_a_workspace_would_be_empty() {
        let mut workspaces = WorkspaceSet::new("Default", TabId(1));
        workspaces.add_tab(TabId(2)).unwrap();
        workspaces.add_tab(TabId(3)).unwrap();
        workspaces.select_tab(TabId(2)).unwrap();
        workspaces.create("Only private", TabId(4)).unwrap();
        workspaces.set_session_order(&[TabId(1), TabId(3)]);
        let excluded = HashSet::from([TabId(2), TabId(4)]);
        let saved = workspaces.without_tabs(&excluded);
        assert_eq!(saved.get(WorkspaceId(0)).unwrap().tabs(), &[TabId(1), TabId(3)]);
        assert_eq!(saved.get(WorkspaceId(0)).unwrap().active_tab(), TabId(1));
        // The only tab of a workspace stays, so the file remains valid.
        assert_eq!(saved.get(WorkspaceId(1)).unwrap().tabs(), &[TabId(4)]);
        let text = saved.serialize();
        assert!(!text.contains("1,2,3"), "{text}");
        assert!(WorkspaceSet::parse(&text).is_ok(), "{text}");
        // The original is untouched.
        assert_eq!(workspaces.get(WorkspaceId(0)).unwrap().tabs().len(), 3);
    }

    #[test]
    fn workspace_reconcile_keeps_saved_membership_and_assigns_restored_tabs_once() {
        let mut workspaces = WorkspaceSet::new("Default", TabId(1));
        workspaces.add_tab(TabId(2)).unwrap();
        let research = workspaces.create("Research", TabId(3)).unwrap();
        workspaces.switch_to(research).unwrap();

        workspaces.reconcile(&[TabId(1), TabId(3), TabId(4)], TabId(3));

        assert_eq!(workspaces.active_id(), research);
        assert_eq!(workspaces.active().tabs(), &[TabId(3), TabId(4)]);
        assert_eq!(workspaces.get(WorkspaceId(0)).unwrap().tabs(), &[TabId(1)]);
        assert_eq!(workspaces.workspace_for(TabId(4)), Some(research));
    }

    #[test]
    fn persisted_session_order_remaps_process_local_ids_on_restore() {
        let mut workspaces = WorkspaceSet::new("Default", TabId(17));
        workspaces.add_tab(TabId(42)).unwrap();
        workspaces.set_session_order(&[TabId(42), TabId(17)]);
        let mut restored = WorkspaceSet::parse(&workspaces.serialize()).unwrap();

        restored.remap_session_order(&[TabId(0), TabId(1)]);
        restored.reconcile(&[TabId(0), TabId(1)], TabId(1));

        assert_eq!(restored.active().tabs(), &[TabId(1), TabId(0)]);
        assert_eq!(restored.active().active_tab(), TabId(1));
    }

    #[test]
    fn switching_back_selects_the_last_active_tab_and_hides_all_old_members() {
        let mut workspaces = WorkspaceSet::new("Default", TabId(1));
        workspaces.add_tab(TabId(2)).unwrap();
        let research = workspaces.create("Research", TabId(3)).unwrap();
        workspaces.select_tab(TabId(2)).unwrap();
        workspaces.switch_to(research).unwrap();
        let transition = workspaces.switch_to(WorkspaceId(0)).unwrap();

        assert_eq!(transition.hidden, vec![TabId(3)]);
        assert_eq!(transition.shown, vec![TabId(1), TabId(2)]);
        assert_eq!(transition.selected, TabId(2));
    }

    #[test]
    fn rejects_empty_duplicate_and_last_tab_removal() {
        let mut workspaces = WorkspaceSet::new("Default", TabId(1));
        assert_eq!(
            workspaces.create("  ", TabId(2)),
            Err(WorkspaceError::InvalidName)
        );
        assert_eq!(
            workspaces.create("Default", TabId(2)),
            Err(WorkspaceError::DuplicateName)
        );
        assert_eq!(
            workspaces.remove_tab(TabId(1)),
            Err(WorkspaceError::LastTab)
        );
        assert_eq!(
            workspaces.remove_workspace(workspaces.active_id()),
            Err(WorkspaceError::LastWorkspace)
        );
    }

    #[test]
    fn serialized_state_round_trips_and_rejects_duplicate_tab_membership() {
        let mut workspaces = WorkspaceSet::new("Default", TabId(1));
        let research = workspaces.create("Research & Notes", TabId(2)).unwrap();
        workspaces.add_tab(TabId(3)).unwrap();
        workspaces.switch_to(research).unwrap();
        let restored = WorkspaceSet::parse(&workspaces.serialize()).unwrap();
        assert_eq!(restored, workspaces);

        let corrupt = "active=0\nworkspace=0\tDefault\t1\t1,2\nworkspace=1\tOther\t2\t2,3\n";
        assert_eq!(
            WorkspaceSet::parse(corrupt),
            Err(WorkspaceError::DuplicateTab)
        );
    }

    #[test]
    fn save_and_load_round_trip_at_the_profile_scoped_path() {
        let mut workspaces = WorkspaceSet::new("Default", TabId(1));
        workspaces.create("Research", TabId(2)).unwrap();
        let dir = std::env::temp_dir().join(format!("bowser-workspaces-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        workspaces.save(&path).unwrap();
        assert_eq!(WorkspaceSet::load(&path).unwrap(), Some(workspaces));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn icon_colour_round_trip_and_legacy_defaults() {
        let mut workspaces = WorkspaceSet::new("Default", TabId(1));
        workspaces
            .set_icon(WorkspaceId(0), WorkspaceIcon::Code)
            .unwrap();
        workspaces
            .set_colour(WorkspaceId(0), Some(WorkspaceColour::Teal))
            .unwrap();
        let parsed = WorkspaceSet::parse(&workspaces.serialize()).unwrap();
        assert_eq!(parsed.active().icon(), WorkspaceIcon::Code);
        assert_eq!(parsed.active().colour(), Some(WorkspaceColour::Teal));
        let legacy = WorkspaceSet::parse("active=0\nworkspace=0\tDefault\t1\t1\n").unwrap();
        assert_eq!(legacy.active().icon(), WorkspaceIcon::Briefcase);
        assert_eq!(legacy.active().colour(), None);
    }

    #[test]
    fn reorder_preserves_ids_and_delete_active_selects_neighbor() {
        let mut set = WorkspaceSet::new("Default", TabId(1));
        let second = set.create("Work", TabId(2)).unwrap();
        let third = set.create("Play", TabId(3)).unwrap();
        set.reorder(third, 0).unwrap();
        assert_eq!(set.all()[0].id(), third);
        set.switch_to(second).unwrap();
        assert_eq!(set.remove_workspace(second).unwrap(), vec![TabId(2)]);
        assert_ne!(set.active_id(), second);
        let remaining_id = set.all()[0].id();
        assert_eq!(set.remove_workspace(remaining_id).unwrap().len(), 1);
        assert_eq!(
            set.remove_workspace(set.active_id()),
            Err(WorkspaceError::LastWorkspace)
        );
    }
}
