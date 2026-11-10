//! The annotation document: objects on top of a captured image, with undo.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::bitmap::Bitmap;
use crate::color::{Color, PALETTE, rgb};
use crate::geometry::{Pt, Rect, distance_to_polyline, pt, rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    Select,
    Arrow,
    Line,
    Rect,
    Oval,
    Freehand,
    Highlighter,
    Text,
    Counter,
    Pixelate,
    Blur,
    Erase,
    Spotlight,
    Magnifier,
    Ruler,
    ColorPicker,
}

impl Tool {
    pub const ALL: [Tool; 16] = [
        Tool::Select,
        Tool::Arrow,
        Tool::Line,
        Tool::Rect,
        Tool::Oval,
        Tool::Freehand,
        Tool::Highlighter,
        Tool::Text,
        Tool::Counter,
        Tool::Pixelate,
        Tool::Blur,
        Tool::Erase,
        Tool::Spotlight,
        Tool::Magnifier,
        Tool::Ruler,
        Tool::ColorPicker,
    ];

    /// Single-key shortcut in the editor.
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "v",
            Tool::Arrow => "a",
            Tool::Line => "l",
            Tool::Rect => "r",
            Tool::Oval => "o",
            Tool::Freehand => "f",
            Tool::Highlighter => "h",
            Tool::Text => "t",
            Tool::Counter => "n",
            Tool::Pixelate => "p",
            Tool::Blur => "b",
            Tool::Erase => "e",
            Tool::Spotlight => "s",
            Tool::Magnifier => "m",
            Tool::Ruler => "u",
            Tool::ColorPicker => "i",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select & Crop",
            Tool::Arrow => "Arrow",
            Tool::Line => "Line",
            Tool::Rect => "Rectangle",
            Tool::Oval => "Oval",
            Tool::Freehand => "Freehand",
            Tool::Highlighter => "Highlighter",
            Tool::Text => "Text",
            Tool::Counter => "Step Counter",
            Tool::Pixelate => "Pixelate",
            Tool::Blur => "Blur",
            Tool::Erase => "Erase",
            Tool::Spotlight => "Spotlight",
            Tool::Magnifier => "Magnifier",
            Tool::Ruler => "Ruler",
            Tool::ColorPicker => "Color Picker",
        }
    }

    /// Whether objects made by this tool take the stroke colour.
    pub fn uses_color(self) -> bool {
        !matches!(
            self,
            Tool::Select | Tool::Pixelate | Tool::Blur | Tool::Erase | Tool::Spotlight | Tool::ColorPicker
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArrowStyle {
    Tapered,
    Line,
    Double,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub color: Color,
    /// Stroke width in image pixels.
    pub width: f32,
    pub fill: bool,
    pub hand_drawn: bool,
    pub arrow: ArrowStyle,
    pub text_size: f32,
    pub text_background: bool,
    /// New arrows get a step number at their tail.
    #[serde(default)]
    pub numbered: bool,
    /// Tool-specific strength: pixel block size, blur radius, spotlight dim
    /// (0..1) or magnifier zoom.
    pub strength: f32,
    /// Spotlight and magnifier shape.
    pub ellipse: bool,
}

impl Style {
    pub fn for_tool(tool: Tool, scale: f32) -> Style {
        let s = scale.max(1.0);
        let color = match tool {
            Tool::Highlighter => rgb(0xFF, 0xD6, 0x0A),
            Tool::Arrow | Tool::Text => rgb(0, 0, 0),
            _ => PALETTE[0],
        };
        let strength = match tool {
            Tool::Pixelate => 10.0 * s,
            Tool::Blur => 8.0 * s,
            Tool::Spotlight => 0.55,
            Tool::Magnifier => 3.0,
            _ => 0.0,
        };
        Style {
            color,
            width: 4.0 * s,
            fill: false,
            hand_drawn: false,
            arrow: ArrowStyle::Tapered,
            text_size: 22.0 * s,
            text_background: tool != Tool::Text,
            numbered: false,
            strength,
            ellipse: matches!(tool, Tool::Magnifier),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A numbered arrow shows `number` in a step-counter badge at its tail.
    Arrow {
        from: Pt,
        to: Pt,
        number: Option<u32>,
    },
    Line {
        from: Pt,
        to: Pt,
    },
    Rect {
        rect: Rect,
    },
    Oval {
        rect: Rect,
    },
    Freehand {
        points: Vec<Pt>,
    },
    Highlighter {
        from: Pt,
        to: Pt,
    },
    /// `extent` is the measured text size, written back by whoever lays the
    /// text out, so hit testing and selection match what is drawn. A label
    /// attached to an `arrow` sits at that arrow's tail and follows it.
    Text {
        origin: Pt,
        text: String,
        extent: Pt,
        arrow: Option<u64>,
    },
    Counter {
        center: Pt,
        number: u32,
    },
    Pixelate {
        rect: Rect,
    },
    Blur {
        rect: Rect,
    },
    Erase {
        rect: Rect,
    },
    Spotlight {
        rect: Rect,
    },
    /// Shows `source` enlarged in a circle of `radius` around `center`.
    Magnifier {
        source: Rect,
        center: Pt,
        radius: f32,
    },
    Ruler {
        from: Pt,
        to: Pt,
    },
    Image {
        rect: Rect,
        image: Arc<Bitmap>,
        opacity: f32,
    },
}

impl Shape {
    pub fn tool(&self) -> Tool {
        match self {
            Shape::Arrow { .. } => Tool::Arrow,
            Shape::Line { .. } => Tool::Line,
            Shape::Rect { .. } => Tool::Rect,
            Shape::Oval { .. } => Tool::Oval,
            Shape::Freehand { .. } => Tool::Freehand,
            Shape::Highlighter { .. } => Tool::Highlighter,
            Shape::Text { .. } => Tool::Text,
            Shape::Counter { .. } => Tool::Counter,
            Shape::Pixelate { .. } => Tool::Pixelate,
            Shape::Blur { .. } => Tool::Blur,
            Shape::Erase { .. } => Tool::Erase,
            Shape::Spotlight { .. } => Tool::Spotlight,
            Shape::Magnifier { .. } => Tool::Magnifier,
            Shape::Ruler { .. } => Tool::Ruler,
            Shape::Image { .. } => Tool::Select,
        }
    }

    /// The rectangle of box-like shapes.
    pub fn rect(&self) -> Option<Rect> {
        match self {
            Shape::Rect { rect }
            | Shape::Oval { rect }
            | Shape::Pixelate { rect }
            | Shape::Blur { rect }
            | Shape::Erase { rect }
            | Shape::Spotlight { rect }
            | Shape::Image { rect, .. } => Some(*rect),
            _ => None,
        }
    }

    fn rect_mut(&mut self) -> Option<&mut Rect> {
        match self {
            Shape::Rect { rect }
            | Shape::Oval { rect }
            | Shape::Pixelate { rect }
            | Shape::Blur { rect }
            | Shape::Erase { rect }
            | Shape::Spotlight { rect }
            | Shape::Image { rect, .. } => Some(rect),
            _ => None,
        }
    }

    /// Endpoints of line-like shapes.
    pub fn endpoints(&self) -> Option<(Pt, Pt)> {
        match self {
            Shape::Arrow { from, to, .. }
            | Shape::Line { from, to }
            | Shape::Highlighter { from, to }
            | Shape::Ruler { from, to } => Some((*from, *to)),
            _ => None,
        }
    }

    fn endpoints_mut(&mut self) -> Option<(&mut Pt, &mut Pt)> {
        match self {
            Shape::Arrow { from, to, .. }
            | Shape::Line { from, to }
            | Shape::Highlighter { from, to }
            | Shape::Ruler { from, to } => Some((from, to)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Start,
    End,
    /// 0 top-left, 1 top-right, 2 bottom-right, 3 bottom-left.
    Corner(u8),
    /// 0 top, 1 right, 2 bottom, 3 left.
    Edge(u8),
    MagnifierSource,
    MagnifierRadius,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub id: u64,
    pub shape: Shape,
    pub style: Style,
}

pub fn counter_radius(style: &Style) -> f32 {
    style.text_size * 0.72
}

impl Object {
    /// A numbered arrow's badge: centre (the tail), radius and number.
    pub fn badge(&self) -> Option<(Pt, f32, u32)> {
        match self.shape {
            Shape::Arrow {
                from, number: Some(n), ..
            } => Some((from, counter_radius(&self.style), n)),
            _ => None,
        }
    }

    /// Visual bounds including stroke.
    pub fn bounds(&self) -> Rect {
        let pad = self.style.width;
        match &self.shape {
            Shape::Arrow { from, to, .. } => {
                let line = Rect::from_points(*from, *to).inflate(pad * 3.0);
                match self.badge() {
                    Some((c, r, _)) => line.union(&rect(c.x - r, c.y - r, r * 2.0, r * 2.0)),
                    None => line,
                }
            }
            Shape::Line { from, to } | Shape::Ruler { from, to } => Rect::from_points(*from, *to).inflate(pad + 12.0),
            Shape::Highlighter { from, to } => Rect::from_points(*from, *to).inflate(pad * 3.0),
            Shape::Freehand { points } => bounds_of(points).inflate(pad),
            Shape::Text { origin, extent, .. } => rect(origin.x, origin.y, extent.x, extent.y),
            Shape::Counter { center, .. } => {
                let r = counter_radius(&self.style);
                rect(center.x - r, center.y - r, r * 2.0, r * 2.0)
            }
            Shape::Magnifier { source, center, radius } => source
                .union(&rect(center.x - radius, center.y - radius, radius * 2.0, radius * 2.0))
                .inflate(4.0),
            Shape::Rect { rect } | Shape::Oval { rect } => rect.inflate(pad / 2.0),
            other => other.rect().unwrap_or_default(),
        }
    }

    pub fn hit(&self, p: Pt, tolerance: f32) -> bool {
        let tol = tolerance + self.style.width / 2.0;
        match &self.shape {
            Shape::Arrow { from, to, .. } => {
                distance_to_polyline(p, &[*from, *to]) <= tol * 1.5
                    || self.badge().is_some_and(|(c, r, _)| p.distance(c) <= r)
            }
            Shape::Line { from, to } | Shape::Ruler { from, to } => {
                distance_to_polyline(p, &[*from, *to]) <= tol.max(6.0)
            }
            Shape::Highlighter { from, to } => {
                distance_to_polyline(p, &[*from, *to]) <= self.style.width * 2.5 + tolerance
            }
            Shape::Freehand { points } => distance_to_polyline(p, points) <= tol,
            Shape::Rect { rect } if !self.style.fill => {
                let edges = rect.corners();
                let closed = [edges[0], edges[1], edges[2], edges[3], edges[0]];
                distance_to_polyline(p, &closed) <= tol
            }
            Shape::Oval { rect } if !self.style.fill => {
                let c = rect.center();
                let rx = (rect.w / 2.0).max(0.5);
                let ry = (rect.h / 2.0).max(0.5);
                let d = (((p.x - c.x) / rx).powi(2) + ((p.y - c.y) / ry).powi(2)).sqrt();
                (d - 1.0).abs() * rx.min(ry) <= tol
            }
            Shape::Counter { center, .. } => p.distance(*center) <= counter_radius(&self.style),
            Shape::Magnifier { source, center, radius } => p.distance(*center) <= *radius || source.contains(p),
            Shape::Spotlight { rect } => {
                let edges = rect.corners();
                let closed = [edges[0], edges[1], edges[2], edges[3], edges[0]];
                rect.contains(p) || distance_to_polyline(p, &closed) <= tol
            }
            _ => self.bounds().inflate(tolerance).contains(p),
        }
    }

    pub fn handles(&self) -> Vec<(Handle, Pt)> {
        match &self.shape {
            Shape::Magnifier { source, center, radius } => vec![
                (Handle::MagnifierSource, source.center()),
                (Handle::MagnifierRadius, pt(center.x + radius, center.y)),
            ],
            shape => {
                if let Some((a, b)) = shape.endpoints() {
                    vec![(Handle::Start, a), (Handle::End, b)]
                } else if let Some(r) = shape.rect() {
                    let c = r.corners();
                    let m = r.center();
                    vec![
                        (Handle::Corner(0), c[0]),
                        (Handle::Corner(1), c[1]),
                        (Handle::Corner(2), c[2]),
                        (Handle::Corner(3), c[3]),
                        (Handle::Edge(0), pt(m.x, r.y)),
                        (Handle::Edge(1), pt(r.right(), m.y)),
                        (Handle::Edge(2), pt(m.x, r.bottom())),
                        (Handle::Edge(3), pt(r.x, m.y)),
                    ]
                } else {
                    Vec::new()
                }
            }
        }
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        match &mut self.shape {
            Shape::Freehand { points } => points.iter_mut().for_each(|p| *p = p.offset(dx, dy)),
            Shape::Text { origin, .. } => *origin = origin.offset(dx, dy),
            Shape::Counter { center, .. } => *center = center.offset(dx, dy),
            Shape::Magnifier { source, center, .. } => {
                *source = source.translate(dx, dy);
                *center = center.offset(dx, dy);
            }
            shape => {
                if let Some((a, b)) = shape.endpoints_mut() {
                    *a = a.offset(dx, dy);
                    *b = b.offset(dx, dy);
                } else if let Some(r) = shape.rect_mut() {
                    *r = r.translate(dx, dy);
                }
            }
        }
    }

    /// Moves `handle` to `p`. `constrain` keeps rectangles square and snaps
    /// lines to 45 degrees, like holding Shift.
    pub fn drag_handle(&mut self, handle: Handle, p: Pt, constrain: bool) {
        match (&mut self.shape, handle) {
            (Shape::Magnifier { source, .. }, Handle::MagnifierSource) => {
                *source = rect(p.x - source.w / 2.0, p.y - source.h / 2.0, source.w, source.h);
            }
            (Shape::Magnifier { source, center, radius }, Handle::MagnifierRadius) => {
                let zoom = (*radius * 2.0 / source.w.max(1.0)).max(1.0);
                *radius = (p.x - center.x).abs().max(12.0);
                let side = *radius * 2.0 / zoom;
                let c = source.center();
                *source = rect(c.x - side / 2.0, c.y - side / 2.0, side, side);
            }
            (shape, Handle::Start | Handle::End) => {
                if let Some((a, b)) = shape.endpoints_mut() {
                    let (moving, fixed) = if handle == Handle::Start { (a, *b) } else { (b, *a) };
                    *moving = if constrain { snap_45(fixed, p) } else { p };
                }
            }
            (shape, Handle::Corner(i)) => {
                if let Some(r) = shape.rect_mut() {
                    let opposite = r.corners()[(i as usize + 2) % 4];
                    let p = if constrain { square_from(opposite, p) } else { p };
                    *r = Rect::from_points(opposite, p);
                }
            }
            (shape, Handle::Edge(i)) => {
                if let Some(r) = shape.rect_mut() {
                    let (mut x0, mut y0, mut x1, mut y1) = (r.x, r.y, r.right(), r.bottom());
                    match i {
                        0 => y0 = p.y,
                        1 => x1 = p.x,
                        2 => y1 = p.y,
                        _ => x0 = p.x,
                    }
                    *r = Rect::from_points(pt(x0, y0), pt(x1, y1));
                }
            }
            _ => {}
        }
    }
}

pub fn bounds_of(points: &[Pt]) -> Rect {
    let mut it = points.iter();
    let Some(first) = it.next() else { return Rect::default() };
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
    for p in it {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    rect(x0, y0, x1 - x0, y1 - y0)
}

/// Snaps `p` so the segment from `origin` is a multiple of 45 degrees.
pub fn snap_45(origin: Pt, p: Pt) -> Pt {
    let dx = p.x - origin.x;
    let dy = p.y - origin.y;
    let len = (dx * dx + dy * dy).sqrt();
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    pt(origin.x + len * angle.cos(), origin.y + len * angle.sin())
}

/// Moves `p` so the rectangle from `origin` is square.
pub fn square_from(origin: Pt, p: Pt) -> Pt {
    let side = (p.x - origin.x).abs().max((p.y - origin.y).abs());
    pt(
        origin.x + side * (p.x - origin.x).signum(),
        origin.y + side * (p.y - origin.y).signum(),
    )
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Backdrop {
    pub enabled: bool,
    pub preset: usize,
    /// Padding around the screenshot, in image pixels.
    pub padding: f32,
    pub radius: f32,
    /// Shadow strength, 0..1.
    pub shadow: f32,
}

impl Default for Backdrop {
    fn default() -> Self {
        Backdrop {
            enabled: false,
            preset: 0,
            padding: 64.0,
            radius: 12.0,
            shadow: 0.5,
        }
    }
}

/// Backdrop gradients, top-left colour to bottom-right colour. Equal colours
/// are solid fills.
pub const BACKDROP_PRESETS: [(Color, Color); 10] = [
    (rgb(0x8E, 0xC5, 0xFC), rgb(0xE0, 0xC3, 0xFC)),
    (rgb(0xFF, 0x9A, 0x8B), rgb(0xFF, 0x6A, 0x88)),
    (rgb(0x85, 0xFF, 0xBD), rgb(0xFF, 0xFB, 0x7D)),
    (rgb(0x21, 0xD4, 0xFD), rgb(0xB7, 0x21, 0xFF)),
    (rgb(0xFA, 0x8B, 0xFF), rgb(0x2B, 0xD2, 0xFF)),
    (rgb(0xFE, 0xE1, 0x40), rgb(0xFA, 0x70, 0x9A)),
    (rgb(0x0F, 0x20, 0x27), rgb(0x2C, 0x53, 0x64)),
    (rgb(0xE9, 0xE9, 0xEE), rgb(0xC9, 0xCB, 0xD3)),
    (rgb(0xFF, 0xFF, 0xFF), rgb(0xFF, 0xFF, 0xFF)),
    (rgb(0x1C, 0x1C, 0x1E), rgb(0x1C, 0x1C, 0x1E)),
];

#[derive(Clone)]
struct Snapshot {
    objects: Vec<Object>,
    crop: Option<Rect>,
    backdrop: Backdrop,
}

/// Objects drawn over a captured image. The image pixels live with the caller
/// (GPUI already owns them for display), so the document only stores sizes.
pub struct Document {
    pub image_width: u32,
    pub image_height: u32,
    /// Image pixels per point: 2 for Retina captures.
    pub scale: f32,
    pub objects: Vec<Object>,
    pub crop: Option<Rect>,
    pub backdrop: Backdrop,
    /// Grow the canvas to include drawings outside the image. Off, drawings
    /// are clipped at the image edge.
    pub expand_canvas: bool,
    next_id: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Document {
    pub fn new(image_width: u32, image_height: u32, scale: f32) -> Document {
        Document {
            image_width,
            image_height,
            scale,
            objects: Vec::new(),
            crop: None,
            backdrop: Backdrop::default(),
            expand_canvas: false,
            next_id: 1,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            objects: self.objects.clone(),
            crop: self.crop,
            backdrop: self.backdrop.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.objects = s.objects;
        self.crop = s.crop;
        self.backdrop = s.backdrop;
    }

    /// Records the current state so the next change can be undone. Call
    /// once before a change (or at the start of a drag).
    pub fn checkpoint(&mut self) {
        self.undo.push(self.snapshot());
        self.redo.clear();
        // 200 steps keeps memory bounded: image overlays are shared Arcs, so
        // a snapshot is only the object list.
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(self.snapshot());
        self.restore(prev);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(self.snapshot());
        self.restore(next);
        true
    }

    pub fn add(&mut self, shape: Shape, style: Style) -> u64 {
        self.checkpoint();
        self.insert(shape, style)
    }

    /// Removes an object added by the last [`Document::add`] and forgets that
    /// undo step, for shapes that were clicked but never dragged out.
    pub fn cancel_insert(&mut self, id: u64) {
        if let Some(i) = self.objects.iter().position(|o| o.id == id) {
            self.objects.remove(i);
            self.undo.pop();
        }
    }

    /// The drawable state without undo history, for exporting off the main
    /// thread.
    pub fn export_copy(&self) -> Document {
        Document {
            image_width: self.image_width,
            image_height: self.image_height,
            scale: self.scale,
            objects: self.objects.clone(),
            crop: self.crop,
            backdrop: self.backdrop.clone(),
            expand_canvas: self.expand_canvas,
            next_id: self.next_id,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Adds without a checkpoint, for callers that already recorded one.
    pub fn insert(&mut self, shape: Shape, style: Style) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.objects.push(Object { id, shape, style });
        id
    }

    pub fn get(&self, id: u64) -> Option<&Object> {
        self.objects.iter().find(|o| o.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.id == id)
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let Some(i) = self.objects.iter().position(|o| o.id == id) else {
            return false;
        };
        self.checkpoint();
        self.objects.remove(i);
        true
    }

    pub fn duplicate(&mut self, id: u64, offset: f32) -> Option<u64> {
        let mut copy = self.get(id)?.clone();
        copy.translate(offset, offset);
        match &mut copy.shape {
            Shape::Counter { number, .. } => *number = self.next_counter_number(),
            Shape::Arrow { number: Some(n), .. } => *n = self.next_counter_number(),
            // A copy stays where it was put instead of snapping onto the arrow.
            Shape::Text { arrow, .. } => *arrow = None,
            _ => {}
        }
        Some(self.add(copy.shape, copy.style))
    }

    /// Moves an object to the top of the stack without an undo step.
    pub fn bring_to_front(&mut self, id: u64) {
        if let Some(i) = self.objects.iter().position(|o| o.id == id) {
            let o = self.objects.remove(i);
            self.objects.push(o);
        }
    }

    /// Topmost object under `p`.
    pub fn hit_test(&self, p: Pt, tolerance: f32) -> Option<u64> {
        self.objects.iter().rev().find(|o| o.hit(p, tolerance)).map(|o| o.id)
    }

    pub fn next_counter_number(&self) -> u32 {
        self.objects
            .iter()
            .filter_map(|o| match o.shape {
                Shape::Counter { number, .. }
                | Shape::Arrow {
                    number: Some(number), ..
                } => Some(number),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            + 1
    }

    pub fn image_rect(&self) -> Rect {
        rect(0.0, 0.0, self.image_width as f32, self.image_height as f32)
    }

    /// The visible part of the image: the crop, or the whole image.
    pub fn base_rect(&self) -> Rect {
        self.crop.unwrap_or_else(|| self.image_rect())
    }

    /// What gets exported: the base rect, grown to include drawings when
    /// `expand_canvas` is on.
    pub fn canvas_rect(&self) -> Rect {
        let base = self.base_rect();
        if !self.expand_canvas {
            return base;
        }
        let grown = self
            .objects
            .iter()
            .filter(|o| !matches!(o.shape, Shape::Spotlight { .. }))
            .fold(base, |acc, o| acc.union(&o.bounds()));
        // Whole pixels, so exports never get a half-pixel edge.
        let x = grown.x.floor();
        let y = grown.y.floor();
        rect(
            x,
            y,
            (grown.right().ceil() - x).max(1.0),
            (grown.bottom().ceil() - y).max(1.0),
        )
    }

    pub fn apply_crop(&mut self, area: Rect) {
        let area = area.intersect(&self.image_rect());
        if area.is_empty() {
            return;
        }
        self.checkpoint();
        let r = area.to_pixels(self.image_width, self.image_height).to_rect();
        self.crop = Some(r);
    }

    pub fn reset_crop(&mut self) {
        if self.crop.is_some() {
            self.checkpoint();
            self.crop = None;
        }
    }

    pub fn set_backdrop(&mut self, backdrop: Backdrop) {
        if backdrop != self.backdrop {
            self.checkpoint();
            self.backdrop = backdrop;
        }
    }

    /// Records the measured size of a text object (not an undo step).
    pub fn set_text_extent(&mut self, id: u64, extent: Pt) {
        if let Some(Object {
            shape: Shape::Text { extent: e, .. },
            ..
        }) = self.get_mut(id)
        {
            *e = extent;
        }
    }

    /// Moves every arrow label so its box sits just beyond its arrow's tail,
    /// on the line the arrow points along. Run after text extents change.
    pub fn place_arrow_labels(&mut self) {
        let base = self.base_rect();
        let tails: Vec<(usize, Pt, Pt, f32)> = self
            .objects
            .iter()
            .enumerate()
            .filter_map(|(i, o)| match o.shape {
                Shape::Text { arrow: Some(a), .. } => {
                    let arrow = self.get(a)?;
                    let (from, to) = arrow.shape.endpoints()?;
                    // A numbered arrow's label starts past its badge.
                    let skip = arrow.badge().map_or(0.0, |(_, r, _)| r * 1.2);
                    Some((i, from, to, skip))
                }
                _ => None,
            })
            .collect();
        for (i, tail, to, skip) in tails {
            if let Shape::Text { origin, extent, .. } = &mut self.objects[i].shape {
                let len = tail.distance(to).max(0.0001);
                let (dx, dy) = ((tail.x - to.x) / len, (tail.y - to.y) / len);
                let from = pt(tail.x + dx * skip, tail.y + dy * skip);
                // How far the box centre must sit from the tail so the tail
                // just touches the box edge along the arrow's direction.
                let reach_x = if dx.abs() > 0.0001 {
                    extent.x / 2.0 / dx.abs()
                } else {
                    f32::MAX
                };
                let reach_y = if dy.abs() > 0.0001 {
                    extent.y / 2.0 / dy.abs()
                } else {
                    f32::MAX
                };
                let reach = reach_x.min(reach_y);
                let c = pt(from.x + dx * reach, from.y + dy * reach);
                *origin = pt(c.x - extent.x / 2.0, c.y - extent.y / 2.0);
                if !self.expand_canvas {
                    origin.x = origin.x.clamp(base.x, (base.right() - extent.x).max(base.x));
                    origin.y = origin.y.clamp(base.y, (base.bottom() - extent.y).max(base.y));
                }
            }
        }
    }

    /// Size of the exported image, before any resize.
    pub fn export_size(&self) -> (u32, u32) {
        let c = self.canvas_rect();
        let pad = if self.backdrop.enabled {
            self.backdrop.padding * 2.0
        } else {
            0.0
        };
        ((c.w + pad).round() as u32, (c.h + pad).round() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Document {
        Document::new(400, 300, 2.0)
    }

    #[test]
    fn add_undo_redo() {
        let mut d = doc();
        let id = d.add(
            Shape::Line {
                from: pt(0.0, 0.0),
                to: pt(10.0, 10.0),
            },
            Style::for_tool(Tool::Line, 1.0),
        );
        assert_eq!(d.objects.len(), 1);
        assert!(d.undo());
        assert!(d.objects.is_empty());
        assert!(d.redo());
        assert_eq!(d.objects[0].id, id);
        assert!(!d.redo());
    }

    #[test]
    fn hit_test_prefers_topmost() {
        let mut d = doc();
        let style = Style {
            fill: true,
            ..Style::for_tool(Tool::Rect, 1.0)
        };
        let a = d.add(
            Shape::Rect {
                rect: rect(0.0, 0.0, 100.0, 100.0),
            },
            style.clone(),
        );
        let b = d.add(
            Shape::Rect {
                rect: rect(50.0, 50.0, 100.0, 100.0),
            },
            style,
        );
        assert_eq!(d.hit_test(pt(75.0, 75.0), 2.0), Some(b));
        assert_eq!(d.hit_test(pt(10.0, 10.0), 2.0), Some(a));
        assert_eq!(d.hit_test(pt(300.0, 10.0), 2.0), None);
    }

    #[test]
    fn outline_rect_is_hit_on_its_edge_only() {
        let mut d = doc();
        let id = d.add(
            Shape::Rect {
                rect: rect(0.0, 0.0, 100.0, 100.0),
            },
            Style::for_tool(Tool::Rect, 1.0),
        );
        assert_eq!(d.hit_test(pt(100.0, 50.0), 2.0), Some(id));
        assert_eq!(d.hit_test(pt(50.0, 50.0), 2.0), None);
    }

    #[test]
    fn corner_drag_resizes_and_constrains_square() {
        let mut o = Object {
            id: 1,
            shape: Shape::Rect {
                rect: rect(10.0, 10.0, 20.0, 20.0),
            },
            style: Style::for_tool(Tool::Rect, 1.0),
        };
        o.drag_handle(Handle::Corner(2), pt(60.0, 40.0), false);
        assert_eq!(o.shape.rect(), Some(rect(10.0, 10.0, 50.0, 30.0)));
        o.drag_handle(Handle::Corner(2), pt(60.0, 40.0), true);
        assert_eq!(o.shape.rect(), Some(rect(10.0, 10.0, 50.0, 50.0)));
    }

    #[test]
    fn line_snaps_to_45_degrees() {
        let p = snap_45(pt(0.0, 0.0), pt(100.0, 8.0));
        assert!(p.y.abs() < 0.01 && (p.x - 100.3).abs() < 0.5);
    }

    #[test]
    fn counters_number_up() {
        let mut d = doc();
        let s = Style::for_tool(Tool::Counter, 1.0);
        assert_eq!(d.next_counter_number(), 1);
        let id = d.add(
            Shape::Counter {
                center: pt(5.0, 5.0),
                number: 1,
            },
            s,
        );
        assert_eq!(d.next_counter_number(), 2);
        let dup = d.duplicate(id, 10.0).unwrap();
        assert!(matches!(d.get(dup).unwrap().shape, Shape::Counter { number: 2, .. }));
    }

    #[test]
    fn crop_and_canvas() {
        let mut d = doc();
        d.apply_crop(rect(10.5, 10.5, 100.0, 50.0));
        assert_eq!(d.base_rect(), rect(10.0, 10.0, 101.0, 51.0));
        d.add(
            Shape::Counter {
                center: pt(5.0, 5.0),
                number: 1,
            },
            Style::for_tool(Tool::Counter, 1.0),
        );
        assert_eq!(
            d.canvas_rect(),
            rect(10.0, 10.0, 101.0, 51.0),
            "drawings outside are clipped by default"
        );
        d.expand_canvas = true;
        let c = d.canvas_rect();
        assert!(c.x < 10.0 && c.y < 10.0, "canvas grows to the counter: {c:?}");
        d.reset_crop();
        assert_eq!(d.base_rect(), d.image_rect());
        assert!(d.undo());
        assert!(d.crop.is_some(), "reset crop is undoable");
    }

    #[test]
    fn cancel_insert_leaves_no_undo_step() {
        let mut d = doc();
        d.add(
            Shape::Line {
                from: pt(0.0, 0.0),
                to: pt(9.0, 9.0),
            },
            Style::for_tool(Tool::Line, 1.0),
        );
        let id = d.add(
            Shape::Line {
                from: pt(1.0, 1.0),
                to: pt(1.0, 1.0),
            },
            Style::for_tool(Tool::Line, 1.0),
        );
        d.cancel_insert(id);
        assert_eq!(d.objects.len(), 1);
        assert!(d.undo());
        assert!(d.objects.is_empty());
        assert!(!d.undo());
        let copy = d.export_copy();
        assert!(!copy.can_undo() && copy.objects.is_empty());
    }

    #[test]
    fn arrow_label_sits_beyond_the_tail_and_follows_it() {
        let mut d = doc();
        let arrow = d.add(
            Shape::Arrow {
                from: pt(100.0, 100.0),
                to: pt(200.0, 100.0),
                number: None,
            },
            Style::for_tool(Tool::Arrow, 1.0),
        );
        let label = d.add(
            Shape::Text {
                origin: pt(0.0, 0.0),
                text: "Hi".into(),
                extent: pt(40.0, 20.0),
                arrow: Some(arrow),
            },
            Style::for_tool(Tool::Text, 1.0),
        );
        d.place_arrow_labels();
        let b = d.get(label).unwrap().bounds();
        assert_eq!(
            b,
            rect(60.0, 90.0, 40.0, 20.0),
            "right edge on the tail, centred on the line"
        );

        // Pointing straight down, the label sits above the tail.
        if let Some(Object {
            shape: Shape::Arrow { to, .. },
            ..
        }) = d.get_mut(arrow)
        {
            *to = pt(100.0, 300.0);
        }
        d.place_arrow_labels();
        assert_eq!(d.get(label).unwrap().bounds(), rect(80.0, 80.0, 40.0, 20.0));

        let copy = d.duplicate(label, 10.0).unwrap();
        assert!(matches!(d.get(copy).unwrap().shape, Shape::Text { arrow: None, .. }));
    }

    #[test]
    fn numbered_arrows_share_the_step_sequence_and_push_labels_past_the_badge() {
        let mut d = doc();
        let s = Style::for_tool(Tool::Counter, 1.0);
        d.add(
            Shape::Counter {
                center: pt(5.0, 5.0),
                number: 1,
            },
            s.clone(),
        );
        assert_eq!(d.next_counter_number(), 2);
        let arrow = d.add(
            Shape::Arrow {
                from: pt(100.0, 100.0),
                to: pt(200.0, 100.0),
                number: Some(2),
            },
            s,
        );
        assert_eq!(d.next_counter_number(), 3, "arrows and counters count together");
        let o = d.get(arrow).unwrap();
        let (c, r, n) = o.badge().unwrap();
        assert_eq!((c, n), (pt(100.0, 100.0), 2));
        assert!(o.hit(pt(100.0 - r * 0.8, 100.0), 0.0), "the badge is part of the arrow");
        assert!(o.bounds().x <= 100.0 - r);

        let label = d.add(
            Shape::Text {
                origin: pt(0.0, 0.0),
                text: "Hi".into(),
                extent: pt(40.0, 20.0),
                arrow: Some(arrow),
            },
            Style::for_tool(Tool::Text, 1.0),
        );
        d.place_arrow_labels();
        let b = d.get(label).unwrap().bounds();
        assert!(
            (b.right() - (100.0 - r * 1.2)).abs() < 0.01,
            "clear of the badge: {b:?}"
        );

        let copy = d.duplicate(arrow, 10.0).unwrap();
        assert!(matches!(
            d.get(copy).unwrap().shape,
            Shape::Arrow { number: Some(3), .. }
        ));
    }

    #[test]
    fn export_size_includes_backdrop() {
        let mut d = Document::new(100, 50, 1.0);
        d.set_backdrop(Backdrop {
            enabled: true,
            padding: 20.0,
            ..Backdrop::default()
        });
        assert_eq!(d.export_size(), (140, 90));
    }
}
