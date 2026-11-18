//! Album rows (table 3).

use super::super::defaults;
use super::super::{AlbumId, ArtistId, RawRow};
use super::{Layout, NameTail, TailContexts, read_id};
use crate::bytes::Writer;
use crate::encoding::DeviceSqlString;
use crate::error::{DecodeContext, DecodeError, EncodeError};

/// `subtype` of the far layout.
const SUBTYPE_FAR: u16 = 0x84;

/// The layout `subtype` chooses: far exactly for [`SUBTYPE_FAR`] (ADR-007).
fn layout(subtype: u16) -> Layout {
    if subtype == SUBTYPE_FAR {
        Layout::Far
    } else {
        Layout::Near
    }
}
/// Row offset of the marker field, `unknown3`.
const MARKER_AT: usize = 0x14;

const CONTEXTS: TailContexts = TailContexts {
    marker: "pdb album unknown3",
    offset: "pdb album name offset",
    gap: "pdb album name gap",
};

/// An album row (table 3).
///
/// | bytes        | field         | meaning                                             |
/// |--------------|---------------|-----------------------------------------------------|
/// | 0..2         | `subtype`     | 0x80 near, 0x84 far; any other value reads as near  |
/// | 2..4         | `index_shift` | djl-analysis' name; 0x20 × the row's slot in its page's row index |
/// | 4..8         | `unknown1`    | not understood; 0 in real files                     |
/// | 8..0x0c      | `artist_id`   | the album artist, 0 for none                        |
/// | 0x0c..0x10   | `id`          | [`AlbumId`], never 0                                |
/// | 0x10..0x14   | `unknown2`    | not understood; 0 in real files                     |
/// | near: 0x14   | `unknown3`    | not understood; 3 in real files                     |
/// | near: 0x15   | name offset   | `u8`, from the row start                            |
/// | far: 0x14..0x16 | `unknown3` | as near, but a `u16`                                |
/// | far: 0x16..0x18 | name offset | `u16`, from the row start                          |
/// | ..name       | name gap      | bytes between the fixed part and the name, verbatim |
/// | name..       | `name`        | DeviceSQL string                                    |
///
/// The name offset is not stored: it is the fixed part's length plus the gap's. The row
/// ends after its name. Rekordbox writes every row near (all 239 on the real stick), with
/// the name right after the fixed part, or, for a long-form name (UTF-16LE or long ASCII),
/// at offset 0x18, the next multiple of 4, after two zero gap bytes. `index_shift` is
/// 0x20 × the row's slot in its page's row index, absent rows included, on every real row.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Album {
    subtype: u16,
    index_shift: u16,
    unknown1: u32,
    artist_id: Option<ArtistId>,
    id: AlbumId,
    unknown2: u32,
    tail: NameTail,
}

impl Album {
    /// Creates an album row as rekordbox does: near, `index_shift` 0 (the page writer sets
    /// it), rekordbox's unknowns, and the name in rekordbox's form and place.
    pub fn new(id: AlbumId, name: &str, artist_id: Option<ArtistId>) -> Self {
        Self {
            subtype: defaults::ALBUM_SUBTYPE,
            index_shift: 0,
            unknown1: defaults::ALBUM_UNKNOWN1,
            artist_id,
            id,
            unknown2: defaults::ALBUM_UNKNOWN2,
            tail: NameTail::built(defaults::ALBUM_UNKNOWN3, MARKER_AT, name),
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

    /// Replaces `index_shift` (offset 2).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the page writer sets it in slice 7")
    )]
    pub(crate) fn with_index_shift(mut self, index_shift: u16) -> Self {
        self.index_shift = index_shift;
        self
    }

    /// Replaces `unknown1` (offset 4).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_unknown1(mut self, unknown1: u32) -> Self {
        self.unknown1 = unknown1;
        self
    }

    /// Replaces `unknown2` (offset 0x10).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_unknown2(mut self, unknown2: u32) -> Self {
        self.unknown2 = unknown2;
        self
    }

    /// Replaces `unknown3` (offset 0x14).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_unknown3(mut self, unknown3: u16) -> Self {
        self.tail = self.tail.with_marker(unknown3);
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
    pub fn id(&self) -> AlbumId {
        self.id
    }

    /// The name, with its stored form.
    pub fn name(&self) -> &DeviceSqlString {
        self.tail.name()
    }

    /// The album artist, or `None` when the field is 0.
    pub fn artist_id(&self) -> Option<ArtistId> {
        self.artist_id
    }

    /// The `subtype` field (offset 0), as stored.
    pub fn subtype(&self) -> u16 {
        self.subtype
    }

    /// Whether the row uses the far layout, with a `u16` name offset: `subtype` 0x84.
    pub fn is_far(&self) -> bool {
        layout(self.subtype) == Layout::Far
    }

    /// `index_shift` (offset 2), as stored.
    pub fn index_shift(&self) -> u16 {
        self.index_shift
    }

    /// `unknown1` (offset 4), not understood.
    pub fn unknown1(&self) -> u32 {
        self.unknown1
    }

    /// `unknown2` (offset 0x10), not understood.
    pub fn unknown2(&self) -> u32 {
        self.unknown2
    }

    /// `unknown3` (offset 0x14), not understood: a byte when near, a `u16` when far.
    pub fn unknown3(&self) -> u16 {
        self.tail.marker()
    }

    /// The bytes between the fixed part and the name; empty or zero in rekordbox's rows.
    pub fn name_gap(&self) -> &[u8] {
        self.tail.gap()
    }

    /// Decodes an album row.
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
        let subtype = r.u16_le().context("pdb album subtype")?;
        let index_shift = r.u16_le().context("pdb album index_shift")?;
        let unknown1 = r.u32_le().context("pdb album unknown1")?;
        let artist_id = ArtistId::new(r.u32_le().context("pdb album artist_id")?);
        let id = read_id(&mut r, "pdb album id", AlbumId::new)?;
        let unknown2 = r.u32_le().context("pdb album unknown2")?;
        let tail = NameTail::decode(&mut r, row_at, layout(subtype), &CONTEXTS)?;
        Ok(Self {
            subtype,
            index_shift,
            unknown1,
            artist_id,
            id,
            unknown2,
            tail,
        })
    }

    /// Encodes the row: the row's bytes.
    ///
    /// # Errors
    ///
    /// - [`EncodeError::LengthOverflow`] when the name offset does not fit its field (a
    ///   byte when near), or the name is too long for its form;
    /// - [`EncodeError::ValueOutOfRange`] when `unknown3` does not fit a byte when near.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut w = Writer::new();
        w.u16_le(self.subtype);
        w.u16_le(self.index_shift);
        w.u32_le(self.unknown1);
        w.u32_le(self.artist_id.map_or(0, ArtistId::get));
        w.u32_le(self.id.get());
        w.u32_le(self.unknown2);
        self.tail.encode(&mut w, layout(self.subtype), &CONTEXTS)?;
        Ok(w.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{AT, raw};
    use super::*;
    use crate::DecodeErrorKind;

    fn album_id(id: u32) -> AlbumId {
        AlbumId::new(id).unwrap()
    }

    /// The fixed part of near album 9 by artist 5, at `index_shift` 0x40, without the
    /// name offset.
    const FIXED: [u8; 0x15] = [
        0x80, 0, 0x40, 0, 0, 0, 0, 0, 5, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 3,
    ];

    /// Near album 9, "Arrival", by artist 5, then a byte of the next row.
    fn arrival() -> Vec<u8> {
        let mut bytes = FIXED.to_vec();
        bytes.push(0x16);
        bytes.extend_from_slice(b"\x11Arrival\xee");
        bytes
    }

    fn decode(bytes: &[u8]) -> Album {
        Album::decode(&raw(bytes)).unwrap()
    }

    fn err(bytes: &[u8]) -> (u64, &'static str, DecodeErrorKind) {
        let e = Album::decode(&raw(bytes)).unwrap_err();
        (e.offset, e.context, e.kind)
    }

    #[test]
    fn near_album_decodes_and_re_encodes() {
        let bytes = arrival();
        let album = decode(&bytes);
        assert_eq!(album.subtype(), 0x80);
        assert!(!album.is_far());
        assert_eq!(album.index_shift(), 0x40);
        assert_eq!(album.unknown1(), 0);
        assert_eq!(album.artist_id(), ArtistId::new(5));
        assert_eq!(album.id(), album_id(9));
        assert_eq!(album.unknown2(), 0);
        assert_eq!(album.unknown3(), 3);
        assert_eq!(album.name_gap(), []);
        assert_eq!(album.name(), &DeviceSqlString::new("Arrival"));
        assert_eq!(album.encode().unwrap(), bytes[..bytes.len() - 1]);
    }

    #[test]
    fn artist_id_zero_is_none() {
        let mut bytes = arrival();
        bytes[8] = 0;
        let album = decode(&bytes);
        assert_eq!(album.artist_id(), None);
        assert_eq!(album.encode().unwrap(), bytes[..bytes.len() - 1]);
    }

    #[test]
    fn far_album_with_a_gap_and_unknowns_round_trips() {
        let mut bytes = FIXED[..0x14].to_vec();
        bytes[0] = 0x84;
        bytes[4] = 0x11;
        bytes[0x10] = 0x22;
        bytes.extend_from_slice(&[3, 1, 0x1a, 0, 0xab, 0xcd, 0x03]);
        let album = decode(&bytes);
        assert!(album.is_far());
        assert_eq!(album.unknown3(), 0x103);
        assert_eq!((album.unknown1(), album.unknown2()), (0x11, 0x22));
        assert_eq!(album.name_gap(), [0xab, 0xcd]);
        assert_eq!(album.encode().unwrap(), bytes);
    }

    #[test]
    fn unknown_subtype_reads_near_and_round_trips() {
        let mut bytes = arrival();
        bytes[0] = 0x85;
        let album = decode(&bytes);
        assert!(!album.is_far());
        assert_eq!(album.encode().unwrap(), bytes[..bytes.len() - 1]);
    }

    #[test]
    fn builder_gives_rekordbox_layout() {
        let mut expected = arrival();
        expected.pop();
        expected[2] = 0;
        let built = Album::new(album_id(9), "Arrival", ArtistId::new(5));
        assert_eq!(built.encode().unwrap(), expected);
        // A UTF-16LE name starts at 0x18, the next multiple of 4.
        let built = Album::new(album_id(9), "Ö", None).encode().unwrap();
        assert_eq!(built[0x15], 0x18);
        assert_eq!(built[0x16..], [0, 0, 0x90, 6, 0, 0, 0xd6, 0]);
        assert_eq!(built[8..12], [0; 4]);
    }

    #[test]
    fn decode_errors_are_at_the_field() {
        let mut bytes = arrival();
        bytes[0x0c] = 0;
        assert_eq!(
            err(&bytes),
            (AT + 0x0c, "pdb album id", DecodeErrorKind::InvalidValue)
        );
        let mut bytes = arrival();
        bytes[0x15] = 0x15;
        assert_eq!(
            err(&bytes),
            (
                AT + 0x15,
                "pdb album name offset",
                DecodeErrorKind::InvalidValue
            )
        );
        let eof = |needed, available| DecodeErrorKind::UnexpectedEof { needed, available };
        assert_eq!(err(&FIXED[..6]), (AT + 4, "pdb album unknown1", eof(4, 2)));
        assert_eq!(err(&FIXED[..9]), (AT + 8, "pdb album artist_id", eof(4, 1)));
        assert_eq!(
            err(&FIXED[..0x12]),
            (AT + 0x10, "pdb album unknown2", eof(4, 2))
        );
        assert_eq!(err(&FIXED), (AT + 0x15, "pdb album name offset", eof(1, 0)));
        let mut bytes = arrival();
        bytes[0x15] = 0x30;
        assert_eq!(err(&bytes).1, "pdb album name gap");
    }

    #[test]
    fn values_that_do_not_fit_the_near_layout_fail_to_encode() {
        let album = Album::new(album_id(9), "x", None).with_unknown3(0x100);
        assert_eq!(
            album.encode(),
            Err(EncodeError::ValueOutOfRange {
                context: "pdb album unknown3"
            })
        );
        let album = Album::new(album_id(9), "x", None).with_name_gap(vec![0; 0xea]);
        assert_eq!(
            album.encode(),
            Err(EncodeError::LengthOverflow {
                context: "pdb album name offset"
            })
        );
        let far = Album::new(album_id(9), "x", None)
            .with_subtype(0x84)
            .with_unknown3(0x100)
            .with_name_gap(vec![0; 0xea]);
        assert_eq!(decode(&far.encode().unwrap()), far);
    }
}
