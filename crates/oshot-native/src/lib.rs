//! Thin wrappers over the macOS frameworks OShot draws and encodes with:
//! CoreGraphics and CoreText for export, ImageIO for formats, Vision for
//! text recognition. None of this needs a window, so it runs in tests.

pub mod cf;
pub mod cg;
pub mod imageio;
pub mod ocr;
pub mod render;
pub mod shrink;
pub mod text;
pub mod webp;
