//! The page's context menu on a selection, shown by Bowser with live rows on top: a currency
//! conversion for a price, a translation for text in another language. Deferred: never enter a
//! modal loop inside a WebView2 callback.
use super::menu::{self, Entry, Icon, Item, Menu};
use std::{cell::RefCell, rc::Rc};
use webview2_com::{CoTaskMemPWSTR, Microsoft::Web::WebView2::Win32::*};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, WPARAM},
        UI::WindowsAndMessaging::{PostMessageW, WM_APP},
    },
    core::{BOOL, PWSTR, Result},
};

pub const WM_SHOW: u32 = WM_APP + 0xB8;
const CURRENCY_ID: usize = 0x7fff_fffe;
const TRANSLATION_ID: usize = 0x7fff_fffd;
struct Pending {
    owner: HWND,
    anchor: POINT,
    menu: Menu,
    args: ICoreWebView2ContextMenuRequestedEventArgs,
    deferral: Completion,
    current: Rc<dyn Fn() -> bool>,
}
struct Completion(ICoreWebView2Deferral);
impl Drop for Completion {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.Complete();
        }
    }
}
thread_local! { static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) }; }

unsafe fn entries(
    collection: &ICoreWebView2ContextMenuItemCollection,
    depth: usize,
) -> Result<Vec<Entry>> {
    unsafe {
        if depth > 8 {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        let mut count = 0;
        collection.Count(&mut count)?;
        let mut result = Vec::new();
        if count > 100 {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        for index in 0..count {
            let original = collection.GetValueAtIndex(index)?;
            let mut kind = COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND;
            original.Kind(&mut kind)?;
            if kind == COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR {
                result.push(Entry::Separator);
                continue;
            }
            let mut label = PWSTR::null();
            original.Label(&mut label)?;
            let label = CoTaskMemPWSTR::from(label).to_string().replace('&', "");
            let mut id = 0;
            original.CommandId(&mut id)?;
            let mut enabled = BOOL::default();
            original.IsEnabled(&mut enabled)?;
            let mut checked = BOOL::default();
            original.IsChecked(&mut checked)?;
            let mut item = Item::new(id as usize, label, Icon::None)
                .disabled_if(!enabled.as_bool())
                .checked(checked.as_bool());
            let mut name = PWSTR::null();
            original.Name(&mut name)?;
            let name = CoTaskMemPWSTR::from(name).to_string();
            let glyph = match name.as_str() {
                "copy" => Some("copy"),
                "print" => Some("print"),
                "reload" => Some("reload"),
                _ if item.label.starts_with("Search ") => Some("search"),
                _ => None,
            };
            if let Some(glyph) = glyph {
                item = item.glyph(glyph);
            }
            let shortcut = match name.as_str() {
                "copy" => Some("Ctrl+C"),
                "cut" => Some("Ctrl+X"),
                "paste" => Some("Ctrl+V"),
                "selectAll" => Some("Ctrl+A"),
                "print" => Some("Ctrl+P"),
                "undo" => Some("Ctrl+Z"),
                "redo" => Some("Ctrl+Shift+Z"),
                _ => None,
            };
            if let Some(shortcut) = shortcut {
                item = item.shortcut(shortcut);
            }
            if kind == COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SUBMENU {
                item = item.submenu_with(entries(&original.Children()?, depth + 1)?);
            }
            result.push(Entry::Item(item));
        }
        Ok(result)
    }
}

/// Takes over the menu with `price` converted and `translation` (the selected text and the language
/// to translate it into) on top. Does nothing when there is neither.
pub unsafe fn queue(
    owner: HWND,
    anchor: POINT,
    args: &ICoreWebView2ContextMenuRequestedEventArgs,
    price: Option<crate::currency::Price>,
    translation: Option<(String, String)>,
    current: Rc<dyn Fn() -> bool>,
) -> Result<()> {
    unsafe {
        if (price.is_none() && translation.is_none()) || PENDING.with(|pending| pending.borrow().is_some()) {
            return Ok(());
        }
        let mut items = entries(&args.MenuItems()?, 0)?;
        if let Some(price) = price {
            let mut conversion = Item::new(CURRENCY_ID, "Currency conversion", Icon::None)
                .described("Approximate reference rate · Frankfurter. Click the currency symbol or press Enter to switch currency.");
            conversion.currency = Some(Box::new(price));
            items.insert(0, Entry::Item(conversion));
        }
        if !current() {
            return Ok(());
        }
        if let Some((text, target)) = translation {
            let mut row = Item::new(TRANSLATION_ID, "Translation", Icon::None)
                .glyph("translate")
                .described("Translated by Google Translate. Choose it to copy the translation.");
            row.translation = Some(crate::translate::selection::start(text, target));
            items.insert(0, Entry::Item(row));
        }
        let deferral = Completion(args.GetDeferral()?);
        args.SetHandled(true)?;
        let pending = Pending {
            owner,
            anchor,
            menu: Menu {
                header: None,
                entries: items,
                footer: String::new(),
            },
            args: args.clone(),
            deferral,
            current,
        };
        PENDING.with(|slot| *slot.borrow_mut() = Some(pending));
        if let Err(error) = PostMessageW(Some(owner), WM_SHOW, WPARAM(0), LPARAM(0)) {
            PENDING.with(|slot| slot.borrow_mut().take());
            return Err(error);
        }
        Ok(())
    }
}

pub fn run(owner: HWND) {
    let pending = PENDING.with(|slot| {
        if slot
            .borrow()
            .as_ref()
            .is_some_and(|pending| pending.owner == owner)
        {
            slot.borrow_mut().take()
        } else {
            None
        }
    });
    let Some(pending) = pending else {
        return;
    };
    if !(pending.current)() {
        return;
    }
    let price = pending.menu.entries.iter().find_map(|entry| match entry {
        Entry::Item(item) => item.currency.as_deref().copied(),
        _ => None,
    });
    if price.is_some_and(|price| price.currency == crate::currency::target()) {
        crate::currency::cycle();
    }
    let lookup = pending.menu.entries.iter().find_map(|entry| match entry {
        Entry::Item(item) => item.translation,
        _ => None,
    });
    let Pending {
        owner,
        anchor,
        menu,
        args,
        deferral,
        current,
    } = pending;
    menu::show(
        owner,
        anchor,
        menu::Placement::BelowLeft,
        menu,
        "selection",
        move |_app, chosen| {
            if let Some(chosen) = chosen
                && current()
            {
                if chosen.id == TRANSLATION_ID {
                    if let Some(text) = lookup.and_then(crate::translate::selection::translation) {
                        super::window::copy_text_to_clipboard(owner, &text);
                    }
                } else {
                    unsafe {
                        let _ = args.SetSelectedCommandId(chosen.id as i32);
                    }
                }
            }
            drop(deferral);
        },
    );
}

pub fn cancel(owner: HWND) {
    let pending = PENDING.with(|slot| {
        if slot
            .borrow()
            .as_ref()
            .is_some_and(|pending| pending.owner == owner)
        {
            slot.borrow_mut().take()
        } else {
            None
        }
    });
    drop(pending);
}
