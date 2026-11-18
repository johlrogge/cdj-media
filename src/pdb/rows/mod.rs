//! Typed rows of the `export.pdb` track table and small tables (ADR-005, ADR-006,
//! ADR-007).

mod album;
mod artist;
mod inline;
mod tail;
mod track;

pub use album::Album;
pub use artist::Artist;
pub use inline::{Artwork, Color, Genre, Key, Label};
pub use track::{FileType, OtherFileType, Track, TrackFields, TrackString};

use super::{ExportTable, RawRow};
use crate::error::{DecodeError, EncodeError};
use tail::{Layout, NameTail, TailContexts, read_id};

/// A typed row of the track table or one of the small `export.pdb` tables.
///
/// Each variant's row ends after its last string; what follows it in the heap (zero
/// padding in rekordbox's files) belongs to the page, not to the row.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportRow {
    /// A row of [`ExportTable::Tracks`].
    Track(Track),
    /// A row of [`ExportTable::Genres`].
    Genre(Genre),
    /// A row of [`ExportTable::Artists`].
    Artist(Artist),
    /// A row of [`ExportTable::Albums`].
    Album(Album),
    /// A row of [`ExportTable::Labels`].
    Label(Label),
    /// A row of [`ExportTable::Keys`].
    Key(Key),
    /// A row of [`ExportTable::Colors`].
    Color(Color),
    /// A row of [`ExportTable::Artwork`].
    Artwork(Artwork),
}

impl ExportRow {
    /// Decodes `row` as a row of `table`; `None` for tables whose rows are not typed yet
    /// (playlists, history and the unnamed tables).
    ///
    /// # Errors
    ///
    /// The errors of the table's row type, at absolute offsets.
    pub fn decode(table: ExportTable, row: &RawRow<'_>) -> Result<Option<Self>, DecodeError> {
        Ok(Some(match table {
            ExportTable::Tracks => Self::Track(Track::decode(row)?),
            ExportTable::Genres => Self::Genre(Genre::decode(row)?),
            ExportTable::Artists => Self::Artist(Artist::decode(row)?),
            ExportTable::Albums => Self::Album(Album::decode(row)?),
            ExportTable::Labels => Self::Label(Label::decode(row)?),
            ExportTable::Keys => Self::Key(Key::decode(row)?),
            ExportTable::Colors => Self::Color(Color::decode(row)?),
            ExportTable::Artwork => Self::Artwork(Artwork::decode(row)?),
            ExportTable::PlaylistTree
            | ExportTable::PlaylistEntries
            | ExportTable::HistoryPlaylists
            | ExportTable::HistoryEntries
            | ExportTable::Columns
            | ExportTable::History
            | ExportTable::Unknown(_) => return Ok(None),
        }))
    }

    /// Encodes the row: the row's bytes.
    ///
    /// # Errors
    ///
    /// The errors of the row type's `encode`.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        match self {
            Self::Track(row) => row.encode(),
            Self::Genre(row) => row.encode(),
            Self::Artist(row) => row.encode(),
            Self::Album(row) => row.encode(),
            Self::Label(row) => row.encode(),
            Self::Key(row) => row.encode(),
            Self::Color(row) => row.encode(),
            Self::Artwork(row) => row.encode(),
        }
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::super::RawRow;

    /// Absolute file offset of every test row.
    pub(crate) const AT: u64 = 0x1234;

    /// A row over `bytes` at [`AT`].
    pub(crate) fn raw(bytes: &[u8]) -> RawRow<'_> {
        RawRow::new(0x10, bytes, AT)
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::{AT, raw};
    use super::*;
    use crate::pdb::{AlbumId, ArtistId, ArtworkId, ColorId, GenreId, KeyId, LabelId, TrackId};

    /// One built row of every typed table.
    fn built_rows() -> Vec<(ExportTable, ExportRow)> {
        let track = TrackFields {
            title: "Jóga".into(),
            isrc: "GBAYE9700001".into(),
            artist_id: ArtistId::new(2),
            album_id: AlbumId::new(3),
            ..TrackFields::default()
        };
        vec![
            (
                ExportTable::Tracks,
                ExportRow::Track(Track::new(TrackId::new(8).unwrap(), track).unwrap()),
            ),
            (
                ExportTable::Genres,
                ExportRow::Genre(Genre::new(GenreId::new(1).unwrap(), "House")),
            ),
            (
                ExportTable::Artists,
                ExportRow::Artist(Artist::new(ArtistId::new(2).unwrap(), "Björk")),
            ),
            (
                ExportTable::Albums,
                ExportRow::Album(Album::new(
                    AlbumId::new(3).unwrap(),
                    "Debut",
                    ArtistId::new(2),
                )),
            ),
            (
                ExportTable::Labels,
                ExportRow::Label(Label::new(LabelId::new(4).unwrap(), "One Little")),
            ),
            (
                ExportTable::Keys,
                ExportRow::Key(Key::new(KeyId::new(5).unwrap(), "8A")),
            ),
            (
                ExportTable::Colors,
                ExportRow::Color(Color::new(ColorId::new(6).unwrap(), "Aqua")),
            ),
            (
                ExportTable::Artwork,
                ExportRow::Artwork(Artwork::new(ArtworkId::new(7).unwrap(), "/a.jpg")),
            ),
        ]
    }

    #[test]
    fn decode_picks_the_row_type_of_the_table() {
        for (table, row) in built_rows() {
            let bytes = row.encode().unwrap();
            assert!(!bytes.is_empty(), "{table:?}");
            let decoded = ExportRow::decode(table, &raw(&bytes)).unwrap();
            assert_eq!(decoded, Some(row), "{table:?}");
        }
    }

    #[test]
    fn untyped_tables_decode_to_none() {
        for table in [
            ExportTable::PlaylistTree,
            ExportTable::History,
            ExportTable::from_u32(9),
        ] {
            assert_eq!(ExportRow::decode(table, &raw(&[0; 64])), Ok(None));
        }
    }

    #[test]
    fn decode_errors_come_from_the_row_type() {
        let e = ExportRow::decode(ExportTable::Labels, &raw(&[0, 0, 0, 0, 3])).unwrap_err();
        assert_eq!((e.offset, e.context), (AT, "pdb label id"));
        let e = ExportRow::decode(ExportTable::Tracks, &raw(&[0; 0x42])).unwrap_err();
        assert_eq!((e.offset, e.context), (AT + 0x40, "pdb track album_id"));
    }

    mod properties {
        use proptest::collection::vec;
        use proptest::prelude::*;

        use super::*;

        const TABLES: [ExportTable; 8] = [
            ExportTable::Tracks,
            ExportTable::Genres,
            ExportTable::Artists,
            ExportTable::Albums,
            ExportTable::Labels,
            ExportTable::Keys,
            ExportTable::Colors,
            ExportTable::Artwork,
        ];

        fn name() -> impl Strategy<Value = String> {
            prop_oneof![any::<String>(), "[ -~]{120,130}"]
        }

        fn id() -> impl Strategy<Value = u32> {
            1u32..
        }

        /// A subtype: near, far, or anything else.
        fn subtype(near: u16, far: u16) -> impl Strategy<Value = u16> {
            prop_oneof![Just(near), Just(far), any::<u16>()]
        }

        /// Any row a builder and the crate-private setters can make, with its table.
        fn row() -> impl Strategy<Value = (ExportTable, ExportRow)> {
            prop_oneof![
                (id(), name()).prop_map(|(id, name)| (
                    ExportTable::Genres,
                    ExportRow::Genre(Genre::new(GenreId::new(id).unwrap(), &name))
                )),
                (id(), name()).prop_map(|(id, name)| (
                    ExportTable::Labels,
                    ExportRow::Label(Label::new(LabelId::new(id).unwrap(), &name))
                )),
                (id(), name()).prop_map(|(id, name)| (
                    ExportTable::Artwork,
                    ExportRow::Artwork(Artwork::new(ArtworkId::new(id).unwrap(), &name))
                )),
                (id(), any::<u32>(), name()).prop_map(|(id, id2, name)| (
                    ExportTable::Keys,
                    ExportRow::Key(Key::new(KeyId::new(id).unwrap(), &name).with_id2(id2))
                )),
                (1u16.., any::<(u32, u8, u8)>(), name()).prop_map(|(id, unknowns, name)| (
                    ExportTable::Colors,
                    ExportRow::Color(
                        Color::new(ColorId::new(id).unwrap(), &name)
                            .with_unknown1(unknowns.0)
                            .with_unknown2(unknowns.1)
                            .with_unknown3(unknowns.2)
                    )
                )),
                (
                    id(),
                    name(),
                    subtype(0x60, 0x64),
                    any::<(u16, u8)>(),
                    vec(any::<u8>(), 0..8)
                )
                    .prop_map(|(id, name, subtype, (shift, marker), gap)| (
                        ExportTable::Artists,
                        ExportRow::Artist(
                            Artist::new(ArtistId::new(id).unwrap(), &name)
                                .with_subtype(subtype)
                                .with_index_shift(shift)
                                .with_unknown1(u16::from(marker))
                                .with_name_gap(gap)
                        )
                    )),
                (
                    (id(), any::<u32>(), name()),
                    subtype(0x80, 0x84),
                    any::<(u16, u32, u32, u8)>(),
                    vec(any::<u8>(), 0..8)
                )
                    .prop_map(|((id, artist, name), subtype, unknowns, gap)| (
                        ExportTable::Albums,
                        ExportRow::Album(
                            Album::new(AlbumId::new(id).unwrap(), &name, ArtistId::new(artist))
                                .with_subtype(subtype)
                                .with_index_shift(unknowns.0)
                                .with_unknown1(unknowns.1)
                                .with_unknown2(unknowns.2)
                                .with_unknown3(u16::from(unknowns.3))
                                .with_name_gap(gap)
                        )
                    )),
            ]
        }

        /// Decodes `bytes` as a row of every typed table; whatever decodes must re-encode to
        /// a prefix of `bytes`.
        fn check_any_bytes(bytes: &[u8]) -> Result<(), TestCaseError> {
            for table in TABLES {
                if let Ok(Some(row)) = ExportRow::decode(table, &raw(bytes)) {
                    let encoded = row.encode().unwrap();
                    prop_assert_eq!(&bytes[..encoded.len()], &encoded[..], "{:?}", table);
                }
            }
            Ok(())
        }

        proptest! {
            #[test]
            fn built_rows_encode_and_decode_to_themselves((table, row) in row()) {
                let bytes = row.encode().unwrap();
                let decoded = ExportRow::decode(table, &raw(&bytes)).unwrap();
                prop_assert_eq!(decoded, Some(row));
            }

            #[test]
            fn decoding_arbitrary_bytes_never_panics(bytes in vec(any::<u8>(), 0..64)) {
                check_any_bytes(&bytes)?;
            }

            #[test]
            fn decoding_plausible_rows_never_panics(
                subtype in prop_oneof![
                    Just(0x60u16), Just(0x64), Just(0x80), Just(0x84), any::<u16>()
                ],
                id in 0u32..4,
                offset in 0u8..0x30,
                flag in prop_oneof![Just(0x03u8), Just(0x09), Just(0x40), Just(0x90)],
                rest in vec(any::<u8>(), 0..48),
            ) {
                // Near and far artist and album fixed parts with small values, then
                // arbitrary bytes with a string flag where the name may start.
                let mut bytes = subtype.to_le_bytes().to_vec();
                bytes.extend_from_slice(&[0, 0]);
                for value in [id, id, id, 0] {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
                bytes[8] = 3;
                bytes[9] = offset;
                bytes[0x0a] = offset;
                bytes.extend_from_slice(&[3, offset, offset, 0]);
                bytes.extend(rest);
                if let Some(slot) = bytes.get_mut(usize::from(offset)) {
                    *slot = flag;
                }
                check_any_bytes(&bytes)?;
            }
        }
    }
}
