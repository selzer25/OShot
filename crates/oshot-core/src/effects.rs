//! Pixel effects for pixelate, blur, erase and the magnifier, with a cache so
//! each patch is computed once per change instead of once per frame.

use std::collections::HashMap;
use std::sync::Arc;

use crate::bitmap::{Bitmap, BitmapRef};
use crate::geometry::{IRect, Rect};
use crate::model::{Object, Shape};

/// Mosaic of `block` x `block` squares, each the average of its pixels.
pub fn pixelate(image: BitmapRef, area: IRect, block: u32) -> Bitmap {
    let block = block.max(2);
    let mut out = image.crop(area);
    let (w, h) = (out.width, out.height);
    let mut by = 0;
    while by < h {
        let mut bx = 0;
        while bx < w {
            let bw = block.min(w - bx);
            let bh = block.min(h - by);
            let mut sum = [0u32; 4];
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let i = (y * w + x) as usize * 4;
                    for (s, v) in sum.iter_mut().zip(&out.data[i..i + 4]) {
                        *s += *v as u32;
                    }
                }
            }
            let n = bw * bh;
            let avg = [sum[0] / n, sum[1] / n, sum[2] / n, sum[3] / n];
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let i = (y * w + x) as usize * 4;
                    for (d, a) in out.data[i..i + 4].iter_mut().zip(avg) {
                        *d = a as u8;
                    }
                }
            }
            bx += block;
        }
        by += block;
    }
    out
}

/// Approximate Gaussian blur: three box-blur passes each way. Samples a margin
/// outside `area` so the patch edge blends into the surroundings.
pub fn blur(image: BitmapRef, area: IRect, radius: u32) -> Bitmap {
    let r = radius.max(1);
    let margin = r * 3;
    let x0 = area.x.saturating_sub(margin);
    let y0 = area.y.saturating_sub(margin);
    let x1 = (area.x + area.w + margin).min(image.width);
    let y1 = (area.y + area.h + margin).min(image.height);
    let padded = IRect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    };
    let mut buf = image.crop(padded);
    let mut tmp = buf.data.clone();
    for _ in 0..3 {
        box_pass(&buf.data, &mut tmp, buf.width, buf.height, r, true);
        box_pass(&tmp, &mut buf.data, buf.width, buf.height, r, false);
    }
    buf.view().crop(IRect {
        x: area.x - x0,
        y: area.y - y0,
        w: area.w.min(buf.width),
        h: area.h.min(buf.height),
    })
}

fn box_pass(src: &[u8], dst: &mut [u8], w: u32, h: u32, r: u32, horizontal: bool) {
    let (w, h, r) = (w as i64, h as i64, r as i64);
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let idx = |line: i64, i: i64| -> usize {
        let i = i.clamp(0, len - 1);
        (if horizontal { line * w + i } else { i * w + line }) as usize * 4
    };
    let n = (2 * r + 1) as u32;
    for line in 0..lines {
        let mut sum = [0u32; 4];
        for i in -r..=r {
            let p = idx(line, i);
            for c in 0..4 {
                sum[c] += src[p + c] as u32;
            }
        }
        for i in 0..len {
            let o = idx(line, i);
            for c in 0..4 {
                dst[o + c] = (sum[c] / n) as u8;
            }
            let add = idx(line, i + r + 1);
            let sub = idx(line, i - r);
            for c in 0..4 {
                sum[c] = sum[c] + src[add + c] as u32 - src[sub + c] as u32;
            }
        }
    }
}

/// Removes what is inside `area` by blending the pixels around its border
/// across it, so flat backgrounds look untouched.
pub fn erase(image: BitmapRef, area: IRect) -> Bitmap {
    let mut out = Bitmap::new(area.w, area.h);
    if area.is_empty() || image.width == 0 || image.height == 0 {
        return out;
    }
    let clamp_x = |x: i64| x.clamp(0, image.width as i64 - 1) as u32;
    let clamp_y = |y: i64| y.clamp(0, image.height as i64 - 1) as u32;
    let left = clamp_x(area.x as i64 - 1);
    let right = clamp_x((area.x + area.w) as i64);
    let top = clamp_y(area.y as i64 - 1);
    let bottom = clamp_y((area.y + area.h) as i64);
    for y in 0..area.h {
        let iy = clamp_y((area.y + y) as i64);
        let l = image.pixel(left, iy).unwrap_or_default_color();
        let r = image.pixel(right, iy).unwrap_or_default_color();
        let ty = (y as f32 + 1.0) / (area.h as f32 + 1.0);
        for x in 0..area.w {
            let ix = clamp_x((area.x + x) as i64);
            let t = image.pixel(ix, top).unwrap_or_default_color();
            let b = image.pixel(ix, bottom).unwrap_or_default_color();
            let tx = (x as f32 + 1.0) / (area.w as f32 + 1.0);
            let mix = |a: u8, b: u8, t: f32| a as f32 + (b as f32 - a as f32) * t;
            let ch = |hl: u8, hr: u8, vt: u8, vb: u8| ((mix(hl, hr, tx) + mix(vt, vb, ty)) / 2.0).round() as u8;
            out.set_pixel(
                x,
                y,
                crate::color::rgba(
                    ch(l.r, r.r, t.r, b.r),
                    ch(l.g, r.g, t.g, b.g),
                    ch(l.b, r.b, t.b, b.b),
                    255,
                ),
            );
        }
    }
    out
}

trait OrDefaultColor {
    fn unwrap_or_default_color(self) -> crate::color::Color;
}

impl OrDefaultColor for Option<crate::color::Color> {
    fn unwrap_or_default_color(self) -> crate::color::Color {
        self.unwrap_or(crate::color::rgb(255, 255, 255))
    }
}

/// Nearest-neighbour enlargement of `source` to `out_w` x `out_h`, keeping
/// pixels crisp the way a loupe should.
pub fn magnify(image: BitmapRef, source: IRect, out_w: u32, out_h: u32) -> Bitmap {
    let mut out = Bitmap::new(out_w.max(1), out_h.max(1));
    if source.is_empty() {
        return out;
    }
    for y in 0..out.height {
        let sy = source.y + (y as u64 * source.h as u64 / out.height as u64) as u32;
        for x in 0..out.width {
            let sx = source.x + (x as u64 * source.w as u64 / out.width as u64) as u32;
            if let Some(c) = image.pixel(sx, sy) {
                out.set_pixel(x, y, c);
            }
        }
    }
    out
}

#[derive(Clone, PartialEq)]
enum Key {
    Pixelate(IRect, u32),
    Blur(IRect, u32),
    Erase(IRect),
    Magnify(IRect, u32),
}

/// Computed effect pixels, keyed by object id. An entry is recomputed only
/// when the object's area or strength changes.
#[derive(Default)]
pub struct EffectCache {
    entries: HashMap<u64, (Key, Arc<Bitmap>)>,
}

impl EffectCache {
    /// Pixels for an effect object, plus the image-space rect they cover
    /// (clamped to the image).
    pub fn get(&mut self, obj: &Object, image: BitmapRef) -> Option<(Rect, Arc<Bitmap>)> {
        let clamp = |r: &Rect| r.to_pixels(image.width, image.height);
        let (key, dest) = match &obj.shape {
            Shape::Pixelate { rect } => (
                Key::Pixelate(clamp(rect), obj.style.strength.round() as u32),
                clamp(rect).to_rect(),
            ),
            Shape::Blur { rect } => (
                Key::Blur(clamp(rect), obj.style.strength.round() as u32),
                clamp(rect).to_rect(),
            ),
            Shape::Erase { rect } => (Key::Erase(clamp(rect)), clamp(rect).to_rect()),
            Shape::Magnifier { source, center, radius } => {
                let side = (radius * 2.0).round().max(1.0) as u32;
                let dest = Rect {
                    x: center.x - radius,
                    y: center.y - radius,
                    w: radius * 2.0,
                    h: radius * 2.0,
                };
                (Key::Magnify(clamp(source), side), dest)
            }
            _ => return None,
        };
        let area = match &key {
            Key::Pixelate(a, _) | Key::Blur(a, _) | Key::Erase(a) | Key::Magnify(a, _) => *a,
        };
        if area.is_empty() {
            return None;
        }
        if let Some((k, bmp)) = self.entries.get(&obj.id)
            && *k == key
        {
            return Some((dest, bmp.clone()));
        }
        let bmp = Arc::new(match &key {
            Key::Pixelate(a, block) => pixelate(image, *a, *block),
            Key::Blur(a, r) => blur(image, *a, *r),
            Key::Erase(a) => erase(image, *a),
            Key::Magnify(a, side) => magnify(image, *a, *side, *side),
        });
        self.entries.insert(obj.id, (key, bmp.clone()));
        Some((dest, bmp))
    }

    /// Drops entries for objects that no longer exist.
    pub fn retain(&mut self, alive: impl Fn(u64) -> bool) {
        self.entries.retain(|id, _| alive(*id));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::rgb;
    use crate::geometry::{pt, rect};
    use crate::model::{Style, Tool};

    fn stripes() -> Bitmap {
        let mut b = Bitmap::new(40, 40);
        for y in 0..40 {
            for x in 0..40 {
                b.set_pixel(x, y, if x % 2 == 0 { rgb(0, 0, 0) } else { rgb(255, 255, 255) });
            }
        }
        b
    }

    #[test]
    fn pixelate_flattens_blocks() {
        let p = pixelate(stripes().view(), IRect { x: 0, y: 0, w: 8, h: 8 }, 4);
        assert_eq!(p.pixel(0, 0), p.pixel(3, 3));
        assert_eq!(p.pixel(0, 0), Some(rgb(127, 127, 127)));
    }

    #[test]
    fn blur_smooths_stripes() {
        let b = blur(
            stripes().view(),
            IRect {
                x: 10,
                y: 10,
                w: 10,
                h: 10,
            },
            3,
        );
        assert_eq!((b.width, b.height), (10, 10));
        let c = b.pixel(5, 5).unwrap();
        assert!((110..=145).contains(&c.r), "blurred to grey, got {c:?}");
    }

    #[test]
    fn erase_fills_from_border() {
        let mut img = Bitmap::filled(20, 20, rgb(30, 60, 90));
        img.fill_rect(IRect { x: 5, y: 5, w: 6, h: 6 }, rgb(255, 0, 0));
        let e = erase(img.view(), IRect { x: 5, y: 5, w: 6, h: 6 });
        assert_eq!(e.pixel(3, 3), Some(rgb(30, 60, 90)), "red is gone");
    }

    #[test]
    fn magnify_is_nearest_neighbour() {
        let m = magnify(stripes().view(), IRect { x: 0, y: 0, w: 2, h: 2 }, 8, 8);
        assert_eq!(m.pixel(0, 0), Some(rgb(0, 0, 0)));
        assert_eq!(m.pixel(3, 0), Some(rgb(0, 0, 0)));
        assert_eq!(m.pixel(4, 0), Some(rgb(255, 255, 255)));
    }

    #[test]
    fn cache_recomputes_only_on_change() {
        let img = stripes();
        let mut cache = EffectCache::default();
        let mut obj = Object {
            id: 1,
            shape: Shape::Pixelate {
                rect: rect(0.0, 0.0, 8.0, 8.0),
            },
            style: Style::for_tool(Tool::Pixelate, 1.0),
        };
        let (_, a) = cache.get(&obj, img.view()).unwrap();
        let (_, b) = cache.get(&obj, img.view()).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        obj.translate(4.0, 0.0);
        let (dest, c) = cache.get(&obj, img.view()).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(dest, rect(4.0, 0.0, 8.0, 8.0));
        cache.retain(|_| false);
        assert!(cache.is_empty());
        let _ = pt(0.0, 0.0);
    }
}
