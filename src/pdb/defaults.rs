//! Rekordbox's values for row fields this crate does not understand, used when building rows
//! from scratch (see ADR-002 and ADR-007).
//!
//! Each value is the most common value of its field in the real-stick fixture
//! (`local-fixtures/real-stick`), checked by the local test
//! `tests/pdb_rows_local.rs::real_stick_row_defaults_are_modal`, which also prints every
//! distribution. The survey covers 827 track, 277 artist, 239 album and 8 colour rows. The
//! one exception is track `unknown2`, which has no common value; see [`track_unknown2`]
//! and ADR-008.
//!
//! Fields that rekordbox appears to derive are not defaults: the builders compute them, and
//! `tests/pdb_rows_local.rs::real_stick_rows_rebuild_from_their_values` checks them. They
//! are a key's `id2` (equal to its id in 42 of 43 rows), a colour's `unknown2` (equal to its
//! id in all 8 rows), and the `index_shift` of tracks, artists and albums, which is 0x20 ×
//! the row's slot in its page's row index; it is 0 in built rows until the page writer sets
//! it.

use super::{TrackId, TrackString};

/// Track `subtype` (offset 0): 0x24 in all 827 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const TRACK_SUBTYPE: u16 = 0x24;

/// Track `bitmask` (offset 4): 0xc0700 in 816 of 827 rows, 0xd0700 in the other 11. Checked
/// by `real_stick_row_defaults_are_modal`.
pub(crate) const TRACK_BITMASK: u32 = 0xc0700;

/// Track `unknown2` (offset 0x14) in built rows: the track's id. **Not a modal default**
/// (see ADR-008): the field is different in every one of the 827 rows (up to 28 bits,
/// checked by `real_stick_track_unknown2_is_unique`), so there is no rekordbox value to
/// copy. The id is unique and non-zero like rekordbox's values, where a constant would make
/// a pattern rekordbox never writes. Not checked by `real_stick_row_defaults_are_modal`.
pub(crate) fn track_unknown2(id: TrackId) -> u32 {
    id.get()
}

/// Track `unknown3` (offset 0x18): 0x3520 in all 827 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const TRACK_UNKNOWN3: u16 = 0x3520;

/// Track `unknown4` (offset 0x1a): 0x1f58 in all 827 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const TRACK_UNKNOWN4: u16 = 0x1f58;

/// Track `unknown5` (offset 0x56): 0x29 in all 827 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const TRACK_UNKNOWN5: u16 = 0x29;

/// Track `unknown7` (offset 0x5c): 3 in all 827 rows. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const TRACK_UNKNOWN7: u16 = 3;

/// The text of the track string slots that are not understood, by slot. Each is the most
/// common text of its slot across the 827 rows: slot 2 is a number of 1 or 2 digits, never
/// empty ("2" in 307 rows); slot 3 a single digit, never empty ("1" in 623); slot 4 empty
/// (741) or 1 or 2 digits; slots 8, 9, 13 and 18 are empty in every row. Checked by
/// `real_stick_row_defaults_are_modal`.
pub(crate) const TRACK_UNKNOWN_STRINGS: [(TrackString, &str); 7] = [
    (TrackString::Unknown2, "2"),
    (TrackString::Unknown3, "1"),
    (TrackString::Unknown4, ""),
    (TrackString::Unknown8, ""),
    (TrackString::Unknown9, ""),
    (TrackString::Unknown13, ""),
    (TrackString::Unknown18, ""),
];

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

/// Playlist tree `unknown` (offset 4): 0 in all 32 rows of the real stick and the one row of
/// the fresh export. Checked by `writer::local::real_playlist_rows_round_trip`.
pub(crate) const PLAYLIST_UNKNOWN: u32 = 0;
