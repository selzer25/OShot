//! The image converter: drop, paste or pick images, choose a format, convert.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, AppContext, ClipboardEntry, Context, ExternalPaths, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, ObjectFit, ParentElement, PathPromptOptions, Render, RenderImage, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, Window, div, img, prelude::FluentBuilder, px,
};
use oshot_core::format::{ImageFormat, human_size};
use oshot_core::settings::{ConverterDefaults, OutputLocation};
use oshot_native::imageio::{self, ConvertOptions, Info};

use crate::state::{Notice, Shot, notify, state, update_settings};
use crate::theme::theme;
use crate::ui::{
    Kind, button, card, glass_root, icon, icon_button, listen, row, segmented, slider, titlebar_spacer, toggle, tooltip,
};
use crate::windows::{self, Spec};

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Pending,
    Working,
    Done { path: PathBuf, size: u64 },
    Failed(String),
}

pub struct Item {
    pub path: Option<PathBuf>,
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
    pub info: Result<Info, String>,
    pub status: Status,
    thumb: Option<Arc<RenderImage>>,
}

pub struct Converter {
    pub items: Vec<Item>,
    pub opts: ConverterDefaults,
    focus: FocusHandle,
}

pub const MAX_SIZES: [Option<u32>; 5] = [None, Some(4096), Some(2048), Some(1024), Some(512)];

impl Converter {
    pub fn new(cx: &mut Context<Self>) -> Converter {
        Converter {
            items: Vec::new(),
            opts: state(cx).settings.converter.clone(),
            focus: cx.focus_handle(),
        }
    }

    pub fn add_bytes(&mut self, name: String, path: Option<PathBuf>, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let info = imageio::info(&bytes);
        let thumb = info.as_ref().ok().and_then(|_| {
            let small = imageio::convert(
                &bytes,
                &ConvertOptions {
                    format: ImageFormat::Png,
                    quality: 1.0,
                    lossless: true,
                    max_dimension: Some(96),
                    downscale_retina: false,
                    strip_metadata: true,
                },
            )
            .ok()?;
            let (bmp, _) = imageio::decode(&small.bytes).ok()?;
            Some(Shot::new(bmp, 2.0).image)
        });
        self.items.push(Item {
            path,
            name,
            bytes: Arc::new(bytes),
            info,
            status: Status::Pending,
            thumb,
        });
        cx.notify();
    }

    pub fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for p in paths {
            if p.is_dir() {
                let mut files: Vec<PathBuf> = std::fs::read_dir(&p)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|f| f.is_file())
                    .collect();
                files.sort();
                self.add_paths(files, cx);
                continue;
            }
            match std::fs::read(&p) {
                Ok(bytes) => {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.add_bytes(name, Some(p), bytes, cx);
                }
                Err(e) => notify(cx, Notice::Error(format!("Could not read {}: {e}", p.display()))),
            }
        }
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) {
        let entries = cx
            .read_from_clipboard()
            .map(|item| item.entries().to_vec())
            .unwrap_or_default();
        let mut added = false;
        for e in &entries {
            match e {
                ClipboardEntry::Image(image) => {
                    let n = self.items.len() + 1;
                    self.add_bytes(format!("Pasted image {n}"), None, image.bytes.clone(), cx);
                    added = true;
                }
                ClipboardEntry::String(s) => {
                    let paths: Vec<PathBuf> = s
                        .text()
                        .lines()
                        .map(|l| PathBuf::from(l.trim().trim_start_matches("file://")))
                        .filter(|p| p.is_file())
                        .collect();
                    if !paths.is_empty() {
                        self.add_paths(paths, cx);
                        added = true;
                    }
                }
                _ => {}
            }
        }
        if !added {
            notify(cx, Notice::Info("The clipboard has no image".into()));
        }
    }

    pub fn choose(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                let _ = this.update(cx, |this, cx| this.add_paths(paths, cx));
            }
        })
        .detach();
    }

    pub fn set_opts(&mut self, f: impl FnOnce(&mut ConverterDefaults), cx: &mut Context<Self>) {
        f(&mut self.opts);
        for i in &mut self.items {
            if matches!(i.status, Status::Done { .. } | Status::Failed(_)) {
                i.status = Status::Pending;
            }
        }
        let opts = self.opts.clone();
        update_settings(cx, |s| s.converter = opts);
        cx.notify();
    }

    pub fn convert_all(&mut self, cx: &mut Context<Self>) {
        let folder = state(cx).save_folder();
        let opts = self.opts.clone();
        for (index, item) in self.items.iter_mut().enumerate() {
            if item.info.is_err() || matches!(item.status, Status::Working | Status::Done { .. }) {
                continue;
            }
            item.status = Status::Working;
            let bytes = item.bytes.clone();
            let dir = match (&opts.output, &item.path) {
                (OutputLocation::NextToOriginal, Some(p)) => {
                    p.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| folder.clone())
                }
                _ => folder.clone(),
            };
            let stem = item
                .path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| item.name.clone());
            let o = opts.clone();
            let task = cx.background_spawn(async move {
                let out = imageio::convert(
                    &bytes,
                    &ConvertOptions {
                        format: o.format,
                        quality: o.quality,
                        lossless: o.lossless,
                        max_dimension: o.max_dimension,
                        downscale_retina: o.downscale_retina,
                        strip_metadata: o.strip_metadata,
                    },
                )?;
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let path = oshot_core::naming::unique_path(&dir, &stem, o.format.extension());
                std::fs::write(&path, &out.bytes).map_err(|e| e.to_string())?;
                Ok::<(PathBuf, u64), String>((path, out.bytes.len() as u64))
            });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(item) = this.items.get_mut(index) {
                        item.status = match result {
                            Ok((path, size)) => Status::Done { path, size },
                            Err(e) => Status::Failed(e),
                        };
                    }
                    if this.items.iter().all(|i| !matches!(i.status, Status::Working)) {
                        let done = this
                            .items
                            .iter()
                            .filter(|i| matches!(i.status, Status::Done { .. }))
                            .count();
                        if done > 0 {
                            notify(
                                cx,
                                Notice::Info(format!("Converted {done} image{}", if done == 1 { "" } else { "s" })),
                            );
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for i in self.items.drain(..) {
            if let Some(t) = i.thumb {
                let _ = window.drop_image(t);
            }
        }
        cx.notify();
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        match (k.key.as_str(), k.modifiers.platform) {
            ("v", true) => self.paste(cx),
            ("o", true) => self.choose(cx),
            ("enter", false) => self.convert_all(cx),
            ("escape", _) | ("w", true) => window.remove_window(),
            _ => {}
        }
    }

    pub fn pending(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.info.is_ok() && i.status == Status::Pending)
            .count()
    }
}

impl Render for Converter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        let o = self.opts.clone();
        let fmt_idx = ImageFormat::WRITABLE.iter().position(|f| *f == o.format).unwrap_or(0);
        let labels: Vec<SharedString> = ImageFormat::WRITABLE.iter().map(|f| f.label().into()).collect();
        let size_idx = MAX_SIZES.iter().position(|s| *s == o.max_dimension).unwrap_or(0);

        let drop_zone = div()
            .id("drop-zone")
            .debug_selector(|| "drop-zone".into())
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .h(px(190.))
            .rounded(px(14.))
            .border_2()
            .border_dashed()
            .border_color(t.border)
            .bg(t.panel)
            .child(icon("file-image").size(px(32.)).text_color(t.text_faint))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(15.))
                    .child("Drop images here"),
            )
            .child(
                div()
                    .text_color(t.text_muted)
                    .child("or paste with ⌘V. Folders work too."),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        button("choose", "Choose Files…", &t, Kind::Secondary)
                            .debug_selector(|| "choose".into())
                            .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
                    )
                    .child(
                        button("paste", "Paste", &t, Kind::Secondary)
                            .debug_selector(|| "paste".into())
                            .on_click(cx.listener(|this, _, _, cx| this.paste(cx))),
                    ),
            );

        let list = div()
            .id("items")
            .flex()
            .flex_col()
            .gap(px(6.))
            .max_h(px(230.))
            .overflow_y_scroll()
            .children(self.items.iter().enumerate().map(|(i, item)| {
                let detail = match &item.info {
                    Ok(info) => format!(
                        "{} · {} × {} · {}",
                        info.format.map(|f| f.label()).unwrap_or("Image"),
                        info.width,
                        info.height,
                        human_size(item.bytes.len() as u64)
                    ),
                    Err(e) => e.clone(),
                };
                let result: Option<gpui::AnyElement> = match &item.status {
                    Status::Done { path, size } => {
                        let before = item.bytes.len() as f64;
                        let change = ((*size as f64 - before) / before.max(1.0) * 100.0).round();
                        let path = path.clone();
                        Some(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_color(if change <= 0.0 { t.success } else { t.text_muted })
                                        .child(format!("→ {} {} ({:+}%)", o.format.label(), human_size(*size), change)),
                                )
                                .child(
                                    icon_button(("reveal", i), "folder-open", &t, false)
                                        .debug_selector(move || format!("reveal-{i}"))
                                        .tooltip(tooltip("Show in Finder"))
                                        .on_click(move |_, _, cx| cx.reveal_path(&path)),
                                )
                                .into_any_element(),
                        )
                    }
                    Status::Working => Some(div().text_color(t.text_muted).child("Converting…").into_any_element()),
                    Status::Failed(e) => Some(div().text_color(t.danger).child(e.clone()).into_any_element()),
                    Status::Pending => None,
                };
                card(&t)
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(8.))
                    .child(
                        div()
                            .size(px(40.))
                            .flex_none()
                            .rounded(px(8.))
                            .overflow_hidden()
                            .bg(t.canvas)
                            .when_some(item.thumb.clone(), |d, th| {
                                d.child(img(th).size_full().object_fit(ObjectFit::Cover))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(item.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(if item.info.is_ok() { t.text_muted } else { t.danger })
                                    .child(detail),
                            ),
                    )
                    .children(result)
                    .child(
                        icon_button(("remove", i), "x", &t, false)
                            .debug_selector(move || format!("remove-{i}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if i < this.items.len() {
                                    this.items.remove(i);
                                }
                                cx.notify();
                            })),
                    )
            }));

        let options = card(&t)
            .flex()
            .flex_col()
            .child(row(
                "Format",
                None,
                segmented(
                    "format",
                    &labels,
                    fmt_idx,
                    &t,
                    listen(cx, |this, i: usize, _, cx| {
                        this.set_opts(|o| o.format = ImageFormat::WRITABLE[i], cx)
                    }),
                ),
                &t,
            ))
            .when(
                o.format.is_lossy() && !(o.format == ImageFormat::WebP && o.lossless),
                |d| {
                    d.child(row(
                        format!("Quality {}%", (o.quality * 100.0).round()),
                        None,
                        div().w(px(220.)).child(slider(
                            "quality",
                            o.quality,
                            &t,
                            listen(cx, |this, v: f32, _, cx| {
                                this.set_opts(|o| o.quality = (v * 100.0).round() / 100.0, cx)
                            }),
                        )),
                        &t,
                    ))
                },
            )
            .when(o.format == ImageFormat::WebP, |d| {
                d.child(row(
                    "Lossless",
                    Some("Bigger files, identical pixels"),
                    toggle("lossless", o.lossless, &t)
                        .debug_selector(|| "lossless".into())
                        .on_click(cx.listener(|this, _, _, cx| this.set_opts(|o| o.lossless = !o.lossless, cx))),
                    &t,
                ))
            })
            .child(row(
                "Max size",
                None,
                segmented(
                    "max-size",
                    &[
                        "Original".into(),
                        "4096".into(),
                        "2048".into(),
                        "1024".into(),
                        "512".into(),
                    ],
                    size_idx,
                    &t,
                    listen(cx, |this, i: usize, _, cx| {
                        this.set_opts(|o| o.max_dimension = MAX_SIZES[i], cx)
                    }),
                ),
                &t,
            ))
            .child(row(
                "Retina to standard",
                Some("Halve @2x images to 1x pixels"),
                toggle("retina", o.downscale_retina, &t)
                    .debug_selector(|| "retina".into())
                    .on_click(
                        cx.listener(|this, _, _, cx| this.set_opts(|o| o.downscale_retina = !o.downscale_retina, cx)),
                    ),
                &t,
            ))
            .child(row(
                "Remove metadata",
                Some("Drops location, camera and other EXIF data"),
                toggle("strip", o.strip_metadata, &t)
                    .debug_selector(|| "strip".into())
                    .on_click(
                        cx.listener(|this, _, _, cx| this.set_opts(|o| o.strip_metadata = !o.strip_metadata, cx)),
                    ),
                &t,
            ))
            .child(row(
                "Save to",
                None,
                segmented(
                    "output",
                    &["Next to original".into(), "OShot folder".into()],
                    (o.output == OutputLocation::SaveFolder) as usize,
                    &t,
                    listen(cx, |this, i: usize, _, cx| {
                        this.set_opts(
                            |o| {
                                o.output = if i == 1 {
                                    OutputLocation::SaveFolder
                                } else {
                                    OutputLocation::NextToOriginal
                                }
                            },
                            cx,
                        )
                    }),
                ),
                &t,
            ));

        let pending = self.pending();
        glass_root(&t)
            .id("converter")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.add_paths(paths.paths().to_vec(), cx)))
            .child(titlebar_spacer())
            .child(
                div()
                    .id("converter-body")
                    .debug_selector(|| "window-body".into())
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .px(px(20.))
                    .pb(px(16.))
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Convert Images"),
                            )
                            .child(div().text_color(t.text_muted).child(
                                "PNG, JPEG, WebP, HEIC, AVIF, TIFF and GIF. Formats are detected from the file itself.",
                            )),
                    )
                    .child(if self.items.is_empty() {
                        drop_zone.into_any_element()
                    } else {
                        list.into_any_element()
                    })
                    .child(options)
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(div().flex().gap(px(8.)).when(!self.items.is_empty(), |d| {
                                d.child(
                                    button("add-more", "Add…", &t, Kind::Ghost)
                                        .debug_selector(|| "add-more".into())
                                        .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
                                )
                                .child(
                                    button("clear", "Clear", &t, Kind::Ghost)
                                        .debug_selector(|| "clear".into())
                                        .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))),
                                )
                            }))
                            .child(
                                button(
                                    "convert",
                                    if pending == 1 {
                                        "Convert 1 image".to_string()
                                    } else {
                                        format!("Convert {pending} images")
                                    },
                                    &t,
                                    Kind::Primary,
                                )
                                .debug_selector(|| "convert".into())
                                .when(pending == 0, |d| d.opacity(0.5))
                                .on_click(cx.listener(|this, _, _, cx| this.convert_all(cx))),
                            ),
                    ),
            )
    }
}

pub fn open(paths: Vec<PathBuf>, cx: &mut App) {
    let handle = windows::find::<Converter>(cx).or_else(|| {
        windows::open(Spec::window("Convert Images", 620.0, 700.0), cx, |_, cx| {
            cx.new(Converter::new)
        })
    });
    if let Some(handle) = handle {
        let _ = handle.update(cx, |c, window, cx| {
            window.activate_window();
            c.focus.focus(window, cx);
            if !paths.is_empty() {
                c.add_paths(paths, cx);
            }
        });
        cx.activate(true);
    }
}
