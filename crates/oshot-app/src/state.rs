//! App-wide state, the platform seam, and the commands every entry point
//! (menu bar, hotkeys, buttons, tests) goes through.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, Global, RenderImage, Window};
use oshot_core::bitmap::{Bitmap, BitmapRef};
use oshot_core::geometry::{Pt, Rect};
use oshot_core::gesture::MenuGesture;
use oshot_core::settings::{Hotkey, HotkeyAction, Settings, ThemeChoice};
use smallvec::smallvec;

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayInfo {
    pub id: u32,
    /// In global points, top-left origin.
    pub frame: Rect,
    pub scale: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowInfo {
    pub id: u32,
    /// In global points, top-left origin.
    pub frame: Rect,
    pub owner: String,
    pub title: String,
}

pub struct Capture {
    pub bitmap: Bitmap,
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glass {
    /// Regular window with a native titlebar.
    Window,
    /// Borderless floating panel with rounded corners.
    Panel { radius: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Item {
        title: String,
        shortcut: Option<String>,
        command: Command,
        enabled: bool,
    },
    Recent {
        title: String,
        thumbnail_png: Vec<u8>,
        path: PathBuf,
    },
    Submenu {
        title: String,
        entries: Vec<MenuEntry>,
    },
    Header(String),
    Separator,
}

/// Everything the app asks of macOS. The real implementation lives in
/// `platform`; tests use a fake so every flow runs without permissions,
/// a screen, or global state.
pub trait Platform: Send + Sync + 'static {
    fn has_screen_permission(&self) -> bool;
    fn request_screen_permission(&self);
    fn open_privacy_settings(&self);
    fn has_accessibility(&self) -> bool;
    /// Shows macOS's prompt, which also lists OShot under Accessibility.
    fn request_accessibility(&self);
    fn open_accessibility_settings(&self);
    /// Installs (or removes, for `Off`) the capture menu gesture listener.
    /// Events arrive as `PlatformEvent::MenuGesture`.
    fn set_menu_gesture(&self, gesture: MenuGesture);
    /// The `.app` OShot runs from, which the user drags into System Settings.
    fn app_bundle(&self) -> Option<PathBuf>;
    /// System Settings' main window in global points, while it is frontmost.
    fn system_settings_frame(&self) -> Option<Rect>;
    /// Starts a Finder-style drag of `path` from the current mouse-down.
    fn begin_file_drag(&self, window: &mut Window, path: &Path) -> bool;
    /// Moves a window to `frame` in global points, top-left origin.
    fn place_window(&self, window: &mut Window, frame: Rect);
    /// Makes a window cover `frame` (a whole display, global points),
    /// menu bar included.
    fn cover_display(&self, window: &mut Window, frame: Rect);
    fn displays(&self) -> Vec<DisplayInfo>;
    /// Cursor position in global points.
    fn cursor(&self) -> Pt;
    /// On-screen app windows, front to back, excluding OShot's own.
    fn windows(&self) -> Vec<WindowInfo>;
    /// Blocking: captures what is on screen inside `rect` (global points).
    fn capture_rect(&self, rect: Rect) -> Result<Capture, String>;
    /// Blocking: captures one window on its own, optionally with its shadow.
    fn capture_window(&self, id: u32, shadow: bool) -> Result<Capture, String>;
    /// Replaces all global hotkeys. Returns the actions that failed to register.
    fn register_hotkeys(&self, bindings: &[(HotkeyAction, Hotkey)]) -> Vec<HotkeyAction>;
    fn set_menu(&self, entries: &[MenuEntry]);
    /// Puts native glass behind a window's content.
    fn decorate(&self, window: &mut Window, glass: Glass);
    fn set_appearance(&self, choice: ThemeChoice);
    fn set_launch_at_login(&self, on: bool) -> Result<(), String>;
    /// Puts files (not their contents) on the pasteboard, as Finder's copy does.
    fn copy_files(&self, paths: &[PathBuf]);
    /// Where macOS's own screenshots are saved.
    fn system_screenshot_folder(&self) -> PathBuf;
    /// Starts watching a folder; events arrive as `PlatformEvent::FolderChanged`.
    fn watch_folder(&self, path: Option<&Path>);
}

/// Events from native callbacks, delivered to the GPUI main loop.
#[derive(Clone, Debug, PartialEq)]
pub enum PlatformEvent {
    Command(Command),
    Hotkey(HotkeyAction),
    MenuGesture,
    FolderChanged,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    CaptureArea,
    CaptureWindow,
    CaptureFullScreen,
    CaptureText,
    CaptureMenu,
    RepeatArea,
    DelayedCapture(u32),
    ColorPicker,
    Ruler,
    ConvertImage,
    ConvertFiles(Vec<PathBuf>),
    OpenImage,
    OpenFile(PathBuf),
    PasteImage,
    OpenRecent(PathBuf),
    CopyRecent(PathBuf),
    RevealRecent(PathBuf),
    OpenSaveFolder,
    Settings,
    Onboarding,
    Quit,
}

impl Command {
    pub fn for_hotkey(action: HotkeyAction) -> Command {
        match action {
            HotkeyAction::CaptureFullScreen => Command::CaptureFullScreen,
            HotkeyAction::CaptureArea => Command::CaptureArea,
            HotkeyAction::CaptureMenu => Command::CaptureMenu,
            HotkeyAction::CaptureWindow => Command::CaptureWindow,
            HotkeyAction::CaptureText => Command::CaptureText,
            HotkeyAction::RepeatArea => Command::RepeatArea,
            HotkeyAction::ColorPicker => Command::ColorPicker,
            HotkeyAction::Ruler => Command::Ruler,
            HotkeyAction::Converter => Command::ConvertImage,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub support: PathBuf,
    pub settings: PathBuf,
    pub recent: PathBuf,
    pub default_save: PathBuf,
}

impl Paths {
    pub fn under(support: PathBuf, default_save: PathBuf) -> Paths {
        Paths {
            settings: support.join("settings.json"),
            recent: support.join("Recent"),
            support,
            default_save,
        }
    }

    pub fn standard() -> Paths {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        Paths::under(
            home.join("Library/Application Support/OShot"),
            home.join("Pictures/OShot"),
        )
    }
}

/// A captured or opened image. GPUI owns the pixels (BGRA) so the image is
/// stored once and drawn without copies.
#[derive(Clone)]
pub struct Shot {
    pub image: Arc<RenderImage>,
    pub width: u32,
    pub height: u32,
    /// Pixels per point.
    pub scale: f32,
}

impl Shot {
    pub fn new(bitmap: Bitmap, scale: f32) -> Shot {
        let (width, height) = (bitmap.width, bitmap.height);
        let buffer = image::RgbaImage::from_raw(width, height, bitmap.data).expect("buffer matches size");
        let image = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
        Shot {
            image,
            width,
            height,
            scale,
        }
    }

    pub fn pixels(&self) -> BitmapRef<'_> {
        BitmapRef {
            width: self.width,
            height: self.height,
            data: self.image.as_bytes(0).unwrap_or(&[]),
        }
    }

    pub fn crop(&self, area: oshot_core::geometry::IRect) -> Shot {
        Shot::new(self.pixels().crop(area), self.scale)
    }
}

/// Notable things that happened, for toasts and for tests to assert on.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    Copied(String),
    Saved(PathBuf),
    Error(String),
    Info(String),
}

pub struct AppState {
    pub settings: Settings,
    pub paths: Paths,
    pub platform: Arc<dyn Platform>,
    pub notices: Vec<Notice>,
    /// Hotkeys that failed to register, shown in Settings.
    pub hotkey_failures: Vec<HotkeyAction>,
}

impl Global for AppState {}

impl AppState {
    pub fn new(paths: Paths, platform: Arc<dyn Platform>) -> AppState {
        let settings = Settings::load(&paths.settings);
        AppState {
            settings,
            paths,
            platform,
            notices: Vec::new(),
            hotkey_failures: Vec::new(),
        }
    }

    pub fn save_folder(&self) -> PathBuf {
        self.settings.save_folder_or(&self.paths.default_save)
    }
}

pub fn state(cx: &App) -> &AppState {
    cx.global::<AppState>()
}

pub fn platform(cx: &App) -> Arc<dyn Platform> {
    cx.global::<AppState>().platform.clone()
}

/// Changes settings, writes them to disk and applies side effects.
pub fn update_settings(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    let before = cx.global::<AppState>().settings.clone();
    let after = {
        let st = cx.global_mut::<AppState>();
        f(&mut st.settings);
        st.settings.clone()
    };
    if before == after {
        return;
    }
    let path = cx.global::<AppState>().paths.settings.clone();
    if let Err(e) = after.save(&path) {
        notify(cx, Notice::Error(format!("Could not save settings: {e}")));
    }
    let p = platform(cx);
    if before.hotkeys != after.hotkeys {
        crate::commands::apply_hotkeys(cx);
    }
    if before.menu_gesture != after.menu_gesture {
        p.set_menu_gesture(after.menu_gesture);
    }
    if before.theme != after.theme {
        p.set_appearance(after.theme);
        cx.refresh_windows();
    }
    if before.launch_at_login != after.launch_at_login
        && let Err(e) = p.set_launch_at_login(after.launch_at_login)
    {
        notify(cx, Notice::Error(e));
    }
    if before.tidy_desktop != after.tidy_desktop {
        crate::organize::apply_watcher(cx);
    }
    crate::commands::rebuild_menu(cx);
}

pub fn notify(cx: &mut App, notice: Notice) {
    cx.global_mut::<AppState>().notices.push(notice.clone());
    crate::toast::show(notice, cx);
}
