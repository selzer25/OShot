//! Colour tokens for the glass look, in light and dark.
//!
//! Windows sit on a native glass view; these tints go on top of it, so they
//! are translucent by design.

use gpui::{App, Hsla, Rgba, Window, WindowAppearance};
use oshot_core::settings::ThemeChoice;

use crate::state::AppState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub mode: Mode,
    /// Tint over the window's glass.
    pub glass: Hsla,
    /// Raised surfaces: cards, toolbars, inputs.
    pub panel: Hsla,
    pub hover: Hsla,
    /// Selected segment or active tool.
    pub selected: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub danger: Hsla,
    pub success: Hsla,
    /// Behind the image in the editor.
    pub canvas: Hsla,
    pub shadow: Hsla,
    /// Darkening outside the selection in the capture overlay.
    pub dim: Hsla,
}

pub fn hex(rgb: u32, alpha: f32) -> Hsla {
    let mut c: Hsla = Rgba {
        r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
        g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
        b: (rgb & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
    .into();
    c.a = alpha;
    c
}

pub fn core_color(c: oshot_core::color::Color) -> Hsla {
    Rgba {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a: c.a as f32 / 255.0,
    }
    .into()
}

pub const UI_FONT: &str = ".SystemUIFont";
pub const ACCENT: u32 = 0x0A84FF;

impl Theme {
    pub fn light() -> Theme {
        Theme {
            mode: Mode::Light,
            glass: hex(0xF6F6F8, 0.62),
            panel: hex(0xFFFFFF, 0.72),
            hover: hex(0x000000, 0.055),
            selected: hex(0xFFFFFF, 0.98),
            border: hex(0x000000, 0.09),
            text: hex(0x1D1D1F, 1.0),
            text_muted: hex(0x1D1D1F, 0.62),
            text_faint: hex(0x1D1D1F, 0.40),
            accent: hex(ACCENT, 1.0),
            on_accent: hex(0xFFFFFF, 1.0),
            danger: hex(0xFF3B30, 1.0),
            success: hex(0x28A745, 1.0),
            canvas: hex(0xE8E8ED, 0.55),
            shadow: hex(0x000000, 0.14),
            dim: hex(0x000000, 0.38),
        }
    }

    pub fn dark() -> Theme {
        Theme {
            mode: Mode::Dark,
            glass: hex(0x1C1C1E, 0.58),
            panel: hex(0xFFFFFF, 0.07),
            hover: hex(0xFFFFFF, 0.08),
            selected: hex(0xFFFFFF, 0.17),
            border: hex(0xFFFFFF, 0.10),
            text: hex(0xF5F5F7, 1.0),
            text_muted: hex(0xF5F5F7, 0.64),
            text_faint: hex(0xF5F5F7, 0.40),
            accent: hex(ACCENT, 1.0),
            on_accent: hex(0xFFFFFF, 1.0),
            danger: hex(0xFF453A, 1.0),
            success: hex(0x32D74B, 1.0),
            canvas: hex(0x000000, 0.35),
            shadow: hex(0x000000, 0.45),
            dim: hex(0x000000, 0.45),
        }
    }

    pub fn for_mode(mode: Mode) -> Theme {
        match mode {
            Mode::Light => Theme::light(),
            Mode::Dark => Theme::dark(),
        }
    }

    pub fn is_dark(&self) -> bool {
        self.mode == Mode::Dark
    }
}

/// The theme for a window: the user's choice, or the system appearance.
pub fn theme(window: &Window, cx: &App) -> Theme {
    let choice = cx
        .try_global::<AppState>()
        .map(|s| s.settings.theme)
        .unwrap_or(ThemeChoice::System);
    Theme::for_mode(mode_for(choice, window.appearance()))
}

/// The warm paper-and-ink palette of the onboarding and its helpers: an
/// opaque page instead of glass, so the hand-drawn ink reads clearly.
#[derive(Clone, Copy, Debug)]
pub struct Paper {
    pub bg: Hsla,
    /// Raised cards on the page.
    pub card: Hsla,
    pub ink: Hsla,
    pub soft: Hsla,
    pub accent: Hsla,
    pub green: Hsla,
    /// Hairlines, empty checkboxes, disabled outlines.
    pub track: Hsla,
}

impl Paper {
    pub fn for_mode(mode: Mode) -> Paper {
        match mode {
            Mode::Light => Paper {
                bg: hex(0xFAF8F3, 1.0),
                card: hex(0xFFFFFF, 0.78),
                ink: hex(0x1F232A, 1.0),
                soft: hex(0x7D828C, 1.0),
                accent: hex(0x3D7BFF, 1.0),
                green: hex(0x2C9A66, 1.0),
                track: hex(0xDDD8CC, 1.0),
            },
            Mode::Dark => Paper {
                bg: hex(0x18181B, 1.0),
                card: hex(0xFFFFFF, 0.05),
                ink: hex(0xF2F2F3, 1.0),
                soft: hex(0x92929B, 1.0),
                accent: hex(0x5B95FF, 1.0),
                green: hex(0x3DD68C, 1.0),
                track: hex(0x3A3A41, 1.0),
            },
        }
    }
}

pub fn paper(window: &Window, cx: &App) -> Paper {
    Paper::for_mode(theme(window, cx).mode)
}
pub fn mode_for(choice: ThemeChoice, appearance: WindowAppearance) -> Mode {
    match choice {
        ThemeChoice::Light => Mode::Light,
        ThemeChoice::Dark => Mode::Dark,
        ThemeChoice::System => match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Mode::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => Mode::Light,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_contrasts_with_glass_in_both_modes() {
        for t in [Theme::light(), Theme::dark()] {
            let diff = (t.text.l - t.glass.l).abs();
            assert!(diff > 0.6, "{:?}: text {} vs glass {}", t.mode, t.text.l, t.glass.l);
        }
    }

    #[test]
    fn paper_ink_contrasts_with_the_page() {
        for mode in [Mode::Light, Mode::Dark] {
            let p = Paper::for_mode(mode);
            assert!((p.ink.l - p.bg.l).abs() > 0.7, "{mode:?}");
            assert!((p.soft.l - p.bg.l).abs() > 0.3, "{mode:?}");
        }
    }

    #[test]
    fn choice_overrides_system() {
        assert_eq!(mode_for(ThemeChoice::Dark, WindowAppearance::Light), Mode::Dark);
        assert_eq!(mode_for(ThemeChoice::System, WindowAppearance::VibrantDark), Mode::Dark);
        assert_eq!(mode_for(ThemeChoice::System, WindowAppearance::Light), Mode::Light);
    }
}
