//! Capture flows: freeze the screen, then hand off to the overlay or straight
//! to the after-capture flow.

use std::time::Duration;

use gpui::{App, AppContext};
use oshot_core::bitmap::Bitmap;
use oshot_core::color::rgba;
use oshot_core::geometry::{Rect, rect};
use oshot_core::settings::SavedArea;

use crate::overlay::{self, Mode};
use crate::state::{Capture, DisplayInfo, Notice, Shot, notify, platform, state};
use crate::{onboarding, output, windows};

/// Checks Screen Recording permission; asks for it if missing.
fn permitted(cx: &mut App) -> bool {
    let p = platform(cx);
    if p.has_screen_permission() {
        return true;
    }
    p.request_screen_permission();
    onboarding::open_permission(cx);
    false
}

/// Captures `area` (global points) off the main thread and calls `then`.
fn grab(area: Rect, cx: &mut App, then: impl FnOnce(Shot, &mut App) + 'static) {
    let p = platform(cx);
    let task = cx.background_spawn(async move { p.capture_rect(area) });
    cx.spawn(async move |cx| {
        let result: Result<Capture, String> = task.await;
        cx.update(|cx| match result {
            Ok(c) => then(Shot::new(c.bitmap, c.scale), cx),
            Err(e) => notify(cx, Notice::Error(e)),
        });
    })
    .detach();
}

/// Freezes the display under the cursor and opens the overlay in `mode`.
pub fn begin(mode: Mode, cx: &mut App) {
    if overlay::close_all(cx) {
        return;
    }
    if !permitted(cx) {
        return;
    }
    let Some(display) = windows::cursor_display(cx) else {
        return;
    };
    if mode == Mode::Window {
        // Window mode works over the live screen, so there is nothing to
        // freeze; the overlay only needs a placeholder image.
        let window_list = platform(cx).windows();
        let empty = Shot::new(Bitmap::filled(1, 1, rgba(0, 0, 0, 0)), display.scale);
        overlay::open(mode, empty, display, window_list, cx);
        return;
    }
    grab(display.frame, cx, move |shot, cx| {
        overlay::open(mode, shot, display, Vec::new(), cx)
    });
}

pub fn full_screen(cx: &mut App) {
    if !permitted(cx) {
        return;
    }
    let Some(display) = windows::cursor_display(cx) else {
        return;
    };
    grab(display.frame, cx, output::finish);
}

/// Remembers an area so "Repeat Last Area" can capture it again.
pub fn remember_area(display: &DisplayInfo, area: Rect, cx: &mut App) {
    crate::state::update_settings(cx, |s| {
        s.last_area = Some(SavedArea {
            display: display.id,
            rect: area,
        })
    });
}

pub fn repeat_area(cx: &mut App) {
    let Some(last) = state(cx).settings.last_area.clone() else {
        notify(cx, Notice::Info("Capture an area first, then repeat it".into()));
        return;
    };
    if !permitted(cx) {
        return;
    }
    let displays = platform(cx).displays();
    let Some(display) = displays.iter().find(|d| d.id == last.display).or(displays.first()) else {
        return;
    };
    let r = last.rect;
    let area = rect(display.frame.x + r.x, display.frame.y + r.y, r.w, r.h);
    grab(area, cx, output::finish);
}

/// Waits, then freezes the screen for an area capture, so menus and hover
/// states can be opened first.
pub fn delayed(seconds: u32, cx: &mut App) {
    cx.spawn(async move |cx| {
        for left in (1..=seconds).rev() {
            cx.update(|cx| notify(cx, Notice::Info(format!("Capturing in {left}…"))));
            cx.background_executor().timer(Duration::from_secs(1)).await;
        }
        cx.update(|cx| {
            crate::toast::hide(cx);
            begin(Mode::Area, cx);
        });
    })
    .detach();
}
