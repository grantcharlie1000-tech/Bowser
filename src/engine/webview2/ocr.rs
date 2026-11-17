//! Per-tab glue of the Spoiler Firewall's native OCR adapter (`crate::spoiler_ocr`,
//! docs/spoiler-firewall.md). Everything here runs on the UI thread; only decoding and OCR run on
//! the worker thread, whose results are collected by a short UI-thread poll timer.
//!
//! Flow: `WebResourceResponseReceived` on a high-risk site hands an image body (read from
//! `GetContent`) to the worker; the page's bundle asks for text through its hook; a debounced pump
//! drains those requests with `ExecuteScript` on the top frame and every tracked child frame, and
//! answers from the cache with a second `ExecuteScript`. Nothing is fetched on a page's behalf and
//! no web message channel is involved.

use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

use super::*;
use webview2_com::{
    FrameChildFrameCreatedEventHandler, FrameCreatedEventHandler, FrameDOMContentLoadedEventHandler,
    FrameDestroyedEventHandler, FrameNavigationStartingEventHandler, WebResourceResponseReceivedEventHandler,
    WebResourceResponseViewGetContentCompletedHandler,
};
use crate::spoiler::OcrCredentials;
use crate::spoiler_ocr as ocr;
use ocr::{Delivery, Entry, Group, Key, Status};

/// Debounce after a burst of image responses, the follow-up cadence while requests are pending,
/// how many empty follow-ups are tried after a trigger, and how long a request for an image that
/// has not been seen yet is held (the response event normally comes first).
const BURST_DELAY_MS: u32 = 150;
const FOLLOW_UP_MS: u32 = 300;
const EMPTY_FOLLOW_UPS: u8 = 2;
const WAITER_TICK_MS: u32 = 250;
const UNSEEN_GRACE: Duration = Duration::from_secs(2);
const MAX_WAITERS: usize = 512;
const POLL_MS: u32 = 100;
const MAX_FRAMES: usize = 64;

// ---- UI-thread timers ----

thread_local! {
    static TIMERS: RefCell<HashMap<usize, Box<dyn FnOnce()>>> = RefCell::new(HashMap::new());
    static HUB: RefCell<Hub> = RefCell::new(Hub::new());
}

unsafe extern "system" fn timer_fired(_hwnd: HWND, _message: u32, timer: usize, _time: u32) {
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::KillTimer(None, timer) };
    let job = TIMERS.try_with(|timers| timers.borrow_mut().remove(&timer)).ok().flatten();
    if let Some(job) = job {
        job();
    }
}

/// Runs `job` once, `ms` from now, on this thread's message loop. When no timer can be had the job
/// is dropped: every caller re-arms itself from the next trigger.
fn after(ms: u32, job: Box<dyn FnOnce()>) {
    let timer = unsafe { windows::Win32::UI::WindowsAndMessaging::SetTimer(None, 0, ms, Some(timer_fired)) };
    if timer != 0 {
        let _ = TIMERS.try_with(|timers| timers.borrow_mut().insert(timer, job));
    }
}

// ---- process-wide state: the caches and the worker ----

struct Hub {
    /// Index 0 is the profile's cache, 1 the private tabs' (cleared when the last private tab goes).
    caches: [ocr::Lru; 2],
    worker: Option<ocr::Worker>,
    worker_failed: bool,
    tickets: HashMap<u64, (usize, Key)>,
    next_ticket: u64,
    polling: bool,
    tabs: Vec<std::rc::Weak<TabOcr>>,
}

impl Hub {
    fn new() -> Self {
        Self {
            caches: [ocr::Lru::new(ocr::LRU_CAPACITY), ocr::Lru::new(ocr::LRU_CAPACITY)],
            worker: None,
            worker_failed: false,
            tickets: HashMap::new(),
            next_ticket: 1,
            polling: false,
            tabs: Vec::new(),
        }
    }
}

fn cache_get(private: bool, key: &Key) -> Option<Entry> {
    HUB.with(|hub| hub.borrow_mut().caches[usize::from(private)].get(key))
}

fn cache_put(private: bool, key: Key, entry: Entry) {
    HUB.with(|hub| hub.borrow_mut().caches[usize::from(private)].insert(key, entry));
}

fn cache_remove(private: bool, key: &Key) {
    HUB.with(|hub| hub.borrow_mut().caches[usize::from(private)].remove(key));
}

/// Hands a body to the worker. On any failure the entry is settled `Unavailable` at once.
fn submit(private: bool, key: Key, bytes: Vec<u8>, epoch: Arc<AtomicU64>, queued_at: u64) {
    let queued = HUB.with(|hub| {
        let mut hub = hub.borrow_mut();
        if hub.worker.is_none() && !hub.worker_failed {
            hub.worker = ocr::Worker::spawn();
            hub.worker_failed = hub.worker.is_none();
        }
        let ticket = hub.next_ticket;
        hub.next_ticket += 1;
        let Some(worker) = hub.worker.as_ref() else { return false };
        worker.push(ocr::Job { ticket, bytes, epoch, queued_at });
        hub.tickets.insert(ticket, (usize::from(private), key.clone()));
        true
    });
    if !queued {
        cache_put(private, key, Entry::done(Status::Unavailable, ""));
        return;
    }
    start_polling();
}

fn start_polling() {
    let start = HUB.with(|hub| {
        let mut hub = hub.borrow_mut();
        !std::mem::replace(&mut hub.polling, true)
    });
    if start {
        after(POLL_MS, Box::new(poll));
    }
}

/// Collects finished jobs into the caches, then lets every tab answer requests that were waiting.
fn poll() {
    let (results, again) = HUB.with(|hub| {
        let mut hub = hub.borrow_mut();
        let results = hub.worker.as_ref().map(|worker| worker.take_results()).unwrap_or_default();
        let mut settled = Vec::new();
        for result in results {
            let Some((cache, key)) = hub.tickets.remove(&result.ticket) else { continue };
            match result.outcome {
                Some((status, text)) => {
                    // Text is kept only for an ok scan; other states carry none.
                    let text = if status == Status::Ok { text } else { String::new() };
                    hub.caches[cache].insert(key, Entry::done(status, text));
                    settled.push(());
                }
                // Dropped (navigation, overflow): forget it so a later sighting is scanned afresh.
                None => hub.caches[cache].remove(&key),
            }
        }
        let again = !hub.tickets.is_empty();
        hub.polling = again;
        (settled.len(), again)
    });
    if results > 0 {
        for tab in live_tabs() {
            tab.tick_waiters();
        }
    }
    if again {
        after(POLL_MS, Box::new(poll));
    }
}

fn live_tabs() -> Vec<Rc<TabOcr>> {
    HUB.with(|hub| {
        let mut hub = hub.borrow_mut();
        hub.tabs.retain(|tab| tab.strong_count() > 0);
        hub.tabs.iter().filter_map(|tab| tab.upgrade()).collect()
    })
}

// ---- per tab ----

/// One frame the pump can drain: the top document (`frame` is `None`) or a tracked child frame.
struct Target {
    id: u32,
    inflight: Cell<bool>,
    /// The last drain returned requests, so follow-ups continue.
    pending: Cell<bool>,
    /// The frame's site group, from its last navigation. A page the bundle does not run in, or one
    /// off the allowlist, has none.
    group: Cell<Option<Group>>,
    frame: RefCell<Option<ICoreWebView2Frame2>>,
}

impl Target {
    fn new(id: u32, frame: Option<ICoreWebView2Frame2>) -> Rc<Self> {
        Rc::new(Self {
            id,
            inflight: Cell::new(false),
            pending: Cell::new(false),
            group: Cell::new(None),
            frame: RefCell::new(frame),
        })
    }
}

struct Waiter {
    target: u32,
    id: String,
    key: Key,
    deadline: Instant,
}

pub(super) struct TabOcr {
    private: bool,
    /// `Some` once the hooks are installed on this tab; absent credentials mean the bundle is
    /// told `ocr:false` and nothing here runs.
    creds: RefCell<Option<OcrCredentials>>,
    visible: Rc<Cell<bool>>,
    webview: RefCell<Option<ICoreWebView2>>,
    /// Bumped on a top-level navigation: queued jobs of the old page are dropped unrun.
    epoch: Arc<AtomicU64>,
    top_group: Cell<Option<Group>>,
    top: Rc<Target>,
    frames: RefCell<Vec<Rc<Target>>>,
    next_frame: Cell<u32>,
    waiters: RefCell<Vec<Waiter>>,
    waiter_timer: Cell<bool>,
    pump_scheduled: Cell<bool>,
    tail: Cell<u8>,
    log: Option<Rc<dyn Fn(&'static str)>>,
}

impl Drop for TabOcr {
    fn drop(&mut self) {
        if self.private {
            let _ = HUB.try_with(|hub| {
                if let Ok(mut hub) = hub.try_borrow_mut() {
                    hub.tabs.retain(|tab| tab.strong_count() > 0);
                    // The last private tab is closing (this one is mid-drop, so it no longer counts).
                    let others = hub.tabs.iter().filter_map(|tab| tab.upgrade()).any(|tab| tab.private);
                    if !others {
                        hub.caches[1].clear();
                    }
                }
            });
        }
    }
}

fn setting_on() -> bool {
    let settings = crate::spoiler::current();
    settings.enabled && settings.ocr
}

impl TabOcr {
    pub(super) fn new(private: bool, visible: Rc<Cell<bool>>, log: Option<Rc<dyn Fn(&'static str)>>) -> Rc<Self> {
        let tab = Rc::new(Self {
            private,
            creds: RefCell::new(None),
            visible,
            webview: RefCell::new(None),
            epoch: Arc::new(AtomicU64::new(0)),
            top_group: Cell::new(None),
            top: Target::new(0, None),
            frames: RefCell::new(Vec::new()),
            next_frame: Cell::new(1),
            waiters: RefCell::new(Vec::new()),
            waiter_timer: Cell::new(false),
            pump_scheduled: Cell::new(false),
            tail: Cell::new(0),
            log,
        });
        HUB.with(|hub| hub.borrow_mut().tabs.push(Rc::downgrade(&tab)));
        tab
    }

    /// The credentials the bundle's config is built with, once the hooks are in place.
    pub(super) fn credentials(&self) -> Option<OcrCredentials> {
        self.creds.borrow().clone()
    }

    /// Subscribes the tab's events. `WebResourceResponseReceived` only fires for requests a resource
    /// filter matches, and the blocker's own filters are narrow (its rules, not images), so the tab
    /// adds image filters for the listed platforms' hosts only: every other image request still never
    /// waits on the UI thread. A failure leaves OCR off for this tab (the bundle is told `ocr:false`);
    /// it is never fatal.
    pub(super) unsafe fn install(self: &Rc<Self>, webview: &ICoreWebView2) {
        if unsafe { self.try_install(webview) }.is_err() {
            *self.creds.borrow_mut() = None;
            if let Some(log) = self.log.as_ref() {
                log("spoiler_ocr_failed");
            }
        }
    }

    unsafe fn try_install(self: &Rc<Self>, webview: &ICoreWebView2) -> Result<()> {
        unsafe {
            let responses: ICoreWebView2_2 = webview.cast()?;
            let frames_api: ICoreWebView2_4 = webview.cast()?;
            let creds = OcrCredentials::generate().ok_or_else(|| Error::from(windows::Win32::Foundation::E_FAIL))?;
            *self.webview.borrow_mut() = Some(webview.clone());
            let webview22 = webview.cast::<ICoreWebView2_22>().ok();
            for uri in ocr::filter_uris() {
                let uri = HSTRING::from(uri);
                match &webview22 {
                    Some(webview22) => webview22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                        PCWSTR(uri.as_ptr()),
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
                        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                    )?,
                    None => webview.AddWebResourceRequestedFilter(PCWSTR(uri.as_ptr()), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE)?,
                }
            }
            let mut token = 0i64;

            let weak = Rc::downgrade(self);
            responses.add_WebResourceResponseReceived(
                &WebResourceResponseReceivedEventHandler::create(Box::new(move |_sender, args| {
                    if let (Some(tab), Some(args)) = (weak.upgrade(), args) {
                        let _ = tab.on_response(&args);
                    }
                    Ok(())
                })),
                &mut token,
            )?;

            let weak = Rc::downgrade(self);
            frames_api.add_FrameCreated(
                &FrameCreatedEventHandler::create(Box::new(move |_sender, args| {
                    if let (Some(tab), Some(args)) = (weak.upgrade(), args)
                        && let Ok(frame) = args.Frame()
                    {
                        tab.track_frame(frame);
                    }
                    Ok(())
                })),
                &mut token,
            )?;

            let weak = Rc::downgrade(self);
            webview.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_sender, _args| {
                    if let Some(tab) = weak.upgrade() {
                        // A new document: its jobs and questions are over. (Same-document
                        // navigations, the SPA case, do not raise this event.)
                        tab.epoch.fetch_add(1, Ordering::Relaxed);
                        tab.waiters.borrow_mut().clear();
                        tab.top.pending.set(false);
                    }
                    Ok(())
                })),
                &mut token,
            )?;

            let weak = Rc::downgrade(self);
            webview.add_SourceChanged(
                &SourceChangedEventHandler::create(Box::new(move |sender, _args| {
                    if let (Some(tab), Some(sender)) = (weak.upgrade(), sender) {
                        tab.refresh_top(&sender);
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            let weak = Rc::downgrade(self);
            webview.add_ContentLoading(
                &ContentLoadingEventHandler::create(Box::new(move |sender, _args| {
                    if let (Some(tab), Some(sender)) = (weak.upgrade(), sender) {
                        tab.refresh_top(&sender);
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            let weak = Rc::downgrade(self);
            responses.add_DOMContentLoaded(
                &DOMContentLoadedEventHandler::create(Box::new(move |_sender, _args| {
                    if let Some(tab) = weak.upgrade() {
                        tab.kick();
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            let weak = Rc::downgrade(self);
            webview.add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_sender, _args| {
                    if let Some(tab) = weak.upgrade() {
                        tab.kick();
                    }
                    Ok(())
                })),
                &mut token,
            )?;

            // Set last: the credentials are what tells the bundle OCR exists.
            *self.creds.borrow_mut() = Some(creds);
            Ok(())
        }
    }

    fn refresh_top(&self, webview: &ICoreWebView2) {
        let group = current_web_url(webview).and_then(|url| url.host_str().and_then(ocr::group_for_host));
        self.top_group.set(group);
        self.top.group.set(group);
    }

    /// Becomes visible or loads something: look for requests soon.
    pub(super) fn kick(self: &Rc<Self>) {
        if self.creds.borrow().is_none() || !setting_on() {
            return;
        }
        self.tail.set(EMPTY_FOLLOW_UPS);
        self.schedule_pump(BURST_DELAY_MS);
    }

    // ---- frames ----

    fn track_frame(self: &Rc<Self>, frame: ICoreWebView2Frame) {
        if self.frames.borrow().len() >= MAX_FRAMES {
            return;
        }
        let Ok(frame2) = frame.cast::<ICoreWebView2Frame2>() else { return };
        let id = self.next_frame.get();
        self.next_frame.set(id.wrapping_add(1).max(1));
        let target = Target::new(id, Some(frame2.clone()));
        self.frames.borrow_mut().push(target.clone());
        let mut token = 0i64;
        unsafe {
            let weak_target = Rc::downgrade(&target);
            let _ = frame2.add_NavigationStarting(
                &FrameNavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                    if let (Some(target), Some(args)) = (weak_target.upgrade(), args) {
                        let mut uri = PWSTR::null();
                        let group = if args.Uri(&mut uri).is_ok() {
                            let wide = CoTaskMemPWSTR::from(uri);
                            if bounded_wide_len(uri, 4096).is_some() {
                                ocr::group_for_url(&wide.to_string())
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        target.group.set(group);
                        target.pending.set(false);
                    }
                    Ok(())
                })),
                &mut token,
            );
            let weak_tab = Rc::downgrade(self);
            let _ = frame2.add_DOMContentLoaded(
                &FrameDOMContentLoadedEventHandler::create(Box::new(move |_sender, _args| {
                    if let Some(tab) = weak_tab.upgrade() {
                        tab.kick();
                    }
                    Ok(())
                })),
                &mut token,
            );
            let weak_tab = Rc::downgrade(self);
            let weak_target = Rc::downgrade(&target);
            let _ = frame.add_Destroyed(
                &FrameDestroyedEventHandler::create(Box::new(move |_sender, _args| {
                    if let (Some(tab), Some(target)) = (weak_tab.upgrade(), weak_target.upgrade()) {
                        target.frame.borrow_mut().take();
                        tab.frames.borrow_mut().retain(|t| t.id != target.id);
                        tab.waiters.borrow_mut().retain(|w| w.target != target.id);
                    }
                    Ok(())
                })),
                &mut token,
            );
            // Frames inside this frame.
            if let Ok(frame7) = frame.cast::<ICoreWebView2Frame7>() {
                let weak_tab = Rc::downgrade(self);
                let _ = frame7.add_FrameCreated(
                    &FrameChildFrameCreatedEventHandler::create(Box::new(move |_sender, args| {
                        if let (Some(tab), Some(args)) = (weak_tab.upgrade(), args)
                            && let Ok(child) = args.Frame()
                        {
                            tab.track_frame(child);
                        }
                        Ok(())
                    })),
                    &mut token,
                );
            }
        }
    }

    fn find_target(&self, id: u32) -> Option<Rc<Target>> {
        if id == 0 {
            return Some(self.top.clone());
        }
        self.frames.borrow().iter().find(|t| t.id == id).cloned()
    }

    // ---- responses ----

    /// An image response arrived. Decides whether to read it, and reads it.
    fn on_response(self: &Rc<Self>, args: &ICoreWebView2WebResourceResponseReceivedEventArgs) -> Result<()> {
        if self.creds.borrow().is_none() || !setting_on() || ocr::engine_unavailable() {
            return Ok(());
        }
        let Some(top) = self.top_group.get() else { return Ok(()) };
        unsafe {
            let request = args.Request()?;
            let mut uri = PWSTR::null();
            request.Uri(&mut uri)?;
            let raw = if bounded_wide_len(uri, ocr::MAX_URL_LEN).is_some() {
                CoTaskMemPWSTR::from(uri).to_string()
            } else {
                drop(CoTaskMemPWSTR::from(uri));
                return Ok(());
            };
            let Some(url) = ocr::normalize_url(&raw) else { return Ok(()) };
            let Some(host_group) = ocr::group_for_url(&url) else { return Ok(()) };
            let _ = host_group; // any listed image host: the serve check compares groups per frame
            let key: Key = (top, url);
            // Seen already (pending, scanned, or settled): nothing to read.
            if cache_get(self.private, &key).is_some() {
                return Ok(());
            }
            let mut method = PWSTR::null();
            if request.Method(&mut method).is_ok() {
                let method = CoTaskMemPWSTR::from(method).to_string();
                if !method.eq_ignore_ascii_case("GET") {
                    return Ok(());
                }
            }
            let response = args.Response()?;
            let mut status = 0i32;
            response.StatusCode(&mut status)?;
            if status != 200 {
                return Ok(());
            }
            let headers = response.Headers()?;
            let content_type = header(&headers, w!("Content-Type")).unwrap_or_default().to_ascii_lowercase();
            if !content_type.starts_with("image/") || content_type.contains("svg") {
                return Ok(());
            }
            let length = header(&headers, w!("Content-Length")).and_then(|v| v.trim().parse::<u64>().ok());
            match length {
                Some(n) if n > ocr::MAX_IMAGE_BYTES as u64 => {
                    cache_put(self.private, key, Entry::done(Status::Skipped, ""));
                    return Ok(());
                }
                Some(n) if n < ocr::MIN_IMAGE_BYTES => {
                    cache_put(self.private, key, Entry::done(Status::Skipped, ""));
                    return Ok(());
                }
                _ => {}
            }
            cache_put(self.private, key.clone(), Entry::Pending);
            let private = self.private;
            let epoch = self.epoch.clone();
            let queued_at = epoch.load(Ordering::Relaxed);
            let weak = Rc::downgrade(self);
            let handler = WebResourceResponseViewGetContentCompletedHandler::create(Box::new(move |status, stream| {
                let bytes = if status.is_ok() { stream.and_then(read_image_stream) } else { None };
                let settled_key = key.clone();
                match bytes {
                    Some(bytes) if epoch.load(Ordering::Relaxed) == queued_at => {
                        submit(private, key.clone(), bytes, epoch.clone(), queued_at);
                    }
                    Some(_) => cache_remove(private, &settled_key),
                    None => cache_put(private, settled_key, Entry::done(Status::Skipped, "")),
                }
                if let Some(tab) = weak.upgrade() {
                    tab.kick();
                }
                Ok(())
            }));
            if response.GetContent(&handler).is_err() {
                // Not readable: settle it so a request is answered rather than held.
                // (The handler owns the key; rebuild it from the request.)
                if let Some(url) = ocr::normalize_url(&raw) {
                    cache_put(self.private, (top, url), Entry::done(Status::Skipped, ""));
                }
            }
        }
        Ok(())
    }

    // ---- the pump ----

    fn schedule_pump(self: &Rc<Self>, ms: u32) {
        if self.pump_scheduled.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        after(
            ms,
            Box::new(move || {
                if let Some(tab) = weak.upgrade() {
                    tab.pump_scheduled.set(false);
                    tab.pump();
                }
            }),
        );
    }

    /// A frame's last drain returned requests (held ones have their own timer).
    fn any_pending(&self) -> bool {
        self.top.pending.get() || self.frames.borrow().iter().any(|t| t.pending.get())
    }

    /// Hands a newly verified term update to the engine of the document already showing: the top frame
    /// and every tracked child frame, through the per-tab hook. Fire-and-forget; pages loaded later get
    /// the feed from the registered script. No-op without hooks (OCR install failed) or without a feed.
    pub(super) fn push_feed(self: &Rc<Self>) {
        let Some(creds) = self.creds.borrow().clone() else { return };
        let Some(lexicon) = crate::spoiler_feed::current_lexicon_json() else { return };
        let Some(script) = ocr::feed_script(&creds, &lexicon) else { return };
        let mut targets = vec![self.top.clone()];
        targets.extend(self.frames.borrow().iter().cloned());
        for target in targets {
            self.execute(&target, &script, Box::new(|_| {}));
        }
    }

    /// Drains every frame that may hold requests.
    fn pump(self: &Rc<Self>) {
        let Some(creds) = self.creds.borrow().clone() else { return };
        if !setting_on() || self.top_group.get().is_none() {
            return;
        }
        if !self.visible.get() {
            // No timers while hidden: becoming visible calls `kick`.
            return;
        }
        let Some(script) = ocr::drain_script(&creds) else { return };
        let mut targets = vec![self.top.clone()];
        targets.extend(self.frames.borrow().iter().cloned());
        let mut started = false;
        for target in targets {
            if target.inflight.replace(true) {
                continue;
            }
            started = true;
            let weak = Rc::downgrade(self);
            let handler_target = target.clone();
            self.execute(
                &target,
                &script,
                Box::new(move |raw| {
                    handler_target.inflight.set(false);
                    if let Some(tab) = weak.upgrade() {
                        tab.on_drain(&handler_target, raw);
                    }
                }),
            );
        }
        if !started && (self.any_pending() || self.tail.get() > 0) {
            self.schedule_pump(FOLLOW_UP_MS);
        }
    }

    /// `ExecuteScript` in a frame. `done` gets the raw JSON result, or `None` on any failure.
    fn execute(&self, target: &Target, script: &str, done: Box<dyn FnOnce(Option<String>)>) {
        let slot = Rc::new(RefCell::new(Some(done)));
        let completion = slot.clone();
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |status, value| {
            let answer = status.is_ok().then(|| value.to_string()).filter(|text| text.len() <= ocr::MAX_DRAIN_CHARS);
            if let Some(done) = completion.borrow_mut().take() {
                done(answer);
            }
            Ok(())
        }));
        let wide = CoTaskMemPWSTR::from(script);
        let result = unsafe {
            if target.id == 0 {
                match self.webview.borrow().as_ref() {
                    Some(webview) => webview.ExecuteScript(*wide.as_ref().as_pcwstr(), &handler),
                    None => Err(Error::from(windows::Win32::Foundation::E_POINTER)),
                }
            } else {
                match target.frame.borrow().as_ref() {
                    Some(frame) => frame.ExecuteScript(*wide.as_ref().as_pcwstr(), &handler),
                    None => Err(Error::from(windows::Win32::Foundation::E_POINTER)),
                }
            }
        };
        if result.is_err()
            && let Some(done) = slot.borrow_mut().take()
        {
            done(None);
        }
    }

    /// Takes a hostile drain result apart and answers or queues each request.
    fn on_drain(self: &Rc<Self>, target: &Rc<Target>, raw: Option<String>) {
        let items = raw.as_deref().map(ocr::parse_drain).unwrap_or_default();
        target.pending.set(!items.is_empty());
        if items.is_empty() {
            let tail = self.tail.get();
            if tail > 0 {
                self.tail.set(tail - 1);
                self.schedule_pump(FOLLOW_UP_MS);
            } else if self.any_pending() {
                self.schedule_pump(FOLLOW_UP_MS);
            }
            return;
        }
        let top = self.top_group.get();
        let frame_group = target.group.get();
        let mut now_answers: Vec<Delivery> = Vec::new();
        let mut hold: Vec<Waiter> = Vec::new();
        for item in items {
            let unavailable = |id: String| Delivery { id, status: Status::Unavailable, text: String::new() };
            let Some(url) = item.url else {
                now_answers.push(unavailable(item.id));
                continue;
            };
            let Some(top) = top else {
                now_answers.push(unavailable(item.id));
                continue;
            };
            if !setting_on() || ocr::engine_unavailable() || !ocr::may_serve(frame_group, ocr::group_for_url(&url)) {
                now_answers.push(unavailable(item.id));
                continue;
            }
            let key: Key = (top, url);
            match cache_get(self.private, &key) {
                Some(Entry::Done { status, text }) => now_answers.push(Delivery { id: item.id, status, text }),
                // Pending, or not seen yet (its response may still be on its way): hold it.
                pending => {
                    let seen = pending.is_some();
                    hold.push(Waiter {
                        target: target.id,
                        id: item.id,
                        key,
                        deadline: Instant::now() + if seen { Duration::from_secs(120) } else { UNSEEN_GRACE },
                    });
                }
            }
        }
        {
            let mut waiters = self.waiters.borrow_mut();
            for waiter in hold {
                if waiters.len() >= MAX_WAITERS {
                    now_answers.push(Delivery { id: waiter.id, status: Status::Unavailable, text: String::new() });
                } else {
                    waiters.push(waiter);
                }
            }
        }
        self.deliver(target, now_answers);
        self.arm_waiters();
        self.schedule_pump(FOLLOW_UP_MS);
    }

    // ---- waiting requests ----

    fn arm_waiters(self: &Rc<Self>) {
        if self.waiters.borrow().is_empty() || self.waiter_timer.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        after(
            WAITER_TICK_MS,
            Box::new(move || {
                if let Some(tab) = weak.upgrade() {
                    tab.waiter_timer.set(false);
                    tab.tick_waiters();
                }
            }),
        );
    }

    /// Answers every held request whose image is settled, and every unseen one past its grace.
    pub(super) fn tick_waiters(self: &Rc<Self>) {
        let now = Instant::now();
        let mut ready: HashMap<u32, Vec<Delivery>> = HashMap::new();
        self.waiters.borrow_mut().retain(|waiter| {
            let entry = cache_get(self.private, &waiter.key);
            let delivery = match entry {
                Some(Entry::Done { status, text }) => Some(Delivery { id: waiter.id.clone(), status, text }),
                Some(Entry::Pending) if now < waiter.deadline => None,
                None if now < waiter.deadline => None,
                _ => Some(Delivery { id: waiter.id.clone(), status: Status::Unavailable, text: String::new() }),
            };
            match delivery {
                Some(delivery) => {
                    ready.entry(waiter.target).or_default().push(delivery);
                    false
                }
                None => true,
            }
        });
        for (id, deliveries) in ready {
            if let Some(target) = self.find_target(id) {
                self.deliver(&target, deliveries);
            }
        }
        self.arm_waiters();
    }

    fn deliver(&self, target: &Target, deliveries: Vec<Delivery>) {
        if deliveries.is_empty() {
            return;
        }
        let Some(creds) = self.creds.borrow().clone() else { return };
        let Some(scripts) = ocr::delivery_scripts(&creds, &deliveries) else { return };
        for script in scripts {
            self.execute(target, &script, Box::new(|_| {}));
        }
    }
}

fn header(headers: &ICoreWebView2HttpResponseHeaders, name: PCWSTR) -> Option<String> {
    let mut value = PWSTR::null();
    unsafe { headers.GetHeader(name, &mut value) }.ok()?;
    if value.is_null() {
        return None;
    }
    if unsafe { bounded_wide_len(value, 256) }.is_none() {
        drop(CoTaskMemPWSTR::from(value));
        return None;
    }
    Some(CoTaskMemPWSTR::from(value).to_string())
}

/// Reads an image body, at most `MAX_IMAGE_BYTES`; `None` for a longer one.
fn read_image_stream(stream: IStream) -> Option<Vec<u8>> {
    let mut result = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let mut read = 0u32;
        unsafe { stream.Read(chunk.as_mut_ptr().cast(), chunk.len() as u32, Some(&mut read)) }.ok().ok()?;
        if read == 0 {
            break;
        }
        if result.len().saturating_add(read as usize) > ocr::MAX_IMAGE_BYTES {
            return None;
        }
        result.extend_from_slice(&chunk[..read as usize]);
    }
    Some(result)
}
