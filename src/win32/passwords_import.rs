//! Passwords & Passkeys import flow: the pure state model and the bridge to the password core.
//!
//! The panel (`passwords_panel`) drives the flow. This module reads the chosen file, asks the core
//! for a preview and commits the previewed plan. Only counts and fixed messages leave it, so no
//! credential value reaches the UI. Errors are payload-free for the same reason.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

use crate::passwords::{
    Classification, ImportPlan, MAX_IMPORT_BYTES, PasswordError, PasswordStore, PreviewRow,
};

/// An import source. The core reads official password CSV exports by their column headers, so this
/// is the only source offered. It names no browser or manager the core has not been shown to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub id: &'static str,
    pub label: &'static str,
    /// File extension without the dot, used in the file picker title.
    pub extension: &'static str,
}

pub const CSV_PROFILE: Profile = Profile {
    id: "csv",
    label: "Password CSV export",
    extension: "csv",
};

pub fn profiles() -> Vec<Profile> {
    vec![CSV_PROFILE]
}

/// Preview counts for a file. Nothing is written until the user confirms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub imported: usize,
    pub duplicates: usize,
    pub conflicts: usize,
    pub errors: usize,
}

/// Result of a confirmed import.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub succeeded: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The file could not be read, or is not a password CSV the core accepts.
    Unreadable,
    /// The encrypted vault could not be opened or written. Nothing was imported.
    VaultUnavailable,
}

/// A previewed file. The plan holds the parsed credentials in memory until it is committed or dropped.
pub struct PreviewedFile {
    pub counts: Counts,
    pub plan: ImportPlan,
}

/// Reads and classifies a file against the vault without writing anything.
pub fn preview_file(path: &Path) -> Result<PreviewedFile, ImportError> {
    // Bound the actual read, not only a metadata check: the file may change between the two.
    let mut bytes = Zeroizing::new(Vec::new());
    File::open(path)
        .map_err(|_| ImportError::Unreadable)?
        .take(MAX_IMPORT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ImportError::Unreadable)?;
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err(ImportError::Unreadable);
    }
    let store = PasswordStore::open_default().map_err(|_| ImportError::VaultUnavailable)?;
    let plan = store.preview_csv(&bytes).map_err(|error| match error {
        PasswordError::Io | PasswordError::Protection | PasswordError::InvalidStore => {
            ImportError::VaultUnavailable
        }
        _ => ImportError::Unreadable,
    })?;
    Ok(PreviewedFile {
        counts: count(plan.rows()),
        plan,
    })
}

/// Writes the previewed plan. The core reclassifies against the vault at this point, so entries
/// added since the preview are skipped rather than duplicated.
pub fn commit_plan(plan: ImportPlan) -> Result<Outcome, ImportError> {
    let planned = count(plan.rows());
    let mut store = PasswordStore::open_default().map_err(|_| ImportError::VaultUnavailable)?;
    let added = store
        .commit(plan)
        .map_err(|_| ImportError::VaultUnavailable)?;
    Ok(Outcome {
        succeeded: added,
        skipped: (planned.imported - added.min(planned.imported))
            + planned.duplicates
            + planned.conflicts,
        failed: planned.errors,
    })
}

fn count(rows: &[PreviewRow]) -> Counts {
    let mut counts = Counts::default();
    for row in rows {
        match row.classification {
            Classification::New => counts.imported += 1,
            Classification::ExactDuplicate => counts.duplicates += 1,
            Classification::Conflict => counts.conflicts += 1,
            Classification::Reject => counts.errors += 1,
        }
    }
    counts
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Choose a source and a file.
    Choose,
    /// Counts for the chosen file, waiting for explicit confirmation.
    Preview { counts: Counts },
    /// The import finished; `outcome` is what was written.
    Done { outcome: Outcome },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flow {
    pub profiles: Vec<Profile>,
    pub selected: usize,
    pub file: Option<PathBuf>,
    pub stage: Stage,
    /// Set when the last attempt failed. It is always one of the fixed messages from [`ImportError`].
    pub error: Option<ImportError>,
}

impl Flow {
    pub fn new(profiles: Vec<Profile>) -> Self {
        Self {
            profiles,
            selected: 0,
            file: None,
            stage: Stage::Choose,
            error: None,
        }
    }

    pub fn selected_profile(&self) -> Option<Profile> {
        self.profiles.get(self.selected).copied()
    }

    pub fn select_next(&mut self) {
        if !self.profiles.is_empty() {
            self.selected = (self.selected + 1) % self.profiles.len();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.profiles.is_empty() {
            self.selected = (self.selected + self.profiles.len() - 1) % self.profiles.len();
        }
    }

    pub fn can_choose_file(&self) -> bool {
        self.stage == Stage::Choose && self.selected_profile().is_some()
    }

    pub fn can_import(&self) -> bool {
        matches!(self.stage, Stage::Preview { counts } if counts.imported > 0)
    }

    /// Records the preview for a picked file. A failed preview keeps the user on the source step.
    pub fn apply_preview(&mut self, path: PathBuf, result: Result<Counts, ImportError>) {
        match result {
            Ok(counts) => {
                self.file = Some(path);
                self.error = None;
                self.stage = Stage::Preview { counts };
            }
            Err(error) => {
                self.file = None;
                self.error = Some(error);
                self.stage = Stage::Choose;
            }
        }
    }

    pub fn apply_commit(&mut self, result: Result<Outcome, ImportError>) {
        match result {
            Ok(outcome) => {
                self.error = None;
                self.stage = Stage::Done { outcome };
            }
            Err(error) => {
                self.error = Some(error);
                self.file = None;
                self.stage = Stage::Choose;
            }
        }
    }

    pub fn back(&mut self) {
        if matches!(self.stage, Stage::Preview { .. }) {
            self.file = None;
            self.error = None;
            self.stage = Stage::Choose;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME: Profile = Profile {
        id: "chrome-csv",
        label: "Chrome, Edge or Google CSV",
        extension: "csv",
    };
    const FIREFOX: Profile = Profile {
        id: "firefox-csv",
        label: "Firefox CSV",
        extension: "csv",
    };

    fn counts(imported: usize) -> Counts {
        Counts {
            imported,
            duplicates: 2,
            conflicts: 1,
            errors: 0,
        }
    }

    #[test]
    fn with_no_detected_sources_nothing_can_be_chosen() {
        let flow = Flow::new(Vec::new());
        assert!(!flow.can_choose_file());
        assert_eq!(flow.selected_profile(), None);
    }

    #[test]
    fn source_selection_wraps_in_both_directions() {
        let mut flow = Flow::new(vec![CHROME, FIREFOX]);
        flow.select_previous();
        assert_eq!(flow.selected_profile(), Some(FIREFOX));
        flow.select_next();
        assert_eq!(flow.selected_profile(), Some(CHROME));
    }

    #[test]
    fn a_previewed_file_waits_for_confirmation_and_can_be_backed_out_of() {
        let mut flow = Flow::new(vec![CHROME]);
        flow.apply_preview(PathBuf::from("passwords.csv"), Ok(counts(3)));
        assert_eq!(flow.stage, Stage::Preview { counts: counts(3) });
        assert!(flow.can_import());
        flow.back();
        assert_eq!(flow.stage, Stage::Choose);
        assert_eq!(flow.file, None);
    }

    #[test]
    fn a_preview_with_nothing_to_import_cannot_be_confirmed() {
        let mut flow = Flow::new(vec![CHROME]);
        flow.apply_preview(PathBuf::from("passwords.csv"), Ok(counts(0)));
        assert!(!flow.can_import());
    }

    #[test]
    fn a_failed_preview_stays_on_source_with_a_fixed_message() {
        let mut flow = Flow::new(vec![CHROME]);
        flow.apply_preview(PathBuf::from("bad.csv"), Err(ImportError::Unreadable));
        assert_eq!(flow.stage, Stage::Choose);
        assert_eq!(flow.error, Some(ImportError::Unreadable));
        assert_eq!(flow.file, None);
    }

    #[test]
    fn a_confirmed_import_reports_its_outcome() {
        let mut flow = Flow::new(vec![CHROME]);
        flow.apply_preview(PathBuf::from("passwords.csv"), Ok(counts(3)));
        let outcome = Outcome {
            succeeded: 3,
            skipped: 3,
            failed: 0,
        };
        flow.apply_commit(Ok(outcome));
        assert_eq!(flow.stage, Stage::Done { outcome });
    }

    #[test]
    fn an_interrupted_import_returns_to_source_with_a_warning() {
        let mut flow = Flow::new(vec![CHROME]);
        flow.apply_preview(PathBuf::from("passwords.csv"), Ok(counts(3)));
        flow.apply_commit(Err(ImportError::VaultUnavailable));
        assert_eq!(flow.stage, Stage::Choose);
        assert_eq!(flow.error, Some(ImportError::VaultUnavailable));
    }
}
