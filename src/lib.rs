//! Read and write Pioneer CDJ/rekordbox USB exports without rekordbox.
//!
//! # Scope
//!
//! This crate owns Pioneer's on-stick formats: DeviceSQL `export.pdb` pages,
//! ANLZ tags (`.DAT`/`.EXT`), Pioneer's encodings (waveform bytes, key table,
//! string forms), and the `PIONEER/` and `/Contents/Artist/Album` path layout.
//!
//! # Boundary
//!
//! This crate has no audio decoding, no analysis and no image processing.
//! Callers supply beatgrids, cues, waveform envelopes and JPEG artwork.
//!
//! # References
//!
//! Format reference: Deep Symmetry's djl-analysis,
//! <https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/>.
//! rekordcrate (MIT) may be used as a dev-dependency to cross-check output.
//! Code from GPL projects such as rbox is not copied.

pub mod anlz;
mod bytes;
pub mod encoding;
mod error;
pub mod pdb;

pub use error::{DecodeError, DecodeErrorKind, EncodeError, Error};
