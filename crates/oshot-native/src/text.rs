//! CoreText layout for text annotations. The editor lays text out with the
//! same font names and line height, so measurements agree.

#![allow(non_upper_case_globals)]

use oshot_core::color::Color;
use oshot_core::prims::{Font, LINE_HEIGHT};

use crate::cf::{self, CFStringRef, CFTypeRef, Cf};
use crate::cg::{self, CGAffineTransform, CGContextRef};

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    static kCTFontAttributeName: CFStringRef;
    static kCTForegroundColorAttributeName: CFStringRef;
    fn CTFontCreateWithName(name: CFStringRef, size: f64, matrix: *const CGAffineTransform) -> CFTypeRef;
    fn CTFontGetAscent(font: CFTypeRef) -> f64;
    fn CTFontGetDescent(font: CFTypeRef) -> f64;
    fn CTLineCreateWithAttributedString(s: CFTypeRef) -> CFTypeRef;
    fn CTLineGetTypographicBounds(line: CFTypeRef, ascent: *mut f64, descent: *mut f64, leading: *mut f64) -> f64;
    fn CTLineDraw(line: CFTypeRef, ctx: CGContextRef);
}

/// PostScript name for each annotation font.
pub fn postscript_name(font: Font) -> &'static str {
    match font {
        Font::Regular => "HelveticaNeue",
        Font::Bold => "HelveticaNeue-Bold",
        Font::Hand => "ChalkboardSE-Regular",
    }
}

pub struct Line {
    line: Cf,
    pub width: f64,
}

pub struct Layout {
    pub lines: Vec<Line>,
    pub width: f64,
    pub height: f64,
    pub line_height: f64,
    /// Offset from a line's top to its baseline.
    pub baseline: f64,
}

pub fn layout(text: &str, size: f32, font: Font, color: Color) -> Layout {
    let name = cf::string(postscript_name(font));
    // SAFETY: valid font name and size; null matrix means identity.
    let ct_font = Cf::wrap(unsafe { CTFontCreateWithName(name.ptr(), size as f64, std::ptr::null()) }).expect("CTFont");
    // SAFETY: valid font.
    let (ascent, descent) = unsafe { (CTFontGetAscent(ct_font.ptr()), CTFontGetDescent(ct_font.ptr())) };
    let line_height = size as f64 * LINE_HEIGHT as f64;
    let baseline = (line_height - (ascent + descent)) / 2.0 + ascent;
    // SAFETY: plain sRGB components.
    let cg_color = Cf::wrap(unsafe {
        cg::CGColorCreateSRGB(
            color.r as f64 / 255.0,
            color.g as f64 / 255.0,
            color.b as f64 / 255.0,
            color.a as f64 / 255.0,
        )
    })
    .expect("CGColor");
    // SAFETY: the attribute keys are framework constants.
    let attrs = cf::dict(&[
        (unsafe { kCTFontAttributeName }, ct_font.ptr()),
        (unsafe { kCTForegroundColorAttributeName }, cg_color.ptr()),
    ]);
    let mut lines = Vec::new();
    let mut width = 0f64;
    for line_text in text.split('\n') {
        let s = cf::string(line_text);
        // SAFETY: valid string and attributes.
        let attributed = Cf::wrap(unsafe { cf::CFAttributedStringCreate(std::ptr::null(), s.ptr(), attrs.ptr()) })
            .expect("attributed string");
        // SAFETY: valid attributed string.
        let line = Cf::wrap(unsafe { CTLineCreateWithAttributedString(attributed.ptr()) }).expect("CTLine");
        // SAFETY: valid line; out-params may be null.
        let w = unsafe {
            CTLineGetTypographicBounds(
                line.ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        width = width.max(w);
        lines.push(Line { line, width: w });
    }
    let height = line_height * lines.len() as f64;
    Layout {
        lines,
        width,
        height,
        line_height,
        baseline,
    }
}

/// Size of `text` in pixels.
pub fn measure(text: &str, size: f32, font: Font) -> (f32, f32) {
    let l = layout(text, size, font, oshot_core::color::rgb(0, 0, 0));
    (l.width.ceil() as f32, l.height.ceil() as f32)
}

impl Layout {
    /// Draws with the top-left of the block at (`x`, `y`) in a y-down context.
    pub fn draw(&self, cx: &cg::Context, x: f64, y: f64) {
        let ctx = cx.ptr();
        // SAFETY: valid context; the text matrix flip undoes the context's
        // y-down flip so glyphs are upright.
        unsafe {
            cg::CGContextSetTextMatrix(
                ctx,
                CGAffineTransform {
                    a: 1.0,
                    b: 0.0,
                    c: 0.0,
                    d: -1.0,
                    tx: 0.0,
                    ty: 0.0,
                },
            );
            for (i, line) in self.lines.iter().enumerate() {
                cg::CGContextSetTextPosition(ctx, x, y + i as f64 * self.line_height + self.baseline);
                CTLineDraw(line.line.ptr(), ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_grow_with_text_and_lines() {
        let (w1, h1) = measure("Hi", 20.0, Font::Bold);
        let (w2, _) = measure("Hello there", 20.0, Font::Bold);
        let (_, h3) = measure("a\nb", 20.0, Font::Bold);
        assert!(w1 > 10.0 && w2 > w1 * 2.0);
        assert_eq!(h1, 25.0);
        assert_eq!(h3, 50.0);
    }
}
