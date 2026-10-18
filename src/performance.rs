use crate::engine::TrackingPreventionLevel;
use std::{
    cell::RefCell,
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Instant,
};
use url::Url;

/// Most `--open-url` addresses one launch takes (--- tab stacks (T10) ---: a stack moved to a new
/// window; the split view tiles at most four, a stack may hold more).
pub const MAX_OPEN_URLS: usize = 16;

#[derive(Debug, Default)]
pub struct LaunchOptions {
    pub benchmark_log: Option<PathBuf>,
    /// Timing only: never selects benchmark behavior or suppresses production features.
    pub diagnostic_log: Option<PathBuf>,
    /// Additional native attribution, deliberately excluded from primary timing arms.
    pub diagnostic_detail: bool,
    pub user_data_dir: Option<PathBuf>,
    pub benchmark_url: Option<Url>,
    /// `--data-dir <path>` / `BOWSER_DATA_DIR`: the app-data root that replaces
    /// `%LOCALAPPDATA%\Bowser` for every persisted file. Benchmark launches always get one:
    /// the explicit value, or a throwaway temp directory (`data_dir_ephemeral`).
    pub data_dir: Option<PathBuf>,
    /// True when `data_dir` was generated for a benchmark run and may be deleted on exit.
    pub data_dir_ephemeral: bool,
    pub benchmark_tracking_prevention: Option<TrackingPreventionLevel>,
    pub benchmark_memory_pressure: bool,
    /// `--profile <name>`: a named profile; `None` is the default profile.
    pub profile: Option<String>,
    /// `--open-url <url>`: initial address for a newly opened window.
    pub open_url: Option<Url>,
    /// --- tab stacks (T10) --- a repeated `--open-url` ("Move the stack to a new window"): the
    /// addresses after the first, opened as further tabs, at most [`MAX_OPEN_URLS`] in all.
    pub more_urls: Vec<Url>,
    /// `--guest`: a fresh, disposable browser data directory with no profile identity.
    pub guest: bool,
    /// `--new-window`: a window started on purpose for a profile that is already open. Skips the
    /// single-instance refusal; see `session::decide_launch`.
    pub new_window: bool,
    /// `--dev`: a working copy. Its data lives in `%LOCALAPPDATA%\Bowser-Dev` and it never
    /// reads or updates the installed browser's profile.
    pub dev: bool,
    /// `--agent`: listen on loopback and drive a hidden page. The human window is not opened.
    pub agent: bool,
    /// `--agent-bind`: loopback IP literal. The default is `127.0.0.1`.
    pub agent_bind_host: Option<String>,
    /// `--agent-port`. The default is [`crate::agent::DEFAULT_PORT`].
    pub agent_port: Option<u16>,
    /// `--agent-call`: one command sent to a running agent, then this process exits.
    pub agent_call: Option<String>,
    pub agent_session: Option<String>,
    pub agent_url: Option<String>,
    pub agent_ref: Option<String>,
    pub agent_selector: Option<String>,
    pub agent_text: Option<String>,
    pub agent_key: Option<String>,
    pub agent_expression: Option<String>,
    pub agent_format: Option<String>,
}

impl LaunchOptions {
    pub fn parse() -> Result<Self, String> {
        let args = std::env::args_os()
            .skip(1)
            .map(|arg| {
                arg.into_string()
                    .map_err(|_| "Arguments must be valid Unicode".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::parse_from(
            args,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/fixtures"),
        )
    }
    pub fn parse_from(
        args: impl IntoIterator<Item = String>,
        fixtures: &Path,
    ) -> Result<Self, String> {
        let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        Self::parse_from_with_production_profile(args, fixtures, local_app_data.as_deref())
    }

    #[cfg(test)]
    fn parse_from_with_local_app_data(
        args: impl IntoIterator<Item = String>,
        fixtures: &Path,
        local_app_data: &Path,
    ) -> Result<Self, String> {
        Self::parse_from_with_production_profile(args, fixtures, Some(local_app_data))
    }

    fn parse_from_with_production_profile(
        args: impl IntoIterator<Item = String>,
        fixtures: &Path,
        local_app_data: Option<&Path>,
    ) -> Result<Self, String> {
        let mut options = Self::default();
        let mut raw_url = None;
        let mut explicit_data_dir: Option<PathBuf> = None;
        let mut iter = args.into_iter();
        while let Some(flag) = iter.next() {
            if flag == "--diagnostic-detail" {
                if options.diagnostic_detail { return Err("Duplicate diagnostic detail argument".into()); }
                options.diagnostic_detail = true;
                continue;
            }
            if flag == "--new-window" {
                if options.new_window {
                    return Err("Duplicate new window argument".into());
                }
                options.new_window = true;
                continue;
            }
            if flag == "--dev" {
                if options.dev {
                    return Err("Duplicate dev argument".into());
                }
                options.dev = true;
                continue;
            }
            if flag == "--guest" {
                if options.guest {
                    return Err("Duplicate guest argument".into());
                }
                options.guest = true;
                continue;
            }
            if flag == "--agent" {
                if options.agent {
                    return Err("Duplicate agent argument".into());
                }
                options.agent = true;
                continue;
            }
            if !matches!(
                flag.as_str(),
                "--benchmark-log"
                    | "--diagnostic-log"
                    | "--benchmark-user-data-dir"
                    | "--benchmark-url"
                    | "--data-dir"
                    | "--benchmark-tracking-prevention"
                    | "--benchmark-memory-pressure"
                    | "--profile"
                    | "--open-url"
                    | "--agent-bind"
                    | "--agent-port"
                    | "--agent-call"
                    | "--session"
                    | "--url"
                    | "--ref"
                    | "--selector"
                    | "--text"
                    | "--key"
                    | "--expression"
                    | "--format"
            ) {
                return Err(format!("Unknown argument: {flag}"));
            }
            let value = iter
                .next()
                .filter(|v| !v.is_empty() && !v.starts_with("--"))
                .ok_or_else(|| format!("Missing value for {flag}"))?;
            match flag.as_str() {
                "--diagnostic-log" => {
                    if options.diagnostic_log.replace(PathBuf::from(value)).is_some() {
                        return Err("Duplicate diagnostic log argument".into());
                    }
                }
                "--benchmark-log" => {
                    if options
                        .benchmark_log
                        .replace(PathBuf::from(value))
                        .is_some()
                    {
                        return Err("Duplicate benchmark log argument".into());
                    }
                }
                "--benchmark-user-data-dir" => {
                    if options
                        .user_data_dir
                        .replace(PathBuf::from(value))
                        .is_some()
                    {
                        return Err("Duplicate benchmark profile argument".into());
                    }
                }
                "--data-dir" => {
                    if explicit_data_dir.replace(PathBuf::from(value)).is_some() {
                        return Err("Duplicate data directory argument".into());
                    }
                }
                "--benchmark-tracking-prevention" => {
                    let level = match value.as_str() {
                        "balanced" => TrackingPreventionLevel::Balanced,
                        "strict" => TrackingPreventionLevel::Strict,
                        _ => {
                            return Err(
                                "Benchmark tracking prevention must be balanced or strict".into()
                            );
                        }
                    };
                    if options
                        .benchmark_tracking_prevention
                        .replace(level)
                        .is_some()
                    {
                        return Err("Duplicate benchmark tracking prevention argument".into());
                    }
                }
                "--benchmark-memory-pressure" => {
                    if value != "on" {
                        return Err("Benchmark memory pressure must be on".into());
                    }
                    if options.benchmark_memory_pressure {
                        return Err("Duplicate benchmark memory pressure argument".into());
                    }
                    options.benchmark_memory_pressure = true;
                }
                "--profile" => {
                    crate::profile::validate_name(&value).map_err(|error| error.to_string())?;
                    if options.profile.replace(value).is_some() {
                        return Err("Duplicate profile argument".into());
                    }
                }
                "--open-url" => {
                    let url =
                        Url::parse(&value).map_err(|error| format!("Invalid URL: {error}"))?;
                    if !matches!(url.scheme(), "http" | "https" | "file")
                        || !url.username().is_empty()
                        || url.password().is_some()
                    {
                        return Err("The initial address must be an HTTP(S) or file URL without credentials".into());
                    }
                    // --- tab stacks (T10) --- a new window may be given several addresses.
                    if options.open_url.is_none() {
                        options.open_url = Some(url);
                    } else if options.more_urls.len() + 1 < MAX_OPEN_URLS {
                        options.more_urls.push(url);
                    } else {
                        return Err("Too many initial URL arguments".into());
                    }
                }
                "--agent-bind" => {
                    if options.agent_bind_host.replace(value).is_some() {
                        return Err("Duplicate agent bind argument".into());
                    }
                }
                "--agent-port" => {
                    let port = crate::agent::parse_port(&value)?;
                    if options.agent_port.replace(port).is_some() {
                        return Err("Duplicate agent port argument".into());
                    }
                }
                "--agent-call" => {
                    if options.agent_call.replace(value).is_some() {
                        return Err("Duplicate agent command".into());
                    }
                }
                "--session" => {
                    if options.agent_session.replace(value).is_some() {
                        return Err("Duplicate session argument".into());
                    }
                }
                "--url" => {
                    if options.agent_url.replace(value).is_some() {
                        return Err("Duplicate URL argument".into());
                    }
                }
                "--ref" => {
                    if options.agent_ref.replace(value).is_some() {
                        return Err("Duplicate ref argument".into());
                    }
                }
                "--selector" => {
                    if options.agent_selector.replace(value).is_some() {
                        return Err("Duplicate selector argument".into());
                    }
                }
                "--text" => {
                    if options.agent_text.replace(value).is_some() {
                        return Err("Duplicate text argument".into());
                    }
                }
                "--key" => {
                    if options.agent_key.replace(value).is_some() {
                        return Err("Duplicate key argument".into());
                    }
                }
                "--expression" => {
                    if options.agent_expression.replace(value).is_some() {
                        return Err("Duplicate expression argument".into());
                    }
                }
                "--format" => {
                    if options.agent_format.replace(value).is_some() {
                        return Err("Duplicate format argument".into());
                    }
                }
                _ => {
                    if raw_url.replace(value).is_some() {
                        return Err("Duplicate benchmark URL argument".into());
                    }
                }
            }
        }
        if options.diagnostic_detail && options.diagnostic_log.is_none() {
            return Err("Diagnostic detail requires --diagnostic-log".into());
        }
        if options.diagnostic_log.is_some() {
            if options.benchmark_log.is_some() || options.agent || options.agent_call.is_some() {
                return Err("Diagnostic logging cannot be combined with benchmark or agent mode".into());
            }
            // Diagnostics are intended for synthetic profiles, never an accidental real profile.
            let dir = explicit_data_dir.as_ref().ok_or("Diagnostic logging requires explicit --data-dir")?;
            if let Some(local) = local_app_data {
                for real in [local.join("Bowser"), local.join("Bowser-Dev")] {
                    if profile_contains(&real, dir)? {
                        return Err("Diagnostic logging requires an isolated data directory".into());
                    }
                }
            }
        }
        if options.benchmark_log.is_none()
            && (options.user_data_dir.is_some()
                || raw_url.is_some()
                || options.benchmark_tracking_prevention.is_some())
        {
            return Err("Benchmark profile and URL require --benchmark-log".into());
        }
        if options.profile.is_some() && options.benchmark_log.is_some() {
            return Err("--profile cannot be combined with benchmark options".into());
        }
        if options.guest && options.profile.is_some() {
            return Err("--guest cannot be combined with --profile".into());
        }
        if options.guest && options.benchmark_log.is_some() {
            return Err("--guest cannot be combined with benchmark options".into());
        }
        if options.benchmark_tracking_prevention.is_some() && options.user_data_dir.is_none() {
            return Err("Benchmark tracking prevention requires --benchmark-user-data-dir".into());
        }
        if options.benchmark_memory_pressure && options.benchmark_log.is_none() {
            return Err("Benchmark memory pressure requires --benchmark-log".into());
        }
        if options
            .user_data_dir
            .as_ref()
            .is_some_and(|p| !p.is_absolute())
        {
            return Err("Benchmark user-data directory must be absolute".into());
        }
        if let Some(text) = raw_url {
            let url = Url::parse(&text).map_err(|e| format!("Invalid benchmark URL: {e}"))?;
            let path = url
                .to_file_path()
                .map_err(|_| "Benchmark URL must be a local file URL".to_owned())?;
            let path = path
                .canonicalize()
                .map_err(|e| format!("Cannot resolve benchmark fixture: {e}"))?;
            let root = fixtures
                .canonicalize()
                .map_err(|e| format!("Cannot resolve fixture directory: {e}"))?;
            if !path.is_file() || !path.starts_with(&root) {
                return Err("Benchmark URL must be a file inside benchmarks/fixtures".into());
            }
            options.benchmark_url = Some(
                Url::from_file_path(path).map_err(|_| "Cannot encode fixture path".to_owned())?,
            );
        }
        let env_data_dir = std::env::var_os("BOWSER_DATA_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let benchmark = options.benchmark_log.is_some() || options.benchmark_url.is_some();
        let real_root = local_app_data.map(|local| local.join("Bowser"));
        let (data_dir, ephemeral) = resolve_data_dir(
            explicit_data_dir,
            env_data_dir,
            benchmark,
            real_root.as_deref(),
            &std::env::temp_dir(),
            options.dev,
        )?;
        options.data_dir = data_dir;
        options.data_dir_ephemeral = ephemeral;
        if let Some(profile) = &options.user_data_dir {
            if options.benchmark_tracking_prevention.is_some() {
                let local_app_data = local_app_data.ok_or_else(|| {
                    "Cannot safely compare the benchmark profile without LOCALAPPDATA".to_owned()
                })?;
                let production_profile =
                    crate::profile::Profile::default_in(&local_app_data.join("Bowser"))
                        .webview2_dir();
                let requested = canonicalize_for_comparison(profile)?;
                let production = canonicalize_for_comparison(&production_profile)?;
                if windows_path_keys_equal_paths(&requested, &production) {
                    return Err(
                        "Benchmark tracking prevention cannot use Bowser's production WebView2 profile".into(),
                    );
                }
            }
            ensure_benchmark_profile(profile)?;
        }
        if options.agent
            && let (Some(dir), Some(local)) = (options.data_dir.as_ref(), local_app_data)
        {
            let real = local.join("Bowser");
            let dev = local.join("Bowser-Dev");
            if profile_contains(&real, dir)? || profile_contains(&dev, dir)? {
                return Err(
                    "--agent cannot use the Bowser or Bowser-Dev profile directory".into(),
                );
            }
        }
        crate::agent::validate_agent_launch(&crate::agent::AgentLaunch {
            serve: options.agent,
            call: options.agent_call.clone(),
            bind_host: options.agent_bind_host.clone(),
            port: options.agent_port,
            session: options.agent_session.clone(),
            url: options.agent_url.clone(),
            refer: options.agent_ref.clone(),
            selector: options.agent_selector.clone(),
            text: options.agent_text.clone(),
            key: options.agent_key.clone(),
            expression: options.agent_expression.clone(),
            format: options.agent_format.clone(),
            data_dir: options.data_dir.is_some(),
            dev: options.dev,
            guest: options.guest,
            new_window: options.new_window,
            profile: options.profile.is_some(),
            open_url: options.open_url.is_some(),
            benchmark: options.benchmark_log.is_some() || options.benchmark_url.is_some(),
        })?;
        Ok(options)
    }
}

fn profile_contains(real: &Path, dir: &Path) -> Result<bool, String> {
    let requested = canonicalize_for_comparison(dir)?;
    let real = canonicalize_for_comparison(real)?;
    let key = |path: &Path| {
        let mut key = path.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
        while key.ends_with('\\') {
            key.pop();
        }
        key
    };
    let (requested, real) = (key(&requested), key(&real));
    Ok(requested == real || requested.starts_with(&format!("{real}\\")))
}

/// Decides the app-data root override. The explicit `--data-dir` wins over `BOWSER_DATA_DIR`.
/// A benchmark launch never uses the real root: with no explicit directory it gets a fresh
/// throwaway one under `temp_base`, and an explicit one that is (or is inside) the real root is
/// refused. A dev launch defaults to `%LOCALAPPDATA%\Bowser-Dev` and also refuses the real root.
/// Returns the directory and whether it is ephemeral. `None` means a normal launch.
pub fn resolve_data_dir(
    explicit: Option<PathBuf>,
    env: Option<PathBuf>,
    benchmark: bool,
    real_root: Option<&Path>,
    temp_base: &Path,
    dev: bool,
) -> Result<(Option<PathBuf>, bool), String> {
    let chosen = explicit.or(env);
    if let Some(dir) = chosen {
        if !dir.is_absolute() {
            return Err("The data directory must be an absolute path".into());
        }
        if let Some(real) = real_root.filter(|_| benchmark || dev) {
            if profile_contains(real, &dir)? {
                return Err(if dev {
                    "Dev Bowser cannot use the real Bowser profile".into()
                } else {
                    "Benchmark runs cannot use Bowser's real app data directory".into()
                });
            }
        }
        return Ok((Some(dir), false));
    }
    if dev {
        let Some(real) = real_root else {
            return Err("Dev Bowser needs a LocalAppData directory".into());
        };
        let Some(parent) = real.parent() else {
            return Err("Dev Bowser needs a LocalAppData directory".into());
        };
        return Ok((Some(parent.join("Bowser-Dev")), false));
    }
    if benchmark {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        return Ok((
            Some(temp_base.join(format!(
                "bowser-benchmark-data-{}-{nonce}",
                std::process::id()
            ))),
            true,
        ));
    }
    Ok((None, false))
}

fn canonicalize_for_comparison(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Benchmark profile path must be absolute".into());
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                let _ = normalized.pop();
            }
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }

    let mut existing = normalized.clone();
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&existing) {
            Ok(mut canonical) => {
                for component in missing.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = existing.file_name().ok_or_else(|| {
                    format!("Cannot resolve profile path {}: {error}", path.display())
                })?;
                missing.push(name.to_os_string());
                if !existing.pop() {
                    return Err(format!(
                        "Cannot resolve profile path {}: {error}",
                        path.display()
                    ));
                }
            }
            Err(error) => {
                return Err(format!(
                    "Cannot resolve profile path {}: {error}",
                    path.display()
                ));
            }
        }
    }
}

fn windows_path_keys_equal(left: &str, right: &str) -> bool {
    fn key(path: &str) -> String {
        let path = path.replace('/', "\\");
        let trimmed = path.trim_end_matches('\\');
        let normalized = if trimmed.len() == 2 && trimmed.ends_with(':') && path.ends_with('\\') {
            format!("{trimmed}\\")
        } else {
            trimmed.to_owned()
        };
        normalized.to_ascii_lowercase()
    }
    key(left) == key(right)
}

fn windows_path_keys_equal_paths(left: &Path, right: &Path) -> bool {
    windows_path_keys_equal(&left.to_string_lossy(), &right.to_string_lossy())
}

fn ensure_benchmark_profile(path: &Path) -> Result<(), String> {
    const OWNER: &str = "Bowser benchmark WebView2 profile v1\n";
    let marker = path.join(".bowser-benchmark-profile");
    if path.exists() {
        if std::fs::read_to_string(&marker).ok().as_deref() == Some(OWNER) {
            return Ok(());
        }
        let mut entries = std::fs::read_dir(path)
            .map_err(|e| format!("Cannot inspect benchmark profile: {e}"))?;
        if entries.next().is_some() {
            return Err("Benchmark profile must be empty or owned by Bowser; existing browser data will not be reused".into());
        }
    }
    std::fs::create_dir_all(path).map_err(|e| format!("Cannot create benchmark profile: {e}"))?;
    std::fs::write(marker, OWNER)
        .map_err(|e| format!("Cannot mark isolated benchmark profile: {e}"))
}

pub struct PerfLog {
    active: Option<ActiveLog>,
}
struct ActiveLog {
    start: Instant,
    path: PathBuf,
    output: RefCell<Vec<u8>>,
    failure: RefCell<Option<String>>,
}
impl PerfLog {
    pub fn new(path: Option<&Path>) -> io::Result<Self> {
        let active = if let Some(path) = path {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let _file = File::create(path)?;
            Some(ActiveLog {
                start: Instant::now(),
                path: path.to_owned(),
                output: RefCell::new(b"event,elapsed_ms,qpc_ticks,tab_id,navigation_id,success,fixture_token,view,pid,private_bytes,count\n".to_vec()),
                failure: RefCell::new(None),
            })
        } else {
            None
        };
        Ok(Self { active })
    }
    pub fn enabled(&self) -> bool {
        self.active.is_some()
    }
    pub fn failure(&self) -> Option<String> {
        self.active
            .as_ref()
            .and_then(|active| active.failure.borrow().clone())
    }
    pub fn record(&self, event: &str) -> io::Result<()> {
        self.record_fields(event, "")
    }
    pub fn record_navigation(&self, event: &str, tab: u64, navigation: u64, success: Option<bool>, token: &str) -> io::Result<()> {
        if token.len() > 80 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid fixture token"));
        }
        self.record_fields(event, &format!("{tab},{navigation},{},{token}", success.map(|value| value.to_string()).unwrap_or_default()))
    }
    pub fn record_detail_count(&self, event: &str, tab: u64, count: u64) -> io::Result<()> {
        self.record_fields(event, &format!("{tab},,,,content,0,0,{count}"))
    }
    /// Fixed native labels and numeric attribution only: never document URLs or page text.
    pub fn record_detail(&self, event: &str, view: &str, pid: u32, private_bytes: u64, count: u64) -> io::Result<()> {
        if view.is_empty() || !view.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid diagnostic view"));
        }
        self.record_fields(event, &format!(",,,,{view},{pid},{private_bytes},{count}"))
    }
    fn record_fields(&self, event: &str, fields: &str) -> io::Result<()> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        let result = (|| {
            if event.is_empty() || !event.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid event name",
                ));
            }
            let mut tick = 0;
            // QPC is a system-wide monotonic timestamp shared with the harness.
            unsafe { windows::Win32::System::Performance::QueryPerformanceCounter(&mut tick) }
                .map_err(io::Error::other)?;
            let mut output = active.output.borrow_mut();
            writeln!(
                output,
                "{event},{:.6},{tick},{fields}",
                active.start.elapsed().as_secs_f64() * 1000.0
            )?;
            Ok(())
        })();
        if let Err(error) = &result {
            let mut failure = active.failure.borrow_mut();
            if failure.is_none() {
                *failure = Some(format!(
                    "Benchmark event '{event}' could not be recorded: {error}"
                ));
            }
        }
        result
    }
}

impl Drop for PerfLog {
    fn drop(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        let bytes = active.output.get_mut();
        if let Ok(mut file) = File::create(&active.path)
            && let Err(error) = file.write_all(bytes)
        {
            let mut failure = active.failure.borrow_mut();
            if failure.is_none() {
                *failure = Some(format!("Benchmark log could not be flushed: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }
    #[test]
    fn diagnostics_preserve_production_launch_semantics() {
        let temp = std::env::temp_dir().join("bowser-diagnostic-test");
        let options = LaunchOptions::parse_from(vec!["--diagnostic-log".into(), temp.join("events.csv").to_string_lossy().into_owned(), "--data-dir".into(), temp.to_string_lossy().into_owned(), "--open-url".into(), "http://127.0.0.1:1234/light".into()], Path::new("missing")).unwrap();
        assert!(!options.diagnostic_detail);
        assert!(LaunchOptions::parse_from(args(&["--diagnostic-detail"]), Path::new("missing")).is_err());
        let mut detail_args = vec!["--diagnostic-log".into(), temp.join("detail.csv").to_string_lossy().into_owned(), "--data-dir".into(), temp.to_string_lossy().into_owned(), "--diagnostic-detail".into()];
        assert!(LaunchOptions::parse_from(detail_args.clone(), Path::new("missing")).unwrap().diagnostic_detail);
        detail_args.push("--diagnostic-detail".into());
        assert!(LaunchOptions::parse_from(detail_args, Path::new("missing")).is_err());
        assert!(options.benchmark_log.is_none());
        assert!(options.benchmark_url.is_none());
        assert!(options.user_data_dir.is_none());
        assert!(!options.data_dir_ephemeral);
        assert!(options.open_url.is_some());
        assert!(LaunchOptions::parse_from(args(&["--diagnostic-log", "events.csv"]), Path::new("missing")).is_err());
    }
    #[test]
    fn profile_flag_accepts_only_valid_names_and_not_with_benchmarks() {
        let options =
            LaunchOptions::parse_from(args(&["--profile", "Work-1"]), Path::new("missing"))
                .unwrap();
        assert_eq!(options.profile.as_deref(), Some("Work-1"));
        assert!(
            LaunchOptions::parse_from(Vec::<String>::new(), Path::new("missing"))
                .unwrap()
                .profile
                .is_none()
        );
        for bad in ["..", "a/b", "CON", "default", "x y"] {
            assert!(
                LaunchOptions::parse_from(args(&["--profile", bad]), Path::new("missing")).is_err(),
                "{bad}"
            );
        }
        assert!(
            LaunchOptions::parse_from(
                args(&["--profile", "A", "--profile", "B"]),
                Path::new("missing")
            )
            .is_err()
        );
        assert!(
            LaunchOptions::parse_from(
                args(&["--profile", "A", "--benchmark-log", "x.csv"]),
                Path::new("missing")
            )
            .is_err()
        );
    }

    #[test]
    fn ordinary_launch_has_no_instrumentation() {
        let options =
            LaunchOptions::parse_from(Vec::<String>::new(), Path::new("missing")).unwrap();
        assert!(options.benchmark_log.is_none());
        assert!(!PerfLog::new(None).unwrap().enabled());
    }
    #[test]
    fn guest_argument_is_standalone_and_duplicate_guest_is_rejected() {
        let options = LaunchOptions::parse_from(args(&["--guest"]), Path::new("missing")).unwrap();
        assert!(options.guest);
        assert!(
            LaunchOptions::parse_from(args(&["--guest", "--guest"]), Path::new("missing")).is_err()
        );
        assert!(
            LaunchOptions::parse_from(
                args(&["--guest", "--profile", "Work"]),
                Path::new("missing")
            )
            .is_err()
        );
    }
    #[test]
    fn new_window_flag_is_standalone_and_duplicate_is_rejected() {
        let plain = LaunchOptions::parse_from(Vec::<String>::new(), Path::new("missing")).unwrap();
        assert!(!plain.new_window);
        let options = LaunchOptions::parse_from(
            args(&["--new-window", "--profile", "Work"]),
            Path::new("missing"),
        )
        .unwrap();
        assert!(options.new_window);
        assert_eq!(options.profile.as_deref(), Some("Work"));
        assert!(
            LaunchOptions::parse_from(
                args(&["--new-window", "--new-window"]),
                Path::new("missing")
            )
            .is_err()
        );
    }
    #[test]
    fn measurement_flags_require_log() {
        assert!(
            LaunchOptions::parse_from(
                args(&["--benchmark-user-data-dir", "C:\\somewhere"]),
                Path::new("missing")
            )
            .is_err()
        );
    }
    #[test]
    fn unknown_or_missing_options_fail() {
        assert!(LaunchOptions::parse_from(args(&["--unknown"]), Path::new("missing")).is_err());
        assert!(
            LaunchOptions::parse_from(args(&["--benchmark-log"]), Path::new("missing")).is_err()
        );
    }
    #[test]
    fn fixture_escape_is_rejected() {
        let root = std::env::temp_dir().join(format!("bowser-perf-path-{}", std::process::id()));
        std::fs::create_dir_all(root.join("fixtures")).unwrap();
        std::fs::write(root.join("outside.html"), "outside").unwrap();
        let url = Url::from_file_path(root.join("outside.html"))
            .unwrap()
            .to_string();
        let result = LaunchOptions::parse_from(
            args(&["--benchmark-log", "events.csv", "--benchmark-url", &url]),
            &root.join("fixtures"),
        );
        std::fs::remove_dir_all(root).unwrap();
        assert!(result.is_err());
    }
    #[test]
    fn existing_fixture_and_isolated_profile_are_accepted_only_in_measurement_mode() {
        let root = std::env::temp_dir().join(format!("bowser-perf-valid-{}", std::process::id()));
        std::fs::create_dir_all(root.join("fixtures")).unwrap();
        let fixture = root.join("fixtures/light.html");
        std::fs::write(&fixture, "light").unwrap();
        let url = Url::from_file_path(&fixture).unwrap().to_string();
        let profile = root.join("profile").to_string_lossy().to_string();
        let result = LaunchOptions::parse_from(
            args(&[
                "--benchmark-log",
                "events.csv",
                "--benchmark-user-data-dir",
                &profile,
                "--benchmark-url",
                &url,
            ]),
            &root.join("fixtures"),
        );
        std::fs::remove_dir_all(root).unwrap();
        let options = result.unwrap();
        assert_eq!(options.user_data_dir, Some(PathBuf::from(profile)));
        assert_eq!(options.benchmark_url.unwrap().scheme(), "file");
    }

    #[test]
    fn initial_url_is_allowed_with_a_profile_and_rejects_unsafe_schemes() {
        let url = "https://example.com/path?q=one%20two";
        let options = LaunchOptions::parse_from(
            args(&["--profile", "Research", "--open-url", url]),
            Path::new("missing"),
        )
        .unwrap();
        assert_eq!(options.profile.as_deref(), Some("Research"));
        assert_eq!(options.open_url.unwrap().as_str(), url);
        assert!(
            LaunchOptions::parse_from(
                args(&["--open-url", "javascript:alert(1)"]),
                Path::new("missing"),
            )
            .is_err()
        );
    }

    /// --- tab stacks (T10) --- "Move the stack to a new window" passes every page of the stack.
    #[test]
    fn a_new_window_takes_several_initial_urls_up_to_the_limit() {
        let options = LaunchOptions::parse_from(
            args(&["--new-window", "--open-url", "https://a.example/", "--open-url", "https://b.example/", "--open-url", "https://c.example/"]),
            Path::new("missing"),
        )
        .unwrap();
        assert_eq!(options.open_url.unwrap().as_str(), "https://a.example/");
        assert_eq!(options.more_urls.iter().map(Url::as_str).collect::<Vec<_>>(), ["https://b.example/", "https://c.example/"]);
        assert!(
            LaunchOptions::parse_from(args(&["--open-url", "https://a.example/", "--open-url", "file:///c:/x?u=p"]), Path::new("missing")).is_ok(),
            "each address passes the same check as the first"
        );
        assert!(LaunchOptions::parse_from(args(&["--open-url", "https://a.example/", "--open-url", "javascript:alert(1)"]), Path::new("missing")).is_err());
        let mut many = Vec::new();
        for n in 0..MAX_OPEN_URLS {
            many.extend(["--open-url".to_owned(), format!("https://s{n}.example/")]);
        }
        assert_eq!(LaunchOptions::parse_from(many.clone(), Path::new("missing")).unwrap().more_urls.len(), MAX_OPEN_URLS - 1);
        many.extend(["--open-url".to_owned(), "https://one-more.example/".to_owned()]);
        assert!(LaunchOptions::parse_from(many, Path::new("missing")).is_err());
    }
    #[test]
    fn remote_benchmark_url_and_duplicate_flags_are_rejected() {
        assert!(
            LaunchOptions::parse_from(
                args(&[
                    "--benchmark-log",
                    "events.csv",
                    "--benchmark-url",
                    "https://example.com"
                ]),
                Path::new("missing")
            )
            .is_err()
        );
        assert!(
            LaunchOptions::parse_from(
                args(&["--benchmark-log", "one.csv", "--benchmark-log", "two.csv"]),
                Path::new("missing")
            )
            .is_err()
        );
    }

    #[test]
    fn tracking_prevention_is_opt_in_and_requires_an_isolated_logged_profile() {
        let root = std::env::temp_dir().join(format!("bowser-policy-{}", std::process::id()));
        let fixtures = root.join("fixtures");
        let profile = root.join("profile");
        std::fs::create_dir_all(&fixtures).unwrap();
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            profile.join(".bowser-benchmark-profile"),
            "Bowser benchmark WebView2 profile v1\n",
        )
        .unwrap();
        let local_app_data = root.join("local-app-data");

        let default = LaunchOptions::parse_from_with_local_app_data(
            Vec::<String>::new(),
            &fixtures,
            &local_app_data,
        )
        .unwrap();
        assert_eq!(default.benchmark_tracking_prevention, None);

        for (value, expected) in [
            ("balanced", crate::engine::TrackingPreventionLevel::Balanced),
            ("strict", crate::engine::TrackingPreventionLevel::Strict),
        ] {
            let profile_text = profile.to_string_lossy().to_string();
            let options = LaunchOptions::parse_from_with_local_app_data(
                args(&[
                    "--benchmark-log",
                    "events.csv",
                    "--benchmark-user-data-dir",
                    &profile_text,
                    "--benchmark-tracking-prevention",
                    value,
                ]),
                &fixtures,
                &local_app_data,
            )
            .unwrap();
            assert_eq!(options.benchmark_tracking_prevention, Some(expected));
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tracking_prevention_rejects_missing_invalid_and_duplicate_values() {
        let root = std::env::temp_dir().join(format!("bowser-policy-args-{}", std::process::id()));
        let fixtures = root.join("fixtures");
        let profile = root.join("profile");
        std::fs::create_dir_all(&fixtures).unwrap();
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            profile.join(".bowser-benchmark-profile"),
            "Bowser benchmark WebView2 profile v1\n",
        )
        .unwrap();
        let profile_text = profile.to_string_lossy().to_string();
        let local_app_data = root.join("local-app-data");

        for values in [
            vec!["--benchmark-tracking-prevention"],
            vec!["--benchmark-tracking-prevention", "none"],
            vec![
                "--benchmark-log",
                "events.csv",
                "--benchmark-user-data-dir",
                &profile_text,
                "--benchmark-tracking-prevention",
                "balanced",
                "--benchmark-tracking-prevention",
                "strict",
            ],
        ] {
            assert!(
                LaunchOptions::parse_from_with_local_app_data(
                    args(&values),
                    &fixtures,
                    &local_app_data,
                )
                .is_err(),
                "arguments should be rejected: {values:?}"
            );
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tracking_prevention_requires_logging_and_a_benchmark_profile() {
        let root =
            std::env::temp_dir().join(format!("bowser-policy-requires-{}", std::process::id()));
        let fixtures = root.join("fixtures");
        std::fs::create_dir_all(&fixtures).unwrap();
        let profile = root.join("profile").to_string_lossy().to_string();
        let local_app_data = root.join("local-app-data");

        for values in [
            vec!["--benchmark-tracking-prevention", "balanced"],
            vec![
                "--benchmark-log",
                "events.csv",
                "--benchmark-tracking-prevention",
                "strict",
            ],
        ] {
            assert!(
                LaunchOptions::parse_from_with_local_app_data(
                    args(&values),
                    &fixtures,
                    &local_app_data,
                )
                .is_err(),
                "arguments should be rejected: {values:?}"
            );
        }

        let no_log_but_profile = args(&[
            "--benchmark-user-data-dir",
            &profile,
            "--benchmark-tracking-prevention",
            "balanced",
        ]);
        assert!(
            LaunchOptions::parse_from_with_local_app_data(
                no_log_but_profile,
                &fixtures,
                &local_app_data,
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn memory_pressure_is_opt_in_and_accepted_only_with_logging() {
        let fixtures = Path::new("missing");
        assert!(
            !LaunchOptions::parse_from(Vec::<String>::new(), fixtures)
                .unwrap()
                .benchmark_memory_pressure
        );
        let options = LaunchOptions::parse_from(
            args(&[
                "--benchmark-log",
                "events.csv",
                "--benchmark-memory-pressure",
                "on",
            ]),
            fixtures,
        )
        .unwrap();
        assert!(options.benchmark_memory_pressure);

        assert!(
            LaunchOptions::parse_from(args(&["--benchmark-memory-pressure", "on"]), fixtures)
                .is_err(),
            "memory pressure must require --benchmark-log"
        );
    }

    #[test]
    fn memory_pressure_rejects_missing_invalid_and_duplicate_values() {
        let fixtures = Path::new("missing");
        for values in [
            vec![
                "--benchmark-log",
                "events.csv",
                "--benchmark-memory-pressure",
            ],
            vec![
                "--benchmark-log",
                "events.csv",
                "--benchmark-memory-pressure",
                "off",
            ],
            vec![
                "--benchmark-log",
                "events.csv",
                "--benchmark-memory-pressure",
                "on",
                "--benchmark-memory-pressure",
                "on",
            ],
        ] {
            assert!(
                LaunchOptions::parse_from(args(&values), fixtures).is_err(),
                "arguments should be rejected: {values:?}"
            );
        }
    }

    #[test]
    fn policy_never_accepts_the_production_profile_even_when_it_is_owned() {
        let root = std::env::temp_dir().join(format!("bowser-policy-prod-{}", std::process::id()));
        let fixtures = root.join("fixtures");
        let local_app_data = root.join("local-app-data");
        let production = local_app_data.join("Bowser").join("WebView2");
        std::fs::create_dir_all(&fixtures).unwrap();
        std::fs::create_dir_all(&production).unwrap();
        std::fs::write(
            production.join(".bowser-benchmark-profile"),
            "Bowser benchmark WebView2 profile v1\n",
        )
        .unwrap();
        let alias = local_app_data
            .join("bowser")
            .join("unused")
            .join("..")
            .join("WebView2");
        let alias_text = alias.to_string_lossy().to_string();
        let options = args(&[
            "--benchmark-log",
            "events.csv",
            "--benchmark-user-data-dir",
            &alias_text,
            "--benchmark-tracking-prevention",
            "strict",
        ]);

        let result =
            LaunchOptions::parse_from_with_local_app_data(options, &fixtures, &local_app_data);
        assert!(
            result.is_err(),
            "production profile aliases must be refused"
        );
        assert!(production.join(".bowser-benchmark-profile").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn policy_rejects_a_junction_alias_before_writing_a_profile_marker() {
        struct Cleanup {
            root: PathBuf,
            junction: PathBuf,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                // Remove the reparse point itself before deleting its target's parent tree.
                let _ = std::fs::remove_dir(&self.junction);
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }

        let root = std::env::temp_dir().join(format!(
            "bowser-policy-junction-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let local_app_data = root.join("local-app-data");
        let production = local_app_data.join("Bowser").join("WebView2");
        let junction = root.join("production-profile-alias");
        let _cleanup = Cleanup {
            root: root.clone(),
            junction: junction.clone(),
        };
        std::fs::create_dir_all(&production).unwrap();

        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&production)
            .output()
            .expect("cmd.exe must be available for creating a temporary junction");
        assert!(
            output.status.success(),
            "creating the temporary junction failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            windows_path_keys_equal_paths(
                &canonicalize_for_comparison(&junction).unwrap(),
                &canonicalize_for_comparison(&production).unwrap(),
            ),
            "the test path must resolve through the junction to the simulated production profile"
        );

        let alias = junction.to_string_lossy().to_string();
        let result = LaunchOptions::parse_from_with_local_app_data(
            args(&[
                "--benchmark-log",
                "events.csv",
                "--benchmark-user-data-dir",
                &alias,
                "--benchmark-tracking-prevention",
                "balanced",
            ]),
            Path::new("missing"),
            &local_app_data,
        );
        assert!(
            result.is_err(),
            "junction aliases of production must be refused"
        );
        assert!(
            std::fs::read_dir(&production).unwrap().next().is_none(),
            "rejecting the alias must happen before writing the benchmark ownership marker"
        );
        assert!(
            !production.join(".bowser-benchmark-profile").exists(),
            "the production profile must remain untouched"
        );
    }

    #[test]
    fn windows_path_keys_ignore_case_and_normalize_separators() {
        assert!(windows_path_keys_equal(
            r"C:\Users\Charlie\AppData\Local\Bowser\WebView2\",
            r"c:/users/charlie/appdata/local/bowser/webview2",
        ));
        assert!(!windows_path_keys_equal(
            r"C:\Bowser\WebView2",
            r"C:\Bowser\Other"
        ));
    }
    #[test]
    fn benchmark_profile_does_not_reuse_existing_browser_data() {
        let root = std::env::temp_dir().join(format!("bowser-perf-unowned-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Local State"), "existing browser data").unwrap();
        let path = root.to_string_lossy().to_string();
        let result = LaunchOptions::parse_from_with_local_app_data(
            args(&[
                "--benchmark-log",
                "events.csv",
                "--benchmark-user-data-dir",
                &path,
                "--benchmark-tracking-prevention",
                "strict",
            ]),
            Path::new("missing"),
            &std::env::temp_dir().join("bowser-local-app-data"),
        );
        assert!(result.is_err(), "must not share an unowned browser profile");
        assert_eq!(
            std::fs::read_to_string(root.join("Local State")).unwrap(),
            "existing browser data"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn log_records_ordered_events_and_elapsed_time() {
        let path = std::env::temp_dir().join(format!("bowser-events-{}.csv", std::process::id()));
        let log = PerfLog::new(Some(&path)).unwrap();
        assert!(log.enabled());
        log.record("window_visible").unwrap();
        log.record("input_ready").unwrap();
        drop(log);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let rows: Vec<_> = text.lines().collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], "event,elapsed_ms,qpc_ticks,tab_id,navigation_id,success,fixture_token,view,pid,private_bytes,count");
        let a: Vec<_> = rows[1].split(',').collect();
        let b: Vec<_> = rows[2].split(',').collect();
        assert_eq!(a[0], "window_visible");
        assert_eq!(b[0], "input_ready");
        assert!(b[1].parse::<f64>().unwrap() >= a[1].parse::<f64>().unwrap());
    }

    #[test]
    fn detail_rows_have_fixed_labels_and_numeric_columns() {
        let path = std::env::temp_dir().join(format!("bowser-detail-{}.csv", std::process::id()));
        let log = PerfLog::new(Some(&path)).unwrap();
        assert!(log.record_detail("chrome_host_ready", "https://private.test/", 1, 2, 3).is_err());
        log.record_detail("chrome_renderer_private", "toolbar", 123, 456, 7).unwrap();
        log.record_detail_count("configure_filter_registration_finished", 9, 42).unwrap();
        drop(log);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let rows: Vec<_> = text.lines().collect();
        let chrome: Vec<_> = rows[1].split(',').collect();
        assert_eq!(&chrome[7..], &["toolbar", "123", "456", "7"]);
        let content: Vec<_> = rows[2].split(',').collect();
        assert_eq!(content[3], "9");
        assert_eq!(&content[7..], &["content", "0", "0", "42"]);
    }

    #[test]
    fn records_are_buffered_until_log_drop() {
        let path =
            std::env::temp_dir().join(format!("bowser-buffered-events-{}.csv", std::process::id()));
        let log = PerfLog::new(Some(&path)).unwrap();
        log.record("enter_received").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        drop(log);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("enter_received,")
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn log_records_webview_startup_phase_events() {
        let path = std::env::temp_dir().join(format!(
            "bowser-engine-phase-events-{}.csv",
            std::process::id()
        ));
        let log = PerfLog::new(Some(&path)).unwrap();
        let expected = [
            "webview_profile_started",
            "webview_profile_ready",
            "webview_environment_requested",
            "webview_environment_result",
            "webview_controller_requested",
            "webview_controller_result",
            "webview_configure_started",
            "webview_configure_finished",
            "webview_create_completed",
        ];
        for event in expected {
            log.record(event).unwrap();
        }
        assert_eq!(log.failure(), None);
        drop(log);

        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let recorded: Vec<_> = text
            .lines()
            .skip(1)
            .map(|line| line.split(',').next().unwrap())
            .collect();
        assert_eq!(recorded, expected);
    }

    #[test]
    fn benchmark_without_a_data_dir_gets_a_temp_dir_outside_the_real_root() {
        let real = Path::new(r"C:\Users\u\AppData\Local\Bowser");
        let temp = std::env::temp_dir();
        let (dir, ephemeral) = resolve_data_dir(None, None, true, Some(real), &temp, false).unwrap();
        let dir = dir.expect("benchmark always has a data dir");
        assert!(ephemeral);
        assert!(dir.starts_with(&temp));
        assert!(!dir.starts_with(real));
    }

    #[test]
    fn explicit_data_dir_wins_and_benchmarks_refuse_the_real_root() {
        let real = Path::new(r"C:\Users\u\AppData\Local\Bowser");
        let temp = std::env::temp_dir();
        let explicit = PathBuf::from(r"D:\data");
        let env = PathBuf::from(r"D:\env-data");
        assert_eq!(
            resolve_data_dir(Some(explicit.clone()), Some(env.clone()), true, Some(real), &temp, false)
                .unwrap(),
            (Some(explicit), false)
        );
        assert_eq!(
            resolve_data_dir(None, Some(env.clone()), false, Some(real), &temp, false).unwrap(),
            (Some(env), false)
        );
        assert_eq!(
            resolve_data_dir(None, None, false, Some(real), &temp, false).unwrap(),
            (None, false)
        );
        for bad in [
            r"C:\Users\u\AppData\Local\Bowser",
            r"c:\users\u\appdata\local\bowser\",
            r"C:\Users\u\AppData\Local\Bowser\Profiles\x",
            r"C:\Users\u\AppData\Local\Other\..\Bowser",
        ] {
            assert!(
                resolve_data_dir(Some(PathBuf::from(bad)), None, true, Some(real), &temp, false).is_err(),
                "{bad}"
            );
        }
        assert!(resolve_data_dir(Some(PathBuf::from("rel")), None, false, None, &temp, false).is_err());
        assert!(
            resolve_data_dir(
                Some(PathBuf::from(r"C:\Users\u\AppData\Local\Bowser2")),
                None,
                true,
                Some(real),
                &temp,
                false
            )
            .is_ok()
        );
    }

    #[test]
    fn dev_launch_uses_its_own_profile_and_refuses_the_real_one() {
        let local = Path::new(r"C:\Users\u\AppData\Local");
        let dev = LaunchOptions::parse_from_with_local_app_data(
            ["--dev".to_owned()],
            Path::new("missing"),
            local,
        )
        .unwrap();
        assert!(dev.dev);
        assert_eq!(
            dev.data_dir.as_deref(),
            Some(Path::new(r"C:\Users\u\AppData\Local\Bowser-Dev"))
        );
        let isolated = LaunchOptions::parse_from_with_local_app_data(
            ["--dev".to_owned(), "--data-dir".to_owned(), r"D:\bowser-work".to_owned()],
            Path::new("missing"),
            local,
        )
        .unwrap();
        assert_eq!(
            isolated.data_dir.as_deref(),
            Some(Path::new(r"D:\bowser-work"))
        );
        let refused = LaunchOptions::parse_from_with_local_app_data(
            [
                "--dev".to_owned(),
                "--data-dir".to_owned(),
                r"C:\Users\u\AppData\Local\Bowser\Profiles\x".to_owned(),
            ],
            Path::new("missing"),
            local,
        );
        assert!(refused.is_err(), "{refused:?}");
    }
}
