//! One dispatch point for the menu bar, hotkeys, HUD buttons and tests.

use gpui::{App, PathPromptOptions};

use crate::overlay::Mode;
use crate::state::{AppState, Command, MenuEntry, PlatformEvent, platform, state};
use crate::{capture, converter, editor, hud, onboarding, organize, output, recent, settings_window};

pub fn dispatch(command: Command, cx: &mut App) {
    match command {
        Command::CaptureArea => capture::begin(Mode::Area, cx),
        Command::CaptureWindow => capture::begin(Mode::Window, cx),
        Command::CaptureText => capture::begin(Mode::Text, cx),
        Command::ColorPicker => capture::begin(Mode::Color, cx),
        Command::Ruler => capture::begin(Mode::Ruler, cx),
        Command::CaptureFullScreen => capture::full_screen(cx),
        Command::CaptureMenu => {
            if !onboarding::take_menu(cx) {
                hud::toggle(cx);
            }
        }
        Command::RepeatArea => capture::repeat_area(cx),
        Command::DelayedCapture(seconds) => capture::delayed(seconds, cx),
        Command::ConvertImage => converter::open(Vec::new(), cx),
        Command::ConvertFiles(paths) => converter::open(paths, cx),
        Command::OpenImage => open_image_prompt(cx),
        Command::OpenFile(path) | Command::OpenRecent(path) => editor::open_file(path, cx),
        Command::PasteImage => editor::open_clipboard(cx),
        Command::CopyRecent(path) => output::copy_file(&path, cx),
        Command::RevealRecent(path) => cx.reveal_path(&path),
        Command::OpenSaveFolder => {
            let dir = state(cx).save_folder();
            let _ = std::fs::create_dir_all(&dir);
            cx.open_with_system(&dir);
        }
        Command::Settings => settings_window::open(cx),
        Command::Onboarding => onboarding::open(cx),
        Command::Quit => cx.quit(),
    }
}

pub fn handle_event(event: PlatformEvent, cx: &mut App) {
    match event {
        PlatformEvent::Command(c) => dispatch(c, cx),
        PlatformEvent::Hotkey(a) => dispatch(Command::for_hotkey(a), cx),
        PlatformEvent::MenuGesture => dispatch(Command::CaptureMenu, cx),
        PlatformEvent::FolderChanged => organize::folder_changed(cx),
    }
}

fn open_image_prompt(cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Open".into()),
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = rx.await
            && let Some(path) = paths.into_iter().next()
        {
            cx.update(|cx| editor::open_file(path, cx));
        }
    })
    .detach();
}

pub fn apply_hotkeys(cx: &mut App) {
    let bindings = state(cx).settings.hotkeys.bindings();
    let failures = platform(cx).register_hotkeys(&bindings);
    cx.global_mut::<AppState>().hotkey_failures = failures;
}

/// (Re)installs the capture menu gesture. Called again once Accessibility
/// is granted, since monitors set up before that see no other app's keys.
pub fn apply_gesture(cx: &mut App) {
    let gesture = state(cx).settings.menu_gesture;
    platform(cx).set_menu_gesture(gesture);
}

/// The menu bar menu, built from current settings and recent captures.
pub fn menu_entries(cx: &App) -> Vec<MenuEntry> {
    use oshot_core::settings::HotkeyAction as H;
    let st = state(cx);
    let shortcut = |a: H| st.settings.hotkeys.get(a).cloned();
    let item = |title: &str, command: Command, action: Option<H>| MenuEntry::Item {
        title: title.into(),
        shortcut: action.and_then(shortcut),
        command,
        enabled: true,
    };
    let recent: Vec<MenuEntry> = recent::list(&st.paths.recent)
        .into_iter()
        .take(st.settings.recent_limit as usize)
        .map(|r| MenuEntry::Recent {
            title: r.title(),
            thumbnail_png: std::fs::read(&r.thumb).unwrap_or_default(),
            path: r.path,
        })
        .collect();
    let mut entries = vec![
        MenuEntry::Header("Capture".into()),
        item("Capture Area", Command::CaptureArea, Some(H::CaptureArea)),
        item("Capture Window", Command::CaptureWindow, Some(H::CaptureWindow)),
        item(
            "Capture Full Screen",
            Command::CaptureFullScreen,
            Some(H::CaptureFullScreen),
        ),
        item("Capture Text", Command::CaptureText, Some(H::CaptureText)),
        item("Repeat Last Area", Command::RepeatArea, Some(H::RepeatArea)),
        MenuEntry::Submenu {
            title: "Delayed Capture".into(),
            entries: [3, 5, 10]
                .into_iter()
                .map(|s| item(&format!("In {s} seconds"), Command::DelayedCapture(s), None))
                .collect(),
        },
        item("Capture Menu…", Command::CaptureMenu, Some(H::CaptureMenu)),
        MenuEntry::Separator,
        MenuEntry::Header("Tools".into()),
        item("Color Picker", Command::ColorPicker, Some(H::ColorPicker)),
        item("Screen Ruler", Command::Ruler, Some(H::Ruler)),
        item("Convert Image…", Command::ConvertImage, Some(H::Converter)),
        item("Open Image…", Command::OpenImage, None),
        item("Edit Clipboard Image", Command::PasteImage, None),
        MenuEntry::Separator,
    ];
    if recent.is_empty() {
        entries.push(MenuEntry::Item {
            title: "No Recent Captures".into(),
            shortcut: None,
            command: Command::OpenSaveFolder,
            enabled: false,
        });
    } else {
        entries.push(MenuEntry::Submenu {
            title: "Recent Captures".into(),
            entries: recent,
        });
    }
    entries.extend([
        item("Open Screenshots Folder", Command::OpenSaveFolder, None),
        MenuEntry::Separator,
        MenuEntry::Item {
            title: "Settings…".into(),
            shortcut: Some("cmd-,".into()),
            command: Command::Settings,
            enabled: true,
        },
        item("Welcome Guide", Command::Onboarding, None),
        MenuEntry::Item {
            title: "Quit OShot".into(),
            shortcut: Some("cmd-q".into()),
            command: Command::Quit,
            enabled: true,
        },
    ]);
    entries
}

pub fn rebuild_menu(cx: &mut App) {
    let entries = menu_entries(cx);
    platform(cx).set_menu(&entries);
}
