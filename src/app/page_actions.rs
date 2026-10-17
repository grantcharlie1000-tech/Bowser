//! Page tools, the app's side (thread T7): applying the settings, reading a page's selection, putting a
//! note into a text field, and capturing the page.
//!
//! A capture is a chain of completions, never a wait: the engine is asked for the page's size and then
//! for one picture per slice (`devtools_call`, which only ever runs two DevTools methods on an http(s)
//! page), and the pieces are joined and saved when the last arrives. Every step checks that the tab is
//! still on the page the capture started on, so a navigation or a closed tab ends it quietly. Only one
//! capture runs at a time. The window turns the result into a toast, a clipboard bitmap or a note.

use std::{cell::{Cell, RefCell}, path::PathBuf, rc::Rc};

use super::{App, AppRef};
use crate::{
    capture::{self, Clip},
    engine::EngineAdapter,
    page_tools::{self, CaptureFormat, CaptureMode, CaptureTarget},
    tabs::TabId,
};

/// What a capture came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureResult {
    /// Saved to a file. `cut`: the page was longer or wider than a capture covers.
    Saved { path: PathBuf, cut: bool },
    /// A bottom-up 24-bit bitmap for the clipboard.
    Clipboard { dib: Vec<u8>, cut: bool },
    /// Saved to a file the new note points at.
    Note { path: PathBuf, page_title: String, addr: String, cut: bool },
    NothingSelected,
    /// Not a web page, or no live page.
    Unavailable,
    /// Another capture is running.
    Busy,
    Failed(String),
}

/// What the page has selected, as a note wants it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selected {
    Text { text: String, title: String, addr: String },
    Nothing,
    Unavailable,
}

thread_local! {
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

struct Job {
    tab: TabId,
    mode: CaptureMode,
    target: CaptureTarget,
    format: CaptureFormat,
    url: String,
    host: String,
    title: String,
    addr: String,
    slices: Vec<Vec<u8>>,
    plan: Vec<(u32, u32)>,
    width: u32,
    cut: bool,
    done: Rc<dyn Fn(CaptureResult)>,
}

/// Whether a capture is running (the next one is refused until it ends).
pub fn capture_busy() -> bool {
    BUSY.with(Cell::get)
}

type Answer = Box<dyn FnOnce(Option<String>)>;

impl App {
    /// Applies Settings > Page tools: what the engine reads for new tabs, every open tab, and the
    /// reader's session defaults.
    pub fn apply_page_tools_settings(app: &AppRef, settings: &crate::settings::Settings) {
        {
            let mut state = app.borrow_mut();
            let next = &settings.page_tools;
            if state.settings.page_tools.reader_prefs() != next.reader_prefs() {
                state.reader_prefs = next.reader_prefs();
            }
            state.settings.page_tools = next.clone();
        }
        if page_tools::apply_runtime(&settings.page_tools) {
            Self::call_engine(app, |engine| engine.apply_page_tools());
        }
    }

    /// Runs an engine call whose answer comes back as text. `done` always runs with the app unborrowed,
    /// also for an engine that answers at once.
    fn page_answer(app: &AppRef, call: impl FnOnce(&mut dyn EngineAdapter, Answer), done: Answer) {
        let done = Rc::new(RefCell::new(Some(done)));
        let early = Rc::new(RefCell::new(None::<Option<String>>));
        let in_call = Rc::new(Cell::new(true));
        let answer = {
            let (done, early, in_call) = (done.clone(), early.clone(), in_call.clone());
            Box::new(move |text: Option<String>| {
                if in_call.get() {
                    *early.borrow_mut() = Some(text);
                } else if let Some(done) = done.borrow_mut().take() {
                    done(text);
                }
            }) as Answer
        };
        let called = Self::call_engine(app, |engine| call(engine, answer));
        in_call.set(false);
        let text = match (called, early.borrow_mut().take()) {
            (None, _) => Some(None),
            (Some(()), early) => early,
        };
        if let Some(text) = text
            && let Some(done) = done.borrow_mut().take()
        {
            done(text);
        }
    }

    /// The selected text of the page in `tab`, with its title and address.
    pub fn read_selection(app: &AppRef, tab: TabId, done: Box<dyn FnOnce(Selected)>) {
        Self::page_answer(
            app,
            |engine, answer| engine.run_page_script(tab, capture::SELECTION_SCRIPT, capture::SELECTION_ANSWER_LIMIT, answer),
            Box::new(move |raw| {
                done(match raw.as_deref().map(capture::parse_selection) {
                    Some(Ok(Some(found))) if !found.text.trim().is_empty() => {
                        let addr = url::Url::parse(&found.address).ok().filter(|u| matches!(u.scheme(), "http" | "https")).map(|u| u.to_string()).unwrap_or_default();
                        Selected::Text { text: found.text, title: found.title, addr }
                    }
                    Some(Ok(_)) => Selected::Nothing,
                    _ => Selected::Unavailable,
                });
            }),
        );
    }

    /// The script that puts `text` where the cursor is in the focused text field (never a password field).
    pub fn insert_script(text: &str) -> String {
        format!(
            "(function(t){{var e=document.activeElement;if(!e||e.type==='password')return false;try{{if(e.isContentEditable||e.tagName==='INPUT'||e.tagName==='TEXTAREA'){{if(!document.execCommand('insertText',false,t)&&typeof e.setRangeText==='function'){{e.setRangeText(t,e.selectionStart,e.selectionEnd,'end');e.dispatchEvent(new Event('input',{{bubbles:true}}));}}return true;}}}}catch(x){{}}return false;}})({})",
            crate::chrome_host::protocol::json_string(text)
        )
    }

    /// Types `text` into the text field the page has focused, when the tab shows a web page.
    pub fn insert_text(app: &AppRef, tab: TabId, text: &str) -> bool {
        let Some(url) = Self::tab_committed_url(app, tab).filter(crate::reader::url_allowed) else { return false };
        let Some(origin) = crate::permissions::origin_of(url.as_str()) else { return false };
        let script = Self::insert_script(text);
        Self::call_engine(app, |engine| engine.execute_script_for_origin(tab, &origin, &script).is_ok()).unwrap_or(false)
    }

    /// Captures the page in `tab`. `done` runs once, with the app unborrowed, when it is over.
    pub fn capture_page(app: &AppRef, tab: TabId, mode: CaptureMode, target: CaptureTarget, done: Rc<dyn Fn(CaptureResult)>) {
        let (url, title, format) = {
            let state = app.borrow();
            let Some(entry) = state.tabs.iter().find(|entry| entry.id == tab) else { drop(state); return done(CaptureResult::Unavailable) };
            (entry.url.clone(), entry.title.clone(), state.settings.page_tools.capture_format)
        };
        let Some(url) = url.filter(crate::reader::url_allowed) else { return done(CaptureResult::Unavailable) };
        if BUSY.with(|busy| busy.replace(true)) {
            return done(CaptureResult::Busy);
        }
        let job = Rc::new(RefCell::new(Job {
            tab,
            mode,
            target,
            format,
            url: url.to_string(),
            host: url.host_str().unwrap_or("").to_owned(),
            title,
            addr: url.to_string(),
            slices: Vec::new(),
            plan: Vec::new(),
            width: 0,
            cut: false,
            done,
        }));
        match mode {
            CaptureMode::Visible => Self::shoot(app, &job, None),
            CaptureMode::FullPage => {
                let weak = job.clone();
                let app2 = app.clone();
                Self::page_answer(
                    app,
                    |engine, answer| engine.devtools_call(tab, capture::METHOD_METRICS, "{}", answer),
                    Box::new(move |raw| {
                        if !Self::capture_alive(&app2, &weak) {
                            return Self::capture_end(&weak, CaptureResult::Failed("The page changed before it could be captured.".into()));
                        }
                        let Some((w, h, cut)) = raw.as_deref().and_then(capture::parse_metrics).and_then(|(w, h)| page_tools::clamp_page(w, h)) else {
                            return Self::capture_end(&weak, CaptureResult::Failed("The page's size could not be read.".into()));
                        };
                        {
                            let mut job = weak.borrow_mut();
                            job.width = w;
                            job.cut = cut;
                            job.plan = page_tools::plan_slices(h);
                            job.plan.reverse(); // popped from the end, so top first
                        }
                        Self::next_slice(&app2, &weak);
                    }),
                );
            }
            CaptureMode::Selection => {
                let weak = job.clone();
                let app2 = app.clone();
                Self::page_answer(
                    app,
                    |engine, answer| engine.run_page_script(tab, capture::SELECTION_SCRIPT, capture::SELECTION_ANSWER_LIMIT, answer),
                    Box::new(move |raw| {
                        if !Self::capture_alive(&app2, &weak) {
                            return Self::capture_end(&weak, CaptureResult::Failed("The page changed before it could be captured.".into()));
                        }
                        match raw.as_deref().map(capture::parse_selection) {
                            Some(Ok(Some(found))) => {
                                let (x, y, w, h) = found.rect;
                                let Some((x, y, w, h)) = page_tools::clamp_selection(x as f64, y as f64, w as f64, h as f64) else {
                                    return Self::capture_end(&weak, CaptureResult::NothingSelected);
                                };
                                Self::shoot(&app2, &weak, Some(Clip { x, y, width: w, height: h }));
                            }
                            Some(Ok(None)) => Self::capture_end(&weak, CaptureResult::NothingSelected),
                            _ => Self::capture_end(&weak, CaptureResult::Unavailable),
                        }
                    }),
                );
            }
        }
    }

    fn capture_alive(app: &AppRef, job: &Rc<RefCell<Job>>) -> bool {
        let job = job.borrow();
        Self::tab_committed_url(app, job.tab).is_some_and(|url| url.as_str() == job.url)
    }

    /// Takes the picture of `clip` (the visible area when `None`) and finishes.
    fn shoot(app: &AppRef, job: &Rc<RefCell<Job>>, clip: Option<Clip>) {
        let (tab, format) = {
            let job = job.borrow();
            (job.tab, job.format)
        };
        let params = capture::shot_params(format, clip);
        let (weak, app2) = (job.clone(), app.clone());
        Self::page_answer(
            app,
            |engine, answer| engine.devtools_call(tab, capture::METHOD_SHOT, &params, answer),
            Box::new(move |raw| {
                if !Self::capture_alive(&app2, &weak) {
                    return Self::capture_end(&weak, CaptureResult::Failed("The page changed before it could be captured.".into()));
                }
                match raw.as_deref().and_then(capture::parse_shot) {
                    Some(bytes) => {
                        weak.borrow_mut().slices.push(bytes);
                        Self::capture_finish(&app2, &weak);
                    }
                    None => Self::capture_end(&weak, CaptureResult::Failed("The browser could not capture this page.".into())),
                }
            }),
        );
    }

    /// Captures the next slice of a full page, or finishes when there is none left.
    fn next_slice(app: &AppRef, job: &Rc<RefCell<Job>>) {
        let next = job.borrow_mut().plan.pop();
        let Some((top, height)) = next else { return Self::capture_finish(app, job) };
        let (tab, format, width) = {
            let job = job.borrow();
            (job.tab, job.format, job.width)
        };
        let params = capture::shot_params(format, Some(Clip { x: 0, y: top, width, height }));
        let (weak, app2) = (job.clone(), app.clone());
        Self::page_answer(
            app,
            |engine, answer| engine.devtools_call(tab, capture::METHOD_SHOT, &params, answer),
            Box::new(move |raw| {
                if !Self::capture_alive(&app2, &weak) {
                    return Self::capture_end(&weak, CaptureResult::Failed("The page changed before it could be captured.".into()));
                }
                match raw.as_deref().and_then(capture::parse_shot) {
                    Some(bytes) => {
                        weak.borrow_mut().slices.push(bytes);
                        Self::next_slice(&app2, &weak);
                    }
                    None => Self::capture_end(&weak, CaptureResult::Failed("The browser could not capture the whole page.".into())),
                }
            }),
        );
    }

    fn capture_end(job: &Rc<RefCell<Job>>, result: CaptureResult) {
        BUSY.with(|busy| busy.set(false));
        let done = job.borrow().done.clone();
        done(result);
    }

    /// Joins the slices and sends the picture where it was asked to go.
    fn capture_finish(app: &AppRef, job: &Rc<RefCell<Job>>) {
        let (settings, format, slices, mode, target, host, title, addr, cut) = {
            let mut job = job.borrow_mut();
            (
                app.borrow().settings.page_tools.clone(),
                job.format,
                std::mem::take(&mut job.slices),
                job.mode,
                job.target,
                job.host.clone(),
                job.title.clone(),
                job.addr.clone(),
                job.cut,
            )
        };
        let bytes = match capture::stitch(&slices, format) {
            Ok(bytes) => bytes,
            Err(message) => return Self::capture_end(job, CaptureResult::Failed(message)),
        };
        let result = match target {
            CaptureTarget::Clipboard => match capture::dib_of(&bytes) {
                Ok(dib) => CaptureResult::Clipboard { dib, cut },
                Err(message) => CaptureResult::Failed(message),
            },
            CaptureTarget::File | CaptureTarget::Note => {
                let Some(dir) = capture::resolve_dir(&settings.capture_dir) else {
                    return Self::capture_end(job, CaptureResult::Failed("There is no folder to save captures in.".into()));
                };
                let stem = page_tools::capture_stem(&settings.capture_name, &host, &title, mode, page_tools::Stamp::now());
                match capture::save_file(&dir, &stem, format, &bytes) {
                    Ok(path) if target == CaptureTarget::Note => CaptureResult::Note { path, page_title: title, addr, cut },
                    Ok(path) => CaptureResult::Saved { path, cut },
                    Err(message) => CaptureResult::Failed(message),
                }
            }
        };
        Self::capture_end(job, result);
    }
}
