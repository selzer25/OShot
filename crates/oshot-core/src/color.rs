use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::bitmap::BitmapRef;
use crate::geometry::IRect;

/// Straight (non-premultiplied) sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color { r, g, b, a }
}

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

/// Annotation palette, in toolbar order.
pub const PALETTE: [Color; 9] = [
    rgb(0xFF, 0x3B, 0x30),
    rgb(0xFF, 0x95, 0x00),
    rgb(0xFF, 0xCC, 0x00),
    rgb(0x34, 0xC7, 0x59),
    rgb(0x0A, 0x84, 0xFF),
    rgb(0xAF, 0x52, 0xDE),
    rgb(0xFF, 0x2D, 0x55),
    rgb(0x1C, 0x1C, 0x1E),
    rgb(0xFF, 0xFF, 0xFF),
];

impl Color {
    pub fn with_alpha(self, a: u8) -> Color {
        Color { a, ..self }
    }

    pub fn alpha_f(self) -> f32 {
        self.a as f32 / 255.0
    }

    /// `#RRGGBB`, or `#RRGGBBAA` when not opaque.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }

    /// Accepts `RGB`, `RRGGBB` and `RRGGBBAA`, with or without `#`.
    pub fn from_hex(s: &str) -> Option<Color> {
        let s = s.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            3 => {
                let nib = |i: usize| u8::from_str_radix(s.get(i..i + 1)?, 16).ok().map(|v| v * 17);
                Some(rgb(nib(0)?, nib(1)?, nib(2)?))
            }
            6 => Some(rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// Relative luminance, 0 (black) to 1 (white).
    pub fn luminance(self) -> f32 {
        fn lin(c: u8) -> f32 {
            let c = c as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    /// Black or white, whichever reads better on top of `self`.
    pub fn contrasting(self) -> Color {
        if self.luminance() > 0.45 {
            rgb(0, 0, 0)
        } else {
            rgb(255, 255, 255)
        }
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::from_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("bad colour {s}")))
    }
}

/// Mean colour of the pixels inside `area`.
pub fn average_color(image: BitmapRef, area: IRect) -> Option<Color> {
    if area.is_empty() {
        return None;
    }
    let mut sum = [0u64; 4];
    let mut n = 0u64;
    for y in area.y..area.y + area.h {
        for x in area.x..area.x + area.w {
            let p = image.pixel(x, y)?;
            for (s, v) in sum.iter_mut().zip([p.r, p.g, p.b, p.a]) {
                *s += v as u64;
            }
            n += 1;
        }
    }
    Some(rgba(
        (sum[0] / n) as u8,
        (sum[1] / n) as u8,
        (sum[2] / n) as u8,
        (sum[3] / n) as u8,
    ))
}

/// Darkest pixel in the `size` x `size` square centred on (`x`, `y`). Used to
/// copy a text colour without pointing exactly at a glyph stroke.
pub fn darkest_near(image: BitmapRef, x: u32, y: u32, size: u32) -> Option<Color> {
    let half = size / 2;
    let x0 = x.saturating_sub(half);
    let y0 = y.saturating_sub(half);
    let x1 = (x + half).min(image.width.saturating_sub(1));
    let y1 = (y + half).min(image.height.saturating_sub(1));
    let mut best: Option<Color> = None;
    for yy in y0..=y1 {
        for xx in x0..=x1 {
            let c = image.pixel(xx, yy)?;
            if best.is_none_or(|b| c.luminance() < b.luminance()) {
                best = Some(c);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::Bitmap;

    #[test]
    fn hex_round_trip() {
        assert_eq!(Color::from_hex("#0A84FF"), Some(rgb(10, 132, 255)));
        assert_eq!(Color::from_hex("fff"), Some(rgb(255, 255, 255)));
        assert_eq!(rgba(1, 2, 3, 4).to_hex(), "#01020304");
        assert_eq!(Color::from_hex("#12345"), None);
    }

    #[test]
    fn average_and_darkest() {
        let mut bmp = Bitmap::filled(4, 4, rgb(200, 200, 200));
        bmp.set_pixel(1, 1, rgb(10, 20, 30));
        assert_eq!(darkest_near(bmp.view(), 2, 2, 4), Some(rgb(10, 20, 30)));
        let avg = average_color(bmp.view(), IRect { x: 0, y: 0, w: 2, h: 1 }).unwrap();
        assert_eq!(avg, rgb(200, 200, 200));
    }
}
