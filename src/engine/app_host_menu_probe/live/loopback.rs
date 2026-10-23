//! Disposable fixture transport only. Synthetic tests below are not native/runtime evidence.
//! Partial reads/writes are forward progress, not retries: any error or interruption is terminal.
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};

const CONNECTION_BUDGET: Duration = Duration::from_secs(1);
const HEADER_CAP: usize = 8192;
const RESPONSE_CAP: usize = 16 * 1024;
const DIAGNOSTIC_RECORD_CAP: usize = 32;
const DIAGNOSTIC_JSON_CAP: usize = 32 * 1024;

#[derive(Clone, Copy, Default)]
pub(super) struct Receipts {
    pub(super) paste: usize,
    pub(super) note: usize,
    pub(super) destination: usize,
    pub(super) paste_full_responses: usize,
    pub(super) note_full_responses: usize,
    pub(super) destination_full_responses: usize,
    pub(super) full_responses: usize,
}
impl Receipts {
    // Preserve the original valid-effect count: increment before attempting the response.
    fn valid(&mut self, effect: Effect) {
        match effect {
            Effect::Paste => self.paste = self.paste.saturating_add(1),
            Effect::Note => self.note = self.note.saturating_add(1),
            Effect::Destination => self.destination = self.destination.saturating_add(1),
            Effect::None => {}
        }
    }
    fn full(&mut self, effect: Effect) {
        self.full_responses = self.full_responses.saturating_add(1);
        match effect {
            Effect::Paste => self.paste_full_responses = self.paste_full_responses.saturating_add(1),
            Effect::Note => self.note_full_responses = self.note_full_responses.saturating_add(1),
            Effect::Destination => self.destination_full_responses = self.destination_full_responses.saturating_add(1),
            Effect::None => {}
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Effect { #[default] None, Paste, Note, Destination }
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum PathClass { #[default] Unknown, Source, Negative, Destination, Effect, Other }
impl PathClass {
    fn name(self) -> &'static str {
        match self { Self::Unknown => "unknown", Self::Source => "source", Self::Negative => "negative",
            Self::Destination => "destination", Self::Effect => "effect", Self::Other => "other" }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Outcome {
    #[default] NotStarted, Complete, Eof, Oversize, InvalidText, Malformed, DuplicateKey,
    Timeout, Interrupted, Error, TimeoutSetupError, LateCompletion, WriteZero, PeerRejected,
}
impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Self::NotStarted => "not-started", Self::Complete => "complete", Self::Eof => "eof",
            Self::Oversize => "oversize", Self::InvalidText => "invalid-text", Self::Malformed => "malformed",
            Self::DuplicateKey => "duplicate-key", Self::Timeout => "timeout", Self::Interrupted => "interrupted",
            Self::Error => "error", Self::TimeoutSetupError => "timeout-setup-error",
            Self::LateCompletion => "late-completion", Self::WriteZero => "write-zero", Self::PeerRejected => "peer-rejected",
        }
    }
}
#[derive(Clone, Default)]
struct ConnectionDiagnostic {
    read_calls: u32, read_bytes: usize, write_calls: u32, write_bytes: usize, partial_write_calls: u32,
    header_complete: bool, nonce_match: bool, kind_match: bool, value_match: bool,
    path: PathClass, parse: Outcome, read: Outcome, write: Outcome,
    read_deadline_exhausted: bool, write_deadline_exhausted: bool,
    effect: Effect,
}
impl ConnectionDiagnostic {
    fn json(&self) -> String {
        format!(concat!("{{\"path_class\":\"{}\",\"header_complete\":{},\"nonce_match\":{},",
            "\"kind_match\":{},\"value_match\":{},\"read_calls\":{},\"read_bytes\":{},",
            "\"write_calls\":{},\"write_bytes\":{},\"partial_write_calls\":{},",
            "\"parse_outcome\":\"{}\",\"read_outcome\":\"{}\",\"write_outcome\":\"{}\",",
            "\"read_deadline_exhausted\":{},\"write_deadline_exhausted\":{}}}"),
            self.path.name(), self.header_complete, self.nonce_match, self.kind_match, self.value_match,
            self.read_calls, self.read_bytes, self.write_calls, self.write_bytes, self.partial_write_calls,
            self.parse.name(), self.read.name(), self.write.name(), self.read_deadline_exhausted, self.write_deadline_exhausted)
    }
}
#[derive(Default)]
struct Diagnostics {
    accepted: u64, read_calls: u64, write_calls: u64, discarded_records: u64,
    records: VecDeque<ConnectionDiagnostic>,
}
impl Diagnostics {
    fn record(&mut self, diagnostic: ConnectionDiagnostic) {
        self.read_calls = self.read_calls.saturating_add(u64::from(diagnostic.read_calls));
        self.write_calls = self.write_calls.saturating_add(u64::from(diagnostic.write_calls));
        if self.records.len() == DIAGNOSTIC_RECORD_CAP {
            self.records.pop_front();
            self.discarded_records = self.discarded_records.saturating_add(1);
        }
        self.records.push_back(diagnostic);
    }
}

trait Clock { fn elapsed(&self) -> Duration; }
struct MonotonicClock(Instant);
impl Clock for MonotonicClock { fn elapsed(&self) -> Duration { self.0.elapsed() } }
fn remaining(clock: &impl Clock) -> Option<Duration> {
    CONNECTION_BUDGET.checked_sub(clock.elapsed()).filter(|left| !left.is_zero())
}
trait Transport: Read + Write {
    fn read_timeout(&mut self, remaining: Duration) -> io::Result<()>;
    fn write_timeout(&mut self, remaining: Duration) -> io::Result<()>;
}
impl Transport for TcpStream {
    fn read_timeout(&mut self, remaining: Duration) -> io::Result<()> { self.set_read_timeout(Some(remaining)) }
    fn write_timeout(&mut self, remaining: Duration) -> io::Result<()> { self.set_write_timeout(Some(remaining)) }
}
fn error_outcome(error: &io::Error) -> Outcome {
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => Outcome::Timeout,
        io::ErrorKind::Interrupted => Outcome::Interrupted,
        _ => Outcome::Error,
    }
}

fn read_header(io: &mut impl Transport, clock: &impl Clock, diagnostic: &mut ConnectionDiagnostic) -> Option<Vec<u8>> {
    let mut bytes = [0u8; HEADER_CAP];
    let mut used = 0;
    loop {
        let Some(left) = remaining(clock) else {
            diagnostic.read = Outcome::Timeout; diagnostic.read_deadline_exhausted = true; return None;
        };
        let configured = io.read_timeout(left);
        if remaining(clock).is_none() {
            diagnostic.read = Outcome::LateCompletion; diagnostic.read_deadline_exhausted = true; return None;
        }
        if configured.is_err() { diagnostic.read = Outcome::TimeoutSetupError; return None; }
        // Recheck immediately before the read syscall, as well as after timeout configuration.
        if remaining(clock).is_none() {
            diagnostic.read = Outcome::Timeout; diagnostic.read_deadline_exhausted = true; return None;
        }
        let result = io.read(&mut bytes[used..]);
        diagnostic.read_calls = diagnostic.read_calls.saturating_add(1);
        let late = remaining(clock).is_none();
        diagnostic.read_deadline_exhausted |= late;
        let count = match result {
            Err(error) => { diagnostic.read = error_outcome(&error); return None; }
            Ok(count) => count,
        };
        // Even a final successful read may not be accepted at or beyond the original deadline.
        if count > HEADER_CAP - used { diagnostic.read = Outcome::Error; return None; }
        diagnostic.read_bytes = diagnostic.read_bytes.saturating_add(count);
        if late { diagnostic.read = Outcome::LateCompletion; return None; }
        if count == 0 { diagnostic.read = Outcome::Eof; return None; }
        used += count;
        if let Some(end) = bytes[..used].windows(4).position(|part| part == b"\r\n\r\n") {
            diagnostic.header_complete = true;
            // This fixture accepts one bodyless GET, never pipelined requests or trailing body bytes.
            if end + 4 != used { diagnostic.read = Outcome::Malformed; return None; }
            diagnostic.read = Outcome::Complete;
            return Some(bytes[..used].to_vec());
        }
        if used == HEADER_CAP { diagnostic.read = Outcome::Oversize; return None; }
    }
}

fn write_response(io: &mut impl Transport, clock: &impl Clock, bytes: &[u8], diagnostic: &mut ConnectionDiagnostic) {
    if bytes.len() > RESPONSE_CAP { diagnostic.write = Outcome::Oversize; return; }
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(left) = remaining(clock) else {
            diagnostic.write = Outcome::Timeout; diagnostic.write_deadline_exhausted = true; return;
        };
        let configured = io.write_timeout(left);
        if remaining(clock).is_none() {
            diagnostic.write = Outcome::LateCompletion; diagnostic.write_deadline_exhausted = true; return;
        }
        if configured.is_err() { diagnostic.write = Outcome::TimeoutSetupError; return; }
        if remaining(clock).is_none() {
            diagnostic.write = Outcome::Timeout; diagnostic.write_deadline_exhausted = true; return;
        }
        let result = io.write(&bytes[offset..]);
        diagnostic.write_calls = diagnostic.write_calls.saturating_add(1);
        let late = remaining(clock).is_none();
        diagnostic.write_deadline_exhausted |= late;
        let count = match result {
            Err(error) => { diagnostic.write = error_outcome(&error); return; }
            Ok(count) => count,
        };
        if count > bytes.len() - offset { diagnostic.write = Outcome::Error; return; }
        diagnostic.write_bytes = diagnostic.write_bytes.saturating_add(count);
        if count < bytes.len() - offset && count != 0 {
            diagnostic.partial_write_calls = diagnostic.partial_write_calls.saturating_add(1);
        }
        if late { diagnostic.write = Outcome::LateCompletion; return; }
        if count == 0 { diagnostic.write = Outcome::WriteZero; return; }
        offset += count;
    }
    diagnostic.write = Outcome::Complete;
}

fn header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}
fn query_component(value: &str) -> Option<String> {
    let mut output = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        output.push(match byte {
            b'+' => b' ',
            b'%' => {
                let high = char::from(bytes.next()?).to_digit(16)?;
                let low = char::from(bytes.next()?).to_digit(16)?;
                (high * 16 + low) as u8
            }
            _ => byte,
        });
    }
    let text = String::from_utf8(output).ok()?;
    (!text.chars().any(char::is_control)).then_some(text)
}
fn parse_header(bytes: &[u8], nonce: &str, diagnostic: &mut ConnectionDiagnostic) -> Result<(), Outcome> {
    let text = std::str::from_utf8(bytes).map_err(|_| Outcome::InvalidText)?;
    if !text.is_ascii() || text.bytes().any(|byte| byte == 127 || (byte < 32 && !b"\r\n\t".contains(&byte))) {
        return Err(Outcome::InvalidText);
    }
    let mut lines = text.strip_suffix("\r\n\r\n").ok_or(Outcome::Malformed)?.split("\r\n");
    let mut words = lines.next().ok_or(Outcome::Malformed)?.split(' ');
    if words.next() != Some("GET") { return Err(Outcome::Malformed); }
    let target = words.next().ok_or(Outcome::Malformed)?;
    if words.next() != Some("HTTP/1.1") || words.next().is_some() || !target.starts_with('/')
        || target.starts_with("//") || target.contains(['#', '\\'])
        || target.bytes().any(|byte| !(33..=126).contains(&byte)) { return Err(Outcome::Malformed); }
    // URL parsing alone tolerates malformed percent escapes and normalizes path dot segments.
    // Neither is part of this controlled fixture's origin-form GET protocol.
    query_component(target).ok_or(Outcome::InvalidText)?;
    let (mut host, mut length) = (false, false);
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(Outcome::Malformed)?;
        if name.is_empty() || !name.bytes().all(header_name_byte)
            || value.bytes().any(|byte| byte < 32 && byte != b'\t') { return Err(Outcome::Malformed); }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("host") {
            if host || value.is_empty() { return Err(Outcome::Malformed); }
            host = true;
        } else if name.eq_ignore_ascii_case("content-length") {
            if length || value != "0" { return Err(Outcome::Malformed); }
            length = true;
        } else if name.eq_ignore_ascii_case("transfer-encoding") { return Err(Outcome::Malformed); }
    }
    if !host { return Err(Outcome::Malformed); }
    let request = url::Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| Outcome::Malformed)?;
    if request.path() != target.split('?').next().ok_or(Outcome::Malformed)? { return Err(Outcome::Malformed); }
    diagnostic.path = match request.path() {
        "/source" => PathClass::Source, "/negative" => PathClass::Negative,
        "/destination" => PathClass::Destination, "/effect" => PathClass::Effect, _ => PathClass::Other,
    };
    let (mut found_nonce, mut found_kind, mut found_value) = (None, None, None);
    if let Some(query) = request.query() {
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = query_component(key).ok_or(Outcome::InvalidText)?;
            let value = query_component(value).ok_or(Outcome::InvalidText)?;
            let slot = match key.as_str() {
                "nonce" => &mut found_nonce, "kind" => &mut found_kind, "value" => &mut found_value,
                _ => continue,
            };
            if slot.is_some() { return Err(Outcome::DuplicateKey); }
            *slot = Some(value);
        }
    }
    diagnostic.nonce_match = found_nonce.as_deref() == Some(nonce);
    diagnostic.kind_match = matches!(found_kind.as_deref(), Some("paste" | "note"));
    diagnostic.value_match = found_kind.as_ref().zip(found_value.as_ref())
        .is_some_and(|(kind, value)| value == &format!("{kind}-{nonce}"));
    if diagnostic.nonce_match {
        diagnostic.effect = match diagnostic.path {
            PathClass::Destination => Effect::Destination,
            PathClass::Effect if diagnostic.kind_match && diagnostic.value_match => {
                if found_kind.as_deref() == Some("paste") { Effect::Paste } else { Effect::Note }
            }
            _ => Effect::None,
        };
    }
    Ok(())
}

fn serve_connection(io: &mut impl Transport, clock: &impl Clock, nonce: &str, source: &str,
    mut valid: impl FnMut(Effect)) -> ConnectionDiagnostic {
    let mut diagnostic = ConnectionDiagnostic::default();
    let Some(header) = read_header(io, clock, &mut diagnostic) else { return diagnostic; };
    if let Err(outcome) = parse_header(&header, nonce, &mut diagnostic) {
        diagnostic.parse = outcome; return diagnostic;
    }
    diagnostic.parse = Outcome::Complete;
    let (status, body) = match (diagnostic.nonce_match, diagnostic.path, diagnostic.effect) {
        (true, PathClass::Source | PathClass::Negative, _) => ("200 OK", source.to_owned()),
        (true, PathClass::Destination, Effect::Destination) => ("200 OK", format!("<!doctype html><meta charset=utf-8><title>Disposable split destination</title><body data-destination-nonce={nonce}>known split destination</body>")),
        (true, PathClass::Effect, Effect::Paste) => ("200 OK", format!("{nonce}:paste")),
        (true, PathClass::Effect, Effect::Note) => ("200 OK", format!("{nonce}:note")),
        (true, PathClass::Effect, _) => ("400 Bad Request", String::new()),
        _ => ("404 Not Found", String::new()),
    };
    valid(diagnostic.effect);
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len());
    write_response(io, clock, response.as_bytes(), &mut diagnostic);
    diagnostic
}

pub(super) struct Loopback {
    pub(super) url: url::Url,
    pub(super) nonce: String,
    pub(super) receipts: Arc<Mutex<Receipts>>,
    stop: Arc<AtomicBool>, diagnostics: Arc<Mutex<Diagnostics>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Loopback {
    pub(super) fn start(nonce: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = url::Url::parse(&format!("http://127.0.0.1:{}/source?nonce={nonce}", listener.local_addr().unwrap().port())).unwrap();
        let stop = Arc::new(AtomicBool::new(false)); let stopping = stop.clone();
        let receipts = Arc::new(Mutex::new(Receipts::default())); let sink = receipts.clone();
        let diagnostics = Arc::new(Mutex::new(Diagnostics::default())); let reports = diagnostics.clone();
        let token = nonce.clone();
        let source = include_str!("../../../../benchmarks/fixtures/app-host-context-menu/index.html").replace("__NONCE__", &nonce);
        assert!(source.len() <= RESPONSE_CAP - 1024, "controlled fixture response exceeds its fixed transport cap");
        let worker = std::thread::spawn(move || while !stopping.load(Ordering::Relaxed) {
            let Ok((mut socket, peer)) = listener.accept() else { std::thread::sleep(Duration::from_millis(10)); continue; };
            // The same clock starts immediately after accept, before bookkeeping, parsing or I/O.
            let clock = MonotonicClock(Instant::now());
            if let Ok(mut reports) = reports.lock() { reports.accepted = reports.accepted.saturating_add(1); }
            let diagnostic = if peer.ip().is_loopback() {
                serve_connection(&mut socket, &clock, &token, &source, |effect| {
                    if let Ok(mut receipts) = sink.lock() { receipts.valid(effect); }
                })
            } else { ConnectionDiagnostic { read: Outcome::PeerRejected, ..Default::default() } };
            // All socket operations have ended. No mutex guard is held over any socket syscall.
            if diagnostic.write == Outcome::Complete {
                if let Ok(mut receipts) = sink.lock() { receipts.full(diagnostic.effect); }
            }
            if let Ok(mut reports) = reports.lock() { reports.record(diagnostic); }
        });
        Self { url, nonce, receipts, stop, diagnostics, worker: Some(worker) }
    }

    /// Bounded, best-effort evidence only. No raw URLs, requests, bodies, nonce or error text.
    /// This method never changes any acceptance predicate or performs socket/native I/O.
    pub(super) fn diagnostic_json(&self) -> Option<String> {
        let receipts = *self.receipts.try_lock().ok()?;
        let reports = self.diagnostics.try_lock().ok()?;
        let records = reports.records.iter().map(ConnectionDiagnostic::json).collect::<Vec<_>>().join(",");
        let json = format!(concat!("{{\"schema\":\"controlled-loopback-transport-v1\",\"non_atomic\":true,\"capture_status\":\"primitive-counter-snapshot-not-authority\",\"connection_budget_ms\":1000,",
            "\"header_cap\":{},\"response_cap\":{},\"record_cap\":{},\"accepted\":{},\"read_calls\":{},",
            "\"write_calls\":{},\"discarded_records\":{},\"valid_paste\":{},\"valid_note\":{},\"valid_destination\":{},",
            "\"full_responses\":{},\"paste_full_responses\":{},\"note_full_responses\":{},",
            "\"destination_full_responses\":{},\"records\":[{}]}}"),
            HEADER_CAP, RESPONSE_CAP, DIAGNOSTIC_RECORD_CAP, reports.accepted, reports.read_calls,
            reports.write_calls, reports.discarded_records, receipts.paste, receipts.note, receipts.destination,
            receipts.full_responses, receipts.paste_full_responses, receipts.note_full_responses,
            receipts.destination_full_responses, records);
        (json.len() <= DIAGNOSTIC_JSON_CAP).then_some(json)
    }
}
impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

// This entire module is reached only by the cfg(test) live probe module.
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct FakeClock(Cell<Duration>);
impl Clock for FakeClock { fn elapsed(&self) -> Duration { self.0.get() } }
impl FakeClock { fn advance(&self, duration: Duration) { self.0.set(self.0.get() + duration); } }
enum ReadStep { Bytes(Vec<u8>, Duration), Eof, Error(io::ErrorKind, Duration) }
enum WriteStep { Bytes(usize, Duration), Error(io::ErrorKind, Duration) }
struct FakeIo {
    clock: Rc<FakeClock>, reads: VecDeque<ReadStep>, writes: VecDeque<WriteStep>,
    written: Vec<u8>, read_timeouts: Vec<Duration>, write_timeouts: Vec<Duration>,
}
impl FakeIo {
    fn new(reads: impl IntoIterator<Item = ReadStep>, writes: impl IntoIterator<Item = WriteStep>) -> Self {
        Self { clock: Rc::new(FakeClock::default()), reads: reads.into_iter().collect(), writes: writes.into_iter().collect(),
            written: Vec::new(), read_timeouts: Vec::new(), write_timeouts: Vec::new() }
    }
}
impl Read for FakeIo {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        match self.reads.pop_front().unwrap_or(ReadStep::Eof) {
            ReadStep::Bytes(bytes, duration) => {
                self.clock.advance(duration);
                let count = output.len().min(bytes.len()); output[..count].copy_from_slice(&bytes[..count]);
                if count < bytes.len() { self.reads.push_front(ReadStep::Bytes(bytes[count..].to_vec(), Duration::ZERO)); }
                Ok(count)
            }
            ReadStep::Eof => Ok(0),
            ReadStep::Error(kind, duration) => { self.clock.advance(duration); Err(io::Error::from(kind)) }
        }
    }
}
impl Write for FakeIo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self.writes.pop_front().unwrap_or(WriteStep::Bytes(bytes.len(), Duration::ZERO)) {
            WriteStep::Bytes(limit, duration) => {
                self.clock.advance(duration); let count = limit.min(bytes.len()); self.written.extend_from_slice(&bytes[..count]); Ok(count)
            }
            WriteStep::Error(kind, duration) => { self.clock.advance(duration); Err(io::Error::from(kind)) }
        }
    }
    fn flush(&mut self) -> io::Result<()> { panic!("bounded helper must not use a hidden flush/write_all loop") }
}
impl Transport for FakeIo {
    fn read_timeout(&mut self, timeout: Duration) -> io::Result<()> { self.read_timeouts.push(timeout); Ok(()) }
    fn write_timeout(&mut self, timeout: Duration) -> io::Result<()> { self.write_timeouts.push(timeout); Ok(()) }
}
const NONCE: &str = "synthetic-nonce";
fn request(target: &str) -> Vec<u8> { format!("GET {target} HTTP/1.1\r\nHost: localhost:1234\r\nConnection: close\r\n\r\n").into_bytes() }
fn effect_request() -> Vec<u8> { request("/effect?nonce=synthetic-nonce&kind=note&value=note-synthetic-nonce") }
fn run(io: &mut FakeIo) -> (ConnectionDiagnostic, Receipts) {
    let mut receipts = Receipts::default(); let clock = io.clock.clone();
    let diagnostic = serve_connection(io, clock.as_ref(), NONCE, "synthetic-source", |effect| receipts.valid(effect));
    if diagnostic.write == Outcome::Complete { receipts.full(diagnostic.effect); }
    (diagnostic, receipts)
}
fn bytes(bytes: Vec<u8>) -> ReadStep { ReadStep::Bytes(bytes, Duration::ZERO) }

#[test]
fn synthetic_loopback_request_and_header_split_at_every_byte_uses_actual_bounded_io() {
    for target in ["/source?nonce=synthetic-nonce", "/negative?nonce=synthetic-nonce",
        "/destination?nonce=synthetic-nonce", "/effect?nonce=synthetic-nonce&kind=note&value=note-synthetic-nonce"] {
        let whole = request(target);
        for split in 0..=whole.len() {
            let chunks = [&whole[..split], &whole[split..]].into_iter().filter(|part| !part.is_empty()).map(|part| bytes(part.to_vec()));
            let mut io = FakeIo::new(chunks, []); let (diagnostic, receipts) = run(&mut io);
            assert_eq!(diagnostic.read, Outcome::Complete, "split={split}"); assert_eq!(diagnostic.parse, Outcome::Complete);
            assert_eq!(diagnostic.write, Outcome::Complete); assert!(diagnostic.header_complete && diagnostic.nonce_match);
            assert_eq!(receipts.full_responses, 1); assert_eq!(io.read_timeouts.len(), diagnostic.read_calls as usize);
            if target.starts_with("/effect") { assert_eq!((receipts.note, receipts.note_full_responses), (1, 1)); assert!(io.written.ends_with(b"synthetic-nonce:note")); }
        }
    }
    let whole = effect_request(); let mut io = FakeIo::new(whole.iter().map(|byte| bytes(vec![*byte])), []);
    let (diagnostic, _) = run(&mut io); assert_eq!(diagnostic.read_calls as usize, whole.len()); assert_eq!(diagnostic.write, Outcome::Complete);
}

#[test]
fn synthetic_loopback_incomplete_oversize_malformed_and_duplicate_requests_fail_closed() {
    let mut incomplete = FakeIo::new([bytes(b"GET /source HTTP/1.1\r\nHost: local".to_vec()), ReadStep::Eof], []);
    assert_eq!(run(&mut incomplete).0.read, Outcome::Eof); assert!(incomplete.written.is_empty());
    let prefix = "GET /source?nonce=synthetic-nonce HTTP/1.1\r\nHost: localhost\r\nX-Pad: ";
    let exact = format!("{prefix}{}\r\n\r\n", "a".repeat(HEADER_CAP - prefix.len() - 4)).into_bytes();
    assert_eq!(exact.len(), HEADER_CAP);
    let mut io = FakeIo::new([bytes(exact.clone())], []); assert_eq!(run(&mut io).0.write, Outcome::Complete);
    let over = format!("{prefix}{}\r\n\r\n", "a".repeat(HEADER_CAP - prefix.len() - 3)).into_bytes();
    assert_eq!(over.len(), HEADER_CAP + 1);
    let mut io = FakeIo::new([bytes(over)], []); let (diagnostic, _) = run(&mut io);
    assert_eq!(diagnostic.read, Outcome::Oversize); assert_eq!(diagnostic.read_bytes, HEADER_CAP); assert!(io.written.is_empty());
    let malformed = [
        b"POST /source HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec(),
        b"GET /source HTTP/1.1\r\n\r\n".to_vec(),
        b"GET /source HTTP/1.1\r\nHost: a\r\nContent-Length: 1\r\n\r\n".to_vec(),
        b"GET /source HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
        b"GET /source HTTP/1.1\r\nHost: a\r\n bad: value\r\n\r\n".to_vec(),
        b"GET /source HTTP/1.1\r\nHost: a\r\nX: \xff\r\n\r\n".to_vec(),
        request("/source?nonce=%ZZ"), request("/source?nonce=%FF"), request("/source%ZZ?nonce=synthetic-nonce"),
        request("/unused/../source?nonce=synthetic-nonce"),
        request("/source?nonce=synthetic-nonce&nonce=synthetic-nonce"),
        request("/effect?nonce=synthetic-nonce&kind=note&kind=note&value=note-synthetic-nonce"),
        request("/effect?nonce=synthetic-nonce&kind=note&value=note-synthetic-nonce&value=note-synthetic-nonce"),
        request("/source?nonce=synthetic-nonce&%6eonce=synthetic-nonce"),
        [request("/source?nonce=synthetic-nonce"), b"extra".to_vec()].concat(),
    ];
    for input in malformed {
        let mut io = FakeIo::new([bytes(input)], []); let (diagnostic, receipts) = run(&mut io);
        assert!(diagnostic.read != Outcome::Complete || diagnostic.parse != Outcome::Complete);
        assert_eq!((receipts.note, receipts.full_responses), (0, 0)); assert!(io.written.is_empty());
    }
    let mut io = FakeIo::new([bytes(request("/source?nonce=x&nonce=x"))], []);
    assert_eq!(run(&mut io).0.parse, Outcome::DuplicateKey);
}

#[test]
fn synthetic_loopback_one_shared_budget_rejects_idle_boundary_and_late_read_or_write() {
    let mut io = FakeIo::new([ReadStep::Bytes(effect_request(), Duration::from_millis(600))], [WriteStep::Bytes(usize::MAX, Duration::from_millis(450))]);
    let (diagnostic, receipts) = run(&mut io); assert_eq!(diagnostic.write, Outcome::LateCompletion);
    assert_eq!(io.write_timeouts, [Duration::from_millis(400)]); assert_eq!((receipts.note, receipts.note_full_responses), (1, 0));
    for duration in [CONNECTION_BUDGET, Duration::from_millis(1001)] {
        let mut io = FakeIo::new([ReadStep::Bytes(effect_request(), duration)], []); let (diagnostic, receipts) = run(&mut io);
        assert_eq!(diagnostic.read, Outcome::LateCompletion); assert!(diagnostic.read_deadline_exhausted);
        assert_eq!(receipts.note, 0); assert!(io.write_timeouts.is_empty());
    }
    let mut io = FakeIo::new([ReadStep::Error(io::ErrorKind::TimedOut, CONNECTION_BUDGET)], []);
    let (diagnostic, _) = run(&mut io); assert_eq!(diagnostic.read, Outcome::Timeout); assert!(diagnostic.read_deadline_exhausted);
    let mut io = FakeIo::new([ReadStep::Error(io::ErrorKind::Interrupted, Duration::ZERO), bytes(effect_request())], []);
    assert_eq!(run(&mut io).0.read, Outcome::Interrupted); assert_eq!(io.reads.len(), 1);
    let mut io = FakeIo::new([ReadStep::Bytes(effect_request(), Duration::from_millis(800))], []);
    let clock = io.clock.clone(); let mut receipts = Receipts::default();
    let diagnostic = serve_connection(&mut io, clock.as_ref(), NONCE, "synthetic-source", |effect| { receipts.valid(effect); clock.advance(Duration::from_millis(200)); });
    assert_eq!(diagnostic.write, Outcome::Timeout); assert!(io.write_timeouts.is_empty()); assert_eq!(receipts.note, 1);
    let mut io = FakeIo::new([ReadStep::Bytes(effect_request(), Duration::from_millis(900))], [WriteStep::Bytes(usize::MAX, Duration::from_millis(100))]);
    assert_eq!(run(&mut io).0.write, Outcome::LateCompletion);
}

#[test]
fn synthetic_loopback_partial_write_failure_zero_interruption_and_success_use_actual_loop() {
    let mut io = FakeIo::new([bytes(effect_request())], [WriteStep::Bytes(3, Duration::from_millis(10)), WriteStep::Bytes(2, Duration::from_millis(10)), WriteStep::Error(io::ErrorKind::BrokenPipe, Duration::ZERO)]);
    let (diagnostic, receipts) = run(&mut io); assert_eq!(diagnostic.write, Outcome::Error);
    assert_eq!((diagnostic.write_bytes, diagnostic.partial_write_calls, diagnostic.write_calls), (5, 2, 3));
    assert_eq!((receipts.note, receipts.note_full_responses), (1, 0));
    assert_eq!(io.write_timeouts, [Duration::from_millis(1000), Duration::from_millis(990), Duration::from_millis(980)]);
    let mut io = FakeIo::new([bytes(effect_request())], [WriteStep::Bytes(0, Duration::ZERO)]);
    assert_eq!(run(&mut io).0.write, Outcome::WriteZero);
    let mut io = FakeIo::new([bytes(effect_request())], [WriteStep::Error(io::ErrorKind::Interrupted, Duration::ZERO), WriteStep::Bytes(1, Duration::ZERO)]);
    assert_eq!(run(&mut io).0.write, Outcome::Interrupted); assert_eq!(io.writes.len(), 1);
    let mut io = FakeIo::new([bytes(effect_request())], [WriteStep::Bytes(1, Duration::ZERO), WriteStep::Bytes(2, Duration::ZERO)]);
    let (diagnostic, receipts) = run(&mut io); assert_eq!(diagnostic.write, Outcome::Complete);
    assert_eq!((diagnostic.partial_write_calls, diagnostic.write_calls), (2, 3)); assert_eq!(diagnostic.write_bytes, io.written.len());
    assert_eq!((receipts.note, receipts.note_full_responses, receipts.full_responses), (1, 1, 1));
}
