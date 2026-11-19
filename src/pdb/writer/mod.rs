//! Fresh page layout for `export.pdb` tables (slice 7c): the values ADR-006 and ADR-007
//! leave to the writer, computed instead of stored.
//!
//! - [`page_sizes`]: a data page's `free_size` and `used_size`;
//! - [`index_shift_for_slot`]: track, artist and album `index_shift`;
//! - [`ExportRow::heap_footprint`]: the heap bytes a row takes, padding included;
//! - [`build_data_page`]: one data page from a table's rows, as rekordbox lays it out;
//! - [`pack`]: how many rows go on each page.
//!
//! Each rule is checked against every real page in `local` (`local-fixtures/`). Chaining
//! pages, index pages and the file header are not done here: the builder takes the page
//! links ([`PageLinks`]) as inputs.

use super::page::{DataHeader, HEAP_START, ROWS_PER_GROUP, row_index_len};
use super::{DataBody, ExportRow, ExportTable, Page, RowGroup};
use crate::error::EncodeError;

/// `page_flags` of a data page without deleted rows: on all 109 such real data pages.
/// Pages with deleted rows have 0x34 (bit 4 set) instead, on all 183 of them
/// (`local::real_data_page_header_fields`).
pub(crate) const DATA_PAGE_FLAGS: u8 = 0x24;

const CTX_HEAP: &str = "pdb page heap";
/// `index_shift` per row-index slot.
const INDEX_SHIFT_STEP: usize = 0x20;

/// A data page's `free_size` and `used_size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageSizes {
    pub(crate) free: u16,
    pub(crate) used: u16,
}

/// `free_size` and `used_size` of a data page `len_page` bytes long whose heap holds `used`
/// bytes, padding included, and whose row index holds `num_row_offsets` offsets: `used`, and
/// what the header, the heap and the row index leave of the page.
///
/// That is `free_size = len_page − 0x28 − row_index_len(num_row_offsets) − used_size` (the
/// row index with only its last group partial, see [`Page`](super::Page)), which holds on
/// every real data page (`local::real_page_sizes_are_computed`).
///
/// # Errors
///
/// [`EncodeError::LengthOverflow`] when the heap and row index do not fit the page
/// (`"pdb page heap"`), or a size does not fit its `u16` field.
pub(crate) fn page_sizes(
    len_page: usize,
    used: usize,
    num_row_offsets: usize,
) -> Result<PageSizes, EncodeError> {
    let free = len_page
        .checked_sub(HEAP_START)
        .and_then(|heap| heap.checked_sub(row_index_len(num_row_offsets)))
        .and_then(|heap| heap.checked_sub(used))
        .ok_or_else(heap_overflow)?;
    let field = |value: usize, context| {
        u16::try_from(value).map_err(|_| EncodeError::LengthOverflow { context })
    };
    Ok(PageSizes {
        free: field(free, "pdb page free_size")?,
        used: field(used, "pdb page used_size")?,
    })
}

/// The `index_shift` of a track, artist or album row in slot `slot` of its page's row
/// index, absent rows counted: 0x20 × `slot`. This holds on every track, artist and album
/// row of the real stick (`local::real_index_shift_is_computed_from_the_slot`); the other
/// typed tables have no such field.
///
/// # Errors
///
/// [`EncodeError::ValueOutOfRange`] when the value does not fit the `u16` field, from slot
/// 0x800 on, far more rows than a 4,096-byte page can hold.
pub(crate) fn index_shift_for_slot(slot: usize) -> Result<u16, EncodeError> {
    slot.checked_mul(INDEX_SHIFT_STEP)
        .and_then(|shift| u16::try_from(shift).ok())
        .ok_or(EncodeError::ValueOutOfRange {
            context: "pdb row index_shift",
        })
}

/// The header fields that link a page into its table's chain and order it among writes.
/// Chaining pages decides them (slice 7d); the page builder takes them as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageLinks {
    pub(crate) page_index: u32,
    pub(crate) next_page: u32,
    pub(crate) sequence: u32,
}

/// The transaction fields of a data page: `transaction_row_count` and
/// `transaction_row_index` at 0x20 and each row group's `tranrf`.
///
/// On the real stick they look like a record of the page's last write: `tranrf` flags some
/// rows, and `transaction_row_count` is the number of flags on every data page but 15,
/// which hold 0x1fff in both fields and no flag. The 33 pages written in one go (all rows, from row 0, `tranrf` equal to
/// `rowpf`) are what a fresh layout makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Transaction {
    /// The page's rows were all written in one transaction: the count is the number of
    /// rows, the index 0, and each `tranrf` equals its `rowpf`.
    WholePage,
    /// Stored values, one `tranrf` per row group, for rebuilding a real page.
    Recorded {
        row_count: u16,
        row_index: u16,
        tranrf: Vec<u16>,
    },
}

/// Builds a data page of `table`, `len_page` bytes long, holding `rows` in order, laid out
/// as rekordbox lays out its pages:
///
/// - row `i` takes slot `i` of the row index, every row present (`rowpf` all set), so
///   `num_rows` = `num_row_offsets` = the number of rows; the row index holds groups of 16
///   offsets, only the last one shorter;
/// - the rows lie in the heap in slot order from offset 0, each taking its
///   [`ExportRow::heap_footprint`], zero-padded; `used_size` is their sum and the heap is
///   zero after it; `free_size` comes from [`page_sizes`];
/// - track, artist and album rows get [`index_shift_for_slot`] of their slot, whatever
///   they held;
/// - `page_flags` is [`DATA_PAGE_FLAGS`]; the unknown header fields (`unknown1`,
///   `unknown2`, `unknown3` and `unknown4`) are 0, as on every real data page;
/// - `links` and the [`Transaction`] fields are taken as given.
///
/// # Errors
///
/// - [`EncodeError::LengthOverflow`] (`"pdb page heap"`) when the rows and their row index
///   do not fit the page, and the errors of [`page_sizes`] and the rows' `encode`;
/// - [`EncodeError::ValueOutOfRange`] when a recorded transaction does not have one
///   `tranrf` per row group, or an `index_shift` does not fit.
pub(crate) fn build_data_page(
    len_page: usize,
    table: ExportTable,
    rows: Vec<ExportRow>,
    links: PageLinks,
    transaction: &Transaction,
) -> Result<Page, EncodeError> {
    let mut heap = Vec::new();
    let mut offsets = Vec::with_capacity(rows.len());
    for (slot, row) in rows.into_iter().enumerate() {
        let row = match row.index_shift() {
            Some(_) => row.with_index_shift(index_shift_for_slot(slot)?),
            None => row,
        };
        let start = heap.len();
        offsets.push(u16::try_from(start).map_err(|_| heap_overflow())?);
        let footprint = row.heap_footprint()?;
        heap.extend(row.encode()?);
        heap.resize(start.saturating_add(footprint), 0);
    }
    let sizes = page_sizes(len_page, heap.len(), offsets.len())?;
    heap.resize(heap.len() + usize::from(sizes.free), 0);

    let groups = offsets.chunks(ROWS_PER_GROUP);
    let (row_count, row_index, tranrf) = match transaction {
        Transaction::WholePage => {
            let count = u16::try_from(offsets.len()).map_err(|_| heap_overflow())?;
            (count, 0, groups.clone().map(all_present).collect())
        }
        Transaction::Recorded {
            row_count,
            row_index,
            tranrf,
        } => {
            if tranrf.len() != groups.len() {
                return Err(EncodeError::ValueOutOfRange {
                    context: "pdb row group tranrf",
                });
            }
            (*row_count, *row_index, tranrf.clone())
        }
    };
    let row_groups = groups
        .zip(tranrf)
        .map(|(group, tranrf)| RowGroup::new(group.to_vec(), all_present(group), tranrf))
        .collect();
    let header = DataHeader {
        page_index: links.page_index,
        page_type: table.to_u32(),
        next_page: links.next_page,
        sequence: links.sequence,
        page_flags: DATA_PAGE_FLAGS,
        free_size: sizes.free,
        used_size: sizes.used,
    };
    Ok(Page::data(
        header,
        DataBody::new(row_count, row_index, heap, row_groups),
    ))
}

fn heap_overflow() -> EncodeError {
    EncodeError::LengthOverflow { context: CTX_HEAP }
}

/// The `rowpf` of a row group whose rows are all present.
fn all_present(group: &[u16]) -> u16 {
    // A group holds 1 to 16 offsets.
    u16::MAX >> (ROWS_PER_GROUP - group.len().clamp(1, ROWS_PER_GROUP))
}

/// Splits `rows`, in order, into data pages `len_page` bytes long the way rekordbox fills
/// them, and returns the number of rows on each page: a page takes rows until the next one,
/// its footprint and its row index entry, no longer fits ([`page_sizes`]). A page may end
/// with no free space at all. There is no limit on the number of rows besides the space.
///
/// # Errors
///
/// [`EncodeError::LengthOverflow`] (`"pdb page heap"`) for a row that does not fit even an
/// empty page, and the errors of the rows' `encode`.
pub(crate) fn pack(len_page: usize, rows: &[ExportRow]) -> Result<Vec<usize>, EncodeError> {
    let mut pages = Vec::new();
    let (mut count, mut used) = (0usize, 0usize);
    for row in rows {
        let footprint = row.heap_footprint()?;
        let with_row = used.saturating_add(footprint);
        if page_sizes(len_page, with_row, count + 1).is_err() {
            if count > 0 {
                pages.push(count);
            }
            page_sizes(len_page, footprint, 1)?;
            (count, used) = (0, 0);
        }
        count += 1;
        used += footprint;
    }
    if count > 0 {
        pages.push(count);
    }
    Ok(pages)
}

#[cfg(test)]
mod local;

#[cfg(test)]
mod tests {
    use super::super::{Artist, ArtistId, Genre, GenreId, PageBody};
    use super::*;

    const LEN: usize = 512;
    const LINKS: PageLinks = PageLinks {
        page_index: 5,
        next_page: 9,
        sequence: 7,
    };

    fn genre(id: u32, name: &str) -> ExportRow {
        ExportRow::Genre(Genre::new(GenreId::new(id).unwrap(), name))
    }

    fn artist(id: u32, name: &str) -> ExportRow {
        ExportRow::Artist(Artist::new(ArtistId::new(id).unwrap(), name))
    }

    /// A whole-page genre page `len` bytes long.
    fn try_build(len: usize, rows: Vec<ExportRow>) -> Result<Page, EncodeError> {
        let whole = Transaction::WholePage;
        build_data_page(len, ExportTable::Genres, rows, LINKS, &whole)
    }

    fn build(rows: Vec<ExportRow>, transaction: &Transaction) -> Page {
        build_data_page(LEN, ExportTable::Genres, rows, LINKS, transaction).expect("builds")
    }

    #[test]
    fn a_page_is_laid_out_as_rekordbox_does() {
        let rows = vec![genre(1, "House"), genre(2, "Pop")];
        let page = build(rows, &Transaction::WholePage);
        let mut expected = Vec::new();
        for value in [0u32, 5, 1, 9, 7, 0] {
            expected.extend_from_slice(&value.to_le_bytes());
        }
        // 2 rows of 2 offsets, flags 0x24, free 512 − 0x28 − 8 − 20, used 20.
        expected.extend_from_slice(&[0x02, 0x40, 0x00, 0x24]);
        expected.extend_from_slice(&444u16.to_le_bytes());
        expected.extend_from_slice(&20u16.to_le_bytes());
        // Both rows in this transaction, from row 0; then two zero u16s.
        expected.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 0]);
        // "House" is padded to 12 bytes; "Pop" takes 8.
        expected.extend_from_slice(&[1, 0, 0, 0, 0x0d, b'H', b'o', b'u', b's', b'e', 0, 0]);
        expected.extend_from_slice(&[2, 0, 0, 0, 0x09, b'P', b'o', b'p']);
        expected.resize(LEN - 8, 0);
        // Row 1's offset, row 0's, `rowpf` and `tranrf`.
        expected.extend_from_slice(&[12, 0, 0, 0, 0b11, 0, 0b11, 0]);
        assert_eq!(page.to_bytes().unwrap(), expected);
        assert_eq!(Page::decode(&expected, 0).unwrap(), page);
    }

    #[test]
    fn index_shift_is_set_from_the_slot() {
        let rows = vec![artist(1, "A"), artist(2, "B"), artist(3, "C")];
        let page = build(rows, &Transaction::WholePage);
        let shifts: Vec<u16> = page
            .rows()
            .map(|raw| match ExportRow::decode(ExportTable::Artists, &raw) {
                Ok(Some(ExportRow::Artist(a))) => a.index_shift(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(shifts, [0, 0x20, 0x40]);
    }

    #[test]
    fn seventeen_rows_take_a_full_group_and_one_of_one() {
        let rows: Vec<ExportRow> = (1..=17).map(|id| genre(id, "")).collect();
        let page = build(rows, &Transaction::WholePage);
        let PageBody::Data(body) = page.body() else {
            panic!("data page");
        };
        let groups: Vec<(usize, u16, u16)> = body
            .row_groups()
            .iter()
            .map(|g| (g.offsets().len(), g.rowpf(), g.tranrf()))
            .collect();
        assert_eq!(groups, [(16, 0xffff, 0xffff), (1, 1, 1)]);
        assert_eq!(body.row_groups()[1].offsets(), [16 * 8]);
        assert_eq!((page.num_row_offsets(), page.num_rows()), (17, 17));
        assert_eq!(page.used_size(), 17 * 8);
        assert_eq!(usize::from(page.free_size()), LEN - 0x28 - 36 - 6 - 17 * 8);
        assert_eq!(
            (body.transaction_row_count(), body.transaction_row_index()),
            (17, 0)
        );
    }

    #[test]
    fn recorded_transaction_fields_are_kept() {
        let rows: Vec<ExportRow> = (1..=17).map(|id| genre(id, "")).collect();
        let recorded = Transaction::Recorded {
            row_count: 1,
            row_index: 16,
            tranrf: vec![0, 1],
        };
        let page = build(rows.clone(), &recorded);
        let PageBody::Data(body) = page.body() else {
            panic!("data page");
        };
        assert_eq!(
            (body.transaction_row_count(), body.transaction_row_index()),
            (1, 16)
        );
        let tranrf: Vec<u16> = body.row_groups().iter().map(|g| g.tranrf()).collect();
        assert_eq!(tranrf, [0, 1]);
        let wrong = Transaction::Recorded {
            row_count: 1,
            row_index: 16,
            tranrf: vec![0],
        };
        assert_eq!(
            build_data_page(LEN, ExportTable::Genres, rows, LINKS, &wrong),
            Err(EncodeError::ValueOutOfRange {
                context: "pdb row group tranrf"
            })
        );
    }

    #[test]
    fn rows_that_do_not_fit_the_page_fail() {
        // 46 rows of 8 bytes and their row index (2 full groups and one of 14) take all 472
        // bytes after the header; one more row does not fit.
        let rows: Vec<ExportRow> = (1..=46).map(|id| genre(id, "")).collect();
        let page = build(rows, &Transaction::WholePage);
        assert_eq!(page.free_size(), 0);
        let rows: Vec<ExportRow> = (1..=47).map(|id| genre(id, "")).collect();
        assert_eq!(
            try_build(LEN, rows),
            Err(EncodeError::LengthOverflow {
                context: "pdb page heap"
            })
        );
    }

    #[test]
    fn row_counts_that_do_not_fit_the_header_fail_to_encode() {
        // 0x800 rows of 8 bytes fit a page long enough for them and their row index, but
        // not the 11 bits of `num_rows`.
        let rows: Vec<ExportRow> = (1..=0x800).map(|id| genre(id, "")).collect();
        let len = 0x28 + 0x800 * 8 + row_index_len(0x800);
        let page = try_build(len, rows).expect("builds");
        assert_eq!(
            page.to_bytes(),
            Err(EncodeError::ValueOutOfRange {
                context: "pdb page num_rows"
            })
        );
    }

    #[test]
    fn pack_fills_each_page_until_the_next_row_does_not_fit() {
        assert_eq!(pack(LEN, &[]), Ok(vec![]));
        let rows: Vec<ExportRow> = (1..=130).map(|id| genre(id, "")).collect();
        // 46 rows fill a page exactly (see above).
        assert_eq!(pack(LEN, &rows), Ok(vec![46, 46, 38]));
        // 16 rows of 24 bytes and one full group take 420 of 472 bytes; a 17th row needs
        // 24 and a new group of 6 bytes, which leaves 22.
        let name = "x".repeat(19);
        let rows: Vec<ExportRow> = (1..=17).map(|id| genre(id, &name)).collect();
        assert_eq!(rows[0].heap_footprint(), Ok(24));
        assert_eq!(pack(LEN, &rows), Ok(vec![17]));
        let name = "x".repeat(23);
        let rows: Vec<ExportRow> = (1..=17).map(|id| genre(id, &name)).collect();
        assert_eq!(rows[0].heap_footprint(), Ok(28));
        // 16 × 28 + 36 = 484 > 472: the 16th row already does not fit.
        assert_eq!(pack(LEN, &rows), Ok(vec![15, 2]));
    }

    #[test]
    fn pack_fails_on_a_row_no_page_can_hold() {
        let rows = vec![genre(1, ""), genre(2, &"x".repeat(LEN))];
        assert_eq!(
            pack(LEN, &rows),
            Err(EncodeError::LengthOverflow {
                context: "pdb page heap"
            })
        );
    }

    #[test]
    fn free_size_is_what_the_header_heap_and_row_index_leave() {
        // 3 offsets: one partial group of 2 × 3 bytes plus `rowpf` and `tranrf`.
        assert_eq!(
            page_sizes(4096, 100, 3),
            Ok(PageSizes {
                free: 4096 - 0x28 - 10 - 100,
                used: 100
            })
        );
        // 16 offsets: one full group of 36 bytes; 17: another group of 6.
        assert_eq!(page_sizes(4096, 0, 16).unwrap().free, 4096 - 0x28 - 36);
        assert_eq!(page_sizes(4096, 0, 17).unwrap().free, 4096 - 0x28 - 42);
        assert_eq!(page_sizes(4096, 8, 0).unwrap().free, 4096 - 0x28 - 8);
    }

    #[test]
    fn index_shift_is_0x20_per_slot() {
        assert_eq!(index_shift_for_slot(0), Ok(0));
        assert_eq!(index_shift_for_slot(1), Ok(0x20));
        assert_eq!(index_shift_for_slot(17), Ok(0x220));
        assert_eq!(index_shift_for_slot(0x7ff), Ok(0xffe0));
        let overflow = Err(EncodeError::ValueOutOfRange {
            context: "pdb row index_shift",
        });
        assert_eq!(index_shift_for_slot(0x800), overflow);
        assert_eq!(index_shift_for_slot(usize::MAX), overflow);
    }

    #[test]
    fn a_full_page_has_no_free_space_and_an_overfull_one_fails() {
        let full = 4096 - 0x28 - 10;
        assert_eq!(
            page_sizes(4096, full, 3),
            Ok(PageSizes {
                free: 0,
                used: full as u16
            })
        );
        let overflow = EncodeError::LengthOverflow {
            context: "pdb page heap",
        };
        assert_eq!(page_sizes(4096, full + 1, 3), Err(overflow.clone()));
        assert_eq!(page_sizes(0x28, 0, 1), Err(overflow.clone()));
        assert_eq!(page_sizes(0x10, 0, 0), Err(overflow.clone()));
        // Sizes past a u16 do not fit the header fields.
        let too_long = EncodeError::LengthOverflow {
            context: "pdb page free_size",
        };
        assert_eq!(page_sizes(0x10000 + 0x28, 0, 0), Err(too_long));
    }
}
