//! Rows whose only string follows the fixed part directly: genres, labels, keys, colours
//! and artwork.

use super::super::defaults;
use super::super::{ArtworkId, ColorId, GenreId, KeyId, LabelId, RawRow};
use super::read_id;
use crate::bytes::Writer;
use crate::encoding::DeviceSqlString;
use crate::encoding::devicesql;
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Defines a row of an id and one string: `$id` at 0..4, the string `$field` from 4.
macro_rules! id_string_row {
    (
        $(#[$doc:meta])*
        $name:ident($id:ident, $field:ident, $id_context:literal),
        $field_doc:literal
    ) => {
        $(#[$doc])*
        ///
        /// The row ends after its string.
        #[derive(Debug, Clone, PartialEq, Eq)]
        #[non_exhaustive]
        pub struct $name {
            id: $id,
            $field: DeviceSqlString,
        }

        impl $name {
            #[doc = concat!("Creates a row; the ", $field_doc, " gets rekordbox's string form.")]
            pub fn new(id: $id, $field: &str) -> Self {
                Self {
                    id,
                    $field: DeviceSqlString::new($field),
                }
            }

            /// The row's id.
            pub fn id(&self) -> $id {
                self.id
            }

            #[doc = concat!("The ", $field_doc, ", with its stored form.")]
            pub fn $field(&self) -> &DeviceSqlString {
                &self.$field
            }

            /// Decodes the row.
            ///
            /// # Errors
            ///
            /// Errors are at absolute offsets: [`UnexpectedEof`] where the row is cut off,
            /// [`InvalidValue`] at an `id` of 0, and the string decoder's errors.
            ///
            /// [`UnexpectedEof`]: crate::DecodeErrorKind::UnexpectedEof
            /// [`InvalidValue`]: crate::DecodeErrorKind::InvalidValue
            pub fn decode(row: &RawRow<'_>) -> Result<Self, DecodeError> {
                let mut r = row.reader();
                let id = read_id(&mut r, $id_context, $id::new)?;
                let $field = devicesql::decode(&mut r)?;
                Ok(Self { id, $field })
            }

            /// Encodes the row: the row's bytes.
            ///
            /// # Errors
            ///
            /// [`EncodeError::LengthOverflow`] for a string too long for its form.
            pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
                let mut w = Writer::new();
                w.u32_le(self.id.get());
                self.$field.encode(&mut w)?;
                Ok(w.into_inner())
            }
        }
    };
}

id_string_row! {
    /// A genre row (table 1).
    ///
    /// | bytes | field  | meaning              |
    /// |-------|--------|----------------------|
    /// | 0..4  | `id`   | [`GenreId`], never 0 |
    /// | 4..   | `name` | DeviceSQL string     |
    Genre(GenreId, name, "pdb genre id"),
    "name"
}

id_string_row! {
    /// A label row (table 4).
    ///
    /// | bytes | field  | meaning              |
    /// |-------|--------|----------------------|
    /// | 0..4  | `id`   | [`LabelId`], never 0 |
    /// | 4..   | `name` | DeviceSQL string     |
    Label(LabelId, name, "pdb label id"),
    "name"
}

id_string_row! {
    /// An artwork row (table 13): where an album cover is on the stick.
    ///
    /// | bytes | field  | meaning                                                  |
    /// |-------|--------|----------------------------------------------------------|
    /// | 0..4  | `id`   | [`ArtworkId`], never 0                                   |
    /// | 4..   | `path` | DeviceSQL string: the 80×80 image; the 240×240 one has `_m` before the extension |
    Artwork(ArtworkId, path, "pdb artwork id"),
    "path"
}

/// A musical key row (table 5).
///
/// | bytes | field  | meaning                                                     |
/// |-------|--------|-------------------------------------------------------------|
/// | 0..4  | `id`   | [`KeyId`], never 0                                          |
/// | 4..8  | `id2`  | djl-analysis: a copy of the id; stored, not asserted        |
/// | 8..   | `name` | DeviceSQL string, e.g. `"8A"` or `"Am"`                     |
///
/// The row ends after its name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Key {
    id: KeyId,
    id2: u32,
    name: DeviceSqlString,
}

impl Key {
    /// Creates a key row as rekordbox does: `id2` is the id, and the name gets rekordbox's
    /// string form.
    pub fn new(id: KeyId, name: &str) -> Self {
        Self {
            id,
            id2: id.get(),
            name: DeviceSqlString::new(name),
        }
    }

    /// Replaces `id2` (offset 4).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_id2(mut self, id2: u32) -> Self {
        self.id2 = id2;
        self
    }

    /// The row's id.
    pub fn id(&self) -> KeyId {
        self.id
    }

    /// `id2` (offset 4), as stored. Equal to the id in 42 of the real stick's 43 rows.
    pub fn id2(&self) -> u32 {
        self.id2
    }

    /// The name, with its stored form.
    pub fn name(&self) -> &DeviceSqlString {
        &self.name
    }

    /// Decodes a key row.
    ///
    /// # Errors
    ///
    /// [`UnexpectedEof`](crate::DecodeErrorKind::UnexpectedEof) where the row is cut off,
    /// [`InvalidValue`](crate::DecodeErrorKind::InvalidValue) at an `id` of 0, and the
    /// string decoder's errors, all at absolute offsets.
    pub fn decode(row: &RawRow<'_>) -> Result<Self, DecodeError> {
        let mut r = row.reader();
        let id = read_id(&mut r, "pdb key id", KeyId::new)?;
        let id2 = r.u32_le().context("pdb key id2")?;
        let name = devicesql::decode(&mut r)?;
        Ok(Self { id, id2, name })
    }

    /// Encodes the row: the row's bytes.
    ///
    /// # Errors
    ///
    /// [`EncodeError::LengthOverflow`] for a name too long for its form.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut w = Writer::new();
        w.u32_le(self.id.get());
        w.u32_le(self.id2);
        self.name.encode(&mut w)?;
        Ok(w.into_inner())
    }
}

/// A colour label row (table 6).
///
/// | bytes | field      | meaning                                                    |
/// |-------|------------|------------------------------------------------------------|
/// | 0..4  | `unknown1` | not understood; 0 in real files                            |
/// | 4     | `unknown2` | not understood; equal to the id in real files              |
/// | 5..7  | `id`       | [`ColorId`]: 1 pink to 8 purple, never 0                   |
/// | 7     | `unknown3` | not understood; 0 in real files                            |
/// | 8..   | `name`     | DeviceSQL string                                           |
///
/// The offsets are djl-analysis'; all 8 rows of the real stick agree with them. The row
/// ends after its name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Color {
    unknown1: u32,
    unknown2: u8,
    id: ColorId,
    unknown3: u8,
    name: DeviceSqlString,
}

impl Color {
    /// Creates a colour row as rekordbox does: `unknown2` is the id (0 if the id does not
    /// fit a byte, which real files never need), the other unknowns are rekordbox's
    /// defaults, and the name gets rekordbox's string form.
    pub fn new(id: ColorId, name: &str) -> Self {
        Self {
            unknown1: defaults::COLOR_UNKNOWN1,
            unknown2: u8::try_from(id.get()).unwrap_or(0),
            id,
            unknown3: defaults::COLOR_UNKNOWN3,
            name: DeviceSqlString::new(name),
        }
    }

    /// Replaces `unknown1` (offset 0).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_unknown1(mut self, unknown1: u32) -> Self {
        self.unknown1 = unknown1;
        self
    }

    /// Replaces `unknown2` (offset 4).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_unknown2(mut self, unknown2: u8) -> Self {
        self.unknown2 = unknown2;
        self
    }

    /// Replaces `unknown3` (offset 7).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_unknown3(mut self, unknown3: u8) -> Self {
        self.unknown3 = unknown3;
        self
    }

    /// The row's id.
    pub fn id(&self) -> ColorId {
        self.id
    }

    /// The name, with its stored form.
    pub fn name(&self) -> &DeviceSqlString {
        &self.name
    }

    /// `unknown1` (offset 0), not understood.
    pub fn unknown1(&self) -> u32 {
        self.unknown1
    }

    /// `unknown2` (offset 4), not understood.
    pub fn unknown2(&self) -> u8 {
        self.unknown2
    }

    /// `unknown3` (offset 7), not understood.
    pub fn unknown3(&self) -> u8 {
        self.unknown3
    }

    /// Decodes a colour row.
    ///
    /// # Errors
    ///
    /// [`UnexpectedEof`](crate::DecodeErrorKind::UnexpectedEof) where the row is cut off,
    /// [`InvalidValue`](crate::DecodeErrorKind::InvalidValue) at an `id` of 0, and the
    /// string decoder's errors, all at absolute offsets.
    pub fn decode(row: &RawRow<'_>) -> Result<Self, DecodeError> {
        const ID: &str = "pdb color id";
        let mut r = row.reader();
        let unknown1 = r.u32_le().context("pdb color unknown1")?;
        let unknown2 = r.u8().context("pdb color unknown2")?;
        // Like `read_id`, but the colour id is a `u16`; one call site doesn't justify a
        // generic reader.
        let id_at = r.position();
        let id = ColorId::new(r.u16_le().context(ID)?)
            .ok_or_else(|| DecodeError::new(id_at, ID, DecodeErrorKind::InvalidValue))?;
        let unknown3 = r.u8().context("pdb color unknown3")?;
        let name = devicesql::decode(&mut r)?;
        Ok(Self {
            unknown1,
            unknown2,
            id,
            unknown3,
            name,
        })
    }

    /// Encodes the row: the row's bytes.
    ///
    /// # Errors
    ///
    /// [`EncodeError::LengthOverflow`] for a name too long for its form.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut w = Writer::new();
        w.u32_le(self.unknown1);
        w.u8(self.unknown2);
        w.u16_le(self.id.get());
        w.u8(self.unknown3);
        self.name.encode(&mut w)?;
        Ok(w.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{AT, raw};
    use super::*;

    fn genre_id(id: u32) -> GenreId {
        GenreId::new(id).unwrap()
    }

    /// Genre 7, "House", followed by a byte of the next row.
    const HOUSE: [u8; 11] = [7, 0, 0, 0, 0x0d, b'H', b'o', b'u', b's', b'e', 0xee];

    fn located(e: DecodeError) -> (u64, &'static str, DecodeErrorKind) {
        (e.offset, e.context, e.kind)
    }

    fn err(bytes: &[u8]) -> (u64, &'static str, DecodeErrorKind) {
        located(Genre::decode(&raw(bytes)).unwrap_err())
    }

    #[test]
    fn genre_decodes_and_re_encodes_its_bytes() {
        let genre = Genre::decode(&raw(&HOUSE)).unwrap();
        assert_eq!(genre.id(), genre_id(7));
        assert_eq!(genre.name(), &DeviceSqlString::new("House"));
        assert_eq!(genre.encode().unwrap(), HOUSE[..10]);
    }

    #[test]
    fn genre_builder_gives_rekordbox_layout() {
        let genre = Genre::new(genre_id(7), "House");
        assert_eq!(genre.encode().unwrap(), HOUSE[..10]);
    }

    #[test]
    fn genre_id_zero_is_invalid_at_the_id() {
        let mut bytes = HOUSE;
        bytes[0] = 0;
        assert_eq!(
            err(&bytes),
            (AT, "pdb genre id", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn label_keeps_a_utf16_name() {
        // Label 3, "Ö", as UTF-16LE with pad byte 9.
        let bytes = [3, 0, 0, 0, 0x90, 6, 0, 9, 0xd6, 0];
        let label = Label::decode(&raw(&bytes)).unwrap();
        assert_eq!(label.id(), LabelId::new(3).unwrap());
        assert_eq!(label.name().as_str(), "Ö");
        assert_eq!(label.encode().unwrap(), bytes);
        let built = Label::new(LabelId::new(3).unwrap(), "Ö").encode().unwrap();
        assert_eq!(built, [3, 0, 0, 0, 0x90, 6, 0, 0, 0xd6, 0]);
        let e = Label::decode(&raw(&[0, 0, 0, 0, 3])).unwrap_err();
        assert_eq!(
            located(e),
            (AT, "pdb label id", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn artwork_holds_a_path() {
        let path = "/PIONEER/Artwork/00001/a1.jpg";
        let mut bytes = vec![0x0d, 0x01, 0, 0];
        bytes.push(u8::try_from(((path.len() + 1) << 1) | 1).unwrap());
        bytes.extend_from_slice(path.as_bytes());
        let artwork = Artwork::decode(&raw(&bytes)).unwrap();
        assert_eq!(artwork.id(), ArtworkId::new(0x10d).unwrap());
        assert_eq!(artwork.path().as_str(), path);
        assert_eq!(artwork.encode().unwrap(), bytes);
        let built = Artwork::new(ArtworkId::new(0x10d).unwrap(), path);
        assert_eq!(built.encode().unwrap(), bytes);
        let e = Artwork::decode(&raw(&[0, 0, 0, 0, 3])).unwrap_err();
        assert_eq!(
            located(e),
            (AT, "pdb artwork id", DecodeErrorKind::InvalidValue)
        );
    }

    /// Key 44 with `id2` 43, "2B": the one real-stick key whose `id2` is not its id.
    const KEY_2B: [u8; 11] = [0x2c, 0, 0, 0, 0x2b, 0, 0, 0, 0x07, b'2', b'B'];

    #[test]
    fn key_keeps_an_id2_that_differs_from_its_id() {
        let key = Key::decode(&raw(&KEY_2B)).unwrap();
        assert_eq!(key.id(), KeyId::new(44).unwrap());
        assert_eq!(key.id2(), 43);
        assert_eq!(key.name().as_str(), "2B");
        assert_eq!(key.encode().unwrap(), KEY_2B);
        assert_eq!(Key::new(KeyId::new(44).unwrap(), "2B").with_id2(43), key);
    }

    #[test]
    fn key_builder_copies_the_id_into_id2() {
        let key = Key::new(KeyId::new(44).unwrap(), "2B");
        assert_eq!(key.id2(), 44);
        let mut expected = KEY_2B;
        expected[4] = 0x2c;
        assert_eq!(key.encode().unwrap(), expected);
    }

    #[test]
    fn key_errors_are_at_the_field() {
        let mut bytes = KEY_2B;
        bytes[0] = 0;
        let e = Key::decode(&raw(&bytes)).unwrap_err();
        assert_eq!(
            located(e),
            (AT, "pdb key id", DecodeErrorKind::InvalidValue)
        );
        let e = Key::decode(&raw(&KEY_2B[..6])).unwrap_err();
        assert_eq!(
            located(e),
            (
                AT + 4,
                "pdb key id2",
                DecodeErrorKind::UnexpectedEof {
                    needed: 4,
                    available: 2
                }
            )
        );
        let e = Key::decode(&raw(&KEY_2B[..8])).unwrap_err();
        assert_eq!(e.offset, AT + 8);
    }

    /// Colour 1, "Pink", as rekordbox writes it.
    const PINK: [u8; 13] = [0, 0, 0, 0, 1, 1, 0, 0, 0x0b, b'P', b'i', b'n', b'k'];

    #[test]
    fn color_decodes_the_real_layout() {
        let color = Color::decode(&raw(&PINK)).unwrap();
        assert_eq!(color.id(), ColorId::new(1).unwrap());
        assert_eq!(color.name().as_str(), "Pink");
        assert_eq!(
            (color.unknown1(), color.unknown2(), color.unknown3()),
            (0, 1, 0)
        );
        assert_eq!(color.encode().unwrap(), PINK);
    }

    #[test]
    fn color_unknowns_round_trip() {
        let bytes = [1, 2, 3, 4, 9, 2, 1, 7, 0x03];
        let color = Color::decode(&raw(&bytes)).unwrap();
        assert_eq!(color.id(), ColorId::new(0x102).unwrap());
        assert_eq!(
            (color.unknown1(), color.unknown2(), color.unknown3()),
            (0x0403_0201, 9, 7)
        );
        assert_eq!(color.encode().unwrap(), bytes);
        let built = Color::new(ColorId::new(0x102).unwrap(), "")
            .with_unknown1(0x0403_0201)
            .with_unknown2(9)
            .with_unknown3(7);
        assert_eq!(built, color);
    }

    #[test]
    fn color_builder_gives_rekordbox_layout() {
        let built = Color::new(ColorId::new(1).unwrap(), "Pink");
        assert_eq!(built.encode().unwrap(), PINK);
        let wide = Color::new(ColorId::new(0x100).unwrap(), "");
        assert_eq!(wide.unknown2(), 0);
    }

    #[test]
    fn color_errors_are_at_the_field() {
        let mut bytes = PINK;
        bytes[5] = 0;
        let e = Color::decode(&raw(&bytes)).unwrap_err();
        assert_eq!(
            located(e),
            (AT + 5, "pdb color id", DecodeErrorKind::InvalidValue)
        );
        let e = Color::decode(&raw(&PINK[..6])).unwrap_err();
        assert_eq!(
            located(e),
            (
                AT + 5,
                "pdb color id",
                DecodeErrorKind::UnexpectedEof {
                    needed: 2,
                    available: 1
                }
            )
        );
        let e = Color::decode(&raw(&PINK[..7])).unwrap_err();
        assert_eq!((e.offset, e.context), (AT + 7, "pdb color unknown3"));
    }

    #[test]
    fn truncated_genre_is_eof_at_the_field() {
        assert_eq!(
            err(&HOUSE[..3]),
            (
                AT,
                "pdb genre id",
                DecodeErrorKind::UnexpectedEof {
                    needed: 4,
                    available: 3
                }
            )
        );
        assert_eq!(
            err(&HOUSE[..7]),
            (
                AT + 5,
                "DeviceSQL string text",
                DecodeErrorKind::UnexpectedEof {
                    needed: 5,
                    available: 2
                }
            )
        );
    }
}
