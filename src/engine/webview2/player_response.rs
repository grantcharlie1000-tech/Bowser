//! Only body copies and CDP commands run on the WebView2 owner. One bounded worker decodes
//! bodies, strips ad members, and builds the UTF-16 fulfilment parameters.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    thread,
    time::Duration,
};

use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
        ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl,
    },
};
use windows::core::{HRESULT, HSTRING, PCWSTR, Result, implement, w};

use super::{
    MAX_REWRITE_BODY_BYTES, call_devtools, devtools_protocol_error, fetch_continue, fulfill_params,
    json_string_literal, owner_reply, response_body_text,
};
use crate::blocker::youtube::{PlayerPayload, strip_player_ads};

// This includes reads in flight, queued jobs and the running job, across all WebViews.
const MAX_ADMITTED: usize = 2;
// CDP returns a JSON envelope with either base64 or JSON string escaping. A very heavily
// escaped envelope may exceed this limit even if its decoded body would fit; it passes through.
const MAX_ENVELOPE_UNITS: usize = MAX_REWRITE_BODY_BYTES * 2;
const REWRITE_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(super) struct Responses {
    stopped: Cell<bool>,
    navigation: Cell<Option<u64>>,
    pending: RefCell<HashMap<String, Rc<Paused>>>,
}

impl Responses {
    pub(super) fn start_navigation(&self, id: Option<u64>) {
        let previous = self.navigation.replace(id);
        if id.is_none() || previous != id {
            self.cancel_all();
        }
    }

    pub(super) fn cancel_all(&self) {
        // COM can reenter. Leave already submitted fulfilments tracked until CDP acknowledges
        // them, and do not hold the map borrow when issuing a continuation.
        let pending: Vec<_> = self.pending.borrow().values().cloned().collect();
        for pause in pending {
            pause.resolve(None);
        }
    }

    pub(super) fn stop(&self, webview: Option<&ICoreWebView2>) {
        self.stopped.set(true);
        let outstanding = !self.pending.borrow().is_empty();
        self.cancel_all();
        self.pending.borrow_mut().clear();
        if outstanding && let Some(webview) = webview {
            // Fetch detach releases any still-paused jobs before the controller is closed. A
            // job for which Chromium accepted a terminal command is already absent from that set.
            call_devtools(webview, "Fetch.disable", "{}");
        }
    }

    pub(super) fn start(
        self: &Rc<Self>,
        webview: &ICoreWebView2,
        id: &str,
        headers: Vec<(String, String)>,
        payload: PlayerPayload,
    ) {
        if self.pending.borrow().contains_key(id) {
            return;
        }
        let worker = if self.stopped.get() {
            None
        } else {
            Worker::get()
        };
        let Some((worker, lease)) =
            worker.and_then(|worker| Lease::take(&worker.admitted).map(|lease| (worker, lease)))
        else {
            fetch_continue(webview, id);
            return;
        };
        let pause = Rc::new(Paused {
            webview: webview.clone(),
            id: id.to_owned(),
            responses: Rc::downgrade(self),
            state: Cell::new(Resolution::Pending),
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        self.pending
            .borrow_mut()
            .insert(id.to_owned(), pause.clone());
        let weak = Rc::downgrade(&pause);
        let reply = owner_reply::prepare(REWRITE_DEADLINE, move |params| {
            if let Some(pause) = weak.upgrade() {
                pause.resolve(params);
            }
        });
        let Some(reply) = reply else {
            pause.resolve(None);
            return;
        };
        if pause.state.get() != Resolution::Pending {
            return;
        }
        let weak = Rc::downgrade(&pause);
        let completed = BodyCompleted(RefCell::new(Some(Box::new(move |status, result| {
            let Some(pause) = weak.upgrade() else {
                return;
            };
            if pause.state.get() != Resolution::Pending {
                return;
            }
            if status.is_ok() && !reply.is_cancelled()
                // SAFETY: the callback owns the borrow of this null-terminated string.
                && let Some(source) = unsafe { bounded_wide(&result, MAX_ENVELOPE_UNITS) }
            {
                let job = Job {
                    lease,
                    source: source.to_vec(),
                    payload,
                    headers,
                    id: pause.id.clone(),
                    cancelled: pause.cancelled.clone(),
                    reply,
                };
                if worker.jobs.try_send(job).is_ok() {
                    return;
                }
            }
            pause.resolve(None);
        }))));
        let completed: ICoreWebView2CallDevToolsProtocolMethodCompletedHandler = completed.into();
        let params = HSTRING::from(format!("{{\"requestId\":{}}}", json_string_literal(id)));
        let queued = unsafe {
            webview.CallDevToolsProtocolMethod(
                w!("Fetch.getResponseBody"),
                PCWSTR(params.as_ptr()),
                &completed,
            )
        };
        if queued.is_err() {
            pause.resolve(None);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Resolution {
    Pending,
    Fulfilling,
    Finished,
}

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Fulfill,
    Continue,
}

impl Resolution {
    fn resolve(&mut self, rewritten: bool) -> Option<Command> {
        if *self != Self::Pending {
            return None;
        }
        if rewritten {
            *self = Self::Fulfilling;
            Some(Command::Fulfill)
        } else {
            *self = Self::Finished;
            Some(Command::Continue)
        }
    }

    fn fulfilled(&mut self, accepted: bool) -> Option<Command> {
        if *self != Self::Fulfilling {
            return None;
        }
        *self = Self::Finished;
        (!accepted).then_some(Command::Continue)
    }
}

struct Paused {
    webview: ICoreWebView2,
    id: String,
    responses: Weak<Responses>,
    state: Cell<Resolution>,
    cancelled: Arc<AtomicBool>,
}

impl Paused {
    fn resolve(self: &Rc<Self>, rewrite: Option<Rewrite>) {
        let mut state = self.state.get();
        let command = state.resolve(rewrite.is_some());
        self.state.set(state);
        match command {
            Some(Command::Continue) => {
                self.cancelled.store(true, Ordering::Relaxed);
                self.continue_request();
            }
            Some(Command::Fulfill) => {
                self.fulfill(rewrite.expect("rewrite parameters"));
            }
            None => {}
        }
    }

    fn forget(&self) {
        if let Some(responses) = self.responses.upgrade() {
            let mut pending = responses.pending.borrow_mut();
            if pending
                .get(&self.id)
                .is_some_and(|current| std::ptr::eq(current.as_ref(), self))
            {
                pending.remove(&self.id);
            }
        }
    }

    fn continue_request(&self) {
        self.forget();
        fetch_continue(&self.webview, &self.id);
    }

    fn fulfill(self: &Rc<Self>, rewrite: Rewrite) {
        let Rewrite { lease, params } = rewrite;
        let pause = self.clone();
        let completed =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, body| {
                pause.fulfilled(status.is_ok() && !devtools_protocol_error(&body));
                drop(lease);
                Ok(())
            }));
        let queued = unsafe {
            self.webview.CallDevToolsProtocolMethod(
                w!("Fetch.fulfillRequest"),
                PCWSTR(params.as_ptr()),
                &completed,
            )
        };
        if queued.is_err() {
            self.fulfilled(false);
        }
    }

    fn fulfilled(&self, accepted: bool) {
        let mut state = self.state.get();
        let command = state.fulfilled(accepted);
        self.state.set(state);
        if command == Some(Command::Continue) {
            self.continue_request();
        } else if state == Resolution::Finished {
            self.forget();
        }
    }
}

// webview2-com's general completed handler eagerly allocates a Rust String before invoking its
// closure. Here inspect the borrowed string first and copy only an admitted, bounded body.
type BodyCallback = Box<dyn FnOnce(HRESULT, PCWSTR)>;

#[implement(ICoreWebView2CallDevToolsProtocolMethodCompletedHandler)]
struct BodyCompleted(RefCell<Option<BodyCallback>>);

impl ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl for BodyCompleted_Impl {
    fn Invoke(&self, status: HRESULT, result: &PCWSTR) -> Result<()> {
        let done = self.0.borrow_mut().take();
        if let Some(done) = done {
            done(status, *result);
        }
        Ok(())
    }
}

/// The caller keeps `result` valid through the first nul for the life of the returned slice.
pub(super) unsafe fn bounded_wide(result: &PCWSTR, limit: usize) -> Option<&[u16]> {
    if result.is_null() {
        return None;
    }
    let start = result.as_ptr();
    let len = (0..=limit).find(|&len| unsafe { *start.add(len) == 0 })?;
    Some(unsafe { std::slice::from_raw_parts(start, len) })
}

struct Job {
    lease: Lease,
    source: Vec<u16>,
    payload: PlayerPayload,
    headers: Vec<(String, String)>,
    id: String,
    cancelled: Arc<AtomicBool>,
    reply: owner_reply::Reply<Rewrite>,
}

struct Rewrite {
    lease: Lease,
    params: Vec<u16>,
}

impl Job {
    fn run(self) {
        if self.cancelled.load(Ordering::Relaxed) || self.reply.is_cancelled() {
            return;
        }
        let text = String::from_utf16(&self.source).ok();
        drop(self.source);
        let rewritten = text
            .as_deref()
            .and_then(|result| response_body_text(result, MAX_REWRITE_BODY_BYTES))
            .and_then(|body| strip_player_ads(&body, self.payload));
        drop(text);
        if let Some(body) = rewritten
            && !self.cancelled.load(Ordering::Relaxed)
            && !self.reply.is_cancelled()
        {
            let params = fulfill_params(&self.id, 200, &self.headers, body.as_bytes());
            // Finish even the UTF-16 conversion here so CDP fulfilment needs no UI-thread copy.
            self.reply.send(Rewrite {
                lease: self.lease,
                params: params.encode_utf16().chain([0]).collect(),
            });
        }
    }
}

struct Worker {
    jobs: SyncSender<Job>,
    admitted: Arc<AtomicUsize>,
}

impl Worker {
    fn get() -> Option<&'static Self> {
        static WORKER: OnceLock<Option<Worker>> = OnceLock::new();
        WORKER
            .get_or_init(|| {
                let (jobs, receive) = sync_channel::<Job>(MAX_ADMITTED);
                thread::Builder::new()
                    .name("bowser-player-response".to_owned())
                    .spawn(move || {
                        while let Ok(job) = receive.recv() {
                            job.run();
                        }
                    })
                    .ok()?;
                Some(Self {
                    jobs,
                    admitted: Arc::new(AtomicUsize::new(0)),
                })
            })
            .as_ref()
    }
}

struct Lease(Arc<AtomicUsize>);

impl Lease {
    fn take(admitted: &Arc<AtomicUsize>) -> Option<Self> {
        if admitted.fetch_add(1, Ordering::Relaxed) >= MAX_ADMITTED {
            admitted.fetch_sub(1, Ordering::Relaxed);
            return None;
        }
        Some(Self(admitted.clone()))
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_or_failed_work_continues_once_and_ignores_late_results() {
        let mut resolution = Resolution::Pending;
        assert_eq!(resolution.resolve(false), Some(Command::Continue));
        assert_eq!(resolution.resolve(true), None);
        assert_eq!(resolution.fulfilled(false), None);
    }

    #[test]
    fn a_submitted_fulfilment_falls_back_only_if_rejected() {
        for accepted in [true, false] {
            let mut resolution = Resolution::Pending;
            assert_eq!(resolution.resolve(true), Some(Command::Fulfill));
            assert_eq!(
                resolution.resolve(false),
                None,
                "navigation or closure during fulfilment"
            );
            assert_eq!(
                resolution.fulfilled(accepted),
                (!accepted).then_some(Command::Continue)
            );
            assert_eq!(resolution.fulfilled(false), None);
        }
    }

    #[test]
    fn reads_and_jobs_share_the_same_bounded_admission() {
        let admitted = Arc::new(AtomicUsize::new(0));
        let first = Lease::take(&admitted).unwrap();
        let second = Lease::take(&admitted).unwrap();
        assert!(Lease::take(&admitted).is_none());
        drop(first);
        assert!(Lease::take(&admitted).is_some());
        drop(second);
        assert_eq!(admitted.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_cdp_string_must_terminate_within_the_copy_limit() {
        let units = [65, 66, 67, 0];
        let value = PCWSTR(units.as_ptr());
        // SAFETY: the array stays live and includes a terminator.
        assert!(unsafe { bounded_wide(&value, 2) }.is_none());
        assert_eq!(unsafe { bounded_wide(&value, 3) }, Some(&units[..3]));
        let null = PCWSTR::null();
        assert!(unsafe { bounded_wide(&null, 3) }.is_none());
    }
}
