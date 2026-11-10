//! User settings, persisted as JSON. Unknown or missing fields fall back to
//! defaults so older settings files keep loading.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::format::ImageFormat;
use crate::geometry::Rect;
use crate::gesture::MenuGesture;
use crate::model::Style;
use crate::naming::DEFAULT_TEMPLATE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

/// What happens after an area, window or full-screen capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AfterCapture {
    Editor,
    Thumbnail,
    ClipboardOnly,
}

impl AfterCapture {
    pub const ALL: [AfterCapture; 3] = [
        AfterCapture::Editor,
        AfterCapture::Thumbnail,
        AfterCapture::ClipboardOnly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AfterCapture::Editor => "Open editor",
            AfterCapture::Thumbnail => "Show thumbnail",
            AfterCapture::ClipboardOnly => "Copy only",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HotkeyAction {
    CaptureFullScreen,
    CaptureArea,
    CaptureMenu,
    CaptureWindow,
    CaptureText,
    RepeatArea,
    ColorPicker,
    Ruler,
    Converter,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 9] = [
        HotkeyAction::CaptureFullScreen,
        HotkeyAction::CaptureArea,
        HotkeyAction::CaptureMenu,
        HotkeyAction::CaptureWindow,
        HotkeyAction::CaptureText,
        HotkeyAction::RepeatArea,
        HotkeyAction::ColorPicker,
        HotkeyAction::Ruler,
        HotkeyAction::Converter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HotkeyAction::CaptureFullScreen => "Capture full screen",
            HotkeyAction::CaptureArea => "Capture area",
            HotkeyAction::CaptureMenu => "Capture menu",
            HotkeyAction::CaptureWindow => "Capture window",
            HotkeyAction::CaptureText => "Capture text (OCR)",
            HotkeyAction::RepeatArea => "Repeat last area",
            HotkeyAction::ColorPicker => "Color picker",
            HotkeyAction::Ruler => "Screen ruler",
            HotkeyAction::Converter => "Convert image",
        }
    }
}

/// A key chord in GPUI keystroke syntax, e.g. `alt-shift-4`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Lower-case key name: `a`..`z`, `0`..`9`, `f1`.., `space`, …
    pub key: String,
}

impl Hotkey {
    pub fn parse(s: &str) -> Option<Hotkey> {
        let mut hk = Hotkey {
            cmd: false,
            ctrl: false,
            alt: false,
            shift: false,
            key: String::new(),
        };
        let parts: Vec<&str> = s.split('-').collect();
        let (key, mods) = parts.split_last()?;
        for m in mods {
            match *m {
                "cmd" => hk.cmd = true,
                "ctrl" => hk.ctrl = true,
                "alt" => hk.alt = true,
                "shift" => hk.shift = true,
                _ => return None,
            }
        }
        if key.is_empty() {
            return None;
        }
        hk.key = key.to_ascii_lowercase();
        // A global shortcut without a modifier would swallow normal typing.
        if !(hk.cmd || hk.ctrl || hk.alt) {
            return None;
        }
        Some(hk)
    }

    pub fn to_keystroke(&self) -> String {
        let mut s = String::new();
        for (on, name) in [
            (self.ctrl, "ctrl-"),
            (self.alt, "alt-"),
            (self.shift, "shift-"),
            (self.cmd, "cmd-"),
        ] {
            if on {
                s.push_str(name);
            }
        }
        s.push_str(&self.key);
        s
    }

    /// macOS-style symbols, e.g. `⌥⇧4`.
    pub fn symbols(&self) -> String {
        let mut s = String::new();
        for (on, sym) in [(self.ctrl, "⌃"), (self.alt, "⌥"), (self.shift, "⇧"), (self.cmd, "⌘")] {
            if on {
                s.push_str(sym);
            }
        }
        let key = match self.key.as_str() {
            "space" => "Space".to_string(),
            "enter" => "↩".to_string(),
            "escape" => "⎋".to_string(),
            "tab" => "⇥".to_string(),
            k => k.to_uppercase(),
        };
        s.push_str(&key);
        s
    }

    /// The shortcuts macOS itself owns for screenshots.
    pub fn is_system_reserved(&self) -> bool {
        self.cmd && self.shift && !self.alt && !self.ctrl && matches!(self.key.as_str(), "3" | "4" | "5" | "6")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub full_screen: Option<String>,
    pub area: Option<String>,
    pub menu: Option<String>,
    pub window: Option<String>,
    pub text: Option<String>,
    pub repeat_area: Option<String>,
    pub color_picker: Option<String>,
    pub ruler: Option<String>,
    pub converter: Option<String>,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Hotkeys {
            full_screen: Some("alt-shift-3".into()),
            area: Some("alt-shift-4".into()),
            menu: Some("alt-shift-5".into()),
            window: Some("alt-shift-6".into()),
            text: Some("alt-shift-7".into()),
            repeat_area: None,
            color_picker: None,
            ruler: None,
            converter: None,
        }
    }
}

impl Hotkeys {
    pub fn get(&self, action: HotkeyAction) -> Option<&String> {
        match action {
            HotkeyAction::CaptureFullScreen => self.full_screen.as_ref(),
            HotkeyAction::CaptureArea => self.area.as_ref(),
            HotkeyAction::CaptureMenu => self.menu.as_ref(),
            HotkeyAction::CaptureWindow => self.window.as_ref(),
            HotkeyAction::CaptureText => self.text.as_ref(),
            HotkeyAction::RepeatArea => self.repeat_area.as_ref(),
            HotkeyAction::ColorPicker => self.color_picker.as_ref(),
            HotkeyAction::Ruler => self.ruler.as_ref(),
            HotkeyAction::Converter => self.converter.as_ref(),
        }
    }

    pub fn set(&mut self, action: HotkeyAction, value: Option<String>) {
        let slot = match action {
            HotkeyAction::CaptureFullScreen => &mut self.full_screen,
            HotkeyAction::CaptureArea => &mut self.area,
            HotkeyAction::CaptureMenu => &mut self.menu,
            HotkeyAction::CaptureWindow => &mut self.window,
            HotkeyAction::CaptureText => &mut self.text,
            HotkeyAction::RepeatArea => &mut self.repeat_area,
            HotkeyAction::ColorPicker => &mut self.color_picker,
            HotkeyAction::Ruler => &mut self.ruler,
            HotkeyAction::Converter => &mut self.converter,
        };
        *slot = value;
    }

    /// Valid bindings. When two actions share a chord, the first keeps it.
    pub fn bindings(&self) -> Vec<(HotkeyAction, Hotkey)> {
        let mut out: Vec<(HotkeyAction, Hotkey)> = Vec::new();
        for action in HotkeyAction::ALL {
            if let Some(hk) = self.get(action).and_then(|s| Hotkey::parse(s))
                && !out.iter().any(|(_, h)| *h == hk)
            {
                out.push((action, hk));
            }
        }
        out
    }

    /// The action already using `hotkey`, other than `except`.
    pub fn conflict(&self, hotkey: &Hotkey, except: HotkeyAction) -> Option<HotkeyAction> {
        HotkeyAction::ALL
            .into_iter()
            .filter(|a| *a != except)
            .find(|a| self.get(*a).and_then(|s| Hotkey::parse(s)).as_ref() == Some(hotkey))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputLocation {
    NextToOriginal,
    SaveFolder,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConverterDefaults {
    pub format: ImageFormat,
    /// 0..1
    pub quality: f32,
    pub lossless: bool,
    pub max_dimension: Option<u32>,
    pub downscale_retina: bool,
    pub strip_metadata: bool,
    pub output: OutputLocation,
}

impl Default for ConverterDefaults {
    fn default() -> Self {
        ConverterDefaults {
            format: ImageFormat::WebP,
            quality: 0.8,
            lossless: false,
            max_dimension: None,
            downscale_retina: false,
            strip_metadata: true,
            output: OutputLocation::NextToOriginal,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedArea {
    pub display: u32,
    /// In points, relative to the display's top-left corner.
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub onboarding_done: bool,
    pub theme: ThemeChoice,
    pub after_capture: AfterCapture,
    pub copy_on_capture: bool,
    pub save_on_capture: bool,
    /// Finish an area capture as soon as the mouse is released instead of
    /// waiting for ⌘C, ⌘S or Return.
    pub finish_on_release: bool,
    pub thumbnail_corner: Corner,
    pub thumbnail_seconds: u32,
    pub save_folder: Option<PathBuf>,
    pub file_format: ImageFormat,
    pub save_quality: f32,
    pub filename_template: String,
    pub downscale_retina: bool,
    pub shrink_png: bool,
    pub window_shadow: bool,
    pub escape_copies: bool,
    /// Close the editor after its image has been copied with Command+C.
    pub close_on_copy_shortcut: bool,
    /// Text defaults in points, independent of capture pixel density.
    pub text_style: Option<Style>,
    pub hotkeys: Hotkeys,
    /// Opens the capture menu; the recommended way, since other apps can
    /// take any key chord but not a pair of modifiers.
    pub menu_gesture: MenuGesture,
    pub converter: ConverterDefaults,
    pub tidy_desktop: bool,
    pub tidy_convert: bool,
    pub auto_clean_days: Option<u32>,
    pub recent_limit: u32,
    pub launch_at_login: bool,
    pub last_area: Option<SavedArea>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            onboarding_done: false,
            theme: ThemeChoice::System,
            after_capture: AfterCapture::Editor,
            copy_on_capture: true,
            save_on_capture: false,
            finish_on_release: false,
            thumbnail_corner: Corner::BottomRight,
            thumbnail_seconds: 6,
            save_folder: None,
            file_format: ImageFormat::Png,
            save_quality: 0.9,
            filename_template: DEFAULT_TEMPLATE.into(),
            downscale_retina: false,
            shrink_png: false,
            window_shadow: true,
            escape_copies: false,
            close_on_copy_shortcut: true,
            text_style: None,
            hotkeys: Hotkeys::default(),
            menu_gesture: MenuGesture::BothCommand,
            converter: ConverterDefaults::default(),
            tidy_desktop: false,
            tidy_convert: false,
            auto_clean_days: None,
            recent_limit: 10,
            launch_at_login: false,
            last_area: None,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Writes atomically (temp file then rename) so a crash mid-write never
    /// leaves a truncated settings file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }

    pub fn save_folder_or(&self, default: &Path) -> PathBuf {
        self.save_folder.clone().unwrap_or_else(|| default.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_parse_and_display() {
        let hk = Hotkey::parse("alt-shift-4").unwrap();
        assert!(hk.alt && hk.shift && !hk.cmd);
        assert_eq!(hk.symbols(), "⌥⇧4");
        assert_eq!(hk.to_keystroke(), "alt-shift-4");
        assert!(Hotkey::parse("shift-4").is_none(), "needs a real modifier");
        assert!(Hotkey::parse("cmd-shift-4").unwrap().is_system_reserved());
        assert!(Hotkey::parse("bogus-4").is_none());
    }

    #[test]
    fn default_bindings_match_the_spec() {
        let b = Hotkeys::default().bindings();
        let find = |a| b.iter().find(|(x, _)| *x == a).map(|(_, h)| h.symbols());
        assert_eq!(find(HotkeyAction::CaptureFullScreen).as_deref(), Some("⌥⇧3"));
        assert_eq!(find(HotkeyAction::CaptureArea).as_deref(), Some("⌥⇧4"));
        assert_eq!(find(HotkeyAction::CaptureMenu).as_deref(), Some("⌥⇧5"));
        assert_eq!(find(HotkeyAction::CaptureWindow).as_deref(), Some("⌥⇧6"));
        assert_eq!(find(HotkeyAction::CaptureText).as_deref(), Some("⌥⇧7"));
    }

    #[test]
    fn conflicts_are_detected_and_first_wins() {
        let mut h = Hotkeys::default();
        h.set(HotkeyAction::Ruler, Some("alt-shift-4".into()));
        let hk = Hotkey::parse("alt-shift-4").unwrap();
        assert_eq!(h.conflict(&hk, HotkeyAction::Ruler), Some(HotkeyAction::CaptureArea));
        assert!(!h.bindings().iter().any(|(a, _)| *a == HotkeyAction::Ruler));
    }

    #[test]
    fn settings_round_trip_and_tolerate_missing_fields() {
        let dir = std::env::temp_dir().join(format!("oshot-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let s = Settings {
            after_capture: AfterCapture::Thumbnail,
            converter: ConverterDefaults {
                quality: 0.5,
                ..ConverterDefaults::default()
            },
            ..Settings::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        std::fs::write(&path, br#"{"theme":"Dark"}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.theme, ThemeChoice::Dark);
        assert_eq!(loaded.hotkeys, Hotkeys::default());
        assert!(
            loaded.close_on_copy_shortcut,
            "older settings close on Command+C by default"
        );
        assert_eq!(loaded.text_style, None);
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
