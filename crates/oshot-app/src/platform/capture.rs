//! Screen and window capture through ScreenCaptureKit, plus the display and
//! window geometry the capture overlay needs.

use std::ffi::c_void;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::encode::{Encoding, RefEncode};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use oshot_core::geometry::{Pt, Rect, pt, rect};
use oshot_native::cf::{self, CFTypeRef, Cf};
use oshot_native::cg::{self, CGRect};

use crate::state::{Capture, DisplayInfo, WindowInfo};

#[repr(C)]
pub struct CGImageOpaque {
    _private: [u8; 0],
}

// SAFETY: matches the Objective-C encoding of CGImageRef.
unsafe impl RefEncode for CGImageOpaque {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
}

#[link(name = "ScreenCaptureKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGDisplayCopyDisplayMode(display: u32) -> CFTypeRef;
    fn CGDisplayModeGetPixelWidth(mode: CFTypeRef) -> usize;
    fn CGDisplayModeGetWidth(mode: CFTypeRef) -> usize;
    fn CGEventCreate(source: CFTypeRef) -> CFTypeRef;
    fn CGEventGetLocation(event: CFTypeRef) -> cg::CGPoint;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
    fn CGRectMakeWithDictionaryRepresentation(dict: CFTypeRef, rect: *mut CGRect) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFArrayGetCount(array: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
    fn CFNumberGetValue(number: CFTypeRef, kind: isize, out: *mut c_void) -> bool;
}

unsafe extern "C" {
    static kCGWindowNumber: CFTypeRef;
    static kCGWindowLayer: CFTypeRef;
    static kCGWindowBounds: CFTypeRef;
    static kCGWindowOwnerName: CFTypeRef;
    static kCGWindowName: CFTypeRef;
    static kCGWindowOwnerPID: CFTypeRef;
}

pub fn has_permission() -> bool {
    // SAFETY: no arguments.
    unsafe { CGPreflightScreenCaptureAccess() }
}

pub fn request_permission() {
    // SAFETY: no arguments; shows the system prompt once.
    unsafe { CGRequestScreenCaptureAccess() };
}

pub fn displays() -> Vec<DisplayInfo> {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    // SAFETY: buffer of 16 ids.
    if unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut count) } != 0 {
        return Vec::new();
    }
    ids[..count as usize]
        .iter()
        .map(|&id| {
            // SAFETY: id comes from the active display list.
            let (b, scale) = unsafe {
                let b = CGDisplayBounds(id);
                let mode = Cf::wrap(CGDisplayCopyDisplayMode(id));
                let scale = mode
                    .map(|m| CGDisplayModeGetPixelWidth(m.ptr()) as f32 / CGDisplayModeGetWidth(m.ptr()).max(1) as f32)
                    .unwrap_or(1.0);
                (b, scale)
            };
            DisplayInfo {
                id,
                frame: rect(
                    b.origin.x as f32,
                    b.origin.y as f32,
                    b.size.width as f32,
                    b.size.height as f32,
                ),
                scale: scale.max(1.0),
            }
        })
        .collect()
}

pub fn cursor() -> Pt {
    // SAFETY: a null-source event just reports the current cursor location.
    unsafe {
        match Cf::wrap(CGEventCreate(std::ptr::null())) {
            Some(e) => {
                let p = CGEventGetLocation(e.ptr());
                pt(p.x as f32, p.y as f32)
            }
            None => pt(0.0, 0.0),
        }
    }
}

fn dict_i64(dict: CFTypeRef, key: CFTypeRef) -> Option<i64> {
    // SAFETY: borrowed value from a valid dictionary; type 4 is SInt64.
    unsafe {
        let v = cf::CFDictionaryGetValue(dict, key);
        if v.is_null() {
            return None;
        }
        let mut out = 0i64;
        CFNumberGetValue(v, 4, &mut out as *mut i64 as *mut c_void).then_some(out)
    }
}

pub fn windows() -> Vec<WindowInfo> {
    let own_pid = std::process::id() as i64;
    // Options: on-screen only (1) and no desktop elements (16).
    // SAFETY: returns a +1 CFArray of CFDictionaries, released by `Cf`.
    let Some(list) = Cf::wrap(unsafe { CGWindowListCopyWindowInfo(1 | 16, 0) }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // SAFETY: indices are within the array; values are borrowed.
    unsafe {
        for i in 0..CFArrayGetCount(list.ptr()) {
            let d = CFArrayGetValueAtIndex(list.ptr(), i);
            if dict_i64(d, kCGWindowLayer) != Some(0) || dict_i64(d, kCGWindowOwnerPID) == Some(own_pid) {
                continue;
            }
            let Some(id) = dict_i64(d, kCGWindowNumber) else {
                continue;
            };
            let mut r = CGRect::default();
            let bounds = cf::CFDictionaryGetValue(d, kCGWindowBounds);
            if bounds.is_null() || !CGRectMakeWithDictionaryRepresentation(bounds, &mut r) {
                continue;
            }
            if r.size.width < 40.0 || r.size.height < 40.0 {
                continue;
            }
            out.push(WindowInfo {
                id: id as u32,
                frame: rect(
                    r.origin.x as f32,
                    r.origin.y as f32,
                    r.size.width as f32,
                    r.size.height as f32,
                ),
                owner: cf::to_string(cf::CFDictionaryGetValue(d, kCGWindowOwnerName)).unwrap_or_default(),
                title: cf::to_string(cf::CFDictionaryGetValue(d, kCGWindowName)).unwrap_or_default(),
            });
        }
    }
    out
}

/// On-screen, normal-layer window frames owned by process `pid`.
pub fn window_frames_of(pid: i64) -> Vec<Rect> {
    // SAFETY: as in `windows`: a +1 array of borrowed dictionaries.
    let Some(list) = Cf::wrap(unsafe { CGWindowListCopyWindowInfo(1 | 16, 0) }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // SAFETY: indices are within the array; values are borrowed.
    unsafe {
        for i in 0..CFArrayGetCount(list.ptr()) {
            let d = CFArrayGetValueAtIndex(list.ptr(), i);
            if dict_i64(d, kCGWindowLayer) != Some(0) || dict_i64(d, kCGWindowOwnerPID) != Some(pid) {
                continue;
            }
            let mut r = CGRect::default();
            let bounds = cf::CFDictionaryGetValue(d, kCGWindowBounds);
            if !bounds.is_null() && CGRectMakeWithDictionaryRepresentation(bounds, &mut r) {
                out.push(rect(
                    r.origin.x as f32,
                    r.origin.y as f32,
                    r.size.width as f32,
                    r.size.height as f32,
                ));
            }
        }
    }
    out
}

/// Converts a CGImage handed to a completion block into a capture.
fn to_capture(image: *mut CGImageOpaque, points_wide: f32) -> Result<Capture, String> {
    if image.is_null() {
        return Err("Screen capture returned no image. Check Screen Recording permission.".into());
    }
    // SAFETY: ScreenCaptureKit hands the completion block a valid CGImage,
    // alive for the duration of the block.
    let bitmap = unsafe { cg::bitmap_from_image(image as CFTypeRef) }.ok_or("Could not read captured pixels")?;
    let scale = if points_wide > 0.0 {
        bitmap.width as f32 / points_wide
    } else {
        1.0
    };
    Ok(Capture {
        bitmap,
        scale: scale.max(1.0),
    })
}

const TIMEOUT: Duration = Duration::from_secs(8);

pub fn capture_rect(area: Rect) -> Result<Capture, String> {
    let (tx, rx) = mpsc::channel();
    let width = area.w;
    let block = RcBlock::new(move |image: *mut CGImageOpaque, _error: *mut AnyObject| {
        let _ = tx.send(to_capture(image, width));
    });
    let r = NSRect::new(
        NSPoint::new(area.x as f64, area.y as f64),
        NSSize::new(area.w as f64, area.h as f64),
    );
    // SAFETY: documented ScreenCaptureKit class method (macOS 15.2+); the
    // block matches `void (^)(CGImageRef, NSError *)`.
    unsafe {
        let _: () = msg_send![class!(SCScreenshotManager), captureImageInRect: r, completionHandler: &*block];
    }
    rx.recv_timeout(TIMEOUT)
        .map_err(|_| "Screen capture timed out".to_string())?
}

/// Retained Objective-C object that we move between threads. ScreenCaptureKit
/// objects are immutable snapshots, so sharing them is safe.
struct SendObject(Retained<AnyObject>);
// SAFETY: see above; the object is only read.
unsafe impl Send for SendObject {}

pub fn capture_window(id: u32, shadow: bool) -> Result<Capture, String> {
    let (tx, rx) = mpsc::channel::<Option<SendObject>>();
    let block = RcBlock::new(move |content: *mut AnyObject, _error: *mut AnyObject| {
        // SAFETY: the content object is valid for the block's duration; we
        // retain it before sending.
        let retained = unsafe { Retained::retain(content) };
        let _ = tx.send(retained.map(SendObject));
    });
    // SAFETY: documented SCShareableContent class method.
    unsafe {
        let _: () = msg_send![
            class!(SCShareableContent),
            getShareableContentExcludingDesktopWindows: true,
            onScreenWindowsOnly: true,
            completionHandler: &*block
        ];
    }
    let content = rx
        .recv_timeout(TIMEOUT)
        .map_err(|_| "Listing windows timed out".to_string())?;
    let content = content
        .ok_or("Screen Recording permission is needed to capture windows")?
        .0;

    // SAFETY: Objective-C calls on valid ScreenCaptureKit objects, matching
    // the documented selectors and types.
    unsafe {
        let windows: Retained<AnyObject> = msg_send![&*content, windows];
        let count: usize = msg_send![&*windows, count];
        let mut target: Option<Retained<AnyObject>> = None;
        for i in 0..count {
            let w: Retained<AnyObject> = msg_send![&*windows, objectAtIndex: i];
            let wid: u32 = msg_send![&*w, windowID];
            if wid == id {
                target = Some(w);
                break;
            }
        }
        let window = target.ok_or("That window is no longer on screen")?;
        let filter: Allocated<AnyObject> = msg_send![class!(SCContentFilter), alloc];
        let filter: Retained<AnyObject> = msg_send![filter, initWithDesktopIndependentWindow: &*window];
        let content_rect: NSRect = msg_send![&*filter, contentRect];
        let scale: f32 = msg_send![&*filter, pointPixelScale];
        let config: Retained<AnyObject> = msg_send![class!(SCStreamConfiguration), new];
        let _: () = msg_send![&*config, setWidth: (content_rect.size.width * scale as f64).round() as usize];
        let _: () = msg_send![&*config, setHeight: (content_rect.size.height * scale as f64).round() as usize];
        let _: () = msg_send![&*config, setShowsCursor: false];
        let _: () = msg_send![&*config, setIgnoreShadowsSingleWindow: !shadow];
        let _: () = msg_send![&*config, setScalesToFit: false];

        let (tx, rx) = mpsc::channel();
        let points = content_rect.size.width as f32;
        let block = RcBlock::new(move |image: *mut CGImageOpaque, _error: *mut AnyObject| {
            let _ = tx.send(to_capture(image, points));
        });
        let _: () = msg_send![
            class!(SCScreenshotManager),
            captureImageWithFilter: &*filter,
            configuration: &*config,
            completionHandler: &*block
        ];
        rx.recv_timeout(TIMEOUT)
            .map_err(|_| "Window capture timed out".to_string())?
    }
}
