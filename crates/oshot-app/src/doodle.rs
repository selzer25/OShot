//! Hand-drawn ink doodles for the onboarding: SVG-style path data drawn as
//! round-capped strokes with a slight, fixed wobble, and entrance motion
//! (strokes drawing on, dots popping in) timed from when they appeared.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use gpui::{App, Bounds, Hsla, IntoElement, PathBuilder, PathStyle, Pixels, Styled, Window, canvas, point, px};
use lyon::tessellation::{FillOptions, LineCap, LineJoin, StrokeOptions};

use crate::theme::Paper;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Line,
    Soft,
    Accent,
    Green,
    Track,
    Paper,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    Still,
    /// The stroke is drawn from its start, like a pen.
    Draw {
        delay: u32,
        ms: u32,
    },
    /// Grows from 30% to 115% and settles at 100%, fading in.
    Pop {
        delay: u32,
        ms: u32,
    },
    Fade {
        delay: u32,
        ms: u32,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    pub d: &'static str,
    pub ink: Ink,
    pub width: f32,
    pub motion: Motion,
    pub fill: bool,
    /// Bobs up and down forever, to point at something.
    pub bob: bool,
    /// Dashes that crawl along the path forever, for "waiting".
    pub march: bool,
}

impl Stroke {
    pub const fn ink(d: &'static str) -> Stroke {
        Stroke {
            d,
            ink: Ink::Line,
            width: 4.0,
            motion: Motion::Still,
            fill: false,
            bob: false,
            march: false,
        }
    }
    pub const fn with(mut self, ink: Ink) -> Stroke {
        self.ink = ink;
        self
    }
    pub const fn width(mut self, w: f32) -> Stroke {
        self.width = w;
        self
    }
    pub const fn draw(mut self, delay: u32, ms: u32) -> Stroke {
        self.motion = Motion::Draw { delay, ms };
        self
    }
    pub const fn pop(mut self, delay: u32, ms: u32) -> Stroke {
        self.motion = Motion::Pop { delay, ms };
        self
    }
    pub const fn fade(mut self, delay: u32, ms: u32) -> Stroke {
        self.motion = Motion::Fade { delay, ms };
        self
    }
    pub const fn filled(mut self) -> Stroke {
        self.fill = true;
        self
    }
    pub const fn bobbing(mut self) -> Stroke {
        self.bob = true;
        self
    }
    pub const fn marching(mut self) -> Stroke {
        self.march = true;
        self
    }

    fn ends_at(&self) -> u32 {
        match self.motion {
            Motion::Still => 0,
            Motion::Draw { delay, ms } | Motion::Pop { delay, ms } | Motion::Fade { delay, ms } => delay + ms,
        }
    }
}

pub struct Doodle {
    /// The coordinate box the path data is written in.
    pub view: (f32, f32),
    pub strokes: &'static [Stroke],
}

impl Doodle {
    /// Milliseconds until every entrance has finished.
    pub fn settles_at(&self) -> u32 {
        self.strokes.iter().map(Stroke::ends_at).max().unwrap_or(0)
    }

    fn forever(&self) -> bool {
        self.strokes.iter().any(|s| s.bob || s.march)
    }
}

fn color(paper: &Paper, ink: Ink) -> Hsla {
    match ink {
        Ink::Line => paper.ink,
        Ink::Soft => paper.soft,
        Ink::Accent => paper.accent,
        Ink::Green => paper.green,
        Ink::Track => paper.track,
        Ink::Paper => paper.bg,
    }
}

type Line = Vec<(f32, f32)>;

/// One stroke's subpaths, flattened and wobbled once, with running lengths
/// for drawing on by distance.
struct Shape {
    parts: Vec<(Line, Vec<f32>, bool)>,
    length: f32,
    center: (f32, f32),
}

fn shapes(doodle: &'static Doodle) -> &'static [Shape] {
    static CACHE: OnceLock<Mutex<HashMap<usize, &'static [Shape]>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    cache.entry(doodle as *const Doodle as usize).or_insert_with(|| {
        let built: Vec<Shape> = doodle
            .strokes
            .iter()
            .enumerate()
            .map(|(i, s)| build(s, i as u32))
            .collect();
        Box::leak(built.into_boxed_slice())
    })
}

fn build(stroke: &Stroke, seed: u32) -> Shape {
    let parts: Vec<(Line, Vec<f32>, bool)> = parse(stroke.d)
        .into_iter()
        .map(|(points, closed)| {
            let points = if stroke.fill { points } else { wobble(&points, seed) };
            let mut lengths = Vec::with_capacity(points.len());
            let mut total = 0.0;
            for (j, p) in points.iter().enumerate() {
                if j > 0 {
                    total += dist(points[j - 1], *p);
                }
                lengths.push(total);
            }
            (points, lengths, closed)
        })
        .collect();
    let length = parts.iter().map(|(_, l, _)| l.last().copied().unwrap_or(0.0)).sum();
    let all: Vec<(f32, f32)> = parts.iter().flat_map(|(p, _, _)| p.iter().copied()).collect();
    let n = all.len().max(1) as f32;
    let center = (
        all.iter().map(|p| p.0).sum::<f32>() / n,
        all.iter().map(|p| p.1).sum::<f32>() / n,
    );
    Shape { parts, length, center }
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
}

/// A small, smooth sideways drift along the stroke so lines look drawn by
/// hand. Fixed per stroke, so nothing shimmers.
fn wobble(points: &[(f32, f32)], seed: u32) -> Line {
    let phase = seed as f32 * 1.7;
    let mut along = 0.0;
    points
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            if i > 0 {
                along += dist(points[i - 1], p);
            }
            let (dx, dy) = match (points.get(i.saturating_sub(1)), points.get(i + 1)) {
                (Some(a), Some(b)) => (b.0 - a.0, b.1 - a.1),
                _ => (0.0, 0.0),
            };
            let len = (dx * dx + dy * dy).sqrt().max(0.0001);
            let off = 0.7 * (along / 23.0 + phase).sin() + 0.35 * (along / 9.0 + phase * 2.3).sin();
            (p.0 - dy / len * off, p.1 + dx / len * off)
        })
        .collect()
}

const STEP: f32 = 1.5;

/// Path data with `M L H V C Q Z`, absolute or relative, repeats allowed.
/// Returns flattened subpaths and whether each is closed.
pub fn parse(d: &str) -> Vec<(Line, bool)> {
    let mut out = Vec::new();
    let mut line: Line = Vec::new();
    let (mut pen, mut start) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
    let mut nums: Vec<f32> = Vec::new();
    let mut tokens = tokenize(d).into_iter().peekable();
    while let Some(tok) = tokens.next() {
        let Token::Cmd(cmd) = tok else { continue };
        nums.clear();
        while let Some(Token::Num(n)) = tokens.peek() {
            nums.push(*n);
            tokens.next();
        }
        let rel = cmd.is_ascii_lowercase();
        let arity = match cmd.to_ascii_uppercase() {
            'M' | 'L' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'Q' => 4,
            _ => 0,
        };
        if arity == 0 {
            if cmd.eq_ignore_ascii_case(&'z') {
                lineto(&mut line, start);
                pen = start;
                if line.len() > 1 {
                    out.push((std::mem::take(&mut line), true));
                }
            }
            continue;
        }
        for (k, c) in nums.chunks_exact(arity).enumerate() {
            let base = if rel { pen } else { (0.0, 0.0) };
            let at = |x: f32, y: f32| (base.0 + x, base.1 + y);
            match cmd.to_ascii_uppercase() {
                'M' if k == 0 => {
                    if line.len() > 1 {
                        out.push((std::mem::take(&mut line), false));
                    }
                    line.clear();
                    pen = at(c[0], c[1]);
                    start = pen;
                    line.push(pen);
                }
                'M' | 'L' => {
                    pen = at(c[0], c[1]);
                    lineto(&mut line, pen);
                }
                'H' => {
                    pen = (if rel { pen.0 + c[0] } else { c[0] }, pen.1);
                    lineto(&mut line, pen);
                }
                'V' => {
                    pen = (pen.0, if rel { pen.1 + c[0] } else { c[0] });
                    lineto(&mut line, pen);
                }
                'C' => {
                    let (c1, c2, to) = (at(c[0], c[1]), at(c[2], c[3]), at(c[4], c[5]));
                    curve(&mut line, pen, c1, c2, to);
                    pen = to;
                }
                'Q' => {
                    let (q, to) = (at(c[0], c[1]), at(c[2], c[3]));
                    // A quadratic is a cubic with both handles 2/3 of the way to its control.
                    let c1 = (pen.0 + (q.0 - pen.0) * 2.0 / 3.0, pen.1 + (q.1 - pen.1) * 2.0 / 3.0);
                    let c2 = (to.0 + (q.0 - to.0) * 2.0 / 3.0, to.1 + (q.1 - to.1) * 2.0 / 3.0);
                    curve(&mut line, pen, c1, c2, to);
                    pen = to;
                }
                _ => {}
            }
        }
    }
    if line.len() == 1 {
        let p = line[0];
        line.push((p.0 + 0.01, p.1));
    }
    if line.len() > 1 {
        out.push((line, false));
    }
    out
}

enum Token {
    Cmd(char),
    Num(f32),
}

fn tokenize(d: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut num = String::new();
    let flush = |num: &mut String, out: &mut Vec<Token>| {
        if let Ok(n) = num.parse() {
            out.push(Token::Num(n));
        }
        num.clear();
    };
    for ch in d.chars() {
        match ch {
            '0'..='9' | '.' => {
                if ch == '.' && num.contains('.') {
                    flush(&mut num, &mut out);
                }
                num.push(ch);
            }
            '-' | '+' => {
                flush(&mut num, &mut out);
                num.push(ch);
            }
            c if c.is_ascii_alphabetic() => {
                flush(&mut num, &mut out);
                out.push(Token::Cmd(c));
            }
            _ => flush(&mut num, &mut out),
        }
    }
    flush(&mut num, &mut out);
    out
}

fn lineto(line: &mut Line, to: (f32, f32)) {
    let Some(&from) = line.last() else {
        line.push(to);
        return;
    };
    let n = (dist(from, to) / STEP).ceil().max(1.0) as usize;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        line.push((from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t));
    }
}

fn curve(line: &mut Line, a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32)) {
    if line.is_empty() {
        line.push(a);
    }
    let n = ((dist(a, b) + dist(b, c) + dist(c, d)) / STEP).ceil().max(2.0) as usize;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        line.push((
            w0 * a.0 + w1 * b.0 + w2 * c.0 + w3 * d.0,
            w0 * a.1 + w1 * b.1 + w2 * c.1 + w3 * d.1,
        ));
    }
}

/// The first `len` of a polyline.
fn trim(points: &[(f32, f32)], lengths: &[f32], len: f32) -> Line {
    let mut out = Vec::new();
    for (i, &p) in points.iter().enumerate() {
        if lengths[i] <= len {
            out.push(p);
            continue;
        }
        if i > 0 {
            let span = (lengths[i] - lengths[i - 1]).max(0.0001);
            let t = (len - lengths[i - 1]) / span;
            let a = points[i - 1];
            out.push((a.0 + (p.0 - a.0) * t, a.1 + (p.1 - a.1) * t));
        }
        break;
    }
    out
}

/// Pieces of a polyline `on` long every `on + off`, shifted by `phase`.
fn dashes(points: &[(f32, f32)], lengths: &[f32], on: f32, off: f32, phase: f32) -> Vec<Line> {
    let total = lengths.last().copied().unwrap_or(0.0);
    let mut out = Vec::new();
    let mut s = -phase.rem_euclid(on + off);
    while s < total {
        let (a, b) = (s.max(0.0), (s + on).min(total));
        if b > a {
            let mut piece: Line = points
                .iter()
                .zip(lengths)
                .filter(|(_, l)| **l > a && **l < b)
                .map(|(p, _)| *p)
                .collect();
            let at = |x: f32| {
                let i = lengths.partition_point(|l| *l < x).clamp(1, points.len() - 1);
                let span = (lengths[i] - lengths[i - 1]).max(0.0001);
                let t = ((x - lengths[i - 1]) / span).clamp(0.0, 1.0);
                let (p, q) = (points[i - 1], points[i]);
                (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t)
            };
            piece.insert(0, at(a));
            piece.push(at(b));
            out.push(piece);
        }
        s += on + off;
    }
    out
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// How far along a motion is, 0..1, `ms` after the doodle appeared.
fn progress(m: Motion, ms: f32) -> f32 {
    match m {
        Motion::Still => 1.0,
        Motion::Draw { delay, ms: d } | Motion::Pop { delay, ms: d } | Motion::Fade { delay, ms: d } => {
            ((ms - delay as f32) / d.max(1) as f32).clamp(0.0, 1.0)
        }
    }
}

/// Draws `doodle` into a `w × h` box with its entrance timed from
/// `shown_at`. Under Reduce Motion it shows the finished drawing, still.
pub fn doodle(doodle: &'static Doodle, paper: Paper, shown_at: Instant, w: f32, h: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window: &mut Window, cx: &mut App| {
            let still = cx.reduce_motion();
            let ms = if still {
                f32::INFINITY
            } else {
                shown_at.elapsed().as_secs_f32() * 1000.0
            };
            paint(doodle, &paper, bounds, ms, window);
            if !still && (ms < doodle.settles_at() as f32 + 32.0 || doodle.forever()) {
                window.request_animation_frame();
            }
        },
    )
    .w(px(w))
    .h(px(h))
}

fn paint(doodle: &'static Doodle, paper: &Paper, bounds: Bounds<Pixels>, ms: f32, window: &mut Window) {
    let (vw, vh) = doodle.view;
    let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let scale = (bw / vw).min(bh / vh);
    let ox = f32::from(bounds.origin.x) + (bw - vw * scale) / 2.0;
    let oy = f32::from(bounds.origin.y) + (bh - vh * scale) / 2.0;
    let seconds = if ms.is_finite() { ms / 1000.0 } else { 0.0 };
    for (stroke, shape) in doodle.strokes.iter().zip(shapes(doodle)) {
        let t = progress(stroke.motion, ms);
        if t <= 0.0 {
            continue;
        }
        let e = ease_out(t);
        let (mut alpha, mut grow, mut drawn) = (1.0, 1.0, 1.0);
        match stroke.motion {
            Motion::Draw { .. } => drawn = e,
            Motion::Fade { .. } => alpha = e,
            Motion::Pop { .. } => {
                alpha = e;
                grow = if t < 0.65 {
                    0.3 + 0.85 * ease_out(t / 0.65)
                } else {
                    1.15 - 0.15 * ease_out((t - 0.65) / 0.35)
                };
            }
            Motion::Still => {}
        }
        let lift = if stroke.bob && ms.is_finite() {
            -3.0 * (0.5 - 0.5 * (seconds * std::f32::consts::TAU / 1.6).cos())
        } else {
            0.0
        };
        let mut c = color(paper, stroke.ink);
        c.a *= alpha;
        let (cx, cy) = shape.center;
        let map = |(x, y): (f32, f32)| {
            let x = cx + (x - cx) * grow;
            let y = cy + (y - cy) * grow + lift;
            point(px(ox + x * scale), px(oy + y * scale))
        };
        let mut builder = if stroke.fill {
            PathBuilder::fill().with_style(PathStyle::Fill(FillOptions::non_zero()))
        } else {
            PathBuilder::stroke(px(stroke.width * scale)).with_style(PathStyle::Stroke(
                StrokeOptions::default()
                    .with_line_width(stroke.width * scale)
                    .with_line_cap(LineCap::Round)
                    .with_line_join(LineJoin::Round),
            ))
        };
        let mut budget = shape.length * drawn;
        let mut any = false;
        for (points, lengths, closed) in &shape.parts {
            if budget <= 0.0 {
                break;
            }
            let len = lengths.last().copied().unwrap_or(0.0);
            let pieces = if stroke.march && !stroke.fill {
                let phase = if ms.is_finite() { seconds * 7.0 } else { 0.0 };
                dashes(
                    points,
                    lengths,
                    2.2 * stroke.width / 2.5,
                    4.0 * stroke.width / 2.5,
                    phase,
                )
            } else {
                vec![trim(points, lengths, budget)]
            };
            let whole = budget >= len;
            budget -= len;
            for piece in pieces {
                if piece.len() < 2 {
                    continue;
                }
                builder.move_to(map(piece[0]));
                for p in &piece[1..] {
                    builder.line_to(map(*p));
                }
                if stroke.fill || (*closed && whole && !stroke.march) {
                    builder.close();
                }
                any = true;
            }
        }
        if any && let Ok(path) = builder.build() {
            window.paint_path(path, c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_absolute_relative_and_repeats() {
        let parts = parse("M0 0 L10 0 10 10 Z m0 20 l5 0 h5 v5");
        assert_eq!(parts.len(), 2);
        assert!(parts[0].1, "first subpath closes");
        assert_eq!(parts[0].0.first(), Some(&(0.0, 0.0)));
        assert_eq!(parts[0].0.last(), Some(&(0.0, 0.0)));
        let last = parts[1].0.last().unwrap();
        assert!((last.0 - 10.0).abs() < 0.01 && (last.1 - 25.0).abs() < 0.01, "{last:?}");
    }

    #[test]
    fn curves_end_where_they_should() {
        let parts = parse("M0 0 Q10 10 20 0 C25 -5 35 -5 40 0");
        let end = parts[0].0.last().unwrap();
        assert!((end.0 - 40.0).abs() < 0.01 && end.1.abs() < 0.01);
    }

    #[test]
    fn numbers_without_separators() {
        let parts = parse("M1.5.5L-3-4");
        assert_eq!(parts[0].0.first(), Some(&(1.5, 0.5)));
        assert_eq!(parts[0].0.last(), Some(&(-3.0, -4.0)));
    }

    #[test]
    fn a_lone_point_is_a_dot() {
        assert!(parse("M5 5").first().is_some_and(|(p, _)| p.len() == 2));
    }

    #[test]
    fn trimming_and_dashes_follow_length() {
        let pts: Line = (0..=10).map(|i| (i as f32, 0.0)).collect();
        let lens: Vec<f32> = (0..=10).map(|i| i as f32).collect();
        assert!((trim(&pts, &lens, 4.5).last().unwrap().0 - 4.5).abs() < 0.001);
        let d = dashes(&pts, &lens, 2.0, 3.0, 0.0);
        assert_eq!(d.len(), 2);
        assert!((d[1][0].0 - 5.0).abs() < 0.001);
    }

    #[test]
    fn wobble_stays_close() {
        let pts: Line = (0..60).map(|i| (i as f32 * 2.0, 10.0)).collect();
        for (a, b) in pts.iter().zip(wobble(&pts, 3)) {
            assert!(dist(*a, b) <= 1.1);
        }
    }
}
