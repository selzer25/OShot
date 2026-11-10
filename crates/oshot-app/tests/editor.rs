//! The editor: every tool, every inspector control, selection actions,
//! exports and panels, driven through real mouse and keyboard events.

mod support;

use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use oshot_app::editor::{Editor, Panel};
use oshot_app::state::{Command, Notice, Shot};
use oshot_core::bitmap::Bitmap;
use oshot_core::color::{PALETTE, rgb};
use oshot_core::format::ImageFormat;
use oshot_core::geometry::{IRect, Pt, pt, rect};
use oshot_core::model::{ArrowStyle, Shape, Tool};
use support::*;

fn plain() -> Bitmap {
    let mut b = Bitmap::filled(800, 500, rgb(255, 255, 255));
    b.fill_rect(
        IRect {
            x: 600,
            y: 50,
            w: 100,
            h: 60,
        },
        rgb(0, 128, 255),
    );
    b
}

fn open(cx: &mut TestAppContext, bmp: Bitmap, scale: f32) -> (WindowHandle<Editor>, VisualTestContext) {
    cx.update(|cx| oshot_app::editor::open_with(Shot::new(bmp, scale), None, cx));
    cx.run_until_parked();
    let (h, mut w) = window_cx::<Editor>(cx);
    w.update(|_, _| {});
    (h, w)
}

fn screen(cx: &TestAppContext, h: &WindowHandle<Editor>, p: Pt) -> gpui::Point<gpui::Pixels> {
    cx.read(|cx| h.read(cx).unwrap().screen_point(p))
}

fn drag_img(cx: &TestAppContext, w: &mut VisualTestContext, h: &WindowHandle<Editor>, a: Pt, b: Pt, m: Modifiers) {
    let (sa, sb) = (screen(cx, h, a), screen(cx, h, b));
    drag(w, sa, sb, m);
}

fn click_img(cx: &TestAppContext, w: &mut VisualTestContext, h: &WindowHandle<Editor>, a: Pt) {
    let s = screen(cx, h, a);
    w.simulate_mouse_move(s, None, Modifiers::none());
    w.simulate_click(s, Modifiers::none());
    w.run_until_parked();
}

fn read<R>(cx: &TestAppContext, h: &WindowHandle<Editor>, f: impl FnOnce(&Editor) -> R) -> R {
    cx.read(|cx| f(h.read(cx).unwrap()))
}

fn last_shape(cx: &TestAppContext, h: &WindowHandle<Editor>) -> Shape {
    read(cx, h, |e| e.doc.objects.last().expect("an object").shape.clone())
}

fn near(a: Pt, b: Pt) -> bool {
    a.distance(b) < 2.0
}

#[test]
fn polish_arrows_measure_the_background_instead_of_starting_red() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let mut bmp = Bitmap::filled(800, 500, rgb(255, 255, 255));
    bmp.fill_rect(
        IRect {
            x: 400,
            y: 0,
            w: 400,
            h: 500,
        },
        rgb(15, 15, 15),
    );
    let (h, mut w) = open(cx, bmp, 1.0);
    for (x, expected) in [(100.0, rgb(0, 0, 0)), (500.0, rgb(255, 255, 255))] {
        drag_img(cx, &mut w, &h, pt(x, 200.0), pt(x + 150.0, 250.0), Modifiers::none());
        assert_eq!(read(cx, &h, |e| e.doc.objects.last().unwrap().style.color), expected);
    }
    click(&mut w, "color-4");
    drag_img(cx, &mut w, &h, pt(100.0, 350.0), pt(250.0, 350.0), Modifiers::none());
    assert_eq!(read(cx, &h, |e| e.doc.objects.last().unwrap().style.color), PALETTE[4]);
}

#[test]
fn polish_text_preferences_survive_new_editors_and_arrow_labels() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    w.simulate_keystrokes("t");
    click_img(cx, &mut w, &h, pt(100.0, 100.0));
    w.simulate_input("Remember me");
    w.simulate_keystrokes("enter");
    assert!(
        !read(cx, &h, |e| e.active_style().text_background),
        "plain text by default"
    );
    click(&mut w, "bubble");
    click(&mut w, "bubble");
    click(&mut w, "color-4");
    click(&mut w, "hand");
    let b = w.debug_bounds("text-size").unwrap();
    w.simulate_click(b.center(), Modifiers::none());
    w.run_until_parked();
    let expected = read(cx, &h, |e| e.style());
    let persisted = oshot_core::settings::Settings::load(&fx.paths.settings);
    cx.update(|cx| {
        oshot_app::windows::close(h, cx);
        cx.global_mut::<oshot_app::state::AppState>().settings = persisted;
    });
    let (h, mut w) = open(cx, plain(), 2.0);
    click(&mut w, "color-0");
    drag_img(cx, &mut w, &h, pt(400.0, 300.0), pt(600.0, 300.0), Modifiers::none());
    w.simulate_keystrokes("t");
    w.simulate_input("Independent");
    w.simulate_keystrokes("enter");
    let actual = read(cx, &h, |e| e.active_style());
    assert_eq!(actual.color, PALETTE[4], "not the red arrow colour");
    assert_eq!(actual.text_background, expected.text_background);
    assert_eq!(actual.hand_drawn, expected.hand_drawn);
    assert!(
        (actual.text_size - expected.text_size * 2.0).abs() < 0.01,
        "size restored in points"
    );
}

#[test]
fn polish_arrow_labels_at_the_top_are_not_clipped() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    drag_img(cx, &mut w, &h, pt(200.0, 5.0), pt(400.0, 200.0), Modifiers::none());
    w.simulate_keystrokes("t");
    w.simulate_input("Entire label");
    w.simulate_keystrokes("enter");
    let bounds = read(cx, &h, |e| e.doc.objects.last().unwrap().bounds());
    assert!(bounds.y >= 0.0, "label must not be cut off: {bounds:?}");
    click(&mut w, "copy");
    let (img, _) = clipboard_image(cx).unwrap();
    let area = bounds.to_pixels(img.width, img.height);
    assert!(
        (area.y..area.y + area.h)
            .any(|y| (area.x..area.x + area.w).any(|x| img.pixel(x, y).unwrap() != rgb(255, 255, 255))),
        "label exported"
    );
}

#[test]
fn polish_command_c_closes_only_the_copied_editor_after_copying() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    w.simulate_keystrokes("r");
    drag_img(cx, &mut w, &h, pt(100.0, 100.0), pt(200.0, 200.0), Modifiers::none());
    let other = cx
        .update(|cx| oshot_app::editor::open_with(Shot::new(plain(), 1.0), None, cx))
        .unwrap();
    w.simulate_keystrokes("cmd-c");
    cx.run_until_parked();
    let (bmp, _) = clipboard_image(cx).expect("image copied before closing");
    assert_eq!(bmp.pixel(150, 100), Some(PALETTE[0]));
    assert!(cx.read(|cx| h.read(cx).is_err()), "Command+C closes by default");
    assert!(cx.read(|cx| other.read(cx).is_ok()), "other editor stays open");
}

#[test]
fn polish_copy_shortcut_can_stay_open_and_the_button_never_closes() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    click(&mut w, "copy");
    assert!(clipboard_image(cx).is_some());
    assert!(find::<Editor>(cx).is_some(), "Copy button stays open");

    cx.update(|cx| oshot_app::commands::dispatch(Command::Settings, cx));
    cx.run_until_parked();
    let (_, mut prefs) = window_cx::<oshot_app::settings_window::SettingsWindow>(cx);
    click(&mut prefs, "close-on-copy-shortcut");
    assert!(!oshot_core::settings::Settings::load(&fx.paths.settings).close_on_copy_shortcut);
    w.simulate_keystrokes("r");
    drag_img(cx, &mut w, &h, pt(100.0, 100.0), pt(200.0, 200.0), Modifiers::none());
    w.simulate_keystrokes("cmd-c");
    let (bmp, _) = clipboard_image(cx).unwrap();
    assert_eq!(bmp.pixel(150, 100), Some(PALETTE[0]), "new edits copied");
    assert!(find::<Editor>(cx).is_some(), "disabled setting keeps the editor open");
}

#[test]
fn every_drag_tool_draws_its_shape() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    let (a, b) = (pt(100.0, 100.0), pt(300.0, 220.0));
    for (key, tool) in [
        ("a", Tool::Arrow),
        ("l", Tool::Line),
        ("r", Tool::Rect),
        ("o", Tool::Oval),
        ("h", Tool::Highlighter),
        ("u", Tool::Ruler),
        ("p", Tool::Pixelate),
        ("b", Tool::Blur),
        ("e", Tool::Erase),
        ("s", Tool::Spotlight),
    ] {
        w.simulate_keystrokes(key);
        assert_eq!(read(cx, &h, |e| e.tool), tool, "key {key}");
        let before = read(cx, &h, |e| e.doc.objects.len());
        drag_img(cx, &mut w, &h, a, b, Modifiers::none());
        assert_eq!(
            read(cx, &h, |e| e.doc.objects.len()),
            before + 1,
            "{tool:?} made an object"
        );
        let shape = last_shape(cx, &h);
        assert_eq!(shape.tool(), tool);
        if let Some((from, to)) = shape.endpoints() {
            assert!(near(from, a) && near(to, b), "{tool:?}: {from:?} → {to:?}");
        }
        if let Some(r) = shape.rect() {
            assert!(
                near(pt(r.x, r.y), a) && near(pt(r.right(), r.bottom()), b),
                "{tool:?}: {r:?}"
            );
        }
    }
    // Magnifier: drag from the detail to where the callout goes.
    w.simulate_keystrokes("m");
    drag_img(cx, &mut w, &h, pt(650.0, 80.0), pt(400.0, 350.0), Modifiers::none());
    let Shape::Magnifier { source, center, radius } = last_shape(cx, &h) else {
        panic!("magnifier")
    };
    assert!(near(source.center(), pt(650.0, 80.0)) && near(center, pt(400.0, 350.0)));
    assert!((radius * 2.0 / source.w - 3.0).abs() < 0.01, "3x by default");
    // Every effect computed something to show.
    read(cx, &h, |e| assert_eq!(e.doc.objects.len(), 11));
}

#[test]
fn freehand_counter_and_text() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    w.simulate_keystrokes("f");
    drag_img(cx, &mut w, &h, pt(50.0, 50.0), pt(250.0, 150.0), Modifiers::none());
    let Shape::Freehand { points } = last_shape(cx, &h) else {
        panic!()
    };
    assert!(points.len() >= 5, "records the path: {}", points.len());

    w.simulate_keystrokes("n");
    for x in [100.0, 150.0, 200.0] {
        click_img(cx, &mut w, &h, pt(x, 300.0));
    }
    let numbers: Vec<u32> = read(cx, &h, |e| {
        e.doc
            .objects
            .iter()
            .filter_map(|o| match o.shape {
                Shape::Counter { number, .. } => Some(number),
                _ => None,
            })
            .collect()
    });
    assert_eq!(numbers, vec![1, 2, 3]);

    w.simulate_keystrokes("t");
    click_img(cx, &mut w, &h, pt(400.0, 400.0));
    assert!(read(cx, &h, |e| e.editing_text.is_some()));
    w.simulate_input("Hello");
    w.simulate_keystrokes("shift-enter");
    w.simulate_input("World");
    let Shape::Text { text, extent, .. } = last_shape(cx, &h) else {
        panic!()
    };
    assert_eq!(text, "Hello\nWorld");
    assert!(extent.x > 0.0 && extent.y > 0.0, "measured for selection");
    w.simulate_keystrokes("backspace");
    w.simulate_keystrokes("enter");
    assert!(read(cx, &h, |e| e.editing_text.is_none()), "↩ finishes");
    let Shape::Text { text, .. } = last_shape(cx, &h) else {
        panic!()
    };
    assert_eq!(text, "Hello\nWorl");

    // An empty label is discarded when you click away.
    let count = read(cx, &h, |e| e.doc.objects.len());
    click_img(cx, &mut w, &h, pt(600.0, 300.0));
    w.simulate_keystrokes("escape");
    assert_eq!(read(cx, &h, |e| e.doc.objects.len()), count);
}

#[test]
fn tiny_drags_draw_nothing_and_shift_constrains() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    w.simulate_keystrokes("r");
    drag_img(cx, &mut w, &h, pt(100.0, 100.0), pt(101.0, 101.0), Modifiers::none());
    assert!(
        read(cx, &h, |e| e.doc.objects.is_empty() && !e.doc.can_undo()),
        "no object and no undo step"
    );

    drag_img(
        cx,
        &mut w,
        &h,
        pt(100.0, 100.0),
        pt(300.0, 160.0),
        Modifiers {
            shift: true,
            ..Modifiers::none()
        },
    );
    let r = last_shape(cx, &h).rect().unwrap();
    assert!((r.w - r.h).abs() < 0.5, "⇧ makes a square: {r:?}");

    w.simulate_keystrokes("l");
    drag_img(
        cx,
        &mut w,
        &h,
        pt(100.0, 300.0),
        pt(300.0, 312.0),
        Modifiers {
            shift: true,
            ..Modifiers::none()
        },
    );
    let (a, b) = last_shape(cx, &h).endpoints().unwrap();
    assert!((a.y - b.y).abs() < 0.5, "⇧ snaps to horizontal");
}

#[test]
fn select_move_resize_delete_undo_redo() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    w.simulate_keystrokes("r");
    drag_img(cx, &mut w, &h, pt(100.0, 100.0), pt(300.0, 200.0), Modifiers::none());
    w.simulate_keystrokes("v");
    assert_eq!(read(cx, &h, |e| e.tool), Tool::Select);

    // Click empty space deselects, click the edge selects.
    click_img(cx, &mut w, &h, pt(500.0, 400.0));
    assert_eq!(read(cx, &h, |e| e.selected), None);
    click_img(cx, &mut w, &h, pt(200.0, 100.0));
    let id = read(cx, &h, |e| e.selected).expect("selected by clicking its edge");

    // Drag the edge (away from its handles) to move it.
    drag_img(cx, &mut w, &h, pt(150.0, 100.0), pt(200.0, 140.0), Modifiers::none());
    assert_eq!(
        last_shape(cx, &h).rect().map(|r| (r.x.round(), r.y.round())),
        Some((150.0, 140.0))
    );

    // Drag the bottom-right handle to resize.
    drag_img(cx, &mut w, &h, pt(350.0, 240.0), pt(400.0, 300.0), Modifiers::none());
    assert_eq!(
        last_shape(cx, &h).rect().map(|r| (r.w.round(), r.h.round())),
        Some((250.0, 160.0))
    );

    // Arrow keys nudge.
    w.simulate_keystrokes("right shift-down");
    assert_eq!(
        last_shape(cx, &h).rect().map(|r| (r.x.round(), r.y.round())),
        Some((151.0, 150.0))
    );

    // Duplicate, delete, undo, redo.
    w.simulate_keystrokes("cmd-d");
    assert_eq!(read(cx, &h, |e| e.doc.objects.len()), 2);
    w.simulate_keystrokes("backspace");
    assert_eq!(read(cx, &h, |e| e.doc.objects.len()), 1);
    w.simulate_keystrokes("cmd-z");
    assert_eq!(read(cx, &h, |e| e.doc.objects.len()), 2);
    click(&mut w, "undo");
    assert_eq!(read(cx, &h, |e| e.doc.objects.len()), 1);
    click(&mut w, "redo");
    assert_eq!(read(cx, &h, |e| e.doc.objects.len()), 2);
    w.simulate_keystrokes("cmd-shift-z");
    let _ = id;
}

#[test]
fn inspector_controls_change_the_selected_object() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    drag_img(cx, &mut w, &h, pt(100.0, 100.0), pt(400.0, 200.0), Modifiers::none());
    let style = |cx: &TestAppContext| read(cx, &h, |e| e.doc.objects.last().unwrap().style.clone());
    let original = style(cx);

    click(&mut w, "color-4");
    assert_eq!(style(cx).color, PALETTE[4]);
    click(&mut w, "arrow-style-2");
    assert_eq!(style(cx).arrow, ArrowStyle::Double);
    click(&mut w, "arrow-style-1");
    assert_eq!(style(cx).arrow, ArrowStyle::Line);
    click(&mut w, "hand");
    assert!(style(cx).hand_drawn);
    let before = style(cx).width;
    let b = w.debug_bounds("width").unwrap();
    w.simulate_click(gpui::point(b.right() - gpui::px(2.), b.center().y), Modifiers::none());
    assert!(style(cx).width > before * 4.0, "slider to the right makes it thick");
    // One undo reverts the whole run of style edits.
    w.simulate_keystrokes("cmd-z");
    assert_eq!(style(cx).color, original.color);

    // Next arrows use the new style.
    click(&mut w, "color-3");
    drag_img(cx, &mut w, &h, pt(100.0, 300.0), pt(400.0, 350.0), Modifiers::none());
    assert_eq!(style(cx).color, PALETTE[3]);

    // Rectangle fill, text bubble and size, effect strength, spotlight shape, magnifier zoom.
    w.simulate_keystrokes("r");
    drag_img(cx, &mut w, &h, pt(450.0, 300.0), pt(550.0, 400.0), Modifiers::none());
    click(&mut w, "fill");
    assert!(style(cx).fill);

    w.simulate_keystrokes("t");
    click_img(cx, &mut w, &h, pt(50.0, 420.0));
    w.simulate_input("Label");
    w.simulate_keystrokes("enter");
    assert!(!style(cx).text_background);
    click(&mut w, "bubble");
    assert!(style(cx).text_background);
    click(&mut w, "bubble");
    assert!(!style(cx).text_background);
    let b = w.debug_bounds("text-size").unwrap();
    w.simulate_click(gpui::point(b.right() - gpui::px(2.), b.center().y), Modifiers::none());
    assert!(style(cx).text_size > 80.0);

    w.simulate_keystrokes("p");
    drag_img(cx, &mut w, &h, pt(600.0, 50.0), pt(700.0, 110.0), Modifiers::none());
    let b = w.debug_bounds("strength").unwrap();
    w.simulate_click(gpui::point(b.left() + gpui::px(1.), b.center().y), Modifiers::none());
    assert!(style(cx).strength <= 4.5, "smallest blocks");

    w.simulate_keystrokes("s");
    drag_img(cx, &mut w, &h, pt(50.0, 50.0), pt(150.0, 90.0), Modifiers::none());
    click(&mut w, "spot-shape-1");
    assert!(style(cx).ellipse);

    w.simulate_keystrokes("m");
    drag_img(cx, &mut w, &h, pt(650.0, 80.0), pt(400.0, 300.0), Modifiers::none());
    click(&mut w, "mag-zoom-3");
    assert_eq!(style(cx).strength, 6.0);

    // Duplicate and delete buttons.
    let n = read(cx, &h, |e| e.doc.objects.len());
    click(&mut w, "duplicate");
    assert_eq!(read(cx, &h, |e| e.doc.objects.len()), n + 1);
    click(&mut w, "delete");
    assert_eq!(read(cx, &h, |e| e.doc.objects.len()), n);
}

#[test]
fn selection_crop_blur_erase_fit_and_colour() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    w.simulate_keystrokes("v");
    drag_img(cx, &mut w, &h, pt(550.0, 20.0), pt(750.0, 140.0), Modifiers::none());
    assert!(has(&mut w, "sel-crop"));
    click(&mut w, "sel-fit");
    assert_eq!(
        read(cx, &h, |e| e.selection),
        Some(rect(600.0, 50.0, 100.0, 60.0)),
        "fit snaps to the blue block"
    );
    click(&mut w, "sel-color");
    assert_eq!(clipboard_text(cx).as_deref(), Some("#0080FF"));

    click(&mut w, "sel-blur");
    assert!(matches!(last_shape(cx, &h), Shape::Blur { .. }));

    drag_img(cx, &mut w, &h, pt(10.0, 10.0), pt(110.0, 110.0), Modifiers::none());
    w.simulate_keystrokes("p");
    assert!(
        matches!(last_shape(cx, &h), Shape::Pixelate { .. }),
        "P with a selection pixelates it"
    );
    assert_eq!(read(cx, &h, |e| e.tool), Tool::Select, "and keeps the select tool");

    drag_img(cx, &mut w, &h, pt(200.0, 200.0), pt(260.0, 260.0), Modifiers::none());
    w.simulate_keystrokes("backspace");
    assert!(
        matches!(last_shape(cx, &h), Shape::Erase { .. }),
        "delete with a selection erases it"
    );

    drag_img(cx, &mut w, &h, pt(300.0, 280.0), pt(580.0, 450.0), Modifiers::none());
    w.simulate_keystrokes("]");
    assert_eq!(
        read(cx, &h, |e| e.selection.unwrap().w),
        282.0,
        "] grows by a pixel each side"
    );
    w.simulate_keystrokes("enter");
    assert_eq!(read(cx, &h, |e| e.doc.crop), Some(rect(299.0, 279.0, 282.0, 172.0)));
    click(&mut w, "reset-crop");
    assert_eq!(read(cx, &h, |e| e.doc.crop), None);

    // ⌘-click selects a flat-coloured region in one go.
    let s = screen(cx, &h, pt(650.0, 80.0));
    w.simulate_click(
        s,
        Modifiers {
            platform: true,
            ..Modifiers::none()
        },
    );
    assert_eq!(read(cx, &h, |e| e.selection), Some(rect(600.0, 50.0, 100.0, 60.0)));
}

#[test]
fn copy_save_and_export_targets() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    w.simulate_keystrokes("r");
    click(&mut w, "color-0");
    drag_img(cx, &mut w, &h, pt(100.0, 100.0), pt(300.0, 200.0), Modifiers::none());

    click(&mut w, "copy");
    let (img, _) = clipboard_image(cx).expect("copied");
    assert_eq!((img.width, img.height), (800, 500));
    assert_eq!(img.pixel(200, 100), Some(PALETTE[0]), "the rectangle is in the copy");
    assert_eq!(img.pixel(200, 150), Some(rgb(255, 255, 255)));

    click(&mut w, "save");
    let saved = files_in(&fx.paths.default_save);
    assert_eq!(saved.len(), 1);
    assert!(matches!(last_notice(cx), Some(Notice::Saved(_))));

    click(&mut w, "export");
    assert_eq!(read(cx, &h, |e| e.panel), Some(Panel::Export));
    for (sel, f) in [
        ("export-png", ImageFormat::Png),
        ("export-jpg", ImageFormat::Jpeg),
        ("export-webp", ImageFormat::WebP),
        ("export-heic", ImageFormat::Heic),
        ("export-avif", ImageFormat::Avif),
        ("export-tiff", ImageFormat::Tiff),
        ("export-gif", ImageFormat::Gif),
    ] {
        click(&mut w, sel);
        let Some(Notice::Saved(path)) = last_notice(cx) else {
            panic!("{sel} saved")
        };
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(ImageFormat::detect(&bytes), Some(f), "{sel}");
        assert_eq!(path.extension().unwrap(), f.extension());
    }
    click(&mut w, "copy-file");
    assert_eq!(fx.platform.calls().copied_files.len(), 1);
    assert!(fx.platform.calls().copied_files[0].exists());
    click(&mut w, "copy-path");
    let path = clipboard_text(cx).unwrap();
    assert!(std::path::Path::new(&path).exists(), "{path}");
    click(&mut w, "copy-base64");
    assert!(clipboard_text(cx).unwrap().starts_with("data:image/png;base64,iVBOR"));

    // Selection copies only the selection.
    w.simulate_keystrokes("v");
    drag_img(cx, &mut w, &h, pt(0.0, 0.0), pt(200.0, 100.0), Modifiers::none());
    w.simulate_keystrokes("cmd-c");
    let (img, _) = clipboard_image(cx).unwrap();
    assert_eq!((img.width, img.height), (200, 100));
}

#[test]
fn save_as_uses_the_chosen_name_and_format() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let (_h, mut w) = open(cx, plain(), 1.0);
    let target = fx.root.join("picked.webp");
    let t2 = target.clone();
    w.simulate_keystrokes("cmd-shift-s");
    cx.simulate_new_path_selection(move |_| Some(t2));
    cx.run_until_parked();
    let bytes = std::fs::read(&target).expect("written where the user chose");
    assert_eq!(ImageFormat::detect(&bytes), Some(ImageFormat::WebP));
}

#[test]
fn resize_and_backdrop_panels() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 2.0);
    click(&mut w, "resize");
    click(&mut w, "scale-50");
    click(&mut w, "copy");
    let (img, _) = clipboard_image(cx).unwrap();
    assert_eq!((img.width, img.height), (400, 250));
    click(&mut w, "scale-standard");
    assert_eq!(read(cx, &h, |e| e.export_scale), 0.5, "Retina to 1x");
    click(&mut w, "scale-100");

    click(&mut w, "backdrop");
    assert!(has(&mut w, "side-panel"));
    click(&mut w, "backdrop-on");
    assert!(read(cx, &h, |e| e.doc.backdrop.enabled));
    let pad = read(cx, &h, |e| e.doc.backdrop.padding);
    assert_eq!(
        read(cx, &h, |e| e.doc.export_size()),
        ((800.0 + pad * 2.0) as u32, (500.0 + pad * 2.0) as u32)
    );
    click(&mut w, "preset-3");
    assert_eq!(read(cx, &h, |e| e.doc.backdrop.preset), 3);
    for slider in ["backdrop-padding", "backdrop-radius", "backdrop-shadow"] {
        let b = w.debug_bounds(slider).unwrap();
        w.simulate_click(
            gpui::point(b.left() + b.size.width * 0.25, b.center().y),
            Modifiers::none(),
        );
    }
    let bd = read(cx, &h, |e| e.doc.backdrop.clone());
    assert!(bd.padding > 80.0 && bd.padding < 120.0, "{bd:?}");
    assert!(bd.shadow > 0.2 && bd.shadow < 0.3);
    click(&mut w, "copy");
    let (img, _) = clipboard_image(cx).unwrap();
    assert_eq!(img.width, 800 + (bd.padding * 2.0) as u32);
    let corner = img.pixel(2, 2).unwrap();
    assert_ne!(corner, rgb(255, 255, 255), "gradient behind the screenshot");
    w.simulate_keystrokes("cmd-z");
    assert!(
        read(cx, &h, |e| e.doc.backdrop.shadow != bd.shadow),
        "backdrop edits undo"
    );
}

#[test]
fn paste_overlay_opacity_and_before_after_gif() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    click(&mut w, "export");
    click(&mut w, "gif-panel");
    assert!(
        matches!(last_notice(cx), Some(Notice::Info(m)) if m.contains("Paste")),
        "needs an overlay first"
    );

    put_image_on_clipboard(cx, &Bitmap::filled(200, 100, rgb(255, 0, 0)));
    w.simulate_keystrokes("cmd-v");
    assert!(matches!(last_shape(cx, &h), Shape::Image { .. }));
    assert_eq!(read(cx, &h, |e| e.tool), Tool::Select);
    let b = w.debug_bounds("overlay-opacity").unwrap();
    w.simulate_click(
        gpui::point(b.left() + b.size.width * 0.5, b.center().y),
        Modifiers::none(),
    );
    let Shape::Image { opacity, .. } = last_shape(cx, &h) else {
        panic!()
    };
    assert!((opacity - 0.525).abs() < 0.05, "{opacity}");
    click(&mut w, "gif");
    let Some(Notice::Saved(path)) = last_notice(cx) else {
        panic!("gif saved")
    };
    let info = oshot_native::imageio::info(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((info.format, info.frames), (Some(ImageFormat::Gif), 2));
    assert!(path.starts_with(&fx.paths.default_save));
    click(&mut w, "delete-overlay");
    assert!(read(cx, &h, |e| e.doc.objects.is_empty()));
}

#[test]
fn ocr_redact_and_pin() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let shot = desktop().view().crop(IRect {
        x: 400,
        y: 300,
        w: 1400,
        h: 900,
    });
    let (h, mut w) = open(cx, shot, 2.0);
    click(&mut w, "ocr");
    assert!(clipboard_text(cx).unwrap().contains("example.com"));
    click(&mut w, "redact");
    let shapes = read(cx, &h, |e| {
        e.doc.objects.iter().map(|o| o.shape.clone()).collect::<Vec<_>>()
    });
    assert_eq!(shapes.len(), 1, "one email found");
    let Shape::Pixelate { rect } = shapes[0] else { panic!() };
    assert!(
        rect.x > 200.0 && rect.y > 60.0 && rect.bottom() < 190.0,
        "covers the email line: {rect:?}"
    );
    assert!(matches!(last_notice(cx), Some(Notice::Info(m)) if m.contains("email")));
    click(&mut w, "pin");
    assert!(find::<oshot_app::pin::Pin>(cx).is_some());
}

#[test]
fn zoom_colour_picker_ruler_and_escape() {
    let cx = &mut real_cx();
    let _fx = start_with(
        cx,
        oshot_core::settings::Settings {
            onboarding_done: true,
            escape_copies: true,
            ..Default::default()
        },
    );
    let (h, mut w) = open(cx, plain(), 1.0);
    let z0 = read(cx, &h, |e| e.zoom_percent());
    click(&mut w, "zoom-in");
    assert!(read(cx, &h, |e| e.zoom_percent()) > z0 * 1.2);
    click(&mut w, "zoom-out");
    click(&mut w, "zoom-out");
    assert!(read(cx, &h, |e| e.zoom_percent()) < z0);
    w.simulate_keystrokes("cmd-0");
    assert!((read(cx, &h, |e| e.zoom_percent()) - 100.0).abs() < 0.01);
    click(&mut w, "zoom-fit");
    assert!((read(cx, &h, |e| e.zoom_percent()) - z0).abs() < 0.01);

    w.simulate_keystrokes("i");
    click_img(cx, &mut w, &h, pt(650.0, 80.0));
    assert_eq!(clipboard_text(cx).as_deref(), Some("#0080FF"));
    let s = screen(cx, &h, pt(20.0, 20.0));
    w.simulate_mouse_move(s, None, Modifiers::none());
    w.simulate_keystrokes("tab");
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("#FFFFFF"),
        "Tab copies the colour under the pointer"
    );

    w.simulate_keystrokes("u");
    assert_eq!(
        read(cx, &h, |e| e.measure_at(pt(300.0, 80.0)).map(|m| m.0.w)),
        Some(600.0),
        "edge to the blue block"
    );

    w.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(find::<Editor>(cx).is_none(), "Esc closes");
    assert!(
        clipboard_image(cx).is_some(),
        "and copies first when that setting is on"
    );
}

#[test]
fn open_file_and_clipboard_images() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let path = fx.root.join("in.png");
    std::fs::write(&path, png_bytes(&plain(), 2)).unwrap();
    cx.update(|cx| oshot_app::commands::dispatch(Command::OpenFile(path), cx));
    cx.run_until_parked();
    let e = find::<Editor>(cx).expect("opened");
    assert_eq!(
        cx.read(|cx| {
            let e = e.read(cx).unwrap();
            (e.shot.width, e.shot.scale)
        }),
        (800, 2.0)
    );
    cx.update(|cx| oshot_app::windows::close(e, cx));

    cx.update(|cx| oshot_app::commands::dispatch(Command::PasteImage, cx));
    assert!(matches!(last_notice(cx), Some(Notice::Info(m)) if m.contains("no image")));
    put_image_on_clipboard(cx, &plain());
    cx.update(|cx| oshot_app::commands::dispatch(Command::PasteImage, cx));
    cx.run_until_parked();
    assert!(find::<Editor>(cx).is_some());

    cx.update(|cx| oshot_app::commands::dispatch(Command::OpenFile(fx.root.join("missing.png")), cx));
    cx.run_until_parked();
    assert!(matches!(last_notice(cx), Some(Notice::Error(_))));
}

#[test]
fn large_images_draw_a_preview_until_zoomed_in() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, Bitmap::filled(6000, 4000, rgb(200, 200, 200)), 2.0);
    let f = read(cx, &h, |e| e.display_factor());
    assert!(f >= 2, "fit-to-window draws a downscaled copy (factor {f})");
    w.simulate_keystrokes("cmd-0");
    w.update(|_, _| {});
    assert_eq!(read(cx, &h, |e| e.display_factor()), 1, "100% draws the full image");
    w.simulate_keystrokes("cmd-1");
    w.update(|_, _| {});
    assert!(
        read(cx, &h, |e| e.display_factor()) >= 2,
        "zooming back out releases it"
    );
}

#[test]
fn shapes_keep_their_on_screen_size_on_huge_images() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, Bitmap::filled(6000, 6000, rgb(200, 200, 200)), 1.0);
    let zoom = read(cx, &h, |e| e.view().zoom);
    w.simulate_keystrokes("l");
    drag_img(
        cx,
        &mut w,
        &h,
        pt(1000.0, 1000.0),
        pt(4000.0, 3000.0),
        Modifiers::none(),
    );
    let width = read(cx, &h, |e| e.doc.objects.last().unwrap().style.width);
    let on_screen = width * zoom;
    assert!(
        (3.0..=5.0).contains(&on_screen),
        "about 4 pt on screen, got {on_screen:.2} (width {width}, zoom {zoom})"
    );
}

#[test]
fn t_after_an_arrow_types_a_plain_label_at_its_tail() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    let (tail, head) = (pt(400.0, 300.0), pt(600.0, 300.0));
    w.simulate_keystrokes("a");
    drag_img(cx, &mut w, &h, tail, head, Modifiers::none());
    let arrow = read(cx, &h, |e| e.selected).expect("the new arrow is selected");

    w.simulate_keystrokes("t");
    assert_eq!(read(cx, &h, |e| e.tool), Tool::Text);
    let label = read(cx, &h, |e| e.editing_text).expect("typing straight away");
    w.simulate_input("Login");
    w.simulate_keystrokes("enter");
    let (shape, color, bounds) = read(cx, &h, |e| {
        let o = e.doc.get(label).unwrap();
        (o.shape.clone(), o.style.color, o.bounds())
    });
    let Shape::Text {
        text, arrow: linked, ..
    } = shape
    else {
        panic!("a text label")
    };
    assert_eq!((text.as_str(), linked), ("Login", Some(arrow)));
    assert_eq!(
        color,
        read(cx, &h, |e| e.doc.get(arrow).unwrap().style.color),
        "arrow colour"
    );
    assert!(
        (bounds.right() - tail.x).abs() < 0.5 && (bounds.center().y - tail.y).abs() < 0.5,
        "label sits just before the tail, on the arrow's line: {bounds:?}"
    );

    // Plain labels have no coloured background.
    click(&mut w, "copy");
    let (img, _) = clipboard_image(cx).unwrap();
    let c = img.pixel((bounds.x + 3.0) as u32, bounds.center().y as u32).unwrap();
    assert_eq!(c, rgb(0, 0, 0), "opaque text, not a translucent bubble");
    assert_eq!(img.pixel(bounds.x as u32, bounds.y as u32), Some(rgb(255, 255, 255)));

    // Dragging the label moves the arrow's tail with it; the head stays.
    w.simulate_keystrokes("v");
    let grab = bounds.center();
    drag_img(cx, &mut w, &h, grab, pt(grab.x, grab.y + 100.0), Modifiers::none());
    let Some((from, to)) = read(cx, &h, |e| e.doc.get(arrow).unwrap().shape.endpoints()) else {
        panic!()
    };
    assert!(
        near(from, pt(tail.x, tail.y + 100.0)) && near(to, head),
        "{from:?} → {to:?}"
    );
    w.update(|_, _| {});
    let moved = read(cx, &h, |e| e.doc.get(label).unwrap().bounds());
    let dir = pt(from.x - to.x, from.y - to.y);
    let len = (dir.x * dir.x + dir.y * dir.y).sqrt();
    let c = moved.center();
    let along = ((c.x - from.x) * dir.x + (c.y - from.y) * dir.y) / len;
    let across = ((c.x - from.x) * dir.y - (c.y - from.y) * dir.x).abs() / len;
    assert!(along > 0.0 && across < 1.0, "still in front of the tail: {moved:?}");

    // T with nothing typed leaves no label behind.
    w.simulate_keystrokes("a");
    drag_img(cx, &mut w, &h, pt(100.0, 400.0), pt(200.0, 450.0), Modifiers::none());
    let count = read(cx, &h, |e| e.doc.objects.len());
    w.simulate_keystrokes("t");
    w.simulate_keystrokes("escape");
    assert_eq!(read(cx, &h, |e| e.doc.objects.len()), count);
}

#[test]
fn drawings_stay_inside_the_image_unless_the_canvas_is_extended() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    w.simulate_keystrokes("a");
    drag_img(cx, &mut w, &h, pt(200.0, 200.0), pt(-60.0, 200.0), Modifiers::none());
    let tip = last_shape(cx, &h).endpoints().unwrap().1;
    assert!(tip.x < -10.0, "drawn past the left edge: {tip:?}");
    assert_eq!(
        read(cx, &h, |e| e.doc.export_size()),
        (800, 500),
        "the image does not grow"
    );
    click(&mut w, "copy");
    let (img, _) = clipboard_image(cx).unwrap();
    assert_eq!((img.width, img.height), (800, 500));
    assert_eq!(
        img.pixel(2, 200).map(|c| c.g < 100),
        Some(true),
        "the arrow is cut at the edge"
    );

    click(&mut w, "extend-canvas");
    let (ew, eh) = read(cx, &h, |e| e.doc.export_size());
    assert!(
        ew as f32 >= 800.0 - tip.x && eh >= 500,
        "extended to the arrowhead: {ew} × {eh}"
    );
    click(&mut w, "copy");
    let (img, _) = clipboard_image(cx).unwrap();
    assert_eq!((img.width, img.height), (ew, eh));

    click(&mut w, "extend-canvas");
    assert_eq!(read(cx, &h, |e| e.doc.export_size()), (800, 500), "toggles back");
}

#[test]
fn numbered_arrows_count_with_step_counters() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx, plain(), 1.0);
    let number_of = |cx: &TestAppContext, id: u64| {
        read(cx, &h, |e| match e.doc.get(id).unwrap().shape {
            Shape::Arrow { number, .. } => number,
            _ => panic!("an arrow"),
        })
    };
    w.simulate_keystrokes("a");
    click(&mut w, "numbered");
    drag_img(cx, &mut w, &h, pt(100.0, 100.0), pt(250.0, 160.0), Modifiers::none());
    let first = read(cx, &h, |e| e.selected).unwrap();
    assert_eq!(number_of(cx, first), Some(1));

    w.simulate_keystrokes("n");
    click_img(cx, &mut w, &h, pt(400.0, 100.0));
    assert!(matches!(last_shape(cx, &h), Shape::Counter { number: 2, .. }));

    // The toggle is remembered for the next arrow, across tool changes.
    w.simulate_keystrokes("a");
    let tail = pt(300.0, 350.0);
    drag_img(cx, &mut w, &h, tail, pt(500.0, 350.0), Modifiers::none());
    let second = read(cx, &h, |e| e.selected).unwrap();
    assert_eq!(number_of(cx, second), Some(3), "one sequence for arrows and counters");

    // "3. this is misaligned": the label sits past the badge.
    w.simulate_keystrokes("t");
    w.simulate_input("this is misaligned");
    w.simulate_keystrokes("enter");
    let label = read(cx, &h, |e| e.doc.objects.last().unwrap().bounds());
    let r = read(cx, &h, |e| e.counter_radius());
    assert!(label.right() < tail.x - r, "label clear of the badge: {label:?}, r {r}");
    assert!((label.center().y - tail.y).abs() < 0.5);

    click(&mut w, "copy");
    let (img, _) = clipboard_image(cx).unwrap();
    let badge = img.pixel(tail.x as u32, (tail.y - r * 0.7) as u32).unwrap();
    assert_eq!(badge, rgb(0, 0, 0), "contrasting badge at the tail");

    // Turning it off on a selected arrow removes its number.
    w.simulate_keystrokes("v");
    click_img(cx, &mut w, &h, pt(400.0, 350.0));
    assert_eq!(read(cx, &h, |e| e.selected), Some(second));
    click(&mut w, "numbered");
    assert_eq!(number_of(cx, second), None);
    w.simulate_keystrokes("cmd-z");
    assert_eq!(number_of(cx, second), Some(3), "undoable");
}
