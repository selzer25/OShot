//! Shared test fixture: a fake macOS layer and an app started the way
//! `main` starts it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use gpui::{
    AnyWindowHandle, Bounds, ClipboardEntry, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext,
    WindowHandle, point, px,
};
use oshot_app::state::{AppState, Capture, DisplayInfo, Glass, MenuEntry, Notice, Paths, Platform, WindowInfo};
use oshot_core::bitmap::Bitmap;
use oshot_core::color::{Color, rgb};
use oshot_core::effects::EffectCache;
use oshot_core::geometry::{IRect, Pt, Rect, pt, rect};
use oshot_core::gesture::MenuGesture;
use oshot_core::model::{Document, Shape, Style, Tool};
use oshot_core::settings::{Hotkey, HotkeyAction, Settings, ThemeChoice};
use oshot_native::render::{ExportOptions, render};

pub const W: f32 = 1440.0;
pub const H: f32 = 900.0;
pub const SCALE: f32 = 2.0;

/// Points on the fake desktop where things are drawn.
pub const WINDOW_A: Rect = rect(200.0, 150.0, 700.0, 450.0);
pub const RED_BUTTON: Rect = rect(1000.0, 200.0, 120.0, 40.0);
pub const BOX_LEFT: Rect = rect(300.0, 700.0, 100.0, 60.0);
pub const BOX_RIGHT: Rect = rect(500.0, 700.0, 100.0, 60.0);
pub const TEXT_AT: Pt = pt(220.0, 180.0);
pub const RED: Color = rgb(230, 20, 20);
pub const WINDOW_GREY: Color = rgb(60, 60, 70);

fn scaled(r: Rect) -> Rect {
    rect(r.x * SCALE, r.y * SCALE, r.w * SCALE, r.h * SCALE)
}

/// A 2x "desktop" with known content: a window, a red button, two boxes
/// with a 100 pt gap between them, and a line of text with an email.
pub fn desktop() -> &'static Bitmap {
    static DESKTOP: OnceLock<Bitmap> = OnceLock::new();
    DESKTOP.get_or_init(|| {
        let (w, h) = ((W * SCALE) as u32, (H * SCALE) as u32);
        let mut base = Bitmap::filled(w, h, rgb(255, 255, 255));
        base.fill_rect(scaled(WINDOW_A).to_pixels(w, h), WINDOW_GREY);
        base.fill_rect(
            IRect {
                x: 440,
                y: 340,
                w: 1360,
                h: 120,
            },
            rgb(255, 255, 255),
        );
        base.fill_rect(scaled(RED_BUTTON).to_pixels(w, h), RED);
        base.fill_rect(scaled(BOX_LEFT).to_pixels(w, h), rgb(0, 0, 0));
        base.fill_rect(scaled(BOX_RIGHT).to_pixels(w, h), rgb(0, 0, 0));
        let mut doc = Document::new(w, h, SCALE);
        let style = Style {
            color: rgb(0, 0, 0),
            text_size: 56.0,
            text_background: false,
            ..Style::for_tool(Tool::Text, SCALE)
        };
        doc.add(
            Shape::Text {
                origin: pt(TEXT_AT.x * SCALE + 20.0, TEXT_AT.y * SCALE + 30.0),
                text: "Contact max@example.com".into(),
                extent: pt(0.0, 0.0),
                arrow: None,
            },
            style,
        );
        render(&doc, base.view(), &mut EffectCache::default(), ExportOptions::default())
    })
}

#[derive(Default)]
pub struct Calls {
    pub hotkeys: Vec<Vec<(HotkeyAction, Hotkey)>>,
    pub menus: Vec<Vec<MenuEntry>>,
    pub appearance: Vec<ThemeChoice>,
    pub login: Vec<bool>,
    pub copied_files: Vec<PathBuf>,
    pub watched: Vec<Option<PathBuf>>,
    pub captured_rects: Vec<Rect>,
    pub captured_windows: Vec<(u32, bool)>,
    pub decorated: Vec<Glass>,
    pub requested_permission: usize,
    pub opened_privacy: usize,
    pub requested_accessibility: usize,
    pub opened_accessibility: usize,
    pub gestures: Vec<MenuGesture>,
    pub file_drags: Vec<PathBuf>,
    pub placed: Vec<Rect>,
    pub covered: Vec<Rect>,
}

pub struct FakePlatform {
    pub permission: AtomicBool,
    pub accessibility: AtomicBool,
    /// System Settings' window while it is "frontmost".
    pub settings_window: Mutex<Option<Rect>>,
    pub bundle: Option<PathBuf>,
    pub calls: Mutex<Calls>,
    pub screenshot_folder: PathBuf,
    pub fail_hotkeys: Mutex<Vec<HotkeyAction>>,
    pub cursor_moves: AtomicUsize,
}

impl FakePlatform {
    pub fn new(screenshot_folder: PathBuf) -> FakePlatform {
        FakePlatform {
            permission: AtomicBool::new(true),
            accessibility: AtomicBool::new(true),
            settings_window: Mutex::new(None),
            bundle: Some(PathBuf::from("/Applications/OShot.app")),
            calls: Mutex::new(Calls::default()),
            screenshot_folder,
            fail_hotkeys: Mutex::new(Vec::new()),
            cursor_moves: AtomicUsize::new(0),
        }
    }

    pub fn calls(&self) -> std::sync::MutexGuard<'_, Calls> {
        self.calls.lock().unwrap()
    }

    pub fn last_menu(&self) -> Vec<MenuEntry> {
        self.calls().menus.last().cloned().unwrap_or_default()
    }

    pub fn last_hotkeys(&self) -> Vec<(HotkeyAction, Hotkey)> {
        self.calls().hotkeys.last().cloned().unwrap_or_default()
    }
}

impl Platform for FakePlatform {
    fn has_screen_permission(&self) -> bool {
        self.permission.load(Ordering::SeqCst)
    }
    fn request_screen_permission(&self) {
        self.calls().requested_permission += 1;
    }
    fn open_privacy_settings(&self) {
        self.calls().opened_privacy += 1;
    }
    fn has_accessibility(&self) -> bool {
        self.accessibility.load(Ordering::SeqCst)
    }
    fn request_accessibility(&self) {
        self.calls().requested_accessibility += 1;
    }
    fn open_accessibility_settings(&self) {
        self.calls().opened_accessibility += 1;
    }
    fn set_menu_gesture(&self, gesture: MenuGesture) {
        self.calls().gestures.push(gesture);
    }
    fn app_bundle(&self) -> Option<PathBuf> {
        self.bundle.clone()
    }
    fn system_settings_frame(&self) -> Option<Rect> {
        *self.settings_window.lock().unwrap()
    }
    fn begin_file_drag(&self, _window: &mut gpui::Window, path: &Path) -> bool {
        self.calls().file_drags.push(path.to_path_buf());
        true
    }
    fn place_window(&self, _window: &mut gpui::Window, frame: Rect) {
        self.calls().placed.push(frame);
    }
    fn cover_display(&self, _window: &mut gpui::Window, frame: Rect) {
        self.calls().covered.push(frame);
    }
    fn displays(&self) -> Vec<DisplayInfo> {
        vec![DisplayInfo {
            id: 1,
            frame: rect(0.0, 0.0, W, H),
            scale: SCALE,
        }]
    }
    fn cursor(&self) -> Pt {
        pt(100.0, 100.0)
    }
    fn windows(&self) -> Vec<WindowInfo> {
        vec![
            WindowInfo {
                id: 11,
                frame: WINDOW_A,
                owner: "Safari".into(),
                title: "Docs".into(),
            },
            WindowInfo {
                id: 12,
                frame: rect(1000.0, 400.0, 300.0, 300.0),
                owner: "Notes".into(),
                title: String::new(),
            },
        ]
    }
    fn capture_rect(&self, r: Rect) -> Result<Capture, String> {
        self.calls().captured_rects.push(r);
        let d = desktop();
        let px = scaled(r).to_pixels(d.width, d.height);
        Ok(Capture {
            bitmap: d.view().crop(px),
            scale: SCALE,
        })
    }
    fn capture_window(&self, id: u32, shadow: bool) -> Result<Capture, String> {
        self.calls().captured_windows.push((id, shadow));
        Ok(Capture {
            bitmap: Bitmap::filled(1400, 900, WINDOW_GREY),
            scale: SCALE,
        })
    }
    fn register_hotkeys(&self, bindings: &[(HotkeyAction, Hotkey)]) -> Vec<HotkeyAction> {
        self.calls().hotkeys.push(bindings.to_vec());
        let fail = self.fail_hotkeys.lock().unwrap().clone();
        bindings.iter().map(|(a, _)| *a).filter(|a| fail.contains(a)).collect()
    }
    fn set_menu(&self, entries: &[MenuEntry]) {
        self.calls().menus.push(entries.to_vec());
    }
    fn decorate(&self, _window: &mut gpui::Window, glass: Glass) {
        self.calls().decorated.push(glass);
    }
    fn set_appearance(&self, choice: ThemeChoice) {
        self.calls().appearance.push(choice);
    }
    fn set_launch_at_login(&self, on: bool) -> Result<(), String> {
        self.calls().login.push(on);
        Ok(())
    }
    fn copy_files(&self, paths: &[PathBuf]) {
        self.calls().copied_files.extend(paths.iter().cloned());
    }
    fn system_screenshot_folder(&self) -> PathBuf {
        self.screenshot_folder.clone()
    }
    fn watch_folder(&self, path: Option<&Path>) {
        self.calls().watched.push(path.map(|p| p.to_path_buf()));
    }
}

pub struct Fixture {
    pub platform: Arc<FakePlatform>,
    pub paths: Paths,
    pub root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn temp_root() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("oshot-test-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// Starts the app like `main` does, with `settings` already on disk.
pub fn start_with(cx: &mut TestAppContext, settings: Settings) -> Fixture {
    start_configured(cx, settings, |_| {})
}

/// Like `start_with`, with the fake macOS set up by `setup` before launch
/// (for example, permissions not granted yet).
pub fn start_configured(cx: &mut TestAppContext, settings: Settings, setup: impl FnOnce(&FakePlatform)) -> Fixture {
    let root = temp_root();
    let paths = Paths::under(root.join("support"), root.join("Pictures"));
    settings.save(&paths.settings).unwrap();
    let shots = root.join("Desktop");
    std::fs::create_dir_all(&shots).unwrap();
    let platform = Arc::new(FakePlatform::new(shots));
    setup(&platform);
    let p: Arc<dyn Platform> = platform.clone();
    let paths2 = paths.clone();
    cx.update(|cx| oshot_app::init(paths2, p, cx));
    cx.run_until_parked();
    Fixture { platform, paths, root }
}

/// A returning user: onboarding already done.
pub fn start(cx: &mut TestAppContext) -> Fixture {
    start_with(
        cx,
        Settings {
            onboarding_done: true,
            ..Settings::default()
        },
    )
}

pub fn settings(cx: &TestAppContext) -> Settings {
    cx.read(|cx| cx.global::<AppState>().settings.clone())
}

pub fn notices(cx: &TestAppContext) -> Vec<Notice> {
    cx.read(|cx| cx.global::<AppState>().notices.clone())
}

pub fn last_notice(cx: &TestAppContext) -> Option<Notice> {
    notices(cx).last().cloned()
}

pub fn find<V: 'static>(cx: &TestAppContext) -> Option<WindowHandle<V>> {
    cx.read(|cx| oshot_app::windows::find::<V>(cx))
}

pub fn window_cx<V: 'static>(cx: &mut TestAppContext) -> (WindowHandle<V>, VisualTestContext) {
    let handle = find::<V>(cx).unwrap_or_else(|| panic!("no {} window open", std::any::type_name::<V>()));
    let any: AnyWindowHandle = handle.into();
    (handle, VisualTestContext::from_window(any, cx))
}

pub fn center(b: Bounds<Pixels>) -> Point<Pixels> {
    b.center()
}

/// Clicks the element tagged with `selector`.
pub fn click(vcx: &mut VisualTestContext, selector: &'static str) {
    let b = vcx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("no element `{selector}` on screen"));
    vcx.simulate_click(b.center(), Modifiers::none());
    vcx.run_until_parked();
}

/// The traffic lights' corner of a titled window: nothing may be drawn here.
pub const TRAFFIC_LIGHTS: Rect = rect(0.0, 0.0, 76.0, oshot_app::ui::TITLEBAR_H);

/// Asserts `selector` is on screen and clear of the traffic lights.
pub fn assert_clear_of_traffic_lights(vcx: &mut VisualTestContext, selector: &'static str, context: &str) {
    let b = vcx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{context}: no element `{selector}` on screen"));
    let r = rect(
        f32::from(b.origin.x),
        f32::from(b.origin.y),
        f32::from(b.size.width),
        f32::from(b.size.height),
    );
    assert!(
        r.intersect(&TRAFFIC_LIGHTS).is_empty(),
        "{context}: `{selector}` at {r:?} runs under the traffic lights"
    );
}

pub fn has(vcx: &mut VisualTestContext, selector: &'static str) -> bool {
    vcx.debug_bounds(selector).is_some()
}

pub fn drag(vcx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>, modifiers: Modifiers) {
    vcx.simulate_mouse_move(from, None, modifiers);
    vcx.simulate_mouse_down(from, MouseButton::Left, modifiers);
    for i in 1..=6 {
        let t = i as f32 / 6.0;
        let p = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        vcx.simulate_mouse_move(p, MouseButton::Left, modifiers);
    }
    vcx.simulate_mouse_up(to, MouseButton::Left, modifiers);
    vcx.run_until_parked();
}

pub fn p(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

/// The clipboard's PNG, decoded.
pub fn clipboard_image(cx: &TestAppContext) -> Option<(Bitmap, u32)> {
    let item = cx.read_from_clipboard()?;
    let bytes = item.entries().iter().find_map(|e| match e {
        ClipboardEntry::Image(img) => Some(img.bytes.clone()),
        _ => None,
    })?;
    oshot_native::imageio::decode(&bytes).ok()
}

pub fn clipboard_text(cx: &TestAppContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|c| c.text())
}

pub fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

pub fn png_bytes(bmp: &Bitmap, scale: u32) -> Vec<u8> {
    oshot_native::imageio::encode(bmp.view(), oshot_core::format::ImageFormat::Png, 1.0, scale).unwrap()
}

pub fn put_image_on_clipboard(cx: &TestAppContext, bmp: &Bitmap) {
    cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
        gpui::ImageFormat::Png,
        png_bytes(bmp, 1),
    )));
}

/// A test app that measures text with macOS's real CoreText system, so
/// layouts (and anything that overflows) match the shipped app.
pub fn real_cx() -> TestAppContext {
    let text = Arc::new(gpui_apple::AppleTextSystem::new());
    TestAppContext::build_with_text_system(gpui::TestDispatcher::new(0), None, text)
}
