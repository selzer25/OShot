use crate::color::{Color, rgba};
use crate::geometry::IRect;

/// Owned image in BGRA8 with straight alpha. BGRA is what ScreenCaptureKit
/// hands back and what GPUI uploads, so captures never need a channel swap.
#[derive(Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl std::fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bitmap({}x{})", self.width, self.height)
    }
}

/// Borrowed BGRA8 pixels, so callers can pass a buffer GPUI already owns.
#[derive(Clone, Copy)]
pub struct BitmapRef<'a> {
    pub width: u32,
    pub height: u32,
    pub data: &'a [u8],
}

impl Bitmap {
    pub fn new(width: u32, height: u32) -> Bitmap {
        Bitmap {
            width,
            height,
            data: vec![0; width as usize * height as usize * 4],
        }
    }

    pub fn filled(width: u32, height: u32, c: Color) -> Bitmap {
        let mut data = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..width as usize * height as usize {
            data.extend_from_slice(&[c.b, c.g, c.r, c.a]);
        }
        Bitmap { width, height, data }
    }

    pub fn view(&self) -> BitmapRef<'_> {
        BitmapRef {
            width: self.width,
            height: self.height,
            data: &self.data,
        }
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, c: Color) {
        if x < self.width && y < self.height {
            let i = (y as usize * self.width as usize + x as usize) * 4;
            self.data[i..i + 4].copy_from_slice(&[c.b, c.g, c.r, c.a]);
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        self.view().pixel(x, y)
    }

    pub fn fill_rect(&mut self, area: IRect, c: Color) {
        for y in area.y..(area.y + area.h).min(self.height) {
            for x in area.x..(area.x + area.w).min(self.width) {
                self.set_pixel(x, y, c);
            }
        }
    }
}

impl<'a> BitmapRef<'a> {
    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = self.data.get(i..i + 4)?;
        Some(rgba(p[2], p[1], p[0], p[3]))
    }

    pub fn crop(&self, area: IRect) -> Bitmap {
        let x1 = (area.x + area.w).min(self.width);
        let y1 = (area.y + area.h).min(self.height);
        let w = x1.saturating_sub(area.x);
        let h = y1.saturating_sub(area.y);
        let mut data = Vec::with_capacity(w as usize * h as usize * 4);
        for y in area.y..y1 {
            let start = (y as usize * self.width as usize + area.x as usize) * 4;
            data.extend_from_slice(&self.data[start..start + w as usize * 4]);
        }
        Bitmap {
            width: w,
            height: h,
            data,
        }
    }

    pub fn to_owned(&self) -> Bitmap {
        Bitmap {
            width: self.width,
            height: self.height,
            data: self.data.to_vec(),
        }
    }

    /// Box-filter downscale by an integer factor, e.g. 2 to turn a Retina
    /// capture into 1x pixels.
    pub fn downscale(&self, factor: u32) -> Bitmap {
        if factor <= 1 {
            return self.to_owned();
        }
        let w = (self.width / factor).max(1);
        let h = (self.height / factor).max(1);
        let mut out = Bitmap::new(w, h);
        let n = factor * factor;
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 4];
                for dy in 0..factor {
                    for dx in 0..factor {
                        let sx = (x * factor + dx).min(self.width - 1);
                        let sy = (y * factor + dy).min(self.height - 1);
                        let i = (sy as usize * self.width as usize + sx as usize) * 4;
                        for (s, v) in sum.iter_mut().zip(&self.data[i..i + 4]) {
                            *s += *v as u32;
                        }
                    }
                }
                let o = (y as usize * w as usize + x as usize) * 4;
                for (d, s) in out.data[o..o + 4].iter_mut().zip(sum) {
                    *d = (s / n) as u8;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::rgb;

    #[test]
    fn crop_takes_the_right_pixels() {
        let mut bmp = Bitmap::filled(10, 10, rgb(0, 0, 0));
        bmp.set_pixel(3, 4, rgb(255, 0, 0));
        let c = bmp.view().crop(IRect { x: 3, y: 4, w: 2, h: 2 });
        assert_eq!((c.width, c.height), (2, 2));
        assert_eq!(c.pixel(0, 0), Some(rgb(255, 0, 0)));
        assert_eq!(c.pixel(1, 1), Some(rgb(0, 0, 0)));
    }

    #[test]
    fn crop_clamps_to_bounds() {
        let bmp = Bitmap::filled(10, 10, rgb(1, 2, 3));
        let c = bmp.view().crop(IRect { x: 8, y: 8, w: 5, h: 5 });
        assert_eq!((c.width, c.height), (2, 2));
    }

    #[test]
    fn downscale_averages() {
        let mut bmp = Bitmap::filled(2, 2, rgb(0, 0, 0));
        bmp.set_pixel(0, 0, rgb(200, 200, 200));
        let d = bmp.view().downscale(2);
        assert_eq!(d.pixel(0, 0), Some(rgb(50, 50, 50)));
    }
}
