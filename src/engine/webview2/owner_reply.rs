//! One-shot worker replies for the WebView2 owning thread. The worker carries only Send data.
//! The callback stays in the STA and can own a WebView2 interface or other thread-local state.

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    time::Duration,
};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, HWND_MESSAGE, KillTimer, PostMessageW, SetTimer,
                WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_TIMER,
            },
        },
    },
    core::{PCWSTR, w},
};

const REPLY_READY: u32 = WM_APP + 1;

#[derive(Default)]
struct Mailbox {
    hwnd: HWND,
    next: usize,
    pending: HashMap<usize, Box<dyn FnOnce()>>,
}

impl Drop for Mailbox {
    fn drop(&mut self) {
        if !self.hwnd.is_invalid() {
            let _ = unsafe { DestroyWindow(self.hwnd) };
        }
    }
}

thread_local! {
    static MAILBOX: RefCell<Mailbox> = RefCell::new(Mailbox::default());
}

unsafe extern "system" fn dispatch(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass: usize,
    _data: usize,
) -> LRESULT {
    if matches!(message, REPLY_READY | WM_TIMER) {
        let id = wparam.0;
        let _ = unsafe { KillTimer(Some(hwnd), id) };
        let callback = MAILBOX
            .try_with(|mailbox| mailbox.borrow_mut().pending.remove(&id))
            .ok()
            .flatten();
        if let Some(callback) = callback {
            callback();
        }
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

fn window() -> Option<HWND> {
    let hwnd = MAILBOX.with(|mailbox| mailbox.borrow().hwnd);
    if !hwnd.is_invalid() {
        return Some(hwnd);
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            PCWSTR::null(),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
    }
    .ok()?;
    if !unsafe { SetWindowSubclass(hwnd, Some(dispatch), 1, 0) }.as_bool() {
        let _ = unsafe { DestroyWindow(hwnd) };
        return None;
    }
    MAILBOX.with(|mailbox| mailbox.borrow_mut().hwnd = hwnd);
    Some(hwnd)
}

pub(super) struct Reply<T> {
    hwnd: isize,
    id: usize,
    finished: Arc<AtomicBool>,
    sender: Option<SyncSender<T>>,
}

impl<T> Reply<T> {
    pub(super) fn is_cancelled(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    pub(super) fn send(mut self, value: T) {
        if let Some(sender) = self.sender.take()
            && sender.try_send(value).is_ok()
        {
            self.wake();
        }
    }

    fn wake(&self) {
        // A one-shot deadline also drains the channel if the message queue cannot accept a post.
        let _ = unsafe {
            PostMessageW(
                Some(HWND(self.hwnd as _)),
                REPLY_READY,
                WPARAM(self.id),
                LPARAM(0),
            )
        };
    }
}

impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            drop(sender);
            self.wake();
        }
    }
}

/// A result, worker drop, or deadline consumes the callback once. `None` from `prepare` means
/// the caller still owns the fallback decision. Each caller supplies its own work/queue limit.
pub(super) fn prepare<T: Send + 'static>(
    timeout: Duration,
    receive: impl FnOnce(Option<T>) + 'static,
) -> Option<Reply<T>> {
    let hwnd = window()?;
    let id = MAILBOX.with(|mailbox| {
        let mut mailbox = mailbox.borrow_mut();
        mailbox.next = mailbox.next.checked_add(1)?;
        Some(mailbox.next)
    })?;
    let ms = timeout.as_millis().min(u128::from(u32::MAX)) as u32;
    if unsafe { SetTimer(Some(hwnd), id, ms, None) } == 0 {
        return None;
    }
    let (sender, receiver) = sync_channel(1);
    let finished = Arc::new(AtomicBool::new(false));
    let complete = Arc::clone(&finished);
    MAILBOX.with(|mailbox| {
        mailbox.borrow_mut().pending.insert(
            id,
            Box::new(move || {
                complete.store(true, Ordering::Relaxed);
                receive(receiver.try_recv().ok());
            }),
        );
    });
    Some(Reply {
        hwnd: hwnd.0 as isize,
        id,
        finished,
        sender: Some(sender),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc, thread, time::Instant};
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW};

    fn dispatch_ready_messages() {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, window(), 0, 0, PM_REMOVE) }.as_bool() {
            unsafe { DispatchMessageW(&message) };
        }
    }

    fn pump_until(done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !done() && Instant::now() < deadline {
            dispatch_ready_messages();
            thread::sleep(Duration::from_millis(1));
        }
        assert!(done());
    }

    #[test]
    fn worker_completion_runs_once_on_the_owner() {
        let value = Rc::new(Cell::new(None));
        let result = value.clone();
        let owner = thread::current().id();
        let reply = prepare(Duration::from_secs(5), move |value| {
            result.set(Some((thread::current().id(), value)));
        })
        .unwrap();
        thread::spawn(move || reply.send(42)).join().unwrap();
        pump_until(|| value.get().is_some());
        assert_eq!(value.get(), Some((owner, Some(42))));
    }

    #[test]
    fn deadline_and_worker_abandonment_complete_without_a_value() {
        for abandon in [true, false] {
            let values = Rc::new(RefCell::new(Vec::<Option<u32>>::new()));
            let result = values.clone();
            let timeout = if abandon {
                Duration::from_secs(5)
            } else {
                Duration::from_millis(15)
            };
            let mut reply = prepare(timeout, move |value| result.borrow_mut().push(value));
            assert!(reply.is_some());
            if abandon {
                thread::spawn({
                    let reply = reply.take().unwrap();
                    move || drop(reply)
                })
                .join()
                .unwrap();
            }
            pump_until(|| !values.borrow().is_empty());
            if let Some(reply) = reply {
                assert!(reply.is_cancelled());
                reply.send(42);
                dispatch_ready_messages();
            }
            assert_eq!(*values.borrow(), [None]);
        }
    }
}
