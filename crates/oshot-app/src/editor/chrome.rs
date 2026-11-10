//! Editor chrome: toolbar, inspector, side panels and status bar.

use gpui::{
    AnyElement, Context, CursorStyle, ExternalPaths, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, canvas, div,
    prelude::FluentBuilder, px,
};
use oshot_core::color::PALETTE;
use oshot_core::format::ImageFormat;
use oshot_core::model::{ArrowStyle, BACKDROP_PRESETS, Shape, Tool};

use super::canvas::{compute_view, paint};
use super::export::{self, Target};
use super::{Editor, Panel};
use crate::state::update_settings;
use crate::theme::{Theme, core_color, theme};
use crate::ui::{
    Kind, button, glass_root, icon_button, listen, section_title, segmented, slider, swatch, toggle, tooltip,
};

pub fn tool_icon(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "mouse-pointer-2",
        Tool::Arrow => "move-up-right",
        Tool::Line => "minus",
        Tool::Rect => "square",
        Tool::Oval => "circle",
        Tool::Freehand => "pencil",
        Tool::Highlighter => "highlighter",
        Tool::Text => "type",
        Tool::Counter => "list-ordered",
        Tool::Pixelate => "grid-3x3",
        Tool::Blur => "droplet",
        Tool::Erase => "eraser",
        Tool::Spotlight => "sun",
        Tool::Magnifier => "zoom-in",
        Tool::Ruler => "ruler",
        Tool::ColorPicker => "pipette",
    }
}

pub fn tool_hint(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "Drag to select · ↩ crop · B/P/E blur, pixelate or erase it · ⌘-click a flat area · ⇧A fit",
        Tool::Arrow => "Drag to draw an arrow · ⇧ snaps to 45°",
        Tool::Line => "Drag to draw a line · ⇧ snaps to 45°",
        Tool::Rect => "Drag to draw a rectangle · ⇧ for a square",
        Tool::Oval => "Drag to draw an oval · ⇧ for a circle",
        Tool::Freehand => "Draw freely",
        Tool::Highlighter => "Drag across text to highlight it",
        Tool::Text => "Click to type · ⇧↩ new line · ↩ or Esc to finish",
        Tool::Counter => "Click to drop numbered steps",
        Tool::Pixelate => "Drag over anything private",
        Tool::Blur => "Drag to blur an area",
        Tool::Erase => "Drag to remove something, filling from its surroundings",
        Tool::Spotlight => "Drag to highlight an area and dim the rest",
        Tool::Magnifier => "Drag from a detail to where its zoomed callout goes",
        Tool::Ruler => "Hover to measure gaps · drag to imprint a measurement",
        Tool::ColorPicker => "Click to copy a colour · ⇧-click for the darkest nearby · Tab anywhere",
    }
}

const GROUPS: [&[Tool]; 4] = [
    &[Tool::Select],
    &[
        Tool::Arrow,
        Tool::Line,
        Tool::Rect,
        Tool::Oval,
        Tool::Freehand,
        Tool::Highlighter,
        Tool::Text,
        Tool::Counter,
    ],
    &[
        Tool::Pixelate,
        Tool::Blur,
        Tool::Erase,
        Tool::Spotlight,
        Tool::Magnifier,
    ],
    &[Tool::Ruler, Tool::ColorPicker],
];

fn vsep(t: &Theme) -> gpui::Div {
    div().w(px(1.)).h(px(20.)).mx(px(4.)).bg(t.border)
}

impl Editor {
    fn toolbar(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut tools = div().flex().items_center().gap(px(2.));
        for (gi, group) in GROUPS.iter().enumerate() {
            if gi > 0 {
                tools = tools.child(vsep(t));
            }
            for tool in group.iter().copied() {
                let label: SharedString = format!("{}  {}", tool.label(), tool.key().to_uppercase()).into();
                let id: SharedString = format!("tool-{}", tool.key()).into();
                let sel = id.to_string();
                tools = tools.child(
                    icon_button(id, tool_icon(tool), t, self.tool == tool)
                        .debug_selector(move || sel.clone())
                        .tooltip(tooltip(label))
                        .on_click(cx.listener(move |this, _, _, cx| this.set_tool(tool, cx))),
                );
            }
        }
        let action = |id: &'static str, name: &'static str, tip: &'static str, active: bool| {
            icon_button(id, name, t, active)
                .debug_selector(move || id.to_string())
                .tooltip(tooltip(tip))
        };
        // Only the empty areas move the window, so buttons never start a drag.
        let drag_area = |id: &'static str| {
            div().id(id).h_full().on_mouse_down(MouseButton::Left, |e, window, _| {
                if e.click_count == 1 {
                    window.start_window_move();
                }
            })
        };
        div()
            .id("editor-toolbar")
            .flex()
            .flex_none()
            .items_center()
            .h(px(52.))
            .pr(px(12.))
            .border_b_1()
            .border_color(t.border)
            .child(drag_area("drag-left").w(px(84.)).flex_none())
            .child(tools)
            .child(drag_area("drag-mid").flex_1().min_w(px(8.)))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        action("undo", "undo-2", "Undo  ⌘Z", false)
                            .when(!self.doc.can_undo(), |d| d.opacity(0.4))
                            .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
                    )
                    .child(
                        action("redo", "redo-2", "Redo  ⇧⌘Z", false)
                            .when(!self.doc.can_redo(), |d| d.opacity(0.4))
                            .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
                    ),
            )
            .into_any_element()
    }

    fn actions(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let (w, h) = self.doc.export_size();
        let out = (
            ((w as f32) * self.export_scale).round(),
            ((h as f32) * self.export_scale).round(),
        );
        let action = |id: &'static str, name: &'static str, tip: &'static str, active: bool| {
            icon_button(id, name, t, active)
                .debug_selector(move || id.to_string())
                .tooltip(tooltip(tip))
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .child(
                action("backdrop", "image", "Backdrop", self.panel == Some(Panel::Backdrop))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_panel(Panel::Backdrop, cx))),
            )
            .child(
                action(
                    "extend-canvas",
                    "maximize-2",
                    "Extend canvas to fit drawings outside the image",
                    self.doc.expand_canvas,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.doc.expand_canvas = !this.doc.expand_canvas;
                    cx.notify();
                })),
            )
            .child(
                div()
                    .id("resize")
                    .debug_selector(|| "resize".into())
                    .px(px(8.))
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .rounded(px(8.))
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover))
                    .when(self.panel == Some(Panel::Resize), |d| d.bg(t.selected))
                    .tooltip(tooltip("Image size"))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_panel(Panel::Resize, cx)))
                    .child(format!("{} × {}", out.0, out.1)),
            )
            .child(
                action("ocr", "scan-text", "Copy text (OCR)", false)
                    .on_click(cx.listener(|this, _, _, cx| export::ocr(this, cx))),
            )
            .child(
                action("redact", "shield", "Redact emails, keys and card numbers", false)
                    .on_click(cx.listener(|this, _, _, cx| export::redact(this, cx))),
            )
            .child(
                action("pin", "pin", "Pin to screen", false)
                    .on_click(cx.listener(|this, _, _, cx| export::run(this, Target::Pin, cx))),
            )
            .child(
                action("export", "upload", "Export…", self.panel == Some(Panel::Export))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_panel(Panel::Export, cx))),
            )
            .child(vsep(t))
            .child(
                button("save", "Save", t, Kind::Secondary)
                    .debug_selector(|| "save".into())
                    .on_click(cx.listener(|this, _, _, cx| export::run(this, Target::SaveFolder, cx))),
            )
            .child(div().w(px(6.)))
            .child(
                button("copy", "Copy", t, Kind::Primary)
                    .debug_selector(|| "copy".into())
                    .on_click(cx.listener(|this, _, _, cx| export::run(this, Target::Clipboard, cx))),
            )
    }

    fn toggle_panel(&mut self, panel: Panel, cx: &mut Context<Self>) {
        self.panel = if self.panel == Some(panel) { None } else { Some(panel) };
        cx.notify();
    }

    fn inspector(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let st = self.active_style();
        let target_tool = self
            .selected
            .and_then(|id| self.doc.get(id))
            .map(|o| o.shape.tool())
            .unwrap_or(self.tool);
        let s = self.shot.scale.max(1.0);
        let mut bar = div()
            .id("inspector")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .h(px(44.))
            .px(px(14.))
            .overflow_hidden()
            .border_b_1()
            .border_color(t.border);
        let label = |text: &str| {
            div()
                .text_size(px(12.))
                .text_color(t.text_muted)
                .child(text.to_string())
        };

        let overlay = self
            .selected
            .and_then(|id| self.doc.get(id))
            .and_then(|o| match o.shape {
                Shape::Image { opacity, .. } => Some(opacity),
                _ => None,
            });
        if let Some(opacity) = overlay {
            return bar
                .child(label("Overlay opacity"))
                .child(div().w(px(180.)).child(slider(
                    "overlay-opacity",
                    opacity,
                    t,
                    listen(cx, |this, v: f32, _, cx| this.set_overlay_opacity(0.05 + v * 0.95, cx)),
                )))
                .child(
                    button("gif", "Before / After GIF", t, Kind::Secondary)
                        .debug_selector(|| "gif".into())
                        .on_click(cx.listener(|this, _, _, cx| export::run(this, Target::BeforeAfterGif, cx))),
                )
                .child(
                    button("delete-overlay", "Remove", t, Kind::Danger)
                        .debug_selector(|| "delete-overlay".into())
                        .on_click(cx.listener(|this, _, _, cx| this.delete_selected(cx))),
                )
                .into_any_element();
        }

        if self.tool == Tool::Select && self.selection.is_some() && self.selected.is_none() {
            let sel = self.selection.unwrap_or_default();
            return bar
                .child(label(&format!(
                    "Selection {} × {}",
                    (sel.w / s).round(),
                    (sel.h / s).round()
                )))
                .child(
                    button("sel-crop", "Crop  ↩", t, Kind::Primary)
                        .debug_selector(|| "sel-crop".into())
                        .on_click(cx.listener(|this, _, _, cx| this.crop_to_selection(cx))),
                )
                .child(
                    button("sel-blur", "Blur", t, Kind::Secondary)
                        .debug_selector(|| "sel-blur".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.effect_on_selection(Tool::Blur, cx);
                        })),
                )
                .child(
                    button("sel-pixelate", "Pixelate", t, Kind::Secondary)
                        .debug_selector(|| "sel-pixelate".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.effect_on_selection(Tool::Pixelate, cx);
                        })),
                )
                .child(
                    button("sel-erase", "Erase", t, Kind::Secondary)
                        .debug_selector(|| "sel-erase".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.effect_on_selection(Tool::Erase, cx);
                        })),
                )
                .child(
                    button("sel-fit", "Fit  ⇧A", t, Kind::Ghost)
                        .debug_selector(|| "sel-fit".into())
                        .on_click(cx.listener(|this, _, _, cx| this.auto_adjust_selection(cx))),
                )
                .child(
                    button("sel-color", "Average colour", t, Kind::Ghost)
                        .debug_selector(|| "sel-color".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.copy_average_color(cx);
                        })),
                )
                .into_any_element();
        }

        if self.doc.crop.is_some() && self.tool == Tool::Select && self.selected.is_none() {
            bar = bar.child(
                button("reset-crop", "Reset crop", t, Kind::Ghost)
                    .debug_selector(|| "reset-crop".into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.doc.reset_crop();
                        this.zoom = None;
                        cx.notify();
                    })),
            );
        }

        if target_tool.uses_color() {
            bar = bar.child(
                div()
                    .flex()
                    .gap(px(4.))
                    .children(PALETTE.iter().enumerate().map(|(i, c)| {
                        let c = *c;
                        swatch(("color", i), core_color(c), st.color == c, t)
                            .debug_selector(move || format!("color-{i}"))
                            .on_click(cx.listener(move |this, _, _, cx| this.edit_style(|s| s.color = c, cx)))
                    })),
            );
            bar = bar.child(vsep(t));
        }

        let width_tools = [
            Tool::Arrow,
            Tool::Line,
            Tool::Rect,
            Tool::Oval,
            Tool::Freehand,
            Tool::Highlighter,
            Tool::Ruler,
            Tool::Magnifier,
        ];
        if width_tools.contains(&target_tool) {
            let v = ((st.width / s - 1.0) / 29.0).clamp(0.0, 1.0);
            bar = bar.child(label("Width")).child(div().w(px(110.)).child(slider(
                "width",
                v,
                t,
                listen(cx, move |this, v: f32, _, cx| {
                    this.edit_style(|st| st.width = (1.0 + v * 29.0) * s, cx)
                }),
            )));
        }
        match target_tool {
            Tool::Arrow => {
                let idx = match st.arrow {
                    ArrowStyle::Tapered => 0,
                    ArrowStyle::Line => 1,
                    ArrowStyle::Double => 2,
                };
                bar = bar
                    .child(segmented(
                        "arrow-style",
                        &["Tapered".into(), "Line".into(), "Double".into()],
                        idx,
                        t,
                        listen(cx, |this, i: usize, _, cx| {
                            let a = [ArrowStyle::Tapered, ArrowStyle::Line, ArrowStyle::Double][i];
                            this.edit_style(|s| s.arrow = a, cx)
                        }),
                    ))
                    .child(label("Numbered"))
                    .child(
                        toggle("numbered", st.numbered, t)
                            .debug_selector(|| "numbered".into())
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_numbered(cx))),
                    );
            }
            Tool::Rect | Tool::Oval => {
                bar = bar.child(label("Fill")).child(
                    toggle("fill", st.fill, t)
                        .debug_selector(|| "fill".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            let on = !this.active_style().fill;
                            this.edit_style(|s| s.fill = on, cx)
                        })),
                );
            }
            Tool::Text | Tool::Counter => {
                let v = ((st.text_size / s - 10.0) / 86.0).clamp(0.0, 1.0);
                bar = bar.child(label("Size")).child(div().w(px(110.)).child(slider(
                    "text-size",
                    v,
                    t,
                    listen(cx, move |this, v: f32, _, cx| {
                        this.edit_style(|st| st.text_size = (10.0 + v * 86.0) * s, cx)
                    }),
                )));
                if target_tool == Tool::Text {
                    bar = bar.child(label("Bubble")).child(
                        toggle("bubble", st.text_background, t)
                            .debug_selector(|| "bubble".into())
                            .on_click(cx.listener(|this, _, _, cx| {
                                let on = !this.active_style().text_background;
                                this.edit_style(|s| s.text_background = on, cx)
                            })),
                    );
                }
            }
            Tool::Pixelate | Tool::Blur => {
                let (min, span) = if target_tool == Tool::Pixelate {
                    (4.0, 36.0)
                } else {
                    (2.0, 28.0)
                };
                let v = ((st.strength / s - min) / span).clamp(0.0, 1.0);
                bar = bar
                    .child(label(if target_tool == Tool::Pixelate {
                        "Block size"
                    } else {
                        "Radius"
                    }))
                    .child(div().w(px(130.)).child(slider(
                        "strength",
                        v,
                        t,
                        listen(cx, move |this, v: f32, _, cx| {
                            this.edit_style(|st| st.strength = (min + v * span) * s, cx)
                        }),
                    )));
            }
            Tool::Spotlight => {
                bar = bar
                    .child(label("Dim"))
                    .child(div().w(px(120.)).child(slider(
                        "strength",
                        (st.strength - 0.1) / 0.8,
                        t,
                        listen(cx, |this, v: f32, _, cx| {
                            this.edit_style(|s| s.strength = 0.1 + v * 0.8, cx)
                        }),
                    )))
                    .child(segmented(
                        "spot-shape",
                        &["Rectangle".into(), "Ellipse".into()],
                        st.ellipse as usize,
                        t,
                        listen(cx, |this, i: usize, _, cx| this.edit_style(|s| s.ellipse = i == 1, cx)),
                    ));
            }
            Tool::Magnifier => {
                let zooms = [2.0f32, 3.0, 4.0, 6.0];
                let idx = zooms.iter().position(|z| (*z - st.strength).abs() < 0.1).unwrap_or(1);
                bar = bar.child(label("Zoom")).child(segmented(
                    "mag-zoom",
                    &["2×".into(), "3×".into(), "4×".into(), "6×".into()],
                    idx,
                    t,
                    listen(cx, move |this, i: usize, _, cx| {
                        this.edit_style(|s| s.strength = zooms[i], cx)
                    }),
                ));
            }
            Tool::ColorPicker => {
                let c = self.hover_color();
                bar = bar
                    .child(
                        div()
                            .size(px(22.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(t.border)
                            .when_some(c, |d, c| d.bg(core_color(c))),
                    )
                    .child(label(
                        &c.map(|c| c.to_hex()).unwrap_or_else(|| "Hover the image".into()),
                    ));
            }
            Tool::Select => {
                bar = bar.child(label("Click an object to edit it, or drag to select an area"));
            }
            _ => {}
        }
        if [Tool::Arrow, Tool::Line, Tool::Rect, Tool::Oval, Tool::Text].contains(&target_tool) {
            bar = bar.child(vsep(t)).child(label("Hand-drawn")).child(
                toggle("hand", st.hand_drawn, t)
                    .debug_selector(|| "hand".into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        let on = !this.active_style().hand_drawn;
                        this.edit_style(|s| s.hand_drawn = on, cx)
                    })),
            );
        }
        if self.selected.is_some() {
            bar = bar
                .child(div().flex_1())
                .child(
                    icon_button("duplicate", "copy", t, false)
                        .debug_selector(|| "duplicate".into())
                        .tooltip(tooltip("Duplicate  ⌘D"))
                        .on_click(cx.listener(|this, _, _, cx| this.duplicate_selected(cx))),
                )
                .child(
                    icon_button("delete", "trash", t, false)
                        .debug_selector(|| "delete".into())
                        .tooltip(tooltip("Delete  ⌫"))
                        .on_click(cx.listener(|this, _, _, cx| this.delete_selected(cx))),
                );
        }
        bar.into_any_element()
    }

    fn side_panel(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = self.panel?;
        let s = self.shot.scale.max(1.0);
        let body = div()
            .id("side-panel")
            .debug_selector(|| "side-panel".into())
            .w(px(264.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(14.))
            .border_l_1()
            .border_color(t.border)
            .overflow_y_scroll();
        Some(match panel {
            Panel::Backdrop => {
                let b = self.doc.backdrop.clone();
                body.child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("Backdrop"))
                        .child(
                            toggle("backdrop-on", b.enabled, t)
                                .debug_selector(|| "backdrop-on".into())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let mut nb = this.doc.backdrop.clone();
                                    nb.enabled = !nb.enabled;
                                    this.doc.set_backdrop(nb);
                                    this.zoom = None;
                                    cx.notify();
                                })),
                        ),
                )
                .child(section_title("BACKGROUND", t))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .children(BACKDROP_PRESETS.iter().enumerate().map(|(i, (a, c))| {
                            let (a, c) = (*a, *c);
                            div()
                                .id(("preset", i))
                                .debug_selector(move || format!("preset-{i}"))
                                .size(px(38.))
                                .rounded(px(9.))
                                .border_2()
                                .border_color(if b.preset == i && b.enabled { t.accent } else { t.border })
                                .bg(gpui::linear_gradient(
                                    135.0,
                                    gpui::linear_color_stop(core_color(a), 0.0),
                                    gpui::linear_color_stop(core_color(c), 1.0),
                                ))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let mut nb = this.doc.backdrop.clone();
                                    nb.preset = i;
                                    nb.enabled = true;
                                    this.doc.set_backdrop(nb);
                                    this.zoom = None;
                                    cx.notify();
                                }))
                        })),
                )
                .child(section_title("PADDING", t))
                .child(slider(
                    "backdrop-padding",
                    b.padding / (200.0 * s),
                    t,
                    listen(cx, move |this, v: f32, _, cx| {
                        let mut nb = this.doc.backdrop.clone();
                        nb.padding = (v * 200.0 * s).round();
                        this.doc.set_backdrop(nb);
                        cx.notify();
                    }),
                ))
                .child(section_title("CORNERS", t))
                .child(slider(
                    "backdrop-radius",
                    b.radius / (40.0 * s),
                    t,
                    listen(cx, move |this, v: f32, _, cx| {
                        let mut nb = this.doc.backdrop.clone();
                        nb.radius = (v * 40.0 * s).round();
                        this.doc.set_backdrop(nb);
                        cx.notify();
                    }),
                ))
                .child(section_title("SHADOW", t))
                .child(slider(
                    "backdrop-shadow",
                    b.shadow,
                    t,
                    listen(cx, |this, v: f32, _, cx| {
                        let mut nb = this.doc.backdrop.clone();
                        nb.shadow = v;
                        this.doc.set_backdrop(nb);
                        cx.notify();
                    }),
                ))
                .into_any_element()
            }
            Panel::Export => {
                let quality = crate::state::state(cx).settings.save_quality;
                body.child(div().font_weight(FontWeight::SEMIBOLD).child("Export"))
                    .child(section_title("SAVE AS", t))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .children(ImageFormat::WRITABLE.iter().map(|f| {
                                let f = *f;
                                let id: SharedString = format!("export-{}", f.extension()).into();
                                let sel = id.to_string();
                                button(id, f.label(), t, Kind::Secondary)
                                    .debug_selector(move || sel.clone())
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| export::run(this, Target::Format(f), cx)),
                                    )
                            })),
                    )
                    .child(section_title("QUALITY (JPEG, WEBP, HEIC, AVIF)", t))
                    .child(slider("export-quality", quality, t, |v, _, cx| {
                        update_settings(cx, |s| s.save_quality = (v * 100.0).round() / 100.0)
                    }))
                    .child(section_title("COPY AS", t))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                button("copy-file", "File (paste into Finder, Slack…)", t, Kind::Secondary)
                                    .debug_selector(|| "copy-file".into())
                                    .on_click(cx.listener(|this, _, _, cx| export::run(this, Target::File, cx))),
                            )
                            .child(
                                button("copy-path", "File path", t, Kind::Secondary)
                                    .debug_selector(|| "copy-path".into())
                                    .on_click(cx.listener(|this, _, _, cx| export::run(this, Target::Path, cx))),
                            )
                            .child(
                                button("copy-base64", "Base64 data URI", t, Kind::Secondary)
                                    .debug_selector(|| "copy-base64".into())
                                    .on_click(cx.listener(|this, _, _, cx| export::run(this, Target::Base64, cx))),
                            ),
                    )
                    .child(section_title("ANIMATION", t))
                    .child(
                        button("gif-panel", "Before / After GIF", t, Kind::Secondary)
                            .debug_selector(|| "gif-panel".into())
                            .on_click(cx.listener(|this, _, _, cx| export::run(this, Target::BeforeAfterGif, cx))),
                    )
                    .child(
                        button("save-as", "Save As…  ⇧⌘S", t, Kind::Ghost)
                            .debug_selector(|| "save-as".into())
                            .on_click(cx.listener(|this, _, _, cx| export::save_as(this, cx))),
                    )
                    .into_any_element()
            }
            Panel::Resize => {
                let (w, h) = self.doc.export_size();
                let presets: [(f32, &str); 6] = [
                    (0.25, "25%"),
                    (0.5, "50%"),
                    (0.75, "75%"),
                    (1.0, "100%"),
                    (1.5, "150%"),
                    (2.0, "200%"),
                ];
                body.child(div().font_weight(FontWeight::SEMIBOLD).child("Image size"))
                    .child(div().text_color(t.text_muted).child(format!(
                        "Output {} × {} px",
                        (w as f32 * self.export_scale).round(),
                        (h as f32 * self.export_scale).round()
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .children(presets.iter().map(|(v, l)| {
                                let v = *v;
                                let id: SharedString = format!("scale-{}", (v * 100.0) as u32).into();
                                let sel = id.to_string();
                                button(
                                    id,
                                    *l,
                                    t,
                                    if (self.export_scale - v).abs() < 0.001 {
                                        Kind::Primary
                                    } else {
                                        Kind::Secondary
                                    },
                                )
                                .debug_selector(move || sel.clone())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.export_scale = v;
                                    cx.notify();
                                }))
                            })),
                    )
                    .when(self.shot.scale > 1.0, |d| {
                        d.child(
                            button("scale-standard", "Standard resolution (1×)", t, Kind::Secondary)
                                .debug_selector(|| "scale-standard".into())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.export_scale = 1.0 / this.shot.scale;
                                    cx.notify();
                                })),
                        )
                    })
                    .into_any_element()
            }
        })
    }

    fn bottom_bar(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let hover = self.hover.filter(|p| self.doc.image_rect().contains(*p));
        let s = self.shot.scale.max(1.0);
        div()
            .id("status")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .h(px(46.))
            .px(px(12.))
            .border_t_1()
            .border_color(t.border)
            .text_size(px(11.))
            .text_color(t.text_muted)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        icon_button("zoom-out", "zoom-out", t, false)
                            .debug_selector(|| "zoom-out".into())
                            .tooltip(tooltip("Zoom out  ⌘−"))
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_by(0.8, cx))),
                    )
                    .child(
                        div()
                            .id("zoom-fit")
                            .debug_selector(|| "zoom-fit".into())
                            .w(px(44.))
                            .flex()
                            .justify_center()
                            .cursor_pointer()
                            .tooltip(tooltip("Fit  ⌘1 · 100%  ⌘0"))
                            .on_click(cx.listener(|this, _, _, cx| this.set_zoom(None, cx)))
                            .child(format!("{:.0}%", self.zoom_percent())),
                    )
                    .child(
                        icon_button("zoom-in", "zoom-in", t, false)
                            .debug_selector(|| "zoom-in".into())
                            .tooltip(tooltip("Zoom in  ⌘+"))
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_by(1.25, cx))),
                    ),
            )
            .child(div().flex_1().min_w(px(0.)).truncate().child(match hover {
                Some(p) => {
                    let c = self.hover_color().map(|c| c.to_hex()).unwrap_or_default();
                    format!(
                        "{}, {}  {c}  ·  {}",
                        (p.x / s).round(),
                        (p.y / s).round(),
                        tool_hint(self.tool)
                    )
                }
                None => tool_hint(self.tool).to_string(),
            }))
            .child(self.actions(t, cx))
            .into_any_element()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        let scene = self.scene(window, t.accent, t.canvas);
        let bounds_cell = self.canvas_bounds.clone();
        let (content, zoom, pan, scale) = (self.content_rect(), self.zoom, self.pan, self.shot.scale);
        let painter = canvas(
            move |b, _, _| bounds_cell.set(b),
            move |b, _, window, cx| {
                let view = compute_view(b, content, zoom, pan, scale);
                paint(scene, view, b, window, cx);
            },
        )
        .size_full();
        let cursor = match self.tool {
            Tool::Select => CursorStyle::Arrow,
            Tool::Text => CursorStyle::IBeam,
            _ => CursorStyle::Crosshair,
        };
        glass_root(&t)
            .id("editor")
            .key_context("Editor")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_key_up(cx.listener(Self::on_key_up))
            .child(self.toolbar(&t, cx))
            .child(self.inspector(&t, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        div()
                            .id("editor-canvas")
                            .debug_selector(|| "editor-canvas".into())
                            .flex_1()
                            .min_w(px(0.))
                            .relative()
                            .overflow_hidden()
                            .cursor(cursor)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
                            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_down))
                            .on_mouse_move(cx.listener(Self::on_move))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
                            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_up))
                            .on_scroll(cx.listener(Self::on_scroll))
                            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                                if let Some(p) = paths.paths().first()
                                    && let Ok(bytes) = std::fs::read(p)
                                    && let Ok((bmp, _)) = oshot_native::imageio::decode(&bytes)
                                {
                                    this.add_overlay(bmp, cx);
                                }
                            }))
                            .child(painter),
                    )
                    .children(self.side_panel(&t, cx)),
            )
            .child(self.bottom_bar(&t, cx))
    }
}
