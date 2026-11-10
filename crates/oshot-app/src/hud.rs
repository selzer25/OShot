//! The capture menu (both ⌘ keys, or ⌥⇧5): a compact glass bar in the style
//! of macOS's own ⌘⇧5 bar, with every capture mode on a number key.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, AppContext, Context, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, point,
    prelude::FluentBuilder, px,
};

use crate::commands::dispatch;
use crate::state::Command;
use crate::theme::theme;
use crate::ui::{glass_root, icon};
use crate::windows::{self, Spec};

pub struct Hud {
    focus: FocusHandle,
    pub delay: u32,
    /// The button under the pointer, named at the bar's right end.
    pub hovered: Option<usize>,
}

pub fn items(delay: u32) -> Vec<(&'static str, &'static str, SharedString, Command)> {
    vec![
        ("hud-area", "scan", "Area".into(), Command::CaptureArea),
        ("hud-window", "app-window", "Window".into(), Command::CaptureWindow),
        ("hud-full", "monitor", "Screen".into(), Command::CaptureFullScreen),
        ("hud-text", "scan-text", "Text".into(), Command::CaptureText),
        (
            "hud-timer",
            "timer",
            format!("{delay}s Timer").into(),
            Command::DelayedCapture(delay),
        ),
        ("hud-color", "pipette", "Color".into(), Command::ColorPicker),
        ("hud-ruler", "ruler", "Ruler".into(), Command::Ruler),
        (
            "hud-convert",
            "arrow-right-left",
            "Convert".into(),
            Command::ConvertImage,
        ),
    ]
}

impl Hud {
    pub fn new(cx: &mut Context<Self>) -> Hud {
        Hud {
            focus: cx.focus_handle(),
            delay: 3,
            hovered: None,
        }
    }

    fn cycle_delay(&mut self, cx: &mut Context<Self>) {
        self.delay = match self.delay {
            3 => 5,
            5 => 10,
            _ => 3,
        };
        cx.notify();
    }

    fn run(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        window.remove_window();
        cx.defer(move |cx| dispatch(command, cx));
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e.keystroke.key.as_str() {
            "escape" => window.remove_window(),
            "t" => self.cycle_delay(cx),
            k => {
                let n = k.parse::<usize>().ok().filter(|n| (1..=8).contains(n));
                if let Some(n) = n {
                    let command = items(self.delay)[n - 1].3.clone();
                    self.run(command, window, cx);
                }
            }
        }
    }
}

/// Buttons per group, in item order: capture (Area, Window, Screen), more
/// captures (Text, Timer), screen tools (Color, Ruler), then Convert.
const GROUPS: [usize; 4] = [3, 2, 2, 1];
pub const BUTTON_W: f32 = 44.;
pub const BUTTON_H: f32 = 40.;
const GAP: f32 = 2.;
const PAD: f32 = 6.;
const DIVIDER_W: f32 = 13.;
/// Room for the hovered button's name, at the bar's right end.
const CAPTION_W: f32 = 92.;
const FADE_MS: u64 = 180;

pub fn bar_size(count: usize) -> (f32, f32) {
    let n = count as f32;
    let groups = GROUPS.len() as f32;
    let w = PAD * 2. + n * BUTTON_W + (n - groups) * GAP + groups * DIVIDER_W + CAPTION_W;
    (w, BUTTON_H + PAD * 2.)
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn divider(t: &crate::theme::Theme) -> gpui::Div {
    div()
        .flex_none()
        .w(px(DIVIDER_W))
        .flex()
        .justify_center()
        .child(div().w(px(1.)).h(px(22.)).bg(t.border))
}

impl Render for Hud {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        let items = items(self.delay);
        let mut row = div().flex().flex_row().items_center().h_full();
        let mut next = 0;
        for size in GROUPS {
            let mut group = div().flex().flex_row().items_center().gap(px(GAP));
            for (i, (id, name, _, command)) in items.iter().cloned().enumerate().skip(next).take(size) {
                let timer = id == "hud-timer";
                let hovered = self.hovered == Some(i);
                group = group.child(
                    div()
                        .id(id)
                        .debug_selector(move || id.to_string())
                        .relative()
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .w(px(BUTTON_W))
                        .h(px(BUTTON_H))
                        .rounded(px(9.))
                        .cursor_pointer()
                        .when(hovered, |d| d.bg(t.hover))
                        .active(|s| s.bg(t.selected))
                        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                            if *over {
                                this.hovered = Some(i);
                            } else if this.hovered == Some(i) {
                                this.hovered = None;
                            }
                            cx.notify();
                        }))
                        .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                            if timer && e.modifiers().alt {
                                this.cycle_delay(cx);
                            } else {
                                this.run(command.clone(), window, cx);
                            }
                        }))
                        .child(
                            div()
                                .debug_selector(move || format!("{id}-icon"))
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(name).size(px(19.)).text_color(t.text)),
                        )
                        .child(
                            div()
                                .debug_selector(move || format!("{id}-key"))
                                .absolute()
                                .right(px(4.))
                                .bottom(px(2.))
                                .text_size(px(8.5))
                                .line_height(px(10.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if hovered { t.text_muted } else { t.text_faint })
                                .child(format!("{}", i + 1)),
                        ),
                );
            }
            next += size;
            row = row.child(group).child(divider(&t));
        }
        let caption = match self.hovered.and_then(|i| items.get(i)) {
            Some((_, _, label, _)) => div().text_color(t.text).child(label.clone()),
            None => div().text_color(t.text_faint).child("Esc to close"),
        };
        row = row.child(
            div()
                .id("hud-caption")
                .debug_selector(|| "hud-caption".into())
                .w(px(CAPTION_W))
                .whitespace_nowrap()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .child(caption),
        );
        let still = cx.reduce_motion();
        glass_root(&t)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .rounded(px(16.))
            .justify_center()
            .px(px(PAD))
            .child(
                // The whole bar fades in as one, rather than button by button.
                row.with_animation(
                    "hud-in",
                    Animation::new(Duration::from_millis(FADE_MS)),
                    move |el, p| {
                        let e = ease_out(p);
                        el.opacity(e).mt(px(if still { 0. } else { 4. * (1. - e) }))
                    },
                ),
            )
    }
}

pub fn toggle(cx: &mut App) {
    if let Some(h) = windows::find::<Hud>(cx) {
        windows::close(h, cx);
        return;
    }
    let Some(display) = windows::cursor_display(cx) else {
        return;
    };
    let (w, h) = bar_size(items(3).len());
    let mut spec = Spec::panel(w, h, 16);
    spec.focus = true;
    spec.origin = Some(point(px((display.frame.w - w) / 2.0), px(display.frame.h - h - 120.0)));
    spec.display = Some(display);
    if let Some(handle) = windows::open(spec, cx, |_, cx| cx.new(Hud::new)) {
        let _ = handle.update(cx, |hud, window, cx| {
            window.activate_window();
            hud.focus.focus(window, cx);
        });
    }
}
