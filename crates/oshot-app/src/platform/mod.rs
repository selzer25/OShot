//! The real `Platform`: AppKit, ScreenCaptureKit, Carbon hotkeys and friends.

mod capture;
mod gesture;
mod glass;
mod hotkeys;
mod misc;
mod permission;
mod tray;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::Window;
use oshot_core::geometry::{Pt, Rect};
use oshot_core::gesture::MenuGesture;
use oshot_core::settings::{Hotkey, HotkeyAction, ThemeChoice};

use crate::state::{Capture, DisplayInfo, Glass, MenuEntry, Platform, PlatformEvent, WindowInfo};

static SINK: OnceLock<UnboundedSender<PlatformEvent>> = OnceLock::new();

/// Native callbacks run inside AppKit's own loops (menu tracking, Carbon
/// event dispatch) where GPUI state must not be touched, so they only
/// queue an event for the main loop.
pub(crate) fn send(event: PlatformEvent) {
    if let Some(sink) = SINK.get() {
        let _ = sink.unbounded_send(event);
    }
}

/// Queues a command from callbacks that run before the app has started.
pub fn queue(command: crate::state::Command) {
    send(PlatformEvent::Command(command));
}

pub struct MacPlatform;

impl MacPlatform {
    /// Creates the platform and the stream of native events. Call once, on
    /// the main thread.
    pub fn new() -> (MacPlatform, UnboundedReceiver<PlatformEvent>) {
        let (tx, rx) = unbounded();
        let _ = SINK.set(tx);
        hotkeys::install();
        tray::install_services();
        (MacPlatform, rx)
    }
}

impl Platform for MacPlatform {
    fn has_screen_permission(&self) -> bool {
        capture::has_permission()
    }

    fn request_screen_permission(&self) {
        capture::request_permission();
    }

    fn open_privacy_settings(&self) {
        misc::open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture");
    }

    fn has_accessibility(&self) -> bool {
        permission::has_accessibility()
    }

    fn request_accessibility(&self) {
        permission::request_accessibility();
    }

    fn open_accessibility_settings(&self) {
        misc::open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
    }

    fn set_menu_gesture(&self, gesture: MenuGesture) {
        gesture::set(gesture);
    }

    fn app_bundle(&self) -> Option<PathBuf> {
        permission::app_bundle()
    }

    fn system_settings_frame(&self) -> Option<Rect> {
        permission::system_settings_frame()
    }

    fn begin_file_drag(&self, window: &mut Window, path: &Path) -> bool {
        permission::begin_file_drag(window, path)
    }

    fn place_window(&self, window: &mut Window, frame: Rect) {
        permission::place_window(window, frame);
    }

    fn cover_display(&self, window: &mut Window, frame: Rect) {
        permission::cover_display(window, frame);
    }

    fn displays(&self) -> Vec<DisplayInfo> {
        capture::displays()
    }

    fn cursor(&self) -> Pt {
        capture::cursor()
    }

    fn windows(&self) -> Vec<WindowInfo> {
        capture::windows()
    }

    fn capture_rect(&self, rect: Rect) -> Result<Capture, String> {
        capture::capture_rect(rect)
    }

    fn capture_window(&self, id: u32, shadow: bool) -> Result<Capture, String> {
        capture::capture_window(id, shadow)
    }

    fn register_hotkeys(&self, bindings: &[(HotkeyAction, Hotkey)]) -> Vec<HotkeyAction> {
        hotkeys::register(bindings)
    }

    fn set_menu(&self, entries: &[MenuEntry]) {
        tray::set_menu(entries);
    }

    fn decorate(&self, window: &mut Window, glass: Glass) {
        glass::decorate(window, glass);
    }

    fn set_appearance(&self, choice: ThemeChoice) {
        misc::set_appearance(choice);
    }

    fn set_launch_at_login(&self, on: bool) -> Result<(), String> {
        misc::set_launch_at_login(on)
    }

    fn copy_files(&self, paths: &[PathBuf]) {
        misc::copy_files(paths);
    }

    fn system_screenshot_folder(&self) -> PathBuf {
        misc::system_screenshot_folder()
    }

    fn watch_folder(&self, path: Option<&Path>) {
        misc::watch_folder(path);
    }
}
