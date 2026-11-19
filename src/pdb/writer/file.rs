//! The whole `export.pdb` (slice 7d): page layout, chains, sequence numbers and the file
//! header.

use super::super::{ExportRow, ExportTable, Page, Pdb, TablePointer};
use super::fixed::{column_rows, default_colors, table_17_rows, table_18_rows};
use super::index::build_index_page;
use super::{HeapRow, PageLinks, Transaction, build_data_page, build_page, pack, pack_footprints};
use crate::error::EncodeError;

/// The header's `sequence` for a file whose pages are `pages`: one more than the highest
/// page `sequence`, as on the real stick (15,493 after 15,492).
pub(crate) fn file_sequence<'a>(pages: impl IntoIterator<Item = &'a Page>) -> u32 {
    let highest = pages.into_iter().map(Page::sequence).max().unwrap_or(0);
    highest.saturating_add(1)
}

/// The header's `next_unused_page` for a file of `page_count` pages (page 0 included) with
/// `tables`: the first page past both the file and every `empty_candidate`, as on the real
/// stick (333, with 331 pages and an `empty_candidate` of 332).
pub(crate) fn next_unused_page(page_count: u32, tables: &[TablePointer]) -> u32 {
    tables
        .iter()
        .map(|table| table.empty_candidate().saturating_add(1))
        .fold(page_count, u32::max)
}

/// Writes a whole `export.pdb` with pages `len_page` bytes long from typed rows of any
/// table, each table's rows in the order given. Colours are rekordbox's defaults
/// ([`default_colors`]) unless `rows` holds colour rows; the browse columns and tables 17
/// and 18 always hold rekordbox's rows ([`column_rows`], [`table_17_rows`],
/// [`table_18_rows`]); the history and other unnamed tables are empty.
///
/// The layout follows the real stick where it shows a rule:
///
/// - the 20 tables of types 0 to 19, in type order;
/// - table `t` (its type) has its index page at page 2t + 1, and page 2t + 2 is its first
///   data page or, for an empty table, its `empty_candidate`, zero-filled (or past the end
///   of the file, when no later page is written);
/// - the further data pages follow from page 41, table by table, each table's in order;
/// - then each table that has data pages gets its `empty_candidate`, in type order. Those
///   pages lie past the end of the file, which ends with its last table page;
/// - each table's last page has its `empty_candidate` as `next_page`;
/// - index pages are empty ([`build_index_page`]) with `sequence` 1; data pages are packed
///   full ([`pack`]), written in one transaction, and numbered 2, 3, … in page order;
/// - the header's `sequence` and `next_unused_page` follow [`file_sequence`] and
///   [`next_unused_page`].
///
/// # Errors
///
/// The errors of the page builders, and [`EncodeError::LengthOverflow`] when a page index
/// or `sequence` does not fit a `u32`, or `len_page` does not fit the header.
pub(crate) fn write_pdb(len_page: usize, rows: Vec<ExportRow>) -> Result<Pdb, EncodeError> {
    let len = u32::try_from(len_page).map_err(|_| EncodeError::LengthOverflow {
        context: "pdb len_page",
    })?;
    let plans = plan_pages(len_page, rows)?;

    // Page numbers: each table's first data page at 2t + 2, the rest from page 41.
    let mut next = 2 * TABLE_COUNT + 1;
    let mut data_pages: Vec<Vec<u32>> = Vec::with_capacity(plans.len());
    for (t, plan) in (0u32..).zip(&plans) {
        let mut pages = Vec::with_capacity(plan.len());
        for k in 0..plan.len() {
            pages.push(if k == 0 { 2 * t + 2 } else { take(&mut next)? });
        }
        data_pages.push(pages);
    }
    let mut candidates = Vec::with_capacity(data_pages.len());
    for (t, pages) in (0u32..).zip(&data_pages) {
        candidates.push(if pages.is_empty() {
            2 * t + 2
        } else {
            take(&mut next)?
        });
    }
    let mut in_order: Vec<u32> = data_pages.iter().flatten().copied().collect();
    in_order.sort_unstable();
    let sequence_of = |page: u32| {
        let k = in_order.binary_search(&page).unwrap_or(0);
        u32::try_from(k)
            .ok()
            .and_then(|k| k.checked_add(FIRST_DATA_SEQUENCE))
            .ok_or(EncodeError::LengthOverflow {
                context: "pdb page sequence",
            })
    };

    let mut pages: Vec<Option<Page>> = Vec::new();
    let mut tables = Vec::with_capacity(plans.len());
    let tables_in = plans.into_iter().zip(&data_pages).zip(candidates);
    for (t, ((plan, data), candidate)) in (0u32..).zip(tables_in) {
        let index_page = 2 * t + 1;
        let first = data.first().copied();
        let links = PageLinks {
            page_index: index_page,
            next_page: first.unwrap_or(candidate),
            sequence: INDEX_SEQUENCE,
        };
        place(
            &mut pages,
            index_page,
            build_index_page(len_page, t, links, first)?,
        );
        for (k, (rows, &page_index)) in plan.into_iter().zip(data).enumerate() {
            let links = PageLinks {
                page_index,
                next_page: data.get(k + 1).copied().unwrap_or(candidate),
                sequence: sequence_of(page_index)?,
            };
            place(&mut pages, page_index, rows.build(len_page, links)?);
        }
        let last = data.last().copied().unwrap_or(index_page);
        tables.push(TablePointer::new(t, candidate, index_page, last));
    }
    let page_count = u32::try_from(pages.len() + 1).map_err(|_| page_overflow())?;
    let sequence = file_sequence(pages.iter().flatten());
    let next_unused = next_unused_page(page_count, &tables);
    Pdb::new(len, next_unused, sequence, tables, pages)
}

/// The number of tables in a written `export.pdb`: types 0 to 19, as on the real stick.
const TABLE_COUNT: u32 = 20;
/// `sequence` of every index page: 1 on the real stick's untouched ones.
const INDEX_SEQUENCE: u32 = 1;
/// `sequence` of the first data page written; the real stick's colour page has 2, the
/// column page 3.
const FIRST_DATA_SEQUENCE: u32 = 2;

/// The rows of one data page, as they are given to the page builder.
enum PageRows {
    /// Typed rows of a table.
    Typed(ExportTable, Vec<ExportRow>),
    /// Encoded rows of a table of the given type.
    Encoded(u32, Vec<HeapRow>),
}

impl PageRows {
    fn build(self, len_page: usize, links: PageLinks) -> Result<Page, EncodeError> {
        let whole = Transaction::WholePage;
        match self {
            Self::Typed(table, rows) => build_data_page(len_page, table, rows, links, &whole),
            Self::Encoded(page_type, rows) => build_page(len_page, page_type, rows, links, &whole),
        }
    }
}

/// The type of the unnamed table 17, which holds fixed rows ([`table_17_rows`]).
const TABLE_17: u32 = 17;
/// The type of the unnamed table 18, which holds fixed rows ([`table_18_rows`]).
const TABLE_18: u32 = 18;

/// The data pages of each of the 20 tables, in type order: `rows` by table, the default
/// colours if `rows` has none, and the fixed rows of the browse columns and tables 17 and
/// 18, each table packed with [`pack`].
fn plan_pages(len_page: usize, rows: Vec<ExportRow>) -> Result<Vec<Vec<PageRows>>, EncodeError> {
    let mut by_table: Vec<Vec<ExportRow>> = (0..TABLE_COUNT).map(|_| Vec::new()).collect();
    for row in rows {
        let t = usize::try_from(row.table().to_u32()).unwrap_or(usize::MAX);
        if let Some(table) = by_table.get_mut(t) {
            table.push(row);
        }
    }
    let colors = &mut by_table[ExportTable::Colors.to_u32() as usize];
    if colors.is_empty() {
        *colors = default_colors();
    }
    let mut plans = Vec::with_capacity(by_table.len());
    for (t, rows) in (0u32..).zip(by_table) {
        let table = ExportTable::from_u32(t);
        let fixed = match t {
            _ if table == ExportTable::Columns => Some(column_rows()?),
            TABLE_17 => Some(table_17_rows()),
            TABLE_18 => Some(table_18_rows()),
            _ => None,
        };
        let plan = if let Some(rows) = fixed {
            let footprints: Vec<usize> = rows.iter().map(|row| row.footprint).collect();
            let counts = pack_footprints(len_page, &footprints)?;
            split(rows, &counts)
                .into_iter()
                .map(|rows| PageRows::Encoded(t, rows))
                .collect()
        } else {
            let counts = pack(len_page, &rows)?;
            split(rows, &counts)
                .into_iter()
                .map(|rows| PageRows::Typed(table, rows))
                .collect()
        };
        plans.push(plan);
    }
    Ok(plans)
}

/// `items` split into runs of `counts`, in order.
fn split<T>(items: Vec<T>, counts: &[usize]) -> Vec<Vec<T>> {
    let mut items = items.into_iter();
    counts
        .iter()
        .map(|&count| items.by_ref().take(count).collect())
        .collect()
}

/// The page `next` names; `next` moves on.
fn take(next: &mut u32) -> Result<u32, EncodeError> {
    let page = *next;
    *next = page.checked_add(1).ok_or_else(page_overflow)?;
    Ok(page)
}

fn page_overflow() -> EncodeError {
    EncodeError::LengthOverflow {
        context: "pdb page index",
    }
}

/// Puts `page` at position `index` (1 or more) of the pages after page 0.
fn place(pages: &mut Vec<Option<Page>>, index: u32, page: Page) {
    let at = usize::try_from(index)
        .unwrap_or(usize::MAX)
        .saturating_sub(1);
    if pages.len() <= at {
        pages.resize(at + 1, None);
    }
    pages[at] = Some(page);
}

#[cfg(test)]
mod tests {
    use super::super::super::{
        Artist, ArtistId, Color, ColorId, Genre, GenreId, Key, KeyId, PageBody,
    };
    use super::*;
    use crate::pdb::Pdb;
    use crate::pdb::testutil::minimal_file;

    const LEN: usize = 512;

    fn genre(id: u32) -> ExportRow {
        ExportRow::Genre(Genre::new(
            GenreId::new(id).unwrap(),
            &format!("genre {id}"),
        ))
    }

    /// 60 genres (more than a 512-byte page holds), 3 artists and 2 keys.
    fn rows() -> Vec<ExportRow> {
        let mut rows: Vec<ExportRow> = (1..=60).map(genre).collect();
        for (id, name) in [(5, "A"), (2, "B"), (9, "C")] {
            rows.push(ExportRow::Artist(Artist::new(
                ArtistId::new(id).unwrap(),
                name,
            )));
        }
        for (id, name) in [(1, "1A"), (2, "2A")] {
            rows.push(ExportRow::Key(Key::new(KeyId::new(id).unwrap(), name)));
        }
        rows
    }

    /// The written file, encoded and decoded again.
    fn written(rows: Vec<ExportRow>) -> Pdb {
        let bytes = write_pdb(LEN, rows).unwrap().to_bytes().unwrap();
        Pdb::from_bytes(&bytes).expect("the written file decodes")
    }

    /// The typed rows of `table`, in chain order, with `index_shift` 0.
    fn rows_of(pdb: &Pdb, table: ExportTable) -> Vec<ExportRow> {
        pdb.table_pages(table)
            .flat_map(|page| page.rows())
            .map(|raw| ExportRow::decode(table, &raw).unwrap().unwrap())
            .map(|row| row.with_index_shift(0))
            .collect()
    }

    #[test]
    fn rows_read_back_equal_the_rows_put_in() {
        let pdb = written(rows());
        let input = rows();
        for table in [ExportTable::Genres, ExportTable::Artists, ExportTable::Keys] {
            let expected: Vec<ExportRow> = input
                .iter()
                .filter(|row| row.table() == table)
                .cloned()
                .collect();
            assert_eq!(rows_of(&pdb, table), expected, "{table:?}");
        }
        assert!(pdb.table_pages(ExportTable::Genres).count() > 2);
        assert_eq!(rows_of(&pdb, ExportTable::Colors), default_colors());
        assert!(rows_of(&pdb, ExportTable::Tracks).is_empty());
        let columns: Vec<Vec<u8>> = pdb
            .table_pages(ExportTable::Columns)
            .flat_map(|page| page.rows())
            .map(|raw| raw.bytes()[..4].to_vec())
            .collect();
        assert_eq!(columns.len(), 27);
        assert_eq!(columns[26], [27, 0, 0xaa, 0]);
    }

    #[test]
    fn given_colours_replace_the_defaults() {
        let red = ExportRow::Color(Color::new(ColorId::new(2).unwrap(), "Red"));
        let pdb = written(vec![red.clone()]);
        assert_eq!(rows_of(&pdb, ExportTable::Colors), [red]);
    }

    #[test]
    fn every_table_chains_from_its_index_page_to_its_empty_candidate() {
        let pdb = written(rows());
        let mut seen = std::collections::BTreeSet::new();
        assert_eq!(pdb.tables().len(), 20);
        for (t, table) in pdb.tables().iter().enumerate() {
            let t = t as u32;
            assert_eq!(table.raw_type(), t);
            assert_eq!(table.first_page(), 2 * t + 1);
            let pages: Vec<&Page> = pdb.pages_of(table).collect();
            assert_eq!(pages.last().unwrap().page_index(), table.last_page());
            assert_eq!(pages.last().unwrap().next_page(), table.empty_candidate());
            assert!(matches!(pages[0].body(), PageBody::Index(_)), "{t}");
            for (i, page) in pages.iter().enumerate() {
                assert!(seen.insert(page.page_index()), "page in two chains");
                assert_eq!(page.raw_type(), t);
                assert_eq!(i > 0, matches!(page.body(), PageBody::Data(_)));
            }
            if pages.len() == 1 {
                assert_eq!(table.empty_candidate(), 2 * t + 2, "{t}");
                assert_eq!(pdb.unchained_page(2 * t + 2).map(<[u8]>::len), Some(LEN));
            } else {
                assert_eq!(pages[1].page_index(), 2 * t + 2);
            }
        }
        for table in pdb.tables() {
            assert!(!seen.contains(&table.empty_candidate()));
        }
        let candidates: std::collections::BTreeSet<u32> =
            pdb.tables().iter().map(|t| t.empty_candidate()).collect();
        assert_eq!(
            candidates.len(),
            20,
            "every empty_candidate is its own page"
        );
        // Genres, artists, keys, colours and columns have data: 5 candidates from 41 on,
        // after the extra pages.
        let last_page = pdb.page_count() as u32 - 1;
        assert_eq!(last_page, *seen.last().unwrap());
        assert_eq!(pdb.next_unused_page(), *candidates.last().unwrap() + 1);
        assert_eq!(
            pdb.next_unused_page(),
            next_unused_page(pdb.page_count() as u32, pdb.tables())
        );
    }

    #[test]
    fn index_pages_have_sequence_1_and_data_pages_count_from_2_in_page_order() {
        let pdb = written(rows());
        let mut data: Vec<(u32, u32)> = Vec::new();
        for table in pdb.tables() {
            for page in pdb.pages_of(table) {
                match page.body() {
                    PageBody::Index(_) => assert_eq!(page.sequence(), 1),
                    PageBody::Data(_) => data.push((page.page_index(), page.sequence())),
                }
            }
        }
        data.sort_unstable();
        let sequences: Vec<u32> = data.iter().map(|&(_, s)| s).collect();
        let expected: Vec<u32> = (2..2 + data.len() as u32).collect();
        assert_eq!(sequences, expected);
        assert_eq!(pdb.sequence(), 2 + data.len() as u32);
    }

    #[test]
    fn a_file_without_rows_holds_the_fixed_tables() {
        let pdb = written(Vec::new());
        assert_eq!(rows_of(&pdb, ExportTable::Colors).len(), 8);
        // Colours and columns: one data page each, at 14 and 34. With 4096-byte pages
        // the columns fit one page; at 512 they take more, from page 41.
        assert_eq!(pdb.page(14).map(Page::raw_type), Some(6));
        assert_eq!(pdb.page(34).map(Page::raw_type), Some(16));
        // Tables 17 and 18: rekordbox's rows, one page each at 36 and 38.
        for (t, rows) in [(17, table_17_rows()), (18, table_18_rows())] {
            assert_eq!(pdb.page(2 * t + 2).map(Page::raw_type), Some(t));
            let written: Vec<Vec<u8>> = pdb
                .table_pages(ExportTable::from_u32(t))
                .flat_map(|page| page.rows())
                .map(|raw| raw.bytes()[..8].to_vec())
                .collect();
            let fixed: Vec<Vec<u8>> = rows.into_iter().map(|row| row.bytes).collect();
            assert_eq!(written, fixed, "table {t}");
        }
        let bytes = write_pdb(4096, Vec::new()).unwrap().to_bytes().unwrap();
        let pdb = Pdb::from_bytes(&bytes).unwrap();
        // The last table page is history's index page, 39; its empty candidate, 40, and
        // those of colours (41), columns (42) and tables 17 and 18 (43, 44) are past the
        // end.
        assert_eq!(pdb.page_count(), 40);
        assert_eq!(pdb.next_unused_page(), 45);
        assert_eq!(pdb.sequence(), 6);
    }

    #[test]
    fn the_header_is_a_fresh_exports() {
        // `unknown2` is 5 in a fresh rekordbox export and its `exportExt.pdb`; the old stick
        // has 1, which decoding keeps.
        let pdb = written(Vec::new());
        assert_eq!((pdb.unknown1(), pdb.unknown2(), pdb.unknown3()), (0, 5, 0));
        assert!(pdb.header_tail().iter().all(|&b| b == 0));
    }

    #[test]
    fn the_file_sequence_follows_the_highest_page_sequence() {
        let pdb = Pdb::from_bytes(&minimal_file()).unwrap();
        // The index page has sequence 0x11, as has the data page.
        let pages = [pdb.page(1).unwrap(), pdb.page(2).unwrap()];
        assert_eq!(file_sequence(pages), 0x12);
        assert_eq!(file_sequence([]), 1);
    }

    #[test]
    fn next_unused_page_is_past_the_file_and_every_empty_candidate() {
        let tables = [TablePointer::new(0, 7, 1, 2), TablePointer::new(1, 3, 4, 4)];
        assert_eq!(next_unused_page(5, &tables), 8);
        assert_eq!(next_unused_page(9, &tables), 9);
        assert_eq!(next_unused_page(1, &[]), 1);
    }
}
