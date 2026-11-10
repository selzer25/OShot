//! Minimal CoreFoundation bindings.

#![allow(non_upper_case_globals)]

use std::ffi::c_void;

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFDataRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFIndex = isize;

#[repr(C)]
pub struct CFDictionaryCallBacks {
    _private: [u8; 0],
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFStringCreateWithBytes(
        alloc: CFTypeRef,
        bytes: *const u8,
        len: CFIndex,
        encoding: u32,
        external: bool,
    ) -> CFStringRef;
    pub fn CFStringGetLength(s: CFStringRef) -> CFIndex;
    pub fn CFStringGetCString(s: CFStringRef, buf: *mut u8, size: CFIndex, encoding: u32) -> bool;
    pub fn CFStringGetMaximumSizeForEncoding(len: CFIndex, encoding: u32) -> CFIndex;
    pub fn CFDataCreate(alloc: CFTypeRef, bytes: *const u8, len: CFIndex) -> CFDataRef;
    pub fn CFDataCreateWithBytesNoCopy(
        alloc: CFTypeRef,
        bytes: *const u8,
        len: CFIndex,
        deallocator: CFTypeRef,
    ) -> CFDataRef;
    pub fn CFDataCreateMutable(alloc: CFTypeRef, capacity: CFIndex) -> CFDataRef;
    pub fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
    pub fn CFDataGetLength(data: CFDataRef) -> CFIndex;
    pub fn CFDictionaryCreate(
        alloc: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: CFIndex,
        key_callbacks: *const CFDictionaryCallBacks,
        value_callbacks: *const CFDictionaryCallBacks,
    ) -> CFDictionaryRef;
    pub fn CFDictionaryCreateMutableCopy(alloc: CFTypeRef, capacity: CFIndex, dict: CFDictionaryRef)
    -> CFDictionaryRef;
    pub fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    pub fn CFDictionarySetValue(dict: CFDictionaryRef, key: CFTypeRef, value: CFTypeRef);
    pub fn CFDictionaryRemoveValue(dict: CFDictionaryRef, key: CFTypeRef);
    pub fn CFNumberCreate(alloc: CFTypeRef, kind: CFIndex, value: *const c_void) -> CFTypeRef;
    pub fn CFNumberGetValue(number: CFTypeRef, kind: CFIndex, value: *mut c_void) -> bool;
    pub fn CFGetTypeID(cf: CFTypeRef) -> usize;
    pub fn CFNumberGetTypeID() -> usize;
    pub fn CFAttributedStringCreate(alloc: CFTypeRef, s: CFStringRef, attrs: CFDictionaryRef) -> CFTypeRef;

    pub static kCFTypeDictionaryKeyCallBacks: CFDictionaryCallBacks;
    pub static kCFTypeDictionaryValueCallBacks: CFDictionaryCallBacks;
    pub static kCFBooleanTrue: CFTypeRef;
    pub static kCFBooleanFalse: CFTypeRef;
    pub static kCFAllocatorNull: CFTypeRef;
}

pub const UTF8: u32 = 0x0800_0100;
pub const NUMBER_SINT32: CFIndex = 3;
pub const NUMBER_FLOAT64: CFIndex = 13;

/// An owned CoreFoundation (or CoreGraphics/CoreText) object, released on drop.
pub struct Cf(CFTypeRef);

impl Cf {
    /// Takes ownership of a +1 reference. Returns `None` for null.
    pub fn wrap(ptr: CFTypeRef) -> Option<Cf> {
        // `then`, not `then_some`: an eagerly built Cf(null) would be
        // dropped and CFRelease(NULL) traps.
        (!ptr.is_null()).then(|| Cf(ptr))
    }

    /// Retains a borrowed (+0) reference.
    ///
    /// # Safety
    /// `ptr` must be null or a valid CoreFoundation object.
    pub unsafe fn retain(ptr: CFTypeRef) -> Option<Cf> {
        // SAFETY: the caller guarantees `ptr` is a live CF object.
        (!ptr.is_null()).then(|| Cf(unsafe { CFRetain(ptr) }))
    }

    pub fn ptr(&self) -> CFTypeRef {
        self.0
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: we own one reference.
        unsafe { CFRelease(self.0) }
    }
}

pub fn string(s: &str) -> Cf {
    // SAFETY: the bytes are valid UTF-8 for the given length.
    Cf::wrap(unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as CFIndex, UTF8, false) })
        .expect("CFString from valid UTF-8")
}

/// # Safety
/// `s` must be null or a valid CFString.
pub unsafe fn to_string(s: CFStringRef) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: `s` is a valid CFString; the buffer is sized by CF itself.
    unsafe {
        let len = CFStringGetLength(s);
        let size = CFStringGetMaximumSizeForEncoding(len, UTF8) + 1;
        let mut buf = vec![0u8; size as usize];
        if !CFStringGetCString(s, buf.as_mut_ptr(), size, UTF8) {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        buf.truncate(end);
        String::from_utf8(buf).ok()
    }
}

pub fn number_f64(v: f64) -> Cf {
    // SAFETY: the value pointer matches the declared number type.
    Cf::wrap(unsafe { CFNumberCreate(std::ptr::null(), NUMBER_FLOAT64, &v as *const f64 as *const c_void) })
        .expect("CFNumber")
}

pub fn number_i32(v: i32) -> Cf {
    // SAFETY: the value pointer matches the declared number type.
    Cf::wrap(unsafe { CFNumberCreate(std::ptr::null(), NUMBER_SINT32, &v as *const i32 as *const c_void) })
        .expect("CFNumber")
}

/// Reads a CFNumber as f64; `None` if `v` is not a number.
///
/// # Safety
/// `v` must be null or a valid CoreFoundation object.
pub unsafe fn get_f64(v: CFTypeRef) -> Option<f64> {
    // SAFETY: the type is checked before reading.
    unsafe {
        if v.is_null() || CFGetTypeID(v) != CFNumberGetTypeID() {
            return None;
        }
        let mut out = 0f64;
        CFNumberGetValue(v, NUMBER_FLOAT64, &mut out as *mut f64 as *mut c_void).then_some(out)
    }
}

/// Builds an immutable dictionary from borrowed keys and values.
pub fn dict(pairs: &[(CFTypeRef, CFTypeRef)]) -> Cf {
    let keys: Vec<CFTypeRef> = pairs.iter().map(|p| p.0).collect();
    let values: Vec<CFTypeRef> = pairs.iter().map(|p| p.1).collect();
    // SAFETY: arrays have `pairs.len()` valid CF objects; CF retains them.
    Cf::wrap(unsafe {
        CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            pairs.len() as CFIndex,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    })
    .expect("CFDictionary")
}

pub fn data(bytes: &[u8]) -> Cf {
    // SAFETY: CFDataCreate copies `bytes`.
    Cf::wrap(unsafe { CFDataCreate(std::ptr::null(), bytes.as_ptr(), bytes.len() as CFIndex) }).expect("CFData")
}

/// Wraps `bytes` without copying. The returned object must not outlive them.
pub fn data_no_copy(bytes: &[u8]) -> Cf {
    // SAFETY: kCFAllocatorNull tells CF not to free the borrowed bytes.
    Cf::wrap(unsafe {
        CFDataCreateWithBytesNoCopy(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as CFIndex,
            kCFAllocatorNull,
        )
    })
    .expect("CFData")
}

pub fn data_bytes(data: &Cf) -> Vec<u8> {
    // SAFETY: `data` is a CFData; pointer and length come from it.
    unsafe {
        let len = CFDataGetLength(data.ptr()) as usize;
        let ptr = CFDataGetBytePtr(data.ptr());
        if ptr.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(ptr, len).to_vec()
        }
    }
}
