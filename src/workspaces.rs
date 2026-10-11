//! Per-profile workspace membership and persistence, independent of browser handles.

use std::{collections::HashSet, io, path::Path};

use crate::tabs::TabId;

const MAX_WORKSPACES: usize = 128;
const MAX_NAME_BYTES: usize = 256;
const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const FILE_NAME: &str = "workspaces.txt";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WorkspaceId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    id: WorkspaceId,
    name: String,
    tabs: Vec<TabId>,
    active_tab: TabId,
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
            }],
            active: WorkspaceId(0),
            next_id: 1,
        }
    }

    pub fn all(&self) -> &[Workspace] {
        &self.workspaces
    }
    pub fn active_id(&self) -> WorkspaceId {
        self.active
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
        for workspace in &self.workspaces {
            let tabs = workspace
                .tabs
                .iter()
                .map(|id| id.0.to_string())
                .collect::<Vec<_>>()
                .join(",");
            out.push_str(&format!(
                "workspace={}\t{}\t{}\t{}\n",
                workspace.id.0,
                encode(&workspace.name),
                workspace.active_tab.0,
                tabs
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
            if fields.next().is_some()
                || !ids.insert(id)
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
            });
        }
        if workspaces.is_empty() || workspaces.len() > MAX_WORKSPACES {
            return Err(WorkspaceError::InvalidFile);
        }
        let active = active
            .filter(|id| ids.contains(id))
            .ok_or(WorkspaceError::InvalidFile)?;
        Ok(Self {
            workspaces,
            active,
            next_id,
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
}
