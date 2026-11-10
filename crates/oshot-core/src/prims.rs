//! Lowers annotation objects to a handful of drawing primitives.
//!
//! The editor (GPUI) and the exporter (CoreGraphics) both draw these, so a
//! saved image matches what was on screen.

use crate::color::{Color, rgba};
use crate::geometry::{Pt, Rect, circle_points, ellipse_points, pt, rect, rounded_rect_points};
use crate::model::{ArrowStyle, Document, Object, Shape, counter_radius};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    Regular,
    Bold,
    Hand,
}

impl Font {
    /// Family name both renderers resolve through CoreText.
    pub fn family(self) -> &'static str {
        match self {
            Font::Regular | Font::Bold => "Helvetica Neue",
            Font::Hand => "Chalkboard SE",
        }
    }

    pub fn bold(self) -> bool {
        matches!(self, Font::Bold)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bubble {
    pub color: Color,
    pub padding: f32,
    pub radius: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    /// Filled polygons under the non-zero rule; holes wind the other way.
    Fill { contours: Vec<Vec<Pt>>, color: Color },
    /// Polyline with round caps and joins.
    Stroke {
        points: Vec<Pt>,
        width: f32,
        color: Color,
        closed: bool,
    },
    /// One or more lines of text. `at` is the top-left corner, or the centre
    /// of the block for [`Anchor::Center`].
    Text {
        text: String,
        at: Pt,
        anchor: Anchor,
        size: f32,
        color: Color,
        font: Font,
        bubble: Option<Bubble>,
    },
    /// Pixels computed from the image by an effect (pixelate, blur, erase,
    /// magnifier), looked up by object id in an `EffectCache`.
    Effect { object: u64, dest: Rect, circle: bool },
    /// A pasted image overlay.
    Image { object: u64, dest: Rect, opacity: f32 },
}

pub const LINE_HEIGHT: f32 = 1.25;

/// Primitives for the whole document in paint order. Spotlights merge into a
/// single dimming layer so overlapping spotlights do not darken twice.
pub fn document_prims(doc: &Document) -> Vec<Prim> {
    let canvas = doc.canvas_rect().inflate(2.0);
    let mut out = Vec::new();
    let mut spot_done = false;
    for obj in &doc.objects {
        if let Shape::Spotlight { .. } = obj.shape {
            if !spot_done {
                spot_done = true;
                out.push(spotlight_layer(doc, canvas));
            }
            continue;
        }
        object_prims(obj, doc.scale, &mut out);
    }
    out
}

fn spotlight_layer(doc: &Document, canvas: Rect) -> Prim {
    let mut contours = vec![canvas.corners().to_vec()];
    let mut dim = 0.0f32;
    for o in &doc.objects {
        if let Shape::Spotlight { rect } = o.shape {
            dim = dim.max(o.style.strength);
            let mut hole = if o.style.ellipse {
                ellipse_points(&rect, 64)
            } else {
                rounded_rect_points(&rect, 6.0 * doc.scale.max(1.0))
            };
            // The outer contour is clockwise, so holes run counter-clockwise.
            hole.reverse();
            contours.push(hole);
        }
    }
    Prim::Fill {
        contours,
        color: rgba(0, 0, 0, (dim.clamp(0.0, 1.0) * 255.0) as u8),
    }
}

pub fn object_prims(obj: &Object, scale: f32, out: &mut Vec<Prim>) {
    let st = &obj.style;
    let w = st.width;
    let hand = st.hand_drawn;
    let seed = obj.id;
    match &obj.shape {
        Shape::Arrow { from, to, .. } => {
            arrow(*from, *to, st.arrow, w, st.color, hand, seed, out);
            if let Some((center, r, number)) = obj.badge() {
                step_badge(center, r, number, st.color, out);
            }
        }
        Shape::Line { from, to } => out.push(stroke(jitter(vec![*from, *to], w, hand, seed), w, st.color, false)),
        Shape::Rect { rect } => {
            let outline = jitter(rounded_rect_points(rect, w * 0.75), w, hand, seed);
            if st.fill {
                out.push(Prim::Fill {
                    contours: vec![outline.clone()],
                    color: st.color.with_alpha(80),
                });
            }
            out.push(stroke(outline, w, st.color, true));
        }
        Shape::Oval { rect } => {
            let outline = jitter(ellipse_points(rect, 72), w, hand, seed);
            if st.fill {
                out.push(Prim::Fill {
                    contours: vec![outline.clone()],
                    color: st.color.with_alpha(80),
                });
            }
            out.push(stroke(outline, w, st.color, true));
        }
        Shape::Freehand { points } => out.push(stroke(smooth(points), w, st.color, false)),
        Shape::Highlighter { from, to } => {
            out.push(Prim::Fill {
                contours: vec![band(*from, *to, w * 2.5)],
                color: st.color.with_alpha(110),
            });
        }
        Shape::Text {
            origin, text, arrow, ..
        } => {
            // Arrow labels are see-through, so what they sit on still shows.
            let fill = if arrow.is_some() {
                st.color.with_alpha(191)
            } else {
                st.color
            };
            let bubble = st.text_background.then_some(Bubble {
                color: fill,
                padding: st.text_size * 0.3,
                radius: st.text_size * 0.3,
            });
            let color = if st.text_background {
                st.color.contrasting()
            } else {
                st.color
            };
            out.push(Prim::Text {
                text: text.clone(),
                at: *origin,
                anchor: Anchor::TopLeft,
                size: st.text_size,
                color,
                font: if hand { Font::Hand } else { Font::Bold },
                bubble,
            });
        }
        Shape::Counter { center, number } => step_badge(*center, counter_radius(st), *number, st.color, out),
        Shape::Pixelate { rect } | Shape::Blur { rect } | Shape::Erase { rect } => {
            out.push(Prim::Effect {
                object: obj.id,
                dest: *rect,
                circle: false,
            });
        }
        Shape::Spotlight { .. } => {}
        Shape::Magnifier { source, center, radius } => {
            let thin = 1.5 * scale.max(1.0);
            let r = *radius;
            let toward = pt(center.x - source.center().x, center.y - source.center().y);
            let len = (toward.x * toward.x + toward.y * toward.y).sqrt().max(0.001);
            let edge = pt(center.x - toward.x / len * r, center.y - toward.y / len * r);
            out.push(stroke(vec![source.center(), edge], thin, st.color, false));
            out.push(stroke(source.corners().to_vec(), thin, st.color, true));
            let dest = rect(center.x - r, center.y - r, r * 2.0, r * 2.0);
            out.push(Prim::Effect {
                object: obj.id,
                dest,
                circle: true,
            });
            out.push(stroke(circle_points(*center, r, 96), w.max(thin * 2.0), st.color, true));
        }
        Shape::Ruler { from, to } => ruler(*from, *to, scale, st.color, w, out),
        Shape::Image { rect, opacity, .. } => {
            out.push(Prim::Image {
                object: obj.id,
                dest: *rect,
                opacity: *opacity,
            });
        }
    }
}

/// A filled circle with a step number, for counters and numbered arrows.
fn step_badge(center: Pt, r: f32, number: u32, color: Color, out: &mut Vec<Prim>) {
    out.push(Prim::Fill {
        contours: vec![circle_points(center, r, 48)],
        color,
    });
    out.push(Prim::Text {
        text: number.to_string(),
        at: center,
        anchor: Anchor::Center,
        size: r * 1.1,
        color: color.contrasting(),
        font: Font::Bold,
        bubble: None,
    });
}

fn stroke(points: Vec<Pt>, width: f32, color: Color, closed: bool) -> Prim {
    Prim::Stroke {
        points,
        width,
        color,
        closed,
    }
}

/// Rectangle of `half_width` either side of the segment.
fn band(a: Pt, b: Pt, half_width: f32) -> Vec<Pt> {
    let len = a.distance(b).max(0.001);
    let nx = -(b.y - a.y) / len * half_width;
    let ny = (b.x - a.x) / len * half_width;
    vec![
        pt(a.x + nx, a.y + ny),
        pt(b.x + nx, b.y + ny),
        pt(b.x - nx, b.y - ny),
        pt(a.x - nx, a.y - ny),
    ]
}

/// Head length for an arrow of stroke width `w`.
pub fn arrow_head_length(w: f32) -> f32 {
    w * 3.6 + 8.0
}

#[allow(clippy::too_many_arguments)]
fn arrow(from: Pt, to: Pt, style: ArrowStyle, w: f32, color: Color, hand: bool, seed: u64, out: &mut Vec<Prim>) {
    let curve = [from, to];
    let total = curve.windows(2).map(|s| s[0].distance(s[1])).sum::<f32>();
    let head = arrow_head_length(w).min(total * 0.6);
    let (shaft, head_base) = trim_end(&curve, head);
    match style {
        ArrowStyle::Tapered => {
            out.push(Prim::Fill {
                contours: vec![tapered(&shaft, w * 0.3, w)],
                color,
            });
            out.push(Prim::Fill {
                contours: vec![head_triangle(head_base, to, head * 0.62)],
                color,
            });
        }
        ArrowStyle::Line => {
            out.push(stroke(jitter(shaft, w, hand, seed), w, color, false));
            out.push(Prim::Fill {
                contours: vec![head_triangle(head_base, to, head * 0.55)],
                color,
            });
        }
        ArrowStyle::Double => {
            let reversed: Vec<Pt> = shaft.iter().rev().copied().collect();
            let (both, tail_base) = trim_end(&reversed, head.min(total * 0.3));
            out.push(stroke(jitter(both, w, hand, seed), w, color, false));
            out.push(Prim::Fill {
                contours: vec![head_triangle(head_base, to, head * 0.55)],
                color,
            });
            out.push(Prim::Fill {
                contours: vec![head_triangle(tail_base, from, head * 0.55)],
                color,
            });
        }
    }
}

/// Cuts `len` off the end of a polyline; returns the rest and the cut point.
fn trim_end(points: &[Pt], len: f32) -> (Vec<Pt>, Pt) {
    let mut remaining = len;
    for i in (1..points.len()).rev() {
        let seg = points[i].distance(points[i - 1]);
        if seg >= remaining {
            let cut = points[i].lerp(points[i - 1], remaining / seg.max(0.0001));
            let mut shaft = points[..i].to_vec();
            shaft.push(cut);
            return (shaft, cut);
        }
        remaining -= seg;
    }
    (vec![points[0], points[0]], points[0])
}

/// Outline of a polyline whose width grows from `w0` at the tail to `w1`.
fn tapered(points: &[Pt], w0: f32, w1: f32) -> Vec<Pt> {
    let n = points.len();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let a = points[i.saturating_sub(1)];
        let b = points[(i + 1).min(n - 1)];
        let len = a.distance(b).max(0.0001);
        let t = if n > 1 { i as f32 / (n - 1) as f32 } else { 1.0 };
        let half = (w0 + (w1 - w0) * t) / 2.0;
        let nx = -(b.y - a.y) / len * half;
        let ny = (b.x - a.x) / len * half;
        left.push(pt(points[i].x + nx, points[i].y + ny));
        right.push(pt(points[i].x - nx, points[i].y - ny));
    }
    right.reverse();
    left.extend(right);
    left
}

fn head_triangle(base: Pt, tip: Pt, half_width: f32) -> Vec<Pt> {
    let len = base.distance(tip).max(0.0001);
    let nx = -(tip.y - base.y) / len * half_width;
    let ny = (tip.x - base.x) / len * half_width;
    vec![tip, pt(base.x + nx, base.y + ny), pt(base.x - nx, base.y - ny)]
}

fn ruler(from: Pt, to: Pt, scale: f32, color: Color, w: f32, out: &mut Vec<Prim>) {
    let width = (w / 2.0).max(1.5);
    let len = from.distance(to).max(0.0001);
    let tick = 8.0 * scale.max(1.0);
    let nx = -(to.y - from.y) / len * tick;
    let ny = (to.x - from.x) / len * tick;
    out.push(stroke(vec![from, to], width, color, false));
    for p in [from, to] {
        out.push(stroke(
            vec![pt(p.x + nx, p.y + ny), pt(p.x - nx, p.y - ny)],
            width,
            color,
            false,
        ));
    }
    out.push(Prim::Text {
        text: ruler_label(from, to, scale),
        at: from.lerp(to, 0.5),
        anchor: Anchor::Center,
        size: 13.0 * scale.max(1.0),
        color: rgba(255, 255, 255, 255),
        font: Font::Bold,
        bubble: Some(Bubble {
            color: rgba(0, 0, 0, 200),
            padding: 5.0 * scale.max(1.0),
            radius: 5.0 * scale.max(1.0),
        }),
    });
}

/// Length in points (logical pixels), the unit designers measure in.
pub fn ruler_label(from: Pt, to: Pt, scale: f32) -> String {
    format!("{} px", (from.distance(to) / scale.max(1.0)).round() as i64)
}

/// One pass of Chaikin smoothing for freehand strokes.
fn smooth(points: &[Pt]) -> Vec<Pt> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * 2);
    out.push(points[0]);
    for w in points.windows(2) {
        out.push(w[0].lerp(w[1], 0.25));
        out.push(w[0].lerp(w[1], 0.75));
    }
    out.push(*points.last().unwrap());
    out
}

/// Hand-drawn style: subdivides and wobbles a path by a seeded, smooth offset
/// so the same object always looks the same.
fn jitter(points: Vec<Pt>, w: f32, hand: bool, seed: u64) -> Vec<Pt> {
    if !hand || points.len() < 2 {
        return points;
    }
    let amp = (w * 0.6).max(1.2);
    let mut out = Vec::new();
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % 2000) as f32 / 1000.0 - 1.0
    };
    let mut offset = 0.0f32;
    for seg in points.windows(2) {
        let steps = ((seg[0].distance(seg[1]) / (w * 6.0).max(6.0)).ceil() as usize).max(1);
        let len = seg[0].distance(seg[1]).max(0.0001);
        let nx = -(seg[1].y - seg[0].y) / len;
        let ny = (seg[1].x - seg[0].x) / len;
        for i in 0..steps {
            offset = (offset * 0.7 + next() * 0.3).clamp(-1.0, 1.0);
            let p = seg[0].lerp(seg[1], i as f32 / steps as f32);
            out.push(pt(p.x + nx * offset * amp, p.y + ny * offset * amp));
        }
    }
    out.push(*points.last().unwrap());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Style, Tool};

    fn obj(shape: Shape, tool: Tool) -> Object {
        Object {
            id: 7,
            shape,
            style: Style::for_tool(tool, 1.0),
        }
    }

    #[test]
    fn tapered_arrow_has_shaft_and_head_ending_at_tip() {
        let o = obj(
            Shape::Arrow {
                from: pt(0.0, 0.0),
                to: pt(200.0, 0.0),
                number: None,
            },
            Tool::Arrow,
        );
        let mut out = Vec::new();
        object_prims(&o, 1.0, &mut out);
        assert_eq!(out.len(), 2);
        let Prim::Fill { contours, .. } = &out[1] else { panic!() };
        assert_eq!(contours[0][0], pt(200.0, 0.0), "head tip is the arrow end");
    }

    #[test]
    fn double_arrow_has_two_heads() {
        let mut o = obj(
            Shape::Arrow {
                from: pt(0.0, 0.0),
                to: pt(200.0, 0.0),
                number: None,
            },
            Tool::Arrow,
        );
        o.style.arrow = ArrowStyle::Double;
        let mut out = Vec::new();
        object_prims(&o, 1.0, &mut out);
        assert_eq!(out.iter().filter(|p| matches!(p, Prim::Fill { .. })).count(), 2);
    }

    #[test]
    fn hand_drawn_is_deterministic_and_wobbly() {
        let mut o = obj(
            Shape::Line {
                from: pt(0.0, 0.0),
                to: pt(300.0, 0.0),
            },
            Tool::Line,
        );
        o.style.hand_drawn = true;
        let (mut a, mut b) = (Vec::new(), Vec::new());
        object_prims(&o, 1.0, &mut a);
        object_prims(&o, 1.0, &mut b);
        assert_eq!(a, b);
        let Prim::Stroke { points, .. } = &a[0] else { panic!() };
        assert!(points.len() > 2 && points.iter().any(|p| p.y.abs() > 0.1));
    }

    #[test]
    fn spotlights_merge_into_one_layer() {
        let mut d = Document::new(100, 100, 1.0);
        let s = Style::for_tool(Tool::Spotlight, 1.0);
        d.add(
            Shape::Spotlight {
                rect: rect(10.0, 10.0, 20.0, 20.0),
            },
            s.clone(),
        );
        d.add(
            Shape::Spotlight {
                rect: rect(50.0, 50.0, 20.0, 20.0),
            },
            s,
        );
        let prims = document_prims(&d);
        assert_eq!(prims.len(), 1);
        let Prim::Fill { contours, color } = &prims[0] else {
            panic!()
        };
        assert_eq!(contours.len(), 3, "canvas plus two holes");
        assert_eq!(color.a, 140);
    }

    #[test]
    fn ruler_label_uses_points() {
        assert_eq!(ruler_label(pt(0.0, 0.0), pt(200.0, 0.0), 2.0), "100 px");
    }

    #[test]
    fn text_bubble_uses_contrasting_text() {
        let mut o = obj(
            Shape::Text {
                origin: pt(0.0, 0.0),
                text: "Hi".into(),
                extent: pt(0.0, 0.0),
                arrow: None,
            },
            Tool::Text,
        );
        o.style.color = crate::color::rgb(255, 255, 255);
        o.style.text_background = true;
        let mut out = Vec::new();
        object_prims(&o, 1.0, &mut out);
        let Prim::Text { color, bubble, .. } = &out[0] else {
            panic!()
        };
        assert_eq!(*color, crate::color::rgb(0, 0, 0));
        assert!(bubble.is_some());
    }

    #[test]
    fn numbered_arrow_draws_its_badge_at_the_tail() {
        let o = obj(
            Shape::Arrow {
                from: pt(10.0, 20.0),
                to: pt(200.0, 20.0),
                number: Some(4),
            },
            Tool::Arrow,
        );
        let mut out = Vec::new();
        object_prims(&o, 1.0, &mut out);
        let Some(Prim::Text { text, at, .. }) = out.last() else {
            panic!("badge number last")
        };
        assert_eq!((text.as_str(), *at), ("4", pt(10.0, 20.0)));
    }

    #[test]
    fn arrow_label_bubble_is_translucent() {
        let mut o = obj(
            Shape::Text {
                origin: pt(0.0, 0.0),
                text: "Hi".into(),
                extent: pt(0.0, 0.0),
                arrow: Some(3),
            },
            Tool::Text,
        );
        o.style.text_background = true;
        let mut out = Vec::new();
        object_prims(&o, 1.0, &mut out);
        let Prim::Text { bubble: Some(b), .. } = &out[0] else {
            panic!()
        };
        assert_eq!(b.color.a, 191);
    }
}
