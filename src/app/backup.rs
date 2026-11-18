//! Back up and restore, the app's side (docs/design/backup.md). The file format, the cipher and the
//! staging of files are `crate::backup`; this module gathers what a backup holds from the running
//! profile (files, the password vault, the engine's cookies) and puts a restore back.

use std::{cell::Cell, cell::RefCell, path::Path, rc::Rc};

use super::{App, AppRef};
use crate::backup::{self, BackupPassword, CookieRecord, Entry, Summary};
use crate::passwords::PasswordStore;

type Done = Box<dyn FnOnce(Result<Summary, String>)>;

/// Cookies live in the browser engine, reached through an open page.
const NEED_PAGE: &str = "Open a web page in a tab first: Bowser reaches your cookies through it.";

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The largest backup file read for a restore.
fn read_backup_file(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|error| format!("The backup could not be opened: {error}."))?;
    let meta = file.metadata().map_err(|error| format!("The backup could not be read: {error}."))?;
    if !meta.is_file() || meta.len() > backup::MAX_FILE_BYTES as u64 {
        return Err(backup::BackupError::TooLarge.to_string());
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(backup::MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("The backup could not be read: {error}."))?;
    Ok(bytes)
}

impl App {
    /// Runs one engine cookie call with the answer held back until the call has returned, so `done`
    /// never runs with App borrowed (the engine may answer inside the call). `None` when no tab can
    /// reach the profile's cookies or the engine is busy: the caller goes on without cookies.
    fn backup_engine_call<T: 'static>(
        app: &AppRef,
        call: impl FnOnce(&mut dyn crate::engine::EngineAdapter, crate::tabs::TabId, Box<dyn FnOnce(Result<T, String>)>),
        done: Box<dyn FnOnce(Option<T>)>,
    ) {
        let Some(tab) = Self::profile_page_tab(app) else {
            done(None);
            return;
        };
        let finish = Rc::new(RefCell::new(Some(done)));
        let in_call = Rc::new(Cell::new(true));
        let early: Rc<RefCell<Option<Result<T, String>>>> = Rc::new(RefCell::new(None));
        let answer = {
            let (finish, in_call, early) = (finish.clone(), in_call.clone(), early.clone());
            Box::new(move |result: Result<T, String>| {
                if in_call.get() {
                    *early.borrow_mut() = Some(result);
                } else if let Some(finish) = finish.borrow_mut().take() {
                    finish(result.ok());
                }
            })
        };
        let started = Self::call_engine(app, |engine| call(engine, tab, answer));
        in_call.set(false);
        let waiting = early.borrow_mut().take();
        if started.is_none() || waiting.is_some() {
            if let Some(finish) = finish.borrow_mut().take() {
                finish(waiting.and_then(Result::ok));
            }
        }
    }

    /// Writes a backup of the active profile to `path`, encrypted with `password`. `done` runs once
    /// the cookies are in (later, on the UI thread). A guest window has nothing to keep.
    pub fn backup_export(app: &AppRef, path: std::path::PathBuf, password: BackupPassword, done: Done) {
        if !backup::password_ok(password.as_str()) {
            done(Err(format!("Use a password of at least {} characters.", backup::MIN_PASSWORD_CHARS)));
            return;
        }
        let profile = match crate::profile::active() {
            Ok(profile) if profile.is_guest() => {
                done(Err("A guest window keeps nothing to back up.".to_owned()));
                return;
            }
            Ok(profile) => profile,
            Err(error) => {
                done(Err(format!("The profile could not be found: {error}")));
                return;
            }
        };
        if Self::profile_page_tab(app).is_none() {
            done(Err(NEED_PAGE.to_owned()));
            return;
        }
        // What is still in memory goes to disk first, so the backup is current.
        let _ = Self::try_save_settings(app);
        crate::productivity::flush_pending_history();
        let root = profile.root().to_path_buf();
        Self::backup_engine_call(
            app,
            |engine, tab, answer| engine.export_cookies(tab, answer),
            Box::new(move |cookies: Option<Vec<CookieRecord>>| match cookies {
                Some(cookies) => done(write_backup(&root, &path, &password, cookies)),
                None => done(Err("The browser did not hand over the cookies, so no backup was saved. Try again.".to_owned())),
            }),
        );
    }

    /// Restores a backup from `path`: its cookies, then its passwords, are added now, and its files are
    /// staged to replace the profile's when Bowser restarts (the caller restarts on success). If the
    /// cookies cannot be set, nothing else is changed.
    pub fn backup_import(app: &AppRef, path: &Path, password: BackupPassword, done: Done) {
        let profile = match crate::profile::active() {
            Ok(profile) if profile.is_guest() => {
                done(Err("A guest window cannot keep a restored profile. Open your own profile first.".to_owned()));
                return;
            }
            Ok(profile) => profile,
            Err(error) => {
                done(Err(format!("The profile could not be found: {error}")));
                return;
            }
        };
        let entries = match read_backup_file(path).and_then(|bytes| backup::open(password.as_str(), &bytes).map_err(|e| e.to_string())) {
            Ok(entries) => entries,
            Err(error) => {
                done(Err(error));
                return;
            }
        };
        let now = now_secs() as f64;
        let cookies: Vec<CookieRecord> = entries
            .iter()
            .find(|e| e.name == backup::COOKIES_ENTRY)
            .map(|e| backup::decode_cookies(&e.data))
            .unwrap_or_default()
            .into_iter()
            .filter(|cookie| backup::cookie_alive(cookie, now))
            .collect();
        // Checked before anything changes: a restore without its cookies would leave every site signed out.
        if !cookies.is_empty() && Self::profile_page_tab(app).is_none() {
            done(Err(NEED_PAGE.to_owned()));
            return;
        }
        // Cookies first: if the engine refuses them, nothing else has changed yet.
        let root = profile.root().to_path_buf();
        if cookies.is_empty() {
            done(restore_rest(&root, &entries, 0));
            return;
        }
        Self::backup_engine_call(
            app,
            move |engine, tab, answer| engine.import_cookies(tab, cookies, answer),
            Box::new(move |set: Option<usize>| match set {
                Some(set) => done(restore_rest(&root, &entries, set)),
                None => done(Err("The cookies could not be restored, so nothing was changed. Try again.".to_owned())),
            }),
        );
    }
}

/// After the cookies: merges the passwords and stages the files for the restart.
fn restore_rest(root: &Path, entries: &[Entry], cookies_set: usize) -> Result<Summary, String> {
    let mut summary = Summary { cookies: Some(cookies_set), ..Summary::default() };
    // Passwords: only logins the vault does not have yet are added; nothing is overwritten.
    if let Some(csv) = entries.iter().find(|e| e.name == backup::PASSWORDS_ENTRY) {
        summary.passwords = PasswordStore::open_default()
            .and_then(|mut store| {
                let plan = store.preview_csv(&csv.data)?;
                store.commit(plan)
            })
            .map_err(|error| format!("The saved passwords could not be restored: {error}"))?;
    }
    summary.files = backup::stage_restore(root, entries, now_secs())
        .map_err(|error| format!("The restore could not be prepared: {error}"))?;
    Ok(summary)
}

/// Gathers the profile's files and passwords, adds `cookies`, seals and writes the file.
fn write_backup(
    root: &Path,
    path: &Path,
    password: &BackupPassword,
    cookies: Vec<CookieRecord>,
) -> Result<Summary, String> {
    let mut entries = backup::collect_files(root).map_err(|error| error.to_string())?;
    let mut summary = Summary { files: entries.len(), ..Summary::default() };
    match PasswordStore::open_default() {
        Ok(store) => {
            summary.passwords = store.list().len();
            if summary.passwords > 0 {
                let csv = store.export_csv();
                entries.push(Entry::new(backup::PASSWORDS_ENTRY, csv.as_bytes().to_vec()));
            }
        }
        Err(crate::passwords::PasswordError::Disabled) => {}
        Err(error) => return Err(format!("The saved passwords could not be read: {error}")),
    }
    summary.cookies = Some(cookies.len());
    entries.push(Entry::new(backup::COOKIES_ENTRY, backup::encode_cookies(&cookies).to_vec()));
    let file = backup::seal(password.as_str(), &entries).map_err(|error| error.to_string())?;
    crate::productivity::atomic_write(path, &file).map_err(|error| format!("The backup could not be saved: {error}."))?;
    Ok(summary)
}
