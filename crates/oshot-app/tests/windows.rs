//! Window chrome: content in every titled window stays clear of the traffic
//! lights at its default size and squeezed to its minimum, where a column
//! that overflows used to slide its first row up under them.

mod support;

use std::time::Duration;

use gpui::{VisualTestContext, px, size};
use oshot_app::commands::dispatch;
use oshot_app::converter::Converter;
use oshot_app::editor::Editor;
use oshot_app::onboarding::Onboarding;
use oshot_app::settings_window::SettingsWindow;
use oshot_app::state::{Command, Shot};
use oshot_core::bitmap::Bitmap;
use oshot_core::color::rgb;
use support::*;

/// Default size, then the smallest the window allows.
fn at_each_size(w: &mut VisualTestContext, name: &str, resizable: bool, check: impl Fn(&mut VisualTestContext, &str)) {
    w.run_until_parked();
    check(w, &format!("{name} at default size"));
    if resizable {
        w.simulate_resize(size(px(480.), px(320.)));
        w.run_until_parked();
        check(w, &format!("{name} at 480×320"));
    }
}

#[test]
fn converter_title_never_runs_under_the_traffic_lights() {
    let cx = &mut real_cx();
    let _fx = start(cx);
    cx.update(|cx| dispatch(Command::ConvertImage, cx));
    cx.run_until_parked();
    let (_, mut w) = window_cx::<Converter>(cx);
    at_each_size(&mut w, "converter", true, |w, ctx| {
        assert_clear_of_traffic_lights(w, "window-body", ctx);
        let body = w.debug_bounds("window-body").unwrap();
        assert!(
            f32::from(body.origin.y) >= oshot_app::ui::TITLEBAR_H,
            "{ctx}: the body starts below the titlebar"
        );
    });
}

#[test]
fn every_titled_window_keeps_clear_of_the_traffic_lights() {
    let cx = &mut real_cx();
    let _fx = start(cx);

    cx.update(|cx| dispatch(Command::Settings, cx));
    cx.run_until_parked();
    let (_, mut w) = window_cx::<SettingsWindow>(cx);
    at_each_size(&mut w, "settings", true, |w, ctx| {
        assert_clear_of_traffic_lights(w, "nav-general", ctx);
    });

    cx.update(|cx| {
        oshot_app::editor::open_with(Shot::new(Bitmap::filled(800, 500, rgb(200, 200, 200)), 2.0), None, cx)
    });
    cx.run_until_parked();
    let (_, mut w) = window_cx::<Editor>(cx);
    at_each_size(&mut w, "editor", true, |w, ctx| {
        assert_clear_of_traffic_lights(w, "tool-v", ctx);
        assert_clear_of_traffic_lights(w, "editor-canvas", ctx);
    });

    cx.update(|cx| dispatch(Command::Onboarding, cx));
    cx.run_until_parked();
    let (_, mut w) = window_cx::<Onboarding>(cx);
    at_each_size(&mut w, "welcome", false, |w, ctx| {
        assert_clear_of_traffic_lights(w, "get-started", ctx)
    });
    click(&mut w, "get-started");
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    at_each_size(&mut w, "onboarding", false, |w, ctx| {
        assert_clear_of_traffic_lights(w, "window-body", ctx);
        assert_clear_of_traffic_lights(w, "row-screen", ctx);
    });
}
