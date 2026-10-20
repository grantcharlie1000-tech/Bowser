//! Bounded, request-local presentation of an existing WebView2 engine menu.
//!
//! This module does not register handlers, acquire deferrals, mark a request handled,
//! create engine items, dispatch commands, invoke listeners, or grant permissions.
//! It is used only by the default-off experimental ordinary-window coordinator. That caller must keep the
//! original request/deferral alive and enforce its owner, document, profile, view,
//! request-generation and consent fences before showing and again before dispatch.
//! Move a pending request OUT of any `RefCell` before calling any method here:
//! COM calls, menu destruction and the modal menu loop can reenter the application.
//!
//! Admission is all-or-nothing. Text, order, separators, submenus, disabled state,
//! checkbox/radio state, engine accelerator ampersands and shortcut descriptions
//! are retained. Icons are NOT implemented: a non-null Icon rejects the entire
//! tree rather than silently dropping an engine-provided visual. No labels or
//! names are interpreted as authority or logged; display text may contain URLs.
//!
//! Contracts: Microsoft Learn's ICoreWebView2ContextMenuItem reference documents
//! CoTaskMemFree ownership, accelerator ampersands and ShortcutKeyDescription:
//! https://learn.microsoft.com/microsoft-edge/webview2/reference/win32/icorewebview2contextmenuitem
//! TrackPopupMenu with TPM_RETURNCMD returns zero for cancellation OR failure:
//! https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-trackpopupmenu

use std::{cell::Cell, collections::HashSet, marker::PhantomData, rc::Rc};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND, ICoreWebView2ContextMenuItem,
    ICoreWebView2ContextMenuItemCollection,
};
use windows::{
    Win32::{
        Foundation::{HWND, POINT},
        System::{Com::CoTaskMemFree, Threading::GetCurrentThreadId},
        UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, GetForegroundWindow,
            GetWindowThreadProcessId, HMENU, IsWindow, MENUITEMINFOW, MF_CHECKED,
            MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MFT_RADIOCHECK, MIIM_FTYPE,
            SetMenuItemInfoW, TPM_NONOTIFY, TPM_RETURNCMD, TrackPopupMenu,
        },
    },
    core::{BOOL, IUnknown, Interface, PCWSTR, PWSTR},
};

// Declared bounds apply to capture AND every subsequent validation. The root's
// items have depth one; no item may have depth greater than six. A depth-six
// submenu may be empty, but may not have any children. Never truncate to succeed.
pub(crate) const MAX_LABEL_UTF16_UNITS: usize = 512;
pub(crate) const MAX_SHORTCUT_UTF16_UNITS: usize = 512;
pub(crate) const MAX_MENU_NODES: usize = 256;
pub(crate) const MAX_MENU_DEPTH: usize = 6;

/// Safe to report: categories and numeric HRESULTs only, never engine text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuError {
    Getter(i32),
    Native(i32),
    NullString,
    InvalidUtf16,
    TextLimit,
    NodeLimit,
    DepthLimit,
    UnknownKind,
    InvalidCommandId,
    DuplicateCommandId,
    UnsupportedIcon,
    SnapshotChanged,
    NotAnEnabledLeaf,
    UnexpectedState,
    WrongThread,
    InvalidOwner,
}

type Result<T> = std::result::Result<T, MenuError>;

fn getter<T>(value: windows::core::Result<T>) -> Result<T> {
    value.map_err(|error| MenuError::Getter(error.code().0))
}

fn native<T>(value: windows::core::Result<T>) -> Result<T> {
    value.map_err(|error| MenuError::Native(error.code().0))
}

/// Holds the allocation even if the getter fails after writing its out pointer.
struct TaskText(PWSTR);

impl Drop for TaskText {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CoTaskMemFree(Some(self.0.0.cast_const().cast())) };
        }
    }
}

/// Read no more than `max_units + 1` units, including the terminator probe.
/// WebView2 owns the pointer validity contract; this is not an arbitrary pointer
/// reader. A bounded scan prevents an unbounded `PWSTR::to_string` walk.
fn task_text(
    max_units: usize,
    read: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>,
) -> Result<String> {
    let mut allocation = TaskText(PWSTR::null());
    getter(read(&mut allocation.0))?;
    if allocation.0.is_null() {
        return Err(MenuError::NullString);
    }
    let mut units = Vec::with_capacity(max_units);
    for index in 0..=max_units {
        let unit = unsafe { allocation.0.0.add(index).read() };
        if unit == 0 {
            return decode_utf16(&units);
        }
        if index == max_units {
            return Err(MenuError::TextLimit);
        }
        units.push(unit);
    }
    unreachable!("the bounded scan always finds a terminator or rejects")
}

fn decode_utf16(units: &[u16]) -> Result<String> {
    String::from_utf16(units).map_err(|_| MenuError::InvalidUtf16)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Command,
    CheckBox,
    Radio,
    Separator,
    Submenu,
}

impl Kind {
    fn from_engine(value: i32) -> Result<Self> {
        match value {
            0 => Ok(Self::Command),
            1 => Ok(Self::CheckBox),
            2 => Ok(Self::Radio),
            3 => Ok(Self::Separator),
            4 => Ok(Self::Submenu),
            _ => Err(MenuError::UnknownKind),
        }
    }

    fn is_leaf(self) -> bool {
        matches!(self, Self::Command | Self::CheckBox | Self::Radio)
    }

    fn has_check_state(self) -> bool {
        matches!(self, Self::CheckBox | Self::Radio)
    }
}

// Do not derive Debug: these values contain untrusted, potentially private text.
#[derive(Eq, PartialEq)]
struct Properties {
    kind: Kind,
    command_id: i32,
    enabled: bool,
    checked: Option<bool>,
    label: String,
    shortcut: String,
}

/// The generated Icon() wrapper turns S_OK + null into an error. Use its pinned
/// vtable signature to distinguish successful absence from a failing getter.
/// Release any returned interface even on a failed getter. No stream is decoded.
fn require_no_icon(item: &ICoreWebView2ContextMenuItem) -> Result<()> {
    let mut raw = std::ptr::null_mut();
    let status = unsafe { (item.vtable().Icon)(item.as_raw(), &mut raw) };
    let icon = if raw.is_null() {
        None
    } else {
        // IStream inherits IUnknown. Its first three vtable entries implement
        // IUnknown, so this owning wrapper releases the returned reference.
        Some(unsafe { IUnknown::from_raw(raw) })
    };
    getter(status.ok())?;
    if icon.is_some() {
        return Err(MenuError::UnsupportedIcon);
    }
    Ok(())
}

fn properties(item: &ICoreWebView2ContextMenuItem) -> Result<Properties> {
    let mut kind = COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND(0);
    getter(unsafe { item.Kind(&mut kind) })?;
    let kind = Kind::from_engine(kind.0)?;
    let mut command_id = 0;
    getter(unsafe { item.CommandId(&mut command_id) })?;
    let mut enabled = BOOL::default();
    getter(unsafe { item.IsEnabled(&mut enabled) })?;
    // IsChecked is documented only for checkboxes/radio items. Do not make an
    // invalid getter call on a separator, command or submenu.
    let checked = if kind.has_check_state() {
        let mut checked = BOOL::default();
        getter(unsafe { item.IsChecked(&mut checked) })?;
        Some(checked.as_bool())
    } else {
        None
    };
    let label = task_text(MAX_LABEL_UTF16_UNITS, |out| unsafe { item.Label(out) })?;
    let shortcut = task_text(MAX_SHORTCUT_UTF16_UNITS, |out| unsafe {
        item.ShortcutKeyDescription(out)
    })?;
    require_no_icon(item)?;
    Ok(Properties {
        kind,
        command_id,
        enabled: enabled.as_bool(),
        checked,
        label,
        shortcut,
    })
}

struct Node {
    item: ICoreWebView2ContextMenuItem,
    identity: IUnknown,
    value: Properties,
    children: Option<Tree>,
}

/// Every collection and every actual engine item remains retained. The tree is
/// private and never mutated after capture; only the engine's live COM state can
/// change. Canonical IUnknown identity detects a replacement with identical text.
struct Tree {
    source: ICoreWebView2ContextMenuItemCollection,
    identity: IUnknown,
    nodes: Vec<Node>,
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    leaf_ids: HashSet<i32>,
}

impl Budget {
    fn collection(&self, count: u32, depth: usize) -> Result<()> {
        if count != 0 && depth > MAX_MENU_DEPTH {
            return Err(MenuError::DepthLimit);
        }
        if count as usize > MAX_MENU_NODES.saturating_sub(self.nodes) {
            return Err(MenuError::NodeLimit);
        }
        Ok(())
    }

    fn leaf(&mut self, id: i32) -> Result<()> {
        if id <= 0 {
            return Err(MenuError::InvalidCommandId);
        }
        if !self.leaf_ids.insert(id) {
            return Err(MenuError::DuplicateCommandId);
        }
        Ok(())
    }
}

impl Tree {
    fn capture(source: ICoreWebView2ContextMenuItemCollection) -> Result<Self> {
        Self::read(source, 1, &mut Budget::default())
    }

    fn read(
        source: ICoreWebView2ContextMenuItemCollection,
        depth: usize,
        budget: &mut Budget,
    ) -> Result<Self> {
        let identity = getter(source.cast::<IUnknown>())?;
        let mut count = 0;
        getter(unsafe { source.Count(&mut count) })?;
        budget.collection(count, depth)?;
        let mut nodes = Vec::with_capacity(count as usize);
        for index in 0..count {
            // A previous subtree may have exhausted the shared budget since the
            // collection's initial Count. Never overshoot it on a later sibling.
            budget.collection(1, depth)?;
            budget.nodes += 1;
            let item = getter(unsafe { source.GetValueAtIndex(index) })?;
            let identity = getter(item.cast::<IUnknown>())?;
            let value = properties(&item)?;
            if value.kind.is_leaf() {
                budget.leaf(value.command_id)?;
            }
            let children = if value.kind == Kind::Submenu {
                Some(Self::read(getter(unsafe { item.Children() })?, depth + 1, budget)?)
            } else {
                None
            };
            nodes.push(Node { item, identity, value, children });
        }
        let mut final_count = 0;
        getter(unsafe { source.Count(&mut final_count) })?;
        if final_count != count {
            return Err(MenuError::SnapshotChanged);
        }
        Ok(Self { source, identity, nodes })
    }

    fn same(&self, other: &Self) -> bool {
        self.identity.as_raw() == other.identity.as_raw()
            && self.nodes.len() == other.nodes.len()
            && self.nodes.iter().zip(&other.nodes).all(|(left, right)| {
                left.identity.as_raw() == right.identity.as_raw()
                    && left.value == right.value
                    && match (&left.children, &right.children) {
                        (None, None) => true,
                        (Some(left), Some(right)) => left.same(right),
                        _ => false,
                    }
            })
    }

    fn revalidate(&self) -> Result<()> {
        let current = Self::capture(self.source.clone())?;
        if !self.same(&current) {
            return Err(MenuError::SnapshotChanged);
        }
        Ok(())
    }

    fn enabled_leaf(&self, id: i32, ancestors_enabled: bool) -> Option<&Node> {
        for node in &self.nodes {
            let enabled = ancestors_enabled && node.value.enabled;
            if node.value.kind.is_leaf() && node.value.command_id == id {
                return enabled.then_some(node);
            }
            if let Some(children) = &node.children {
                if let Some(found) = children.enabled_leaf(id, enabled) {
                    return Some(found);
                }
            }
        }
        None
    }
}

struct NativeMenu {
    handle: Option<HMENU>,
}

impl NativeMenu {
    fn create() -> Result<Self> {
        Ok(Self { handle: Some(native(unsafe { CreatePopupMenu() })?) })
    }

    fn handle(&self) -> Result<HMENU> {
        self.handle.ok_or(MenuError::UnexpectedState)
    }

    fn close(&mut self) -> Result<()> {
        if let Some(handle) = self.handle {
            native(unsafe { DestroyMenu(handle) })?;
            self.handle = None;
        }
        Ok(())
    }

    fn from_tree(tree: &Tree) -> Result<Self> {
        let menu = Self::create()?;
        let handle = menu.handle()?;
        for node in &tree.nodes {
            let value = &node.value;
            if value.kind == Kind::Separator {
                native(unsafe { AppendMenuW(handle, MF_SEPARATOR, 0, PCWSTR::null()) })?;
                continue;
            }
            let text = display_text(value);
            let mut flags = MF_STRING;
            if !value.enabled {
                flags |= MF_GRAYED;
            }
            if value.checked == Some(true) {
                flags |= MF_CHECKED;
            }
            if let Some(children) = &node.children {
                let mut child = Self::from_tree(children)?;
                let child_handle = child.handle()?;
                native(unsafe {
                    AppendMenuW(handle, flags | MF_POPUP, child_handle.0 as usize, PCWSTR(text.as_ptr()))
                })?;
                // Transfer only AFTER a successful append. DestroyMenu(parent)
                // now owns all descendants, including on later sibling failure.
                child.handle = None;
            } else {
                native(unsafe {
                    AppendMenuW(handle, flags, value.command_id as usize, PCWSTR(text.as_ptr()))
                })?;
                if value.kind == Kind::Radio {
                    let info = MENUITEMINFOW {
                        cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                        fMask: MIIM_FTYPE,
                        fType: MFT_RADIOCHECK,
                        ..Default::default()
                    };
                    native(unsafe { SetMenuItemInfoW(handle, value.command_id as u32, false, &info) })?;
                }
            }
        }
        Ok(menu)
    }
}

impl Drop for NativeMenu {
    fn drop(&mut self) {
        // The caller can explicitly observe close() errors. Drop is best-effort
        // and never logs text or panics during unwinding.
        let _ = self.close();
    }
}

fn display_text(value: &Properties) -> Vec<u16> {
    // Preserve engine-provided ampersands and ordinary text exactly. Win32's tab
    // convention displays the documented shortcut description at the right.
    let mut text: Vec<u16> = value.label.encode_utf16().collect();
    if !value.shortcut.is_empty() {
        text.push(b'\t' as u16);
        text.extend(value.shortcut.encode_utf16());
    }
    text.push(0);
    text
}

#[derive(Clone, Copy)]
enum State {
    Ready,
    NoSelection,
    Selected(i32),
    Spent,
}

/// Genuine OS choice, movable across a deferred fresh-inventory continuation.
/// No public constructor, ID getter or Clone implementation: an integrator cannot
/// fabricate this value from a command ID. It is bound to one captured menu.
pub(crate) struct MenuSelection {
    id: i32,
    request: Rc<()>,
    validated: Cell<bool>,
}

/// Single-use native menu, confined to the capturing UI apartment. No raw HMENU,
/// item, label or unvalidated command identifier escapes this object.
pub(crate) struct EngineMenu {
    tree: Tree,
    native: NativeMenu,
    thread: u32,
    state: State,
    request: Rc<()>,
    _apartment: PhantomData<Rc<()>>,
}

impl EngineMenu {
    pub(crate) fn capture(collection: ICoreWebView2ContextMenuItemCollection) -> Result<Self> {
        let thread = unsafe { GetCurrentThreadId() };
        let tree = Tree::capture(collection)?;
        let native = NativeMenu::from_tree(&tree)?;
        // Refuse changes observed across capture/construction; do not show a
        // partial or hybrid admission as a successful snapshot.
        tree.revalidate()?;
        Ok(Self {
            tree, native, thread, state: State::Ready, request: Rc::new(()),
            _apartment: PhantomData,
        })
    }

    fn on_thread(&self) -> Result<()> {
        if unsafe { GetCurrentThreadId() } != self.thread {
            return Err(MenuError::WrongThread);
        }
        Ok(())
    }

    /// Exact retained root identity for the coordinator's cancellation-only modal guard.
    /// This is not a selection/dispatch authority and is invalid outside the lexical show call.
    pub(super) fn modal_root(&self)->Result<HMENU> {
        self.on_thread()?;
        if !matches!(self.state,State::Ready) {return Err(MenuError::UnexpectedState);}
        self.native.handle()
    }

    /// Call only from the deferred continuation, AFTER the engine callback has
    /// returned and with no Rust RefCell guard alive. `point` is already in screen
    /// pixels; controller bounds/rasterization conversion belongs to the caller.
    /// This method neither activates a window nor supplies automatic selection.
    /// None means cancellation OR Win32 failure, which TPM_RETURNCMD does not
    /// distinguish. A returned opaque choice may be held while a second native
    /// inventory callback settles, then passed to validate_selection().
    pub(crate) fn show(&mut self, owner: HWND, point: POINT, final_admission: impl FnOnce() -> bool) -> Result<Option<MenuSelection>> {
        self.on_thread()?;
        if !matches!(self.state, State::Ready) {
            return Err(MenuError::UnexpectedState);
        }
        self.state = State::Spent;
        self.tree.revalidate()?;
        let mut process = 0;
        if !unsafe { IsWindow(Some(owner)) }.as_bool()
            || unsafe { GetWindowThreadProcessId(owner, Some(&mut process)) } != self.thread
            || process != std::process::id()
            || unsafe { GetForegroundWindow() } != owner
        {
            return Err(MenuError::InvalidOwner);
        }
        if !final_admission() {return Err(MenuError::InvalidOwner);}
        let selected = unsafe {
            TrackPopupMenu(
                self.native.handle()?, TPM_RETURNCMD | TPM_NONOTIFY,
                point.x, point.y, Some(0), owner, None,
            )
        }.0;
        if selected == 0 {
            self.state = State::NoSelection;
            return Ok(None);
        }
        if selected < 0 || self.tree.enabled_leaf(selected, true).is_none() {
            return Err(MenuError::NotAnEnabledLeaf);
        }
        self.state = State::Selected(selected);
        Ok(Some(MenuSelection { id: selected, request: self.request.clone(), validated:Cell::new(false) }))
    }

    /// Consume the genuine TrackPopupMenu result, destroy all native menus, then
    /// reread and compare the COMPLETE current tree, canonical item identities,
    /// order, text, IDs, kinds, enabled/check states, shortcuts and icon absence.
    /// Finally reread the actual retained selected item immediately before return.
    /// A choice from any other captured menu, a replay, or a stale/unknown/disabled
    /// selection is an error. No caller-supplied integer is accepted.
    ///
    /// This is NOT a grant or a reusable dispatch token. After success the caller
    /// must recheck its original request/owner/document/profile/view/consent fences
    /// and, without yielding or unrelated work, set SelectedCommandId only on
    /// that exact request. Never dispatch after a failing fence or a close error.
    pub(crate) fn validate_selection(&mut self, selection: &MenuSelection) -> Result<i32> {
        self.on_thread()?;
        let state = std::mem::replace(&mut self.state, State::Spent);
        self.native.close()?;
        let id = match state {
            State::Selected(id)
                if id == selection.id && Rc::ptr_eq(&self.request, &selection.request) => id,
            _ => return Err(MenuError::UnexpectedState),
        };
        self.tree.revalidate()?;
        let node = self.tree.enabled_leaf(id, true).ok_or(MenuError::NotAnEnabledLeaf)?;
        let fresh = properties(&node.item)?;
        if fresh != node.value {
            return Err(MenuError::SnapshotChanged);
        }
        if !fresh.kind.is_leaf() || !fresh.enabled || fresh.command_id != id || id <= 0 {
            return Err(MenuError::NotAnEnabledLeaf);
        }
        selection.validated.set(true);
        Ok(id)
    }

    /// Last selected-item native reads after the caller's final profile/authority reads. The
    /// caller must follow this only with pure request/authority fences and the one native setter.
    /// This cannot create an ID: the opaque genuine choice must already have passed validation.
    pub(crate) fn recheck_selected(&self, selection:&MenuSelection, id:i32)->Result<()> {
        self.on_thread()?;
        if !selection.validated.get() || selection.id!=id || !Rc::ptr_eq(&self.request,&selection.request) {
            return Err(MenuError::UnexpectedState);
        }
        let node=self.tree.enabled_leaf(id,true).ok_or(MenuError::NotAnEnabledLeaf)?;
        if properties(&node.item)?!=node.value {return Err(MenuError::SnapshotChanged);}
        Ok(())
    }

    /// Read-only full-tree diagnostic for the separately armed host regression test. The
    /// production tree/selection remains private; these values confer no dispatch authority.
    #[cfg(test)]
    pub(crate) fn probe_host_snapshot(&self) -> Result<Vec<super::super::app_host_menu_probe::NodeWitness>> {
        use super::super::app_host_menu_probe::{NodeWitness,role};
        fn visit(tree:&Tree,path:&[usize])->Result<Vec<NodeWitness>> {
            tree.nodes.iter().enumerate().map(|(index,node)|{
                let mut path=path.to_vec();path.push(index);
                let name=task_text(MAX_LABEL_UTF16_UNITS,|out|unsafe{node.item.Name(out)})?;
                let rendered=if node.value.kind==Kind::Separator {vec![0]}else{display_text(&node.value)};let units=&rendered[..rendered.len()-1];
                let bytes=units.iter().flat_map(|unit|unit.to_le_bytes()).collect::<Vec<_>>();
                let fingerprint=crate::updater::sha256(&bytes).map_err(|_|MenuError::Native(0x80004005u32 as i32))?;
                let fingerprint=fingerprint.iter().map(|byte|format!("{byte:02x}")).collect::<String>();
                Ok(NodeWitness {path:path.clone(),collection:tree.identity.as_raw() as usize,
                    identity:node.identity.as_raw() as usize,kind:match node.value.kind {Kind::Command=>0,Kind::CheckBox=>1,Kind::Radio=>2,Kind::Separator=>3,Kind::Submenu=>4},
                    command:node.value.command_id,enabled:node.value.enabled,checked:node.value.checked,name,
                    rendered_fingerprint:fingerprint,rendered_units:units.len(),host_role:role(&node.identity),
                    children:if let Some(children)=&node.children {visit(children,&path)?}else{Vec::new()},
                })
            }).collect()
        }
        self.tree.revalidate()?;let snapshot=visit(&self.tree,&[])?;self.tree.revalidate()?;Ok(snapshot)
    }

    // Test-only diagnostic: never a selection or dispatch authority.
    #[cfg(test)]
    pub(crate) fn probe_root_menu(&self)->Option<usize> {self.native.handle().ok().map(|menu|menu.0 as usize)}

    #[cfg(test)]
    pub(crate) fn probe_fixture_command(&self)->Option<i32> {
        fn visit(tree:&Tree,found:&mut Vec<i32>) {
            for node in &tree.nodes {
                if node.value.kind.is_leaf() && node.value.enabled
                    && node.value.label=="Bowser native context permission probe" {found.push(node.value.command_id);}
                if let Some(children)=&node.children {visit(children,found);}
            }
        }
        let mut found=Vec::new();visit(&self.tree,&mut found);
        (found.len()==1).then(||found[0])
    }

    /// Explicit cancellation/cleanup is observable; drop also cleans up. Once
    /// closed, showing or consuming this menu can never produce a command ID.
    pub(crate) fn close(&mut self) -> Result<()> {
        self.on_thread()?;
        self.state = State::Spent;
        self.native.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(label: &str, shortcut: &str) -> Properties {
        Properties {
            kind: Kind::Command, command_id: 1, enabled: true, checked: None,
            label: label.into(), shortcut: shortcut.into(),
        }
    }

    #[test]
    fn display_preserves_untrusted_urls_and_engine_accelerators() {
        let label = "Open &https://example.test/path?one=1&two=2";
        let text = display_text(&value(label, "Ctrl+Shift+I"));
        assert_eq!(String::from_utf16(&text[..text.len()-1]).unwrap(), format!("{label}\tCtrl+Shift+I"));
        assert_eq!(text.last(), Some(&0));
        assert_eq!(display_text(&value("", "")), vec![0]);
    }

    #[test]
    fn only_known_leaf_kinds_can_dispatch() {
        for kind in 0..=4 {
            let parsed = Kind::from_engine(kind).unwrap();
            assert_eq!(parsed.is_leaf(), kind <= 2);
            assert_eq!(parsed.has_check_state(), kind == 1 || kind == 2);
        }
        for kind in [-1, 5, i32::MIN, i32::MAX] {
            assert_eq!(Kind::from_engine(kind), Err(MenuError::UnknownKind));
        }
    }

    #[test]
    fn utf16_is_exact_and_never_lossy() {
        assert_eq!(decode_utf16(&[0xD83D, 0xDE80]).unwrap(), "\u{1F680}");
        for invalid in [&[0xD800][..], &[0xDC00][..], &[0xD800, 0x0061][..]] {
            assert_eq!(decode_utf16(invalid), Err(MenuError::InvalidUtf16));
        }
    }

    fn allocated_text(units: &[u16]) -> PWSTR {
        let allocation = unsafe {
            windows::Win32::System::Com::CoTaskMemAlloc(std::mem::size_of_val(units))
        } as *mut u16;
        assert!(!allocation.is_null());
        unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), allocation, units.len()) };
        PWSTR(allocation)
    }

    #[test]
    fn allocated_text_admits_exact_bound_but_never_a_prefix() {
        let mut exact = vec![b'x' as u16; MAX_LABEL_UTF16_UNITS];
        exact.push(0);
        let output = allocated_text(&exact);
        let text = task_text(MAX_LABEL_UTF16_UNITS, |out| {
            unsafe { out.write(output) };
            Ok(())
        }).unwrap();
        assert_eq!(text.len(), MAX_LABEL_UTF16_UNITS);

        let mut over = vec![b'x' as u16; MAX_LABEL_UTF16_UNITS + 1];
        over.push(0);
        let output = allocated_text(&over);
        assert_eq!(task_text(MAX_LABEL_UTF16_UNITS, |out| {
            unsafe { out.write(output) };
            Ok(())
        }), Err(MenuError::TextLimit));
    }

    #[test]
    fn null_invalid_and_failed_getters_never_admit_text() {
        assert_eq!(task_text(512, |_| Ok(())), Err(MenuError::NullString));
        let output = allocated_text(&[0xD800, 0]);
        assert_eq!(task_text(512, |out| {
            unsafe { out.write(output) };
            Ok(())
        }), Err(MenuError::InvalidUtf16));

        // An allocation written before a failed HRESULT must still be freed by
        // TaskText, rather than leaked on the getter-error return path.
        let output = allocated_text(&[b'x' as u16, 0]);
        let code = 0x80004005u32 as i32;
        assert_eq!(task_text(512, |out| {
            unsafe { out.write(output) };
            Err(windows::core::Error::from_hresult(windows::core::HRESULT(code)))
        }), Err(MenuError::Getter(code)));
    }

    #[test]
    fn all_nodes_and_nested_siblings_share_a_hard_budget() {
        let mut budget = Budget::default();
        assert_eq!(budget.collection(256, 1), Ok(()));
        assert_eq!(budget.collection(257, 1), Err(MenuError::NodeLimit));
        budget.nodes = 256;
        assert_eq!(budget.collection(0, 1), Ok(()));
        assert_eq!(budget.collection(1, 1), Err(MenuError::NodeLimit));
        assert_eq!(budget.collection(0, 7), Ok(()));
        budget.nodes = 0;
        assert_eq!(budget.collection(1, 6), Ok(()));
        assert_eq!(budget.collection(1, 7), Err(MenuError::DepthLimit));
    }

    #[test]
    fn ids_are_positive_unique_and_request_local() {
        let mut first = Budget::default();
        for invalid in [0, -1, i32::MIN] {
            assert_eq!(first.leaf(invalid), Err(MenuError::InvalidCommandId));
        }
        assert_eq!(first.leaf(1), Ok(()));
        assert_eq!(first.leaf(1), Err(MenuError::DuplicateCommandId));
        assert_eq!(first.leaf(i32::MAX), Ok(()));
        assert_eq!(Budget::default().leaf(1), Ok(()));
    }
}
