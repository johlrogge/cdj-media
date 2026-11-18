//! Rekordbox's values for fields this crate does not understand, used when building tags
//! from scratch (see ADR-004).
//!
//! Each value is the most common value of its field in the real-stick fixture
//! (`local-fixtures/real-stick`), checked by the local test
//! `tests/anlz_local.rs::real_stick_defaults_are_modal`, which also prints every
//! distribution. The survey covers the stick's 848 `.DAT` files only: 1,696 `PCOB` tags
//! and 604 `PCPT` entries (167 hot cues, 437 memory points). `.EXT` files are not surveyed.
//!
//! The order links and `memory_count` of memory point lists are not defaults: the builders
//! compute them (see `CueList::memory`), checked by
//! `tests/anlz_local.rs::real_stick_cue_list_builders_match_rekordbox`.

/// `PQTZ` `unknown1` (offset 0x0c): 0 in all 848 `.DAT` files. Checked by
/// `real_stick_defaults_are_modal`.
pub(crate) const PQTZ_UNKNOWN1: u32 = 0;

/// `PQTZ` `unknown2` (offset 0x10): `0x00080000` in all 848 `.DAT` files. djl-analysis
/// documents `00800000`, which does not match. Checked by `real_stick_defaults_are_modal`.
pub(crate) const PQTZ_UNKNOWN2: u32 = 0x0008_0000;

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
