//! Settings: every page, every toggle, every choice, the shortcut recorder
//! and the file name template, checked against what is saved to disk.

mod support;

use gpui::{TestAppContext, VisualTestContext, WindowHandle};
use oshot_app::settings_window::{Page, SettingsWindow};
use oshot_app::state::Notice;
use oshot_core::format::ImageFormat;
use oshot_core::settings::{
    AfterCapture, Corner, Hotkey, HotkeyAction, Hotkeys, OutputLocation, Settings, ThemeChoice,
};
use support::*;

fn open(cx: &mut TestAppContext) -> (WindowHandle<SettingsWindow>, VisualTestContext) {
    cx.update(|cx| oshot_app::commands::dispatch(oshot_app::state::Command::Settings, cx));
    cx.run_until_parked();
    let (h, mut w) = window_cx::<SettingsWindow>(cx);
    w.update(|_, _| {});
    (h, w)
}

fn disk(fx: &Fixture) -> Settings {
    Settings::load(&fx.paths.settings)
}

#[test]
fn sidebar_reaches_every_page() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    let (h, mut w) = open(cx);
    for (sel, page) in [
        ("nav-capture", Page::Capture),
        ("nav-shortcuts", Page::Shortcuts),
        ("nav-saving", Page::Saving),
        ("nav-converter", Page::Converter),
        ("nav-organize", Page::Organize),
        ("nav-about", Page::About),
        ("nav-general", Page::General),
    ] {
        click(&mut w, sel);
        assert_eq!(cx.read(|cx| h.read(cx).unwrap().page), page, "{sel}");
    }
    // Opening again reuses the window.
    let (_h2, _) = open(cx);
    assert_eq!(
        cx.read(|cx| oshot_app::windows::find_all::<SettingsWindow>(cx).len()),
        1
    );
}

#[test]
fn general_page_controls_save_to_disk() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let (_h, mut w) = open(cx);
    click(&mut w, "theme-2");
    assert_eq!(disk(&fx).theme, ThemeChoice::Dark);
    assert_eq!(
        fx.platform.calls().appearance.last(),
        Some(&ThemeChoice::Dark),
        "native glass follows"
    );
    click(&mut w, "theme-1");
    assert_eq!(disk(&fx).theme, ThemeChoice::Light);
    click(&mut w, "theme-0");

    click(&mut w, "login");
    assert!(disk(&fx).launch_at_login);
    assert_eq!(fx.platform.calls().login, vec![true]);
    click(&mut w, "login");
    assert_eq!(fx.platform.calls().login, vec![true, false]);

    for (i, a) in AfterCapture::ALL.iter().enumerate() {
        let sel: &'static str = Box::leak(format!("after-{i}").into_boxed_str());
        click(&mut w, sel);
        assert_eq!(disk(&fx).after_capture, *a);
    }
    for (sel, field) in [("copy-on-capture", 0), ("save-on-capture", 1), ("esc-copies", 2)] {
        if field == 2 {
            w.simulate_event(gpui::ScrollEvent {
                position: p(600.0, 400.0),
                delta: gpui::ScrollDelta::Pixels(p(0.0, -250.0)),
                ..Default::default()
            });
            w.run_until_parked();
        }
        let before = disk(&fx);
        click(&mut w, sel);
        let after = disk(&fx);
        match field {
            0 => assert_ne!(before.copy_on_capture, after.copy_on_capture),
            1 => assert_ne!(before.save_on_capture, after.save_on_capture),
            _ => assert_ne!(before.escape_copies, after.escape_copies),
        }
    }
    click(&mut w, "corner-0");
    assert_eq!(disk(&fx).thumbnail_corner, Corner::TopLeft);
    click(&mut w, "thumb-secs-3");
    assert_eq!(disk(&fx).thumbnail_seconds, 0, "until closed");
}

#[test]
fn capture_page() {
    let cx = &mut real_cx();
    let fx = start(cx);
    fx.platform.permission.store(false, std::sync::atomic::Ordering::SeqCst);
    let (_h, mut w) = open(cx);
    click(&mut w, "nav-capture");
    click(&mut w, "grant");
    assert_eq!(fx.platform.calls().requested_permission, 1);
    assert_eq!(fx.platform.calls().opened_privacy, 1);
    click(&mut w, "finish-on-release");
    assert!(disk(&fx).finish_on_release);
    click(&mut w, "window-shadow");
    assert!(!disk(&fx).window_shadow);
    click(&mut w, "show-welcome");
    assert!(find::<oshot_app::onboarding::Onboarding>(cx).is_some());
}

#[test]
fn recording_a_shortcut() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let (h, mut w) = open(cx);
    click(&mut w, "nav-shortcuts");

    // Recording pauses global hotkeys so the keys reach this window.
    click(&mut w, "hotkey-1");
    assert_eq!(
        cx.read(|cx| h.read(cx).unwrap().recording),
        Some(HotkeyAction::CaptureArea)
    );
    assert!(fx.platform.last_hotkeys().is_empty(), "paused");
    w.simulate_keystrokes("cmd-alt-8");
    let hk = Hotkey::parse(disk(&fx).hotkeys.area.as_deref().unwrap()).unwrap();
    assert!(hk.cmd && hk.alt && hk.key == "8");
    assert!(
        fx.platform
            .last_hotkeys()
            .iter()
            .any(|(a, h)| *a == HotkeyAction::CaptureArea && *h == hk),
        "re-registered"
    );
    assert_eq!(cx.read(|cx| h.read(cx).unwrap().recording), None);

    // A plain letter is refused; a taken chord is refused.
    click(&mut w, "hotkey-6");
    w.simulate_keystrokes("q");
    assert!(matches!(last_notice(cx), Some(Notice::Info(m)) if m.contains("need")));
    w.simulate_keystrokes("alt-shift-3");
    assert!(matches!(last_notice(cx), Some(Notice::Error(m)) if m.contains("already used")));
    w.simulate_keystrokes("escape");
    assert_eq!(cx.read(|cx| h.read(cx).unwrap().recording), None, "Esc cancels");
    assert_eq!(disk(&fx).hotkeys.color_picker, None);

    // macOS's own chords are allowed with a warning.
    click(&mut w, "hotkey-6");
    w.simulate_keystrokes("cmd-shift-5");
    assert!(matches!(last_notice(cx), Some(Notice::Info(m)) if m.contains("System Settings")));
    assert_eq!(disk(&fx).hotkeys.color_picker.as_deref(), Some("shift-cmd-5"));

    // Backspace clears.
    click(&mut w, "hotkey-4");
    w.simulate_keystrokes("backspace");
    assert_eq!(disk(&fx).hotkeys.text, None);

    // The button sits below the fold at the default height.
    w.simulate_resize(gpui::size(gpui::px(820.), gpui::px(980.)));
    w.run_until_parked();
    click(&mut w, "reset-hotkeys");
    assert_eq!(disk(&fx).hotkeys, Hotkeys::default());
}

#[test]
fn saving_page_and_file_name_template() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let (_h, mut w) = open(cx);
    click(&mut w, "nav-saving");
    click(&mut w, "save-format-2");
    assert_eq!(disk(&fx).file_format, ImageFormat::WebP);
    assert!(has(&mut w, "save-quality"), "quality for lossy formats");
    click(&mut w, "save-format-0");
    assert!(!has(&mut w, "save-quality"));
    click(&mut w, "shrink-png");
    assert!(disk(&fx).shrink_png);
    click(&mut w, "downscale");
    assert!(disk(&fx).downscale_retina);

    click(&mut w, "text-input");
    w.simulate_keystrokes("cmd-a");
    w.simulate_input("Shot {year}");
    assert_eq!(disk(&fx).filename_template, "Shot {year}");
    w.simulate_keystrokes("backspace");
    assert_eq!(disk(&fx).filename_template, "Shot {year");

    click(&mut w, "choose-folder");
    let dir = fx.root.join("Elsewhere");
    let d = dir.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![d]));
    cx.run_until_parked();
    assert_eq!(disk(&fx).save_folder, Some(dir));
}

#[test]
fn converter_and_organize_pages() {
    let cx = &mut real_cx();
    let fx = start(cx);
    let (_h, mut w) = open(cx);
    click(&mut w, "nav-converter");
    click(&mut w, "conv-format-4");
    assert_eq!(disk(&fx).converter.format, ImageFormat::Avif);
    click(&mut w, "conv-strip");
    assert!(!disk(&fx).converter.strip_metadata);
    click(&mut w, "conv-output-1");
    assert_eq!(disk(&fx).converter.output, OutputLocation::SaveFolder);
    click(&mut w, "open-converter");
    assert!(find::<oshot_app::converter::Converter>(cx).is_some());

    let (_h, mut w) = open(cx);
    click(&mut w, "nav-organize");
    click(&mut w, "tidy");
    assert!(disk(&fx).tidy_desktop);
    assert_eq!(
        fx.platform.calls().watched.last(),
        Some(&Some(fx.platform.screenshot_folder.clone())),
        "watches the macOS screenshot folder"
    );
    click(&mut w, "tidy-convert");
    assert!(disk(&fx).tidy_convert);
    click(&mut w, "tidy");
    assert_eq!(fx.platform.calls().watched.last(), Some(&None), "stops watching");
    click(&mut w, "auto-clean-2");
    assert_eq!(disk(&fx).auto_clean_days, Some(30));
    click(&mut w, "recent-limit-2");
    assert_eq!(disk(&fx).recent_limit, 20);
    std::fs::create_dir_all(&fx.paths.recent).unwrap();
    std::fs::write(fx.paths.recent.join("1.png"), b"x").unwrap();
    click(&mut w, "clear-recent");
    assert!(!fx.paths.recent.exists());
}

#[test]
fn tidy_moves_new_macos_screenshots() {
    let cx = &mut real_cx();
    let fx = start_with(
        cx,
        Settings {
            onboarding_done: true,
            tidy_desktop: true,
            ..Settings::default()
        },
    );
    let shot = fx
        .platform
        .screenshot_folder
        .join("Screenshot 2026-11-08 at 10.00.00.png");
    std::fs::write(
        &shot,
        png_bytes(
            &oshot_core::bitmap::Bitmap::filled(10, 10, oshot_core::color::rgb(1, 2, 3)),
            1,
        ),
    )
    .unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&shot)
        .unwrap()
        .set_modified(old)
        .unwrap();
    cx.update(|cx| oshot_app::commands::handle_event(oshot_app::state::PlatformEvent::FolderChanged, cx));
    cx.run_until_parked();
    assert!(!shot.exists());
    assert!(
        fx.paths
            .default_save
            .join("Screenshot 2026-11-08 at 10.00.00.png")
            .exists()
    );
    assert!(matches!(last_notice(cx), Some(Notice::Info(m)) if m.contains("Moved 1")));
}

#[test]
fn choosing_the_menu_gesture() {
    let cx = &mut real_cx();
    let fx = start(cx);
    fx.platform
        .accessibility
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let (_h, mut w) = open(cx);
    click(&mut w, "nav-shortcuts");
    assert!(
        has(&mut w, "allow-accessibility"),
        "explains the gesture needs Accessibility"
    );
    click(&mut w, "allow-accessibility");
    assert_eq!(fx.platform.calls().requested_accessibility, 1);
    assert_eq!(fx.platform.calls().opened_accessibility, 1);
    click(&mut w, "gesture-1");
    assert_eq!(disk(&fx).menu_gesture, oshot_core::gesture::MenuGesture::BothControl);
    assert_eq!(
        fx.platform.calls().gestures.last(),
        Some(&oshot_core::gesture::MenuGesture::BothControl),
        "the listener follows the setting"
    );
    click(&mut w, "gesture-2");
    assert_eq!(disk(&fx).menu_gesture, oshot_core::gesture::MenuGesture::Off);
    assert!(!has(&mut w, "allow-accessibility"), "off needs no permission");
}
