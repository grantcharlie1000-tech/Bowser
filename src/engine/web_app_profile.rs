//! Native WebApps profile retirement. This registry deliberately outlives panel factories/hosts.
//! Deletion is only performed by an explicit clear call after callbacks AND native UDF release;
//! event callbacks never erase data, pump messages, borrow App, or repair focus.
use std::{cell::{Cell, RefCell}, collections::HashMap, path::{Path, PathBuf}, rc::Rc};
use webview2_com::{BrowserProcessExitedEventHandler, Microsoft::Web::WebView2::Win32::*};
use windows::core::{Error, Interface, Result};
use crate::web_app_lifecycle::ProfileRetirement;

type ProfileRef = Rc<RefCell<Profile>>;
#[derive(Default)]
struct Profile {
    retirement: ProfileRetirement,
    shutdown: bool,
    environments: HashMap<u64, Subscription>,
}
struct Subscription { environment: ICoreWebView2Environment5, token: i64 }
thread_local! {
    static SHUTDOWN: Cell<bool> = const { Cell::new(false) };
    static PROFILES: RefCell<HashMap<PathBuf, ProfileRef>> = RefCell::new(HashMap::new());
}
fn profile_key(path: &Path) -> std::io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| std::io::Error::other("missing profile parent"))?;
    if !path.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("WebApps")) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Expected isolated WebApps directory"));
    }
    // The installed profile root already exists. Canonicalize ONLY that parent, not a possibly
    // absent/newly erased WebApps directory; create, retry and clear therefore use one stable key.
    Ok(std::fs::canonicalize(parent)?.join("WebApps"))
}
fn profile(path: &Path) -> std::io::Result<ProfileRef> {
    let key = profile_key(path)?;
    Ok(PROFILES.with(|profiles| profiles.borrow_mut().entry(key).or_default().clone()))
}

fn blocked() -> Error {
    Error::new(windows::core::HRESULT(0x800700AAu32 as i32),
        "Web app data is still closing. Retry clearing it before opening another web app.")
}
#[derive(Clone)]
pub(super) struct NativeRequest {
    profile: ProfileRef,
    id: u64,
    environment_observed: Rc<Cell<bool>>,
    dispatched: Rc<Cell<bool>>,
}
impl NativeRequest {
    fn start(profile: ProfileRef, environment: Option<u64>) -> Result<Self> {
        let id = profile.borrow_mut().retirement.request(environment).ok_or_else(blocked)?;
        Ok(Self { profile, id, environment_observed: Rc::new(Cell::new(environment.is_some())), dispatched: Rc::new(Cell::new(false)) })
    }
    pub(super) fn environment(path: &Path) -> Result<Self> {
        if SHUTDOWN.with(Cell::get) { return Err(blocked()); }
        sweep();
        let profile = profile(path).map_err(|error| Error::new(windows::core::HRESULT(0x80070003u32 as i32), error.to_string()))?;
        Self::start(profile, None)
    }
    pub(super) fn preparing(&self) -> RequestPreparation { RequestPreparation(self.clone()) }
    pub(super) fn dispatch(&self) { self.dispatched.set(true); }
    pub(super) fn completing(&self) -> CompletionRetirement { CompletionRetirement(self.clone()) }
    pub(super) fn finish(&self) {
        self.profile.borrow_mut().retirement.finish(self.id, self.environment_observed.get() || !self.dispatched.get());
    }
    /// Register even an environment accompanying an error: it may still own native resources.
    pub(super) fn observe_environment(&self, environment: &ICoreWebView2Environment) -> Option<EnvironmentLease> {
        let id = self.profile.borrow_mut().retirement.environment()?;
        self.environment_observed.set(true);
        self.profile.borrow_mut().retirement.attach_environment(self.id, id);
        let weak = Rc::downgrade(&self.profile);
        let observed = (|| -> Result<Subscription> {
            if self.profile.borrow().shutdown { return Err(blocked()); }
            let environment: ICoreWebView2Environment5 = environment.cast()?;
            let handler = BrowserProcessExitedEventHandler::create(Box::new(move |_sender, args| {
                if let Some(profile) = weak.upgrade() {
                    let mut pid = 0;
                    let pid = args.and_then(|args| unsafe { args.BrowserProcessId(&mut pid) }.ok().map(|()| pid));
                    profile.borrow_mut().retirement.released(id, pid);
                }
                Ok(())
            }));
            let mut token = 0;
            unsafe { environment.add_BrowserProcessExited(&handler, &mut token)?; }
            Ok(Subscription { environment, token })
        })();
        let mut profile = self.profile.borrow_mut();
        profile.retirement.registration(id, observed.is_ok());
        if let Ok(subscription) = observed { profile.environments.insert(id, subscription); }
        Some(EnvironmentLease { owner: Rc::new(EnvironmentOwner { profile: self.profile.clone(), id }) })
    }
}
pub(super) struct RequestPreparation(NativeRequest);
impl Drop for RequestPreparation {
    fn drop(&mut self) { if !self.0.dispatched.get() { self.0.finish(); } }
}
pub(super) struct CompletionRetirement(NativeRequest);
impl Drop for CompletionRetirement { fn drop(&mut self) { self.0.finish(); } }

struct EnvironmentOwner { profile: ProfileRef, id: u64 }
impl Drop for EnvironmentOwner {
    fn drop(&mut self) { self.profile.borrow_mut().retirement.retire_environment(self.id); }
}
#[derive(Clone)]
pub(super) struct EnvironmentLease { owner: Rc<EnvironmentOwner> }
impl EnvironmentLease {
    pub(super) fn request(&self) -> Result<NativeRequest> { NativeRequest::start(self.owner.profile.clone(), Some(self.owner.id)) }
    pub(super) fn is_clearing(&self) -> bool { self.owner.profile.borrow().retirement.is_clearing() }
    pub(super) fn observe_controller(&self, request: &NativeRequest, controller: &ICoreWebView2Controller) {
        let pid = (|| -> Result<u32> {
            let webview = unsafe { controller.CoreWebView2()? };
            let mut pid = 0;
            unsafe { webview.BrowserProcessId(&mut pid)?; }
            Ok(pid)
        })().ok();
        self.owner.profile.borrow_mut().retirement.browser_pid(self.owner.id, request.id, pid);
    }
}

/// Existing create/clear/shutdown boundaries reap retired observers; no new timer or COM work in
/// BrowserProcessExited callbacks. An unknown retired failure leaves a sticky clear-data error.
pub(super) fn sweep() {
    let profiles = PROFILES.with(|profiles| profiles.borrow().values().cloned().collect::<Vec<_>>());
    for profile in profiles {
        let subscriptions = {
            let mut state = profile.borrow_mut();
            state.retirement.reapable().into_iter().filter_map(|id| state.environments.remove(&id)).collect::<Vec<_>>()
        };
        for subscription in subscriptions {
            if unsafe { subscription.environment.remove_BrowserProcessExited(subscription.token) }.is_err() {
                profile.borrow_mut().retirement.uncertain();
            }
        }
    }
}
pub(super) fn shutdown() {
    SHUTDOWN.with(|shutdown| shutdown.set(true));
    let profiles = PROFILES.with(|profiles| profiles.borrow().values().cloned().collect::<Vec<_>>());
    for profile in profiles {
        let subscriptions = {
            let mut state = profile.borrow_mut();
            state.shutdown = true;
            state.retirement.begin_clear();
            std::mem::take(&mut state.environments)
        };
        for (_, subscription) in subscriptions {
            let _ = unsafe { subscription.environment.remove_BrowserProcessExited(subscription.token) };
        }
    }
}

pub(super) fn begin_clear(path: &Path) -> std::io::Result<()> {
    profile(path)?.borrow_mut().retirement.begin_clear();
    Ok(())
}
/// No UI-thread wait or deferred erase: the user must explicitly retry this operation.
pub(super) fn finish_clear(path: &Path) -> std::io::Result<()> {
    sweep();
    let profile = profile(path)?;
    if !profile.borrow().retirement.can_erase() {
        return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock,
            "Web app data is still in use or release is unconfirmed. Retry after it closes; unknown release may require restarting Bowser."));
    }
    // Remove subscriptions and release COM references outside all registry/state borrows. Retaining
    // the environment until its event is necessary for WebView2 to deliver BrowserProcessExited.
    let subscriptions = std::mem::take(&mut profile.borrow_mut().environments);
    let failed = crate::web_app_lifecycle::detach(subscriptions, |subscription| unsafe {
        subscription.environment.remove_BrowserProcessExited(subscription.token)
    });
    if !failed.is_empty() {
        profile.borrow_mut().environments.extend(failed);
        return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock,
            "Web app native cleanup is incomplete. Retry clearing; a restart may be required."));
    }
    // Unsubscription/destruction may reenter. No new request can pass the clear barrier, but
    // malformed/unknown release evidence must still block deletion after that COM boundary.
    if !profile.borrow().retirement.can_erase() {
        return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock,
            "Web app release evidence changed during cleanup. Retry clearing; a restart may be required."));
    }
    match std::fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    profile.borrow_mut().retirement.erased();
    Ok(())
}

#[cfg(test)]
pub(super) fn clear_ready_for_test(path: &Path) -> bool {
    profile(path).is_ok_and(|profile| profile.borrow().retirement.can_erase())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("bowser-webapp-retirement-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap(); root
    }
    #[test]
    fn profile_key_is_stable_before_creation_after_creation_and_after_erase() {
        let root = root(); let dir = root.join("WebApps"); let before = profile_key(&dir).unwrap();
        std::fs::create_dir(&dir).unwrap(); assert_eq!(profile_key(&dir).unwrap(), before);
        std::fs::remove_dir(&dir).unwrap(); assert_eq!(profile_key(&dir).unwrap(), before);
        assert_eq!(profile_key(&root.join("webapps")).unwrap(), before);
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn missing_directory_is_not_clear_success_while_native_request_is_pending() {
        let root = root(); let dir = root.join("WebApps");
        let request = NativeRequest::environment(&dir).unwrap();
        begin_clear(&dir).unwrap();
        assert_eq!(finish_clear(&dir).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        assert!(NativeRequest::environment(&dir).is_err());
        request.finish(); // Explicitly never dispatched, so no native resource is being inferred.
        finish_clear(&dir).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn filesystem_error_retains_barrier_until_an_explicit_successful_retry() {
        let root = root(); let dir = root.join("WebApps"); std::fs::write(&dir, b"not a directory").unwrap();
        begin_clear(&dir).unwrap(); assert!(finish_clear(&dir).is_err());
        assert!(NativeRequest::environment(&dir).is_err());
        std::fs::remove_file(&dir).unwrap(); finish_clear(&dir).unwrap();
        let request = NativeRequest::environment(&dir).unwrap(); request.finish();
        std::fs::remove_dir(root).unwrap();
    }
}
