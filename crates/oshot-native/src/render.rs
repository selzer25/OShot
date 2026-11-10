//! Exports a document to pixels with CoreGraphics, drawing the same
//! primitives the editor shows.

use oshot_core::bitmap::{Bitmap, BitmapRef};
use oshot_core::color::{Color, rgb};
use oshot_core::effects::EffectCache;
use oshot_core::geometry::{Pt, Rect, rect};
use oshot_core::model::{BACKDROP_PRESETS, Document, Shape};
use oshot_core::prims::{Anchor, Prim, document_prims};

use crate::cf::Cf;
use crate::cg::{self, CGPoint, CGSize, cg_rect};
use crate::text;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportOptions {
    /// Leave image overlays out (the "before" frame of a before/after GIF).
    pub skip_overlays: bool,
    /// Output scale; 0.5 halves a Retina capture.
    pub scale: f32,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            skip_overlays: false,
            scale: 1.0,
        }
    }
}

fn set_fill(cx: &cg::Context, c: Color) {
    let ctx = cx.ptr();
    // SAFETY: valid context.
    unsafe {
        cg::CGContextSetRGBFillColor(
            ctx,
            c.r as f64 / 255.0,
            c.g as f64 / 255.0,
            c.b as f64 / 255.0,
            c.a as f64 / 255.0,
        )
    }
}

fn set_stroke(cx: &cg::Context, c: Color) {
    let ctx = cx.ptr();
    // SAFETY: valid context.
    unsafe {
        cg::CGContextSetRGBStrokeColor(
            ctx,
            c.r as f64 / 255.0,
            c.g as f64 / 255.0,
            c.b as f64 / 255.0,
            c.a as f64 / 255.0,
        )
    }
}

fn add_polygon(cx: &cg::Context, points: &[Pt], close: bool) {
    let ctx = cx.ptr();
    let Some(first) = points.first() else { return };
    // SAFETY: valid context.
    unsafe {
        cg::CGContextMoveToPoint(ctx, first.x as f64, first.y as f64);
        for p in &points[1..] {
            cg::CGContextAddLineToPoint(ctx, p.x as f64, p.y as f64);
        }
        if close {
            cg::CGContextClosePath(ctx);
        }
    }
}

fn add_rounded_rect(cx: &cg::Context, r: Rect, radius: f32) {
    let ctx = cx.ptr();
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0) as f64;
    // SAFETY: valid rect and radius within bounds.
    let path = Cf::wrap(unsafe {
        cg::CGPathCreateWithRoundedRect(
            cg_rect(r.x as f64, r.y as f64, r.w as f64, r.h as f64),
            radius,
            radius,
            std::ptr::null(),
        )
    })
    .expect("CGPath");
    // SAFETY: valid context and path.
    unsafe { cg::CGContextAddPath(ctx, path.ptr()) };
}

/// Draws `bitmap` into `dest` in a y-down context.
fn draw_bitmap(cx: &cg::Context, bitmap: BitmapRef, dest: Rect, smooth: bool) {
    let ctx = cx.ptr();
    let Some(image) = cg::image_from_bitmap(bitmap) else {
        return;
    };
    // SAFETY: valid context and image. CGContextDrawImage assumes y-up, so
    // flip locally around the destination.
    unsafe {
        cg::CGContextSaveGState(ctx);
        cg::CGContextSetInterpolationQuality(ctx, if smooth { 3 } else { 1 });
        cg::CGContextTranslateCTM(ctx, dest.x as f64, (dest.y + dest.h) as f64);
        cg::CGContextScaleCTM(ctx, 1.0, -1.0);
        cg::CGContextDrawImage(ctx, cg_rect(0.0, 0.0, dest.w as f64, dest.h as f64), image.ptr());
        cg::CGContextRestoreGState(ctx);
    }
}

/// Paints one primitive. Shared by export and tests.
pub fn draw_prim(
    cx: &cg::Context,
    prim: &Prim,
    doc: &Document,
    image: BitmapRef,
    effects: &mut EffectCache,
    opts: ExportOptions,
) {
    let ctx = cx.ptr();
    match prim {
        Prim::Fill { contours, color } => {
            set_fill(cx, *color);
            // SAFETY: valid context.
            unsafe { cg::CGContextBeginPath(ctx) };
            for c in contours {
                add_polygon(cx, c, true);
            }
            // SAFETY: valid context with a path.
            unsafe { cg::CGContextFillPath(ctx) };
        }
        Prim::Stroke {
            points,
            width,
            color,
            closed,
        } => {
            set_stroke(cx, *color);
            // SAFETY: valid context.
            unsafe {
                cg::CGContextSetLineWidth(ctx, *width as f64);
                cg::CGContextSetLineCap(ctx, 1);
                cg::CGContextSetLineJoin(ctx, 1);
                cg::CGContextBeginPath(ctx);
            }
            add_polygon(cx, points, *closed);
            // SAFETY: valid context with a path.
            unsafe { cg::CGContextStrokePath(ctx) };
        }
        Prim::Text {
            text,
            at,
            anchor,
            size,
            color,
            font,
            bubble,
        } => {
            let layout = text::layout(text, *size, *font, *color);
            let pad = bubble.map_or(0.0, |b| b.padding) as f64;
            let (bw, bh) = (layout.width + pad * 2.0, layout.height + pad * 2.0);
            let (x, y) = match anchor {
                Anchor::TopLeft => (at.x as f64, at.y as f64),
                Anchor::Center => (at.x as f64 - bw / 2.0, at.y as f64 - bh / 2.0),
            };
            if let Some(b) = bubble {
                set_fill(cx, b.color);
                // SAFETY: valid context.
                unsafe { cg::CGContextBeginPath(ctx) };
                add_rounded_rect(cx, rect(x as f32, y as f32, bw as f32, bh as f32), b.radius);
                // SAFETY: valid context with a path.
                unsafe { cg::CGContextFillPath(ctx) };
            }
            layout.draw(cx, x + pad, y + pad);
        }
        Prim::Effect { object, dest, circle } => {
            let Some(obj) = doc.get(*object) else { return };
            let Some((patch_rect, patch)) = effects.get(obj, image) else {
                return;
            };
            let target = if *circle { *dest } else { patch_rect };
            // SAFETY: valid context.
            unsafe { cg::CGContextSaveGState(ctx) };
            if *circle {
                // SAFETY: valid context.
                unsafe {
                    cg::CGContextBeginPath(ctx);
                    cg::CGContextAddEllipseInRect(
                        ctx,
                        cg_rect(dest.x as f64, dest.y as f64, dest.w as f64, dest.h as f64),
                    );
                    cg::CGContextClip(ctx);
                }
            }
            draw_bitmap(cx, patch.view(), target, false);
            // SAFETY: balanced with the save above.
            unsafe { cg::CGContextRestoreGState(ctx) };
        }
        Prim::Image { object, dest, opacity } => {
            if opts.skip_overlays {
                return;
            }
            let Some(Shape::Image { image: overlay, .. }) = doc.get(*object).map(|o| &o.shape) else {
                return;
            };
            // SAFETY: valid context.
            unsafe {
                cg::CGContextSaveGState(ctx);
                cg::CGContextSetAlpha(ctx, *opacity as f64);
            }
            draw_bitmap(cx, overlay.view(), *dest, true);
            // SAFETY: balanced with the save above.
            unsafe { cg::CGContextRestoreGState(ctx) };
        }
    }
}

fn draw_backdrop(cx: &cg::Context, doc: &Document, w: f64, h: f64) {
    let ctx = cx.ptr();
    let (a, b) = BACKDROP_PRESETS[doc.backdrop.preset.min(BACKDROP_PRESETS.len() - 1)];
    let comps = [
        a.r as f64 / 255.0,
        a.g as f64 / 255.0,
        a.b as f64 / 255.0,
        1.0,
        b.r as f64 / 255.0,
        b.g as f64 / 255.0,
        b.b as f64 / 255.0,
        1.0,
    ];
    let locs = [0.0, 1.0];
    let space = cg::srgb();
    // SAFETY: two RGBA stops in sRGB.
    let gradient =
        Cf::wrap(unsafe { cg::CGGradientCreateWithColorComponents(space.ptr(), comps.as_ptr(), locs.as_ptr(), 2) })
            .expect("CGGradient");
    // SAFETY: valid context and gradient; option 3 extends past both ends.
    unsafe {
        cg::CGContextDrawLinearGradient(
            ctx,
            gradient.ptr(),
            CGPoint { x: 0.0, y: 0.0 },
            CGPoint { x: w, y: h },
            3,
        )
    };
}

/// Renders the document: crop, expanded canvas, annotations and backdrop.
pub fn render(doc: &Document, image: BitmapRef, effects: &mut EffectCache, opts: ExportOptions) -> Bitmap {
    let canvas = doc.canvas_rect();
    let base = doc.base_rect();
    let pad = if doc.backdrop.enabled {
        doc.backdrop.padding
    } else {
        0.0
    };
    let (w, h) = doc.export_size();
    let s = opts.scale.clamp(0.05, 8.0) as f64;
    let out_w = ((w as f64 * s).round() as u32).max(1);
    let out_h = ((h as f64 * s).round() as u32).max(1);
    let ctx = cg::Context::new(out_w, out_h).expect("bitmap context");
    let c = ctx.ptr();
    // SAFETY: all calls go to the valid context created above, with balanced
    // save/restore and transparency layers.
    unsafe {
        cg::CGContextTranslateCTM(c, 0.0, out_h as f64);
        cg::CGContextScaleCTM(c, s, -s);
        if doc.backdrop.enabled {
            draw_backdrop(&ctx, doc, w as f64, h as f64);
            let strength = doc.backdrop.shadow.clamp(0.0, 1.0) as f64;
            if strength > 0.0 {
                let shadow = Cf::wrap(cg::CGColorCreateSRGB(0.0, 0.0, 0.0, 0.25 + 0.35 * strength)).expect("CGColor");
                // The y offset is negative because the CTM is flipped: this
                // casts the shadow downward.
                cg::CGContextSetShadowWithColor(
                    c,
                    CGSize {
                        width: 0.0,
                        height: -(pad as f64) * 0.12 * strength,
                    },
                    pad as f64 * 0.5 * strength,
                    shadow.ptr(),
                );
            }
            cg::CGContextBeginTransparencyLayer(c, std::ptr::null());
            cg::CGContextBeginPath(c);
            add_rounded_rect(&ctx, rect(pad, pad, canvas.w, canvas.h), doc.backdrop.radius);
            cg::CGContextClip(c);
        }
        cg::CGContextTranslateCTM(c, (pad - canvas.x) as f64, (pad - canvas.y) as f64);
        if canvas != base {
            set_fill(&ctx, rgb(255, 255, 255));
            cg::CGContextFillRect(
                c,
                cg_rect(canvas.x as f64, canvas.y as f64, canvas.w as f64, canvas.h as f64),
            );
        }
        cg::CGContextSaveGState(c);
        cg::CGContextClipToRect(c, cg_rect(base.x as f64, base.y as f64, base.w as f64, base.h as f64));
        draw_bitmap(&ctx, image, doc.image_rect(), true);
        cg::CGContextRestoreGState(c);
        for prim in document_prims(doc) {
            draw_prim(&ctx, &prim, doc, image, effects, opts);
        }
        if doc.backdrop.enabled {
            cg::CGContextEndTransparencyLayer(c);
        }
    }
    ctx.into_bitmap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oshot_core::geometry::pt;
    use oshot_core::model::{Backdrop, Style, Tool};

    fn screen() -> Bitmap {
        Bitmap::filled(200, 100, rgb(255, 255, 255))
    }

    #[test]
    fn plain_export_matches_image() {
        let img = screen();
        let doc = Document::new(200, 100, 1.0);
        let out = render(&doc, img.view(), &mut EffectCache::default(), ExportOptions::default());
        assert_eq!((out.width, out.height), (200, 100));
        assert_eq!(out.pixel(100, 50), Some(rgb(255, 255, 255)));
    }

    #[test]
    fn crop_changes_output_size_and_origin() {
        let mut img = screen();
        img.set_pixel(50, 20, rgb(255, 0, 0));
        let mut doc = Document::new(200, 100, 1.0);
        doc.apply_crop(rect(50.0, 20.0, 30.0, 10.0));
        let out = render(&doc, img.view(), &mut EffectCache::default(), ExportOptions::default());
        assert_eq!((out.width, out.height), (30, 10));
        assert_eq!(out.pixel(0, 0), Some(rgb(255, 0, 0)));
    }

    #[test]
    fn rectangle_is_drawn_in_its_colour() {
        let img = screen();
        let mut doc = Document::new(200, 100, 1.0);
        let style = Style {
            color: rgb(0, 0, 255),
            width: 6.0,
            ..Style::for_tool(Tool::Rect, 1.0)
        };
        doc.add(
            Shape::Rect {
                rect: rect(20.0, 20.0, 100.0, 50.0),
            },
            style,
        );
        let out = render(&doc, img.view(), &mut EffectCache::default(), ExportOptions::default());
        assert_eq!(out.pixel(70, 20), Some(rgb(0, 0, 255)), "top edge");
        assert_eq!(out.pixel(70, 45), Some(rgb(255, 255, 255)), "hollow inside");
    }

    #[test]
    fn arrow_head_reaches_the_tip() {
        let img = screen();
        let mut doc = Document::new(200, 100, 1.0);
        let style = Style {
            color: rgb(255, 0, 0),
            ..Style::for_tool(Tool::Arrow, 1.0)
        };
        doc.add(
            Shape::Arrow {
                from: pt(10.0, 50.0),
                to: pt(180.0, 50.0),
                number: None,
            },
            style,
        );
        let out = render(&doc, img.view(), &mut EffectCache::default(), ExportOptions::default());
        assert_eq!(out.pixel(175, 50), Some(rgb(255, 0, 0)));
        assert_eq!(out.pixel(100, 50), Some(rgb(255, 0, 0)), "shaft");
        assert_eq!(out.pixel(100, 30), Some(rgb(255, 255, 255)));
    }

    #[test]
    fn text_bubble_is_drawn() {
        let img = screen();
        let mut doc = Document::new(200, 100, 1.0);
        let style = Style {
            color: rgb(0, 128, 0),
            text_background: true,
            ..Style::for_tool(Tool::Text, 1.0)
        };
        doc.add(
            Shape::Text {
                origin: pt(10.0, 10.0),
                text: "Hello".into(),
                extent: pt(0.0, 0.0),
                arrow: None,
            },
            style,
        );
        let out = render(&doc, img.view(), &mut EffectCache::default(), ExportOptions::default());
        assert_eq!(out.pixel(12, 12), Some(rgb(0, 128, 0)), "bubble corner area");
        let has_white_text = (14..60).any(|x| out.pixel(x, 25).is_some_and(|c| c.r > 200 && c.g > 200));
        assert!(has_white_text, "contrasting text inside bubble");
    }

    #[test]
    fn pixelate_changes_pixels() {
        let mut img = screen();
        for x in 0..200 {
            for y in 0..100 {
                if (x / 2 + y / 2) % 2 == 0 {
                    img.set_pixel(x, y, rgb(0, 0, 0));
                }
            }
        }
        let mut doc = Document::new(200, 100, 1.0);
        doc.add(
            Shape::Pixelate {
                rect: rect(0.0, 0.0, 40.0, 40.0),
            },
            Style::for_tool(Tool::Pixelate, 1.0),
        );
        let out = render(&doc, img.view(), &mut EffectCache::default(), ExportOptions::default());
        assert_eq!(out.pixel(1, 1), out.pixel(8, 8), "flattened block");
        assert_ne!(out.pixel(1, 1), Some(rgb(0, 0, 0)));
    }

    #[test]
    fn spotlight_dims_outside() {
        let img = screen();
        let mut doc = Document::new(200, 100, 1.0);
        doc.add(
            Shape::Spotlight {
                rect: rect(50.0, 25.0, 100.0, 50.0),
            },
            Style::for_tool(Tool::Spotlight, 1.0),
        );
        let out = render(&doc, img.view(), &mut EffectCache::default(), ExportOptions::default());
        assert_eq!(out.pixel(100, 50), Some(rgb(255, 255, 255)), "inside untouched");
        assert!(out.pixel(5, 5).unwrap().r < 140, "outside dimmed");
    }

    #[test]
    fn backdrop_adds_padding_and_gradient() {
        let img = screen();
        let mut doc = Document::new(200, 100, 1.0);
        doc.set_backdrop(Backdrop {
            enabled: true,
            preset: 9,
            padding: 30.0,
            radius: 10.0,
            shadow: 0.0,
        });
        let out = render(&doc, img.view(), &mut EffectCache::default(), ExportOptions::default());
        assert_eq!((out.width, out.height), (260, 160));
        assert_eq!(out.pixel(2, 2), Some(rgb(0x1C, 0x1C, 0x1E)), "solid backdrop");
        assert_eq!(out.pixel(130, 80), Some(rgb(255, 255, 255)), "screenshot in the middle");
        assert_eq!(out.pixel(31, 31).map(|c| c.r < 255), Some(true), "rounded corner clips");
    }

    #[test]
    fn half_scale_export() {
        let img = screen();
        let doc = Document::new(200, 100, 2.0);
        let out = render(
            &doc,
            img.view(),
            &mut EffectCache::default(),
            ExportOptions {
                scale: 0.5,
                ..ExportOptions::default()
            },
        );
        assert_eq!((out.width, out.height), (100, 50));
    }

    #[test]
    fn overlays_can_be_skipped() {
        let img = screen();
        let mut doc = Document::new(200, 100, 1.0);
        let overlay = std::sync::Arc::new(Bitmap::filled(10, 10, rgb(255, 0, 0)));
        doc.add(
            Shape::Image {
                rect: rect(0.0, 0.0, 10.0, 10.0),
                image: overlay,
                opacity: 1.0,
            },
            Style::for_tool(Tool::Select, 1.0),
        );
        let with = render(&doc, img.view(), &mut EffectCache::default(), ExportOptions::default());
        let without = render(
            &doc,
            img.view(),
            &mut EffectCache::default(),
            ExportOptions {
                skip_overlays: true,
                ..ExportOptions::default()
            },
        );
        assert_eq!(with.pixel(5, 5), Some(rgb(255, 0, 0)));
        assert_eq!(without.pixel(5, 5), Some(rgb(255, 255, 255)));
    }
}
