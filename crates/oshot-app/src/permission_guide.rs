//! Drag-to-grant: while System Settings shows a privacy list, a small panel
//! sits just under its window with OShot's icon to drag into the list.
//!
//! macOS adds a `.app` dropped on the Screen Recording or Accessibility
//! list, which rescues the common case where OShot is missing from it (or
//! listed under an old signature). The panel follows the Settings window,
//! hides while the user is elsewhere, and goes away once access is granted.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    App, AppContext, Context, CursorStyle, FontWeight, Global, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Render, Styled, Task, Window, WindowHandle, div, img, point, px,
};
use oshot_core::geometry::{Rect, rect};

use crate::doodle::doodle;
use crate::state::{DisplayInfo, platform};
use crate::theme::{UI_FONT, paper};
use crate::windows::{self, Spec};
use crate::{art, commands};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    ScreenRecording,
    Accessibility,
}

impl Permission {
    pub fn granted(self, cx: &App) -> bool {
        let p = platform(cx);
        match self {
            Permission::ScreenRecording => p.has_screen_permission(),
            Permission::Accessibility => p.has_accessibility(),
        }
    }
}

/// Panel size including a margin for its drawn shadow.
pub const PANEL_W: f32 = 404.;
pub const PANEL_H: f32 = 132.;
const SHADOW: f32 = 12.;
/// System Settings' sidebar; the panel centres on the pane to its right.
const SIDEBAR: f32 = 230.;
const TICK: Duration = Duration::from_millis(150);
/// 2000 ticks of 150 ms: five minutes, then the helper gives up.
const LIFETIME_TICKS: u32 = 2000;
/// 200 ticks of 150 ms: thirty seconds without seeing System Settings.
const UNSEEN_TICKS: u32 = 200;
/// Check the grant every 7th tick, about once a second.
const GRANT_EVERY: u32 = 7;

/// Asks macOS for `permission` the way it allows: the system prompt (which
/// also lists OShot), then the right System Settings pane with the helper.
pub fn ask(permission: Permission, cx: &mut App) {
    let p = platform(cx);
    match permission {
        Permission::ScreenRecording => {
            p.request_screen_permission();
            p.open_privacy_settings();
        }
        Permission::Accessibility => {
            p.request_accessibility();
            p.open_accessibility_settings();
        }
    }
    start(permission, cx);
}

#[derive(Default)]
struct Guide {
    permission: Option<Permission>,
    panel: Option<WindowHandle<GuidePanel>>,
    _tracker: Option<Task<()>>,
}

impl Global for Guide {}

pub struct GuidePanel {
    pub permission: Permission,
    app: PathBuf,
    shown_at: Instant,
}

/// The panel's frame for a Settings window at `settings`: centred under its
/// content pane, or tucked inside its bottom edge when the screen ends.
pub fn panel_frame(settings: Rect, display: Option<&DisplayInfo>) -> Rect {
    let sidebar = if settings.w > 640. { SIDEBAR } else { 0. };
    let x = settings.x + sidebar + (settings.w - sidebar - PANEL_W) / 2.;
    let below = settings.bottom() - SHADOW + 8.;
    let y = match display {
        Some(d) if below + PANEL_H > d.frame.bottom() => settings.bottom() - PANEL_H - 10.,
        _ => below,
    };
    rect(x.round(), y.round(), PANEL_W, PANEL_H)
}

/// Shows the helper for `permission` while the user is in System Settings.
/// Does nothing outside an app bundle: macOS attributes a bare binary's
/// permissions to the terminal that started it.
pub fn start(permission: Permission, cx: &mut App) {
    let Some(app) = platform(cx).app_bundle() else { return };
    if cx.try_global::<Guide>().and_then(|g| g.permission) == Some(permission) {
        return;
    }
    stop(cx);
    let tracker = cx.spawn(async move |cx| {
        let mut placed: Option<Rect> = None;
        let mut unseen = 0u32;
        for tick in 1..=LIFETIME_TICKS {
            cx.background_executor().timer(TICK).await;
            let keep = cx.update(|cx| {
                if tick % GRANT_EVERY == 0 && permission.granted(cx) {
                    return false;
                }
                let p = platform(cx);
                let Some(settings) = p.system_settings_frame() else {
                    if placed.take().is_some() {
                        close_panel(cx);
                    }
                    unseen += 1;
                    return unseen < UNSEEN_TICKS;
                };
                unseen = 0;
                let displays = p.displays();
                let display = displays.iter().find(|d| d.frame.contains(settings.center()));
                let frame = panel_frame(settings, display);
                if placed == Some(frame) {
                    return true;
                }
                placed = Some(frame);
                match cx.global::<Guide>().panel {
                    Some(h) => {
                        let _ = h.update(cx, |_, window, _| p.place_window(window, frame));
                    }
                    None => open_panel(permission, app.clone(), frame, display.cloned(), cx),
                }
                true
            });
            if !keep {
                break;
            }
        }
        cx.update(stop);
    });
    cx.set_global(Guide {
        permission: Some(permission),
        panel: None,
        _tracker: Some(tracker),
    });
}

fn open_panel(permission: Permission, app: PathBuf, frame: Rect, display: Option<DisplayInfo>, cx: &mut App) {
    let mut spec = Spec::panel(PANEL_W, PANEL_H, 0);
    spec.glass = None;
    if let Some(d) = &display {
        spec.origin = Some(point(px(frame.x - d.frame.x), px(frame.y - d.frame.y)));
    }
    spec.display = display;
    let handle = windows::open(spec, cx, |_, cx| cx.new(|_| GuidePanel::new(permission, app)));
    cx.global_mut::<Guide>().panel = handle;
}

impl GuidePanel {
    pub fn new(permission: Permission, app: PathBuf) -> GuidePanel {
        GuidePanel {
            permission,
            app,
            shown_at: Instant::now(),
        }
    }
}

fn close_panel(cx: &mut App) {
    if let Some(h) = cx.global_mut::<Guide>().panel.take() {
        windows::close(h, cx);
    }
}

/// Removes the helper. Also called once access is granted.
pub fn stop(cx: &mut App) {
    if cx.try_global::<Guide>().is_none() {
        return;
    }
    let granted_accessibility =
        cx.global::<Guide>().permission == Some(Permission::Accessibility) && Permission::Accessibility.granted(cx);
    close_panel(cx);
    cx.set_global(Guide::default());
    if granted_accessibility {
        commands::apply_gesture(cx);
    }
}

impl Render for GuidePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = paper(window, cx);
        let body = match self.permission {
            Permission::ScreenRecording => "Drop it on the Screen Recording list above, then switch it on.",
            Permission::Accessibility => "Drop it on the Accessibility list above, then switch it on.",
        };
        div().size_full().p(px(SHADOW)).font_family(UI_FONT).child(
            div()
                .size_full()
                .flex()
                .items_center()
                .gap(px(14.))
                .px(px(14.))
                .rounded(px(20.))
                .border_1()
                .border_color(p.track)
                .bg(p.bg)
                .shadow_lg()
                .child(
                    div()
                        .id("drag-app")
                        .debug_selector(|| "drag-app".into())
                        .relative()
                        .flex_none()
                        .size(px(80.))
                        .cursor(CursorStyle::OpenHand)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                platform(cx).begin_file_drag(window, &this.app);
                            }),
                        )
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .child(doodle(&art::DRAG_FRAME, p, self.shown_at, 80., 80.)),
                        )
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(img("brand/mark.svg").size(px(54.))),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.ink)
                                .child("Drag OShot into the list"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(17.))
                                .text_color(p.soft)
                                .child(body),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .child(doodle(&art::UP_ARROW, p, self.shown_at, 34., 44.)),
                ),
        )
    }
}
