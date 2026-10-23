//! Test-only, one-challenge evidence for one retained disposable WebView2 environment.
//!
//! This is not UIA authority, a page/host endpoint, an input API, or a general RPC server.
//! The helper must independently establish the exact owned popup and the complete bounded
//! inventory. Runtime IDs and the inventory digest below are challenge-bound echoes only.
//! Keep `GpuMenuAuthority` alive until the helper child has exited: its process handles must
//! not be dropped after the response is written. Failure to construct it leaves the original
//! browser-only strict ownership path available; it never substitutes a made-up GPU identity.

#![cfg(test)]

use std::{
    cell::Cell,
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::windows::{ffi::OsStrExt, fs::{MetadataExt, OpenOptionsExt}},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use webview2_com::{
    CoTaskMemPWSTR,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2_2, ICoreWebView2Environment,
        ICoreWebView2Environment7, ICoreWebView2Environment8,
        COREWEBVIEW2_PROCESS_KIND, COREWEBVIEW2_PROCESS_KIND_BROWSER,
        COREWEBVIEW2_PROCESS_KIND_GPU,
    },
};
use windows::{
    core::{Interface, IUnknown, PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, HWND, WAIT_TIMEOUT},
        Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG},
        Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS},
        System::Threading::{
            GetProcessId, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
            WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE,
        },
        System::SystemInformation::GetTickCount64,
        UI::{
            Input::KeyboardAndMouse::IsWindowEnabled,
            WindowsAndMessaging::{
                GetAncestor, GetClassNameW, GetParent, GetWindow, GetWindowThreadProcessId,
                IsWindow, IsWindowVisible, GA_ROOT, GA_ROOTOWNER, GW_OWNER,
            },
        },
    },
};

use crate::extensions::json::{self, Value};

const MAX_BYTES: usize = 8192;
const MAX_PROCESSES: usize = 64;
const MAX_WIDE_PATH: usize = 32768;
const REQUEST: &str = "gpu-membership-request.json";
const RESPONSE: &str = "gpu-membership-response.json";
const PENDING: &str = "gpu-membership-response.pending";
const REQUEST_SCHEMA: &str = "bowser-gpu-menu-membership-request-v1";
const RESPONSE_SCHEMA: &str = "bowser-gpu-menu-membership-response-v1";
// File operations must not follow a request reparse point or allow concurrent writers.
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

fn error(stage: &str, value: windows::core::Error) -> String {
    format!("{stage}-hresult-{}", value.code().0)
}

fn deadline_and_fence(deadline: Instant, fence: &dyn Fn() -> bool) -> Result<(), String> {
    if Instant::now() >= deadline { return Err("ownership-deadline-expired".into()); }
    if !fence() { return Err("original-source-document-fence-unavailable".into()); }
    // The fence can reenter user code; neither its success nor its runtime renews the budget.
    if Instant::now() >= deadline { return Err("ownership-deadline-expired".into()); }
    Ok(())
}

fn bootstrap_com<T>(
    deadline: Instant, fence: &dyn Fn() -> bool,
    read: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    deadline_and_fence(deadline, fence)?;
    let result = read();
    deadline_and_fence(deadline, fence)?;
    result
}

fn path_equal(left: &Path, right: &Path) -> bool {
    // Reject undecodable paths instead of comparing lossy replacement characters.
    left.to_str().zip(right.to_str()).is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err("ownership-artifact-already-exists".into()),
        Err(_) => Err("ownership-artifact-state-unavailable".into()),
    }
}

fn fresh_output_paths(out: &Path, include_request: bool) -> Result<(), String> {
    if include_request { absent(&out.join(REQUEST))?;absent(&out.join("gpu-membership-request.pending"))?; }
    absent(&out.join(PENDING))?;
    absent(&out.join(RESPONSE))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MembershipEntry { pid: i32, kind: i32 }

/// Pure bounded policy, not a native ownership witness. Only the real COM collection feeds it.
fn validate_membership(
    entries: &[MembershipEntry], browser_pid: u32, expected_gpu: Option<u32>,
) -> Result<u32, String> {
    if entries.is_empty() || entries.len() > MAX_PROCESSES || browser_pid == 0 {
        return Err("membership-count-or-browser-outside-bound".into());
    }
    let mut seen = BTreeSet::new();
    let (mut browsers, mut gpus, mut gpu_pid) = (0usize, 0usize, 0u32);
    for entry in entries {
        // Current documented WebView2 process kinds are 0..=6. New/unknown kinds fail closed.
        if entry.pid <= 0 || !seen.insert(entry.pid) || !(0..=6).contains(&entry.kind) {
            return Err("membership-invalid-duplicate-or-unknown-entry".into());
        }
        if entry.kind == COREWEBVIEW2_PROCESS_KIND_BROWSER.0 {
            if entry.pid as u32 != browser_pid { return Err("membership-browser-mismatch".into()); }
            browsers += 1;
        }
        if entry.kind == COREWEBVIEW2_PROCESS_KIND_GPU.0 {
            gpu_pid = entry.pid as u32;
            gpus += 1;
        }
    }
    if browsers != 1 || gpus != 1 || gpu_pid == browser_pid
        || expected_gpu.is_some_and(|expected| expected != gpu_pid)
    { return Err("membership-exact-browser-and-single-gpu-unavailable".into()); }
    Ok(gpu_pid)
}

struct ProcessWitness { handle: HANDLE, pid: u32, creation: u64, image: String }

impl Drop for ProcessWitness {
    fn drop(&mut self) { unsafe { let _ = CloseHandle(self.handle); } }
}

fn filetime(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

fn process_snapshot(handle: HANDLE, pid: u32) -> Result<(u64, String), String> {
    // Zero-time waits on retained SYNCHRONIZE handles bind the actual process lifetime,
    // not a PID lookup or GetExitCodeProcess's ambiguous STILL_ACTIVE exit code.
    if unsafe { WaitForSingleObject(handle, 0) } != WAIT_TIMEOUT
        || unsafe { GetProcessId(handle) } != pid
    { return Err("retained-process-not-alive-or-pid-mismatch".into()); }
    let (mut creation, mut exit, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) }
        .map_err(|e| error("retained-process-times", e))?;
    let mut buffer = vec![0u16; MAX_WIDE_PATH];
    let mut length = buffer.len() as u32;
    unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut length) }
        .map_err(|e| error("retained-process-image", e))?;
    let length = length as usize;
    if creation.dwLowDateTime == 0 && creation.dwHighDateTime == 0
        || filetime(exit) != 0 || length == 0 || length >= buffer.len()
        || buffer[..length].contains(&0)
    { return Err("retained-process-lifetime-or-image-unavailable".into()); }
    let image = String::from_utf16(&buffer[..length]).map_err(|_| "retained-process-image-invalid-utf16")?;
    if unsafe { WaitForSingleObject(handle, 0) } != WAIT_TIMEOUT {
        return Err("retained-process-exited-during-read".into());
    }
    Ok((filetime(creation), image))
}

impl ProcessWitness {
    fn open(pid: u32) -> Result<Self, String> {
        if pid == 0 { return Err("process-pid-zero".into()); }
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, pid) }
            .map_err(|e| error("open-retained-process", e))?;
        // Install ownership immediately so every following failure closes this real handle.
        let mut witness = Self { handle, pid, creation: 0, image: String::new() };
        let (creation, image) = process_snapshot(handle, pid)?;
        witness.creation = creation;
        witness.image = image;
        Ok(witness)
    }

    fn check(&self) -> Result<(), String> {
        let (creation, image) = process_snapshot(self.handle, self.pid)?;
        if creation != self.creation || !image.eq_ignore_ascii_case(&self.image) {
            return Err("retained-process-creation-or-image-changed".into());
        }
        Ok(())
    }
}

struct Source {
    view: ICoreWebView2,
    view2: ICoreWebView2_2,
    view_identity: IUnknown,
    environment: ICoreWebView2Environment,
    environment_identity: IUnknown,
    expected_udf: PathBuf,
    browser_pid: u32,
    host: HWND,
    fence: Box<dyn Fn() -> bool>,
    tick_deadline: Cell<Option<u64>>,
}

type Processes<'a> = Option<(&'a ProcessWitness, &'a ProcessWitness)>;

#[derive(Clone, Copy)]
struct PopupFacts {
    popup: isize, host: isize, pid: u32, expected_browser: u32,
    valid: bool, visible: bool, enabled: bool, exact_class: bool,
    parent: Option<isize>, owner: Option<isize>, root_owner: isize, root: isize,
}

/// Pure refusal predicate. Production values below come only from real Win32 getters.
fn popup_policy(facts: PopupFacts) -> bool {
    facts.popup > 0 && facts.host != 0 && facts.popup != facts.host
        && facts.pid != 0 && facts.pid == facts.expected_browser
        && facts.valid && facts.visible && facts.enabled && facts.exact_class
        && facts.parent == Some(facts.host) && facts.owner == Some(facts.host)
        && facts.root_owner == facts.host && facts.root == facts.popup
}

impl Source {
    fn check_shared_tick(&self) -> Result<(), String> {
        if self.tick_deadline.get().is_some_and(|deadline| unsafe { GetTickCount64() } >= deadline) {
            return Err("ownership-shared-monotonic-deadline-expired".into());
        }
        Ok(())
    }

    fn quick_check(&self, deadline: Instant, processes: Processes<'_>, popup: Option<HWND>) -> Result<(), String> {
        self.check_shared_tick()?;
        deadline_and_fence(deadline, self.fence.as_ref())?;
        let mut owner_pid = 0;
        if self.host.0.is_null() || !unsafe { IsWindow(Some(self.host)) }.as_bool()
            || unsafe { GetWindowThreadProcessId(self.host, Some(&mut owner_pid)) } == 0
            || owner_pid != std::process::id()
        { return Err("original-host-unavailable".into()); }
        if let Some((browser, gpu)) = processes { browser.check()?; gpu.check()?; }
        if let Some(popup) = popup { self.check_popup(popup)?; }
        deadline_and_fence(deadline, self.fence.as_ref())?;
        self.check_shared_tick()
    }

    fn check_popup(&self, popup: HWND) -> Result<(), String> {
        let mut popup_pid = 0;
        let mut class = [0u16; 64];
        let class_len = unsafe { GetClassNameW(popup, &mut class) };
        let thread = unsafe { GetWindowThreadProcessId(popup, Some(&mut popup_pid)) };
        let facts = PopupFacts {
            popup: popup.0 as isize, host: self.host.0 as isize,
            pid: if thread == 0 { 0 } else { popup_pid }, expected_browser: self.browser_pid,
            valid: unsafe { IsWindow(Some(popup)) }.as_bool(),
            visible: unsafe { IsWindowVisible(popup) }.as_bool(),
            enabled: unsafe { IsWindowEnabled(popup) }.as_bool(),
            exact_class: class_len > 0 && (class_len as usize) < class.len() - 1
                && class[..class_len as usize] == "Chrome_WidgetWin_2".encode_utf16().collect::<Vec<_>>(),
            parent: unsafe { GetParent(popup) }.ok().map(|value| value.0 as isize),
            owner: unsafe { GetWindow(popup, GW_OWNER) }.ok().map(|value| value.0 as isize),
            root_owner: unsafe { GetAncestor(popup, GA_ROOTOWNER) }.0 as isize,
            root: unsafe { GetAncestor(popup, GA_ROOT) }.0 as isize,
        };
        if !popup_policy(facts) { return Err("challenge-popup-native-ownership-unavailable".into()); }
        Ok(())
    }

    /// Identity reads themselves are bracketed by the non-COM original-source/process fence.
    /// Other COM getters below additionally bracket their operation with this exact identity check.
    fn check_context(&self, deadline: Instant, processes: Processes<'_>, popup: Option<HWND>) -> Result<(), String> {
        self.quick_check(deadline, processes, popup)?;
        let view_identity = self.view.cast::<IUnknown>().map_err(|e| error("current-view-identity", e));
        self.quick_check(deadline, processes, popup)?;
        if view_identity?.as_raw() != self.view_identity.as_raw() { return Err("original-view-identity-changed".into()); }
        let environment = unsafe { self.view2.Environment() }.map_err(|e| error("current-view-environment", e));
        self.quick_check(deadline, processes, popup)?;
        let environment = environment?;
        let identity = environment.cast::<IUnknown>().map_err(|e| error("current-environment-identity", e));
        self.quick_check(deadline, processes, popup)?;
        if identity?.as_raw() != self.environment_identity.as_raw() {
            return Err("original-environment-identity-changed".into());
        }
        Ok(())
    }

    fn checked<T>(
        &self, deadline: Instant, processes: Processes<'_>, popup: Option<HWND>,
        read: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        self.check_context(deadline, processes, popup)?;
        let result = read();
        self.check_context(deadline, processes, popup)?;
        result
    }

    /// A complete bounded membership read. The UDF and exact browser/GPU entries are
    /// validated on BOTH complete captures, not inferred from a prior diagnostic file.
    fn membership(
        &self, deadline: Instant, processes: Processes<'_>, popup: Option<HWND>, expected_gpu: Option<u32>,
    ) -> Result<u32, String> {
        let environment7 = self.checked(deadline, processes, popup, ||
            self.environment.cast::<ICoreWebView2Environment7>().map_err(|e| error("environment7", e)))?;
        let folder = self.checked(deadline, processes, popup, || {
            let mut raw = PWSTR::null();
            let result = unsafe { environment7.UserDataFolder(&mut raw) };
            let allocation = CoTaskMemPWSTR::from(raw);
            result.map_err(|e| error("exact-udf", e))?;
            if raw.is_null() { return Err("exact-udf-null".into()); }
            let length = (0..MAX_WIDE_PATH).find(|&i| unsafe { *raw.0.add(i) } == 0)
                .ok_or("exact-udf-outside-bound")?;
            let text = String::from_utf16(unsafe { std::slice::from_raw_parts(raw.0, length) })
                .map_err(|_| "exact-udf-invalid-utf16")?;
            drop(allocation);
            Ok(text)
        })?;
        let actual = fs::canonicalize(&folder).map_err(|_| "actual-disposable-udf-unavailable")?;
        let expected = fs::canonicalize(&self.expected_udf).map_err(|_| "expected-disposable-udf-unavailable")?;
        if !path_equal(&actual, &expected) || !path_equal(&expected, &self.expected_udf) {
            return Err("exact-disposable-udf-mismatch".into());
        }
        let current_browser = self.checked(deadline, processes, popup, || {
            let mut pid = 0;
            unsafe { self.view.BrowserProcessId(&mut pid) }.map_err(|e| error("source-browser-pid", e))?;
            Ok(pid)
        })?;
        if current_browser != self.browser_pid { return Err("source-browser-pid-changed".into()); }
        let environment8 = self.checked(deadline, processes, popup, ||
            self.environment.cast::<ICoreWebView2Environment8>().map_err(|e| error("environment8", e)))?;
        let collection = self.checked(deadline, processes, popup, ||
            unsafe { environment8.GetProcessInfos() }.map_err(|e| error("process-infos", e)))?;
        let count = self.checked(deadline, processes, popup, || {
            let mut count = 0;
            unsafe { collection.Count(&mut count) }.map_err(|e| error("process-count", e))?;
            Ok(count)
        })?;
        if count == 0 || count as usize > MAX_PROCESSES { return Err("membership-count-outside-bound".into()); }
        let mut entries = Vec::with_capacity(count as usize);
        for index in 0..count {
            let process = self.checked(deadline, processes, popup, ||
                unsafe { collection.GetValueAtIndex(index) }.map_err(|e| error("process-entry", e)))?;
            let pid = self.checked(deadline, processes, popup, || {
                let mut pid = 0;
                unsafe { process.ProcessId(&mut pid) }.map_err(|e| error("process-entry-pid", e))?;
                Ok(pid)
            })?;
            let kind = self.checked(deadline, processes, popup, || {
                let mut kind = COREWEBVIEW2_PROCESS_KIND::default();
                unsafe { process.Kind(&mut kind) }.map_err(|e| error("process-entry-kind", e))?;
                Ok(kind.0)
            })?;
            entries.push(MembershipEntry { pid, kind });
        }
        let gpu = validate_membership(&entries, current_browser, expected_gpu)?;
        self.check_context(deadline, processes, popup)?;
        Ok(gpu)
    }
}

#[derive(Debug)]
struct Request {
    nonce: String,
    proof: [String; 3],
    popup: String,
    popup_value: isize,
    runtime: String,
    digest: String,
}

fn canonical_u64(text: &str) -> bool {
    !text.is_empty() && text.len() <= 20 && text.bytes().all(|b| b.is_ascii_digit())
        && text.parse::<u64>().is_ok_and(|value| value.to_string() == text)
}

fn canonical_runtime(text: &str) -> bool {
    if text.is_empty() || text.len() > 383 { return false; }
    let mut count = 0;
    for part in text.split(',') {
        count += 1;
        if count > 32 || part.is_empty() || part.len() > 11
            || !part.parse::<i32>().is_ok_and(|value| value.to_string() == part)
        { return false; }
    }
    true
}

fn parse_request(bytes: &[u8], nonce: &str, proof: (u64, u64, u64)) -> Result<Request, String> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES { return Err("request-size-outside-bound".into()); }
    let text = std::str::from_utf8(bytes).map_err(|_| "request-invalid-utf8")?;
    // This parser rejects duplicate keys, including differently escaped spellings of one key.
    let Some(Value::Object(fields)) = json::parse(text) else { return Err("request-invalid-or-duplicate-json".into()); };
    let names = ["schema", "nonce", "phase", "source_proof", "popup", "root_runtime", "inventory_digest"];
    if fields.len() != names.len() || fields.iter().any(|(key, _)| !names.contains(&key.as_str())) {
        return Err("request-fields-not-exact".into());
    }
    let string = |key: &str| -> Result<&str, String> {
        match json::get(&fields, key) { Some(Value::String(value)) => Ok(value), _ => Err("request-string-field-required".into()) }
    };
    if string("schema")? != REQUEST_SCHEMA || string("nonce")? != nonce || string("phase")? != "pre-escape" {
        return Err("request-schema-nonce-or-phase-mismatch".into());
    }
    let expected = [proof.0.to_string(), proof.1.to_string(), proof.2.to_string()];
    let Some(Value::Array(values)) = json::get(&fields, "source_proof") else { return Err("request-proof-array-required".into()); };
    if values.len() != 3 || values.iter().zip(&expected).any(|(value, expected)|
        !matches!(value, Value::String(text) if canonical_u64(text) && text == expected))
    { return Err("request-original-source-proof-mismatch".into()); }
    let popup = string("popup")?;
    let popup_value = popup.parse::<isize>().ok().filter(|&value| value > 0 && value.to_string() == popup)
        .ok_or("request-popup-not-canonical-positive-handle")?;
    let runtime = string("root_runtime")?;
    let digest = string("inventory_digest")?;
    if !canonical_runtime(runtime) { return Err("request-runtime-not-bounded-canonical-csv".into()); }
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err("request-digest-not-lowercase-sha256".into());
    }
    Ok(Request { nonce: nonce.to_owned(), proof: expected, popup: popup.to_owned(), popup_value,
        runtime: runtime.to_owned(), digest: digest.to_owned() })
}

fn read_request(path: &Path) -> Result<Vec<u8>, String> {
    let before = fs::symlink_metadata(path).map_err(|_| "request-metadata-unavailable")?;
    if !before.is_file() || before.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || before.len() == 0 || before.len() > MAX_BYTES as u64
    { return Err("request-not-regular-bounded-file".into()); }
    let file = OpenOptions::new().read(true).share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path).map_err(|_| "request-exclusive-read-unavailable")?;
    let metadata = file.metadata().map_err(|_| "request-open-metadata-unavailable")?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() == 0 || metadata.len() > MAX_BYTES as u64
    { return Err("request-open-file-outside-bound".into()); }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|_| "request-read-failed")?;
    if bytes.len() != metadata.len() as usize || bytes.len() > MAX_BYTES { return Err("request-size-changed".into()); }
    Ok(bytes)
}

fn atomic_rename_no_replace(from: &Path, to: &Path) -> Result<(), String> {
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // No MOVEFILE_REPLACE_EXISTING, COPY_ALLOWED, or delayed/reboot fallback.
    unsafe { MoveFileExW(PCWSTR(from.as_ptr()), PCWSTR(to.as_ptr()), MOVE_FILE_FLAGS(0)) }
        .map_err(|e| error("response-atomic-no-replace-rename", e))
}

fn publish_response(out: &Path, text: &str, final_check: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    if text.is_empty() || text.len() > MAX_BYTES { return Err("response-size-outside-bound".into()); }
    fresh_output_paths(out, false)?;
    let mut file = OpenOptions::new().write(true).create_new(true).share_mode(0)
        .open(out.join(PENDING)).map_err(|_| "response-create-new-pending-failed")?;
    file.write_all(text.as_bytes()).and_then(|()| file.sync_all()).map_err(|_| "response-pending-flush-failed")?;
    drop(file);
    final_check()?;
    absent(&out.join(RESPONSE))?;
    atomic_rename_no_replace(&out.join(PENDING), &out.join(RESPONSE))
}

pub(super) struct GpuMenuAuthority {
    source: Source,
    browser: ProcessWitness,
    gpu: ProcessWitness,
    proof: (u64, u64, u64),
    nonce: String,
    out: PathBuf,
    attempted: bool,
    readiness_deadline: Option<(Instant, u64)>,
}

impl GpuMenuAuthority {
    /// Capture once immediately before helper spawn, and pass the returned shared tick to
    /// the helper. GetTickCount64 is the same monotonic clock in both Windows processes.
    /// A response arriving after this tick is unusable even if helper startup was delayed.
    pub(super) fn readiness_budget() -> Result<(Instant, u64), String> {
        let tick = unsafe { GetTickCount64() }.checked_add(10_000)
            .ok_or("ownership-shared-monotonic-clock-overflow")?;
        Ok((Instant::now() + Duration::from_secs(10), tick))
    }

    pub(super) fn new(
        view: &ICoreWebView2, expected_udf: &Path, browser_pid: u32, host: HWND,
        proof: (u64, u64, u64), fence: Box<dyn Fn() -> bool>, out: &Path,
    ) -> Result<Self, String> {
        // Initialization is separately bounded before helper spawn. It is NOT the helper's
        // readiness budget, and cannot renew service_once's fixed caller-supplied deadline.
        let deadline = Instant::now() + Duration::from_secs(10);
        deadline_and_fence(deadline, fence.as_ref())?;
        fresh_output_paths(out, true)?;
        if browser_pid == 0 || browser_pid == std::process::id() { return Err("source-browser-pid-invalid".into()); }
        let expected_udf = fs::canonicalize(expected_udf).map_err(|_| "expected-disposable-udf-unavailable")?;
        if !expected_udf.is_dir() || expected_udf.to_str().is_none() { return Err("expected-disposable-udf-invalid".into()); }
        let out = fs::canonicalize(out).map_err(|_| "ownership-output-directory-unavailable")?;
        if !out.is_dir() { return Err("ownership-output-not-directory".into()); }
        let view_identity = bootstrap_com(deadline, fence.as_ref(), ||
            view.cast::<IUnknown>().map_err(|e| error("original-view-identity", e)))?;
        let view2 = bootstrap_com(deadline, fence.as_ref(), ||
            view.cast::<ICoreWebView2_2>().map_err(|e| error("original-view2", e)))?;
        let environment = bootstrap_com(deadline, fence.as_ref(), ||
            unsafe { view2.Environment() }.map_err(|e| error("original-environment", e)))?;
        let environment_identity = bootstrap_com(deadline, fence.as_ref(), ||
            environment.cast::<IUnknown>().map_err(|e| error("original-environment-identity", e)))?;
        let source = Source { view: view.clone(), view2, view_identity, environment,
            environment_identity, expected_udf, browser_pid, host, fence, tick_deadline: Cell::new(None) };
        let gpu_pid = source.membership(deadline, None, None, None)?;
        source.check_context(deadline, None, None)?;
        let browser = ProcessWitness::open(browser_pid)?;
        source.check_context(deadline, None, None)?;
        let gpu = ProcessWitness::open(gpu_pid)?;
        let processes = Some((&browser, &gpu));
        source.check_context(deadline, processes, None)?;
        if !browser.image.eq_ignore_ascii_case(&gpu.image)
            || !Path::new(&browser.image).file_name().and_then(|s| s.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("msedgewebview2.exe"))
        { return Err("gpu-and-browser-exact-runtime-image-mismatch".into()); }
        // The second live enumeration occurs AFTER both handles are acquired. PID recycling
        // between the initial membership lookup and either OpenProcess therefore fails closed.
        source.membership(deadline, processes, None, Some(gpu_pid))?;
        let mut random = [0u8; 32];
        if unsafe { BCryptGenRandom(None, &mut random, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.0 < 0 {
            return Err("ownership-nonce-entropy-unavailable".into());
        }
        let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        source.check_context(deadline, processes, None)?;
        fresh_output_paths(&out, true)?;
        Ok(Self { source, browser, gpu, proof, nonce, out, attempted: false, readiness_deadline: None })
    }

    pub(super) fn nonce(&self) -> &str { &self.nonce }
    pub(super) fn gpu_pid(&self) -> u32 { self.gpu.pid }
    pub(super) fn gpu_creation(&self) -> u64 { self.gpu.creation }
    pub(super) fn browser_creation(&self) -> u64 { self.browser.creation }
    pub(super) fn environment_identity(&self) -> usize { self.source.environment_identity.as_raw() as usize }

    /// A missing request is not a consumed challenge. The first present request, including a
    /// malformed one, consumes the only attempt. No second response, retry, or rebaseline exists.
    pub(super) fn service_once(&mut self, deadline: Instant, tick_deadline: u64) -> Result<bool, String> {
        if self.attempted { return Err("ownership-request-already-consumed".into()); }
        match self.readiness_deadline {
            Some(original) if original != (deadline, tick_deadline) => {
                self.attempted = true;
                return Err("ownership-readiness-deadline-changed".into());
            }
            None => {
                self.readiness_deadline = Some((deadline, tick_deadline));
                self.source.tick_deadline.set(Some(tick_deadline));
            }
            _ => {}
        }
        if let Err(reason) = fresh_output_paths(&self.out, false) {
            self.attempted = true;
            return Err(reason);
        }
        let request_path = self.out.join(REQUEST);
        match fs::symlink_metadata(&request_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => { self.attempted = true; return Err("request-state-unavailable".into()); }
            Ok(_) => {}
        }
        self.attempted = true;
        let processes = Some((&self.browser, &self.gpu));
        self.source.check_context(deadline, processes, None)?;
        let bytes = read_request(&request_path)?;
        let request = parse_request(&bytes, &self.nonce, self.proof)?;
        let popup = HWND(request.popup_value as *mut std::ffi::c_void);
        self.source.membership(deadline, processes, Some(popup), Some(self.gpu.pid))?;
        // Every field is bounded ASCII after parsing; no user text is interpolated unescaped.
        let response = format!(concat!(
            "{{\"schema\":\"{}\",\"request_schema\":\"{}\",\"nonce\":\"{}\",\"phase\":\"pre-escape\",",
            "\"source_proof\":[\"{}\",\"{}\",\"{}\"],\"popup\":\"{}\",\"root_runtime\":\"{}\",\"inventory_digest\":\"{}\",",
            "\"browser_pid\":\"{}\",\"gpu_pid\":\"{}\",\"browser_creation\":\"{}\",\"gpu_creation\":\"{}\",",
            "\"environment_identity\":\"{}\",\"deadline_tick\":\"{}\",\"status\":\"fresh-exact-retained-membership\"}}"),
            RESPONSE_SCHEMA, REQUEST_SCHEMA, request.nonce, request.proof[0], request.proof[1], request.proof[2],
            request.popup, request.runtime, request.digest, self.browser.pid, self.gpu.pid,
            self.browser.creation, self.gpu.creation, self.environment_identity(), tick_deadline);
        publish_response(&self.out, &response, || {
            if read_request(&request_path)? != bytes { return Err("ownership-request-changed-after-capture".into()); }
            self.source.membership(deadline, processes, Some(popup), Some(self.gpu.pid))?;
            self.source.check_context(deadline, processes, Some(popup))
        })?;
        // Do not try to revoke an already observable response by deleting it. Both processes
        // independently enforce the immutable shared tick, including immediately before input.
        self.source.check_context(deadline, processes, Some(popup))?;
        Ok(true)
    }
}

#[cfg(test)]
mod synthetic_policy_tests {
    //! Synthetic JSON/membership/file refusal policy only. None of these tests is Windows
    //! acceptance, a real process-handle witness, UIA evidence, or native input authorization.
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const NONCE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const PROOF: (u64, u64, u64) = (1, 2, 0);

    fn request() -> String {
        format!(r#"{{"schema":"{REQUEST_SCHEMA}","nonce":"{NONCE}","phase":"pre-escape","source_proof":["1","2","0"],"popup":"1234","root_runtime":"42,1234,-7","inventory_digest":"{}"}}"#, "a".repeat(64))
    }

    #[test]
    fn synthetic_request_refuses_malformed_oversized_duplicate_or_unknown_fields() {
        assert!(parse_request(request().as_bytes(), NONCE, PROOF).is_ok());
        for text in [String::new(), "{}".into(), "[]".into(), "{".into(),
            format!("{} trailing", request()), format!("{} ", "x".repeat(MAX_BYTES)),
            request().replacen("{", r#"{"nonce":"duplicate","# , 1),
            request().replacen("{", r#"{"n\u006fnce":"duplicate","# , 1),
            request().replacen("{", r#"{"unknown":"no","# , 1),
            request().replace(r#""popup":"1234""#, r#""popup":1234"#),
        ] { assert!(parse_request(text.as_bytes(), NONCE, PROOF).is_err(), "{text}"); }
        assert!(parse_request(&[0xff], NONCE, PROOF).is_err());
    }

    #[test]
    fn synthetic_request_refuses_wrong_challenge_fields() {
        for text in [
            request().replace(NONCE, "wrong"),
            request().replace("pre-escape", "post-escape"),
            request().replace(REQUEST_SCHEMA, "other-schema"),
            request().replace(r#"["1","2","0"]"#, r#"["1","3","0"]"#),
            request().replace(r#"["1","2","0"]"#, r#"["01","2","0"]"#),
            request().replace(r#"["1","2","0"]"#, r#"[1,"2","0"]"#),
            request().replace(r#"["1","2","0"]"#, r#"["1","2","0","0"]"#),
        ] { assert!(parse_request(text.as_bytes(), NONCE, PROOF).is_err(), "{text}"); }
        for popup in ["", "0", "-1", "+1", "01", "1.0", "18446744073709551616", "1 "] {
            assert!(parse_request(request().replace("1234\",\"root", &format!("{popup}\",\"root")).as_bytes(), NONCE, PROOF).is_err());
        }
        for runtime in ["", "42, 1", "42,01", "42,-0", "42,+1", "42,", ",42", "2147483648", "42,,1"] {
            assert!(parse_request(request().replace("42,1234,-7", runtime).as_bytes(), NONCE, PROOF).is_err());
        }
        assert!(!canonical_runtime(&vec!["1"; 33].join(",")));
        for digest in ["a".repeat(63), "a".repeat(65), "A".repeat(64), "g".repeat(64)] {
            assert!(parse_request(request().replace(&"a".repeat(64), &digest).as_bytes(), NONCE, PROOF).is_err());
        }
        // Valid runtime/digest values are only challenge-bound echoes, never independently
        // asserted UIA facts. Native popup ownership is checked separately on the real HWND.
        assert!(canonical_u64("18446744073709551615"));
        assert!(!canonical_u64("18446744073709551616"));
    }

    fn browser() -> MembershipEntry { MembershipEntry { pid: 100, kind: COREWEBVIEW2_PROCESS_KIND_BROWSER.0 } }
    fn gpu() -> MembershipEntry { MembershipEntry { pid: 200, kind: COREWEBVIEW2_PROCESS_KIND_GPU.0 } }

    #[test]
    fn synthetic_membership_refuses_duplicates_wrong_kinds_and_missing_entries() {
        assert_eq!(validate_membership(&[browser(), gpu()], 100, Some(200)), Ok(200));
        for entries in [vec![], vec![browser()], vec![gpu()], vec![browser(), browser(), gpu()],
            vec![browser(), gpu(), gpu()],
            vec![browser(), MembershipEntry { pid: 200, kind: 1 }],
            vec![browser(), gpu(), MembershipEntry { pid: 201, kind: COREWEBVIEW2_PROCESS_KIND_GPU.0 }],
            vec![browser(), gpu(), MembershipEntry { pid: 0, kind: 1 }],
            vec![browser(), gpu(), MembershipEntry { pid: -1, kind: 1 }],
            vec![browser(), gpu(), MembershipEntry { pid: 300, kind: 99 }],
        ] { assert!(validate_membership(&entries, 100, Some(200)).is_err(), "{entries:?}"); }
        assert!(validate_membership(&[browser(), gpu()], 101, None).is_err());
        assert!(validate_membership(&[browser(), gpu()], 100, Some(201)).is_err());
        let mut entries = vec![browser(), gpu()];
        entries.extend((300..363).map(|pid| MembershipEntry { pid, kind: 1 }));
        assert_eq!(entries.len(), 65);
        assert!(validate_membership(&entries, 100, Some(200)).is_err());
        entries.pop();
        assert_eq!(validate_membership(&entries, 100, Some(200)), Ok(200));
    }

    #[test]
    fn synthetic_popup_policy_refuses_wrong_native_facts_without_granting_native_rights() {
        let baseline = PopupFacts { popup: 10, host: 20, pid: 100, expected_browser: 100,
            valid: true, visible: true, enabled: true, exact_class: true,
            parent: Some(20), owner: Some(20), root_owner: 20, root: 10 };
        // A synthetic predicate result does not construct GpuMenuAuthority or a real HWND.
        assert!(popup_policy(baseline));
        for changed in [
            PopupFacts { popup: 0, ..baseline }, PopupFacts { popup: -1, ..baseline },
            PopupFacts { popup: 20, ..baseline }, PopupFacts { host: 0, ..baseline },
            PopupFacts { pid: 0, ..baseline }, PopupFacts { pid: 200, ..baseline },
            PopupFacts { valid: false, ..baseline }, PopupFacts { visible: false, ..baseline },
            PopupFacts { enabled: false, ..baseline }, PopupFacts { exact_class: false, ..baseline },
            PopupFacts { parent: None, ..baseline }, PopupFacts { parent: Some(21), ..baseline },
            PopupFacts { owner: None, ..baseline }, PopupFacts { owner: Some(21), ..baseline },
            PopupFacts { root_owner: 21, ..baseline }, PopupFacts { root: 20, ..baseline },
        ] { assert!(!popup_policy(changed)); }
    }

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!("bowser-synthetic-gpu-policy-{}-{}-{}",
                std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn synthetic_response_refuses_preexisting_replay_and_failed_final_fence() {
        for filename in [PENDING, RESPONSE, REQUEST] {
            let temp = Temp::new();
            fs::write(temp.0.join(filename), b"original").unwrap();
            assert!(fresh_output_paths(&temp.0, true).is_err());
            if filename != REQUEST { assert!(publish_response(&temp.0, "{}", || Ok(())).is_err()); }
            assert_eq!(fs::read(temp.0.join(filename)).unwrap(), b"original");
        }
        let temp = Temp::new();
        publish_response(&temp.0, "{}", || Ok(())).unwrap();
        assert_eq!(fs::read(temp.0.join(RESPONSE)).unwrap(), b"{}");
        assert!(publish_response(&temp.0, "changed", || Ok(())).is_err());
        assert_eq!(fs::read(temp.0.join(RESPONSE)).unwrap(), b"{}");
        let temp = Temp::new();
        assert!(publish_response(&temp.0, "{}", || Err("synthetic-refused".into())).is_err());
        assert!(!temp.0.join(RESPONSE).exists());
        assert!(temp.0.join(PENDING).exists());
        assert!(publish_response(&temp.0, "{}", || Ok(())).is_err());
    }

    #[test]
    fn synthetic_response_rename_never_replaces_a_racing_destination() {
        let temp = Temp::new();
        assert!(publish_response(&temp.0, "new", || {
            fs::write(temp.0.join(RESPONSE), b"racing-original").unwrap();
            Ok(())
        }).is_err());
        assert_eq!(fs::read(temp.0.join(RESPONSE)).unwrap(), b"racing-original");
    }
}
