//! Artist rows (table 2).

use super::super::defaults;
use super::super::{ArtistId, RawRow};
use super::{Layout, NameTail, TailContexts, read_id};
use crate::bytes::Writer;
use crate::encoding::DeviceSqlString;
use crate::error::{DecodeContext, DecodeError, EncodeError};

/// `subtype` of the far layout.
const SUBTYPE_FAR: u16 = 0x64;

/// The layout `subtype` chooses: far exactly for [`SUBTYPE_FAR`] (ADR-007).
fn layout(subtype: u16) -> Layout {
    if subtype == SUBTYPE_FAR {
        Layout::Far
    } else {
        Layout::Near
    }
}
/// Row offset of the marker field, `unknown1`.
const MARKER_AT: usize = 8;

const CONTEXTS: TailContexts = TailContexts {
    marker: "pdb artist unknown1",
    offset: "pdb artist name offset",
    gap: "pdb artist name gap",
};

/// An artist row (table 2).
///
/// | bytes      | field         | meaning                                               |
/// |------------|---------------|-------------------------------------------------------|
/// | 0..2       | `subtype`     | 0x60 near, 0x64 far; any other value reads as near    |
/// | 2..4       | `index_shift` | djl-analysis' name; 0x20 × the row's slot in its page's row index |
/// | 4..8       | `id`          | [`ArtistId`], never 0                                 |
/// | near: 8    | `unknown1`    | not understood; 3 in real files                       |
/// | near: 9    | name offset   | `u8`, from the row start                              |
/// | far: 8..10 | `unknown1`    | as near, but a `u16`                                  |
/// | far: 10..12| name offset   | `u16`, from the row start                             |
/// | ..name     | name gap      | bytes between the fixed part and the name, verbatim   |
/// | name..     | `name`        | DeviceSQL string                                      |
///
/// The name offset is not stored: it is the fixed part's length plus the gap's. The row
/// ends after its name. Rekordbox writes every row near (all 277 on the real stick), with
/// the name right after the fixed part, or, for a long-form name (UTF-16LE or long ASCII),
/// at offset 12, the next multiple of 4, after two zero gap bytes. `index_shift` is
/// 0x20 × the row's slot in its page's row index, absent rows included, on every real row.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Artist {
    subtype: u16,
    index_shift: u16,
    id: ArtistId,
    tail: NameTail,
}

impl Artist {
    /// Creates an artist row as rekordbox does: near, `index_shift` 0 (the page writer
    /// sets it), rekordbox's `unknown1`, and the name in rekordbox's form and place.
    pub fn new(id: ArtistId, name: &str) -> Self {
        Self {
            subtype: defaults::ARTIST_SUBTYPE,
            index_shift: 0,
            id,
            tail: NameTail::built(defaults::ARTIST_UNKNOWN1, MARKER_AT, name),
        }
    }

    /// Replaces `subtype` (offset 0), which also chooses the near or far layout.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_subtype(mut self, subtype: u16) -> Self {
        self.subtype = subtype;
        self
    }

    /// Replaces `index_shift` (offset 2); the page writer sets it.
    #[must_use]
    pub(crate) fn with_index_shift(mut self, index_shift: u16) -> Self {
        self.index_shift = index_shift;
        self
    }

    /// Replaces `unknown1` (offset 8).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_unknown1(mut self, unknown1: u16) -> Self {
        self.tail = self.tail.with_marker(unknown1);
        self
    }

    /// Replaces the bytes between the fixed part and the name.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_name_gap(mut self, gap: Vec<u8>) -> Self {
        self.tail = self.tail.with_gap(gap);
        self
    }

    /// The row's id.
    pub fn id(&self) -> ArtistId {
        self.id
    }

    /// The name, with its stored form.
    pub fn name(&self) -> &DeviceSqlString {
        self.tail.name()
    }

    /// The `subtype` field (offset 0), as stored.
    pub fn subtype(&self) -> u16 {
        self.subtype
    }

    /// Whether the row uses the far layout, with a `u16` name offset: `subtype` 0x64.
    pub fn is_far(&self) -> bool {
        layout(self.subtype) == Layout::Far
    }

    /// `index_shift` (offset 2), as stored.
    pub fn index_shift(&self) -> u16 {
        self.index_shift
    }

    /// `unknown1` (offset 8), not understood: a byte when near, a `u16` when far.
    pub fn unknown1(&self) -> u16 {
        self.tail.marker()
    }

    /// The bytes between the fixed part and the name; empty or zero in rekordbox's rows.
    pub fn name_gap(&self) -> &[u8] {
        self.tail.gap()
    }

    /// The row's length as if it were far, with the name right after the fixed part: what
    /// rekordbox's heap layout sizes an artist row by (see [`ExportRow::heap_footprint`]).
    ///
    /// [`ExportRow::heap_footprint`]: super::ExportRow::heap_footprint
    pub(super) fn far_len(&self) -> usize {
        MARKER_AT.saturating_add(self.tail.far_len())
    }

    /// Decodes an artist row.
    ///
    /// # Errors
    ///
    /// Errors are at absolute offsets: [`UnexpectedEof`] where the row is cut off,
    /// [`InvalidValue`] at an `id` of 0 and at a name offset inside the fixed part, and
    /// the string decoder's errors in the name.
    ///
    /// [`UnexpectedEof`]: crate::DecodeErrorKind::UnexpectedEof
    /// [`InvalidValue`]: crate::DecodeErrorKind::InvalidValue
    pub fn decode(row: &RawRow<'_>) -> Result<Self, DecodeError> {
        let mut r = row.reader();
        let row_at = r.position();
        let subtype = r.u16_le().context("pdb artist subtype")?;
        let index_shift = r.u16_le().context("pdb artist index_shift")?;
        let id = read_id(&mut r, "pdb artist id", ArtistId::new)?;
        let tail = NameTail::decode(&mut r, row_at, layout(subtype), &CONTEXTS)?;
        Ok(Self {
            subtype,
            index_shift,
            id,
            tail,
        })
    }

    /// Encodes the row: the row's bytes.
    ///
    /// # Errors
    ///
    /// - [`EncodeError::LengthOverflow`] when the name offset does not fit its field (a
    ///   byte when near), or the name is too long for its form;
    /// - [`EncodeError::ValueOutOfRange`] when `unknown1` does not fit a byte when near.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut w = Writer::new();
        w.u16_le(self.subtype);
        w.u16_le(self.index_shift);
        w.u32_le(self.id.get());
        self.tail.encode(&mut w, layout(self.subtype), &CONTEXTS)?;
        Ok(w.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{AT, raw};
    use super::*;
    use crate::DecodeErrorKind;

    fn artist_id(id: u32) -> ArtistId {
        ArtistId::new(id).unwrap()
    }

    /// Near artist 5, "Abba", at `index_shift` 0x20, then a byte of the next row.
    const ABBA: [u8; 16] = [
        0x60, 0, 0x20, 0, 5, 0, 0, 0, 3, 10, 0x0b, b'A', b'b', b'b', b'a', 0xee,
    ];

    /// Near artist 5, "Ö" in UTF-16LE at offset 12, as rekordbox lays it out.
    const UTF16: [u8; 18] = [
        0x60, 0, 0, 0, 5, 0, 0, 0, 3, 12, 0, 0, 0x90, 6, 0, 0, 0xd6, 0,
    ];

    /// Far artist 5, "Abba".
    const FAR: [u8; 17] = [
        0x64, 0, 0, 0, 5, 0, 0, 0, 3, 0, 12, 0, 0x0b, b'A', b'b', b'b', b'a',
    ];

    fn decode(bytes: &[u8]) -> Artist {
        Artist::decode(&raw(bytes)).unwrap()
    }

    fn err(bytes: &[u8]) -> (u64, &'static str, DecodeErrorKind) {
        let e = Artist::decode(&raw(bytes)).unwrap_err();
        (e.offset, e.context, e.kind)
    }

    #[test]
    fn near_artist_decodes_and_re_encodes() {
        let artist = decode(&ABBA);
        assert_eq!(artist.subtype(), 0x60);
        assert!(!artist.is_far());
        assert_eq!(artist.index_shift(), 0x20);
        assert_eq!(artist.id(), artist_id(5));
        assert_eq!(artist.unknown1(), 3);
        assert_eq!(artist.name_gap(), []);
        assert_eq!(artist.name(), &DeviceSqlString::new("Abba"));
        assert_eq!(artist.encode().unwrap(), ABBA[..15]);
    }

    #[test]
    fn long_form_name_after_a_gap_round_trips() {
        let artist = decode(&UTF16);
        assert_eq!(artist.name().as_str(), "Ö");
        assert_eq!(artist.name_gap(), [0, 0]);
        assert_eq!(artist.encode().unwrap(), UTF16);
    }

    #[test]
    fn far_artist_decodes_and_re_encodes() {
        let artist = decode(&FAR);
        assert!(artist.is_far());
        assert_eq!(artist.unknown1(), 3);
        assert_eq!(artist.name_gap(), []);
        assert_eq!(artist.name().as_str(), "Abba");
        assert_eq!(artist.encode().unwrap(), FAR);
    }

    #[test]
    fn unknown_subtype_reads_near_and_round_trips() {
        let mut bytes = ABBA;
        bytes[0] = 0x61;
        bytes[1] = 0x99;
        let artist = decode(&bytes);
        assert_eq!(artist.subtype(), 0x9961);
        assert!(!artist.is_far());
        assert_eq!(artist.encode().unwrap(), bytes[..15]);
    }

    #[test]
    fn non_zero_gap_bytes_round_trip() {
        let mut bytes = ABBA[..9].to_vec();
        bytes.extend_from_slice(&[13, 0xab, 0xcd, 0xef, 0x03]);
        let artist = decode(&bytes);
        assert_eq!(artist.name_gap(), [0xab, 0xcd, 0xef]);
        assert_eq!(artist.name().as_str(), "");
        assert_eq!(artist.encode().unwrap(), bytes);
        let mut far = FAR[..10].to_vec();
        far.extend_from_slice(&[13, 0, 0x5a, 0x03]);
        let artist = decode(&far);
        assert_eq!(artist.name_gap(), [0x5a]);
        assert_eq!(artist.encode().unwrap(), far);
    }

    #[test]
    fn builder_gives_rekordbox_layout() {
        let mut expected = ABBA[..15].to_vec();
        expected[2] = 0;
        assert_eq!(
            Artist::new(artist_id(5), "Abba").encode().unwrap(),
            expected
        );
        assert_eq!(Artist::new(artist_id(5), "Ö").encode().unwrap(), UTF16);
        let long = "a".repeat(127);
        let built = Artist::new(artist_id(5), &long);
        assert_eq!(built.name_gap(), [0, 0]);
        assert!(!built.is_far());
    }

    #[test]
    fn name_offset_inside_the_fixed_part_is_invalid_at_the_offset() {
        let mut bytes = ABBA;
        bytes[9] = 9;
        assert_eq!(
            err(&bytes),
            (
                AT + 9,
                "pdb artist name offset",
                DecodeErrorKind::InvalidValue
            )
        );
        let mut far = FAR;
        far[10] = 11;
        assert_eq!(
            err(&far),
            (
                AT + 10,
                "pdb artist name offset",
                DecodeErrorKind::InvalidValue
            )
        );
    }

    #[test]
    fn truncated_artist_is_eof_at_the_field() {
        let eof = |needed, available| DecodeErrorKind::UnexpectedEof { needed, available };
        assert_eq!(
            err(&ABBA[..3]),
            (AT + 2, "pdb artist index_shift", eof(2, 1))
        );
        assert_eq!(
            err(&ABBA[..9]),
            (AT + 9, "pdb artist name offset", eof(1, 0))
        );
        assert_eq!(
            err(&FAR[..11]),
            (AT + 10, "pdb artist name offset", eof(2, 1))
        );
        let mut bytes = ABBA;
        bytes[9] = 200;
        assert_eq!(err(&bytes), (AT + 10, "pdb artist name gap", eof(190, 6)));
        assert_eq!(err(&ABBA[..12]).0, AT + 11);
    }

    #[test]
    fn id_zero_is_invalid_at_the_id() {
        let mut bytes = ABBA;
        bytes[4] = 0;
        assert_eq!(
            err(&bytes),
            (AT + 4, "pdb artist id", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn values_that_do_not_fit_the_near_layout_fail_to_encode() {
        let artist = Artist::new(artist_id(5), "x").with_unknown1(0x100);
        assert_eq!(
            artist.encode(),
            Err(EncodeError::ValueOutOfRange {
                context: "pdb artist unknown1"
            })
        );
        let artist = Artist::new(artist_id(5), "x").with_name_gap(vec![0; 246]);
        assert_eq!(
            artist.encode(),
            Err(EncodeError::LengthOverflow {
                context: "pdb artist name offset"
            })
        );
        let artist = Artist::new(artist_id(5), "x").with_name_gap(vec![0; 245]);
        assert_eq!(artist.encode().unwrap()[9], 255);
    }
}
