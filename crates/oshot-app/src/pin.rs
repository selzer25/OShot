//! Pinned screenshots: borderless, always-on-top image windows.

use gpui::{
    App, AppContext, Context, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ObjectFit,
    ParentElement, Render, ScrollEvent, Styled, StyledImage, Window, WindowKind, div, img, px, size,
};

use crate::state::Shot;
use crate::windows::{self, Spec};

pub struct Pin {
    pub shot: Shot,
    pub opacity: f32,
    focus: FocusHandle,
}

impl Pin {
    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = e.keystroke.key.as_str();
        match key {
            "escape" => window.remove_window(),
            "0" => self.opacity = 1.0,
            d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => {
                self.opacity = (d.as_bytes()[0] - b'0') as f32 / 10.0;
            }
            _ => return,
        }
        cx.notify();
    }

    fn on_scroll(&mut self, e: &ScrollEvent, window: &mut Window, _: &mut Context<Self>) {
        let dy = f32::from(e.delta.pixel_delta(px(16.)).y);
        let b = window.bounds();
        let factor = (1.0 + dy / 300.0).clamp(0.8, 1.25);
        let w = (f32::from(b.size.width) * factor).clamp(80.0, 4000.0);
        let aspect = self.shot.height as f32 / self.shot.width.max(1) as f32;
        window.resize(size(px(w), px(w * aspect)));
    }
}

impl Render for Pin {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("pin")
            .debug_selector(|| "pin".into())
            .track_focus(&self.focus)
            .size_full()
            .rounded(px(8.))
            .overflow_hidden()
            .opacity(self.opacity)
            .on_key_down(cx.listener(Self::on_key))
            .on_scroll(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, |e, window, _| {
                if e.click_count >= 2 {
                    window.remove_window();
                } else {
                    window.start_window_move();
                }
            })
            .child(img(self.shot.image.clone()).size_full().object_fit(ObjectFit::Fill))
    }
}

pub fn open(shot: Shot, cx: &mut App) {
    let Some(display) = windows::cursor_display(cx) else {
        return;
    };
    let mut w = shot.width as f32 / shot.scale.max(1.0);
    let mut h = shot.height as f32 / shot.scale.max(1.0);
    let max_w = display.frame.w * 0.6;
    let max_h = display.frame.h * 0.6;
    let fit = (max_w / w).min(max_h / h).min(1.0);
    w *= fit;
    h *= fit;
    let mut spec = Spec::panel(w, h, 8);
    spec.glass = None;
    spec.kind = WindowKind::Floating;
    spec.focus = true;
    spec.display = Some(display);
    let handle = windows::open(spec, cx, |_, cx| {
        cx.new(|cx| Pin {
            shot,
            opacity: 1.0,
            focus: cx.focus_handle(),
        })
    });
    if let Some(handle) = handle {
        let _ = handle.update(cx, |pin, window, cx| pin.focus.focus(window, cx));
    }
}
