//! Capture flows from a hotkey to the clipboard, a file, the editor or a
//! thumbnail, plus window, text, colour and ruler modes.

mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui::{Modifiers, TestAppContext, px};
use oshot_app::commands::handle_event;
use oshot_app::editor::Editor;
use oshot_app::hud::Hud;
use oshot_app::onboarding::{Item, Onboarding};
use oshot_app::overlay::{Mode, Overlay};
use oshot_app::state::{Command, MenuEntry, Notice, PlatformEvent};
use oshot_app::thumbnail::Thumbnail;
use oshot_core::color::rgb;
use oshot_core::format::ImageFormat;
use oshot_core::geometry::rect;
use oshot_core::gesture::MenuGesture;
use oshot_core::settings::{AfterCapture, HotkeyAction, Settings};
use support::*;

fn hotkey(cx: &mut TestAppContext, action: HotkeyAction) {
    cx.update(|cx| handle_event(PlatformEvent::Hotkey(action), cx));
    cx.run_until_parked();
}

fn command(cx: &mut TestAppContext, c: Command) {
    cx.update(|cx| handle_event(PlatformEvent::Command(c), cx));
    cx.run_until_parked();
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::none()
    }
}

#[test]
fn area_drag_then_cmd_c_copies_exactly_the_selection() {
    let cx = &mut real_cx();
    let fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureArea);
    assert_eq!(
        fx.platform.calls().captured_rects,
        vec![rect(0.0, 0.0, W, H)],
        "froze the whole display first"
    );
    assert_eq!(
        fx.platform.calls().covered,
        vec![rect(0.0, 0.0, W, H)],
        "the frozen screen lines up with the real one, menu bar included"
    );
    let (overlay, mut w) = window_cx::<Overlay>(cx);
    assert_eq!(cx.read(|cx| overlay.read(cx).unwrap().mode), Mode::Area);

    // Select the red button exactly.
    let b = RED_BUTTON;
    drag(&mut w, p(b.x, b.y), p(b.right(), b.bottom()), Modifiers::none());
    assert_eq!(cx.read(|cx| overlay.read(cx).unwrap().selection), Some(b));
    assert!(has(&mut w, "selection-hints"), "shows ⌘C / ⌘S / ↩ hints");

    w.simulate_keystrokes("cmd-c");
    cx.run_until_parked();
    assert!(find::<Overlay>(cx).is_none(), "overlay closes");
    let (img, scale) = clipboard_image(cx).expect("PNG on the clipboard");
    assert_eq!((img.width, img.height), (240, 80), "2x pixels of a 120x40 pt area");
    assert_eq!(scale, 2, "saved as 144 dpi");
    assert_eq!(img.pixel(120, 40), Some(RED));
    assert!(matches!(last_notice(cx), Some(Notice::Copied(_))));
    assert!(find::<Editor>(cx).is_none(), "⌘C skips the editor");

    // Recents and the menu update.
    let recents = files_in(&fx.paths.recent);
    assert_eq!(recents.len(), 2, "capture and its thumbnail: {recents:?}");
    let menu = fx.platform.last_menu();
    assert!(menu.iter().any(
        |e| matches!(e, MenuEntry::Submenu { title, entries } if title == "Recent Captures" && entries.len() == 1)
    ));
    assert_eq!(
        settings(cx).last_area.map(|a| a.rect),
        Some(b),
        "remembered for Repeat Last Area"
    );
}

#[test]
fn area_cmd_s_saves_a_file() {
    let cx = &mut real_cx();
    let fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureArea);
    let (_, mut w) = window_cx::<Overlay>(cx);
    drag(&mut w, p(100.0, 100.0), p(300.0, 200.0), Modifiers::none());
    w.simulate_keystrokes("cmd-s");
    cx.run_until_parked();
    let saved = files_in(&fx.paths.default_save);
    assert_eq!(saved.len(), 1);
    let name = saved[0].file_name().unwrap().to_string_lossy().to_string();
    assert!(name.starts_with("OShot ") && name.ends_with(".png"), "{name}");
    let bytes = std::fs::read(&saved[0]).unwrap();
    let info = oshot_native::imageio::info(&bytes).unwrap();
    assert_eq!(
        (info.width, info.height, info.format),
        (400, 200, Some(ImageFormat::Png))
    );
    assert!(matches!(last_notice(cx), Some(Notice::Saved(_))));
}

#[test]
fn enter_opens_the_editor_by_default() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureArea);
    let (_, mut w) = window_cx::<Overlay>(cx);
    drag(&mut w, p(200.0, 150.0), p(900.0, 600.0), Modifiers::none());
    w.simulate_keystrokes("enter");
    cx.run_until_parked();
    let editor = find::<Editor>(cx).expect("editor opens");
    let (sw, sh) = cx.read(|cx| {
        let e = editor.read(cx).unwrap();
        (e.shot.width, e.shot.height)
    });
    assert_eq!((sw, sh), (1400, 900));
    assert!(clipboard_image(cx).is_some(), "copy after capture is on by default");
}

#[test]
fn thumbnail_and_copy_only_modes() {
    let cx = &mut real_cx();
    let _fx = start_with(
        cx,
        Settings {
            onboarding_done: true,
            after_capture: AfterCapture::Thumbnail,
            ..Settings::default()
        },
    );
    hotkey(cx, HotkeyAction::CaptureFullScreen);
    assert!(find::<Thumbnail>(cx).is_some(), "thumbnail mode shows a corner preview");
    assert!(find::<Editor>(cx).is_none());

    let _fx2 = start_with(
        cx,
        Settings {
            onboarding_done: true,
            after_capture: AfterCapture::ClipboardOnly,
            copy_on_capture: false,
            ..Settings::default()
        },
    );
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("old".into())));
    hotkey(cx, HotkeyAction::CaptureFullScreen);
    let (img, _) = clipboard_image(cx).expect("copy only still copies");
    assert_eq!((img.width, img.height), ((W * SCALE) as u32, (H * SCALE) as u32));
}

#[test]
fn finish_on_release_skips_the_keypress() {
    let cx = &mut real_cx();
    let _fx = start_with(
        cx,
        Settings {
            onboarding_done: true,
            finish_on_release: true,
            ..Settings::default()
        },
    );
    hotkey(cx, HotkeyAction::CaptureArea);
    let (_, mut w) = window_cx::<Overlay>(cx);
    drag(&mut w, p(10.0, 10.0), p(110.0, 60.0), Modifiers::none());
    assert!(find::<Overlay>(cx).is_none());
    assert!(find::<Editor>(cx).is_some());
}

#[test]
fn overlay_keyboard_editing_of_the_selection() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureArea);
    let (overlay, mut w) = window_cx::<Overlay>(cx);
    let sel = |cx: &TestAppContext| cx.read(|cx| overlay.read(cx).unwrap().selection.unwrap());

    // Loose selection around the left box, then A snaps to it.
    drag(&mut w, p(280.0, 680.0), p(420.0, 780.0), Modifiers::none());
    w.simulate_keystrokes("a");
    assert_eq!(sel(cx), BOX_LEFT, "auto-fit trims to content");

    // Arrow keys nudge by a pixel (half a point), shift by ten.
    w.simulate_keystrokes("right");
    assert_eq!(sel(cx).x, BOX_LEFT.x + 0.5);
    w.simulate_keystrokes("shift-down");
    assert_eq!(sel(cx).y, BOX_LEFT.y + 5.0);
    w.simulate_keystrokes("cmd-right");
    assert_eq!(sel(cx).w, BOX_LEFT.w + 0.5, "⌘ resizes");

    // Dragging inside moves the selection; dragging a corner resizes it.
    let s = sel(cx);
    drag(
        &mut w,
        p(s.x + 20.0, s.y + 20.0),
        p(s.x + 70.0, s.y + 20.0),
        Modifiers::none(),
    );
    assert_eq!(sel(cx).x, s.x + 50.0);
    let s = sel(cx);
    drag(
        &mut w,
        p(s.right(), s.bottom()),
        p(s.right() + 40.0, s.bottom() + 10.0),
        Modifiers::none(),
    );
    assert_eq!((sel(cx).w, sel(cx).h), (s.w + 40.0, s.h + 10.0));

    // Shift-drag makes a square.
    drag(&mut w, p(600.0, 100.0), p(700.0, 160.0), shift());
    assert_eq!((sel(cx).w, sel(cx).h), (100.0, 100.0));

    // A click without a drag clears the selection; Esc cancels.
    w.simulate_click(p(50.0, 850.0), Modifiers::none());
    assert_eq!(cx.read(|cx| overlay.read(cx).unwrap().selection), None);
    w.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(find::<Overlay>(cx).is_none());
    assert!(find::<Editor>(cx).is_none(), "cancel does nothing else");
}

#[test]
fn pressing_the_hotkey_again_cancels() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureArea);
    assert!(find::<Overlay>(cx).is_some());
    hotkey(cx, HotkeyAction::CaptureArea);
    assert!(find::<Overlay>(cx).is_none());
}

#[test]
fn window_mode_highlights_and_captures_the_hovered_window() {
    let cx = &mut real_cx();
    let fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureWindow);
    assert!(
        fx.platform.calls().captured_rects.is_empty(),
        "nothing frozen: windows are picked over the live screen"
    );
    assert_eq!(
        fx.platform.calls().covered,
        vec![rect(0.0, 0.0, W, H)],
        "the overlay covers the whole display, menu bar included"
    );
    let (overlay, mut w) = window_cx::<Overlay>(cx);
    w.simulate_mouse_move(p(400.0, 300.0), None, Modifiers::none());
    assert_eq!(cx.read(|cx| overlay.read(cx).unwrap().hovered_window), Some(0));
    w.simulate_mouse_move(p(1100.0, 500.0), None, Modifiers::none());
    assert_eq!(cx.read(|cx| overlay.read(cx).unwrap().hovered_window), Some(1));
    w.simulate_mouse_move(p(400.0, 300.0), None, Modifiers::none());
    w.simulate_click(p(400.0, 300.0), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        fx.platform.calls().captured_windows,
        vec![(11, true)],
        "captured on its own, with shadow"
    );
    assert!(find::<Editor>(cx).is_some());

    // ⌘C on a hovered window copies without opening anything.
    hotkey(cx, HotkeyAction::CaptureWindow);
    let (_, mut w) = window_cx::<Overlay>(cx);
    w.simulate_mouse_move(p(1100.0, 500.0), None, Modifiers::none());
    w.simulate_keystrokes("cmd-c");
    cx.run_until_parked();
    assert_eq!(fx.platform.calls().captured_windows.last(), Some(&(12, true)));
    let (img, _) = clipboard_image(cx).unwrap();
    assert_eq!(img.width, 1400);
}

#[test]
fn text_mode_copies_recognised_text() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureText);
    let (_, mut w) = window_cx::<Overlay>(cx);
    drag(&mut w, p(210.0, 170.0), p(890.0, 235.0), Modifiers::none());
    cx.run_until_parked();
    assert!(find::<Overlay>(cx).is_none(), "text mode finishes on release");
    let text = clipboard_text(cx).expect("text on the clipboard");
    assert!(text.contains("example.com"), "{text:?}");
    assert!(matches!(last_notice(cx), Some(Notice::Copied(m)) if m.contains("line")));
}

#[test]
fn color_picker_copies_hex_and_darkest() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    command(cx, Command::ColorPicker);
    let (_, mut w) = window_cx::<Overlay>(cx);
    w.simulate_mouse_move(p(1050.0, 220.0), None, Modifiers::none());
    w.simulate_click(p(1050.0, 220.0), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(clipboard_text(cx).as_deref(), Some("#E61414"));
    assert!(find::<Overlay>(cx).is_none());

    command(cx, Command::ColorPicker);
    let (_, mut w) = window_cx::<Overlay>(cx);
    w.simulate_click(p(BOX_LEFT.x - 2.0, BOX_LEFT.y + 10.0), shift());
    cx.run_until_parked();
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("#000000"),
        "⇧ picks the darkest nearby pixel"
    );
}

#[test]
fn ruler_measures_the_gap_between_boxes() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    command(cx, Command::Ruler);
    let (overlay, mut w) = window_cx::<Overlay>(cx);
    let gap = cx
        .read(|cx| {
            overlay
                .read(cx)
                .unwrap()
                .measure(oshot_core::geometry::pt(450.0, 730.0))
        })
        .unwrap();
    assert_eq!(gap.0.w, 100.0, "100 pt between the boxes");
    w.simulate_mouse_move(p(450.0, 730.0), None, Modifiers::none());
    w.simulate_click(p(450.0, 730.0), Modifiers::none());
    cx.run_until_parked();
    let text = clipboard_text(cx).unwrap();
    assert!(text.starts_with("100 × "), "{text}");
}

#[test]
fn full_screen_and_repeat_area() {
    let cx = &mut real_cx();
    let fx = start(cx);
    command(cx, Command::RepeatArea);
    assert!(
        matches!(last_notice(cx), Some(Notice::Info(m)) if m.contains("first")),
        "nothing to repeat yet"
    );

    hotkey(cx, HotkeyAction::CaptureArea);
    let (_, mut w) = window_cx::<Overlay>(cx);
    drag(&mut w, p(10.0, 20.0), p(110.0, 70.0), Modifiers::none());
    w.simulate_keystrokes("cmd-c");
    cx.run_until_parked();

    command(cx, Command::RepeatArea);
    assert_eq!(
        fx.platform.calls().captured_rects.last(),
        Some(&rect(10.0, 20.0, 100.0, 50.0)),
        "same area, no overlay"
    );
    let editor = find::<Editor>(cx).expect("finishes like any capture");
    let size = cx.read(|cx| {
        let e = editor.read(cx).unwrap();
        (e.shot.width, e.shot.height)
    });
    assert_eq!(size, (200, 100));
}

#[test]
fn delayed_capture_counts_down_then_freezes() {
    let cx = &mut real_cx();
    let fx = start(cx);
    command(cx, Command::DelayedCapture(3));
    assert!(matches!(last_notice(cx), Some(Notice::Info(m)) if m == "Capturing in 3…"));
    assert!(find::<Overlay>(cx).is_none());
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(matches!(last_notice(cx), Some(Notice::Info(m)) if m == "Capturing in 2…"));
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert!(find::<Overlay>(cx).is_some(), "area overlay after the delay");
    assert_eq!(fx.platform.calls().captured_rects.len(), 1);
}

#[test]
fn missing_permission_opens_the_permission_step() {
    let cx = &mut real_cx();
    let fx = start(cx);
    fx.platform.permission.store(false, Ordering::SeqCst);
    hotkey(cx, HotkeyAction::CaptureArea);
    assert!(find::<Overlay>(cx).is_none());
    assert_eq!(fx.platform.calls().requested_permission, 1);
    let onboarding = find::<Onboarding>(cx).expect("explains the permission");
    let on_card = cx.read(|cx| {
        let o = onboarding.read(cx).unwrap();
        (o.intro, o.current(cx))
    });
    assert_eq!(
        on_card,
        (false, Some(Item::ScreenRecording)),
        "straight to Screen Recording, no welcome"
    );
    assert!(fx.platform.calls().captured_rects.is_empty());
}

#[test]
fn capture_menu_hud_starts_every_mode() {
    let cx = &mut real_cx();
    let fx = start(cx);
    for (selector, mode) in [
        ("hud-area", Some(Mode::Area)),
        ("hud-window", Some(Mode::Window)),
        ("hud-text", Some(Mode::Text)),
        ("hud-color", Some(Mode::Color)),
        ("hud-ruler", Some(Mode::Ruler)),
    ] {
        hotkey(cx, HotkeyAction::CaptureMenu);
        let (_, mut w) = window_cx::<Hud>(cx);
        click(&mut w, selector);
        cx.run_until_parked();
        assert!(find::<Hud>(cx).is_none(), "{selector} closes the menu");
        let overlay = find::<Overlay>(cx).expect(selector);
        assert_eq!(cx.read(|cx| overlay.read(cx).map(|o| o.mode).ok()), mode);
        cx.update(oshot_app::overlay::close_all);
    }
    hotkey(cx, HotkeyAction::CaptureMenu);
    let (_, mut w) = window_cx::<Hud>(cx);
    click(&mut w, "hud-full");
    cx.run_until_parked();
    assert!(find::<Editor>(cx).is_some(), "Screen captures straight away");

    hotkey(cx, HotkeyAction::CaptureMenu);
    let (hud, mut w) = window_cx::<Hud>(cx);
    w.simulate_keystrokes("t");
    assert_eq!(cx.read(|cx| hud.read(cx).unwrap().delay), 5, "T cycles the timer");
    w.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(find::<Hud>(cx).is_none());

    hotkey(cx, HotkeyAction::CaptureMenu);
    let (_, mut w) = window_cx::<Hud>(cx);
    w.simulate_keystrokes("8");
    cx.run_until_parked();
    assert!(
        find::<oshot_app::converter::Converter>(cx).is_some(),
        "8 opens the converter"
    );

    hotkey(cx, HotkeyAction::CaptureMenu);
    hotkey(cx, HotkeyAction::CaptureMenu);
    assert!(find::<Hud>(cx).is_none(), "the hotkey toggles the menu");
    let _ = fx;
}

#[test]
fn thumbnail_buttons() {
    let cx = &mut real_cx();
    let fx = start_with(
        cx,
        Settings {
            onboarding_done: true,
            after_capture: AfterCapture::Thumbnail,
            copy_on_capture: false,
            ..Settings::default()
        },
    );
    for (selector, check) in [
        "thumb-copy",
        "thumb-save",
        "thumb-edit",
        "thumb-pin",
        "thumb-close",
        "thumbnail-image",
    ]
    .into_iter()
    .zip(0..)
    {
        hotkey(cx, HotkeyAction::CaptureFullScreen);
        let (_, mut w) = window_cx::<Thumbnail>(cx);
        click(&mut w, selector);
        cx.run_until_parked();
        assert!(find::<Thumbnail>(cx).is_none(), "{selector} closes the thumbnail");
        match check {
            0 => assert!(clipboard_image(cx).is_some()),
            1 => assert_eq!(files_in(&fx.paths.default_save).len(), 1),
            2 | 5 => {
                assert!(find::<Editor>(cx).is_some());
                for e in cx.read(oshot_app::windows::find_all::<Editor>) {
                    cx.update(|cx| oshot_app::windows::close(e, cx));
                }
            }
            3 => assert!(find::<oshot_app::pin::Pin>(cx).is_some()),
            _ => {}
        }
    }
}

#[test]
fn thumbnail_disappears_after_its_time() {
    let cx = &mut real_cx();
    let _fx = start_with(
        cx,
        Settings {
            onboarding_done: true,
            after_capture: AfterCapture::Thumbnail,
            thumbnail_seconds: 3,
            ..Settings::default()
        },
    );
    hotkey(cx, HotkeyAction::CaptureFullScreen);
    let (thumb, mut w) = window_cx::<Thumbnail>(cx);

    // While the pointer rests on it, it stays.
    w.simulate_mouse_move(p(40.0, 40.0), None, Modifiers::none());
    assert!(cx.read(|cx| thumb.read(cx).unwrap().hovered));
    cx.executor().advance_clock(Duration::from_secs(10));
    cx.run_until_parked();
    assert!(find::<Thumbnail>(cx).is_some(), "hovering pauses the timer");

    // Once the pointer leaves, it goes after its time.
    w.simulate_mouse_move(p(-200.0, -200.0), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert!(find::<Thumbnail>(cx).is_some());
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert!(find::<Thumbnail>(cx).is_none());
}

#[test]
fn pin_window_opacity_and_close() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let shot = oshot_app::state::Shot::new(oshot_core::bitmap::Bitmap::filled(400, 200, rgb(1, 2, 3)), 2.0);
    cx.update(|cx| oshot_app::pin::open(shot, cx));
    let (pin, mut w) = window_cx::<oshot_app::pin::Pin>(cx);
    w.simulate_keystrokes("5");
    assert_eq!(cx.read(|cx| pin.read(cx).unwrap().opacity), 0.5);
    w.simulate_keystrokes("0");
    assert_eq!(cx.read(|cx| pin.read(cx).unwrap().opacity), 1.0);
    w.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(find::<oshot_app::pin::Pin>(cx).is_none());
}

#[test]
fn recent_menu_commands() {
    let cx = &mut real_cx();
    let fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureArea);
    let (_, mut w) = window_cx::<Overlay>(cx);
    drag(&mut w, p(10.0, 10.0), p(60.0, 60.0), Modifiers::none());
    w.simulate_keystrokes("cmd-c");
    cx.run_until_parked();
    let path = oshot_app::recent::list(&fx.paths.recent)[0].path.clone();
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("x".into())));
    command(cx, Command::CopyRecent(path.clone()));
    let (img, _) = clipboard_image(cx).unwrap();
    assert_eq!(img.width, 100);
    command(cx, Command::OpenRecent(path));
    assert!(find::<Editor>(cx).is_some());
}

#[test]
fn both_command_keys_open_the_menu_and_a_number_starts_that_mode() {
    let cx = &mut real_cx();
    let fx = start(cx);
    assert_eq!(fx.platform.calls().gestures, vec![MenuGesture::BothCommand]);
    cx.update(|cx| handle_event(PlatformEvent::MenuGesture, cx));
    cx.run_until_parked();
    let (_, mut w) = window_cx::<Hud>(cx);
    w.simulate_keystrokes("2");
    cx.run_until_parked();
    assert!(find::<Hud>(cx).is_none(), "picking a mode closes the menu");
    let overlay = find::<Overlay>(cx).expect("window capture started");
    assert_eq!(cx.read(|cx| overlay.read(cx).unwrap().mode), Mode::Window);
    cx.update(|cx| handle_event(PlatformEvent::MenuGesture, cx));
    cx.run_until_parked();
    cx.update(|cx| handle_event(PlatformEvent::MenuGesture, cx));
    cx.run_until_parked();
    assert!(find::<Hud>(cx).is_none(), "the gesture toggles the menu");
}

#[test]
fn capture_menu_names_the_hovered_button() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureMenu);
    let (hud, mut w) = window_cx::<Hud>(cx);
    assert!(has(&mut w, "hud-caption"));
    assert_eq!(cx.read(|cx| hud.read(cx).unwrap().hovered), None);
    let b = w.debug_bounds("hud-window").unwrap();
    w.simulate_mouse_move(b.center(), None, Modifiers::none());
    w.run_until_parked();
    assert_eq!(cx.read(|cx| hud.read(cx).unwrap().hovered), Some(1), "Window is named");
    let b = w.debug_bounds("hud-timer").unwrap();
    w.simulate_mouse_move(b.center(), None, Modifiers::none());
    w.run_until_parked();
    assert_eq!(cx.read(|cx| hud.read(cx).unwrap().hovered), Some(4));
    let cap = w.debug_bounds("hud-caption").unwrap();
    let last = w.debug_bounds("hud-convert").unwrap();
    assert!(cap.left() >= last.right(), "the name sits right of the buttons");
    w.simulate_keystrokes("2");
    cx.run_until_parked();
    let overlay = find::<Overlay>(cx).expect("window capture started");
    assert_eq!(cx.read(|cx| overlay.read(cx).unwrap().mode), Mode::Window);
}

#[test]
fn capture_menu_cells_line_up() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureMenu);
    let (_, mut w) = window_cx::<Hud>(cx);
    let ids: [&'static str; 8] = [
        "hud-area",
        "hud-window",
        "hud-full",
        "hud-text",
        "hud-timer",
        "hud-color",
        "hud-ruler",
        "hud-convert",
    ];
    let icons: [&'static str; 8] = [
        "hud-area-icon",
        "hud-window-icon",
        "hud-full-icon",
        "hud-text-icon",
        "hud-timer-icon",
        "hud-color-icon",
        "hud-ruler-icon",
        "hud-convert-icon",
    ];
    let keys: [&'static str; 8] = [
        "hud-area-key",
        "hud-window-key",
        "hud-full-key",
        "hud-text-key",
        "hud-timer-key",
        "hud-color-key",
        "hud-ruler-key",
        "hud-convert-key",
    ];
    let mut icon_tops = Vec::new();
    for i in 0..ids.len() {
        let cell = w.debug_bounds(ids[i]).unwrap();
        let icon = w.debug_bounds(icons[i]).unwrap();
        let key = w.debug_bounds(keys[i]).unwrap();
        let centre = |b: gpui::Bounds<gpui::Pixels>| b.center();
        assert!(
            (f32::from(centre(icon).x - centre(cell).x)).abs() < 0.5
                && (f32::from(centre(icon).y - centre(cell).y)).abs() < 0.5,
            "{}: icon centred in its button",
            ids[i]
        );
        assert!(
            key.left() >= icon.right() - px(2.) && key.right() <= cell.right() && key.bottom() <= cell.bottom(),
            "{}: number tucked in the corner, clear of the icon",
            ids[i]
        );
        icon_tops.push(f32::from(icon.top()));
    }
    assert!(
        icon_tops.windows(2).all(|p| (p[0] - p[1]).abs() < 0.5),
        "icons share one baseline: {icon_tops:?}"
    );
}

#[test]
fn window_mode_tints_the_hovered_window_and_labels_its_middle() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    hotkey(cx, HotkeyAction::CaptureWindow);
    let (overlay, mut w) = window_cx::<Overlay>(cx);
    w.simulate_mouse_move(p(400.0, 300.0), None, Modifiers::none());
    w.run_until_parked();
    assert_eq!(cx.read(|cx| overlay.read(cx).unwrap().hovered_window), Some(0));
    let label = w.debug_bounds("window-label").expect("names the window");
    let c = label.center();
    let f = WINDOW_A;
    assert!((f32::from(c.x) - (f.x + f.w / 2.0)).abs() < 1.0 && (f32::from(c.y) - (f.y + f.h / 2.0)).abs() < 1.0);
    w.simulate_mouse_move(p(1100.0, 500.0), None, Modifiers::none());
    w.run_until_parked();
    assert_eq!(
        cx.read(|cx| overlay.read(cx).unwrap().hovered_window),
        Some(1),
        "moves to the next window"
    );
}
