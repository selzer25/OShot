//! Screen-ruler measurements and smart selection on captured pixels.

use crate::bitmap::BitmapRef;
use crate::color::Color;
use crate::geometry::IRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

fn similar(a: Color, b: Color, tolerance: u8) -> bool {
    let d = |x: u8, y: u8| x.abs_diff(y);
    d(a.r, b.r) <= tolerance && d(a.g, b.g) <= tolerance && d(a.b, b.b) <= tolerance
}

/// Walks out from (`x`, `y`) along `axis` while pixels match the start pixel
/// and returns the half-open span `[start, end)`. Pointing between two UI
/// elements measures the gap between them.
pub fn measure_span(image: BitmapRef, x: u32, y: u32, axis: Axis, tolerance: u8) -> Option<(u32, u32)> {
    let start = image.pixel(x, y)?;
    let (pos, limit) = match axis {
        Axis::Horizontal => (x, image.width),
        Axis::Vertical => (y, image.height),
    };
    let at = |i: u32| match axis {
        Axis::Horizontal => image.pixel(i, y),
        Axis::Vertical => image.pixel(x, i),
    };
    let mut lo = pos;
    while lo > 0 && at(lo - 1).is_some_and(|c| similar(c, start, tolerance)) {
        lo -= 1;
    }
    let mut hi = pos + 1;
    while hi < limit && at(hi).is_some_and(|c| similar(c, start, tolerance)) {
        hi += 1;
    }
    Some((lo, hi))
}

/// Shrinks `area` while its outermost rows and columns are a single colour,
/// so a loose selection snaps to the content inside it.
pub fn trim_to_content(image: BitmapRef, area: IRect, tolerance: u8) -> IRect {
    let area = IRect {
        x: area.x.min(image.width),
        y: area.y.min(image.height),
        w: area.w.min(image.width.saturating_sub(area.x)),
        h: area.h.min(image.height.saturating_sub(area.y)),
    };
    if area.is_empty() {
        return area;
    }
    let Some(bg) = image.pixel(area.x, area.y) else {
        return area;
    };
    let row_is_bg =
        |y: u32, x0: u32, x1: u32| (x0..x1).all(|x| image.pixel(x, y).is_some_and(|c| similar(c, bg, tolerance)));
    let col_is_bg =
        |x: u32, y0: u32, y1: u32| (y0..y1).all(|y| image.pixel(x, y).is_some_and(|c| similar(c, bg, tolerance)));
    let (mut x0, mut y0, mut x1, mut y1) = (area.x, area.y, area.x + area.w, area.y + area.h);
    while y0 < y1 && row_is_bg(y0, x0, x1) {
        y0 += 1;
    }
    while y1 > y0 && row_is_bg(y1 - 1, x0, x1) {
        y1 -= 1;
    }
    while x0 < x1 && col_is_bg(x0, y0, y1) {
        x0 += 1;
    }
    while x1 > x0 && col_is_bg(x1 - 1, y0, y1) {
        x1 -= 1;
    }
    if x0 >= x1 || y0 >= y1 {
        return area;
    }
    IRect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

/// Bounding box of the same-coloured region under (`x`, `y`), for selecting a
/// flat button or panel with one click. Stops after `max_pixels` to stay fast
/// on huge flat areas.
pub fn monotone_region(image: BitmapRef, x: u32, y: u32, tolerance: u8, max_pixels: usize) -> Option<IRect> {
    let start = image.pixel(x, y)?;
    let w = image.width as usize;
    let mut seen = vec![false; w * image.height as usize];
    let mut stack = vec![(x, y)];
    let (mut x0, mut y0, mut x1, mut y1) = (x, y, x, y);
    let mut count = 0usize;
    while let Some((px, py)) = stack.pop() {
        let i = py as usize * w + px as usize;
        if seen[i] {
            continue;
        }
        seen[i] = true;
        if !image.pixel(px, py).is_some_and(|c| similar(c, start, tolerance)) {
            continue;
        }
        count += 1;
        if count > max_pixels {
            break;
        }
        x0 = x0.min(px);
        y0 = y0.min(py);
        x1 = x1.max(px);
        y1 = y1.max(py);
        if px > 0 {
            stack.push((px - 1, py));
        }
        if py > 0 {
            stack.push((px, py - 1));
        }
        if px + 1 < image.width {
            stack.push((px + 1, py));
        }
        if py + 1 < image.height {
            stack.push((px, py + 1));
        }
    }
    Some(IRect {
        x: x0,
        y: y0,
        w: x1 - x0 + 1,
        h: y1 - y0 + 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::Bitmap;
    use crate::color::rgb;

    fn two_boxes() -> Bitmap {
        let mut b = Bitmap::filled(100, 50, rgb(255, 255, 255));
        b.fill_rect(
            IRect {
                x: 10,
                y: 10,
                w: 20,
                h: 20,
            },
            rgb(0, 0, 0),
        );
        b.fill_rect(
            IRect {
                x: 60,
                y: 10,
                w: 20,
                h: 20,
            },
            rgb(0, 0, 0),
        );
        b
    }

    #[test]
    fn measures_gap_between_boxes() {
        let img = two_boxes();
        assert_eq!(measure_span(img.view(), 45, 20, Axis::Horizontal, 8), Some((30, 60)));
        assert_eq!(measure_span(img.view(), 20, 20, Axis::Vertical, 8), Some((10, 30)));
    }

    #[test]
    fn trims_selection_to_content() {
        let img = two_boxes();
        let t = trim_to_content(
            img.view(),
            IRect {
                x: 0,
                y: 0,
                w: 50,
                h: 50,
            },
            8,
        );
        assert_eq!(
            t,
            IRect {
                x: 10,
                y: 10,
                w: 20,
                h: 20
            }
        );
    }

    #[test]
    fn finds_monotone_region() {
        let img = two_boxes();
        assert_eq!(
            monotone_region(img.view(), 65, 15, 8, 100_000),
            Some(IRect {
                x: 60,
                y: 10,
                w: 20,
                h: 20
            })
        );
    }
}
