//! Delete Browsing Data: what the user can delete and how far back, and the plan that turns a
//! choice into work. Pure: the kinds, the ranges, the checks (saved passwords are never deleted
//! unless the choice says so explicitly) and the words. Running the plan is `app::privacy`.

use crate::engine::{BrowsingDataKind, BrowsingDataRange};

/// What can be deleted, in the order the dialog lists it. The index is the kind's bit in a
/// [`Selection`] on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataKind {
    /// Bowser's own history list, and the browser engine's browsing history.
    History,
    Cookies,
    /// Local storage, IndexedDB, service workers and the other site storage.
    Storage,
    /// Cached files.
    Cache,
    /// Saved logins in Bowser's own vault. Never covered by a time range.
    Passwords,
    /// Form autofill entries.
    Autofill,
    /// Per-site permissions, zoom and translation choices.
    SiteSettings,
}

impl DataKind {
    pub const ALL: [DataKind; 7] = [
        Self::History,
        Self::Cookies,
        Self::Storage,
        Self::Cache,
        Self::Passwords,
        Self::Autofill,
        Self::SiteSettings,
    ];

    pub fn bit(self) -> u8 {
        1 << Self::ALL.iter().position(|kind| *kind == self).unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::History => "browsing history",
            Self::Cookies => "cookies",
            Self::Storage => "site storage",
            Self::Cache => "cached files",
            Self::Passwords => "saved passwords",
            Self::Autofill => "form autofill",
            Self::SiteSettings => "site settings",
        }
    }
}

/// A set of [`DataKind`]s: bit `n` is `DataKind::ALL[n]`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Selection(u8);

impl Selection {
    /// `None` for a value with a bit beyond the kinds that exist.
    pub fn from_bits(bits: u32) -> Option<Self> {
        let mask = (1u32 << DataKind::ALL.len()) - 1;
        (bits & !mask == 0).then_some(Self(bits as u8))
    }

    pub fn contains(self, kind: DataKind) -> bool {
        self.0 & kind.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn kinds(self) -> impl Iterator<Item = DataKind> {
        DataKind::ALL.into_iter().filter(move |kind| self.contains(*kind))
    }
}

/// How far back to delete, in the order the dialog lists it. The index is the range on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Range {
    Hour,
    Day,
    Week,
    FourWeeks,
    Everything,
}

impl Range {
    pub const ALL: [Range; 5] = [Self::Hour, Self::Day, Self::Week, Self::FourWeeks, Self::Everything];

    pub fn from_index(index: u32) -> Option<Self> {
        Self::ALL.get(usize::try_from(index).ok()?).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Hour => "the past hour",
            Self::Day => "the past day",
            Self::Week => "the past week",
            Self::FourWeeks => "the past 4 weeks",
            Self::Everything => "all time",
        }
    }

    /// How far back it reaches in seconds; `None` is everything.
    pub fn seconds(self) -> Option<u64> {
        self.engine().seconds()
    }

    pub fn engine(self) -> BrowsingDataRange {
        match self {
            Self::Hour => BrowsingDataRange::LastHour,
            Self::Day => BrowsingDataRange::LastDay,
            Self::Week => BrowsingDataRange::LastSevenDays,
            Self::FourWeeks => BrowsingDataRange::LastFourWeeks,
            Self::Everything => BrowsingDataRange::AllTime,
        }
    }
}

/// What a choice asks for, split by who does it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub range: Range,
    /// Delete visits from Bowser's own history list within the range.
    pub local_history: bool,
    /// The browser engine's own data, deleted in one request within the range.
    pub engine: Vec<BrowsingDataKind>,
    /// Delete every saved login from the native vault (a vault has no dates to apply a range to).
    pub passwords: bool,
    /// Delete Bowser's own per-site choices (permissions, zoom, translation). These have no dates
    /// either, so they go only with "all time".
    pub own_site_settings: bool,
    /// The kinds that ignore the range, so the dialog and the result can say so.
    pub ignored_range: Vec<DataKind>,
}

/// Why a choice was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanError {
    Nothing,
    /// Passwords were selected without the explicit confirmation.
    PasswordsNeedConfirmation,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Nothing => "Choose what to delete.",
            Self::PasswordsNeedConfirmation => "Deleting saved passwords needs your confirmation. Nothing was deleted.",
        })
    }
}

/// Turns a choice into a plan. `passwords_confirmed` is the dialog's explicit "yes, delete my saved
/// passwords"; without it a selection that includes passwords is refused whole, so nothing at all
/// is deleted by accident.
pub fn plan(selection: Selection, range: Range, passwords_confirmed: bool) -> Result<Plan, PlanError> {
    if selection.is_empty() {
        return Err(PlanError::Nothing);
    }
    if selection.contains(DataKind::Passwords) && !passwords_confirmed {
        return Err(PlanError::PasswordsNeedConfirmation);
    }
    let mut engine = Vec::new();
    for kind in selection.kinds() {
        match kind {
            DataKind::History => engine.push(BrowsingDataKind::History),
            DataKind::Cookies => engine.push(BrowsingDataKind::Cookies),
            DataKind::Storage => engine.push(BrowsingDataKind::Storage),
            DataKind::Cache => engine.push(BrowsingDataKind::Cache),
            DataKind::Autofill => engine.push(BrowsingDataKind::Autofill),
            DataKind::SiteSettings => engine.push(BrowsingDataKind::SiteSettings),
            DataKind::Passwords => {}
        }
    }
    let everything = range == Range::Everything;
    let mut ignored_range = Vec::new();
    if selection.contains(DataKind::Passwords) && !everything {
        ignored_range.push(DataKind::Passwords);
    }
    if selection.contains(DataKind::SiteSettings) && !everything {
        // The engine's own settings honour the range; Bowser's keep no dates, so they wait for
        // "all time" and the result says so.
        ignored_range.push(DataKind::SiteSettings);
    }
    Ok(Plan {
        range,
        local_history: selection.contains(DataKind::History),
        engine,
        passwords: selection.contains(DataKind::Passwords),
        own_site_settings: selection.contains(DataKind::SiteSettings) && everything,
        ignored_range,
    })
}

/// The kind of data a browser-engine kind deletes, for the report. `SiteData` is the older
/// cookies-and-storage kind and reports as cookies.
pub fn data_kind_of(kind: BrowsingDataKind) -> Option<DataKind> {
    Some(match kind {
        BrowsingDataKind::History => DataKind::History,
        BrowsingDataKind::Cookies | BrowsingDataKind::SiteData => DataKind::Cookies,
        BrowsingDataKind::Storage => DataKind::Storage,
        BrowsingDataKind::Cache => DataKind::Cache,
        BrowsingDataKind::Autofill => DataKind::Autofill,
        BrowsingDataKind::SiteSettings => DataKind::SiteSettings,
    })
}

/// What has been deleted so far, and what went wrong.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub done: Vec<DataKind>,
    pub problems: Vec<String>,
}

/// How a Delete Browsing Data request ended: whether everything asked for was deleted, and the
/// sentence to show the user.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteReport {
    pub ok: bool,
    pub text: String,
}

/// The sentence for a finished plan. Kinds that have no dates are named when a time range was
/// chosen, so the user is never told more was deleted from "the past hour" than really was.
pub fn report(plan: &Plan, progress: &Progress) -> DeleteReport {
    let mut done = Selection::default();
    for kind in &progress.done {
        done.0 |= kind.bit();
    }
    let mut text = if done.is_empty() {
        String::new()
    } else {
        let scope = if plan.range == Range::Everything { "" } else { " from " };
        let range = if plan.range == Range::Everything { String::new() } else { plan.range.label().to_owned() };
        format!("Deleted {}{scope}{range}.", kinds_sentence(done))
    };
    if progress.problems.is_empty() && !done.is_empty() {
        if plan.ignored_range.contains(&DataKind::Passwords) && done.contains(DataKind::Passwords) {
            text.push_str(" Saved passwords have no dates, so all of them were deleted.");
        }
        if plan.ignored_range.contains(&DataKind::SiteSettings) {
            text.push_str(" Site choices kept by Bowser have no dates, so they stay unless you choose all time.");
        }
    }
    for problem in &progress.problems {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(problem);
    }
    if text.is_empty() {
        text.push_str("Nothing was deleted.");
    }
    DeleteReport { ok: progress.problems.is_empty(), text }
}

/// "browsing history, cookies and cached files": the chosen kinds as a sentence part.
pub fn kinds_sentence(selection: Selection) -> String {
    let labels: Vec<&str> = selection.kinds().map(DataKind::label).collect();
    match labels.as_slice() {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(kinds: &[DataKind]) -> Selection {
        Selection::from_bits(kinds.iter().map(|kind| u32::from(kind.bit())).sum()).unwrap()
    }

    #[test]
    fn every_kind_has_its_own_bit_in_dialog_order() {
        let bits: Vec<u8> = DataKind::ALL.iter().map(|kind| kind.bit()).collect();
        assert_eq!(bits, [1, 2, 4, 8, 16, 32, 64]);
        assert_eq!(Selection::from_bits(0b111_1111).unwrap().kinds().count(), 7);
        assert_eq!(Selection::from_bits(0b1000_0000), None, "a bit beyond the kinds is refused");
        assert_eq!(Selection::from_bits(u32::MAX), None);
        assert!(Selection::from_bits(0).unwrap().is_empty());
    }

    #[test]
    fn the_ranges_are_the_five_the_dialog_offers() {
        let seconds: Vec<Option<u64>> = Range::ALL.iter().map(|range| range.seconds()).collect();
        assert_eq!(seconds, [Some(3_600), Some(86_400), Some(604_800), Some(2_419_200), None]);
        assert_eq!(Range::from_index(3), Some(Range::FourWeeks));
        assert_eq!(Range::from_index(5), None);
        assert_eq!(Range::from_index(u32::MAX), None);
    }

    #[test]
    fn an_empty_choice_deletes_nothing() {
        assert_eq!(plan(Selection::default(), Range::Day, true), Err(PlanError::Nothing));
    }

    #[test]
    fn passwords_are_refused_without_the_explicit_confirmation_and_take_everything_else_with_them() {
        let all = Selection::from_bits(0b111_1111).unwrap();
        assert_eq!(plan(all, Range::Everything, false), Err(PlanError::PasswordsNeedConfirmation));
        let history_and_passwords = selection(&[DataKind::History, DataKind::Passwords]);
        assert_eq!(plan(history_and_passwords, Range::Hour, false), Err(PlanError::PasswordsNeedConfirmation));
        // The same choice without passwords needs no confirmation.
        let without = selection(&[DataKind::History, DataKind::Cache]);
        assert!(plan(without, Range::Hour, false).is_ok());
        assert!(plan(all, Range::Everything, true).unwrap().passwords);
        assert!(!plan(without, Range::Everything, true).unwrap().passwords, "confirming does not add passwords");
    }

    #[test]
    fn each_kind_reaches_the_right_worker() {
        let plan = plan(Selection::from_bits(0b111_1111).unwrap(), Range::Everything, true).unwrap();
        assert!(plan.local_history && plan.passwords && plan.own_site_settings);
        assert_eq!(
            plan.engine,
            [
                BrowsingDataKind::History,
                BrowsingDataKind::Cookies,
                BrowsingDataKind::Storage,
                BrowsingDataKind::Cache,
                BrowsingDataKind::Autofill,
                BrowsingDataKind::SiteSettings,
            ]
        );
        assert!(plan.ignored_range.is_empty());
        let only_cookies = super::plan(selection(&[DataKind::Cookies]), Range::Week, false).unwrap();
        assert_eq!(only_cookies.engine, [BrowsingDataKind::Cookies]);
        assert!(!only_cookies.local_history && !only_cookies.passwords && !only_cookies.own_site_settings);
        assert_eq!(only_cookies.range.engine(), BrowsingDataRange::LastSevenDays);
    }

    #[test]
    fn kinds_without_dates_say_so_when_a_range_is_chosen() {
        let chosen = selection(&[DataKind::Passwords, DataKind::SiteSettings, DataKind::Cookies]);
        let plan = plan(chosen, Range::Day, true).unwrap();
        assert_eq!(plan.ignored_range, [DataKind::Passwords, DataKind::SiteSettings]);
        assert!(plan.passwords, "passwords still go when asked");
        assert!(!plan.own_site_settings, "Bowser's own site choices wait for all time");
        assert!(plan.engine.contains(&BrowsingDataKind::SiteSettings), "the engine's settings honour the range");
    }

    #[test]
    fn the_report_says_what_was_deleted_from_when_and_what_has_no_dates() {
        let chosen = selection(&[DataKind::History, DataKind::Cookies, DataKind::Passwords, DataKind::SiteSettings]);
        let hour = plan(chosen, Range::Hour, true).unwrap();
        let progress = Progress { done: vec![DataKind::History, DataKind::Cookies, DataKind::Passwords], problems: vec![] };
        let report = report(&hour, &progress);
        assert!(report.ok);
        assert_eq!(
            report.text,
            "Deleted browsing history, cookies and saved passwords from the past hour. \
             Saved passwords have no dates, so all of them were deleted. \
             Site choices kept by Bowser have no dates, so they stay unless you choose all time."
        );
        let everything = plan(selection(&[DataKind::Cache]), Range::Everything, false).unwrap();
        let done = Progress { done: vec![DataKind::Cache], problems: vec![] };
        assert_eq!(super::report(&everything, &done).text, "Deleted cached files.");
        let failed = Progress { done: vec![DataKind::History], problems: vec!["The browser's own data could not be deleted: no.".into()] };
        let report = super::report(&hour, &failed);
        assert!(!report.ok);
        assert_eq!(report.text, "Deleted browsing history from the past hour. The browser's own data could not be deleted: no.");
        assert_eq!(super::report(&hour, &Progress::default()).text, "Nothing was deleted.");
        // Every engine kind reports as a kind the dialog lists.
        for kind in [
            BrowsingDataKind::SiteData,
            BrowsingDataKind::Cache,
            BrowsingDataKind::History,
            BrowsingDataKind::Cookies,
            BrowsingDataKind::Storage,
            BrowsingDataKind::Autofill,
            BrowsingDataKind::SiteSettings,
        ] {
            assert!(data_kind_of(kind).is_some(), "{kind:?}");
        }
    }

    #[test]
    fn the_chosen_kinds_read_as_a_sentence() {
        assert_eq!(kinds_sentence(Selection::default()), "");
        assert_eq!(kinds_sentence(selection(&[DataKind::Cache])), "cached files");
        assert_eq!(
            kinds_sentence(selection(&[DataKind::History, DataKind::Cookies, DataKind::Autofill])),
            "browsing history, cookies and form autofill"
        );
    }
}
