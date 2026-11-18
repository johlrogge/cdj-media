//! `export.pdb` table types and the table pointers of the file header.

/// Length of one table pointer in the file header.
pub(crate) const TABLE_POINTER_LEN: usize = 16;

/// The tables of `export.pdb`, by the `type` of their table pointer and pages.
///
/// Only for `export.pdb`: `exportExt.pdb` uses the same type numbers for other tables. The
/// raw layer itself is schema-agnostic ([`TablePointer::raw_type`], [`Page::raw_type`]);
/// interpret a type with [`ExportTable::from_u32`].
///
/// The names follow djl-analysis. `Unknown` values may become named variants later.
///
/// [`Page::raw_type`]: super::Page::raw_type
///
/// ```compile_fail
/// // `Unknown` cannot be built by hand, so it can never alias a named table.
/// use cdj_media::pdb::{ExportTable, UnknownTable};
/// let _ = ExportTable::Unknown(UnknownTable(2));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExportTable {
    /// Tracks: type 0.
    Tracks,
    /// Genres: type 1.
    Genres,
    /// Artists: type 2.
    Artists,
    /// Albums: type 3.
    Albums,
    /// Labels: type 4.
    Labels,
    /// Musical keys: type 5.
    Keys,
    /// Colour labels: type 6.
    Colors,
    /// The playlist and folder tree: type 7.
    PlaylistTree,
    /// The tracks of each playlist: type 8.
    PlaylistEntries,
    /// History playlists: type 11.
    HistoryPlaylists,
    /// The tracks of each history playlist: type 12.
    HistoryEntries,
    /// Album artwork paths: type 13.
    Artwork,
    /// Browse columns: type 16.
    Columns,
    /// History: type 19.
    History,
    /// Any other type, including 9, 10, 14, 15, 17 and 18, which djl-analysis does not
    /// name. Only [`ExportTable::from_u32`] makes one, so it never holds a named type.
    Unknown(UnknownTable),
}

/// The `type` value of an [`ExportTable::Unknown`]; never that of a named table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnknownTable(u32);

impl UnknownTable {
    /// The `type` value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl ExportTable {
    /// The table for a `type` value.
    pub const fn from_u32(value: u32) -> Self {
        match value {
            0 => Self::Tracks,
            1 => Self::Genres,
            2 => Self::Artists,
            3 => Self::Albums,
            4 => Self::Labels,
            5 => Self::Keys,
            6 => Self::Colors,
            7 => Self::PlaylistTree,
            8 => Self::PlaylistEntries,
            11 => Self::HistoryPlaylists,
            12 => Self::HistoryEntries,
            13 => Self::Artwork,
            16 => Self::Columns,
            19 => Self::History,
            other => Self::Unknown(UnknownTable(other)),
        }
    }

    /// The `type` value.
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::Tracks => 0,
            Self::Genres => 1,
            Self::Artists => 2,
            Self::Albums => 3,
            Self::Labels => 4,
            Self::Keys => 5,
            Self::Colors => 6,
            Self::PlaylistTree => 7,
            Self::PlaylistEntries => 8,
            Self::HistoryPlaylists => 11,
            Self::HistoryEntries => 12,
            Self::Artwork => 13,
            Self::Columns => 16,
            Self::History => 19,
            Self::Unknown(other) => other.get(),
        }
    }
}

/// A table pointer from the file header: where a table's page chain starts and ends.
///
/// Every field is stored verbatim. `empty_candidate` and `last_page` are not checked
/// against the file: real files have `empty_candidate` values past the file end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TablePointer {
    table_type: u32,
    empty_candidate: u32,
    first_page: u32,
    last_page: u32,
}

impl TablePointer {
    pub(crate) const fn new(
        table_type: u32,
        empty_candidate: u32,
        first_page: u32,
        last_page: u32,
    ) -> Self {
        Self {
            table_type,
            empty_candidate,
            first_page,
            last_page,
        }
    }

    /// The `type` field as stored. Its meaning depends on the file: see [`ExportTable`]
    /// for `export.pdb`.
    pub const fn raw_type(&self) -> u32 {
        self.table_type
    }

    /// The `empty_candidate` field: in real files, the `next_page` of the table's last
    /// page. It may point past the end of the file.
    pub const fn empty_candidate(&self) -> u32 {
        self.empty_candidate
    }

    /// The first page of the table's chain.
    pub const fn first_page(&self) -> u32 {
        self.first_page
    }

    /// The last page of the table's chain.
    pub const fn last_page(&self) -> u32 {
        self.last_page
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn named_types_have_djl_analysis_numbers() {
        let named = [
            (0, ExportTable::Tracks),
            (1, ExportTable::Genres),
            (2, ExportTable::Artists),
            (3, ExportTable::Albums),
            (4, ExportTable::Labels),
            (5, ExportTable::Keys),
            (6, ExportTable::Colors),
            (7, ExportTable::PlaylistTree),
            (8, ExportTable::PlaylistEntries),
            (11, ExportTable::HistoryPlaylists),
            (12, ExportTable::HistoryEntries),
            (13, ExportTable::Artwork),
            (16, ExportTable::Columns),
            (19, ExportTable::History),
        ];
        for (value, table) in named {
            assert_eq!(ExportTable::from_u32(value), table, "{value}");
            assert_eq!(table.to_u32(), value, "{table:?}");
        }
    }

    #[test]
    fn unnamed_types_are_unknown() {
        for value in [9, 10, 14, 15, 17, 18, 20, u32::MAX] {
            assert!(
                matches!(ExportTable::from_u32(value), ExportTable::Unknown(u) if u.get() == value),
                "{value}"
            );
        }
    }

    #[test]
    fn table_pointer_keeps_its_fields() {
        let table = TablePointer::new(13, 40, 2, 9);
        assert_eq!(
            ExportTable::from_u32(table.raw_type()),
            ExportTable::Artwork
        );
        assert_eq!(
            (
                table.empty_candidate(),
                table.first_page(),
                table.last_page()
            ),
            (40, 2, 9)
        );
    }

    proptest! {
        #[test]
        fn to_u32_inverts_from_u32(value in any::<u32>()) {
            prop_assert_eq!(ExportTable::from_u32(value).to_u32(), value);
        }
    }
}
