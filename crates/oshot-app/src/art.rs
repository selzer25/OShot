//! The onboarding's drawings, as path data for [`crate::doodle`]. Each
//! one lists its strokes in the order they are drawn.

use crate::doodle::{Doodle, Ink, Stroke};

/// The OShot mark, 96×96: the "O" ring draws itself, the six shutter blade
/// edges follow one by one, then the lens pops in.
pub static APERTURE: Doodle = Doodle {
    view: (96.0, 96.0),
    strokes: &[
        Stroke::ink("M 48 16 C 65.67 16 80 30.33 80 48 C 80 65.67 65.67 80 48 80 C 30.33 80 16 65.67 16 48 C 16 30.33 30.33 16 48 16 Z")
            .width(5.0)
            .draw(0, 750),
        Stroke::ink("M 57.09 42.75 L 77.31 54.42").width(2.6).draw(520, 260),
        Stroke::ink("M 57.09 53.25 L 57.09 76.59").width(2.6).draw(600, 260),
        Stroke::ink("M 48 58.5 L 27.79 70.17").width(2.6).draw(680, 260),
        Stroke::ink("M 38.91 53.25 L 18.69 41.58").width(2.6).draw(760, 260),
        Stroke::ink("M 38.91 42.75 L 38.91 19.41").width(2.6).draw(840, 260),
        Stroke::ink("M 48 37.5 L 68.21 25.83").width(2.6).draw(920, 260),
        Stroke::ink("M 48 37.5 L 57.09 42.75 L 57.09 53.25 L 48 58.5 L 38.91 53.25 L 38.91 42.75 Z")
            .with(Ink::Accent)
            .filled()
            .pop(1060, 420),
        Stroke::ink("M 45.2 43.8 C 46.19 43.8 47 44.61 47 45.6 C 47 46.59 46.19 47.4 45.2 47.4 C 44.21 47.4 43.4 46.59 43.4 45.6 C 43.4 44.61 44.21 43.8 45.2 43.8 Z")
            .with(Ink::Paper)
            .filled()
            .fade(1380, 200),
    ],
};

/// Screen Recording, 120×104: a display with a marching selection and a
/// pointer nudging at it.
pub static SCREEN: Doodle = Doodle {
    view: (120.0, 104.0),
    strokes: &[
        Stroke::ink(
            "M 18 12 L 102 12 Q 110 12 110 20 L 110 66 Q 110 74 102 74 L 18 74 Q 10 74 10 66 L 10 20 Q 10 12 18 12 Z",
        )
        .draw(60, 800),
        Stroke::ink("M 52 74 L 50 88 M 68 74 L 70 88 M 40 90 L 80 90").draw(700, 350),
        Stroke::ink("M 26 25 L 94 25 L 94 61 L 26 61 Z")
            .with(Ink::Accent)
            .width(2.5)
            .marching()
            .fade(850, 300),
        Stroke::ink("M 74 40 L 74 58 L 78.5 53.5 L 82 61 L 85 59.6 L 81.6 52.4 L 87.5 52.4 Z")
            .width(2.5)
            .fade(1100, 300)
            .bobbing(),
    ],
};

/// Accessibility, 120×104: two key caps side by side, a plus between.
/// The key symbols are text laid over the caps.
pub static KEYS: Doodle = Doodle {
    view: (120.0, 104.0),
    strokes: &[
        Stroke::ink(
            "M 19 32 L 39 32 Q 48 32 48 41 L 48 61 Q 48 70 39 70 L 19 70 Q 10 70 10 61 L 10 41 Q 10 32 19 32 Z",
        )
        .draw(60, 600),
        Stroke::ink(
            "M 81 32 L 101 32 Q 110 32 110 41 L 110 61 Q 110 70 101 70 L 81 70 Q 72 70 72 61 L 72 41 Q 72 32 81 32 Z",
        )
        .draw(300, 600),
        Stroke::ink("M 60 46.5 L 60 55.5 M 55.5 51 L 64.5 51")
            .with(Ink::Accent)
            .width(3.0)
            .pop(900, 380),
    ],
};

/// The editor choice, 64×56: a pen drawing a line.
pub static PEN: Doodle = Doodle {
    view: (64.0, 56.0),
    strokes: &[
        Stroke::ink("M 14 44 Q 28 30 38 38 T 54 30")
            .with(Ink::Accent)
            .width(3.5)
            .draw(200, 500),
        Stroke::ink("M 40 8 L 50 18 L 26 42 L 14 44 L 16 32 Z")
            .width(3.0)
            .draw(0, 450),
    ],
};

/// The thumbnail choice, 64×56: a screen with a small card in its corner.
pub static CORNER: Doodle = Doodle {
    view: (64.0, 56.0),
    strokes: &[
        Stroke::ink("M 12 6 L 52 6 Q 58 6 58 12 L 58 40 Q 58 46 52 46 L 12 46 Q 6 46 6 40 L 6 12 Q 6 6 12 6 Z")
            .width(3.0)
            .draw(0, 500),
        Stroke::ink(
            "M 37 28 L 49 28 Q 52 28 52 31 L 52 37 Q 52 40 49 40 L 37 40 Q 34 40 34 37 L 34 31 Q 34 28 37 28 Z",
        )
        .with(Ink::Accent)
        .width(3.0)
        .pop(380, 360),
    ],
};

/// The copy-only choice, 64×56: a clipboard with a tick.
pub static CLIP: Doodle = Doodle {
    view: (64.0, 56.0),
    strokes: &[
        Stroke::ink("M 21 8 L 43 8 Q 48 8 48 13 L 48 45 Q 48 50 43 50 L 21 50 Q 16 50 16 45 L 16 13 Q 16 8 21 8 Z")
            .width(3.0)
            .draw(0, 500),
        Stroke::ink("M 25 8 L 25 4 L 39 4 L 39 8").width(3.0).draw(300, 200),
        Stroke::ink("M 24 30 L 30 36 L 41 23")
            .with(Ink::Accent)
            .width(3.5)
            .draw(420, 300),
    ],
};

/// Everything done, 120×104: a loose circle and a big tick.
pub static ALL_SET: Doodle = Doodle {
    view: (120.0, 104.0),
    strokes: &[
        Stroke::ink("M 38 54 L 54 70 L 84 34").with(Ink::Green).width(6.0).draw(60, 480),
        Stroke::ink("M 60 12 C 84 12 102 30 102 52 C 102 75 84 92 60 92 C 36 92 18 75 18 52 C 18 31 35 13 58 12.5 C 64 12.3 69 13.2 74 15")
            .with(Ink::Soft)
            .width(3.0)
            .draw(420, 700),
    ],
};

/// Checklist markers, 40×40.
pub static MARK_TODO: Doodle = Doodle {
    view: (40.0, 40.0),
    strokes: &[Stroke::ink(
        "M 20 6 C 27.73 6 34 12.27 34 20 C 34 27.73 27.73 34 20 34 C 12.27 34 6 27.73 6 20 C 6 12.27 12.27 6 20 6 Z",
    )
    .with(Ink::Track)
    .width(3.0)],
};

pub static MARK_ACTIVE: Doodle = Doodle {
    view: (40.0, 40.0),
    strokes: &[
        Stroke::ink("M 20 6 C 27.73 6 34 12.27 34 20 C 34 27.73 27.73 34 20 34 C 12.27 34 6 27.73 6 20 C 6 12.27 12.27 6 20 6 Z").with(Ink::Accent).width(3.0).draw(0, 420),
        Stroke::ink("M 20 15 C 22.76 15 25 17.24 25 20 C 25 22.76 22.76 25 20 25 C 17.24 25 15 22.76 15 20 C 15 17.24 17.24 15 20 15 Z").with(Ink::Accent).filled().pop(280, 340),
    ],
};

pub static MARK_DONE: Doodle = Doodle {
    view: (40.0, 40.0),
    strokes: &[
        Stroke::ink("M 20 6 C 27.73 6 34 12.27 34 20 C 34 27.73 27.73 34 20 34 C 12.27 34 6 27.73 6 20 C 6 12.27 12.27 6 20 6 Z").with(Ink::Green).width(3.0).draw(0, 420),
        Stroke::ink("M 13 21 L 18 26 L 27.5 14.5").with(Ink::Green).width(3.5).draw(240, 320),
    ],
};

/// The swoosh under a heading, 132×12.
pub static UNDERLINE: Doodle = Doodle {
    view: (132.0, 12.0),
    strokes: &[Stroke::ink("M 4 8 C 34 4 78 3 128 6")
        .with(Ink::Accent)
        .width(3.5)
        .draw(300, 600)],
};

/// "Waiting on System Settings", 200×20.
pub static WAITING: Doodle = Doodle {
    view: (200.0, 20.0),
    strokes: &[Stroke::ink("M 6 10 q 6 -6 12 0 t 12 0 t 12 0 t 12 0 t 12 0 t 12 0 t 12 0 t 12 0 t 12 0 t 12 0 t 12 0 t 12 0 t 12 0 t 12 0 t 12 0")
        .with(Ink::Soft)
        .width(2.5)
        .marching()],
};

/// The drag helper's frame around the app icon, 80×80.
pub static DRAG_FRAME: Doodle = Doodle {
    view: (80.0, 80.0),
    strokes: &[
        Stroke::ink("M 22 6 L 58 6 Q 74 6 74 22 L 74 58 Q 74 74 58 74 L 22 74 Q 6 74 6 58 L 6 22 Q 6 6 22 6 Z")
            .with(Ink::Accent)
            .width(2.5)
            .marching()
            .fade(0, 300),
    ],
};

/// The drag helper's arrow, 34×44: straight up, toward the list.
pub static UP_ARROW: Doodle = Doodle {
    view: (34.0, 44.0),
    strokes: &[
        Stroke::ink("M 17 40 L 17 8")
            .with(Ink::Accent)
            .width(3.5)
            .draw(150, 350)
            .bobbing(),
        Stroke::ink("M 7 18 L 17 8 L 27 18")
            .with(Ink::Accent)
            .width(3.5)
            .draw(450, 250)
            .bobbing(),
    ],
};
