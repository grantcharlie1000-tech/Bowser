//! Per-profile data locations: the single place that decides where Bowser keeps its files.
//!
//! The default profile uses `%LOCALAPPDATA%\Bowser` exactly as before. A named profile
//! (`--profile <name>`) lives in `%LOCALAPPDATA%\Bowser\Profiles\<name>\` and owns its own
//! WebView2 user-data folder, session, tab groups, settings, blocker sites and password vault,
//! so profiles share no browser data. The app-wide updater files stay under `%LOCALAPPDATA%\Bowser`.

use std::{
    fs::{self, OpenOptions},
    io,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::OnceLock,
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
}

impl Profile {
    /// The default profile, rooted at `%LOCALAPPDATA%\Bowser` (`app_data`).
    pub fn default_in(app_data: &Path) -> Self {
        Self {
            name: None,
            root: app_data.to_path_buf(),
        }
    }

    /// A named profile under `<app_data>\Profiles\<name>`. Validates the name; does not create it.
    pub fn named(app_data: &Path, name: &str) -> Result<Self, ProfileError> {
        validate_name(name)?;
        Ok(Self {
            name: Some(name.to_owned()),
            root: profiles_dir(app_data).join(name),
        })
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

    pub fn is_default(&self) -> bool {
        self.name.is_none()
    }

    /// The listed name: `default` or the profile name.
    pub fn listed_name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_NAME)
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

    pub fn tab_groups_file(&self) -> PathBuf {
        self.root.join(TAB_GROUPS_FILE_NAME)
    }

    pub fn search_provider_file(&self) -> PathBuf {
        self.root.join(SEARCH_PROVIDER_FILE_NAME)
    }

    pub fn passwords_file(&self) -> PathBuf {
        self.root.join(PASSWORDS_FILE_NAME)
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
        match &self.name {
            Some(name) => format!(" [{name}]"),
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
            Ok(()) => return Ok(profile),
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

/// Starts a new Bowser process for `profile`. The current window is left open.
pub fn spawn_window(profile: &Profile) -> bool {
    spawn_window_inner(profile, None)
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
        assert_eq!(profile.extensions_dir(), root.join("Extensions"));
        assert_eq!(profile.extensions_file(), root.join("extensions.txt"));
        assert_eq!(profile.settings_file(), root.join("settings.txt"));
        assert_eq!(profile.title_suffix(), " [Work]");
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
}
