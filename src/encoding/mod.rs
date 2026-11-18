//! Pioneer's value encodings, shared by the file formats.
//!
//! - [`Bpm`] and [`BeatInBar`]: tempo and position in the bar, as stored in beat grids.
//! - [`Millis`]: a time in milliseconds.
//! - UTF-16BE strings ending in a two-byte NUL (crate-private), as in the ANLZ `PPTH` tag.

mod tempo;
mod time;
pub(crate) mod utf16;

pub use tempo::{BeatInBar, Bpm};
pub use time::Millis;
