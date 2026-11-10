//! What happens to a finished capture: clipboard, file, recents, and the
//! editor or thumbnail.

use std::path::{Path, PathBuf};

use gpui::{App, AppContext, ClipboardItem, Image, ImageFormat as GpuiFormat};
use oshot_core::format::ImageFormat;
use oshot_core::naming::{LocalTime, expand_template, unique_path};
use oshot_core::settings::{AfterCapture, Settings};
use oshot_native::imageio::{self, ConvertOptions};

use crate::state::{Notice, Shot, notify, state};
use crate::{commands, editor, recent, thumbnail};

/// The after-capture flow chosen in Settings.
pub fn finish(shot: Shot, cx: &mut App) {
    remember(&shot, cx);
    let s = state(cx).settings.clone();
    if s.copy_on_capture || s.after_capture == AfterCapture::ClipboardOnly {
        copy(&shot, s.after_capture == AfterCapture::ClipboardOnly, cx);
    }
    if s.save_on_capture {
        save(&shot, cx);
    }
    match s.after_capture {
        AfterCapture::Editor => editor::open_shot(shot, cx),
        AfterCapture::Thumbnail => thumbnail::show(shot, cx),
        AfterCapture::ClipboardOnly => {}
    }
}

pub fn png(shot: &Shot) -> Result<Vec<u8>, String> {
    imageio::encode(shot.pixels(), ImageFormat::Png, 1.0, shot.scale.round() as u32)
}

pub fn write_png_to_clipboard(bytes: Vec<u8>, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(GpuiFormat::Png, bytes)));
}

/// Encodes off the main thread, then puts a PNG on the clipboard.
pub fn copy(shot: &Shot, announce: bool, cx: &mut App) {
    let shot = shot.clone();
    let task = cx.background_spawn(async move { png(&shot) });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok(bytes) => {
                write_png_to_clipboard(bytes, cx);
                if announce {
                    notify(cx, Notice::Copied("Copied to clipboard".into()));
                }
            }
            Err(e) => notify(cx, Notice::Error(e)),
        });
    })
    .detach();
}

pub fn copy_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

/// Encodes with the user's format, downscaling and PNG shrinking.
pub fn encode_for_save(shot: &Shot, s: &Settings) -> Result<(Vec<u8>, ImageFormat), String> {
    let format = s.file_format;
    let downscale = s.downscale_retina && shot.scale >= 2.0;
    let scale = shot.scale.round() as u32;
    let bitmap;
    let (pixels, dpi_scale) = if downscale {
        bitmap = shot.pixels().downscale(scale);
        (bitmap.view(), 1)
    } else {
        (shot.pixels(), scale)
    };
    let mut bytes = imageio::encode(pixels, format, s.save_quality, dpi_scale)?;
    if format == ImageFormat::Png && s.shrink_png {
        bytes = oshot_native::shrink::shrink_png(&bytes)?;
    }
    Ok((bytes, format))
}

pub fn save_path(folder: &Path, s: &Settings, format: ImageFormat) -> PathBuf {
    let name = expand_template(&s.filename_template, LocalTime::now());
    unique_path(folder, &name, format.extension())
}

pub fn save(shot: &Shot, cx: &mut App) {
    let shot = shot.clone();
    let s = state(cx).settings.clone();
    let folder = state(cx).save_folder();
    let task = cx.background_spawn(async move {
        let (bytes, format) = encode_for_save(&shot, &s)?;
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let path = save_path(&folder, &s, format);
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok::<PathBuf, String>(path)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok(path) => notify(cx, Notice::Saved(path)),
            Err(e) => notify(cx, Notice::Error(format!("Could not save: {e}"))),
        });
    })
    .detach();
}

/// Copies an image file to the clipboard as PNG.
pub fn copy_file(path: &Path, cx: &mut App) {
    let path = path.to_path_buf();
    let task = cx.background_spawn(async move {
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if ImageFormat::detect(&bytes) == Some(ImageFormat::Png) {
            return Ok(bytes);
        }
        let opts = ConvertOptions {
            format: ImageFormat::Png,
            quality: 1.0,
            lossless: true,
            max_dimension: None,
            downscale_retina: false,
            strip_metadata: false,
        };
        imageio::convert(&bytes, &opts).map(|c| c.bytes)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok(bytes) => {
                write_png_to_clipboard(bytes, cx);
                notify(cx, Notice::Copied("Copied to clipboard".into()));
            }
            Err(e) => notify(cx, Notice::Error(e)),
        });
    })
    .detach();
}

/// Adds the capture to Recent Captures in the menu bar.
pub fn remember(shot: &Shot, cx: &mut App) {
    let shot = shot.clone();
    let dir = state(cx).paths.recent.clone();
    let limit = state(cx).settings.recent_limit;
    let task = cx.background_spawn(async move {
        let full = png(&shot)?;
        let thumb = imageio::convert(
            &full,
            &ConvertOptions {
                format: ImageFormat::Png,
                quality: 1.0,
                lossless: true,
                max_dimension: Some(160),
                downscale_retina: false,
                strip_metadata: true,
            },
        )?
        .bytes;
        recent::add(&dir, &full, &thumb, limit).map_err(|e| e.to_string())
    });
    cx.spawn(async move |cx| {
        if task.await.is_ok() {
            cx.update(commands::rebuild_menu);
        }
    })
    .detach();
}
