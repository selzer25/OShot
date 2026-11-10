//! Accessibility trust and the drag-to-grant helper's native pieces: the
//! app bundle, System Settings' window, a Finder-style file drag and moving
//! a window from outside a GPUI update.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, ClassBuilder, NSObject, Sel};
use objc2::{ClassType, class, msg_send, sel};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use oshot_core::geometry::Rect;
use oshot_native::cf::{self, CFTypeRef};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
    static kCFBooleanTrue: CFTypeRef;
}

pub fn has_accessibility() -> bool {
    // SAFETY: no arguments.
    unsafe { AXIsProcessTrusted() }
}

/// Shows macOS's Accessibility prompt once, which also adds OShot to the
/// list in System Settings.
pub fn request_accessibility() {
    // SAFETY: a one-entry dictionary of CF constants.
    unsafe {
        let options = cf::dict(&[(kAXTrustedCheckOptionPrompt, kCFBooleanTrue)]);
        AXIsProcessTrustedWithOptions(options.ptr());
    }
}

/// The `.app` this binary runs from. A bare binary (cargo run) has none,
/// and macOS attributes its permissions to the terminal anyway.
pub fn app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf)
}

const SETTINGS_BUNDLE: &str = "com.apple.systempreferences";

fn settings_pid() -> Option<i64> {
    // SAFETY: NSRunningApplication lookups with a valid string.
    unsafe {
        let id = NSString::from_str(SETTINGS_BUNDLE);
        let apps: Option<Retained<AnyObject>> =
            msg_send![class!(NSRunningApplication), runningApplicationsWithBundleIdentifier: &*id];
        let app: Option<Retained<AnyObject>> = msg_send![&*apps?, firstObject];
        let pid: i32 = msg_send![&*app?, processIdentifier];
        Some(pid as i64)
    }
}

fn settings_frontmost() -> bool {
    // SAFETY: NSWorkspace reads.
    unsafe {
        let ws: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        let app: Option<Retained<AnyObject>> = msg_send![&*ws, frontmostApplication];
        let Some(app) = app else { return false };
        let id: Option<Retained<NSString>> = msg_send![&*app, bundleIdentifier];
        // The password sheet a privacy change raises belongs to these.
        matches!(
            id.map(|s| s.to_string()).as_deref(),
            Some(SETTINGS_BUNDLE | "com.apple.SecurityAgent" | "com.apple.LocalAuthentication.UIAgent")
        )
    }
}

/// System Settings' main window in global points, while the user is in it.
/// Window bounds and owners are readable without Screen Recording.
pub fn system_settings_frame() -> Option<Rect> {
    if !settings_frontmost() {
        return None;
    }
    let pid = settings_pid()?;
    super::capture::window_frames_of(pid)
        .into_iter()
        .filter(|r| r.w >= 320.0 && r.h >= 240.0)
        .max_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)))
}

fn ns_window(window: &mut gpui::Window) -> Option<Retained<AnyObject>> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return None;
    };
    let view = h.ns_view.as_ptr() as *mut AnyObject;
    // SAFETY: GPUI's live NSView; `window` is retained by objc2.
    unsafe { msg_send![view, window] }
}

/// Starts dragging `path` from the mouse-down being handled right now, the
/// way Finder does. System Settings adds a `.app` dropped on its Screen
/// Recording or Accessibility list.
pub fn begin_file_drag(window: &mut gpui::Window, path: &Path) -> bool {
    let Some(ns) = ns_window(window) else { return false };
    let source = drag_source();
    // SAFETY: AppKit drag APIs on the main thread with live objects; the
    // dragging item is released after the array retains it.
    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let event: Option<Retained<AnyObject>> = msg_send![&*app, currentEvent];
        let view: Option<Retained<AnyObject>> = msg_send![&*ns, contentView];
        let (Some(event), Some(view)) = (event, view) else {
            return false;
        };
        let path = NSString::from_str(&path.to_string_lossy());
        let url: Retained<AnyObject> = msg_send![class!(NSURL), fileURLWithPath: &*path];
        let item: *mut AnyObject = msg_send![class!(NSDraggingItem), alloc];
        let item: *mut AnyObject = msg_send![item, initWithPasteboardWriter: &*url];
        if item.is_null() {
            return false;
        }
        let ws: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        let icon: Retained<AnyObject> = msg_send![&*ws, iconForFile: &*path];
        let _: () = msg_send![&*icon, setSize: NSSize::new(64.0, 64.0)];
        let at: NSPoint = msg_send![&*event, locationInWindow];
        let at: NSPoint = msg_send![&*view, convertPoint: at, fromView: std::ptr::null::<AnyObject>()];
        let frame = NSRect::new(NSPoint::new(at.x - 32.0, at.y - 32.0), NSSize::new(64.0, 64.0));
        let _: () = msg_send![item, setDraggingFrame: frame, contents: &*icon];
        let items: Retained<AnyObject> = msg_send![class!(NSArray), arrayWithObject: item];
        let _: () = msg_send![item, release];
        let session: Option<Retained<AnyObject>> =
            msg_send![&*view, beginDraggingSessionWithItems: &*items, event: &*event, source: source];
        match session {
            Some(s) => {
                let _: () = msg_send![&*s, setAnimatesToStartingPositionsOnCancelOrFail: true];
                true
            }
            None => false,
        }
    }
}

/// An `NSDraggingSource` that offers copy outside the app and nothing
/// inside it. Built once and kept for the process.
fn drag_source() -> &'static AnyObject {
    extern "C" fn mask(_: *mut AnyObject, _: Sel, _: *mut AnyObject, context: isize) -> usize {
        const OUTSIDE_APP: isize = 0;
        const COPY: usize = 1;
        if context == OUTSIDE_APP { COPY } else { 0 }
    }
    static SOURCE: OnceLock<usize> = OnceLock::new();
    let ptr = *SOURCE.get_or_init(|| {
        let class: &AnyClass = match ClassBuilder::new(c"OShotDragSource", NSObject::class()) {
            Some(mut b) => {
                // SAFETY: the method matches the selector's signature.
                unsafe {
                    b.add_method(
                        sel!(draggingSession:sourceOperationMaskForDraggingContext:),
                        mask as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, isize) -> usize,
                    );
                }
                if let Some(p) = AnyProtocol::get(c"NSDraggingSource") {
                    b.add_protocol(p);
                }
                b.register()
            }
            None => AnyClass::get(c"OShotDragSource").expect("registered"),
        };
        // SAFETY: plain NSObject subclass; leaked on purpose.
        let obj: *mut AnyObject = unsafe { msg_send![class, new] };
        obj as usize
    });
    // SAFETY: never freed.
    unsafe { &*(ptr as *const AnyObject) }
}

unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

struct Placement {
    window: Retained<AnyObject>,
    frame: Rect,
    /// GPUI's view, when the window also turns borderless (see
    /// `cover_display`) and must take keyboard focus back afterwards.
    cover: Option<Retained<AnyObject>>,
}

/// Moves a window to `frame` (global points, top-left origin) on the next
/// main-queue turn. `setFrame:` calls straight back into GPUI's window
/// callbacks, which must not happen while GPUI is mid-update.
pub fn place_window(window: &mut gpui::Window, frame: Rect) {
    let Some(ns) = ns_window(window) else { return };
    schedule(Placement {
        window: ns,
        frame,
        cover: None,
    });
}

/// Makes a window cover `frame` exactly, menu bar included. AppKit keeps
/// titled windows below the menu bar, even through `setFrame:`, so the window
/// first drops its (invisible) title bar, as GPUI's own simple fullscreen does.
pub fn cover_display(window: &mut gpui::Window, frame: Rect) {
    let Some(ns) = ns_window(window) else { return };
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return;
    };
    // SAFETY: GPUI's live NSView, retained for the deferred call.
    let Some(view) = (unsafe { Retained::retain(h.ns_view.as_ptr() as *mut AnyObject) }) else {
        return;
    };
    schedule(Placement {
        window: ns,
        frame,
        cover: Some(view),
    });
}

fn schedule(placement: Placement) {
    extern "C" fn apply(context: *mut c_void) {
        // SAFETY: `context` is the Box leaked below, run once on main.
        let p = unsafe { Box::from_raw(context as *mut Placement) };
        // SAFETY: AppKit window and screen calls on the main thread.
        unsafe {
            let screens: Retained<AnyObject> = msg_send![class!(NSScreen), screens];
            let primary: Option<Retained<AnyObject>> = msg_send![&*screens, firstObject];
            let Some(primary) = primary else { return };
            let full: NSRect = msg_send![&*primary, frame];
            let f = p.frame;
            let y = full.size.height - (f.y + f.h) as f64;
            let r = NSRect::new(NSPoint::new(f.x as f64, y), NSSize::new(f.w as f64, f.h as f64));
            if p.cover.is_some() {
                // NSWindowStyleMaskNonactivatingPanel alone: borderless, and
                // still a panel that does not activate other apps' windows.
                let _: () = msg_send![&*p.window, setStyleMask: 1usize << 7];
            }
            let _: () = msg_send![&*p.window, setFrame: r, display: true];
            if let Some(view) = &p.cover {
                // Changing the style mask resigns key status; take it back so
                // Esc and ⌘C still reach the overlay.
                let _: () = msg_send![&*p.window, makeKeyAndOrderFront: std::ptr::null_mut::<AnyObject>()];
                let _: bool = msg_send![&*p.window, makeFirstResponder: &**view];
            }
        }
    }
    let context = Box::into_raw(Box::new(placement)) as *mut c_void;
    // SAFETY: the main queue outlives the process; `apply` frees `context`.
    unsafe { dispatch_async_f(&_dispatch_main_q, context, apply) };
}
