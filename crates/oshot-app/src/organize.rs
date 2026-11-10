//! Keeping screenshots tidy: moving macOS's own screenshots off the Desktop
//! and cleaning up old OShot captures.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui::{App, AppContext};
use oshot_core::format::ImageFormat;
use oshot_native::imageio::{ConvertOptions, convert};

use crate::state::{Notice, notify, platform, state};

/// Screenshots macOS saved into `dir` that are finished writing.
pub fn tidy_candidates(dir: &Path, now: SystemTime) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let is_shot =
                (name.starts_with("Screenshot ") || name.starts_with("Screen Shot ")) && !name.starts_with('.');
            let ext_ok = p
                .extension()
                .and_then(|e| e.to_str())
                .and_then(ImageFormat::from_extension)
                .is_some();
            // macOS writes a hidden temp file then renames; anything a second
            // old is complete.
            let settled = std::fs::metadata(p)
                .and_then(|m| m.modified())
                .map(|m| now.duration_since(m).unwrap_or_default() >= Duration::from_secs(1))
                .unwrap_or(false);
            is_shot && ext_ok && settled
        })
        .collect()
}

/// The fixed text before the first `{` in a file name template. Only files
/// starting with it are ever auto-deleted, so user files are never touched.
pub fn template_prefix(template: &str) -> &str {
    template.split('{').next().unwrap_or("").trim_end()
}

/// OShot captures in `dir` older than `days`.
pub fn clean_candidates(dir: &Path, prefix: &str, days: u32, now: SystemTime) -> Vec<PathBuf> {
    if prefix.len() < 2 || days == 0 {
        return Vec::new();
    }
    let max_age = Duration::from_secs(days as u64 * 86_400);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let image = p
                .extension()
                .and_then(|e| e.to_str())
                .and_then(ImageFormat::from_extension)
                .is_some();
            let old = std::fs::metadata(p)
                .and_then(|m| m.modified())
                .map(|m| now.duration_since(m).unwrap_or_default() > max_age)
                .unwrap_or(false);
            p.is_file() && name.starts_with(prefix) && image && old
        })
        .collect()
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to).or_else(|_| {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)
    })
}

pub fn apply_watcher(cx: &mut App) {
    let p = platform(cx);
    if state(cx).settings.tidy_desktop {
        let folder = p.system_screenshot_folder();
        p.watch_folder(Some(&folder));
        folder_changed(cx);
    } else {
        p.watch_folder(None);
    }
}

pub fn folder_changed(cx: &mut App) {
    if !state(cx).settings.tidy_desktop {
        return;
    }
    let source = platform(cx).system_screenshot_folder();
    let dest = state(cx).save_folder();
    let convert_to = state(cx)
        .settings
        .tidy_convert
        .then(|| state(cx).settings.converter.clone());
    let task = cx.background_spawn(async move { tidy(&source, &dest, convert_to) });
    cx.spawn(async move |cx| {
        let moved = task.await;
        if moved > 0 {
            cx.update(|cx| {
                notify(
                    cx,
                    Notice::Info(format!(
                        "Moved {moved} screenshot{} to your OShot folder",
                        if moved == 1 { "" } else { "s" }
                    )),
                )
            });
        }
    })
    .detach();
}

/// Moves (and optionally converts) settled screenshots from `source` to `dest`.
pub fn tidy(source: &Path, dest: &Path, convert_to: Option<oshot_core::settings::ConverterDefaults>) -> usize {
    let files = tidy_candidates(source, SystemTime::now());
    if files.is_empty() || std::fs::create_dir_all(dest).is_err() {
        return 0;
    }
    let mut moved = 0;
    for f in files {
        let stem = f
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Screenshot")
            .to_string();
        let ok = match &convert_to {
            Some(c) => std::fs::read(&f)
                .ok()
                .and_then(|bytes| {
                    convert(
                        &bytes,
                        &ConvertOptions {
                            format: c.format,
                            quality: c.quality,
                            lossless: c.lossless,
                            max_dimension: c.max_dimension,
                            downscale_retina: c.downscale_retina,
                            strip_metadata: c.strip_metadata,
                        },
                    )
                    .ok()
                })
                .map(|out| {
                    let target = oshot_core::naming::unique_path(dest, &stem, c.format.extension());
                    std::fs::write(&target, out.bytes).is_ok() && std::fs::remove_file(&f).is_ok()
                })
                .unwrap_or(false),
            None => {
                let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("png").to_string();
                let target = oshot_core::naming::unique_path(dest, &stem, &ext);
                move_file(&f, &target).is_ok()
            }
        };
        if ok {
            moved += 1;
        }
    }
    moved
}

/// Deletes OShot captures older than the configured number of days.
pub fn auto_clean(cx: &mut App) {
    let Some(days) = state(cx).settings.auto_clean_days else {
        return;
    };
    let dir = state(cx).save_folder();
    let prefix = template_prefix(&state(cx).settings.filename_template).to_string();
    cx.background_spawn(async move {
        for f in clean_candidates(&dir, &prefix, days, SystemTime::now()) {
            let _ = std::fs::remove_file(f);
        }
    })
    .detach();
}

/// Runs auto-clean now and every six hours.
pub fn schedule(cx: &mut App) {
    auto_clean(cx);
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_secs(6 * 3600)).await;
            cx.update(auto_clean);
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oshot-org-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_settled_macos_screenshots_only() {
        let d = temp("tidy");
        std::fs::write(d.join("Screenshot 2026-11-08 at 10.00.00.png"), b"x").unwrap();
        std::fs::write(d.join("notes.png"), b"x").unwrap();
        std::fs::write(d.join("Screenshot draft.txt"), b"x").unwrap();
        let later = SystemTime::now() + Duration::from_secs(5);
        let found = tidy_candidates(&d, later);
        assert_eq!(found.len(), 1);
        assert!(
            tidy_candidates(&d, SystemTime::now() - Duration::from_secs(10)).is_empty(),
            "not settled yet"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn moves_into_destination() {
        let src = temp("src");
        let dst = temp("dst");
        let f = src.join("Screenshot 1.png");
        std::fs::write(&f, b"png").unwrap();
        let old = SystemTime::now() - Duration::from_secs(10);
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(tidy(&src, &dst, None), 1);
        assert!(!f.exists());
        assert!(dst.join("Screenshot 1.png").exists());
        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&dst).unwrap();
    }

    #[test]
    fn cleaning_only_touches_old_oshot_files() {
        let d = temp("clean");
        for name in ["OShot 2026-01-01 at 10.00.00.png", "Holiday.png", "OShot notes.txt"] {
            std::fs::write(d.join(name), b"x").unwrap();
        }
        let future = SystemTime::now() + Duration::from_secs(40 * 86_400);
        let found = clean_candidates(&d, template_prefix("OShot {date} at {time}"), 30, future);
        assert_eq!(found, vec![d.join("OShot 2026-01-01 at 10.00.00.png")]);
        assert!(
            clean_candidates(&d, template_prefix("{date}"), 30, future).is_empty(),
            "no prefix, no deletes"
        );
        assert!(
            clean_candidates(&d, "OShot", 30, SystemTime::now()).is_empty(),
            "too new"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}
