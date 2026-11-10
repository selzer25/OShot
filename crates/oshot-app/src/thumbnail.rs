//! Floating preview in a screen corner after a capture.

use std::time::Duration;

use gpui::{
    App, AppContext, Context, InteractiveElement, IntoElement, ObjectFit, ParentElement, Render,
    StatefulInteractiveElement, Styled, StyledImage, Window, div, img, point, px,
};
use oshot_core::settings::Corner;

use crate::state::{Shot, state};
use crate::theme::theme;
use crate::ui::{glass_root, icon_button};
use crate::windows::{self, Spec};
use crate::{editor, output, pin};

pub struct Thumbnail {
    pub shot: Shot,
    pub hovered: bool,
    pub generation: u64,
}

pub const WIDTH: f32 = 260.0;

impl Thumbnail {
    fn close(&self, window: &mut Window) {
        window.remove_window();
    }

    pub fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        output::copy(&self.shot, true, cx);
        self.close(window);
    }

    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        output::save(&self.shot, cx);
        self.close(window);
    }

    pub fn edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shot = self.shot.clone();
        cx.defer(move |cx| editor::open_shot(shot, cx));
        self.close(window);
    }

    pub fn pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shot = self.shot.clone();
        cx.defer(move |cx| pin::open(shot, cx));
        self.close(window);
    }
}

impl Render for Thumbnail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        glass_root(&t)
            .id("thumbnail")
            .rounded(px(16.))
            .p(px(8.))
            .gap(px(8.))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.hovered = *hovered;
                if !*hovered {
                    this.generation += 1;
                    schedule_close(this.generation, cx);
                }
            }))
            .child(
                div()
                    .id("thumbnail-image")
                    .debug_selector(|| "thumbnail-image".into())
                    .flex_1()
                    .rounded(px(10.))
                    .overflow_hidden()
                    .bg(t.canvas)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| this.edit(window, cx)))
                    .child(img(self.shot.image.clone()).size_full().object_fit(ObjectFit::Contain)),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(
                                icon_button("thumb-copy", "copy", &t, false)
                                    .debug_selector(|| "thumb-copy".into())
                                    .on_click(cx.listener(|this, _, w, cx| this.copy(w, cx))),
                            )
                            .child(
                                icon_button("thumb-save", "download", &t, false)
                                    .debug_selector(|| "thumb-save".into())
                                    .on_click(cx.listener(|this, _, w, cx| this.save(w, cx))),
                            )
                            .child(
                                icon_button("thumb-edit", "pen-tool", &t, false)
                                    .debug_selector(|| "thumb-edit".into())
                                    .on_click(cx.listener(|this, _, w, cx| this.edit(w, cx))),
                            )
                            .child(
                                icon_button("thumb-pin", "pin", &t, false)
                                    .debug_selector(|| "thumb-pin".into())
                                    .on_click(cx.listener(|this, _, w, cx| this.pin(w, cx))),
                            ),
                    )
                    .child(
                        icon_button("thumb-close", "x", &t, false)
                            .debug_selector(|| "thumb-close".into())
                            .on_click(cx.listener(|this, _, w, _| this.close(w))),
                    ),
            )
    }
}

fn schedule_close(generation: u64, cx: &mut Context<Thumbnail>) {
    let seconds = state(cx).settings.thumbnail_seconds;
    if seconds == 0 {
        return;
    }
    cx.spawn(async move |this, cx| {
        cx.background_executor()
            .timer(Duration::from_secs(seconds as u64))
            .await;
        // Decide inside the entity, close outside it: a window cannot be
        // removed while its root view is being updated.
        let expired = this
            .update(cx, |this, _| !this.hovered && this.generation == generation)
            .unwrap_or(false);
        if expired {
            cx.update(|cx| {
                if let Some(h) = windows::find::<Thumbnail>(cx) {
                    windows::close(h, cx);
                }
            });
        }
    })
    .detach();
}

pub fn show(shot: Shot, cx: &mut App) {
    for h in windows::find_all::<Thumbnail>(cx) {
        windows::close(h, cx);
    }
    let Some(display) = windows::cursor_display(cx) else {
        return;
    };
    let aspect = shot.height as f32 / shot.width.max(1) as f32;
    let image_h = (WIDTH - 16.0) * aspect.clamp(0.3, 1.2);
    let h = image_h + 16.0 + 38.0;
    let margin = 20.0;
    let (x, y) = match state(cx).settings.thumbnail_corner {
        Corner::TopLeft => (margin, margin + 30.0),
        Corner::TopRight => (display.frame.w - WIDTH - margin, margin + 30.0),
        Corner::BottomLeft => (margin, display.frame.h - h - margin),
        Corner::BottomRight => (display.frame.w - WIDTH - margin, display.frame.h - h - margin),
    };
    let mut spec = Spec::panel(WIDTH, h, 16);
    spec.origin = Some(point(px(x), px(y)));
    spec.display = Some(display);
    let handle = windows::open(spec, cx, |_, cx| {
        cx.new(|_| Thumbnail {
            shot,
            hovered: false,
            generation: 0,
        })
    });
    if let Some(handle) = handle {
        let _ = handle.update(cx, |_, _, cx| schedule_close(0, cx));
    }
}
