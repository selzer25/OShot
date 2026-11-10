//! Small glass confirmations ("Copied", "Saved") near the bottom of the screen.

use std::time::Duration;

use gpui::{
    App, AppContext, Context, FontWeight, IntoElement, ParentElement, Render, Styled, Window, WindowKind, div, point,
    px,
};

use crate::state::Notice;
use crate::theme::theme;
use crate::ui::{glass_root, icon};
use crate::windows::{self, Spec};

pub struct Toast {
    pub notice: Notice,
    generation: u64,
}

impl Toast {
    pub fn new(notice: Notice) -> Toast {
        Toast { notice, generation: 0 }
    }

    pub fn message(&self) -> String {
        match &self.notice {
            Notice::Copied(s) | Notice::Info(s) | Notice::Error(s) => s.clone(),
            Notice::Saved(p) => format!(
                "Saved {}",
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            ),
        }
    }
}

impl Render for Toast {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        let (name, color) = match self.notice {
            Notice::Copied(_) => ("copy", t.accent),
            Notice::Saved(_) => ("check", t.success),
            Notice::Error(_) => ("info", t.danger),
            Notice::Info(_) => ("sparkles", t.accent),
        };
        glass_root(&t).rounded(px(22.)).items_center().justify_center().child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(16.))
                .child(icon(name).text_color(color))
                .child(div().font_weight(FontWeight::MEDIUM).truncate().child(self.message())),
        )
    }
}

const SHOW_FOR: Duration = Duration::from_millis(1800);

pub fn hide(cx: &mut App) {
    if let Some(handle) = windows::find::<Toast>(cx) {
        windows::close(handle, cx);
    }
}

pub fn show(notice: Notice, cx: &mut App) {
    let generation = if let Some(handle) = windows::find::<Toast>(cx) {
        handle
            .update(cx, |toast, _, cx| {
                toast.notice = notice.clone();
                toast.generation += 1;
                cx.notify();
                toast.generation
            })
            .unwrap_or(0)
    } else {
        let Some(display) = windows::cursor_display(cx) else {
            return;
        };
        let (w, h) = (340.0, 44.0);
        let mut spec = Spec::panel(w, h, 22);
        spec.kind = WindowKind::PopUp;
        spec.origin = Some(point(px((display.frame.w - w) / 2.0), px(display.frame.h - h - 96.0)));
        spec.display = Some(display);
        let notice = notice.clone();
        if windows::open(spec, cx, |_, cx| cx.new(|_| Toast { notice, generation: 0 })).is_none() {
            return;
        }
        0
    };
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SHOW_FOR).await;
        cx.update(|cx| {
            if let Some(handle) = windows::find::<Toast>(cx) {
                let same = handle.read_with(cx, |t, _| t.generation == generation).unwrap_or(false);
                if same {
                    windows::close(handle, cx);
                }
            }
        });
    })
    .detach();
}
