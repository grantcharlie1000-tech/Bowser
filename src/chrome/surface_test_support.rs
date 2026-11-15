//! Thread-local faults at the real allocation/upload boundary. No production build includes these.
use super::*;
use std::cell::Cell;

thread_local! {
    static ALLOCATION: Cell<bool> = const { Cell::new(false) };
    static UPLOAD: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn fail_allocation() { ALLOCATION.with(|slot| slot.set(true)); }
pub(crate) fn fail_upload() { UPLOAD.with(|slot| slot.set(true)); }
pub(super) fn take_allocation_failure() -> bool { ALLOCATION.with(|slot| slot.replace(false)) }
pub(super) fn take_upload_failure() -> bool { UPLOAD.with(|slot| slot.replace(false)) }
pub(crate) fn reset() {
    ALLOCATION.with(|slot| slot.set(false));
    UPLOAD.with(|slot| slot.set(false));
}

pub(crate) fn pixels(surface: &ChromeSurface) -> Vec<u8> {
    if surface.dib.bits.is_null() { return Vec::new(); }
    let stride = surface.dib.stride();
    (0..surface.height as usize).flat_map(|y| unsafe {
        std::slice::from_raw_parts(surface.dib.bits.add(y * stride), surface.width as usize * 4)
    }.iter().copied()).collect()
}

pub(crate) fn damage_pixels(surface: &mut ChromeSurface) {
    surface.dib.pixels_mut().fill(0x55);
}
