//! The capture menu gesture: AppKit event monitors for modifier changes,
//! fed to the core detector. Seeing other apps' key events needs
//! Accessibility trust, so the monitors are reinstalled once it is granted.

use std::cell::RefCell;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use oshot_core::gesture::{GestureDetector, MenuGesture};

use crate::state::PlatformEvent;

const KEY_DOWN: u64 = 1 << 10;
const FLAGS_CHANGED: u64 = 1 << 12;
const TYPE_KEY_DOWN: usize = 10;
const TYPE_FLAGS_CHANGED: usize = 12;

thread_local! {
    static MONITORS: RefCell<Vec<Retained<AnyObject>>> = const { RefCell::new(Vec::new()) };
    static DETECTOR: RefCell<GestureDetector> = RefCell::new(GestureDetector::new(MenuGesture::Off));
}

fn on_event(event: &AnyObject) {
    // SAFETY: NSEvent getters on an event AppKit handed us.
    let (kind, flags) = unsafe {
        let kind: usize = msg_send![event, type];
        let flags: usize = msg_send![event, modifierFlags];
        (kind, flags as u64)
    };
    let fired = DETECTOR.with(|d| {
        let mut d = d.borrow_mut();
        match kind {
            TYPE_FLAGS_CHANGED => d.flags_changed(flags),
            TYPE_KEY_DOWN => {
                d.key_down(flags);
                false
            }
            _ => false,
        }
    });
    if fired {
        super::send(PlatformEvent::MenuGesture);
    }
}

pub fn set(gesture: MenuGesture) {
    MONITORS.with(|m| {
        for monitor in m.borrow_mut().drain(..) {
            // SAFETY: a monitor returned by addGlobal/LocalMonitor.
            unsafe {
                let _: () = msg_send![class!(NSEvent), removeMonitor: &*monitor];
            }
        }
    });
    DETECTOR.with(|d| *d.borrow_mut() = GestureDetector::new(gesture));
    if gesture == MenuGesture::Off {
        return;
    }
    let mask = KEY_DOWN | FLAGS_CHANGED;
    // Global monitors see other apps' events; local ones see OShot's own,
    // which global monitors skip.
    let global = RcBlock::new(|event: NonNull<AnyObject>| {
        // SAFETY: AppKit passes a live NSEvent.
        on_event(unsafe { event.as_ref() });
    });
    let local = RcBlock::new(|event: NonNull<AnyObject>| -> *mut AnyObject {
        // SAFETY: AppKit passes a live NSEvent.
        on_event(unsafe { event.as_ref() });
        event.as_ptr()
    });
    // SAFETY: NSEvent class methods on the main thread; AppKit copies the
    // blocks and keeps the monitors until removeMonitor.
    let monitors: Vec<Retained<AnyObject>> = unsafe {
        let g: Option<Retained<AnyObject>> =
            msg_send![class!(NSEvent), addGlobalMonitorForEventsMatchingMask: mask, handler: &*global];
        let l: Option<Retained<AnyObject>> =
            msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: mask, handler: &*local];
        [g, l].into_iter().flatten().collect()
    };
    MONITORS.with(|m| *m.borrow_mut() = monitors);
}
