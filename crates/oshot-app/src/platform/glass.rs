//! Native Liquid Glass behind GPUI content (`NSGlassEffectView`, macOS 26).

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSRect;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::state::Glass;

pub fn decorate(window: &mut gpui::Window, glass: Glass) {
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return;
    };
    let view = h.ns_view.as_ptr() as *mut AnyObject;
    // SAFETY: `view` is GPUI's live NSView, a subview of the window's
    // content view; we add a sibling below it on the main thread.
    unsafe {
        let superview: *mut AnyObject = msg_send![view, superview];
        if superview.is_null() {
            return;
        }
        let bounds: NSRect = msg_send![superview, bounds];
        let alloc: Allocated<AnyObject> = msg_send![class!(NSGlassEffectView), alloc];
        let effect: Retained<AnyObject> = msg_send![alloc, initWithFrame: bounds];
        // Width and height sizable, so the glass follows window resizes.
        let _: () = msg_send![&*effect, setAutoresizingMask: 2usize | 16usize];
        if let Glass::Panel { radius } = glass {
            let _: () = msg_send![&*effect, setCornerRadius: radius as f64];
        }
        // NSWindowBelow = -1: under GPUI's view, so content draws on top.
        let _: () = msg_send![superview, addSubview: &*effect, positioned: -1isize, relativeTo: view];
        let native_window: *mut AnyObject = msg_send![view, window];
        if !native_window.is_null() {
            let _: () = msg_send![native_window, setHasShadow: true];
        }
    }
}
