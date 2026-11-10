use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

pub const fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

impl Pt {
    pub fn distance(self, other: Pt) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    pub fn offset(self, dx: f32, dy: f32) -> Pt {
        pt(self.x + dx, self.y + dy)
    }

    pub fn lerp(self, other: Pt, t: f32) -> Pt {
        pt(self.x + (other.x - self.x) * t, self.y + (other.y - self.y) * t)
    }
}

/// Axis-aligned rectangle in image pixels. Width and height are never negative
/// once built through [`Rect::from_points`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub const fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

impl Rect {
    pub fn from_points(a: Pt, b: Pt) -> Rect {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        rect(x, y, (a.x - b.x).abs(), (a.y - b.y).abs())
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> Pt {
        pt(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn is_empty(&self) -> bool {
        self.w < 1.0 || self.h < 1.0
    }

    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }

    pub fn inflate(&self, d: f32) -> Rect {
        rect(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        rect(self.x + dx, self.y + dy, self.w, self.h)
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let r = self.right().max(other.right());
        let b = self.bottom().max(other.bottom());
        rect(x, y, r - x, b - y)
    }

    pub fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        rect(x, y, (r - x).max(0.0), (b - y).max(0.0))
    }

    /// Rounds outward to whole pixels and clamps to `width` x `height`.
    pub fn to_pixels(&self, width: u32, height: u32) -> IRect {
        let x0 = self.x.floor().clamp(0.0, width as f32) as u32;
        let y0 = self.y.floor().clamp(0.0, height as f32) as u32;
        let x1 = self.right().ceil().clamp(0.0, width as f32) as u32;
        let y1 = self.bottom().ceil().clamp(0.0, height as f32) as u32;
        IRect {
            x: x0,
            y: y0,
            w: x1.saturating_sub(x0),
            h: y1.saturating_sub(y0),
        }
    }

    pub fn corners(&self) -> [Pt; 4] {
        [
            pt(self.x, self.y),
            pt(self.right(), self.y),
            pt(self.right(), self.bottom()),
            pt(self.x, self.bottom()),
        ]
    }
}

/// Whole-pixel rectangle used for cropping and pixel effects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl IRect {
    pub fn to_rect(self) -> Rect {
        rect(self.x as f32, self.y as f32, self.w as f32, self.h as f32)
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
}

/// Distance from `p` to the segment `a`-`b`.
pub fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    p.distance(pt(a.x + t * dx, a.y + t * dy))
}

pub fn distance_to_polyline(p: Pt, points: &[Pt]) -> f32 {
    match points {
        [] => f32::INFINITY,
        [only] => p.distance(*only),
        _ => points
            .windows(2)
            .map(|w| distance_to_segment(p, w[0], w[1]))
            .fold(f32::INFINITY, f32::min),
    }
}

pub fn ellipse_points(r: &Rect, steps: usize) -> Vec<Pt> {
    let c = r.center();
    (0..steps)
        .map(|i| {
            let a = i as f32 / steps as f32 * std::f32::consts::TAU;
            pt(c.x + r.w / 2.0 * a.cos(), c.y + r.h / 2.0 * a.sin())
        })
        .collect()
}

pub fn circle_points(c: Pt, radius: f32, steps: usize) -> Vec<Pt> {
    ellipse_points(&rect(c.x - radius, c.y - radius, radius * 2.0, radius * 2.0), steps)
}

/// Rounded rectangle outline, clockwise, with `radius` clamped to fit.
pub fn rounded_rect_points(r: &Rect, radius: f32) -> Vec<Pt> {
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    if radius < 0.5 {
        return r.corners().to_vec();
    }
    let mut out = Vec::with_capacity(4 * 9);
    let centers = [
        (pt(r.right() - radius, r.y + radius), -90.0f32),
        (pt(r.right() - radius, r.bottom() - radius), 0.0),
        (pt(r.x + radius, r.bottom() - radius), 90.0),
        (pt(r.x + radius, r.y + radius), 180.0),
    ];
    for (c, start) in centers {
        for i in 0..=8 {
            let a = (start + i as f32 * 90.0 / 8.0).to_radians();
            out.push(pt(c.x + radius * a.cos(), c.y + radius * a.sin()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_from_points_normalizes() {
        let r = Rect::from_points(pt(10.0, 20.0), pt(4.0, 2.0));
        assert_eq!(r, rect(4.0, 2.0, 6.0, 18.0));
    }

    #[test]
    fn to_pixels_rounds_outward_and_clamps() {
        let r = rect(-3.5, 1.2, 10.0, 200.0).to_pixels(100, 50);
        assert_eq!(
            r,
            IRect {
                x: 0,
                y: 1,
                w: 7,
                h: 49
            }
        );
    }

    #[test]
    fn segment_distance() {
        assert_eq!(distance_to_segment(pt(5.0, 3.0), pt(0.0, 0.0), pt(10.0, 0.0)), 3.0);
        assert_eq!(distance_to_segment(pt(-4.0, 3.0), pt(0.0, 0.0), pt(10.0, 0.0)), 5.0);
    }
}
