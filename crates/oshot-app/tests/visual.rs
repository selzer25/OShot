//! Headless screenshots of every window, light and dark, rendered with the
//! real Metal renderer and CoreText. Saved to target/ui-snapshots for review,
//! and checked for blank or low-contrast output. Also times frame building.
//!
//! The native glass behind windows cannot be rendered offscreen, so each
//! window is drawn over a wallpaper-like gradient to judge its tints.

mod support;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gpui::{AnyWindowHandle, AppContext, HeadlessAppContext, Render, Size, Window, px, size};
use image::{GenericImageView, RgbaImage};
use oshot_app::converter::Converter;
use oshot_app::editor::{Editor, Panel};
use oshot_app::hud::Hud;
use oshot_app::onboarding::Onboarding;
use oshot_app::overlay::{Mode, Overlay};
use oshot_app::permission_guide::{GuidePanel, Permission};
use oshot_app::settings_window::{Page, SettingsWindow};
use oshot_app::state::{AppState, DisplayInfo, Notice, Paths, Platform, Shot};
use oshot_app::thumbnail::Thumbnail;
use oshot_app::toast::Toast;
use oshot_core::color::PALETTE;
use oshot_core::geometry::{IRect, pt, rect};
use oshot_core::model::{ArrowStyle, Shape, Style, Tool};
use oshot_core::settings::{Settings, ThemeChoice};
use support::*;

// Screenshot rendering must not compete with the frame-timing measurement.
static RENDER_LOCK: Mutex<()> = Mutex::new(());

fn out_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn context(theme: ThemeChoice) -> (HeadlessAppContext, Fixture) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(gpui_apple::AppleTextSystem::new()),
        Arc::new(oshot_app::assets::Assets),
        gpui_platform::current_headless_renderer,
    );
    let root = std::env::temp_dir().join(format!(
        "oshot-visual-{}-{:?}-{}",
        std::process::id(),
        theme,
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths::under(root.join("support"), root.join("Pictures"));
    Settings {
        onboarding_done: true,
        theme,
        ..Settings::default()
    }
    .save(&paths.settings)
    .unwrap();
    let shots = root.join("Desktop");
    std::fs::create_dir_all(&shots).unwrap();
    let platform = Arc::new(FakePlatform::new(shots));
    let p: Arc<dyn Platform> = platform.clone();
    let paths2 = paths.clone();
    cx.update(|cx| {
        oshot_app::init(paths2, p, cx);
        // Snapshots show every drawing finished, not its first frame.
        cx.set_reduce_motion(true);
    });
    cx.run_until_parked();
    (cx, Fixture { platform, paths, root })
}

/// Fraction of pixels far from the most common colour: a rough "is there
/// legible content" measure.
fn content_ratio(img: &RgbaImage) -> f32 {
    let mut counts = std::collections::HashMap::new();
    for p in img.pixels() {
        *counts.entry([p[0] / 8, p[1] / 8, p[2] / 8]).or_insert(0u32) += 1;
    }
    let (bg, _) = counts.iter().max_by_key(|(_, n)| **n).unwrap();
    let lum = |r: u8, g: u8, b: u8| 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
    let bgl = lum(bg[0] * 8, bg[1] * 8, bg[2] * 8);
    let far = img
        .pixels()
        .filter(|p| (lum(p[0], p[1], p[2]) - bgl).abs() > 90.0)
        .count();
    far as f32 / (img.width() * img.height()) as f32
}

struct Shots {
    dark: bool,
    saved: Vec<(String, f32)>,
}

impl Shots {
    fn take(&mut self, cx: &mut HeadlessAppContext, window: AnyWindowHandle, name: &str) {
        cx.run_until_parked();
        let _ = cx.update_window(window, |_, window, _| window.refresh());
        cx.run_until_parked();
        let img = cx.capture_screenshot(window).expect("Metal headless render");
        let ratio = content_ratio(&img);
        let file = out_dir().join(format!("{name}-{}.png", if self.dark { "dark" } else { "light" }));
        img.save(&file).unwrap();
        assert!(
            ratio > 0.004,
            "{name}: almost no visible content ({ratio:.4}) — see {}",
            file.display()
        );
        self.saved.push((name.to_string(), ratio));
    }

    /// Like `take`, for windows with traffic lights: the corner they sit
    /// in must hold nothing but background, at whatever size the window is.
    fn take_titled(&mut self, cx: &mut HeadlessAppContext, window: AnyWindowHandle, name: &str) {
        self.take(cx, window, name);
        let file = out_dir().join(format!("{name}-{}.png", if self.dark { "dark" } else { "light" }));
        let img = image::open(&file).unwrap().to_rgba8();
        let width = cx
            .update_window(window, |_, window, _| f32::from(window.viewport_size().width))
            .unwrap();
        let s = img.width() as f32 / width;
        let zone = image::imageops::crop_imm(&img, 0, 0, (TRAFFIC_LIGHTS.w * s) as u32, (TRAFFIC_LIGHTS.h * s) as u32)
            .to_image();
        let busy = drawn_fraction(&zone);
        assert!(
            busy < 0.003,
            "{name}: content drawn under the traffic lights ({:.1}% of that corner) — see {}",
            busy * 100.0,
            file.display()
        );
    }
}

/// Fraction of pixels that stand out from the region's average brightness:
/// near zero for plain background, even over a soft gradient.
fn drawn_fraction(img: &RgbaImage) -> f32 {
    let lum = |p: &image::Rgba<u8>| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
    let n = (img.width() * img.height()).max(1) as f32;
    let mean = img.pixels().map(lum).sum::<f32>() / n;
    img.pixels().filter(|p| (lum(p) - mean).abs() > 45.0).count() as f32 / n
}

/// Stands in for the desktop behind native glass: a wallpaper gradient
/// painted under the window's own (translucent) content.
struct Behind<V: 'static> {
    inner: gpui::Entity<V>,
}

impl<V: Render + 'static> Render for Behind<V> {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui::{ParentElement, Styled};
        let dark = oshot_app::theme::theme(window, cx).is_dark();
        let (a, b) = if dark {
            (0x1C2440, 0x462850)
        } else {
            (0x96BEEB, 0xEBCDE1)
        };
        gpui::div()
            .size_full()
            .bg(gpui::linear_gradient(
                120.0,
                gpui::linear_color_stop(oshot_app::theme::hex(a, 1.0), 0.0),
                gpui::linear_color_stop(oshot_app::theme::hex(b, 1.0), 1.0),
            ))
            .child(self.inner.clone())
    }
}

/// Stands in for the live screen under the window-mode overlay.
struct LiveScreen {
    screen: Arc<gpui::RenderImage>,
    overlay: gpui::Entity<Overlay>,
}

impl Render for LiveScreen {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui::{ParentElement, Styled};
        gpui::div()
            .size_full()
            .relative()
            .child(gpui::img(self.screen.clone()).absolute().size_full())
            .child(self.overlay.clone())
    }
}

fn open<V: Render + 'static>(
    cx: &mut HeadlessAppContext,
    w: f32,
    h: f32,
    build: impl FnOnce(&mut Window, &mut gpui::App) -> gpui::Entity<V>,
) -> Wrapped<V> {
    let mut inner = None;
    let handle = cx
        .open_window(size(px(w), px(h)), |window, cx| {
            let v = build(window, cx);
            inner = Some(v.clone());
            cx.new(|_| Behind { inner: v })
        })
        .unwrap();
    Wrapped {
        handle: handle.into(),
        inner: inner.unwrap(),
    }
}

struct Wrapped<V: 'static> {
    handle: AnyWindowHandle,
    inner: gpui::Entity<V>,
}

impl<V: 'static> Wrapped<V> {
    fn update<R>(
        &self,
        cx: &mut HeadlessAppContext,
        f: impl FnOnce(&mut V, &mut Window, &mut gpui::Context<V>) -> R,
    ) -> R {
        let inner = self.inner.clone();
        cx.update_window(self.handle, |_, window, cx| inner.update(cx, |v, cx| f(v, window, cx)))
            .unwrap()
    }
}

impl<V: 'static> From<&Wrapped<V>> for AnyWindowHandle {
    fn from(w: &Wrapped<V>) -> AnyWindowHandle {
        w.handle
    }
}

fn sample_shot() -> Shot {
    Shot::new(
        desktop().view().crop(IRect {
            x: 300,
            y: 200,
            w: 1700,
            h: 1100,
        }),
        2.0,
    )
}

fn annotate(e: &mut Editor) {
    let s = |tool: Tool| Style::for_tool(tool, 2.0);
    let d = &mut e.doc;
    let arrow = d.add(
        Shape::Arrow {
            from: pt(1250.0, 900.0),
            to: pt(980.0, 620.0),
            number: Some(3),
        },
        s(Tool::Arrow),
    );
    d.add(
        Shape::Text {
            origin: pt(0.0, 0.0),
            text: "Sign in here".into(),
            extent: pt(0.0, 0.0),
            arrow: Some(arrow),
        },
        s(Tool::Text),
    );
    d.add(
        Shape::Arrow {
            from: pt(150.0, 980.0),
            to: pt(560.0, 980.0),
            number: None,
        },
        Style {
            arrow: ArrowStyle::Double,
            color: PALETTE[4],
            ..s(Tool::Arrow)
        },
    );
    d.add(
        Shape::Rect {
            rect: rect(1380.0, 130.0, 300.0, 140.0),
        },
        Style {
            color: PALETTE[3],
            ..s(Tool::Rect)
        },
    );
    d.add(
        Shape::Oval {
            rect: rect(80.0, 560.0, 260.0, 160.0),
        },
        Style {
            color: PALETTE[1],
            hand_drawn: true,
            ..s(Tool::Oval)
        },
    );
    d.add(
        Shape::Highlighter {
            from: pt(150.0, 200.0),
            to: pt(900.0, 200.0),
        },
        s(Tool::Highlighter),
    );
    d.add(
        Shape::Text {
            origin: pt(1300.0, 760.0),
            text: "Click here".into(),
            extent: pt(0.0, 0.0),
            arrow: None,
        },
        s(Tool::Text),
    );
    d.add(
        Shape::Counter {
            center: pt(1300.0, 360.0),
            number: 1,
        },
        s(Tool::Counter),
    );
    d.add(
        Shape::Counter {
            center: pt(1400.0, 360.0),
            number: 2,
        },
        s(Tool::Counter),
    );
    d.add(
        Shape::Pixelate {
            rect: rect(500.0, 160.0, 380.0, 90.0),
        },
        s(Tool::Pixelate),
    );
    d.add(
        Shape::Magnifier {
            source: rect(1390.0, 10.0, 60.0, 60.0),
            center: pt(1550.0, 560.0),
            radius: 90.0,
        },
        s(Tool::Magnifier),
    );
    d.add(
        Shape::Ruler {
            from: pt(200.0, 1050.0),
            to: pt(800.0, 1050.0),
        },
        s(Tool::Ruler),
    );
    d.add(
        Shape::Freehand {
            points: (0..40)
                .map(|i| pt(900.0 + i as f32 * 8.0, 950.0 + (i as f32 * 0.4).sin() * 40.0))
                .collect(),
        },
        Style {
            color: PALETTE[5],
            ..s(Tool::Freehand)
        },
    );
}

fn run(theme: ThemeChoice) -> Vec<(String, f32)> {
    let (mut cx, fx) = context(theme);
    let mut shots = Shots {
        dark: theme == ThemeChoice::Dark,
        saved: Vec::new(),
    };

    use oshot_app::onboarding::{HEIGHT, Item, Practice, WIDTH};
    let w = open(&mut cx, WIDTH, HEIGHT, |window, cx| {
        cx.new(|cx| Onboarding::new(true, window, cx))
    });
    shots.take_titled(&mut cx, (&w).into(), "onboarding-welcome");
    for (name, focus, practice) in [
        ("onboarding-screen", Some(Item::ScreenRecording), Practice::Waiting),
        ("onboarding-accessibility", Some(Item::Accessibility), Practice::Waiting),
        ("onboarding-practice", Some(Item::Practice), Practice::Waiting),
        ("onboarding-practice-open", Some(Item::Practice), Practice::Open),
        ("onboarding-practice-done", Some(Item::Practice), Practice::Done(0)),
        ("onboarding-after", Some(Item::AfterCapture), Practice::Done(0)),
        ("onboarding-all-set", None, Practice::Done(0)),
    ] {
        let w = open(&mut cx, WIDTH, HEIGHT, |window, cx| {
            cx.new(|cx| Onboarding::new(false, window, cx))
        });
        w.update(&mut cx, |o, _, cx| {
            o.focus = focus;
            o.practice = practice;
            o.after_chosen = focus.is_none();
            cx.notify();
        });
        shots.take_titled(&mut cx, (&w).into(), name);
    }
    let w = open(&mut cx, WIDTH, HEIGHT, |window, cx| {
        cx.new(|cx| Onboarding::new(false, window, cx))
    });
    w.update(&mut cx, |o, _, cx| {
        o.focus = Some(Item::ScreenRecording);
        o.waiting = Some(Item::ScreenRecording);
        cx.notify();
    });
    shots.take_titled(&mut cx, (&w).into(), "onboarding-waiting");
    let (pw, ph) = (
        oshot_app::permission_guide::PANEL_W,
        oshot_app::permission_guide::PANEL_H,
    );
    let guide = open(&mut cx, pw, ph, |_, cx| {
        cx.new(|_| GuidePanel::new(Permission::ScreenRecording, "/Applications/OShot.app".into()))
    });
    shots.take(&mut cx, (&guide).into(), "permission-guide");

    let w = open(&mut cx, 820.0, 640.0, |_, cx| cx.new(SettingsWindow::new));
    for page in Page::ALL {
        w.update(&mut cx, |s, _, cx| s.set_page(page, cx));
        shots.take_titled(
            &mut cx,
            (&w).into(),
            &format!("settings-{}", page.title().to_lowercase()),
        );
    }

    let editor = open(&mut cx, 1180.0, 820.0, |_, cx| {
        cx.new(|cx| Editor::new(sample_shot(), cx))
    });
    editor.update(&mut cx, |e, _, cx| {
        annotate(e);
        e.selected = e.doc.objects.first().map(|o| o.id);
        cx.notify();
    });
    shots.take_titled(&mut cx, (&editor).into(), "editor-annotated");
    for (panel, name) in [
        (Panel::Backdrop, "editor-backdrop"),
        (Panel::Export, "editor-export"),
        (Panel::Resize, "editor-resize"),
    ] {
        editor.update(&mut cx, |e, _, cx| {
            e.panel = Some(panel);
            if panel == Panel::Backdrop {
                let mut b = e.doc.backdrop.clone();
                b.enabled = true;
                b.preset = 3;
                e.doc.set_backdrop(b);
                e.zoom = None;
            }
            cx.notify();
        });
        shots.take_titled(&mut cx, (&editor).into(), name);
    }
    editor.update(&mut cx, |e, _, cx| {
        e.panel = None;
        e.doc.backdrop.enabled = false;
        e.tool = Tool::Select;
        e.selected = None;
        e.selection = Some(rect(400.0, 300.0, 600.0, 400.0));
        cx.notify();
    });
    shots.take_titled(&mut cx, (&editor).into(), "editor-selection");

    let conv = open(&mut cx, 620.0, 700.0, |_, cx| cx.new(Converter::new));
    shots.take_titled(&mut cx, (&conv).into(), "converter-empty");
    let small = open(&mut cx, 480.0, 320.0, |_, cx| cx.new(Converter::new));
    shots.take_titled(&mut cx, (&small).into(), "converter-smallest");
    conv.update(&mut cx, |c, _, cx| {
        c.add_bytes(
            "Screenshot 1.png".into(),
            None,
            png_bytes(
                &desktop().view().crop(IRect {
                    x: 0,
                    y: 0,
                    w: 800,
                    h: 500,
                }),
                2,
            ),
            cx,
        );
        c.add_bytes("photo.heic".into(), None, b"not really".to_vec(), cx);
    });
    shots.take_titled(&mut cx, (&conv).into(), "converter-items");

    let display = DisplayInfo {
        id: 1,
        frame: rect(0.0, 0.0, W, H),
        scale: SCALE,
    };
    let full = Shot::new(desktop().clone(), SCALE);
    let ov = open(&mut cx, W, H, |_, cx| {
        cx.new(|cx| Overlay::new(Mode::Area, full.clone(), display.clone(), Vec::new(), cx))
    });
    ov.update(&mut cx, |o, _, cx| {
        o.selection = Some(rect(180.0, 130.0, 760.0, 500.0));
        cx.notify();
    });
    shots.take(&mut cx, (&ov).into(), "overlay-area");
    // Window mode draws over the live screen, so the desktop goes behind it.
    let wins = fx.platform.windows();
    let screen = full.image.clone();
    let handle = cx
        .open_window(size(px(W), px(H)), |_, cx| {
            let overlay = cx.new(|cx| Overlay::new(Mode::Window, full.clone(), display.clone(), wins, cx));
            overlay.update(cx, |o, cx| {
                o.hovered_window = Some(0);
                cx.notify();
            });
            cx.new(|_| LiveScreen { screen, overlay })
        })
        .unwrap();
    shots.take(&mut cx, handle.into(), "overlay-window");

    let (hw, hh) = oshot_app::hud::bar_size(oshot_app::hud::items(3).len());
    let hud = open(&mut cx, hw, hh, |_, cx| cx.new(Hud::new));
    shots.take(&mut cx, (&hud).into(), "capture-menu");
    let th = open(&mut cx, 260.0, 230.0, |_, cx| {
        cx.new(|_| Thumbnail {
            shot: sample_shot(),
            hovered: false,
            generation: 0,
        })
    });
    shots.take(&mut cx, (&th).into(), "thumbnail");
    let toast = open(&mut cx, 340.0, 44.0, |_, cx| {
        cx.new(|_| Toast::new(Notice::Copied("Copied to clipboard".into())))
    });
    shots.take(&mut cx, (&toast).into(), "toast");

    let _ = cx.update(|cx| cx.global::<AppState>().settings.theme);
    shots.saved
}

#[test]
fn arrow_label_at_the_top_renders_as_fully_as_an_interior_label() {
    let _render = RENDER_LOCK.lock().unwrap();
    let (mut cx, _fx) = context(ThemeChoice::Dark);
    let editor = open(&mut cx, 1000.0, 750.0, |_, cx| {
        cx.new(|cx| {
            Editor::new(
                Shot::new(
                    oshot_core::bitmap::Bitmap::filled(800, 500, oshot_core::color::rgb(255, 255, 255)),
                    1.0,
                ),
                cx,
            )
        })
    });
    editor.update(&mut cx, |e, _, cx| {
        e.set_zoom(Some(1.0), cx);
        for y in [5.0, 205.0] {
            let arrow = e.doc.add(
                Shape::Arrow {
                    from: pt(250.0, y),
                    to: pt(450.0, y + 195.0),
                    number: None,
                },
                Style::for_tool(Tool::Arrow, 1.0),
            );
            e.doc.add(
                Shape::Text {
                    origin: pt(0.0, 0.0),
                    text: "Entire label".into(),
                    extent: pt(0.0, 0.0),
                    arrow: Some(arrow),
                },
                Style::for_tool(Tool::Text, 1.0),
            );
        }
        cx.notify();
    });
    cx.run_until_parked();
    let img = cx.capture_screenshot((&editor).into()).unwrap();
    img.save(out_dir().join("editor-top-label.png")).unwrap();
    let scale = editor.update(&mut cx, |_, window, _| {
        img.width() as f32 / f32::from(window.viewport_size().width)
    });
    let boxes = editor.update(&mut cx, |e, _, _| {
        e.doc
            .objects
            .iter()
            .filter(|o| matches!(o.shape, Shape::Text { .. }))
            .map(|o| {
                let b = o.bounds();
                assert!(b.y >= 0.0, "label lies inside the image: {b:?}");
                (e.screen_point(pt(b.x, b.y)), b)
            })
            .collect::<Vec<_>>()
    });
    let dark_pixels = |(at, b): &(gpui::Point<gpui::Pixels>, oshot_core::geometry::Rect)| {
        let x = (f32::from(at.x) * scale).round() as u32;
        let y = (f32::from(at.y) * scale).round() as u32;
        image::imageops::crop_imm(&img, x, y, (b.w * scale).ceil() as u32, (b.h * scale).ceil() as u32)
            .pixels()
            .filter(|(_, _, p)| p[0] < 100 && p[1] < 100 && p[2] < 100)
            .count()
    };
    let (top, interior) = (dark_pixels(&boxes[0]), dark_pixels(&boxes[1]));
    assert!(interior > 100, "reference label is visible");
    assert!(
        top as f32 >= interior as f32 * 0.9,
        "top label must not fade or clip: {top} vs {interior} dark pixels"
    );
}

#[test]
fn every_window_light_and_dark() {
    let _render = RENDER_LOCK.lock().unwrap();
    for theme in [ThemeChoice::Light, ThemeChoice::Dark] {
        let saved = run(theme);
        eprintln!("{theme:?}: {} screenshots", saved.len());
        for (name, ratio) in saved {
            eprintln!("  {name:<22} content {:.1}%", ratio * 100.0);
        }
    }
    eprintln!("saved to {}", out_dir().display());
}

/// Times building a frame of the editor with 300 annotations. Run with
/// `--release` for numbers that match the shipped app.
#[test]
fn editor_frame_time_with_many_objects() {
    let _render = RENDER_LOCK.lock().unwrap();
    let (mut cx, _fx) = context(ThemeChoice::Dark);
    let editor = open(&mut cx, 1440.0, 900.0, |_, cx| {
        cx.new(|cx| Editor::new(Shot::new(desktop().clone(), 2.0), cx))
    });
    editor.update(&mut cx, |e, _, _| {
        for i in 0..25 {
            annotate(e);
            for o in e.doc.objects.iter_mut().rev().take(12) {
                o.translate(i as f32 * 7.0, i as f32 * 5.0);
            }
        }
    });
    cx.run_until_parked();
    let n = 30;
    let mut times = Vec::new();
    for i in 0..n {
        let t = Instant::now();
        editor.update(&mut cx, |e, _, cx| {
            e.hover = Some(pt(100.0 + i as f32, 100.0));
            cx.notify();
        });
        cx.run_until_parked();
        let _ = cx.update_window((&editor).into(), |_, window, _| window.refresh());
        cx.run_until_parked();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let gpu = Instant::now();
    let _ = cx.capture_screenshot((&editor).into()).unwrap();
    let gpu_ms = gpu.elapsed().as_secs_f64() * 1000.0;
    times.sort_by(|a, b| a.total_cmp(b));
    let objects = editor.update(&mut cx, |e, _, _| e.doc.objects.len());
    eprintln!(
        "editor, {objects} objects: frame build median {:.2} ms, worst {:.2} ms; GPU render + readback {:.1} ms",
        times[n / 2],
        times[n - 1],
        gpu_ms
    );
    if !cfg!(debug_assertions) {
        assert!(times[n - 1] < 16.0, "frames must build within one 60 Hz frame");
    }
    let _: Size<gpui::Pixels> = size(px(1.), px(1.));
}
