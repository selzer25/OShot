//! Global hotkeys through Carbon's RegisterEventHotKey, which is still the
//! only public macOS API for system-wide shortcuts that needs no
//! Accessibility permission.

use std::cell::RefCell;
use std::ffi::c_void;

use oshot_core::settings::{Hotkey, HotkeyAction};

use crate::state::PlatformEvent;

#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

type Handler = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: Handler,
        count: u32,
        list: *const EventTypeSpec,
        user_data: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: *mut c_void,
        options: u32,
        out: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey: *mut c_void) -> i32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        kind: u32,
        actual_kind: *mut u32,
        size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> i32;
}

const fn four(s: &[u8; 4]) -> u32 {
    ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | s[3] as u32
}

const SIGNATURE: u32 = four(b"OSHT");

thread_local! {
    static REGISTERED: RefCell<Vec<(*mut c_void, HotkeyAction)>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn on_hotkey(_next: *mut c_void, event: *mut c_void, _data: *mut c_void) -> i32 {
    let mut id = EventHotKeyId { signature: 0, id: 0 };
    // SAFETY: Carbon passes a valid hot key event; we read its ID parameter
    // into a correctly sized struct.
    let status = unsafe {
        GetEventParameter(
            event,
            four(b"----"),
            four(b"hkid"),
            std::ptr::null_mut(),
            std::mem::size_of::<EventHotKeyId>(),
            std::ptr::null_mut(),
            &mut id as *mut _ as *mut c_void,
        )
    };
    if status == 0 && id.signature == SIGNATURE {
        let action = REGISTERED.with(|r| r.borrow().get(id.id as usize).map(|(_, a)| *a));
        if let Some(action) = action {
            super::send(PlatformEvent::Hotkey(action));
        }
    }
    0
}

pub fn install() {
    let spec = EventTypeSpec {
        class: four(b"keyb"),
        kind: 5,
    };
    // SAFETY: installs a C callback with no user data on the app target.
    unsafe {
        InstallEventHandler(
            GetApplicationEventTarget(),
            on_hotkey,
            1,
            &spec,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
}

/// ANSI virtual key code for a key name.
pub fn key_code(key: &str) -> Option<u32> {
    Some(match key {
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "v" => 9,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "=" => 24,
        "9" => 25,
        "7" => 26,
        "-" => 27,
        "8" => 28,
        "0" => 29,
        "]" => 30,
        "o" => 31,
        "u" => 32,
        "[" => 33,
        "i" => 34,
        "p" => 35,
        "enter" => 36,
        "l" => 37,
        "j" => 38,
        "'" => 39,
        "k" => 40,
        ";" => 41,
        "\\" => 42,
        "," => 43,
        "/" => 44,
        "n" => 45,
        "m" => 46,
        "." => 47,
        "tab" => 48,
        "space" => 49,
        "`" => 50,
        "backspace" => 51,
        "escape" => 53,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f3" => 99,
        "f8" => 100,
        "f9" => 101,
        "f11" => 103,
        "f10" => 109,
        "f12" => 111,
        "f4" => 118,
        "f2" => 120,
        "f1" => 122,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        _ => return None,
    })
}

fn modifiers(h: &Hotkey) -> u32 {
    let mut m = 0;
    if h.cmd {
        m |= 1 << 8;
    }
    if h.shift {
        m |= 1 << 9;
    }
    if h.alt {
        m |= 1 << 11;
    }
    if h.ctrl {
        m |= 1 << 12;
    }
    m
}

pub fn register(bindings: &[(HotkeyAction, Hotkey)]) -> Vec<HotkeyAction> {
    REGISTERED.with(|r| {
        let mut r = r.borrow_mut();
        for (hk, _) in r.drain(..) {
            if !hk.is_null() {
                // SAFETY: `hk` came from RegisterEventHotKey.
                unsafe { UnregisterEventHotKey(hk) };
            }
        }
        let mut failed = Vec::new();
        for (action, hotkey) in bindings {
            let mut out: *mut c_void = std::ptr::null_mut();
            let id = EventHotKeyId {
                signature: SIGNATURE,
                id: r.len() as u32,
            };
            let status = match key_code(&hotkey.key) {
                // SAFETY: valid key code and modifiers on the app target.
                Some(code) => unsafe {
                    RegisterEventHotKey(code, modifiers(hotkey), id, GetApplicationEventTarget(), 0, &mut out)
                },
                None => -1,
            };
            if status != 0 {
                failed.push(*action);
            }
            r.push((out, *action));
        }
        failed
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_keys_map_to_ansi_codes() {
        assert_eq!(key_code("3"), Some(20));
        assert_eq!(key_code("4"), Some(21));
        assert_eq!(key_code("5"), Some(23));
        assert_eq!(key_code("6"), Some(22));
        assert_eq!(key_code("7"), Some(26));
        assert_eq!(key_code("nope"), None);
    }

    #[test]
    fn option_shift_modifier_bits() {
        let h = Hotkey::parse("alt-shift-4").unwrap();
        assert_eq!(modifiers(&h), (1 << 9) | (1 << 11));
    }
}
