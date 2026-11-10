//! OShot: a tiny GPUI menu bar screenshot app for macOS.

pub mod art;
pub mod assets;
pub mod capture;
pub mod commands;
pub mod converter;
pub mod doodle;
pub mod editor;
pub mod hud;
pub mod onboarding;
pub mod organize;
pub mod output;
pub mod overlay;
pub mod permission_guide;
pub mod pin;
pub mod platform;
pub mod recent;
pub mod settings_window;
pub mod state;
pub mod theme;
pub mod thumbnail;
pub mod toast;
pub mod ui;
pub mod windows;

use std::sync::Arc;

use gpui::{App, KeyBinding, actions};

use crate::state::{AppState, Command, Paths, Platform};

actions!(oshot, [Quit, OpenSettings]);

/// Sets up global state and everything that follows from settings. Shared by
/// `main` and the tests, so tests start the app the way users do.
pub fn init(paths: Paths, platform: Arc<dyn Platform>, cx: &mut App) {
    let state = AppState::new(paths, platform.clone());
    let theme = state.settings.theme;
    let first_run = !state.settings.onboarding_done;
    cx.set_global(state);
    platform.set_appearance(theme);
    commands::apply_hotkeys(cx);
    commands::apply_gesture(cx);
    commands::rebuild_menu(cx);
    organize::apply_watcher(cx);
    organize::schedule(cx);
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &OpenSettings, cx| commands::dispatch(Command::Settings, cx));
    if first_run {
        onboarding::open(cx);
    }
}
