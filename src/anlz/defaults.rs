//! Rekordbox's values for fields this crate does not understand, used when building tags
//! from scratch (see ADR-004).
//!
//! Each value is the most common value of its field in the real-stick fixture
//! (`local-fixtures/real-stick`), checked by the local test
//! `tests/anlz_local.rs::real_stick_defaults_are_modal`, which also prints every
//! distribution. The survey covers the stick's 848 `.DAT` files: 1,696 `PCOB` tags and 604
//! `PCPT` entries (167 hot cues, 437 memory points). The `.EXT` files' `PWV3` is checked by
//! `real_stick_anlz_tag_orders`, and their header and silence by `ext_files_survey`. The
//! `.DAT` builder `AnlzFile::track_dat` as a whole is checked by
//! `real_stick_dat_files_rebuild_identically`, and the `.EXT` builder `AnlzFile::track_ext`
//! by `ext_prefixes_rebuild_identically`.
//!
//! The order links and `memory_count` of memory point lists are not defaults: the builders
//! compute them (see `CueList::memory`), checked by
//! `tests/anlz_local.rs::real_stick_cue_list_builders_match_rekordbox`.

/// `PMAI` header bytes 12..28 (`len_header` is 28 in all 2,500 ANLZ files): `00000001
/// 00010000 00010000 00000000` in 793 of 848 `.DAT` files. The 22 `.DAT` files without an
/// `.EXT` (stale analyses, 21 without beats) have zeros in the middle two words; the first
/// word is 0 in all 25 MP3 files and 13 others. Checked by `real_stick_defaults_are_modal`.
pub(crate) const PMAI_UNKNOWN_HEADER: [u8; 16] = [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];

/// `PVBR` header bytes 12..16: zero in all 848 `.DAT` files. Checked by
/// `real_stick_dat_files_rebuild_identically`.
pub(crate) const PVBR_HEADER: [u8; 4] = [0; 4];

/// `PVBR` body length: 1,604 bytes in all 848 `.DAT` files. The body is all zero for every
/// AIFF and WAV track (823 files) and holds a seek index for the 25 MP3 tracks. Checked by
/// `real_stick_dat_files_rebuild_identically`.
pub(crate) const PVBR_BODY_LEN: usize = 1_604;

/// `PQTZ` `unknown1` (offset 0x0c): 0 in all 848 `.DAT` files. Checked by
/// `real_stick_defaults_are_modal`.
pub(crate) const PQTZ_UNKNOWN1: u32 = 0;

/// `PQTZ` `unknown2` (offset 0x10): `0x00080000` in all 848 `.DAT` files. djl-analysis
/// documents `00800000`, which does not match. Checked by `real_stick_defaults_are_modal`.
pub(crate) const PQTZ_UNKNOWN2: u32 = 0x0008_0000;

/// `PWAV` `unknown1` (offset 0x10): `0x00010000` in all 848 `.DAT` files. djl-analysis
/// documents `00100000`, which does not match. Checked by `real_stick_defaults_are_modal`.
pub(crate) const PWAV_UNKNOWN1: u32 = 0x0001_0000;

/// Whiteness of the columns of a flat `PWAV` placeholder: 5. The one silent track on the
/// fixture has a `PWAV` of 400 columns `0xa2` (height 2, whiteness 5), and whiteness 5 is
/// the most common one at heights 2 to 9 across all 848 `.DAT` files.
pub(crate) const PWAV_FLAT_WHITENESS: u8 = 5;

/// `PWV2` `unknown1` (offset 0x10): `0x00010000` in all 848 `.DAT` files. djl-analysis
/// documents `00100000`, which does not match. Checked by `real_stick_defaults_are_modal`.
pub(crate) const PWV2_UNKNOWN1: u32 = 0x0001_0000;

/// `PWV3` `unknown1` (offset 0x14): `0x00960000` in all 826 `.EXT` files, as djl-analysis
/// documents. Checked by `real_stick_anlz_tag_orders`.
pub(crate) const PWV3_UNKNOWN1: u32 = 0x0096_0000;

/// Whiteness of the columns of a flat `PWV3` placeholder: 7. The one silent track on the
/// fixture has a `PWV3` of 154 columns `0xe0` (height 0, whiteness 7), and `0xe0` is the
/// last column of 732 of 826 `.EXT` files. Low columns have no modal whiteness. Checked by
/// `tests/anlz_local.rs::ext_files_survey`.
pub(crate) const PWV3_FLAT_WHITENESS: u8 = 7;

/// The `.EXT` `PMAI` header bytes 12..28: the same bytes as [`PMAI_UNKNOWN_HEADER`] in 823
/// of 826 `.EXT` files (all 6 on rekordbox-hw1), first word 0 in the rest. It differs from
/// the `.DAT`'s only where the `.DAT`'s first word is 0 (33 files; 3 of their `.EXT` files
/// have 0 too). Checked by `tests/anlz_local.rs::ext_files_survey`.
pub(crate) const PMAI_EXT_UNKNOWN_HEADER: [u8; 16] = PMAI_UNKNOWN_HEADER;

/// `PCOB` `unknown1` (offset 0x10): 0 in 1,694 of 1,696 lists, 3 in the rest. Checked by
/// `real_stick_defaults_are_modal`.
pub(crate) const PCOB_UNKNOWN1: u16 = 0;

/// `PCOB` `memory_count` (offset 0x14) of hot cue lists and empty memory point lists:
/// `0xFFFFFFFF` in 1,625 of 1,696 lists. Non-empty memory point lists hold the index of
/// their last entry instead. Checked by `real_stick_defaults_are_modal`.
pub(crate) const PCOB_MEMORY_COUNT: u32 = 0xFFFF_FFFF;

/// `PCPT` `status` (offset 0x10): 0 in 596 of 604 entries, 1 in the rest. Checked by
/// `real_stick_defaults_are_modal`.
pub(crate) const PCPT_STATUS: u32 = 0;

/// `PCPT` `unknown1` (offset 0x14): `0x00010000` in 595 of 604 entries, 0 in the rest.
/// Checked by `real_stick_defaults_are_modal`.
pub(crate) const PCPT_UNKNOWN1: u32 = 0x0001_0000;

/// `PCPT` `order_first` (offset 0x18) of hot cues and of the first memory point:
/// `0xFFFF`, in 238 of 604 entries; later memory points hold the previous entry's index.
/// Checked by `real_stick_defaults_are_modal`.
pub(crate) const PCPT_ORDER_FIRST: u16 = 0xFFFF;

/// `PCPT` `order_last` (offset 0x1a) of hot cues and of the last memory point: `0xFFFF`,
/// in 238 of 604 entries; earlier memory points hold the next entry's index. Checked by
/// `real_stick_defaults_are_modal`.
pub(crate) const PCPT_ORDER_LAST: u16 = 0xFFFF;

/// `PCPT` `unknown2` (offsets 0x1d to 0x1f): `00 03 e8` in 598 of 604 entries, `00 00 96`
/// in the rest. Checked by `real_stick_defaults_are_modal`.
pub(crate) const PCPT_UNKNOWN2: [u8; 3] = [0x00, 0x03, 0xe8];

/// `PCPT` `loop_time` (offset 0x24) of a point, which has no loop end: `0xFFFFFFFF` in 595
/// of 603 points, 0 in the rest. Checked by `real_stick_defaults_are_modal`.
pub(crate) const PCPT_POINT_LOOP_TIME: u32 = 0xFFFF_FFFF;

/// `PCPT` `unknown3` (offsets 0x28 to 0x37): all zero in all 604 entries. Checked by
/// `real_stick_defaults_are_modal`.
pub(crate) const PCPT_UNKNOWN3: [u8; 16] = [0; 16];
