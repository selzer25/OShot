//! Smaller AppKit services: appearance, login item, pasteboard, URLs, the
//! system screenshot folder and folder watching.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSString;
use oshot_core::settings::ThemeChoice;
use oshot_native::cf::{self, CFTypeRef, Cf};

use crate::state::PlatformEvent;

#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

pub fn open_url(url: &str) {
    // SAFETY: NSWorkspace and NSURL calls with a valid string.
    unsafe {
        let s = NSString::from_str(url);
        let url: Option<Retained<AnyObject>> = msg_send![class!(NSURL), URLWithString: &*s];
        if let Some(url) = url {
            let ws: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
            let _: bool = msg_send![&*ws, openURL: &*url];
        }
    }
}

pub fn set_appearance(choice: ThemeChoice) {
    // SAFETY: main-thread AppKit calls; nil restores the system appearance.
    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let name = match choice {
            ThemeChoice::System => None,
            ThemeChoice::Light => Some("NSAppearanceNameAqua"),
            ThemeChoice::Dark => Some("NSAppearanceNameDarkAqua"),
        };
        let appearance: Option<Retained<AnyObject>> = match name {
            Some(n) => {
                let s = NSString::from_str(n);
                msg_send![class!(NSAppearance), appearanceNamed: &*s]
            }
            None => None,
        };
        let _: () = msg_send![&*app, setAppearance: appearance.as_deref()];
    }
}

pub fn set_launch_at_login(on: bool) -> Result<(), String> {
    // SAFETY: SMAppService (macOS 13+) with a null error out-param.
    unsafe {
        let service: Retained<AnyObject> = msg_send![class!(SMAppService), mainAppService];
        let ok: bool = if on {
            msg_send![&*service, registerAndReturnError: std::ptr::null_mut::<*mut AnyObject>()]
        } else {
            msg_send![&*service, unregisterAndReturnError: std::ptr::null_mut::<*mut AnyObject>()]
        };
        if ok {
            Ok(())
        } else {
            Err("macOS did not allow changing the login item. Run OShot from the Applications folder.".into())
        }
    }
}

pub fn copy_files(paths: &[PathBuf]) {
    // SAFETY: pasteboard calls with valid file URLs.
    unsafe {
        let pb: Retained<AnyObject> = msg_send![class!(NSPasteboard), generalPasteboard];
        let _: isize = msg_send![&*pb, clearContents];
        let urls: Retained<AnyObject> = msg_send![class!(NSMutableArray), array];
        for p in paths {
            let s = NSString::from_str(&p.to_string_lossy());
            let url: Retained<AnyObject> = msg_send![class!(NSURL), fileURLWithPath: &*s];
            let _: () = msg_send![&*urls, addObject: &*url];
        }
        let _: bool = msg_send![&*pb, writeObjects: &*urls];
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFPreferencesCopyAppValue(key: CFTypeRef, app: CFTypeRef) -> CFTypeRef;
    fn CFArrayCreate(alloc: CFTypeRef, values: *const CFTypeRef, count: isize, callbacks: *const c_void) -> CFTypeRef;
    static kCFTypeArrayCallBacks: c_void;
}

pub fn system_screenshot_folder() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let key = cf::string("location");
    let app = cf::string("com.apple.screencapture");
    // SAFETY: returns a +1 value or null.
    let value = Cf::wrap(unsafe { CFPreferencesCopyAppValue(key.ptr(), app.ptr()) });
    // SAFETY: a preferences value we own; to_string checks it is a string.
    match value.and_then(|v| unsafe { cf::to_string(v.ptr()) }) {
        Some(s) if !s.is_empty() => {
            if let Some(rest) = s.strip_prefix("~/") {
                home.join(rest)
            } else {
                PathBuf::from(s)
            }
        }
        _ => home.join("Desktop"),
    }
}

type FsCallback = extern "C" fn(*const c_void, *mut c_void, usize, *mut c_void, *const u32, *const u64);

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn FSEventStreamCreate(
        alloc: CFTypeRef,
        callback: FsCallback,
        context: *const c_void,
        paths: CFTypeRef,
        since: u64,
        latency: f64,
        flags: u32,
    ) -> *mut c_void;
    fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: *mut c_void);
    fn FSEventStreamStart(stream: *mut c_void) -> bool;
    fn FSEventStreamStop(stream: *mut c_void);
    fn FSEventStreamInvalidate(stream: *mut c_void);
    fn FSEventStreamRelease(stream: *mut c_void);
}

unsafe extern "C" {
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
}

extern "C" fn on_fs_event(_: *const c_void, _: *mut c_void, _: usize, _: *mut c_void, _: *const u32, _: *const u64) {
    super::send(PlatformEvent::FolderChanged);
}

thread_local! {
    static STREAM: RefCell<Option<usize>> = const { RefCell::new(None) };
}

/// Watches `path` for new files (FSEvents, 0.5 s latency). `None` stops.
pub fn watch_folder(path: Option<&Path>) {
    STREAM.with(|s| {
        if let Some(old) = s.borrow_mut().take() {
            let stream = old as *mut c_void;
            // SAFETY: `stream` was created and started below.
            unsafe {
                FSEventStreamStop(stream);
                FSEventStreamInvalidate(stream);
                FSEventStreamRelease(stream);
            }
        }
        let Some(path) = path else { return };
        let p = cf::string(&path.to_string_lossy());
        // SAFETY: one CFString in a CF array; the stream gets the file-events
        // flag (0x10) and starts from now (u64::MAX).
        unsafe {
            let values = [p.ptr()];
            let Some(paths) = Cf::wrap(CFArrayCreate(
                std::ptr::null(),
                values.as_ptr(),
                1,
                &kCFTypeArrayCallBacks,
            )) else {
                return;
            };
            let stream = FSEventStreamCreate(
                std::ptr::null(),
                on_fs_event,
                std::ptr::null(),
                paths.ptr(),
                u64::MAX,
                0.5,
                0x10,
            );
            if stream.is_null() {
                return;
            }
            FSEventStreamSetDispatchQueue(stream, dispatch_get_global_queue(0, 0));
            FSEventStreamStart(stream);
            *s.borrow_mut() = Some(stream as usize);
        }
    });
}
