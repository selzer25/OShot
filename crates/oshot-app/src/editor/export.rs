//! Editor output: clipboard, files, formats, GIF, pin, OCR and redaction.
//! Rendering and encoding run off the main thread.

use std::path::PathBuf;

use gpui::{AppContext, Context, Window, WindowHandle};
use oshot_core::bitmap::Bitmap;
use oshot_core::effects::EffectCache;
use oshot_core::format::{ImageFormat, data_uri};
use oshot_core::model::{Document, Shape, Tool};
use oshot_native::imageio;
use oshot_native::render::{ExportOptions, render};

use super::Editor;
use crate::output;
use crate::state::{Notice, Shot, notify, platform, state};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Clipboard,
    SaveFolder,
    Format(ImageFormat),
    /// Copy as a file, like Finder's ⌘C.
    File,
    /// Save, then copy the file's path.
    Path,
    Base64,
    Pin,
    BeforeAfterGif,
}

enum Done {
    Clipboard(Vec<u8>),
    Saved(PathBuf),
    File(PathBuf),
    Text(String, &'static str),
    Pin(Bitmap),
}

fn rendered(doc: &Document, shot: &Shot, scale: f32, skip_overlays: bool) -> Bitmap {
    render(
        doc,
        shot.pixels(),
        &mut EffectCache::default(),
        ExportOptions { skip_overlays, scale },
    )
}

fn dpi(shot: &Shot, export_scale: f32) -> u32 {
    ((shot.scale * export_scale).round() as u32).max(1)
}

fn encode(bmp: &Bitmap, format: ImageFormat, quality: f32, dpi: u32, shrink: bool) -> Result<Vec<u8>, String> {
    let mut bytes = imageio::encode(bmp.view(), format, quality, dpi)?;
    if format == ImageFormat::Png && shrink {
        bytes = oshot_native::shrink::shrink_png(&bytes)?;
    }
    Ok(bytes)
}

/// The document as it should be exported for `target`. With an active
/// selection, copying exports just the selection.
pub fn export_doc(editor: &Editor, target: Target) -> Document {
    let mut doc = editor.doc.export_copy();
    if target == Target::Clipboard
        && let Some(sel) = editor.selection
    {
        doc.crop = Some(
            sel.intersect(&doc.image_rect())
                .to_pixels(doc.image_width, doc.image_height)
                .to_rect(),
        );
        doc.expand_canvas = false;
        doc.backdrop.enabled = false;
    }
    doc
}

pub fn run(editor: &mut Editor, target: Target, cx: &mut Context<Editor>) {
    run_with_close(editor, target, None, cx);
}

pub fn copy_shortcut(editor: &mut Editor, window: &Window, cx: &mut Context<Editor>) {
    let close = state(cx)
        .settings
        .close_on_copy_shortcut
        .then(|| window.window_handle().downcast::<Editor>().expect("editor window"));
    run_with_close(editor, Target::Clipboard, close, cx);
}

fn run_with_close(editor: &mut Editor, target: Target, close: Option<WindowHandle<Editor>>, cx: &mut Context<Editor>) {
    if target == Target::BeforeAfterGif
        && !editor
            .doc
            .objects
            .iter()
            .any(|o| matches!(o.shape, Shape::Image { .. }))
    {
        notify(
            cx,
            Notice::Info("Paste an image on top first (⌘V) to make a before/after GIF".into()),
        );
        return;
    }
    let doc = export_doc(editor, target);
    let shot = editor.shot.clone();
    let scale = editor.export_scale;
    let settings = state(cx).settings.clone();
    let folder = state(cx).save_folder();
    let exports = state(cx).paths.support.join("Exports");
    let task = cx.background_spawn(async move {
        let bmp = rendered(&doc, &shot, scale, false);
        let d = dpi(&shot, scale);
        let save = |bytes: Vec<u8>, format: ImageFormat, dir: &std::path::Path| -> Result<PathBuf, String> {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            let path = output::save_path(dir, &settings, format);
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok(path)
        };
        Ok::<Done, String>(match target {
            Target::Clipboard => Done::Clipboard(encode(&bmp, ImageFormat::Png, 1.0, d, false)?),
            Target::SaveFolder => {
                let f = settings.file_format;
                Done::Saved(save(
                    encode(&bmp, f, settings.save_quality, d, settings.shrink_png)?,
                    f,
                    &folder,
                )?)
            }
            Target::Format(f) => Done::Saved(save(
                encode(&bmp, f, settings.save_quality, d, settings.shrink_png)?,
                f,
                &folder,
            )?),
            Target::File => Done::File(save(
                encode(&bmp, ImageFormat::Png, 1.0, d, false)?,
                ImageFormat::Png,
                &exports,
            )?),
            Target::Path => {
                let path = save(
                    encode(&bmp, ImageFormat::Png, 1.0, d, settings.shrink_png)?,
                    ImageFormat::Png,
                    &folder,
                )?;
                Done::Text(path.to_string_lossy().to_string(), "Copied file path")
            }
            Target::Base64 => Done::Text(
                data_uri(ImageFormat::Png, &encode(&bmp, ImageFormat::Png, 1.0, d, true)?),
                "Copied as Base64",
            ),
            Target::Pin => Done::Pin(bmp),
            Target::BeforeAfterGif => {
                let before = rendered(&doc, &shot, scale, true);
                let gif = imageio::encode_gif(&[before.view(), bmp.view()], 0.9)?;
                Done::Saved(save(gif, ImageFormat::Gif, &folder)?)
            }
        })
    });
    let shot_scale = editor.shot.scale * editor.export_scale;
    cx.spawn(async move |_, cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok(Done::Clipboard(bytes)) => {
                output::write_png_to_clipboard(bytes, cx);
                notify(cx, Notice::Copied("Copied to clipboard".into()));
                if let Some(window) = close {
                    crate::windows::close(window, cx);
                }
            }
            Ok(Done::Saved(path)) => notify(cx, Notice::Saved(path)),
            Ok(Done::File(path)) => {
                platform(cx).copy_files(std::slice::from_ref(&path));
                notify(cx, Notice::Copied("Copied as file".into()));
            }
            Ok(Done::Text(text, msg)) => {
                output::copy_text(text, cx);
                notify(cx, Notice::Copied(msg.into()));
            }
            Ok(Done::Pin(bmp)) => crate::pin::open(Shot::new(bmp, shot_scale.max(1.0)), cx),
            Err(e) => notify(cx, Notice::Error(e)),
        });
    })
    .detach();
}

/// ⌘⇧S: asks where to save; the extension picks the format.
pub fn save_as(editor: &mut Editor, cx: &mut Context<Editor>) {
    let folder = state(cx).save_folder();
    let _ = std::fs::create_dir_all(&folder);
    let name = format!(
        "{}.png",
        oshot_core::naming::expand_template(
            &state(cx).settings.filename_template,
            oshot_core::naming::LocalTime::now()
        )
    );
    let rx = cx.prompt_for_new_path(&folder, Some(&name));
    let doc = editor.doc.export_copy();
    let shot = editor.shot.clone();
    let scale = editor.export_scale;
    let quality = state(cx).settings.save_quality;
    cx.spawn(async move |_, cx| {
        let Ok(Ok(Some(path))) = rx.await else { return };
        let format = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(ImageFormat::from_extension)
            .unwrap_or(ImageFormat::Png);
        let task = cx.background_spawn(async move {
            let bmp = rendered(&doc, &shot, scale, false);
            let bytes = encode(&bmp, format, quality, dpi(&shot, scale), false)?;
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok::<PathBuf, String>(path)
        });
        let result = task.await;
        cx.update(|cx| match result {
            Ok(path) => notify(cx, Notice::Saved(path)),
            Err(e) => notify(cx, Notice::Error(e)),
        });
    })
    .detach();
}

/// Recognises text in the image (or selection) and copies it.
pub fn ocr(editor: &mut Editor, cx: &mut Context<Editor>) {
    let shot = match editor.selection {
        Some(sel) => editor.shot.crop(sel.to_pixels(editor.shot.width, editor.shot.height)),
        None => editor.shot.clone(),
    };
    let task = cx.background_spawn(async move { oshot_native::ocr::recognize(shot.pixels()) });
    cx.spawn(async move |_, cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok(r) if !r.text().trim().is_empty() => {
                let n = r.text().lines().count();
                output::copy_text(r.text(), cx);
                notify(
                    cx,
                    Notice::Copied(format!("Copied {n} line{} of text", if n == 1 { "" } else { "s" })),
                );
            }
            Ok(_) => notify(cx, Notice::Info("No text found".into())),
            Err(e) => notify(cx, Notice::Error(e)),
        });
    })
    .detach();
}

/// Finds emails, keys, tokens and card numbers and pixelates them.
pub fn redact(editor: &mut Editor, cx: &mut Context<Editor>) {
    let shot = editor.shot.clone();
    let task = cx.background_spawn(async move { oshot_native::ocr::recognize(shot.pixels()) });
    cx.spawn(async move |this, cx| {
        let result = task.await;
        let _ = this.update(cx, |editor, cx| match result {
            Ok(r) if !r.secrets.is_empty() => {
                let style = editor.style_for(Tool::Pixelate);
                editor.doc.checkpoint();
                for (rect, _) in &r.secrets {
                    editor.doc.insert(Shape::Pixelate { rect: *rect }, style.clone());
                }
                let kinds: Vec<&str> = r.secrets.iter().map(|(_, k)| k.label()).collect();
                notify(
                    cx,
                    Notice::Info(format!(
                        "Redacted {} item{}: {}",
                        kinds.len(),
                        if kinds.len() == 1 { "" } else { "s" },
                        kinds.join(", ")
                    )),
                );
                cx.notify();
            }
            Ok(_) => notify(cx, Notice::Info("No secrets found".into())),
            Err(e) => notify(cx, Notice::Error(e)),
        });
    })
    .detach();
}
