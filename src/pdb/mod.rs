//! DeviceSQL `export.pdb`: the raw, lossless page layer (ADR-002).
//!
//! Everything is little-endian. The file is a sequence of pages of `len_page` bytes; page
//! `n` starts at `n * len_page`. Page 0 is the file header:
//!
//! | bytes      | field              | meaning                                         |
//! |------------|--------------------|-------------------------------------------------|
//! | 0x00..0x04 | unknown            | 0 in real files                                 |
//! | 0x04..0x08 | `len_page`         | page length, 4,096 in real files                |
//! | 0x08..0x0c | `num_tables`       | number of table pointers                        |
//! | 0x0c..0x10 | `next_unused_page` | may point past the end of the file              |
//! | 0x10..0x14 | unknown            |                                                 |
//! | 0x14..0x18 | `sequence`         |                                                 |
//! | 0x18..0x1c | unknown            |                                                 |
//! | 0x1c..     | table pointers     | 16 bytes each: type, `empty_candidate`, first and last page |
//!
//! The rest of page 0 is kept verbatim. Each table is a chain of pages from its first page
//! along `next_page` to its last page; see [`Page`] for the page layout. Pages that
//! no chain reaches are kept as raw bytes.
//!
//! [`Pdb::from_bytes`] follows every chain and decodes each page it reaches, so errors are
//! reported at load time with absolute offsets. Rows are not typed at this layer: a
//! [`RawRow`] is a present row's offset and the heap from there on.
//!
//! The rows of the track table and the small tables are typed from a [`RawRow`] with
//! [`ExportRow::decode`] or each type's `decode` ([`Track`], [`Genre`], [`Artist`],
//! [`Album`], [`Label`], [`Key`], [`Color`], [`Artwork`]), and encode back to exactly the
//! row's bytes. A typed row ends after its last string; the padding after it stays in the
//! page heap.

pub(crate) mod defaults;
mod file;
mod ids;
mod page;
mod rows;
mod table;
#[cfg(test)]
mod testutil;

pub use file::Pdb;
pub use ids::{AlbumId, ArtistId, ArtworkId, ColorId, GenreId, KeyId, LabelId, TrackId};
pub use page::{DataBody, IndexBody, Page, PageBody, RawRow, RowGroup};
pub use rows::{
    Album, Artist, Artwork, Color, ExportRow, FileType, Genre, Key, Label, OtherFileType, Track,
    TrackFields, TrackString,
};
pub use table::{ExportTable, TablePointer, UnknownTable};
