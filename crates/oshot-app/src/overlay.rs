//! The full-screen capture overlay, drawn over a frozen copy of the display.
//!
//! Freezing first (as Shottr does) makes selection instant and means the
//! overlay never appears in the capture. Window mode is the exception: like
//! macOS, it tints windows over the live screen, since the window itself is
//! captured on its own afterwards.

use std::sync::Arc;
use std::time::Instant;

use gpui::{
    App, AppContext, Bounds, Context, Corners, CursorStyle, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point,
    Render, RenderImage, Styled, Window, WindowKind, canvas, div, fill, img, outline, point, prelude::FluentBuilder,
    px, quad, size,
};
use oshot_core::color::Color;
use oshot_core::effects::magnify;
use oshot_core::geometry::{IRect, Pt, Rect, pt, rect};
use oshot_core::measure::{Axis, measure_span, trim_to_content};

use crate::capture;
use crate::output;
use crate::state::{DisplayInfo, Notice, Shot, WindowInfo, notify, platform, state};
use crate::theme::{Theme, hex, theme};
use crate::ui::hint;
use crate::windows::{self, Spec};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Area,
    Window,
    Text,
    Color,
    Ruler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Copy,
    Save,
    /// The after-capture flow from Settings.
    Finish,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    New {
        start: Pt,
    },
    Move {
        start: Pt,
        orig: Rect,
    },
    /// Corner 0..4 clockwise from top-left.
    Corner {
        corner: usize,
        orig: Rect,
    },
}

pub struct Overlay {
    pub mode: Mode,
    pub shot: Shot,
    pub display: DisplayInfo,
    /// Window frames in display-local points, front to back.
    pub windows: Vec<WindowInfo>,
    pub selection: Option<Rect>,
    pub cursor: Option<Pt>,
    pub hovered_window: Option<usize>,
    drag: Option<Drag>,
    loupe: Option<Arc<RenderImage>>,
    focus: FocusHandle,
    /// The window highlight glides from one window to the next: where it
    /// started, when, and the window it is heading to.
    glide: Option<(Rect, Instant, Rect)>,
}

/// How long the window highlight takes to move to a newly hovered window.
const GLIDE_MS: f32 = 140.0;
/// macOS 26 window corner radius, so the highlight hugs the window.
const WINDOW_RADIUS: f32 = 12.0;

const HANDLE: f32 = 10.0;
const LOUPE_PIXELS: u32 = 15;
const LOUPE_SIZE: f32 = 120.0;

impl Overlay {
    pub fn new(
        mode: Mode,
        shot: Shot,
        display: DisplayInfo,
        windows: Vec<WindowInfo>,
        cx: &mut Context<Self>,
    ) -> Overlay {
        let local = windows
            .into_iter()
            .filter_map(|mut w| {
                w.frame = w.frame.translate(-display.frame.x, -display.frame.y).intersect(&rect(
                    0.0,
                    0.0,
                    display.frame.w,
                    display.frame.h,
                ));
                (!w.frame.is_empty()).then_some(w)
            })
            .collect();
        Overlay {
            mode,
            shot,
            display,
            windows: local,
            selection: None,
            cursor: None,
            hovered_window: None,
            drag: None,
            loupe: None,
            focus: cx.focus_handle(),
            glide: None,
        }
    }

    /// The highlight to draw for the hovered window, part-way along its glide
    /// from the previous one. Asks for another frame while it moves.
    fn window_highlight(&mut self, window: &mut Window, cx: &App) -> Option<Rect> {
        let target = self.hovered_window.and_then(|i| self.windows.get(i)).map(|w| w.frame)?;
        let now = Instant::now();
        let current = |g: &(Rect, Instant, Rect)| {
            let t = (g.1.elapsed().as_secs_f32() * 1000.0 / GLIDE_MS).clamp(0.0, 1.0);
            let e = 1.0 - (1.0 - t).powi(3);
            let (a, b) = (g.0, g.2);
            rect(
                a.x + (b.x - a.x) * e,
                a.y + (b.y - a.y) * e,
                a.w + (b.w - a.w) * e,
                a.h + (b.h - a.h) * e,
            )
        };
        match self.glide {
            Some(g) if g.2 == target => {}
            Some(g) => self.glide = Some((current(&g), now, target)),
            None => self.glide = Some((target, now, target)),
        }
        let g = self.glide?;
        if cx.reduce_motion() {
            return Some(target);
        }
        if g.1.elapsed().as_secs_f32() * 1000.0 < GLIDE_MS {
            window.request_animation_frame();
        }
        Some(current(&g))
    }

    fn scale(&self) -> f32 {
        self.shot.scale
    }

    /// Selection (points) to image pixels.
    pub fn pixel_rect(&self, r: Rect) -> IRect {
        let s = self.scale();
        rect(r.x * s, r.y * s, r.w * s, r.h * s).to_pixels(self.shot.width, self.shot.height)
    }

    pub fn pixel_at(&self, p: Pt) -> Option<(u32, u32, Color)> {
        let s = self.scale();
        let (x, y) = ((p.x * s).floor().max(0.0) as u32, (p.y * s).floor().max(0.0) as u32);
        self.shot.pixels().pixel(x, y).map(|c| (x, y, c))
    }

    fn window_at(&self, p: Pt) -> Option<usize> {
        self.windows.iter().position(|w| w.frame.contains(p))
    }

    /// Horizontal and vertical spans around `p`, in points.
    pub fn measure(&self, p: Pt) -> Option<(Rect, Rect)> {
        let s = self.scale();
        let (x, y, _) = self.pixel_at(p)?;
        let img = self.shot.pixels();
        let (h0, h1) = measure_span(img, x, y, Axis::Horizontal, 10)?;
        let (v0, v1) = measure_span(img, x, y, Axis::Vertical, 10)?;
        Some((
            rect(h0 as f32 / s, p.y, (h1 - h0) as f32 / s, 0.0),
            rect(p.x, v0 as f32 / s, 0.0, (v1 - v0) as f32 / s),
        ))
    }

    fn update_loupe(&mut self, window: &mut Window) {
        let Some(p) = self.cursor else { return };
        let Some((x, y, _)) = self.pixel_at(p) else { return };
        let half = LOUPE_PIXELS / 2;
        let src = IRect {
            x: x.saturating_sub(half),
            y: y.saturating_sub(half),
            w: LOUPE_PIXELS,
            h: LOUPE_PIXELS,
        };
        let side = (LOUPE_SIZE * 2.0) as u32;
        let patch = magnify(self.shot.pixels(), src, side, side);
        let next = Shot::new(patch, 2.0).image;
        if let Some(old) = self.loupe.replace(next) {
            let _ = window.drop_image(old);
        }
    }

    fn corner_at(&self, p: Pt) -> Option<usize> {
        let sel = self.selection?;
        sel.corners().iter().position(|c| c.distance(p) <= HANDLE)
    }

    fn on_down(&mut self, e: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let p = to_pt(e.position);
        match self.mode {
            Mode::Area | Mode::Text => {
                self.drag = Some(if let Some(corner) = self.corner_at(p) {
                    Drag::Corner {
                        corner,
                        orig: self.selection.unwrap_or_default(),
                    }
                } else if let Some(sel) = self.selection.filter(|s| s.contains(p)) {
                    Drag::Move { start: p, orig: sel }
                } else {
                    self.selection = None;
                    Drag::New { start: p }
                });
            }
            // Clicks in these modes act on release, so the mouse-up never
            // falls through to the app underneath a closed overlay.
            Mode::Window => self.hovered_window = self.window_at(p),
            Mode::Color | Mode::Ruler => {}
        }
        cx.notify();
    }

    fn on_move(&mut self, e: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let p = to_pt(e.position);
        self.cursor = Some(p);
        if self.mode == Mode::Window {
            self.hovered_window = self.window_at(p);
        }
        if let Some(drag) = self.drag
            && e.pressed_button == Some(MouseButton::Left)
        {
            let bounds = rect(0.0, 0.0, self.display.frame.w, self.display.frame.h);
            self.selection = Some(match drag {
                Drag::New { start } => {
                    let end = if e.modifiers.shift {
                        oshot_core::model::square_from(start, p)
                    } else {
                        p
                    };
                    Rect::from_points(start, end).intersect(&bounds)
                }
                Drag::Move { start, orig } => {
                    let dx = (p.x - start.x).clamp(-orig.x, bounds.w - orig.right());
                    let dy = (p.y - start.y).clamp(-orig.y, bounds.h - orig.bottom());
                    orig.translate(dx, dy)
                }
                Drag::Corner { corner, orig } => {
                    Rect::from_points(orig.corners()[(corner + 2) % 4], p).intersect(&bounds)
                }
            });
        }
        if matches!(self.mode, Mode::Area | Mode::Text | Mode::Color) {
            self.update_loupe(window);
        }
        cx.notify();
    }

    fn on_up(&mut self, e: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let p = to_pt(e.position);
        match self.mode {
            Mode::Window => {
                if self.window_at(p).is_some() {
                    self.hovered_window = self.window_at(p);
                    self.complete(Action::Finish, cx);
                }
                return;
            }
            Mode::Color => return self.pick_color(p, e.modifiers.shift, cx),
            Mode::Ruler => return self.copy_measurement(p, cx),
            Mode::Area | Mode::Text => {}
        }
        let Some(drag) = self.drag.take() else { return };
        if let Some(sel) = self.selection
            && (sel.w < 3.0 || sel.h < 3.0)
        {
            self.selection = None;
        }
        let made_new = matches!(drag, Drag::New { .. });
        if self.selection.is_some() && made_new {
            let finish_now = self.mode == Mode::Text || state(cx).settings.finish_on_release;
            if finish_now {
                self.complete(Action::Finish, cx);
            }
        }
        cx.notify();
    }

    fn on_key(&mut self, e: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let m = k.modifiers;
        match (k.key.as_str(), m.platform) {
            ("escape", _) => self.close(cx),
            ("c", true) => self.complete(Action::Copy, cx),
            ("s", true) => self.complete(Action::Save, cx),
            ("enter", _) => self.complete(Action::Finish, cx),
            ("a", false) if self.mode == Mode::Area => self.auto_adjust(),
            (dir @ ("left" | "right" | "up" | "down"), cmd) => {
                if let Some(sel) = self.selection {
                    let step = if m.shift { 10.0 } else { 1.0 } / self.scale();
                    let (dx, dy) = match dir {
                        "left" => (-step, 0.0),
                        "right" => (step, 0.0),
                        "up" => (0.0, -step),
                        _ => (0.0, step),
                    };
                    self.selection = Some(if cmd {
                        rect(sel.x, sel.y, (sel.w + dx).max(1.0), (sel.h + dy).max(1.0))
                    } else {
                        sel.translate(dx, dy)
                    });
                }
            }
            _ => return,
        }
        cx.notify();
    }

    /// Shrinks the selection to the content inside it (Shottr's "A").
    pub fn auto_adjust(&mut self) {
        let Some(sel) = self.selection else { return };
        let px = self.pixel_rect(sel);
        let t = trim_to_content(self.shot.pixels(), px, 12);
        let s = self.scale();
        self.selection = Some(rect(t.x as f32 / s, t.y as f32 / s, t.w as f32 / s, t.h as f32 / s));
    }

    fn pick_color(&mut self, p: Pt, darkest: bool, cx: &mut Context<Self>) {
        let color = if darkest {
            self.pixel_at(p)
                .and_then(|(x, y, _)| oshot_core::color::darkest_near(self.shot.pixels(), x, y, 20))
        } else {
            self.pixel_at(p).map(|(_, _, c)| c)
        };
        if let Some(c) = color {
            let hex = c.to_hex();
            output::copy_text(hex.clone(), cx);
            notify(cx, Notice::Copied(format!("Copied {hex}")));
        }
        self.close(cx);
    }

    fn copy_measurement(&mut self, p: Pt, cx: &mut Context<Self>) {
        if let Some((h, v)) = self.measure(p) {
            let label = format!("{} × {}", h.w.round(), v.h.round());
            output::copy_text(label.clone(), cx);
            notify(cx, Notice::Copied(format!("Copied {label}")));
        }
        self.close(cx);
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.selection = None;
        cx.defer(|cx| {
            close_all(cx);
        });
    }

    pub fn complete(&mut self, action: Action, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Area | Mode::Text => {
                let Some(sel) = self.selection else { return };
                let area = self.pixel_rect(sel);
                if area.is_empty() {
                    return;
                }
                let shot = self.shot.crop(area);
                let mode = self.mode;
                let display = self.display.clone();
                self.close(cx);
                cx.defer(move |cx| {
                    if mode == Mode::Text {
                        recognize_and_copy(shot, cx);
                    } else {
                        capture::remember_area(&display, sel, cx);
                        deliver(shot, action, cx);
                    }
                });
            }
            Mode::Window => {
                let Some(win) = self.hovered_window.and_then(|i| self.windows.get(i)).cloned() else {
                    return;
                };
                self.close(cx);
                let shadow = state(cx).settings.window_shadow;
                let p = platform(cx);
                let task = cx.background_spawn(async move { p.capture_window(win.id, shadow) });
                cx.spawn(async move |_, cx| {
                    let result = task.await;
                    cx.update(|cx| match result {
                        Ok(c) => deliver(Shot::new(c.bitmap, c.scale), action, cx),
                        Err(e) => notify(cx, Notice::Error(e)),
                    });
                })
                .detach();
            }
            Mode::Color | Mode::Ruler => {}
        }
    }
}

fn deliver(shot: Shot, action: Action, cx: &mut App) {
    match action {
        Action::Copy => {
            output::remember(&shot, cx);
            output::copy(&shot, true, cx);
        }
        Action::Save => {
            output::remember(&shot, cx);
            output::save(&shot, cx);
        }
        Action::Finish => output::finish(shot, cx),
    }
}

fn recognize_and_copy(shot: Shot, cx: &mut App) {
    let task = cx.background_spawn(async move { oshot_native::ocr::recognize(shot.pixels()) });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok(r) if !r.text().trim().is_empty() => {
                let text = r.text();
                let lines = text.lines().count();
                output::copy_text(text, cx);
                notify(
                    cx,
                    Notice::Copied(format!(
                        "Copied {lines} line{} of text",
                        if lines == 1 { "" } else { "s" }
                    )),
                );
            }
            Ok(_) => notify(cx, Notice::Info("No text found in that area".into())),
            Err(e) => notify(cx, Notice::Error(e)),
        });
    })
    .detach();
}

fn to_pt(p: Point<Pixels>) -> Pt {
    pt(f32::from(p.x), f32::from(p.y))
}

fn bounds_of(r: Rect) -> Bounds<Pixels> {
    Bounds {
        origin: point(px(r.x), px(r.y)),
        size: size(px(r.w), px(r.h)),
    }
}

impl Focusable for Overlay {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn pill(text: String, t: &Theme) -> gpui::Div {
    div()
        .px(px(8.))
        .py(px(3.))
        .rounded(px(6.))
        .bg(hex(0x000000, 0.72))
        .text_color(hex(0xFFFFFF, 1.0))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .border_1()
        .border_color(t.border)
        .child(text)
}

impl Render for Overlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        let image = self.shot.image.clone();
        let full = rect(0.0, 0.0, self.display.frame.w, self.display.frame.h);
        let highlight = match self.mode {
            Mode::Area | Mode::Text => self.selection,
            Mode::Window => self.window_highlight(window, cx),
            _ => None,
        };
        // Like macOS's own window capture: nothing is dimmed, the window
        // under the pointer takes a blue tint instead.
        let dims = matches!(self.mode, Mode::Area | Mode::Text);
        let tint = self.mode == Mode::Window;
        let crosshair = matches!(self.mode, Mode::Area | Mode::Text) && self.drag.is_none() && self.selection.is_none();
        let cursor = self.cursor;
        let measure = (self.mode == Mode::Ruler)
            .then(|| cursor.and_then(|p| self.measure(p)))
            .flatten();
        let dim = t.dim;
        let accent = t.accent;
        let live = self.mode == Mode::Window;
        let painter = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                if live {
                    // macOS sends clicks on fully clear pixels to the window
                    // below, so the live view keeps a barely-there fill.
                    window.paint_quad(fill(bounds, hex(0x000000, 0.01)));
                } else {
                    let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
                }
                if dims {
                    match highlight {
                        Some(h) => {
                            let h = h.intersect(&full);
                            for r in [
                                rect(0.0, 0.0, full.w, h.y),
                                rect(0.0, h.bottom(), full.w, full.h - h.bottom()),
                                rect(0.0, h.y, h.x, h.h),
                                rect(h.right(), h.y, full.w - h.right(), h.h),
                            ] {
                                if !r.is_empty() {
                                    window.paint_quad(fill(bounds_of(r), dim));
                                }
                            }
                            window.paint_quad(outline(bounds_of(h), hex(0xFFFFFF, 0.95), gpui::BorderStyle::Solid));
                        }
                        None => window.paint_quad(fill(bounds_of(full), hex(0x000000, 0.18))),
                    }
                }
                if let Some(h) = highlight.filter(|_| tint) {
                    window.paint_quad(quad(
                        bounds_of(h),
                        px(WINDOW_RADIUS),
                        accent.opacity(0.28),
                        px(2.),
                        accent.opacity(0.85),
                        gpui::BorderStyle::Solid,
                    ));
                }
                if let Some(p) = cursor.filter(|_| crosshair) {
                    let line = hex(0xFFFFFF, 0.55);
                    window.paint_quad(fill(bounds_of(rect(0.0, p.y.floor(), full.w, 1.0)), line));
                    window.paint_quad(fill(bounds_of(rect(p.x.floor(), 0.0, 1.0, full.h)), line));
                }
                if let Some((h, v)) = measure {
                    window.paint_quad(fill(bounds_of(rect(h.x, h.y - 0.5, h.w, 1.0)), accent));
                    window.paint_quad(fill(bounds_of(rect(v.x - 0.5, v.y, 1.0, v.h)), accent));
                    for p in [pt(h.x, h.y), pt(h.right(), h.y)] {
                        window.paint_quad(fill(bounds_of(rect(p.x - 0.5, p.y - 5.0, 1.0, 10.0)), accent));
                    }
                    for p in [pt(v.x, v.y), pt(v.x, v.bottom())] {
                        window.paint_quad(fill(bounds_of(rect(p.x - 5.0, p.y - 0.5, 10.0, 1.0)), accent));
                    }
                }
            },
        )
        .absolute()
        .size_full();

        let mut root = div()
            .id("overlay")
            .debug_selector(|| "overlay".into())
            .key_context("Overlay")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .cursor(if self.mode == Mode::Window {
                CursorStyle::PointingHand
            } else {
                CursorStyle::Crosshair
            })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_key_down(cx.listener(Self::on_key))
            .child(painter);

        if let (Some(sel), true) = (self.selection, matches!(self.mode, Mode::Area | Mode::Text)) {
            let (w, h) = (sel.w.round(), sel.h.round());
            let label_y = if sel.y > 30.0 { sel.y - 28.0 } else { sel.bottom() + 6.0 };
            root = root.child(
                div()
                    .absolute()
                    .left(px(sel.x))
                    .top(px(label_y))
                    .child(pill(format!("{w} × {h}"), &t)),
            );
            for c in sel.corners() {
                root = root.child(
                    div()
                        .absolute()
                        .left(px(c.x - 4.0))
                        .top(px(c.y - 4.0))
                        .size(px(8.))
                        .rounded_full()
                        .bg(hex(0xFFFFFF, 1.0))
                        .border_1()
                        .border_color(t.accent),
                );
            }
            if self.drag.is_none() && self.mode == Mode::Area {
                let bar_y = if sel.bottom() + 50.0 < full.h {
                    sel.bottom() + 10.0
                } else {
                    (sel.y - 44.0).max(8.0)
                };
                let mut bar = div()
                    .id("selection-hints")
                    .debug_selector(|| "selection-hints".into())
                    .absolute()
                    .left(px(sel.x.min(full.w - 420.0).max(8.0)))
                    .top(px(bar_y))
                    .flex()
                    .gap(px(12.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(10.))
                    .bg(t.glass)
                    .border_1()
                    .border_color(t.border)
                    .text_color(t.text)
                    .font_family(crate::theme::UI_FONT)
                    .child(hint("⌘C", "Copy", &t))
                    .child(hint("⌘S", "Save", &t))
                    .child(hint("↩", "Edit", &t))
                    .child(hint("A", "Fit", &t))
                    .child(hint("Esc", "Cancel", &t));
                bar = bar.bg(if t.is_dark() {
                    hex(0x1C1C1E, 0.92)
                } else {
                    hex(0xF6F6F8, 0.94)
                });
                root = root.child(bar);
            }
        }

        if self.mode == Mode::Window
            && let Some(w) = self.hovered_window.and_then(|i| self.windows.get(i))
        {
            let name = if w.title.is_empty() {
                w.owner.clone()
            } else {
                format!("{} — {}", w.owner, w.title)
            };
            let f = w.frame;
            root = root.child(
                div()
                    .id("window-label")
                    .debug_selector(|| "window-label".into())
                    .absolute()
                    .left(px(f.x))
                    .top(px(f.y))
                    .w(px(f.w))
                    .h(px(f.h))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(pill(name, &t))
                    .child(pill("Click to capture · ⌘C copies · Esc cancels".into(), &t)),
            );
        }

        if let Some((h, v)) = measure
            && let Some(p) = cursor
        {
            root = root.child(
                div()
                    .absolute()
                    .left(px(p.x + 12.0))
                    .top(px(p.y + 12.0))
                    .child(pill(format!("{} × {}", h.w.round(), v.h.round()), &t)),
            );
        }

        if let (Some(p), Some(loupe)) = (cursor, self.loupe.clone())
            && matches!(self.mode, Mode::Area | Mode::Text | Mode::Color)
            && self.drag.is_none()
        {
            let lx = if p.x + 24.0 + LOUPE_SIZE < full.w {
                p.x + 24.0
            } else {
                p.x - 24.0 - LOUPE_SIZE
            };
            let ly = if p.y + 24.0 + LOUPE_SIZE + 26.0 < full.h {
                p.y + 24.0
            } else {
                p.y - 24.0 - LOUPE_SIZE - 26.0
            };
            let info = self
                .pixel_at(p)
                .map(|(x, y, c)| {
                    format!(
                        "{} , {}  {}",
                        (x as f32 / self.scale()).round(),
                        (y as f32 / self.scale()).round(),
                        c.to_hex()
                    )
                })
                .unwrap_or_default();
            let cell = LOUPE_SIZE / LOUPE_PIXELS as f32;
            root = root.child(
                div()
                    .absolute()
                    .left(px(lx))
                    .top(px(ly))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .relative()
                            .size(px(LOUPE_SIZE))
                            .rounded(px(12.))
                            .overflow_hidden()
                            .border_2()
                            .border_color(hex(0xFFFFFF, 0.95))
                            .child(img(loupe).size_full())
                            .child(
                                div()
                                    .absolute()
                                    .left(px(cell * (LOUPE_PIXELS / 2) as f32))
                                    .top(px(cell * (LOUPE_PIXELS / 2) as f32))
                                    .size(px(cell))
                                    .border_1()
                                    .border_color(t.accent),
                            ),
                    )
                    .child(pill(info, &t)),
            );
        }

        if self.mode == Mode::Color && self.cursor.is_none() || self.mode == Mode::Ruler && self.cursor.is_none() {
            let text = if self.mode == Mode::Color {
                "Click to copy a colour · ⇧-click for the darkest nearby"
            } else {
                "Point between two things to measure · click to copy"
            };
            root = root.child(
                div()
                    .absolute()
                    .top(px(40.))
                    .left(px(0.))
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(pill(text.into(), &t)),
            );
        }
        root.when(self.mode == Mode::Window && self.windows.is_empty(), |r| {
            r.child(
                div()
                    .absolute()
                    .top(px(40.))
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(pill("No windows to capture".into(), &t)),
            )
        })
    }
}

/// Opens the overlay on `display` and gives it keyboard focus.
pub fn open(mode: Mode, shot: Shot, display: DisplayInfo, window_list: Vec<WindowInfo>, cx: &mut App) {
    let mut spec = Spec::panel(display.frame.w, display.frame.h, 0);
    spec.glass = None;
    spec.kind = WindowKind::PopUp;
    spec.origin = Some(point(px(0.), px(0.)));
    spec.display = Some(display.clone());
    spec.focus = true;
    let frame = display.frame;
    let handle = windows::open(spec, cx, |_, cx| {
        cx.new(|cx| Overlay::new(mode, shot, display, window_list, cx))
    });
    if let Some(handle) = handle {
        let p = platform(cx);
        let _ = handle.update(cx, |overlay, window, cx| {
            // AppKit opens the overlay below the menu bar, which shifted the
            // frozen screen down by the bar's height and showed it twice.
            p.cover_display(window, frame);
            window.activate_window();
            overlay.focus.focus(window, cx);
        });
    }
}

/// Closes any open overlay. Returns whether one was open.
pub fn close_all(cx: &mut App) -> bool {
    let open = windows::find_all::<Overlay>(cx);
    let any = !open.is_empty();
    for h in open {
        windows::close(h, cx);
    }
    any
}
