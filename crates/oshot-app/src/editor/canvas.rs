//! Editor canvas: view transform, mouse interaction and painting.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    App, Bounds, Corners, Font, FontWeight, Hsla, PathBuilder, PathStyle, Pixels, Point, RenderImage, SharedString,
    TextAlign, TextRun, Window, fill, font, linear_color_stop, linear_gradient, outline, point, px, size,
};
use lyon::tessellation::{FillOptions, LineCap, LineJoin, StrokeOptions};
use oshot_core::geometry::{Pt, Rect, pt, rect};
use oshot_core::model::BACKDROP_PRESETS;
use oshot_core::prims::{Anchor, Font as PrimFont, LINE_HEIGHT, Prim};

use crate::theme::{core_color, hex};

/// Maps image pixels to window points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct View {
    pub origin: Point<Pixels>,
    /// Window points per image pixel.
    pub zoom: f32,
}

impl View {
    pub fn to_screen(&self, p: Pt) -> Point<Pixels> {
        point(self.origin.x + px(p.x * self.zoom), self.origin.y + px(p.y * self.zoom))
    }

    pub fn to_image(&self, s: Point<Pixels>) -> Pt {
        pt(
            f32::from(s.x - self.origin.x) / self.zoom,
            f32::from(s.y - self.origin.y) / self.zoom,
        )
    }

    pub fn bounds(&self, r: Rect) -> Bounds<Pixels> {
        Bounds {
            origin: self.to_screen(pt(r.x, r.y)),
            size: size(px(r.w * self.zoom), px(r.h * self.zoom)),
        }
    }
}

/// Zoom that fits `content` in `bounds` with a margin, never above 100%.
pub fn fit_zoom(bounds: Bounds<Pixels>, content: Rect, scale: f32) -> f32 {
    let bw = (f32::from(bounds.size.width) - 48.0).max(40.0);
    let bh = (f32::from(bounds.size.height) - 48.0).max(40.0);
    (bw / content.w.max(1.0))
        .min(bh / content.h.max(1.0))
        .min(1.0 / scale.max(1.0))
}

pub fn compute_view(bounds: Bounds<Pixels>, content: Rect, zoom: Option<f32>, pan: Pt, scale: f32) -> View {
    let zoom = zoom.unwrap_or_else(|| fit_zoom(bounds, content, scale));
    let c = bounds.center();
    let cc = content.center();
    View {
        origin: point(c.x - px(cc.x * zoom) + px(pan.x), c.y - px(cc.y * zoom) + px(pan.y)),
        zoom,
    }
}

/// Everything the paint closure needs, gathered during render.
pub struct Scene {
    pub image: Arc<RenderImage>,
    pub image_rect: Rect,
    pub base: Rect,
    pub canvas: Rect,
    pub backdrop: Option<(Rect, (oshot_core::color::Color, oshot_core::color::Color), f32, f32)>,
    pub prims: Vec<Prim>,
    pub effects: HashMap<u64, (Rect, Arc<RenderImage>, bool)>,
    pub images: HashMap<u64, Arc<RenderImage>>,
    /// Effect objects being dragged: drawn as placeholders until released.
    pub placeholders: Vec<Rect>,
    pub selected: Option<(Rect, Vec<Pt>)>,
    pub selection: Option<Rect>,
    pub caret: Option<(Pt, f32)>,
    pub measure: Option<(Rect, Rect)>,
    pub accent: Hsla,
    pub canvas_bg: Hsla,
}

fn font_for(f: PrimFont) -> Font {
    let base = font(f.family());
    if f.bold() {
        Font {
            weight: FontWeight::BOLD,
            ..base
        }
    } else {
        base
    }
}

fn polygon_path(contours: &[Vec<Pt>], view: &View) -> Option<gpui::Path<Pixels>> {
    let mut b = PathBuilder::fill().with_style(PathStyle::Fill(FillOptions::non_zero()));
    for c in contours {
        let pts: Vec<Point<Pixels>> = c.iter().map(|p| view.to_screen(*p)).collect();
        if pts.len() >= 3 {
            b.add_polygon(&pts, true);
        }
    }
    b.build().ok()
}

fn stroke_path(points: &[Pt], width: f32, closed: bool, view: &View) -> Option<gpui::Path<Pixels>> {
    if points.len() < 2 {
        return None;
    }
    let opts = StrokeOptions::default()
        .with_line_width((width * view.zoom).max(0.75))
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round);
    let mut b = PathBuilder::stroke(px(width * view.zoom)).with_style(PathStyle::Stroke(opts));
    let pts: Vec<Point<Pixels>> = points.iter().map(|p| view.to_screen(*p)).collect();
    b.add_polygon(&pts, closed);
    b.build().ok()
}

/// Width of `text` in pixels at `size`, the same CoreText layout the
/// exporter uses.
pub fn text_extent(text: &str, size: f32, f: PrimFont, window: &Window) -> (f32, f32) {
    let ts = window.text_system();
    let mut w = 0f32;
    let lines: Vec<&str> = text.split('\n').collect();
    for line in &lines {
        if line.is_empty() {
            continue;
        }
        let run = TextRun {
            len: line.len(),
            font: font_for(f),
            color: hex(0, 1.0),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = ts.shape_line(SharedString::from(line.to_string()), px(size), &[run], None);
        w = w.max(f32::from(shaped.width));
    }
    (w.ceil(), (size * LINE_HEIGHT * lines.len() as f32).ceil())
}

fn paint_text(prim: &Prim, view: &View, window: &mut Window, cx: &mut App) {
    let Prim::Text {
        text,
        at,
        anchor,
        size,
        color,
        font: f,
        bubble,
    } = prim
    else {
        return;
    };
    let (at, anchor, size_px, color, f, bubble) = (*at, *anchor, *size, core_color(*color), *f, *bubble);
    let (w, h) = text_extent(text, size_px, f, window);
    let pad = bubble.map_or(0.0, |b| b.padding);
    let (bw, bh) = (w + pad * 2.0, h + pad * 2.0);
    let top_left = match anchor {
        Anchor::TopLeft => at,
        Anchor::Center => pt(at.x - bw / 2.0, at.y - bh / 2.0),
    };
    if let Some(b) = bubble {
        let mut q = fill(view.bounds(rect(top_left.x, top_left.y, bw, bh)), core_color(b.color));
        q.corner_radii = Corners::all(px(b.radius * view.zoom));
        window.paint_quad(q);
    }
    let size = size_px * view.zoom;
    let line_h = px(size * LINE_HEIGHT);
    for (i, line) in text.split('\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let run = TextRun {
            len: line.len(),
            font: font_for(f),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(SharedString::from(line.to_string()), px(size), &[run], None);
        let origin = view.to_screen(pt(
            top_left.x + pad,
            top_left.y + pad + i as f32 * size_px * LINE_HEIGHT,
        ));
        let _ = shaped.paint(origin, line_h, TextAlign::Left, None, window, cx);
    }
}

pub fn paint(scene: Scene, view: View, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    window.paint_quad(fill(bounds, scene.canvas_bg));
    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
        let radius = if let Some((outer, (a, b), radius, shadow)) = scene.backdrop {
            let bg = linear_gradient(
                135.0,
                linear_color_stop(core_color(a), 0.0),
                linear_color_stop(core_color(b), 1.0),
            );
            window.paint_quad(fill(view.bounds(outer), bg));
            if shadow > 0.0 {
                let cb = view.bounds(scene.canvas);
                window.paint_drop_shadows(
                    cb,
                    Corners::all(px(radius * view.zoom)),
                    &[gpui::BoxShadow {
                        inset: false,
                        color: hex(0, 0.25 + 0.35 * shadow),
                        offset: point(px(0.), px(outer.h * 0.012 * shadow * view.zoom)),
                        blur_radius: px((outer.x - scene.canvas.x).abs() * 0.5 * shadow * view.zoom),
                        spread_radius: px(0.),
                    }],
                );
            }
            radius * view.zoom
        } else {
            0.0
        };
        if scene.canvas != scene.base {
            let mut q = fill(view.bounds(scene.canvas), hex(0xFFFFFF, 1.0));
            q.corner_radii = Corners::all(px(radius));
            window.paint_quad(q);
        }
        let _ = window.paint_image(
            view.bounds(scene.base),
            view.bounds(scene.image_rect),
            Corners::all(px(if scene.canvas == scene.base { radius } else { 0.0 })),
            scene.image.clone(),
            0,
            false,
        );
        // Drawings stop at the canvas edge, as they do in the exported image.
        window.with_content_mask(
            Some(gpui::ContentMask {
                bounds: view.bounds(scene.canvas),
            }),
            |window| {
                for prim in &scene.prims {
                    match prim {
                        Prim::Fill { contours, color } => {
                            if let Some(p) = polygon_path(contours, &view) {
                                window.paint_path(p, core_color(*color));
                            }
                        }
                        Prim::Stroke {
                            points,
                            width,
                            color,
                            closed,
                        } => {
                            if let Some(p) = stroke_path(points, *width, *closed, &view) {
                                window.paint_path(p, core_color(*color));
                            }
                        }
                        Prim::Text { .. } => paint_text(prim, &view, window, cx),
                        Prim::Effect { object, .. } => {
                            if let Some((dest, image, circle)) = scene.effects.get(object) {
                                let b = view.bounds(*dest);
                                let r = if *circle { b.size.width / 2.0 } else { px(0.) };
                                let _ = window.paint_image(b, b, Corners::all(r), image.clone(), 0, false);
                            }
                        }
                        Prim::Image { object, dest, .. } => {
                            if let Some(image) = scene.images.get(object) {
                                let b = view.bounds(*dest);
                                let _ = window.paint_image(b, b, Corners::default(), image.clone(), 0, false);
                            }
                        }
                    }
                }
                for r in &scene.placeholders {
                    window.paint_quad(fill(view.bounds(*r), hex(0x808080, 0.35)));
                    window.paint_quad(outline(view.bounds(*r), hex(0xFFFFFF, 0.9), gpui::BorderStyle::Dashed));
                }
            },
        );
        if let Some((h, v)) = scene.measure {
            let a = scene.accent;
            let hb = view.bounds(rect(h.x, h.y, h.w, 0.0));
            window.paint_quad(fill(
                Bounds {
                    origin: hb.origin,
                    size: size(hb.size.width, px(1.)),
                },
                a,
            ));
            let vb = view.bounds(rect(v.x, v.y, 0.0, v.h));
            window.paint_quad(fill(
                Bounds {
                    origin: vb.origin,
                    size: size(px(1.), vb.size.height),
                },
                a,
            ));
        }
        if let Some(sel) = scene.selection {
            let b = view.bounds(sel);
            let outside = view.bounds(scene.canvas);
            let dim = hex(0, 0.35);
            for r in [
                Bounds::from_corners(outside.origin, point(outside.right(), b.top())),
                Bounds::from_corners(point(outside.left(), b.bottom()), outside.bottom_right()),
                Bounds::from_corners(point(outside.left(), b.top()), point(b.left(), b.bottom())),
                Bounds::from_corners(point(b.right(), b.top()), point(outside.right(), b.bottom())),
            ] {
                if r.size.width > px(0.) && r.size.height > px(0.) {
                    window.paint_quad(fill(r, dim));
                }
            }
            window.paint_quad(outline(b, hex(0xFFFFFF, 1.0), gpui::BorderStyle::Dashed));
        }
        // Selection shows only small square handles on the shape, no box.
        if let Some((_, handles)) = &scene.selected {
            for h in handles {
                let c = view.to_screen(*h);
                let mut q = fill(
                    Bounds {
                        origin: point(c.x - px(4.), c.y - px(4.)),
                        size: size(px(8.), px(8.)),
                    },
                    hex(0xFFFFFF, 1.0),
                );
                q.corner_radii = Corners::all(px(1.5));
                q.border_widths = gpui::Edges::all(px(1.));
                q.border_color = hex(0x000000, 0.55);
                window.paint_quad(q);
            }
        }
        if let Some((p, h)) = scene.caret {
            let c = view.to_screen(p);
            window.paint_quad(fill(
                Bounds {
                    origin: c,
                    size: size(px(2.), px(h * view.zoom)),
                },
                scene.accent,
            ));
        }
    });
}

/// Backdrop colours for a preset index.
pub fn backdrop_colors(preset: usize) -> (oshot_core::color::Color, oshot_core::color::Color) {
    BACKDROP_PRESETS[preset.min(BACKDROP_PRESETS.len() - 1)]
}
