//! Minimal CoreGraphics bindings and helpers for moving pixels between
//! `Bitmap` and `CGImage`.

#![allow(non_upper_case_globals)]

use std::ffi::c_void;

use oshot_core::bitmap::{Bitmap, BitmapRef};

use crate::cf::{CFStringRef, CFTypeRef, Cf};

pub type CGFloat = f64;
pub type CGContextRef = *const c_void;
pub type CGImageRef = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGPoint {
    pub x: CGFloat,
    pub y: CGFloat,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGSize {
    pub width: CGFloat,
    pub height: CGFloat,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

pub fn cg_rect(x: f64, y: f64, w: f64, h: f64) -> CGRect {
    CGRect {
        origin: CGPoint { x, y },
        size: CGSize { width: w, height: h },
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGAffineTransform {
    pub a: CGFloat,
    pub b: CGFloat,
    pub c: CGFloat,
    pub d: CGFloat,
    pub tx: CGFloat,
    pub ty: CGFloat,
}

/// Premultiplied BGRA, the native layout of a CoreGraphics bitmap context.
pub const BGRA_PREMULTIPLIED: u32 = 2 | (2 << 12);
/// Straight-alpha BGRA, accepted for images (not for contexts).
pub const BGRA_STRAIGHT: u32 = 4 | (2 << 12);

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub static kCGColorSpaceSRGB: CFStringRef;
    pub fn CGColorSpaceCreateWithName(name: CFStringRef) -> CFTypeRef;
    pub fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: CFTypeRef,
        info: u32,
    ) -> CGContextRef;
    pub fn CGBitmapContextGetData(c: CGContextRef) -> *mut c_void;
    pub fn CGBitmapContextCreateImage(c: CGContextRef) -> CGImageRef;
    pub fn CGDataProviderCreateWithCFData(data: CFTypeRef) -> CFTypeRef;
    pub fn CGImageCreate(
        width: usize,
        height: usize,
        bits_per_component: usize,
        bits_per_pixel: usize,
        bytes_per_row: usize,
        space: CFTypeRef,
        info: u32,
        provider: CFTypeRef,
        decode: *const CGFloat,
        interpolate: bool,
        intent: i32,
    ) -> CGImageRef;
    pub fn CGImageGetWidth(image: CGImageRef) -> usize;
    pub fn CGImageGetHeight(image: CGImageRef) -> usize;
    pub fn CGContextDrawImage(c: CGContextRef, rect: CGRect, image: CGImageRef);
    pub fn CGContextSetInterpolationQuality(c: CGContextRef, quality: i32);
    pub fn CGContextSaveGState(c: CGContextRef);
    pub fn CGContextRestoreGState(c: CGContextRef);
    pub fn CGContextTranslateCTM(c: CGContextRef, tx: CGFloat, ty: CGFloat);
    pub fn CGContextScaleCTM(c: CGContextRef, sx: CGFloat, sy: CGFloat);
    pub fn CGContextSetRGBFillColor(c: CGContextRef, r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat);
    pub fn CGContextSetRGBStrokeColor(c: CGContextRef, r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat);
    pub fn CGContextSetLineWidth(c: CGContextRef, w: CGFloat);
    pub fn CGContextSetLineCap(c: CGContextRef, cap: i32);
    pub fn CGContextSetLineJoin(c: CGContextRef, join: i32);
    pub fn CGContextBeginPath(c: CGContextRef);
    pub fn CGContextMoveToPoint(c: CGContextRef, x: CGFloat, y: CGFloat);
    pub fn CGContextAddLineToPoint(c: CGContextRef, x: CGFloat, y: CGFloat);
    pub fn CGContextClosePath(c: CGContextRef);
    pub fn CGContextAddPath(c: CGContextRef, path: CFTypeRef);
    pub fn CGContextAddEllipseInRect(c: CGContextRef, rect: CGRect);
    pub fn CGContextFillPath(c: CGContextRef);
    pub fn CGContextStrokePath(c: CGContextRef);
    pub fn CGContextFillRect(c: CGContextRef, rect: CGRect);
    pub fn CGContextClip(c: CGContextRef);
    pub fn CGContextClipToRect(c: CGContextRef, rect: CGRect);
    pub fn CGContextSetAlpha(c: CGContextRef, a: CGFloat);
    pub fn CGContextSetShadowWithColor(c: CGContextRef, offset: CGSize, blur: CGFloat, color: CFTypeRef);
    pub fn CGContextBeginTransparencyLayer(c: CGContextRef, aux: CFTypeRef);
    pub fn CGContextEndTransparencyLayer(c: CGContextRef);
    pub fn CGContextSetTextMatrix(c: CGContextRef, t: CGAffineTransform);
    pub fn CGContextSetTextPosition(c: CGContextRef, x: CGFloat, y: CGFloat);
    pub fn CGColorCreateSRGB(r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat) -> CFTypeRef;
    pub fn CGPathCreateWithRoundedRect(
        rect: CGRect,
        rw: CGFloat,
        rh: CGFloat,
        t: *const CGAffineTransform,
    ) -> CFTypeRef;
    pub fn CGGradientCreateWithColorComponents(
        space: CFTypeRef,
        components: *const CGFloat,
        locations: *const CGFloat,
        count: usize,
    ) -> CFTypeRef;
    pub fn CGContextDrawLinearGradient(c: CGContextRef, g: CFTypeRef, start: CGPoint, end: CGPoint, options: u32);
}

pub fn srgb() -> Cf {
    // SAFETY: kCGColorSpaceSRGB is a valid constant name.
    Cf::wrap(unsafe { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) }).expect("sRGB colour space")
}

/// A BGRA bitmap context drawing straight into a buffer it owns, so taking
/// the pixels out never copies them.
pub struct Context {
    pub cg: Cf,
    pub width: u32,
    pub height: u32,
    buffer: Vec<u8>,
}

impl Context {
    pub fn new(width: u32, height: u32) -> Option<Context> {
        let (width, height) = (width.max(1), height.max(1));
        let mut buffer = vec![0u8; width as usize * height as usize * 4];
        let space = srgb();
        // SAFETY: `buffer` holds width * height BGRA pixels at this stride and
        // lives as long as the context (both are owned by the struct, and the
        // Vec's heap allocation never moves).
        let ctx = unsafe {
            CGBitmapContextCreate(
                buffer.as_mut_ptr() as *mut c_void,
                width as usize,
                height as usize,
                8,
                width as usize * 4,
                space.ptr(),
                BGRA_PREMULTIPLIED,
            )
        };
        Some(Context {
            cg: Cf::wrap(ctx)?,
            width,
            height,
            buffer,
        })
    }

    pub fn ptr(&self) -> CGContextRef {
        self.cg.ptr()
    }

    /// Takes the pixels, converting to straight alpha in place.
    pub fn into_bitmap(self) -> Bitmap {
        let Context {
            cg,
            width,
            height,
            mut buffer,
        } = self;
        drop(cg);
        unpremultiply(&mut buffer);
        Bitmap {
            width,
            height,
            data: buffer,
        }
    }
}

pub fn unpremultiply(data: &mut [u8]) {
    for px in data.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

/// A CGImage over `bitmap`'s pixels. The data is copied once into a CFData
/// so the image is safe to hand to frameworks that keep it.
pub fn image_from_bitmap(bitmap: BitmapRef) -> Option<Cf> {
    let data = crate::cf::data(bitmap.data);
    // SAFETY: `data` is a valid CFData; the provider retains it.
    let provider = Cf::wrap(unsafe { CGDataProviderCreateWithCFData(data.ptr()) })?;
    let space = srgb();
    // SAFETY: dimensions and stride match the buffer.
    Cf::wrap(unsafe {
        CGImageCreate(
            bitmap.width as usize,
            bitmap.height as usize,
            8,
            32,
            bitmap.width as usize * 4,
            space.ptr(),
            BGRA_STRAIGHT,
            provider.ptr(),
            std::ptr::null(),
            false,
            0,
        )
    })
}

/// Draws any CGImage into a fresh BGRA bitmap.
///
/// # Safety
/// `image` must be a valid CGImage.
pub unsafe fn bitmap_from_image(image: CGImageRef) -> Option<Bitmap> {
    // SAFETY: `image` is a valid CGImage.
    let (w, h) = unsafe { (CGImageGetWidth(image) as u32, CGImageGetHeight(image) as u32) };
    let ctx = Context::new(w, h)?;
    // SAFETY: the context is valid and sized to the image.
    unsafe { CGContextDrawImage(ctx.ptr(), cg_rect(0.0, 0.0, w as f64, h as f64), image) };
    Some(ctx.into_bitmap())
}
