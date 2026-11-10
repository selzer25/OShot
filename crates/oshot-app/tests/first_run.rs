//! First launch: onboarding, permission, shortcut registration and the menu.

mod support;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use oshot_app::commands::handle_event;
use oshot_app::hud::Hud;
use oshot_app::onboarding::{Item, Onboarding, Practice};
use oshot_app::overlay::Overlay;
use oshot_app::permission_guide::{GuidePanel, panel_frame};
use oshot_app::state::{Command, MenuEntry, PlatformEvent};
use oshot_core::geometry::rect;
use oshot_core::gesture::MenuGesture;
use oshot_core::settings::{AfterCapture, HotkeyAction, Settings};
use support::*;

#[test]
fn first_launch_sets_everything_up_on_one_page() {
    let cx = &mut real_cx();
    let fx = start_configured(cx, Settings::default(), |p| {
        p.permission.store(false, Ordering::SeqCst);
        p.accessibility.store(false, Ordering::SeqCst);
    });
    assert_eq!(
        fx.platform.calls().gestures,
        vec![MenuGesture::BothCommand],
        "both ⌘ keys listen for the menu from launch"
    );
    let (h, mut w) = window_cx::<Onboarding>(cx);
    let current = |cx: &mut gpui::TestAppContext| cx.read(|cx| h.read(cx).unwrap().current(cx));
    let tick = |cx: &mut gpui::TestAppContext, ms: u64| {
        cx.executor().advance_clock(Duration::from_millis(ms));
        cx.run_until_parked();
    };

    // The welcome comes first; Space moves on to the checklist.
    assert!(has(&mut w, "get-started"));
    assert!(!has(&mut w, "window-body"), "the checklist waits behind the welcome");
    w.simulate_keystrokes("space");
    tick(cx, 500);
    assert!(!has(&mut w, "get-started"));
    for row in ["row-screen", "row-accessibility", "row-practice", "row-after", "finish"] {
        assert!(has(&mut w, row), "{row} is on the one page");
    }
    assert_eq!(current(cx), Some(Item::ScreenRecording));

    // Screen Recording: ask, and System Settings opens with the drag helper.
    click(&mut w, "allow-screen");
    assert_eq!(fx.platform.calls().requested_permission, 1);
    assert_eq!(fx.platform.calls().opened_privacy, 1);
    assert!(has(&mut w, "reopen-settings"), "the card waits on System Settings");
    let settings_window = rect(300.0, 120.0, 820.0, 560.0);
    *fx.platform.settings_window.lock().unwrap() = Some(settings_window);
    tick(cx, 200);
    let (_, mut panel) = window_cx::<GuidePanel>(cx);
    click(&mut panel, "drag-app");
    assert_eq!(
        fx.platform.calls().file_drags,
        vec![PathBuf::from("/Applications/OShot.app")]
    );

    // The helper follows System Settings when it moves.
    let moved = rect(360.0, 140.0, 820.0, 560.0);
    *fx.platform.settings_window.lock().unwrap() = Some(moved);
    tick(cx, 200);
    let display = cx
        .read(|cx| oshot_app::state::platform(cx).displays())
        .into_iter()
        .next();
    assert_eq!(
        fx.platform.calls().placed.last().copied(),
        Some(panel_frame(moved, display.as_ref()))
    );

    // The user switches OShot on: the row ticks, the helper goes away and
    // the next item comes in by itself.
    fx.platform.permission.store(true, Ordering::SeqCst);
    tick(cx, 1100);
    assert_eq!(
        current(cx),
        Some(Item::ScreenRecording),
        "stays a moment to show the tick"
    );
    assert!(find::<GuidePanel>(cx).is_none(), "helper closes once access is granted");
    tick(cx, 1400);
    assert_eq!(current(cx), Some(Item::Accessibility));

    // Accessibility: same flow; the gesture listener is reinstalled once trusted.
    click(&mut w, "allow-accessibility");
    assert_eq!(fx.platform.calls().requested_accessibility, 1);
    assert_eq!(fx.platform.calls().opened_accessibility, 1);
    fx.platform.accessibility.store(true, Ordering::SeqCst);
    tick(cx, 1100);
    assert!(
        fx.platform.calls().gestures.len() > 1,
        "monitors reinstalled after the grant"
    );
    tick(cx, 1400);
    assert_eq!(current(cx), Some(Item::Practice));

    // Practice: the real gesture event opens the menu on the card instead of
    // over the onboarding, and a number picks a mode.
    cx.update(|cx| handle_event(PlatformEvent::MenuGesture, cx));
    cx.run_until_parked();
    assert!(find::<Hud>(cx).is_none(), "the practice takes the menu");
    assert_eq!(cx.read(|cx| h.read(cx).unwrap().practice), Practice::Open);
    w.simulate_keystrokes("2");
    w.run_until_parked();
    assert_eq!(cx.read(|cx| h.read(cx).unwrap().practice), Practice::Done(1));
    assert!(find::<Overlay>(cx).is_none(), "practice never starts a capture");
    tick(cx, 1400);
    assert_eq!(current(cx), Some(Item::AfterCapture));

    // After-capture choice, saved straight away.
    click(&mut w, "after-show-thumbnail");
    assert_eq!(settings(cx).after_capture, AfterCapture::Thumbnail);
    click(&mut w, "after-copy-only");
    assert_eq!(settings(cx).after_capture, AfterCapture::ClipboardOnly);
    tick(cx, 1400);
    assert_eq!(current(cx), None, "all set");

    click(&mut w, "finish");
    assert!(find::<Onboarding>(cx).is_none(), "window closes");
    let on_disk = Settings::load(&fx.paths.settings);
    assert!(on_disk.onboarding_done, "saved to disk");
    assert_eq!(on_disk.after_capture, AfterCapture::ClipboardOnly);

    // Afterwards the same gesture opens the real menu.
    cx.update(|cx| handle_event(PlatformEvent::MenuGesture, cx));
    cx.run_until_parked();
    assert!(find::<Hud>(cx).is_some());
}

#[test]
fn rows_can_be_visited_in_any_order() {
    let cx = &mut real_cx();
    let _fx = start_configured(cx, Settings::default(), |p| p.permission.store(false, Ordering::SeqCst));
    let (h, mut w) = window_cx::<Onboarding>(cx);
    click(&mut w, "get-started");
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    click(&mut w, "row-after");
    assert_eq!(cx.read(|cx| h.read(cx).unwrap().current(cx)), Some(Item::AfterCapture));
    assert!(has(&mut w, "after-open-editor"));
    click(&mut w, "row-screen");
    assert!(has(&mut w, "allow-screen"));
}

#[test]
fn choosing_control_keys_in_the_practice_switches_the_gesture() {
    let cx = &mut real_cx();
    let fx = start_with(cx, Settings::default());
    let (h, mut w) = window_cx::<Onboarding>(cx);
    click(&mut w, "get-started");
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| h.read(cx).unwrap().current(cx)),
        Some(Item::Practice),
        "permissions already granted"
    );
    click(&mut w, "practice-gesture-1");
    assert_eq!(settings(cx).menu_gesture, MenuGesture::BothControl);
    assert_eq!(fx.platform.calls().gestures.last(), Some(&MenuGesture::BothControl));
    // The ⌥⇧5 shortcut counts as practice too.
    cx.update(|cx| handle_event(PlatformEvent::Hotkey(HotkeyAction::CaptureMenu), cx));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| h.read(cx).unwrap().practice), Practice::Open);
}
#[test]
fn returning_user_sees_no_onboarding() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    assert!(find::<Onboarding>(cx).is_none());
}

#[test]
fn launch_registers_default_hotkeys_and_menu() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let hk = fx.platform.last_hotkeys();
    let sym = |a: HotkeyAction| hk.iter().find(|(x, _)| *x == a).map(|(_, h)| h.symbols());
    assert_eq!(sym(HotkeyAction::CaptureFullScreen).as_deref(), Some("⌥⇧3"));
    assert_eq!(sym(HotkeyAction::CaptureArea).as_deref(), Some("⌥⇧4"));
    assert_eq!(sym(HotkeyAction::CaptureMenu).as_deref(), Some("⌥⇧5"));
    assert_eq!(sym(HotkeyAction::CaptureWindow).as_deref(), Some("⌥⇧6"));
    assert_eq!(sym(HotkeyAction::CaptureText).as_deref(), Some("⌥⇧7"));

    let menu = fx.platform.last_menu();
    let titles: Vec<String> = menu
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item { title, .. } | MenuEntry::Submenu { title, .. } => Some(title.clone()),
            _ => None,
        })
        .collect();
    for want in [
        "Capture Area",
        "Capture Window",
        "Capture Full Screen",
        "Capture Text",
        "Repeat Last Area",
        "Delayed Capture",
        "Color Picker",
        "Screen Ruler",
        "Convert Image…",
        "Open Image…",
        "Settings…",
        "Quit OShot",
    ] {
        assert!(titles.iter().any(|t| t == want), "menu has {want}: {titles:?}");
    }
    let area = menu.iter().find_map(|e| match e {
        MenuEntry::Item {
            title,
            shortcut,
            command,
            ..
        } if title == "Capture Area" => Some((shortcut.clone(), command.clone())),
        _ => None,
    });
    assert_eq!(
        area,
        Some((Some("alt-shift-4".into()), Command::CaptureArea)),
        "menu shows the shortcut"
    );
    assert!(
        menu.iter()
            .any(|e| matches!(e, MenuEntry::Item { title, enabled: false, .. } if title == "No Recent Captures"))
    );
    assert!(
        fx.platform
            .calls()
            .appearance
            .contains(&oshot_core::settings::ThemeChoice::System)
    );
}

#[test]
fn failed_hotkeys_are_reported_in_settings() {
    let cx = &mut real_cx();
    let fx = start(cx);
    *fx.platform.fail_hotkeys.lock().unwrap() = vec![HotkeyAction::CaptureText];
    cx.update(oshot_app::commands::apply_hotkeys);
    let failures = cx.read(|cx| cx.global::<oshot_app::state::AppState>().hotkey_failures.clone());
    assert_eq!(failures, vec![HotkeyAction::CaptureText]);
}
