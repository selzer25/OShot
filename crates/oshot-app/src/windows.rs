//! Opening windows consistently: glass, placement and single instances.

use gpui::{
    AnyWindowHandle, App, Bounds, DisplayId, Entity, Pixels, Point, Render, Size, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, point, px, size,
};

use crate::state::{DisplayInfo, Glass, platform};

pub struct Spec {
    pub size: Size<Pixels>,
    /// Display-relative origin; `None` centres the window.
    pub origin: Option<Point<Pixels>>,
    pub display: Option<DisplayInfo>,
    pub kind: WindowKind,
    pub glass: Option<Glass>,
    pub focus: bool,
    pub title: Option<&'static str>,
    pub resizable: bool,
    pub min_size: Option<Size<Pixels>>,
}

impl Spec {
    pub fn window(title: &'static str, w: f32, h: f32) -> Spec {
        Spec {
            size: size(px(w), px(h)),
            origin: None,
            display: None,
            kind: WindowKind::Normal,
            glass: Some(Glass::Window),
            focus: true,
            title: Some(title),
            resizable: true,
            min_size: Some(size(px(w.min(480.)), px(h.min(320.)))),
        }
    }

    pub fn panel(w: f32, h: f32, radius: u32) -> Spec {
        Spec {
            size: size(px(w), px(h)),
            origin: None,
            display: None,
            kind: WindowKind::PopUp,
            glass: Some(Glass::Panel { radius }),
            focus: false,
            title: None,
            resizable: false,
            min_size: None,
        }
    }
}

/// The display the cursor is on, or the first one.
pub fn cursor_display(cx: &App) -> Option<DisplayInfo> {
    let p = platform(cx);
    let cursor = p.cursor();
    let displays = p.displays();
    displays
        .iter()
        .find(|d| d.frame.contains(cursor))
        .cloned()
        .or_else(|| displays.first().cloned())
}

pub fn open<V: Render + 'static>(
    spec: Spec,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> Option<WindowHandle<V>> {
    let display = spec.display.clone().or_else(|| cursor_display(cx));
    let display_id = display.as_ref().map(|d| DisplayId::new(d.id as u64));
    let bounds = match (spec.origin, &display) {
        (Some(origin), _) => Bounds {
            origin,
            size: spec.size,
        },
        (None, Some(d)) => Bounds {
            origin: point(
                px(((d.frame.w - f32::from(spec.size.width)) / 2.0).max(0.0)),
                px(((d.frame.h - f32::from(spec.size.height)) / 2.5).max(0.0)),
            ),
            size: spec.size,
        },
        (None, None) => Bounds::centered(None, spec.size, cx),
    };
    let titlebar = spec.title.map(|title| TitlebarOptions {
        title: Some(title.into()),
        appears_transparent: true,
        traffic_light_position: Some(point(px(14.), px(14.))),
    });
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar,
        focus: spec.focus,
        show: true,
        kind: spec.kind,
        is_movable: true,
        is_resizable: spec.resizable,
        display_id,
        // Glass windows show the native glass through; the overlay paints
        // every pixel itself, so transparent suits both.
        window_background: WindowBackgroundAppearance::Transparent,
        window_min_size: spec.min_size,
        ..Default::default()
    };
    let glass = spec.glass;
    let handle = cx.open_window(options, build).ok()?;
    if let Some(glass) = glass {
        let p = platform(cx);
        let _ = handle.update(cx, |_, window, _| p.decorate(window, glass));
    }
    if spec.focus {
        cx.activate(true);
    }
    Some(handle)
}

/// The open window whose root view is `V`, if any.
pub fn find<V: 'static>(cx: &App) -> Option<WindowHandle<V>> {
    cx.windows()
        .into_iter()
        .find_map(|w: AnyWindowHandle| w.downcast::<V>())
}

pub fn find_all<V: 'static>(cx: &App) -> Vec<WindowHandle<V>> {
    cx.windows()
        .into_iter()
        .filter_map(|w: AnyWindowHandle| w.downcast::<V>())
        .collect()
}

/// Brings an existing single-instance window forward. Returns false if
/// there is none.
pub fn activate_existing<V: 'static>(cx: &mut App) -> bool {
    let Some(handle) = find::<V>(cx) else { return false };
    let ok = handle.update(cx, |_, window, _| window.activate_window()).is_ok();
    if ok {
        cx.activate(true);
    }
    ok
}

pub fn close<V: 'static>(handle: WindowHandle<V>, cx: &mut App) {
    let _ = handle.update(cx, |_, window, _| window.remove_window());
}
