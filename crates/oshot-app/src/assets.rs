//! Icons compiled into the binary, so the app is a single file.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $((concat!("icons/", $name, ".svg"), include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*
        ];
    };
}

icons!(
    "app-window",
    "arrow-right",
    "arrow-right-left",
    "bring-to-front",
    "camera",
    "check",
    "chevron-down",
    "circle",
    "circle-dot",
    "clipboard-paste",
    "clock",
    "copy",
    "crop",
    "download",
    "droplet",
    "eraser",
    "external-link",
    "eye-off",
    "file-image",
    "film",
    "folder-open",
    "grid-3x3",
    "hand",
    "highlighter",
    "image",
    "info",
    "keyboard",
    "layers",
    "list-ordered",
    "maximize-2",
    "minus",
    "monitor",
    "mouse-pointer-2",
    "move",
    "move-up-right",
    "palette",
    "pen-tool",
    "pencil",
    "pin",
    "pipette",
    "plus",
    "redo-2",
    "ruler",
    "scan",
    "scan-eye",
    "scan-text",
    "settings",
    "shield",
    "sliders-horizontal",
    "sparkles",
    "square",
    "square-dashed",
    "sun",
    "timer",
    "trash",
    "type",
    "undo-2",
    "upload",
    "wand-sparkles",
    "x",
    "zoom-in",
    "zoom-out",
);

/// Full-colour artwork, drawn with `img` rather than tinted like icons.
const BRAND: &[(&str, &[u8])] = &[("brand/mark.svg", include_bytes!("../assets/brand/mark.svg"))];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .chain(BRAND)
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .chain(BRAND)
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_loads_and_is_svg() {
        for (path, _) in ICONS.iter().chain(BRAND) {
            let bytes = Assets.load(path).unwrap().unwrap();
            assert!(bytes.starts_with(b"<svg"), "{path}");
        }
        assert!(Assets.load("icons/missing.svg").unwrap().is_none());
    }
}
