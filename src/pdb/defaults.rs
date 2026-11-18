//! Rekordbox's values for row fields this crate does not understand, used when building rows
//! from scratch (see ADR-002 and ADR-007).
//!
//! Each value is the most common value of its field in the real-stick fixture
//! (`local-fixtures/real-stick`), checked by the local test
//! `tests/pdb_rows_local.rs::real_stick_row_defaults_are_modal`, which also prints every
//! distribution. The survey covers 277 artist, 239 album and 8 colour rows.
//!
//! Fields that rekordbox appears to derive are not defaults: the builders compute them, and
//! `tests/pdb_rows_local.rs::real_stick_rows_rebuild_from_their_values` checks them. They
//! are a key's `id2` (equal to its id in 42 of 43 rows), a colour's `unknown2` (equal to its
//! id in all 8 rows), and the `index_shift` of artists and albums, which is 0x20 × the
//! row's slot in its page's row index; it is 0 in built rows until the page writer sets it.

/// Artist `subtype` (offset 0): 0x60, the near layout, in all 277 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const ARTIST_SUBTYPE: u16 = 0x60;

/// Artist `unknown1` (offset 8, the byte before the name offset): 3 in all 277 rows.
/// Checked by `real_stick_row_defaults_are_modal`.
pub(crate) const ARTIST_UNKNOWN1: u16 = 3;

/// Album `subtype` (offset 0): 0x80, the near layout, in all 239 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const ALBUM_SUBTYPE: u16 = 0x80;

/// Album `unknown1` (offset 4): 0 in all 239 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const ALBUM_UNKNOWN1: u32 = 0;

/// Album `unknown2` (offset 0x10): 0 in all 239 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const ALBUM_UNKNOWN2: u32 = 0;

/// Album `unknown3` (offset 0x14, the byte before the name offset): 3 in all 239 rows.
/// Checked by `real_stick_row_defaults_are_modal`.
pub(crate) const ALBUM_UNKNOWN3: u16 = 3;

/// Colour `unknown1` (offset 0): 0 in all 8 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const COLOR_UNKNOWN1: u32 = 0;

/// Colour `unknown3` (offset 7): 0 in all 8 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const COLOR_UNKNOWN3: u8 = 0;
