//! Platform-free logic for OShot: the annotation model, the drawing primitives
//! every shape lowers to, pixel effects, measurement, formats and settings.
//!
//! Everything here works on plain data so it can be tested without a window.

pub mod bitmap;
pub mod color;
pub mod effects;
pub mod format;
pub mod geometry;
pub mod gesture;
pub mod measure;
pub mod model;
pub mod naming;
pub mod prims;
pub mod redact;
pub mod settings;
