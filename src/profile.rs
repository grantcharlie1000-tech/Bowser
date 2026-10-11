//! Per-profile data locations: the single place that decides where Bowser keeps its files.
//!
//! The default profile uses `%LOCALAPPDATA%\Bowser` exactly as before. A named profile
//! (`--profile <name>`) lives in `%LOCALAPPDATA%\Bowser\Profiles\<name>\` and owns its own
//! WebView2 user-data folder, session, tab groups, settings, blocker sites and password vault,
//! so profiles share no browser data. The app-wide updater files stay under `%LOCALAPPDATA%\Bowser`.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_NAME: &str = "default";
pub const PROFILES_DIR_NAME: &str = "Profiles";
pub const MAX_NAME_LEN: usize = 32;
/// Upper bound for auto-named profiles (`Profile-1` ... `Profile-999`).
const MAX_AUTO_NAME: u32 = 999;

const APP_DIR_NAME: &str = "Bowser";
const WEBVIEW2_DIR_NAME: &str = "WebView2";
const PASSWORDS_FILE_NAME: &str = "Passwords.dat";
const SEARCH_PROVIDER_FILE_NAME: &str = "search-provider.txt";
const TAB_GROUPS_FILE_NAME: &str = "tab-groups.txt";
const EXTENSIONS_DIR_NAME: &str = "Extensions";
const EXTENSIONS_FILE_NAME: &str = "extensions.txt";
const SETTINGS_FILE_NAME: &str = "settings.txt";
const METADATA_FILE_NAME: &str = "profile.txt";
const LAST_PROFILE_FILE_NAME: &str = "last-profile.txt";
const IMPORT_OFFER_FILE_NAME: &str = "import-offer.txt";
const GUEST_DIR_NAME: &str = "Guest";
const LOCK_FILE_NAME: &str = "running.lock";
const TRASH_PREFIX: &str = ".trash-";
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// Windows device names. Names are ASCII alphanumerics, `_` and `-` only, so these exact
/// matches cover every device name that could otherwise be reached through a profile name.
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

static ACTIVE: OnceLock<Profile> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    InvalidName,
    ReservedName,
    NoLocalAppData,
    AlreadyExists,
    InvalidMetadata,
    NotFound,
    /// The profile is the running one, or another Bowser window still has it open.
    InUse,
    /// The target resolves outside the Profiles folder, or the Profiles folder is a link.
    OutsideProfiles,
    Io,
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::InvalidName => "Profile names use 1 to 32 letters, digits, '-' or '_'.",
            Self::ReservedName => "That profile name is reserved.",
            Self::NoLocalAppData => "Bowser could not find this user's LocalAppData directory.",
            Self::AlreadyExists => "A profile with that name already exists.",
            Self::InvalidMetadata => "That profile avatar or accent is not supported.",
            Self::NotFound => "That profile does not exist.",
            Self::InUse => "That profile is open in a Bowser window. Close it first.",
            Self::OutsideProfiles => {
                "Bowser refused to touch a folder outside its Profiles folder."
            }
            Self::Io => "The profile folder could not be changed.",
        };
        f.write_str(text)
    }
}

impl std::error::Error for ProfileError {}

/// A profile and the directory that holds its data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    name: Option<String>,
    root: PathBuf,
    guest: bool,
}

/// Lifetime guard that keeps a guest marked live and removes its directory on every exit path.
pub struct GuestSession {
    profile: Profile,
    lock: Option<std::fs::File>,
}

impl GuestSession {
    pub fn start(profile: &Profile) -> Result<Self, ProfileError> {
        if !profile.is_guest() {
            return Err(ProfileError::Io);
        }
        let lock = profile.lock_guest()?;
        Ok(Self {
            profile: profile.clone(),
            lock: Some(lock),
        })
    }
}

impl Drop for GuestSession {
    fn drop(&mut self) {
        self.lock.take();
        let _ = cleanup_guest(&self.profile);
    }
}

/// User-editable presentation details. Unknown keys survive load/save for forward compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileMetadata {
    pub avatar: String,
    pub accent: String,
    pub unknown: BTreeMap<String, String>,
}

const AVATARS: &[&str] = &["initial", "leaf", "star", "wave", "moon"];
const ACCENTS: &[&str] = &["blue", "violet", "teal", "amber", "rose"];

impl Default for ProfileMetadata {
    fn default() -> Self {
        Self {
            avatar: "initial".into(),
            accent: "blue".into(),
            unknown: BTreeMap::new(),
        }
    }
}

impl ProfileMetadata {
    pub fn accent_colour(&self) -> crate::theme::Rgba {
        match self.accent.as_str() {
            "violet" => crate::theme::Rgba::hex(0x6542A5, 255),
            "teal" => crate::theme::Rgba::hex(0x176D62, 255),
            "amber" => crate::theme::Rgba::hex(0x945700, 255),
            "rose" => crate::theme::Rgba::hex(0xA33F5A, 255),
            _ => crate::theme::Rgba::hex(0x2B5FA8, 255),
        }
    }

    pub fn avatar_glyph(&self, name: &str) -> String {
        match self.avatar.as_str() {
            "leaf" => "❧".to_owned(),
            "star" => "✦".to_owned(),
            "wave" => "≈".to_owned(),
            "moon" => "☾".to_owned(),
            _ => name
                .chars()
                .next()
                .map_or_else(|| "B".to_owned(), |c| c.to_uppercase().to_string()),
        }
    }

    pub fn parse(text: &str) -> Self {
        let mut metadata = Self::default();
        for line in text.lines().take(128) {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || key.len() > 64 || value.len() > 256 {
                continue;
            }
            match key {
                "avatar" if AVATARS.contains(&value) => metadata.avatar = value.to_owned(),
                "accent" if ACCENTS.contains(&value) => metadata.accent = value.to_owned(),
                "avatar" | "accent" => {}
                _ => {
                    metadata.unknown.insert(key.to_owned(), value.to_owned());
                }
            }
        }
        metadata
    }

    pub fn load(profile: &Profile) -> Self {
        fs::read_to_string(profile.metadata_file())
            .map_or_else(|_| Self::default(), |text| Self::parse(&text))
    }

    pub fn save(&self, profile: &Profile) -> Result<(), ProfileError> {
        if !AVATARS.contains(&self.avatar.as_str()) || !ACCENTS.contains(&self.accent.as_str()) {
            return Err(ProfileError::InvalidMetadata);
        }
        let mut text = format!("avatar={}\naccent={}\n", self.avatar, self.accent);
        for (key, value) in &self.unknown {
            if !key.is_empty()
                && key.len() <= 64
                && !key.contains(['=', '\r', '\n'])
                && value.len() <= 256
                && !value.contains(['\r', '\n'])
                && key != "avatar"
                && key != "accent"
            {
                text.push_str(key);
                text.push('=');
                text.push_str(value);
                text.push('\n');
            }
        }
        fs::write(profile.metadata_file(), text).map_err(|_| ProfileError::Io)
    }
}

impl Profile {
    /// The default profile, rooted at `%LOCALAPPDATA%\Bowser` (`app_data`).
    pub fn default_in(app_data: &Path) -> Self {
        Self {
            name: None,
            root: app_data.to_path_buf(),
            guest: false,
        }
    }

    /// A named profile under `<app_data>\Profiles\<name>`. Validates the name; does not create it.
    pub fn named(app_data: &Path, name: &str) -> Result<Self, ProfileError> {
        validate_name(name)?;
        Ok(Self {
            name: Some(name.to_owned()),
            root: profiles_dir(app_data).join(name),
            guest: false,
        })
    }

    /// Creates a fresh guest directory under the per-user LocalAppData tree. Windows applies the
    /// parent ACL on creation; guest data never goes into a shared system temporary directory.
    pub fn guest_in(app_data: &Path) -> Result<Self, ProfileError> {
        let guests = app_data.join(GUEST_DIR_NAME);
        fs::create_dir_all(&guests).map_err(|_| ProfileError::Io)?;
        if !is_plain_dir(&guests) {
            return Err(ProfileError::OutsideProfiles);
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for attempt in 0..8u8 {
            let root = guests.join(format!("{}-{nonce}-{attempt}", std::process::id()));
            match fs::create_dir(&root) {
                Ok(()) => {
                    return Ok(Self {
                        name: None,
                        root,
                        guest: true,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(ProfileError::Io),
            }
        }
        Err(ProfileError::Io)
    }

    /// Holds an exclusive marker for the lifetime of a guest window.
    pub fn lock_guest(&self) -> Result<std::fs::File, ProfileError> {
        if !self.is_guest() {
            return Err(ProfileError::Io);
        }
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(0)
            .open(self.root.join(LOCK_FILE_NAME))
            .map_err(|_| ProfileError::Io)
    }

    /// The profile for a listed name: `DEFAULT_NAME` maps to the default profile.
    pub fn for_listed_name(app_data: &Path, name: &str) -> Result<Self, ProfileError> {
        if name == DEFAULT_NAME {
            Ok(Self::default_in(app_data))
        } else {
            Self::named(app_data, name)
        }
    }

    /// `None` for the default profile.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Per-profile local productivity files. These remain plain text so users can back them up
    /// or edit them directly; they are separate for every named profile.
    pub fn notes_file(&self) -> PathBuf {
        self.root.join("notes.txt")
    }
    pub fn tasks_file(&self) -> PathBuf {
        self.root.join("tasks.txt")
    }
    pub fn saved_sessions_file(&self) -> PathBuf {
        self.root.join("sessions.txt")
    }
    /// Local history (`unix_ts<TAB>url<TAB>title`) and bookmarks (`title<TAB>url<TAB>folder`),
    /// one UTF-8 entry per line; the importer writes the same formats.
    pub fn history_file(&self) -> PathBuf {
        self.root.join("history.txt")
    }
    pub fn bookmarks_file(&self) -> PathBuf {
        self.root.join("bookmarks.txt")
    }

    /// Whether the first-run migration offer has not been answered for this profile.
    pub fn import_offer_pending(&self) -> bool {
        !self.is_guest() && !self.root.join(IMPORT_OFFER_FILE_NAME).exists()
    }

    /// Stores any first-run offer choice inside this profile. The marker contains no user data.
    pub fn remember_import_offer_choice(&self) -> io::Result<()> {
        if self.is_guest() { return Ok(()); }
        fs::write(self.root.join(IMPORT_OFFER_FILE_NAME), "answered\n")
    }
    pub fn history_enabled_file(&self) -> PathBuf {
        self.root.join("history-enabled.txt")
    }

    /// History is opt-out; absent or malformed preference keeps the default enabled.
    pub fn history_enabled(&self) -> bool {
        fs::read_to_string(self.history_enabled_file())
            .ok()
            .is_none_or(|value| value.trim() != "0")
    }

    pub fn set_history_enabled(&self, enabled: bool) -> io::Result<()> {
        fs::write(self.history_enabled_file(), if enabled { "1\n" } else { "0\n" })
    }

    pub fn is_default(&self) -> bool {
        self.name.is_none() && !self.guest
    }

    pub fn is_guest(&self) -> bool {
        self.guest
    }

    /// The listed name: `default` or the profile name.
    pub fn listed_name(&self) -> &str {
        self.name
            .as_deref()
            .unwrap_or(if self.guest { "Guest" } else { DEFAULT_NAME })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// WebView2 user-data folder (cookies, storage, cache). Never shared between profiles.
    pub fn webview2_dir(&self) -> PathBuf {
        self.root.join(WEBVIEW2_DIR_NAME)
    }

    /// Crash-recovery session files and the running marker.
    pub fn session_dir(&self) -> PathBuf {
        self.root.clone()
    }

    /// Directory holding `blocker-sites.txt` and the optional `filters.txt`.
    pub fn blocker_dir(&self) -> PathBuf {
        self.root.clone()
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join(SETTINGS_FILE_NAME)
    }

    pub fn metadata_file(&self) -> PathBuf {
        self.root.join(METADATA_FILE_NAME)
    }

    pub fn tab_groups_file(&self) -> PathBuf {
        self.root.join(TAB_GROUPS_FILE_NAME)
    }

    /// Per-profile workspace membership and active workspace.
    pub fn workspaces_file(&self) -> PathBuf {
        self.root.join(crate::workspaces::FILE_NAME)
    }

    pub fn search_provider_file(&self) -> PathBuf {
        self.root.join(SEARCH_PROVIDER_FILE_NAME)
    }

    pub fn passwords_file(&self) -> PathBuf {
        self.root.join(PASSWORDS_FILE_NAME)
    }

    /// Host-only request counters for the Privacy dashboard.
    pub fn privacy_stats_file(&self) -> PathBuf {
        self.root.join(crate::privacy_stats::FILE_NAME)
    }

    /// Per-site camera, microphone, location and notification decisions.
    pub fn permissions_file(&self) -> PathBuf {
        self.root.join(crate::permissions::FILE_NAME)
    }

    /// Copies of unpacked browser extensions installed into this profile, one folder per id.
    pub fn extensions_dir(&self) -> PathBuf {
        self.root.join(EXTENSIONS_DIR_NAME)
    }

    /// The installed-extension list (`id`, enabled flag, name, version), one line each.
    pub fn extensions_file(&self) -> PathBuf {
        self.root.join(EXTENSIONS_FILE_NAME)
    }

    /// Suffix for the window title; empty for the default profile.
    pub fn title_suffix(&self) -> String {
        if self.guest {
            return " [Guest]".to_owned();
        }
        match &self.name {
            Some(name) => {
                let metadata = ProfileMetadata::load(self);
                format!(" [{} {name}]", metadata.avatar_glyph(name))
            }
            None => String::new(),
        }
    }
}

/// `%LOCALAPPDATA%\Bowser`, or `None` when LocalAppData is unset.
pub fn app_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join(APP_DIR_NAME))
}

/// Records the profile this process runs with. Only the first call has an effect.
pub fn set_active(profile: Profile) {
    let _ = ACTIVE.set(profile);
}

/// The profile this process runs with; the default profile when none was set.
pub fn active() -> Result<Profile, ProfileError> {
    if let Some(profile) = ACTIVE.get() {
        return Ok(profile.clone());
    }
    let app_data = app_data_dir().ok_or(ProfileError::NoLocalAppData)?;
    Ok(Profile::default_in(&app_data))
}

pub fn profiles_dir(app_data: &Path) -> PathBuf {
    app_data.join(PROFILES_DIR_NAME)
}

/// Checks a profile name: 1 to 32 ASCII letters, digits, `-` or `_`, and not a reserved name.
/// Because dots, slashes, colons and spaces are rejected, a valid name is always one plain path
/// component (no `..`, no absolute or drive-relative paths, no trailing dot or space).
pub fn validate_name(name: &str) -> Result<(), ProfileError> {
    let well_formed = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !well_formed {
        return Err(ProfileError::InvalidName);
    }
    if name.eq_ignore_ascii_case(DEFAULT_NAME) {
        return Err(ProfileError::ReservedName);
    }
    if RESERVED_NAMES
        .iter()
        .any(|reserved| name.eq_ignore_ascii_case(reserved))
    {
        return Err(ProfileError::InvalidName);
    }
    Ok(())
}

/// The listed profiles: `default` first, then every valid, plain subfolder of `Profiles`
/// sorted case-insensitively. Links, junctions, files and invalid names are skipped.
pub fn list(app_data: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let profiles = profiles_dir(app_data);
    if is_plain_dir(&profiles) {
        if let Ok(entries) = fs::read_dir(&profiles) {
            for entry in entries.flatten() {
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if validate_name(&name).is_ok() && is_plain_dir(&entry.path()) {
                    names.push(name);
                }
            }
        }
    }
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.insert(0, DEFAULT_NAME.to_owned());
    names
}

/// Creates an empty named profile folder. Fails when the name is taken (case-insensitively).
pub fn create_named(app_data: &Path, name: &str) -> Result<Profile, ProfileError> {
    let profile = Profile::named(app_data, name)?;
    ensure_profiles_dir(app_data)?;
    create_profile_dir(profile.root())?;
    ProfileMetadata::default().save(&profile)?;
    Ok(profile)
}

/// Name for the nth auto-created profile: `Profile-1`, `Profile-2`, ...
pub fn auto_name(n: u32) -> String {
    format!("Profile-{n}")
}

/// Creates the lowest-numbered free `Profile-N` folder. The create is atomic, so two
/// concurrent requests cannot pick the same name.
pub fn create_auto(app_data: &Path) -> Result<Profile, ProfileError> {
    ensure_profiles_dir(app_data)?;
    for n in 1..=MAX_AUTO_NAME {
        let name = auto_name(n);
        let profile = Profile::named(app_data, &name)?;
        match fs::create_dir(profile.root()) {
            Ok(()) => {
                ProfileMetadata::default().save(&profile)?;
                return Ok(profile);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(ProfileError::Io),
        }
    }
    Err(ProfileError::AlreadyExists)
}

/// Deletes a named profile folder. Refuses the default profile, the running profile, a profile
/// another window has open, and any target that is not a plain folder directly inside `Profiles`.
/// The caller must have obtained the user's confirmation first.
pub fn delete(app_data: &Path, name: &str, current: &Profile) -> Result<(), ProfileError> {
    validate_name(name)?;
    if current
        .name()
        .is_some_and(|running| running.eq_ignore_ascii_case(name))
    {
        return Err(ProfileError::InUse);
    }
    let profiles = profiles_dir(app_data);
    if !is_plain_dir(&profiles) {
        return Err(ProfileError::NotFound);
    }
    let target = profiles.join(name);
    if !is_plain_dir(&target) {
        return Err(ProfileError::NotFound);
    }
    let canonical_profiles = fs::canonicalize(&profiles).map_err(|_| ProfileError::Io)?;
    let canonical_target = fs::canonicalize(&target).map_err(|_| ProfileError::Io)?;
    if canonical_target.parent() != Some(canonical_profiles.as_path()) {
        return Err(ProfileError::OutsideProfiles);
    }
    if is_locked_by_running_session(&target) {
        return Err(ProfileError::InUse);
    }
    // Renaming fails while another process holds files open in the folder, so a profile that
    // is still in use is never half-deleted. The renamed folder is outside the listed names.
    let trash = profiles.join(format!("{TRASH_PREFIX}{name}-{}", std::process::id()));
    fs::rename(&target, &trash).map_err(|_| ProfileError::InUse)?;
    fs::remove_dir_all(&trash).map_err(|_| ProfileError::Io)
}

/// Renames a closed named profile by moving its directory within the validated Profiles folder.
pub fn rename(
    app_data: &Path,
    old_name: &str,
    new_name: &str,
    current: &Profile,
) -> Result<Profile, ProfileError> {
    validate_name(old_name)?;
    validate_name(new_name)?;
    if current
        .name()
        .is_some_and(|name| name.eq_ignore_ascii_case(old_name))
    {
        return Err(ProfileError::InUse);
    }
    let profiles = profiles_dir(app_data);
    if !is_plain_dir(&profiles) {
        return Err(ProfileError::NotFound);
    }
    if list(app_data)
        .iter()
        .any(|name| name.eq_ignore_ascii_case(new_name))
    {
        return Err(ProfileError::AlreadyExists);
    }
    let source = profiles.join(old_name);
    if !is_plain_dir(&source) {
        return Err(ProfileError::NotFound);
    }
    let canonical_profiles = fs::canonicalize(&profiles).map_err(|_| ProfileError::Io)?;
    let canonical_source = fs::canonicalize(&source).map_err(|_| ProfileError::Io)?;
    if canonical_source.parent() != Some(canonical_profiles.as_path()) {
        return Err(ProfileError::OutsideProfiles);
    }
    if is_locked_by_running_session(&source) {
        return Err(ProfileError::InUse);
    }
    let target = profiles.join(new_name);
    fs::rename(&source, &target).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            ProfileError::AlreadyExists
        } else {
            ProfileError::InUse
        }
    })?;
    let renamed = Profile::named(app_data, new_name)?;
    if fs::read_to_string(app_data.join(LAST_PROFILE_FILE_NAME))
        .is_ok_and(|last| last.trim().eq_ignore_ascii_case(old_name))
    {
        let _ = remember_last_profile(app_data, new_name);
    }
    Ok(renamed)
}

/// Remembers a profile only after it exists, so invalid or removed names cannot be persisted.
pub fn remember_last_profile(app_data: &Path, name: &str) -> Result<(), ProfileError> {
    fs::create_dir_all(app_data).map_err(|_| ProfileError::Io)?;
    if !is_plain_dir(app_data) {
        return Err(ProfileError::OutsideProfiles);
    }
    if name.eq_ignore_ascii_case(DEFAULT_NAME) {
        fs::write(
            app_data.join(LAST_PROFILE_FILE_NAME),
            format!("{DEFAULT_NAME}\n"),
        )
        .map_err(|_| ProfileError::Io)?;
        return Ok(());
    }
    validate_name(name)?;
    let profile = Profile::named(app_data, name)?;
    if !is_plain_dir(profile.root()) {
        return Err(ProfileError::NotFound);
    }
    fs::write(app_data.join(LAST_PROFILE_FILE_NAME), format!("{name}\n"))
        .map_err(|_| ProfileError::Io)
}

/// Explicit flags always win. Persisted selection is limited to normal interactive launches;
/// benchmarks and automation continue using the historical default profile.
pub fn startup_profile(
    app_data: &Path,
    explicit: Option<&str>,
    interactive: bool,
) -> Result<Profile, ProfileError> {
    if let Some(name) = explicit {
        return Profile::for_listed_name(app_data, name);
    }
    if interactive {
        if let Ok(text) = fs::read_to_string(app_data.join(LAST_PROFILE_FILE_NAME)) {
            let name = text.trim();
            if name.eq_ignore_ascii_case(DEFAULT_NAME) {
                return Ok(Profile::default_in(app_data));
            }
            if validate_name(name).is_ok() {
                if let Ok(profile) = Profile::named(app_data, name) {
                    if is_plain_dir(profile.root()) {
                        return Ok(profile);
                    }
                }
            }
        }
    }
    Ok(Profile::default_in(app_data))
}

/// Removes guest directories left by prior crashes. A held session lock marks a live guest.
pub fn sweep_stale_guests(app_data: &Path) -> Result<(), ProfileError> {
    let guests = app_data.join(GUEST_DIR_NAME);
    if !guests.exists() {
        return Ok(());
    }
    if !is_plain_dir(&guests) {
        return Err(ProfileError::OutsideProfiles);
    }
    let canonical_guests = fs::canonicalize(&guests).map_err(|_| ProfileError::Io)?;
    let entries = fs::read_dir(&guests).map_err(|_| ProfileError::Io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_plain_dir(&path) {
            continue;
        }
        let canonical = fs::canonicalize(&path).map_err(|_| ProfileError::Io)?;
        if canonical.parent() != Some(canonical_guests.as_path()) {
            return Err(ProfileError::OutsideProfiles);
        }
        if !is_locked_by_running_session(&path) {
            fs::remove_dir_all(path).map_err(|_| ProfileError::Io)?;
        }
    }
    Ok(())
}

/// Deletes this process's guest directory after its window and WebView2 controller have closed.
pub fn cleanup_guest(profile: &Profile) -> Result<(), ProfileError> {
    if !profile.is_guest() {
        return Ok(());
    }
    let Some(guests) = profile.root().parent() else {
        return Err(ProfileError::OutsideProfiles);
    };
    if !is_plain_dir(guests) || !is_plain_dir(profile.root()) {
        return Err(ProfileError::OutsideProfiles);
    }
    let canonical_guests = fs::canonicalize(guests).map_err(|_| ProfileError::Io)?;
    let canonical_root = fs::canonicalize(profile.root()).map_err(|_| ProfileError::Io)?;
    if canonical_root.parent() != Some(canonical_guests.as_path()) {
        return Err(ProfileError::OutsideProfiles);
    }
    fs::remove_dir_all(profile.root()).map_err(|_| ProfileError::Io)
}

/// Starts a new Bowser process for `profile`. The current window is left open.
pub fn spawn_window(profile: &Profile) -> bool {
    spawn_window_inner(profile, None)
}

/// Starts a new isolated guest window. The new process creates its own unique data directory.
pub fn spawn_guest_window() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    std::process::Command::new(exe)
        .arg("--guest")
        .spawn()
        .is_ok()
}

/// Starts a new Bowser process for `profile` and opens `url` in its initial tab.
pub fn spawn_window_with_url(profile: &Profile, url: &url::Url) -> bool {
    spawn_window_inner(profile, Some(url))
}

fn spawn_window_inner(profile: &Profile, url: Option<&url::Url>) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut command = std::process::Command::new(exe);
    if let Some(name) = profile.name() {
        command.arg("--profile").arg(name);
    }
    if let Some(url) = url {
        command.arg("--open-url").arg(url.as_str());
    }
    command.spawn().is_ok()
}

fn ensure_profiles_dir(app_data: &Path) -> Result<(), ProfileError> {
    let profiles = profiles_dir(app_data);
    fs::create_dir_all(&profiles).map_err(|_| ProfileError::Io)?;
    if is_plain_dir(&profiles) {
        Ok(())
    } else {
        Err(ProfileError::OutsideProfiles)
    }
}

fn create_profile_dir(root: &Path) -> Result<(), ProfileError> {
    match fs::create_dir(root) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            Err(ProfileError::AlreadyExists)
        }
        Err(_) => Err(ProfileError::Io),
    }
}

/// A directory that is not a symlink, junction or other reparse point.
fn is_plain_dir(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            meta.is_dir()
                && !meta.file_type().is_symlink()
                && meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
        }
        Err(_) => false,
    }
}

/// True when the profile's session lock is held open by a live Bowser (the session file is held
/// with no sharing). A stale lock left by a crash can be opened and does not block deletion.
fn is_locked_by_running_session(profile_root: &Path) -> bool {
    let lock = profile_root.join(LOCK_FILE_NAME);
    if !lock.exists() {
        return false;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&lock)
        .is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bowser-profile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn valid_names_are_accepted() {
        let longest = "n".repeat(32);
        for name in ["Work", "work_2", "a-b", "x", longest.as_str()] {
            assert!(validate_name(name).is_ok(), "{name} should be valid");
        }
    }

    #[test]
    fn malformed_and_traversal_names_are_rejected() {
        let long = "n".repeat(33);
        let bad = [
            "",
            ".",
            "..",
            "../x",
            "x/..",
            "a/b",
            "a\\b",
            "/abs",
            "\\abs",
            "C:evil",
            "C:\\evil",
            "\\\\server\\share",
            "trailing.",
            "trailing ",
            " leading",
            "has space",
            "dot.name",
            "émoji",
            "tab\tname",
            "nul\0",
            long.as_str(),
        ];
        for name in bad {
            assert_eq!(
                validate_name(name),
                Err(ProfileError::InvalidName),
                "{name:?}"
            );
        }
    }

    #[test]
    fn device_names_and_default_are_rejected_in_any_case() {
        for name in [
            "CON", "con", "Nul", "PRN", "AUX", "COM1", "com9", "LPT1", "lpt9",
        ] {
            assert_eq!(
                validate_name(name),
                Err(ProfileError::InvalidName),
                "{name}"
            );
        }
        for name in ["default", "Default", "DEFAULT"] {
            assert_eq!(
                validate_name(name),
                Err(ProfileError::ReservedName),
                "{name}"
            );
        }
    }

    #[test]
    fn default_profile_paths_are_unchanged() {
        let app = Path::new(r"C:\Users\u\AppData\Local\Bowser");
        let profile = Profile::default_in(app);
        assert!(profile.is_default());
        assert_eq!(profile.listed_name(), DEFAULT_NAME);
        assert_eq!(profile.root(), app);
        assert_eq!(profile.webview2_dir(), app.join("WebView2"));
        assert_eq!(profile.session_dir(), app);
        assert_eq!(profile.blocker_dir(), app);
        assert_eq!(profile.settings_file(), app.join("settings.txt"));
        assert_eq!(profile.tab_groups_file(), app.join("tab-groups.txt"));
        assert_eq!(
            profile.search_provider_file(),
            app.join("search-provider.txt")
        );
        assert_eq!(profile.passwords_file(), app.join("Passwords.dat"));
        assert_eq!(profile.bookmarks_file(), app.join("bookmarks.txt"));
        assert_eq!(profile.history_file(), app.join("history.txt"));
        assert_eq!(
            profile.privacy_stats_file(),
            app.join(crate::privacy_stats::FILE_NAME)
        );
        assert_eq!(profile.extensions_dir(), app.join("Extensions"));
        assert_eq!(profile.extensions_file(), app.join("extensions.txt"));
        assert_eq!(profile.title_suffix(), "");
    }

    #[test]
    fn named_profile_paths_live_under_profiles() {
        let app = Path::new(r"C:\data\Bowser");
        let profile = Profile::named(app, "Work").unwrap();
        let root = app.join("Profiles").join("Work");
        assert_eq!(profile.name(), Some("Work"));
        assert!(!profile.is_default());
        assert_eq!(profile.root(), root);
        assert_eq!(profile.webview2_dir(), root.join("WebView2"));
        assert_eq!(profile.session_dir(), root);
        assert_eq!(profile.passwords_file(), root.join("Passwords.dat"));
        assert_eq!(
            profile.privacy_stats_file(),
            root.join(crate::privacy_stats::FILE_NAME)
        );
        assert_eq!(profile.extensions_dir(), root.join("Extensions"));
        assert_eq!(profile.extensions_file(), root.join("extensions.txt"));
        assert_eq!(profile.settings_file(), root.join("settings.txt"));
        assert_eq!(profile.title_suffix(), " [W Work]");
        assert!(Profile::named(app, "../Work").is_err());
        assert_eq!(
            Profile::for_listed_name(app, "default").unwrap(),
            Profile::default_in(app)
        );
        assert_eq!(Profile::for_listed_name(app, "Work").unwrap(), profile);
    }

    #[test]
    fn listing_returns_default_then_valid_plain_folders() {
        let app = temp_root("list");
        assert_eq!(list(&app), vec![DEFAULT_NAME.to_owned()]);
        let profiles = app.join("Profiles");
        fs::create_dir_all(profiles.join("beta")).unwrap();
        fs::create_dir_all(profiles.join("Alpha")).unwrap();
        fs::create_dir_all(profiles.join("bad.name")).unwrap();
        fs::write(profiles.join("file-profile"), "x").unwrap();
        assert_eq!(list(&app), vec!["default", "Alpha", "beta"]);
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn auto_names_are_sequential_and_skip_taken_names() {
        assert_eq!(auto_name(1), "Profile-1");
        let app = temp_root("auto");
        let first = create_auto(&app).unwrap();
        assert_eq!(first.name(), Some("Profile-1"));
        fs::create_dir_all(app.join("Profiles").join("Profile-2")).unwrap();
        let third = create_auto(&app).unwrap();
        assert_eq!(third.name(), Some("Profile-3"));
        assert_eq!(
            create_named(&app, "profile-1"),
            Err(ProfileError::AlreadyExists)
        );
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn delete_removes_only_the_named_profile() {
        let app = temp_root("delete");
        let running = Profile::default_in(&app);
        let keep = create_named(&app, "Keep").unwrap();
        create_named(&app, "Gone").unwrap();
        fs::write(keep.root().join("marker.txt"), "keep").unwrap();

        assert_eq!(
            delete(&app, "default", &running),
            Err(ProfileError::ReservedName)
        );
        assert_eq!(delete(&app, "..", &running), Err(ProfileError::InvalidName));
        assert_eq!(
            delete(&app, "Missing", &running),
            Err(ProfileError::NotFound)
        );
        let gone = Profile::named(&app, "Gone").unwrap();
        assert_eq!(delete(&app, "Gone", &gone), Err(ProfileError::InUse));

        delete(&app, "Gone", &running).unwrap();
        assert!(!app.join("Profiles").join("Gone").exists());
        assert!(keep.root().join("marker.txt").exists());
        assert_eq!(list(&app), vec!["default", "Keep"]);
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn delete_refuses_a_stale_running_marker_held_open() {
        let app = temp_root("locked");
        let running = Profile::default_in(&app);
        let held = create_named(&app, "Held").unwrap();
        let lock = held.root().join(LOCK_FILE_NAME);
        let _guard = OpenOptions::new()
            .write(true)
            .create(true)
            .share_mode(0)
            .open(&lock)
            .unwrap();
        assert_eq!(delete(&app, "Held", &running), Err(ProfileError::InUse));
        assert!(held.root().exists());
        drop(_guard);
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn delete_never_follows_a_link_out_of_profiles() {
        let app = temp_root("escape");
        let outside = temp_root("escape-outside");
        fs::write(outside.join("precious.txt"), "keep").unwrap();
        fs::create_dir_all(app.join("Profiles")).unwrap();
        if std::os::windows::fs::symlink_dir(&outside, app.join("Profiles").join("Link")).is_err() {
            // Creating symlinks needs a privilege some accounts lack; the check is then moot.
            let _ = fs::remove_dir_all(&app);
            let _ = fs::remove_dir_all(&outside);
            return;
        }
        let running = Profile::default_in(&app);
        assert_eq!(delete(&app, "Link", &running), Err(ProfileError::NotFound));
        assert!(outside.join("precious.txt").exists());
        assert!(list(&app).iter().all(|name| name != "Link"));
        let _ = fs::remove_dir_all(&app);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn metadata_round_trip_preserves_unknown_keys() {
        let app = temp_root("metadata");
        let profile = Profile::default_in(&app);
        let metadata = ProfileMetadata::parse("avatar=leaf\naccent=violet\nfuture-key=kept\n");
        metadata.save(&profile).unwrap();
        let loaded = ProfileMetadata::load(&profile);
        assert_eq!(loaded.avatar, "leaf");
        assert_eq!(loaded.accent, "violet");
        assert_eq!(
            loaded.unknown.get("future-key").map(String::as_str),
            Some("kept")
        );
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn rename_rejects_current_and_locked_profiles() {
        let app = temp_root("rename-lock");
        let current = create_named(&app, "Current").unwrap();
        assert_eq!(
            rename(&app, "Current", "Renamed", &current),
            Err(ProfileError::InUse)
        );
        let held = create_named(&app, "Held").unwrap();
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .share_mode(0)
            .open(held.root().join(LOCK_FILE_NAME))
            .unwrap();
        assert_eq!(
            rename(&app, "Held", "Renamed", &Profile::default_in(&app)),
            Err(ProfileError::InUse)
        );
        assert!(held.root().exists());
        drop(lock);
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn rename_validates_target_and_moves_a_closed_profile() {
        let app = temp_root("rename");
        let running = Profile::default_in(&app);
        let source = create_named(&app, "Before").unwrap();
        fs::write(source.root().join("marker"), "preserved").unwrap();
        create_named(&app, "Taken").unwrap();
        assert_eq!(
            rename(&app, "Before", "Taken", &running),
            Err(ProfileError::AlreadyExists)
        );
        assert_eq!(
            rename(&app, "Before", "bad.name", &running),
            Err(ProfileError::InvalidName)
        );
        let result = rename(&app, "Before", "After", &running).unwrap();
        assert_eq!(result.name(), Some("After"));
        assert_eq!(
            fs::read_to_string(result.root().join("marker")).unwrap(),
            "preserved"
        );
        assert!(!source.root().exists());
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn rename_refuses_profile_links_outside_profiles() {
        let app = temp_root("rename-link");
        let outside = temp_root("rename-link-outside");
        fs::write(outside.join("marker"), "keep").unwrap();
        let profiles = app.join(PROFILES_DIR_NAME);
        fs::create_dir_all(&profiles).unwrap();
        if std::os::windows::fs::symlink_dir(&outside, profiles.join("Link")).is_err() {
            let _ = fs::remove_dir_all(&app);
            let _ = fs::remove_dir_all(&outside);
            return;
        }
        assert_eq!(
            rename(&app, "Link", "Moved", &Profile::default_in(&app)),
            Err(ProfileError::NotFound)
        );
        assert!(outside.join("marker").exists());
        let _ = fs::remove_dir_all(&app);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn guest_profile_is_unique_and_cleanup_removes_only_its_directory() {
        let app = temp_root("guest-cleanup");
        let first = Profile::guest_in(&app).unwrap();
        let second = Profile::guest_in(&app).unwrap();
        assert_ne!(first.root(), second.root());
        fs::write(first.root().join("guest-data"), "private").unwrap();
        cleanup_guest(&first).unwrap();
        assert!(!first.root().exists());
        assert!(second.root().exists());
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn guest_session_guard_cleans_up_after_early_exit() {
        let app = temp_root("guest-guard");
        let guest = Profile::guest_in(&app).unwrap();
        let root = guest.root().to_path_buf();
        {
            let _session = GuestSession::start(&guest).unwrap();
            assert!(root.exists());
            assert!(is_locked_by_running_session(&root));
        }
        assert!(!root.exists());
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn guest_sweep_removes_only_unlocked_guest_directories() {
        let app = temp_root("guest-sweep");
        let guest_root = app.join(GUEST_DIR_NAME);
        let stale = guest_root.join("stale");
        let live = guest_root.join("live");
        fs::create_dir_all(&stale).unwrap();
        fs::create_dir_all(&live).unwrap();
        fs::write(stale.join("data"), "private").unwrap();
        let _lock = OpenOptions::new()
            .write(true)
            .create(true)
            .share_mode(0)
            .open(live.join(LOCK_FILE_NAME))
            .unwrap();
        sweep_stale_guests(&app).unwrap();
        assert!(!stale.exists());
        assert!(live.exists());
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn last_profile_selection_falls_back_and_explicit_profile_wins() {
        let app = temp_root("last-profile");
        assert!(remember_last_profile(&app, "Work").is_err());
        let work = create_named(&app, "Work").unwrap();
        remember_last_profile(&app, "Work").unwrap();
        assert_eq!(startup_profile(&app, None, true).unwrap(), work);
        assert_eq!(
            startup_profile(&app, None, false).unwrap(),
            Profile::default_in(&app)
        );
        assert_eq!(
            startup_profile(&app, Some("default"), true).unwrap(),
            Profile::default_in(&app)
        );
        fs::remove_dir_all(work.root()).unwrap();
        assert_eq!(
            startup_profile(&app, None, true).unwrap(),
            Profile::default_in(&app)
        );
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn import_offer_choice_is_per_profile_and_guest_never_persists() {
        let app = temp_root("import-offer");
        let default = Profile::default_in(&app);
        assert!(default.import_offer_pending());
        default.remember_import_offer_choice().unwrap();
        assert!(!default.import_offer_pending());
        let named = create_named(&app, "Work").unwrap();
        assert!(named.import_offer_pending());
        let guest = Profile::guest_in(&app).unwrap();
        guest.remember_import_offer_choice().unwrap();
        assert!(!guest.root().join(IMPORT_OFFER_FILE_NAME).exists());
        assert!(default.root().join(IMPORT_OFFER_FILE_NAME).exists());
        let _ = fs::remove_dir_all(&app);
    }
}
