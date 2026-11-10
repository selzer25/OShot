//! Small, hand-built widgets shared by every window.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, BoxShadow, ClickEvent, Div, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Svg, Window, canvas, div, point, prelude::FluentBuilder, px, svg,
};

use crate::theme::{Theme, UI_FONT, hex};

/// Height of the transparent titlebar: the traffic lights sit at 14 pt with
/// a 12 pt button, so content must start below this or it collides with them.
pub const TITLEBAR_H: f32 = 40.;

/// The gap a window leaves for its titlebar. `flex_none` is the point: a
/// shrinkable spacer collapses as soon as the column overflows, which slides
/// the first row of content up under the traffic lights.
pub fn titlebar_spacer() -> Div {
    div().h(px(TITLEBAR_H)).flex_none()
}

pub fn icon(name: &str) -> Svg {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(16.))
        .flex_none()
}

/// Root of a glass window: the tint over the native glass view, default
/// text style and a hairline edge.
pub fn glass_root(t: &Theme) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(t.glass)
        .text_color(t.text)
        .font_family(UI_FONT)
        .text_size(px(13.))
}

pub fn soft_shadow(t: &Theme) -> Vec<BoxShadow> {
    vec![BoxShadow {
        inset: false,
        color: t.shadow,
        offset: point(px(0.), px(8.)),
        blur_radius: px(28.),
        spread_radius: px(0.),
    }]
}

pub fn card(t: &Theme) -> Div {
    div().bg(t.panel).border_1().border_color(t.border).rounded(px(12.))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, t: &Theme, kind: Kind) -> Stateful<Div> {
    let (bg, fg, hover) = match kind {
        Kind::Primary => (t.accent, t.on_accent, hex(0x0071E3, 1.0)),
        Kind::Secondary => (t.panel, t.text, t.selected),
        Kind::Ghost => (hex(0, 0.0), t.text, t.hover),
        Kind::Danger => (t.panel, t.danger, t.hover),
    };
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(28.))
        .px(px(12.))
        .rounded(px(8.))
        .bg(bg)
        .text_color(fg)
        .font_weight(FontWeight::MEDIUM)
        .when(kind == Kind::Secondary || kind == Kind::Danger, |d| {
            d.border_1().border_color(t.border)
        })
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(label.into())
}

pub fn icon_button(id: impl Into<ElementId>, name: &str, t: &Theme, active: bool) -> Stateful<Div> {
    let fg = if active { t.on_accent } else { t.text_muted };
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(30.))
        .rounded(px(8.))
        .cursor_pointer()
        .when(active, |d| d.bg(t.accent))
        .when(!active, |d| d.hover(|s| s.bg(t.hover)))
        .child(icon(name).text_color(fg))
}

pub fn toggle(id: impl Into<ElementId>, on: bool, t: &Theme) -> Stateful<Div> {
    let track = if on {
        t.accent
    } else if t.is_dark() {
        hex(0xFFFFFF, 0.16)
    } else {
        hex(0x000000, 0.12)
    };
    div()
        .id(id)
        .flex_none()
        .w(px(36.))
        .h(px(20.))
        .rounded_full()
        .bg(track)
        .p(px(2.))
        .flex()
        .when(on, |d| d.justify_end())
        .cursor_pointer()
        .child(
            div()
                .size(px(16.))
                .rounded_full()
                .bg(hex(0xFFFFFF, 1.0))
                .shadow(vec![BoxShadow {
                    inset: false,
                    color: hex(0, 0.25),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                }]),
        )
}

/// Segmented control. `on_select` gets the chosen index.
pub fn segmented(
    id: &'static str,
    labels: &[SharedString],
    selected: usize,
    t: &Theme,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = Rc::new(on_select);
    let track = if t.is_dark() {
        hex(0xFFFFFF, 0.06)
    } else {
        hex(0x000000, 0.06)
    };
    div()
        .flex()
        .flex_none()
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(8.))
        .bg(track)
        .children(labels.iter().enumerate().map(|(i, label)| {
            let on_select = on_select.clone();
            let active = i == selected;
            div()
                .id(ElementId::NamedInteger(id.into(), i as u64))
                .debug_selector(move || format!("{id}-{i}"))
                .px(px(10.))
                .h(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .text_color(if active { t.text } else { t.text_muted })
                .when(active, |d| {
                    d.bg(t.selected).shadow(vec![BoxShadow {
                        inset: false,
                        color: hex(0, 0.12),
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(3.),
                        spread_radius: px(0.),
                    }])
                })
                .when(!active, |d| d.hover(|s| s.bg(t.hover)))
                .on_click(move |_, window, cx| on_select(i, window, cx))
                .child(label.clone())
        }))
}

/// Horizontal slider for 0..1. Records its own bounds while painting so
/// clicks and drags map to a value.
pub fn slider(
    id: &'static str,
    value: f32,
    t: &Theme,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));
    let on_change = Rc::new(on_change);
    let value = value.clamp(0.0, 1.0);
    let track = if t.is_dark() {
        hex(0xFFFFFF, 0.14)
    } else {
        hex(0x000000, 0.10)
    };
    let pick = {
        let bounds = bounds.clone();
        move |x: Pixels| {
            let b = bounds.get();
            if b.size.width <= px(0.) {
                0.0
            } else {
                ((x - b.left()) / b.size.width).clamp(0.0, 1.0)
            }
        }
    };
    let down = {
        let (pick, on_change) = (pick.clone(), on_change.clone());
        move |e: &MouseDownEvent, window: &mut Window, cx: &mut App| on_change(pick(e.position.x), window, cx)
    };
    let drag = move |e: &MouseMoveEvent, window: &mut Window, cx: &mut App| {
        if e.pressed_button == Some(MouseButton::Left) {
            on_change(pick(e.position.x), window, cx)
        }
    };
    let recorder = bounds.clone();
    div()
        .id(id)
        .debug_selector(move || id.to_string())
        .relative()
        .w_full()
        .flex_none()
        .min_w(px(80.))
        .h(px(20.))
        .flex()
        .items_center()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, down)
        .on_mouse_move(drag)
        .child(
            canvas(move |b, _, _| recorder.set(b), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .w_full()
                .h(px(4.))
                .rounded_full()
                .bg(track)
                .child(div().h_full().rounded_full().bg(t.accent).w(gpui::relative(value))),
        )
        .child(
            div()
                .absolute()
                .left(gpui::relative(value))
                .ml(px(-8.))
                .size(px(16.))
                .rounded_full()
                .bg(hex(0xFFFFFF, 1.0))
                .border_1()
                .border_color(hex(0, 0.12))
                .shadow(vec![BoxShadow {
                    inset: false,
                    color: hex(0, 0.25),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(3.),
                    spread_radius: px(0.),
                }]),
        )
}

pub fn kbd(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .h(px(20.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .bg(t.panel)
        .border_1()
        .border_color(t.border)
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_muted)
        .child(text.into())
}

pub fn section_title(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.text_faint)
        .child(text.into())
}

/// A settings row: title and description on the left, control on the right.
pub fn row(title: impl Into<SharedString>, description: Option<&str>, control: impl IntoElement, t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .px(px(14.))
        .py(px(10.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flex_1()
                // Lets long descriptions wrap instead of pushing the control
                // past the window edge.
                .min_w(px(0.))
                .child(div().font_weight(FontWeight::MEDIUM).child(title.into()))
                .when_some(description, |d, desc| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child(desc.to_string()),
                    )
                }),
        )
        .child(div().flex_none().child(control))
}

pub fn divider(t: &Theme) -> Div {
    div().h(px(1.)).mx(px(14.)).bg(t.border)
}

pub fn swatch(id: impl Into<ElementId>, color: Hsla, selected: bool, t: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(22.))
        .rounded_full()
        .p(px(2.))
        .border_2()
        .border_color(if selected { t.accent } else { hex(0, 0.0) })
        .cursor_pointer()
        .child(
            div()
                .size_full()
                .rounded_full()
                .bg(color)
                .border_1()
                .border_color(hex(0, 0.15)),
        )
}

/// Hint chip like "⌘C Copy".
pub fn hint(keys: &str, label: &str, t: &Theme) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(kbd(keys.to_string(), t))
        .child(
            div()
                .text_size(px(12.))
                .text_color(t.text_muted)
                .child(label.to_string()),
        )
        .into_any_element()
}

/// Click handler type used by builders that wire many buttons.
pub type OnClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A small dark tooltip.
pub struct Tooltip(pub SharedString);

impl gpui::Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(hex(0x1C1C1E, 0.94))
            .text_color(hex(0xFFFFFF, 1.0))
            .font_family(UI_FONT)
            .text_size(px(12.))
            .child(self.0.clone())
    }
}

pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let text = text.into();
    move |_, cx| gpui::AppContext::new(cx, |_| Tooltip(text.clone())).into()
}

/// Single-line text field. Emits `TextChanged` on every edit.
pub struct TextInput {
    pub text: String,
    pub placeholder: SharedString,
    focus: gpui::FocusHandle,
}

pub struct TextChanged(pub String);

impl gpui::EventEmitter<TextChanged> for TextInput {}

impl TextInput {
    pub fn new(
        text: impl Into<String>,
        placeholder: impl Into<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) -> TextInput {
        TextInput {
            text: text.into(),
            placeholder: placeholder.into(),
            focus: cx.focus_handle(),
        }
    }

    pub fn set_text(&mut self, text: String, cx: &mut gpui::Context<Self>) {
        self.text = text;
        cx.emit(TextChanged(self.text.clone()));
        cx.notify();
    }

    fn on_key(&mut self, e: &gpui::KeyDownEvent, _: &mut Window, cx: &mut gpui::Context<Self>) {
        let k = &e.keystroke;
        let mut text = self.text.clone();
        match (k.key.as_str(), k.modifiers.platform) {
            ("backspace", true) => text.clear(),
            ("backspace", false) => {
                text.pop();
            }
            ("a", true) => text.clear(),
            ("v", true) => {
                if let Some(s) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    text.push_str(s.lines().next().unwrap_or(""));
                }
            }
            (_, false) if !k.modifiers.control => match &k.key_char {
                Some(c) if !c.chars().any(|c| c.is_control()) => text.push_str(c),
                _ => return,
            },
            _ => return,
        }
        cx.stop_propagation();
        self.set_text(text, cx);
    }
}

impl gpui::Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let t = crate::theme::theme(window, cx);
        let focused = self.focus.is_focused(window);
        let empty = self.text.is_empty();
        div()
            .id("text-input")
            .debug_selector(|| "text-input".into())
            .track_focus(&self.focus)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.focus.focus(window, cx)),
            )
            .on_key_down(cx.listener(Self::on_key))
            .flex()
            .items_center()
            .h(px(28.))
            .px(px(8.))
            .min_w(px(220.))
            .rounded(px(7.))
            .bg(t.panel)
            .border_1()
            .border_color(if focused { t.accent } else { t.border })
            .cursor(gpui::CursorStyle::IBeam)
            .child(
                div()
                    .text_color(if empty { t.text_faint } else { t.text })
                    .child(if empty {
                        self.placeholder.to_string()
                    } else {
                        self.text.clone()
                    }),
            )
            .when(focused, |d| d.child(div().w(px(1.5)).h(px(15.)).bg(t.accent)))
    }
}

/// Like `cx.listener`, for widgets whose callbacks take a value (a slider
/// position, a segment index) instead of an event reference.
pub fn listen<V: 'static, A: Copy + 'static>(
    cx: &gpui::Context<V>,
    f: impl Fn(&mut V, A, &mut Window, &mut gpui::Context<V>) + 'static,
) -> impl Fn(A, &mut Window, &mut App) + 'static {
    let listener = cx.listener(move |this, a: &A, window, cx| f(this, *a, window, cx));
    move |a, window, cx| listener(&a, window, cx)
}
