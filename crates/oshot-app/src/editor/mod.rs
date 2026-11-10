//! The annotation editor window.

mod canvas;
mod chrome;
mod export;

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, AppContext, Bounds, ClipboardEntry, Context, FocusHandle, Focusable, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, RenderImage, ScrollEvent, Window, px,
};
use oshot_core::bitmap::Bitmap;
use oshot_core::color::Color;
use oshot_core::effects::EffectCache;
use oshot_core::geometry::{Pt, Rect, pt, rect};
use oshot_core::measure::{Axis, measure_span, monotone_region, trim_to_content};
use oshot_core::model::{Document, Handle, Shape, Style, Tool, counter_radius, snap_45, square_from};
use oshot_core::prims::{Font as PrimFont, Prim, document_prims};

pub use canvas::View;
pub use export::Target;

use crate::state::{Notice, Shot, notify};
use crate::windows::{self, Spec};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Create { id: u64, start: Pt },
    Freehand { id: u64 },
    Move { id: u64, last: Pt },
    Handle { id: u64, handle: Handle },
    Select { start: Pt },
    MoveSelection { start: Pt, orig: Rect },
    Pan { last: Point },
}

type Point = gpui::Point<Pixels>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Backdrop,
    Export,
    Resize,
}

pub struct Editor {
    pub shot: Shot,
    pub doc: Document,
    pub tool: Tool,
    pub styles: HashMap<Tool, Style>,
    pub selected: Option<u64>,
    /// Select-tool rectangle in image pixels.
    pub selection: Option<Rect>,
    /// `None` fits the image to the window.
    pub zoom: Option<f32>,
    pub pan: Pt,
    pub editing_text: Option<u64>,
    pub panel: Option<Panel>,
    /// Output scale for copy/save/export (1.0 = full size).
    pub export_scale: f32,
    pub hover: Option<Pt>,
    pub source: Option<PathBuf>,
    drag: Option<Drag>,
    effects: EffectCache,
    patches: HashMap<u64, (usize, Arc<RenderImage>)>,
    overlays: HashMap<u64, (usize, u32, Arc<RenderImage>)>,
    pub(crate) canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
    focus: FocusHandle,
    /// Downscaled copy drawn while zoomed out, keyed by its factor. Drawing a
    /// 5K capture in a 2K-pixel window needs a quarter of the GPU memory.
    preview: Option<(u32, Arc<RenderImage>)>,
    full_on_gpu: bool,
    /// Object whose style was last edited, so a slider drag is one undo step.
    style_session: Option<u64>,
    space_down: bool,
    arrow_color_override: bool,
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Editor {
    pub fn new(shot: Shot, cx: &mut Context<Self>) -> Editor {
        let doc = Document::new(shot.width, shot.height, shot.scale);
        let mut styles: HashMap<_, _> = Tool::ALL
            .iter()
            .map(|t| (*t, Style::for_tool(*t, shot.scale)))
            .collect();
        if let Some(mut style) = crate::state::state(cx).settings.text_style.clone() {
            style.width *= shot.scale.max(1.0);
            style.text_size *= shot.scale.max(1.0);
            styles.insert(Tool::Text, style);
        }
        Editor {
            shot,
            doc,
            tool: Tool::Arrow,
            styles,
            selected: None,
            selection: None,
            zoom: None,
            pan: pt(0.0, 0.0),
            editing_text: None,
            panel: None,
            export_scale: 1.0,
            hover: None,
            source: None,
            drag: None,
            effects: EffectCache::default(),
            patches: HashMap::new(),
            overlays: HashMap::new(),
            canvas_bounds: Rc::new(Cell::new(Bounds::default())),
            focus: cx.focus_handle(),
            style_session: None,
            space_down: false,
            arrow_color_override: false,
            preview: None,
            full_on_gpu: false,
        }
    }

    /// The image to draw: full resolution when zoomed in enough to see its
    /// pixels, otherwise an integer-downscaled preview. Releases whichever
    /// texture is no longer shown.
    fn display_image(&mut self, window: &mut Window) -> Arc<RenderImage> {
        let mut bounds = self.canvas_bounds.get();
        if bounds.size.width <= px(1.) {
            // Before the first layout, estimate from the window minus chrome.
            let vp = window.viewport_size();
            bounds = Bounds {
                origin: Default::default(),
                size: gpui::size(vp.width, (vp.height - px(142.)).max(px(100.))),
            };
        }
        let view = canvas::compute_view(bounds, self.content_rect(), self.zoom, self.pan, self.shot.scale);
        let density = view.zoom * window.scale_factor();
        let factor = if density > 0.0 {
            ((1.0 / density).floor() as u32).clamp(1, 8)
        } else {
            1
        };
        if factor < 2 {
            self.full_on_gpu = true;
            return self.shot.image.clone();
        }
        if self.full_on_gpu {
            let _ = window.drop_image(self.shot.image.clone());
            self.full_on_gpu = false;
        }
        if self.preview.as_ref().map(|p| p.0) != Some(factor) {
            if let Some((_, old)) = self.preview.take() {
                let _ = window.drop_image(old);
            }
            self.preview = Some((factor, Shot::new(self.shot.pixels().downscale(factor), 1.0).image));
        }
        self.preview
            .as_ref()
            .map(|p| p.1.clone())
            .unwrap_or_else(|| self.shot.image.clone())
    }

    /// Downscale factor of the image currently drawn (1 = full resolution).
    pub fn display_factor(&self) -> u32 {
        if self.full_on_gpu {
            1
        } else {
            self.preview.as_ref().map(|p| p.0).unwrap_or(1)
        }
    }

    /// What the canvas shows: the export canvas plus any backdrop padding.
    pub fn content_rect(&self) -> Rect {
        let c = self.doc.canvas_rect();
        if self.doc.backdrop.enabled {
            c.inflate(self.doc.backdrop.padding)
        } else {
            c
        }
    }

    pub fn view(&self) -> View {
        canvas::compute_view(
            self.canvas_bounds.get(),
            self.content_rect(),
            self.zoom,
            self.pan,
            self.shot.scale,
        )
    }

    /// Zoom as a percentage of actual size (100% = one point per point).
    pub fn zoom_percent(&self) -> f32 {
        self.view().zoom * self.shot.scale * 100.0
    }

    pub fn style(&self) -> Style {
        self.styles
            .get(&self.tool)
            .cloned()
            .unwrap_or_else(|| Style::for_tool(self.tool, self.shot.scale))
    }

    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.finish_text();
        if tool != self.tool && tool != Tool::Select {
            // A new tool starts fresh: the last shape's handles must not
            // catch the next drag.
            self.selected = None;
        }
        self.tool = tool;
        if tool != Tool::Select {
            self.selection = None;
        }
        self.style_session = None;
        cx.notify();
    }

    /// Changes the current tool's style and the selected object's.
    pub fn edit_style(&mut self, f: impl Fn(&mut Style), cx: &mut Context<Self>) {
        let tool = self
            .selected
            .and_then(|id| self.doc.get(id))
            .map(|o| o.shape.tool())
            .unwrap_or(self.tool);
        if let Some(s) = self.styles.get_mut(&tool) {
            let before = s.color;
            f(s);
            if tool == Tool::Arrow && s.color != before {
                self.arrow_color_override = true;
            }
        }
        if tool == Tool::Text {
            let mut style = self.style_for(Tool::Text);
            style.width /= self.shot.scale.max(1.0);
            style.text_size /= self.shot.scale.max(1.0);
            crate::state::update_settings(cx, |s| s.text_style = Some(style));
        }
        if let Some(id) = self.selected {
            if self.style_session != Some(id) {
                self.doc.checkpoint();
                self.style_session = Some(id);
            }
            if let Some(o) = self.doc.get_mut(id) {
                f(&mut o.style);
            }
        }
        cx.notify();
    }

    pub fn active_style(&self) -> Style {
        self.selected
            .and_then(|id| self.doc.get(id))
            .map(|o| o.style.clone())
            .unwrap_or_else(|| self.style())
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.finish_text();
        if self.doc.undo() {
            self.after_history();
        }
        cx.notify();
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if self.doc.redo() {
            self.after_history();
        }
        cx.notify();
    }

    fn after_history(&mut self) {
        if self.selected.is_some_and(|id| self.doc.get(id).is_none()) {
            self.selected = None;
        }
        self.style_session = None;
        let doc = &self.doc;
        self.effects.retain(|id| doc.get(id).is_some());
    }

    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected.take() {
            self.doc.remove(id);
        } else if let Some(sel) = self.selection.take() {
            // Shottr-style object removal: delete with a selection erases it.
            let style = self.style_for(Tool::Erase);
            self.doc.add(Shape::Erase { rect: sel }, style);
        }
        cx.notify();
    }

    fn style_for(&self, tool: Tool) -> Style {
        self.styles
            .get(&tool)
            .cloned()
            .unwrap_or_else(|| Style::for_tool(tool, self.shot.scale))
    }

    /// Applies blur, pixelate or erase to the current selection.
    pub fn effect_on_selection(&mut self, tool: Tool, cx: &mut Context<Self>) -> bool {
        let Some(sel) = self.selection.take() else { return false };
        let shape = match tool {
            Tool::Blur => Shape::Blur { rect: sel },
            Tool::Pixelate => Shape::Pixelate { rect: sel },
            Tool::Erase => Shape::Erase { rect: sel },
            _ => {
                self.selection = Some(sel);
                return false;
            }
        };
        let style = self.style_for(tool);
        self.selected = Some(self.doc.add(shape, style));
        cx.notify();
        true
    }

    pub fn crop_to_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(sel) = self.selection.take() {
            self.doc.apply_crop(sel);
            self.zoom = None;
            self.pan = pt(0.0, 0.0);
            cx.notify();
        }
    }

    pub fn auto_adjust_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(sel) = self.selection {
            let px = sel.to_pixels(self.shot.width, self.shot.height);
            self.selection = Some(trim_to_content(self.shot.pixels(), px, 12).to_rect());
            cx.notify();
        }
    }

    pub fn grow_selection(&mut self, by: f32, cx: &mut Context<Self>) {
        if let Some(sel) = self.selection {
            self.selection = Some(sel.inflate(by).intersect(&self.doc.image_rect()));
            cx.notify();
        }
    }

    pub fn duplicate_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected {
            self.selected = self.doc.duplicate(id, 12.0 * self.shot.scale);
            cx.notify();
        }
    }

    pub fn nudge(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        if let Some(id) = self.selected {
            if self.style_session != Some(u64::MAX - id) {
                self.doc.checkpoint();
                self.style_session = Some(u64::MAX - id);
            }
            self.move_object(id, dx, dy);
        } else if let Some(sel) = self.selection {
            self.selection = Some(sel.translate(dx, dy));
        }
        cx.notify();
    }

    /// Moves an object. An arrow label is pinned to its arrow's tail, so
    /// moving the label moves the tail instead and the label follows.
    fn move_object(&mut self, id: u64, dx: f32, dy: f32) {
        let arrow = match self.doc.get(id).map(|o| &o.shape) {
            Some(Shape::Text { arrow: Some(a), .. }) if self.doc.get(*a).is_some() => *a,
            _ => id,
        };
        match self.doc.get_mut(arrow).map(|o| &mut o.shape) {
            Some(Shape::Arrow { from, .. }) if arrow != id => *from = from.offset(dx, dy),
            Some(_) => {
                if let Some(o) = self.doc.get_mut(id) {
                    o.translate(dx, dy);
                }
            }
            None => {}
        }
    }

    /// Starts typing a label at the tail, using the remembered text style.
    fn label_arrow(&mut self, arrow: u64, cx: &mut Context<Self>) {
        let Some(from) = self
            .doc
            .get(arrow)
            .and_then(|o| o.shape.endpoints().map(|(from, _)| from))
        else {
            return;
        };
        let id = self.insert(Shape::Text {
            origin: from,
            text: String::new(),
            extent: pt(0.0, 0.0),
            arrow: Some(arrow),
        });
        self.editing_text = Some(id);
        cx.notify();
    }

    /// Turns step numbers on or off for new arrows and the selected one.
    pub fn toggle_numbered(&mut self, cx: &mut Context<Self>) {
        let on = !self.active_style().numbered;
        self.edit_style(|s| s.numbered = on, cx);
        let next = self.doc.next_counter_number();
        if let Some(Shape::Arrow { number, .. }) =
            self.selected.and_then(|id| self.doc.get_mut(id)).map(|o| &mut o.shape)
        {
            *number = match (on, *number) {
                (true, None) => Some(next),
                (true, kept) => kept,
                (false, _) => None,
            };
        }
    }

    pub fn set_zoom(&mut self, zoom: Option<f32>, cx: &mut Context<Self>) {
        self.zoom = zoom.map(|z| z.clamp(0.02, 32.0));
        self.pan = pt(0.0, 0.0);
        cx.notify();
    }

    pub fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let z = self.view().zoom * factor;
        self.zoom = Some(z.clamp(0.02, 32.0));
        cx.notify();
    }

    /// Copies the pixel colour under the cursor as hex. `darkest` picks the
    /// darkest pixel in a 20 px square, handy for text colour.
    pub fn copy_color_at(&mut self, p: Pt, darkest: bool, cx: &mut Context<Self>) -> Option<String> {
        let (x, y) = (p.x.floor().max(0.0) as u32, p.y.floor().max(0.0) as u32);
        let img = self.shot.pixels();
        let c = if darkest {
            oshot_core::color::darkest_near(img, x, y, 20)
        } else {
            img.pixel(x, y)
        }?;
        let hex = c.to_hex();
        crate::output::copy_text(hex.clone(), cx);
        notify(cx, Notice::Copied(format!("Copied {hex}")));
        Some(hex)
    }

    /// Copies the average colour of the selection.
    pub fn copy_average_color(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let sel = self.selection?;
        let c = oshot_core::color::average_color(self.shot.pixels(), sel.to_pixels(self.shot.width, self.shot.height))?;
        let hex = c.to_hex();
        crate::output::copy_text(hex.clone(), cx);
        notify(cx, Notice::Copied(format!("Copied {hex}")));
        Some(hex)
    }

    /// Horizontal and vertical gaps around `p` in image pixels.
    pub fn measure_at(&self, p: Pt) -> Option<(Rect, Rect)> {
        let (x, y) = (p.x.floor(), p.y.floor());
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let img = self.shot.pixels();
        let (h0, h1) = measure_span(img, x as u32, y as u32, Axis::Horizontal, 10)?;
        let (v0, v1) = measure_span(img, x as u32, y as u32, Axis::Vertical, 10)?;
        Some((
            rect(h0 as f32, p.y, (h1 - h0) as f32, 0.0),
            rect(p.x, v0 as f32, 0.0, (v1 - v0) as f32),
        ))
    }

    fn finish_text(&mut self) {
        if let Some(id) = self.editing_text.take()
            && let Some(Shape::Text { text, .. }) = self.doc.get(id).map(|o| &o.shape)
            && text.trim().is_empty()
        {
            self.doc.cancel_insert(id);
            if self.selected == Some(id) {
                self.selected = None;
            }
        }
    }

    /// How much to enlarge new shapes so they keep their on-screen size when
    /// a big image is shown zoomed out (1 at 100% or closer).
    fn size_factor(&self) -> f32 {
        let actual = 1.0 / self.shot.scale.max(1.0);
        (actual / self.view().zoom.max(0.0001)).clamp(1.0, 16.0)
    }

    fn insert(&mut self, shape: Shape) -> u64 {
        let mut style = self.style_for(shape.tool());
        if let Shape::Arrow { from, to, .. } = shape
            && !self.arrow_color_override
        {
            style.color = self.arrow_color(from, to);
        }
        let k = self.size_factor();
        if k > 1.0 {
            style.width *= k;
            style.text_size *= k;
            if matches!(shape.tool(), Tool::Pixelate | Tool::Blur) {
                style.strength *= k;
            }
        }
        let id = self.doc.add(shape, style);
        self.selected = Some(id);
        self.style_session = None;
        id
    }

    /// Measure the pixels along the shaft, not unrelated parts of the image.
    fn arrow_color(&self, from: Pt, to: Pt) -> Color {
        let mut luminance = 0.0;
        let mut count = 0;
        for i in 0..=32 {
            let p = from.lerp(to, i as f32 / 32.0);
            if self.doc.image_rect().contains(p)
                && let Some(c) = self.shot.pixels().pixel(p.x as u32, p.y as u32)
            {
                luminance += c.luminance();
                count += 1;
            }
        }
        // Choose the greater contrast ratio against the measured luminance.
        if count > 0 && luminance / count as f32 > 0.179 {
            oshot_core::color::rgb(0, 0, 0)
        } else {
            oshot_core::color::rgb(255, 255, 255)
        }
    }

    // Mouse --------------------------------------------------------------

    fn image_point(&self, p: Point) -> Pt {
        self.view().to_image(p)
    }

    fn handle_at(&self, p: Pt) -> Option<(u64, Handle)> {
        let id = self.selected?;
        let obj = self.doc.get(id)?;
        let tol = 7.0 / self.view().zoom;
        obj.handles()
            .into_iter()
            .find(|(_, h)| h.distance(p) <= tol)
            .map(|(h, _)| (id, h))
    }

    pub(crate) fn on_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        if e.button == MouseButton::Right || e.button == MouseButton::Middle || self.space_down {
            self.drag = Some(Drag::Pan { last: e.position });
            return;
        }
        let p = self.image_point(e.position);
        let shift = e.modifiers.shift;
        if let Some((id, handle)) = self.handle_at(p) {
            self.doc.checkpoint();
            self.drag = Some(Drag::Handle { id, handle });
            return;
        }
        self.finish_text();
        let tol = 6.0 / self.view().zoom;
        match self.tool {
            Tool::Select => {
                if e.modifiers.platform {
                    let (x, y) = (p.x.max(0.0) as u32, p.y.max(0.0) as u32);
                    if let Some(r) = monotone_region(self.shot.pixels(), x, y, 10, 4_000_000) {
                        self.selection = Some(r.to_rect());
                        self.selected = None;
                    }
                } else if let Some(id) = self.doc.hit_test(p, tol) {
                    self.selected = Some(id);
                    self.selection = None;
                    if e.click_count >= 2
                        && let Some(Shape::Text { .. }) = self.doc.get(id).map(|o| &o.shape)
                    {
                        self.editing_text = Some(id);
                    } else {
                        self.doc.checkpoint();
                        self.drag = Some(Drag::Move { id, last: p });
                    }
                } else if let Some(sel) = self.selection.filter(|s| s.contains(p)) {
                    self.drag = Some(Drag::MoveSelection { start: p, orig: sel });
                } else {
                    self.selected = None;
                    self.selection = None;
                    self.drag = Some(Drag::Select { start: p });
                }
            }
            Tool::ColorPicker => {
                self.copy_color_at(p, shift, cx);
            }
            Tool::Text => {
                if let Some(id) = self
                    .doc
                    .hit_test(p, tol)
                    .filter(|id| matches!(self.doc.get(*id).map(|o| &o.shape), Some(Shape::Text { .. })))
                {
                    self.selected = Some(id);
                    self.editing_text = Some(id);
                } else {
                    let id = self.insert(Shape::Text {
                        origin: p,
                        text: String::new(),
                        extent: pt(0.0, 0.0),
                        arrow: None,
                    });
                    self.editing_text = Some(id);
                }
            }
            Tool::Counter => {
                let number = self.doc.next_counter_number();
                let id = self.insert(Shape::Counter { center: p, number });
                self.drag = Some(Drag::Move { id, last: p });
            }
            Tool::Freehand => {
                let id = self.insert(Shape::Freehand { points: vec![p] });
                self.drag = Some(Drag::Freehand { id });
            }
            Tool::Magnifier => {
                let st = self.style();
                let radius = 70.0 * self.shot.scale.max(1.0) * self.size_factor();
                let side = radius * 2.0 / st.strength.max(1.0);
                let source = rect(p.x - side / 2.0, p.y - side / 2.0, side, side);
                let id = self.insert(Shape::Magnifier {
                    source,
                    center: p,
                    radius,
                });
                self.drag = Some(Drag::Create { id, start: p });
            }
            tool => {
                let shape = match tool {
                    Tool::Arrow => Shape::Arrow {
                        from: p,
                        to: p,
                        number: self.style().numbered.then(|| self.doc.next_counter_number()),
                    },
                    Tool::Line => Shape::Line { from: p, to: p },
                    Tool::Highlighter => Shape::Highlighter { from: p, to: p },
                    Tool::Ruler => Shape::Ruler { from: p, to: p },
                    Tool::Rect => Shape::Rect {
                        rect: rect(p.x, p.y, 0.0, 0.0),
                    },
                    Tool::Oval => Shape::Oval {
                        rect: rect(p.x, p.y, 0.0, 0.0),
                    },
                    Tool::Pixelate => Shape::Pixelate {
                        rect: rect(p.x, p.y, 0.0, 0.0),
                    },
                    Tool::Blur => Shape::Blur {
                        rect: rect(p.x, p.y, 0.0, 0.0),
                    },
                    Tool::Erase => Shape::Erase {
                        rect: rect(p.x, p.y, 0.0, 0.0),
                    },
                    Tool::Spotlight => Shape::Spotlight {
                        rect: rect(p.x, p.y, 0.0, 0.0),
                    },
                    _ => return,
                };
                let id = self.insert(shape);
                self.drag = Some(Drag::Create { id, start: p });
            }
        }
        cx.notify();
    }

    pub(crate) fn on_move(&mut self, e: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let p = self.image_point(e.position);
        self.hover = Some(p);
        let shift = e.modifiers.shift;
        let pressed = e.pressed_button.is_some();
        match self.drag {
            Some(Drag::Pan { last }) if pressed => {
                self.pan = pt(
                    self.pan.x + f32::from(e.position.x - last.x),
                    self.pan.y + f32::from(e.position.y - last.y),
                );
                if self.zoom.is_none() {
                    self.zoom = Some(self.view().zoom);
                }
                self.drag = Some(Drag::Pan { last: e.position });
            }
            Some(Drag::Create { id, start }) if pressed => {
                if let Some(o) = self.doc.get_mut(id) {
                    match &mut o.shape {
                        Shape::Arrow { to, .. }
                        | Shape::Line { to, .. }
                        | Shape::Highlighter { to, .. }
                        | Shape::Ruler { to, .. } => {
                            *to = if shift { snap_45(start, p) } else { p };
                        }
                        Shape::Magnifier { center, .. } => *center = p,
                        shape => {
                            let end = if shift { square_from(start, p) } else { p };
                            if let Some(r) = rect_mut(shape) {
                                *r = Rect::from_points(start, end);
                            }
                        }
                    }
                }
                if !self.arrow_color_override
                    && let Some(Shape::Arrow { from, to, .. }) = self.doc.get(id).map(|o| &o.shape)
                {
                    let color = self.arrow_color(*from, *to);
                    if let Some(o) = self.doc.get_mut(id) {
                        o.style.color = color;
                    }
                }
            }
            Some(Drag::Freehand { id }) if pressed => {
                let min = 1.5 / self.view().zoom;
                if let Some(Shape::Freehand { points }) = self.doc.get_mut(id).map(|o| &mut o.shape)
                    && points.last().is_none_or(|l| l.distance(p) >= min)
                {
                    points.push(p);
                }
            }
            Some(Drag::Move { id, last }) if pressed => {
                self.move_object(id, p.x - last.x, p.y - last.y);
                self.drag = Some(Drag::Move { id, last: p });
            }
            Some(Drag::Handle { id, handle }) if pressed => {
                if let Some(o) = self.doc.get_mut(id) {
                    o.drag_handle(handle, p, shift);
                }
            }
            Some(Drag::Select { start }) if pressed => {
                let end = if shift { square_from(start, p) } else { p };
                self.selection = Some(Rect::from_points(start, end).intersect(&self.doc.image_rect()));
            }
            Some(Drag::MoveSelection { start, orig }) if pressed => {
                self.selection = Some(orig.translate(p.x - start.x, p.y - start.y));
            }
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn on_up(&mut self, _e: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match self.drag.take() {
            Some(Drag::Create { id, .. }) => {
                let tiny = self.doc.get(id).is_some_and(|o| {
                    let b = o
                        .shape
                        .rect()
                        .map(|r| r.w.max(r.h))
                        .or_else(|| o.shape.endpoints().map(|(a, b)| a.distance(b)));
                    b.is_some_and(|size| size < 3.0)
                });
                if tiny {
                    self.doc.cancel_insert(id);
                    self.selected = None;
                }
            }
            Some(Drag::Freehand { id }) => {
                if matches!(self.doc.get(id).map(|o| &o.shape), Some(Shape::Freehand { points }) if points.len() < 2) {
                    self.doc.cancel_insert(id);
                    self.selected = None;
                }
            }
            Some(Drag::Select { .. }) if self.selection.is_some_and(|s| s.is_empty()) => {
                self.selection = None;
            }
            _ => {}
        }
        // The next style edit starts its own undo step, separate from the
        // drawing or move that just finished.
        self.style_session = None;
        cx.notify();
    }

    pub(crate) fn on_scroll(&mut self, e: &ScrollEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let d = e.delta.pixel_delta(px(16.));
        if e.modifiers.platform || e.modifiers.control {
            let factor = (1.0 - f32::from(d.y) / 200.0).clamp(0.8, 1.25);
            self.zoom_by(factor, cx);
        } else {
            if self.zoom.is_none() {
                self.zoom = Some(self.view().zoom);
            }
            self.pan = pt(self.pan.x + f32::from(d.x), self.pan.y + f32::from(d.y));
            cx.notify();
        }
    }

    // Keyboard -----------------------------------------------------------

    pub(crate) fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let m = k.modifiers;
        let key = k.key.as_str();
        if let Some(id) = self.editing_text
            && self.type_text(id, key, k.key_char.as_deref(), m, cx)
        {
            return;
        }
        let step = if m.shift { 10.0 } else { 1.0 };
        match (key, m.platform, m.shift) {
            ("z", true, false) => self.undo(cx),
            ("z", true, true) => self.redo(cx),
            ("c", true, _) => export::copy_shortcut(self, window, cx),
            ("s", true, false) => export::run(self, Target::SaveFolder, cx),
            ("s", true, true) => export::save_as(self, cx),
            ("v", true, _) => self.paste(cx),
            ("d", true, _) => self.duplicate_selected(cx),
            ("=" | "+", true, _) => self.zoom_by(1.25, cx),
            ("-", true, _) => self.zoom_by(0.8, cx),
            ("0", true, _) => self.set_zoom(Some(1.0 / self.shot.scale.max(1.0)), cx),
            ("1", true, _) => self.set_zoom(None, cx),
            ("2", true, _) => self.zoom_to_selection(cx),
            ("w", true, _) => window.remove_window(),
            ("escape", _, _) => self.escape(window, cx),
            ("enter", false, _) => self.crop_to_selection(cx),
            ("backspace" | "delete", _, _) => self.delete_selected(cx),
            ("tab", false, shift) => {
                if let Some(p) = self.hover {
                    self.copy_color_at(p, shift, cx);
                }
            }
            ("a", false, true) => self.auto_adjust_selection(cx),
            ("c", false, false) if self.selection.is_some() => {
                self.copy_average_color(cx);
            }
            ("[", false, shift) => self.grow_selection(if shift { -10.0 } else { -1.0 }, cx),
            ("]", false, shift) => self.grow_selection(if shift { 10.0 } else { 1.0 }, cx),
            ("left", false, _) => self.nudge(-step, 0.0, cx),
            ("right", false, _) => self.nudge(step, 0.0, cx),
            ("up", false, _) => self.nudge(0.0, -step, cx),
            ("down", false, _) => self.nudge(0.0, step, cx),
            ("space", false, _) => self.space_down = true,
            (k, false, false) => {
                if let Some(tool) = Tool::ALL.iter().copied().find(|t| t.key() == k)
                    && !(self.selection.is_some() && self.effect_on_selection(tool, cx))
                {
                    let arrow = self
                        .selected
                        .filter(|id| matches!(self.doc.get(*id).map(|o| &o.shape), Some(Shape::Arrow { .. })));
                    self.set_tool(tool, cx);
                    if let (Tool::Text, Some(arrow)) = (tool, arrow) {
                        self.label_arrow(arrow, cx);
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn on_key_up(&mut self, e: &gpui::KeyUpEvent, _: &mut Window, _: &mut Context<Self>) {
        if e.keystroke.key == "space" {
            self.space_down = false;
        }
    }

    fn type_text(&mut self, id: u64, key: &str, ch: Option<&str>, m: gpui::Modifiers, cx: &mut Context<Self>) -> bool {
        let Some(Shape::Text { text, .. }) = self.doc.get_mut(id).map(|o| &mut o.shape) else {
            self.editing_text = None;
            return false;
        };
        match key {
            "escape" => {
                self.finish_text();
                cx.notify();
                return true;
            }
            "enter" if !m.shift => {
                self.finish_text();
                cx.notify();
                return true;
            }
            "enter" => text.push('\n'),
            "backspace" => {
                text.pop();
            }
            "v" if m.platform => {
                if let Some(s) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    text.push_str(&s);
                }
            }
            "a" if m.platform => text.clear(),
            _ if m.platform || m.control => return false,
            _ => match ch {
                Some(c) if !c.chars().any(|c| c.is_control()) => text.push_str(c),
                _ => return false,
            },
        }
        cx.notify();
        true
    }

    /// Esc steps back one level: panel, then selection, then the window.
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let closed_something =
            self.panel.take().is_some() || self.selected.take().is_some() || self.selection.take().is_some();
        if closed_something {
            cx.notify();
            return;
        }
        if crate::state::state(cx).settings.escape_copies {
            export::run(self, Target::Clipboard, cx);
        }
        window.remove_window();
    }

    fn zoom_to_selection(&mut self, cx: &mut Context<Self>) {
        let Some(sel) = self.selection else { return };
        let b = self.canvas_bounds.get();
        let z = canvas::fit_zoom(b, sel, 1.0 / 32.0);
        let content = self.content_rect();
        self.zoom = Some(z);
        self.pan = pt(
            (content.center().x - sel.center().x) * z,
            (content.center().y - sel.center().y) * z,
        );
        cx.notify();
    }

    /// ⌘V: pastes a clipboard image as an overlay (or text into a label).
    pub fn paste(&mut self, cx: &mut Context<Self>) {
        let image = cx.read_from_clipboard().and_then(|item| {
            item.entries().iter().find_map(|e| match e {
                ClipboardEntry::Image(img) => Some(img.bytes.clone()),
                _ => None,
            })
        });
        let Some(bytes) = image else {
            notify(cx, Notice::Info("The clipboard has no image to paste".into()));
            return;
        };
        match oshot_native::imageio::decode(&bytes) {
            Ok((bmp, _)) => self.add_overlay(bmp, cx),
            Err(e) => notify(cx, Notice::Error(e)),
        }
    }

    pub fn add_overlay(&mut self, bmp: Bitmap, cx: &mut Context<Self>) {
        let base = self.doc.base_rect();
        let fit = (base.w * 0.5 / bmp.width as f32)
            .min(base.h * 0.5 / bmp.height as f32)
            .min(1.0);
        let (w, h) = (bmp.width as f32 * fit, bmp.height as f32 * fit);
        let c = base.center();
        let shape = Shape::Image {
            rect: rect(c.x - w / 2.0, c.y - h / 2.0, w, h),
            image: Arc::new(bmp),
            opacity: 1.0,
        };
        let id = self.doc.add(shape, Style::for_tool(Tool::Select, self.shot.scale));
        self.selected = Some(id);
        self.tool = Tool::Select;
        cx.notify();
    }

    pub fn set_overlay_opacity(&mut self, opacity: f32, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        if self.style_session != Some(id) {
            self.doc.checkpoint();
            self.style_session = Some(id);
        }
        if let Some(Shape::Image { opacity: o, .. }) = self.doc.get_mut(id).map(|o| &mut o.shape) {
            *o = opacity.clamp(0.05, 1.0);
        }
        cx.notify();
    }

    // Painting data ------------------------------------------------------

    fn update_text_extents(&mut self, window: &Window) {
        let updates: Vec<(u64, Pt)> = self
            .doc
            .objects
            .iter()
            .filter_map(|o| match &o.shape {
                Shape::Text { text, .. } => {
                    let font = if o.style.hand_drawn {
                        PrimFont::Hand
                    } else {
                        PrimFont::Bold
                    };
                    let shown = if text.is_empty() { " " } else { text.as_str() };
                    let (w, h) = canvas::text_extent(shown, o.style.text_size, font, window);
                    let pad = if o.style.text_background {
                        o.style.text_size * 0.6
                    } else {
                        0.0
                    };
                    Some((o.id, pt(w + pad, h + pad)))
                }
                _ => None,
            })
            .collect();
        for (id, e) in updates {
            self.doc.set_text_extent(id, e);
        }
        self.doc.place_arrow_labels();
    }

    fn dragging_id(&self) -> Option<u64> {
        match self.drag {
            Some(Drag::Create { id, .. } | Drag::Move { id, .. } | Drag::Handle { id, .. }) => Some(id),
            _ => None,
        }
    }

    pub(crate) fn scene(&mut self, window: &mut Window, accent: gpui::Hsla, canvas_bg: gpui::Hsla) -> canvas::Scene {
        self.update_text_extents(window);
        let prims = document_prims(&self.doc);
        let dragging = self.dragging_id();
        let mut effects = HashMap::new();
        let mut placeholders = Vec::new();
        let mut images = HashMap::new();
        for prim in &prims {
            match prim {
                Prim::Effect { object, dest, circle } => {
                    let Some(obj) = self.doc.get(*object) else { continue };
                    if dragging == Some(*object) && !*circle {
                        placeholders.push(*dest);
                        continue;
                    }
                    let Some((patch_rect, bmp)) = self.effects.get(obj, self.shot.pixels()) else {
                        continue;
                    };
                    let key = Arc::as_ptr(&bmp) as usize;
                    let image = match self.patches.get(object) {
                        Some((k, img)) if *k == key => img.clone(),
                        old => {
                            if let Some((_, img)) = old {
                                let _ = window.drop_image(img.clone());
                            }
                            let img = Shot::new((*bmp).clone(), 1.0).image;
                            self.patches.insert(*object, (key, img.clone()));
                            img
                        }
                    };
                    effects.insert(*object, (if *circle { *dest } else { patch_rect }, image, *circle));
                }
                Prim::Image { object, opacity, .. } => {
                    let Some(Shape::Image { image, .. }) = self.doc.get(*object).map(|o| &o.shape) else {
                        continue;
                    };
                    let key = Arc::as_ptr(image) as usize;
                    let alpha = (opacity * 255.0).round() as u32;
                    let ri = match self.overlays.get(object) {
                        Some((k, a, img)) if *k == key && *a == alpha => img.clone(),
                        _ => {
                            let mut bmp = (**image).clone();
                            if alpha < 255 {
                                for px in bmp.data.as_chunks_mut::<4>().0 {
                                    px[3] = (px[3] as u32 * alpha / 255) as u8;
                                }
                            }
                            let img = Shot::new(bmp, 1.0).image;
                            self.overlays.insert(*object, (key, alpha, img.clone()));
                            img
                        }
                    };
                    images.insert(*object, ri);
                }
                _ => {}
            }
        }
        let selected = self
            .selected
            .and_then(|id| self.doc.get(id))
            .map(|o| (o.bounds(), o.handles().into_iter().map(|(_, p)| p).collect()));
        let caret = self
            .editing_text
            .and_then(|id| self.doc.get(id))
            .and_then(|o| match &o.shape {
                Shape::Text { origin, extent, .. } => {
                    let pad = if o.style.text_background {
                        o.style.text_size * 0.3
                    } else {
                        0.0
                    };
                    Some((
                        pt(origin.x + (extent.x - pad).max(pad), origin.y + pad),
                        o.style.text_size * 1.15,
                    ))
                }
                _ => None,
            });
        let measure = (self.tool == Tool::Ruler && self.drag.is_none())
            .then(|| self.hover.and_then(|p| self.measure_at(p)))
            .flatten();
        let backdrop = self.doc.backdrop.enabled.then(|| {
            let b = &self.doc.backdrop;
            (
                self.content_rect(),
                canvas::backdrop_colors(b.preset),
                b.radius,
                b.shadow,
            )
        });
        canvas::Scene {
            image: self.display_image(window),
            image_rect: self.doc.image_rect(),
            base: self.doc.base_rect(),
            canvas: self.doc.canvas_rect(),
            backdrop,
            prims,
            effects,
            images,
            placeholders,
            selected,
            selection: self.selection,
            caret,
            measure,
            accent,
            canvas_bg,
        }
    }

    /// Screen position of an image point, for tests and tooltips.
    pub fn screen_point(&self, p: Pt) -> gpui::Point<Pixels> {
        self.view().to_screen(p)
    }

    pub fn counter_radius(&self) -> f32 {
        counter_radius(&self.style_for(Tool::Counter))
    }

    pub fn hover_color(&self) -> Option<Color> {
        let p = self.hover?;
        self.shot.pixels().pixel(p.x.max(0.0) as u32, p.y.max(0.0) as u32)
    }
}

fn rect_mut(shape: &mut Shape) -> Option<&mut Rect> {
    match shape {
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

/// Opens the editor for a capture.
pub fn open_shot(shot: Shot, cx: &mut App) {
    open_with(shot, None, cx);
}

pub fn open_with(shot: Shot, source: Option<PathBuf>, cx: &mut App) -> Option<gpui::WindowHandle<Editor>> {
    let display = windows::cursor_display(cx);
    let (max_w, max_h) = display
        .as_ref()
        .map(|d| (d.frame.w * 0.85, d.frame.h * 0.85))
        .unwrap_or((1400.0, 900.0));
    let img_w = shot.width as f32 / shot.scale.max(1.0);
    let img_h = shot.height as f32 / shot.scale.max(1.0);
    let w = (img_w + 80.0).clamp(820.0, max_w.max(820.0));
    let h = (img_h + 170.0).clamp(560.0, max_h.max(560.0));
    let mut spec = Spec::window("OShot", w, h);
    spec.min_size = Some(gpui::size(px(760.), px(480.)));
    spec.display = display;
    let handle = windows::open(spec, cx, |_, cx| {
        cx.new(|cx| {
            let mut e = Editor::new(shot, cx);
            e.source = source;
            e
        })
    })?;
    let _ = handle.update(cx, |editor, window, cx| {
        window.set_window_title(&format!("OShot — {} × {}", editor.shot.width, editor.shot.height));
        editor.focus.focus(window, cx);
    });
    Some(handle)
}

pub fn open_file(path: PathBuf, cx: &mut App) {
    let p = path.clone();
    let task = cx.background_spawn(async move {
        let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
        oshot_native::imageio::decode(&bytes)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok((bmp, scale)) => {
                open_with(Shot::new(bmp, scale as f32), Some(path), cx);
            }
            Err(e) => notify(cx, Notice::Error(format!("Could not open image: {e}"))),
        });
    })
    .detach();
}

/// Opens the clipboard image in the editor.
pub fn open_clipboard(cx: &mut App) {
    let bytes = cx.read_from_clipboard().and_then(|item| {
        item.entries().iter().find_map(|e| match e {
            ClipboardEntry::Image(img) => Some(img.bytes.clone()),
            _ => None,
        })
    });
    let Some(bytes) = bytes else {
        notify(cx, Notice::Info("The clipboard has no image".into()));
        return;
    };
    match oshot_native::imageio::decode(&bytes) {
        Ok((bmp, scale)) => {
            open_with(Shot::new(bmp, scale as f32), None, cx);
        }
        Err(e) => notify(cx, Notice::Error(e)),
    }
}
