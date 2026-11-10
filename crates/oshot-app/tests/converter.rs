//! The converter: paste, pick or receive files from Finder, choose a format
//! and options, convert, and check the files that come out.

mod support;

use std::path::PathBuf;

use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use oshot_app::converter::{Converter, Status};
use oshot_app::state::{Command, Notice};
use oshot_core::bitmap::Bitmap;
use oshot_core::color::rgb;
use oshot_core::format::ImageFormat;
use oshot_core::settings::OutputLocation;
use support::*;

fn open(cx: &mut TestAppContext, paths: Vec<PathBuf>) -> (WindowHandle<Converter>, VisualTestContext) {
    cx.update(|cx| {
        oshot_app::commands::dispatch(
            if paths.is_empty() {
                Command::ConvertImage
            } else {
                Command::ConvertFiles(paths)
            },
            cx,
        )
    });
    cx.run_until_parked();
    let (h, mut w) = window_cx::<Converter>(cx);
    w.update(|_, _| {});
    (h, w)
}

fn read<R>(cx: &TestAppContext, h: &WindowHandle<Converter>, f: impl FnOnce(&Converter) -> R) -> R {
    cx.read(|cx| f(h.read(cx).unwrap()))
}

fn noisy(w: u32, h: u32) -> Bitmap {
    let mut b = Bitmap::new(w, h);
    for y in 0..h {
        for x in 0..w {
            b.set_pixel(x, y, rgb(((x * 7) ^ (y * 3)) as u8, (x + y) as u8, (y * 5) as u8));
        }
    }
    b
}

#[test]
fn paste_png_and_convert_to_webp() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let (h, mut w) = open(cx, Vec::new());
    assert!(has(&mut w, "drop-zone"), "empty state invites a drop or paste");
    put_image_on_clipboard(cx, &noisy(300, 200));
    click(&mut w, "paste");
    let (name, info) = read(cx, &h, |c| (c.items[0].name.clone(), c.items[0].info.clone().unwrap()));
    assert_eq!(name, "Pasted image 1");
    assert_eq!(
        (info.format, info.width, info.height),
        (Some(ImageFormat::Png), 300, 200),
        "detected from the bytes"
    );
    assert!(!has(&mut w, "drop-zone"));

    assert_eq!(read(cx, &h, |c| c.opts.format), ImageFormat::WebP, "WebP by default");
    click(&mut w, "convert");
    let Status::Done { path, size } = read(cx, &h, |c| c.items[0].status.clone()) else {
        panic!("converted")
    };
    assert!(
        path.starts_with(&fx.paths.default_save),
        "pasted images go to the OShot folder"
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(ImageFormat::detect(&bytes), Some(ImageFormat::WebP));
    assert_eq!(bytes.len() as u64, size);
    assert!(has(&mut w, "reveal-0"), "Show in Finder for the result");
    assert!(matches!(last_notice(cx), Some(Notice::Info(m)) if m == "Converted 1 image"));
}

#[test]
fn finder_service_files_convert_next_to_the_originals() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let a = fx.root.join("one.png");
    let b = fx.root.join("two.jpg");
    std::fs::write(&a, png_bytes(&noisy(120, 80), 1)).unwrap();
    let jpeg = oshot_native::imageio::encode(noisy(64, 64).view(), ImageFormat::Jpeg, 0.9, 1).unwrap();
    std::fs::write(&b, jpeg).unwrap();
    let (h, mut w) = open(cx, vec![a.clone(), b.clone()]);
    assert_eq!(read(cx, &h, |c| c.items.len()), 2);
    assert_eq!(
        read(cx, &h, |c| c.items[1].info.as_ref().unwrap().format),
        Some(ImageFormat::Jpeg)
    );

    click(&mut w, "format-0");
    assert_eq!(read(cx, &h, |c| c.opts.format), ImageFormat::Png);
    assert_eq!(
        settings(cx).converter.format,
        ImageFormat::Png,
        "remembered as the default"
    );
    click(&mut w, "convert");
    assert!(fx.root.join("one 2.png").exists(), "never overwrites the original");
    assert!(fx.root.join("two.png").exists());
    let _ = b;
}

#[test]
fn every_format_quality_size_and_options() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let src = fx.root.join("big.png");
    std::fs::write(&src, png_bytes(&noisy(1200, 800), 2)).unwrap();
    let (h, mut w) = open(cx, vec![src]);

    for (i, f) in ImageFormat::WRITABLE.iter().enumerate() {
        let sel: &'static str = Box::leak(format!("format-{i}").into_boxed_str());
        click(&mut w, sel);
        click(&mut w, "convert");
        let Status::Done { path, .. } = read(cx, &h, |c| c.items[0].status.clone()) else {
            panic!("{f:?}")
        };
        assert_eq!(ImageFormat::detect(&std::fs::read(&path).unwrap()), Some(*f), "{f:?}");
    }

    // Quality slider changes the JPEG size.
    click(&mut w, "format-1");
    let b = w.debug_bounds("quality").unwrap();
    w.simulate_click(gpui::point(b.left() + gpui::px(2.), b.center().y), Modifiers::none());
    assert!(read(cx, &h, |c| c.opts.quality) < 0.05);
    click(&mut w, "convert");
    let Status::Done { size: small, .. } = read(cx, &h, |c| c.items[0].status.clone()) else {
        panic!()
    };
    w.simulate_click(gpui::point(b.right() - gpui::px(2.), b.center().y), Modifiers::none());
    click(&mut w, "convert");
    let Status::Done { size: large, .. } = read(cx, &h, |c| c.items[0].status.clone()) else {
        panic!()
    };
    assert!(small * 2 < large, "low {small} vs high {large}");

    // Max size and Retina-to-1x.
    click(&mut w, "max-size-4");
    click(&mut w, "convert");
    let Status::Done { path, .. } = read(cx, &h, |c| c.items[0].status.clone()) else {
        panic!()
    };
    let i = oshot_native::imageio::info(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((i.width, i.height), (512, 341));
    click(&mut w, "max-size-0");
    click(&mut w, "retina");
    click(&mut w, "convert");
    let Status::Done { path, .. } = read(cx, &h, |c| c.items[0].status.clone()) else {
        panic!()
    };
    let i = oshot_native::imageio::info(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((i.width, i.height), (600, 400), "@2x halved");

    // WebP lossless toggle hides the quality slider.
    click(&mut w, "format-2");
    assert!(has(&mut w, "quality"));
    click(&mut w, "lossless");
    assert!(read(cx, &h, |c| c.opts.lossless));
    assert!(!has(&mut w, "quality"));

    click(&mut w, "strip");
    assert!(!read(cx, &h, |c| c.opts.strip_metadata));
    click(&mut w, "output-1");
    assert_eq!(read(cx, &h, |c| c.opts.output), OutputLocation::SaveFolder);
    click(&mut w, "convert");
    let Status::Done { path, .. } = read(cx, &h, |c| c.items[0].status.clone()) else {
        panic!()
    };
    assert!(path.starts_with(&fx.paths.default_save));
}

#[test]
fn bad_files_remove_clear_and_choose() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let bad = fx.root.join("notes.txt");
    std::fs::write(&bad, b"hello").unwrap();
    let good = fx.root.join("ok.png");
    std::fs::write(&good, png_bytes(&noisy(40, 40), 1)).unwrap();
    let (h, mut w) = open(cx, vec![bad]);
    assert!(read(cx, &h, |c| c.items[0].info.is_err()), "not an image");
    assert_eq!(read(cx, &h, |c| c.pending()), 0, "nothing to convert");
    click(&mut w, "remove-0");
    assert_eq!(read(cx, &h, |c| c.items.len()), 0);

    click(&mut w, "choose");
    let g = good.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![g]));
    cx.run_until_parked();
    assert_eq!(read(cx, &h, |c| c.items.len()), 1, "picked a file");

    // A folder adds every file inside.
    let (_h2, mut w) = window_cx::<Converter>(cx);
    click(&mut w, "add-more");
    let dir = fx.root.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![dir]));
    cx.run_until_parked();
    assert!(read(cx, &h, |c| c.items.len()) >= 2);

    click(&mut w, "clear");
    assert_eq!(read(cx, &h, |c| c.items.len()), 0);
    w.simulate_keystrokes("cmd-v");
    assert!(matches!(last_notice(cx), Some(Notice::Info(m)) if m.contains("no image")));
    w.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(find::<Converter>(cx).is_none());
}
