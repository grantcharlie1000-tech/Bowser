use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc::{SyncSender, sync_channel},
};

/// Includes captures waiting for WebView2, queued jobs, and the job being decoded.
const MAX_IN_FLIGHT: usize = 2;

#[derive(Default)]
struct Slots {
    in_flight: AtomicUsize,
}

pub(crate) struct CapturePermit<'a>(&'a Slots);

impl Slots {
    fn acquire(&self) -> Option<CapturePermit<'_>> {
        let mut used = self.in_flight.load(Ordering::Relaxed);
        while used < MAX_IN_FLIGHT {
            match self.in_flight.compare_exchange_weak(
                used,
                used + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(CapturePermit(self)),
                Err(current) => used = current,
            }
        }
        None
    }
}

impl Drop for CapturePermit<'_> {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(windows)]
pub(crate) fn reserve_capture() -> Option<CapturePermit<'static>> {
    static SLOTS: Slots = Slots {
        in_flight: AtomicUsize::new(0),
    };
    SLOTS.acquire()
}

type Job = Box<dyn FnOnce() + Send>;

struct Worker(SyncSender<Job>);

impl Worker {
    fn start() -> Option<Self> {
        let (sender, receiver) = sync_channel::<Job>(MAX_IN_FLIGHT);
        std::thread::Builder::new()
            .name("tab-thumbnail".into())
            .spawn(move || {
                #[cfg(windows)]
                {
                    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
                    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                        return;
                    }
                }
                for job in receiver {
                    job();
                }
                #[cfg(windows)]
                unsafe {
                    windows::Win32::System::Com::CoUninitialize();
                }
            })
            .ok()?;
        Some(Self(sender))
    }
}

/// Takes ownership of the bytes and permit. A failed enqueue drops the job and
/// its reply, which lets the owner thread settle the capture without a result.
#[cfg(windows)]
pub(crate) fn submit(permit: CapturePermit<'static>, job: impl FnOnce() + Send + 'static) {
    static WORKER: std::sync::OnceLock<Option<Worker>> = std::sync::OnceLock::new();
    if let Some(worker) = WORKER.get_or_init(Worker::start) {
        let _ = worker.0.try_send(Box::new(move || {
            let _permit = permit;
            job();
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc::channel, time::Duration};

    #[test]
    fn pending_and_running_captures_share_the_same_bound() {
        static SLOTS: Slots = Slots {
            in_flight: AtomicUsize::new(0),
        };
        let worker = Worker::start().unwrap();
        let (release, wait) = channel();
        let (started, running_now) = channel();
        let (finished, done) = channel();
        let running = SLOTS.acquire().unwrap();
        worker
            .0
            .try_send(Box::new(move || {
                started.send(()).unwrap();
                wait.recv().unwrap();
                drop(running);
                finished.send(()).unwrap();
            }))
            .unwrap_or_else(|_| panic!("could not queue the first capture"));
        running_now.recv_timeout(Duration::from_secs(5)).unwrap();

        let waiting_for_webview = SLOTS.acquire().unwrap();
        assert!(SLOTS.acquire().is_none());
        drop(waiting_for_webview);
        let queued = SLOTS.acquire().unwrap();
        let (finished, done_queued) = channel();
        worker
            .0
            .try_send(Box::new(move || {
                drop(queued);
                finished.send(()).unwrap();
            }))
            .unwrap_or_else(|_| panic!("could not queue the second capture"));

        assert!(SLOTS.acquire().is_none());
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
        done_queued.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(SLOTS.acquire().is_some());
    }
}
