//! The whole `export.pdb` file: the file header on page 0 and every other page.

use std::collections::HashSet;

use super::ExportTable;
use super::page::{HEAP_START, NEXT_PAGE_AT, Page};
use super::table::{TABLE_POINTER_LEN, TablePointer};
use crate::bytes::{Reader, Writer};
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Offset of `len_page` in the file header.
const LEN_PAGE_AT: u64 = 0x04;
/// Offset of `num_tables` in the file header.
const NUM_TABLES_AT: u64 = 0x08;
/// Offset of the first table pointer in the file header.
const TABLES_AT: usize = 0x1c;
/// Offset of `first_page` within a table pointer.
const FIRST_PAGE_FIELD: usize = 8;
/// Smallest `len_page` accepted: room for a data page's header, so every page can at least
/// hold its fixed header.
const MIN_LEN_PAGE: usize = HEAP_START;

/// A whole `export.pdb`, as stored.
///
/// Decoding then encoding an unchanged value gives back identical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Pdb {
    unknown1: u32,
    len_page: u32,
    next_unused_page: u32,
    unknown2: u32,
    sequence: u32,
    unknown3: u32,
    tables: Vec<TablePointer>,
    header_tail: Vec<u8>,
    /// Pages 1 onwards; page 0 is the file header.
    pages: Vec<Slot>,
}

/// A page after page 0: one that a table chain reaches, or one no chain reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    Chained(Page),
    Unchained(Vec<u8>),
}

impl Pdb {
    /// Decodes a whole `export.pdb`, following every table's page chain.
    ///
    /// Table pointers are not checked themselves (`empty_candidate` points past the end of
    /// real files); only pages a chain needs must exist.
    ///
    /// # Errors
    ///
    /// A [`DecodeError`] at an absolute offset:
    /// - [`UnexpectedEof`](DecodeErrorKind::UnexpectedEof) at a header field cut off by the
    ///   end of the input, or at the start of a trailing partial page when the file length
    ///   is not a multiple of `len_page`;
    /// - [`InvalidValue`](DecodeErrorKind::InvalidValue) at `len_page` when it is below
    ///   0x28, at `num_tables` when the table pointers overrun page 0, at the `first_page` or
    ///   `next_page` that names page 0 or a page past the end, at the row counts when the
    ///   row index overruns a data page, and at a present row's offset when it points
    ///   outside the heap;
    /// - [`PageChainCycle`](DecodeErrorKind::PageChainCycle) at the `next_page` that leads
    ///   back to a page already in the chain.
    pub fn from_bytes(data: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(data);
        let unknown1 = r.u32_le().context("pdb unknown1")?;
        let len_page = r.u32_le().context("pdb len_page")?;
        let num_tables = r.u32_le().context("pdb num_tables")?;
        let next_unused_page = r.u32_le().context("pdb next_unused_page")?;
        let unknown2 = r.u32_le().context("pdb unknown2")?;
        let sequence = r.u32_le().context("pdb sequence")?;
        let unknown3 = r.u32_le().context("pdb unknown3")?;

        let page_len = usize::try_from(len_page).unwrap_or(usize::MAX);
        if page_len < MIN_LEN_PAGE {
            return Err(invalid(LEN_PAGE_AT, "pdb len_page"));
        }
        let partial = data.len() % page_len;
        if partial != 0 {
            return Err(DecodeError::new(
                offset(data.len() - partial),
                "pdb file length",
                DecodeErrorKind::UnexpectedEof {
                    needed: page_len,
                    available: partial,
                },
            ));
        }
        let tables_end = u64::from(num_tables)
            .saturating_mul(TABLE_POINTER_LEN as u64)
            .saturating_add(TABLES_AT as u64);
        if tables_end > u64::from(len_page) {
            return Err(invalid(NUM_TABLES_AT, "pdb num_tables"));
        }

        // Fits: the file holds at least page 0, as its length is a non-zero multiple.
        let mut header = Reader::new(&data[..page_len]);
        header.seek(r.position()).context("pdb header")?;
        let mut tables = Vec::new();
        for _ in 0..num_tables {
            tables.push(TablePointer::new(
                header.u32_le().context("pdb table type")?,
                header.u32_le().context("pdb table empty_candidate")?,
                header.u32_le().context("pdb table first_page")?,
                header.u32_le().context("pdb table last_page")?,
            ));
        }
        let header_tail = header.take_rest().to_vec();

        let pages: Vec<&[u8]> = data.chunks_exact(page_len).collect();
        let mut chained: Vec<Option<Page>> = vec![None; pages.len()];
        for (t, table) in tables.iter().enumerate() {
            follow_chain(t, table, &pages, page_len, &mut chained)?;
        }
        let pages = chained
            .into_iter()
            .zip(pages)
            .skip(1)
            .map(|(page, bytes)| match page {
                Some(page) => Slot::Chained(page),
                None => Slot::Unchained(bytes.to_vec()),
            })
            .collect();
        Ok(Self {
            unknown1,
            len_page,
            next_unused_page,
            unknown2,
            sequence,
            unknown3,
            tables,
            header_tail,
            pages,
        })
    }

    /// Encodes the whole file.
    ///
    /// # Errors
    ///
    /// None yet; the result leaves room for the checks a file built from scratch needs.
    pub fn to_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let mut w = Writer::new();
        w.u32_le(self.unknown1);
        w.u32_le(self.len_page);
        let num_tables =
            u32::try_from(self.tables.len()).map_err(|_| EncodeError::LengthOverflow {
                context: "pdb num_tables",
            })?;
        w.u32_le(num_tables);
        w.u32_le(self.next_unused_page);
        w.u32_le(self.unknown2);
        w.u32_le(self.sequence);
        w.u32_le(self.unknown3);
        for table in &self.tables {
            w.u32_le(table.raw_type());
            w.u32_le(table.empty_candidate());
            w.u32_le(table.first_page());
            w.u32_le(table.last_page());
        }
        w.bytes(&self.header_tail);
        for slot in &self.pages {
            match slot {
                Slot::Chained(page) => page.encode(&mut w)?,
                Slot::Unchained(bytes) => w.bytes(bytes),
            }
        }
        Ok(w.into_inner())
    }

    /// The first `u32` of the file, 0 in real files.
    pub fn unknown1(&self) -> u32 {
        self.unknown1
    }

    /// The length of every page in bytes, 4,096 in real files.
    pub fn len_page(&self) -> u32 {
        self.len_page
    }

    /// The `next_unused_page` field, as stored. It may point past the end of the file.
    pub fn next_unused_page(&self) -> u32 {
        self.next_unused_page
    }

    /// The `u32` at 0x10, not understood.
    pub fn unknown2(&self) -> u32 {
        self.unknown2
    }

    /// The `sequence` field.
    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    /// The `u32` at 0x18, not understood.
    pub fn unknown3(&self) -> u32 {
        self.unknown3
    }

    /// The table pointers, in file order.
    pub fn tables(&self) -> &[TablePointer] {
        &self.tables
    }

    /// The rest of page 0 after the table pointers, kept verbatim. Zero in real files.
    pub fn header_tail(&self) -> &[u8] {
        &self.header_tail
    }

    /// The number of pages in the file, page 0 included.
    pub fn page_count(&self) -> usize {
        self.pages.len().saturating_add(1)
    }

    /// The page at position `index`, if a table chain reaches it. `None` for page 0, for
    /// pages no chain reaches and for positions past the end of the file.
    pub fn page(&self, index: u32) -> Option<&Page> {
        match self.slot(index)? {
            Slot::Chained(page) => Some(page),
            Slot::Unchained(_) => None,
        }
    }

    /// The bytes of the page at position `index`, if no table chain reaches it. Such pages
    /// are zero-filled in real files.
    pub fn unchained_page(&self, index: u32) -> Option<&[u8]> {
        match self.slot(index)? {
            Slot::Chained(_) => None,
            Slot::Unchained(bytes) => Some(bytes),
        }
    }

    /// The pages of the first `export.pdb` table of type `table`; empty when there is no
    /// such table. A convenience over [`pages_of`](Self::pages_of), which works for any
    /// schema, including `exportExt.pdb`, whose type numbers mean other tables.
    pub fn table_pages(&self, table: ExportTable) -> impl Iterator<Item = &Page> + '_ {
        self.tables
            .iter()
            .find(|pointer| pointer.raw_type() == table.to_u32())
            .into_iter()
            .flat_map(|pointer| self.pages_of(pointer))
    }

    /// The pages of `table`, following its chain from `first_page` to `last_page`.
    ///
    /// For a pointer of this file, decoding checked that the chain reaches `last_page`.
    /// For any other pointer the iterator stops at the first page that is not chained, and
    /// after at most [`page_count`](Self::page_count) pages.
    pub fn pages_of(&self, table: &TablePointer) -> impl Iterator<Item = &Page> + '_ {
        let mut next = Some(table.first_page());
        let last = table.last_page();
        (0..self.page_count()).map_while(move |_| {
            let index = next?;
            let page = self.page(index)?;
            next = (index != last).then(|| page.next_page());
            Some(page)
        })
    }

    fn slot(&self, index: u32) -> Option<&Slot> {
        let index = usize::try_from(index).ok()?.checked_sub(1)?;
        self.pages.get(index)
    }
}

/// Decodes the pages of table number `t`, from `first_page` along `next_page` until
/// `last_page`, into `chained`. A page already decoded by another table is not decoded
/// again.
///
/// Errors are located at the field that names the failing page: the table pointer's
/// `first_page`, or the previous page's `next_page`.
fn follow_chain(
    t: usize,
    table: &TablePointer,
    pages: &[&[u8]],
    page_len: usize,
    chained: &mut [Option<Page>],
) -> Result<(), DecodeError> {
    let mut index = table.first_page();
    let mut named_at = offset(TABLES_AT + TABLE_POINTER_LEN * t + FIRST_PAGE_FIELD);
    let mut named_by = "pdb table first_page";
    let mut visited = HashSet::new();
    loop {
        // Page 0 is the file header, never a table page.
        let at = usize::try_from(index)
            .ok()
            .filter(|&at| at != 0 && at < pages.len())
            .ok_or_else(|| invalid(named_at, named_by))?;
        if !visited.insert(index) {
            return Err(DecodeError::new(
                named_at,
                named_by,
                DecodeErrorKind::PageChainCycle { page: index },
            ));
        }
        let base = offset(at.saturating_mul(page_len));
        let page = match &mut chained[at] {
            Some(page) => page,
            empty => empty.insert(Page::decode(pages[at], base)?),
        };
        if index == table.last_page() {
            return Ok(());
        }
        index = page.next_page();
        named_at = base.saturating_add(NEXT_PAGE_AT);
        named_by = "pdb page next_page";
    }
}

fn invalid(offset: u64, context: &'static str) -> DecodeError {
    DecodeError::new(offset, context, DecodeErrorKind::InvalidValue)
}

fn offset(at: usize) -> u64 {
    u64::try_from(at).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::super::page::PageBody;
    use super::super::testutil::*;
    use super::*;

    fn decode(bytes: &[u8]) -> Pdb {
        Pdb::from_bytes(bytes).expect("decodes")
    }

    fn assert_round_trip(bytes: &[u8]) -> Pdb {
        let pdb = decode(bytes);
        assert_eq!(pdb.to_bytes().expect("encodes"), bytes);
        pdb
    }

    fn page_indices<'a>(pages: impl Iterator<Item = &'a Page>) -> Vec<u32> {
        pages.map(Page::page_index).collect()
    }

    #[test]
    fn minimal_file_decodes_and_round_trips() {
        let bytes = minimal_file();
        let pdb = assert_round_trip(&bytes);

        assert_eq!(pdb.unknown1(), 0);
        assert_eq!(pdb.len_page(), LEN as u32);
        assert_eq!(pdb.next_unused_page(), NEXT_UNUSED);
        assert_eq!(pdb.unknown2(), 1);
        assert_eq!(pdb.sequence(), SEQUENCE);
        assert_eq!(pdb.unknown3(), 0);
        assert_eq!(pdb.header_tail().len(), LEN - 0x1c - 16);
        assert_eq!(pdb.page_count(), 3);

        let [table] = pdb.tables() else {
            panic!("one table");
        };
        assert_eq!(table.raw_type(), 2);
        assert_eq!((table.first_page(), table.last_page()), (1, 2));
        assert_eq!(table.empty_candidate(), 3);

        assert_eq!(page_indices(pdb.table_pages(ExportTable::Artists)), [1, 2]);
        assert_eq!(pdb.table_pages(ExportTable::Tracks).count(), 0);

        let index = pdb.page(1).expect("page 1");
        assert!(matches!(index.body(), PageBody::Index(_)));
        assert_eq!(index.page_flags(), 0x64);
        assert_eq!(index.rows().count(), 0);

        let data = pdb.page(2).expect("page 2");
        assert_eq!(data.raw_type(), 2);
        assert_eq!(data.next_page(), 3);
        assert_eq!(data.num_row_offsets(), 2);
        assert_eq!(data.num_rows(), 1);
        let PageBody::Data(body) = data.body() else {
            panic!("data page");
        };
        // One group holding 2 offsets: 2 × 2 bytes plus `rowpf` and `tranrf`.
        assert_eq!(body.heap().len(), LEN - 0x28 - 8);
        assert_eq!(body.row_groups().len(), 1);
        assert_eq!(body.row_groups()[0].offsets(), [0, 8]);
        assert_eq!(body.row_groups()[0].rowpf(), 0b01);

        let rows: Vec<_> = data.rows().collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].offset(), 0);
        assert_eq!(rows[0].bytes(), body.heap());
        assert!(pdb.page(0).is_none() && pdb.page(3).is_none());
    }

    #[test]
    fn data_header_fields_are_kept() {
        let pdb = decode(&minimal_file());
        let data = pdb.page(2).unwrap();
        assert_eq!(data.unknown1(), 0);
        assert_eq!(data.sequence(), 0x11);
        assert_eq!(data.unknown2(), 0x22);
        assert_eq!(data.free_size(), 0x33);
        assert_eq!(data.used_size(), 16);
        let PageBody::Data(body) = data.body() else {
            panic!("data page");
        };
        assert_eq!(
            (
                body.transaction_row_count(),
                body.transaction_row_index(),
                body.unknown3(),
                body.unknown4()
            ),
            (3, 5, 7, 9)
        );
        assert_eq!(body.row_groups()[0].tranrf(), 0x1234);
    }

    #[test]
    fn chains_are_followed_by_next_page_and_unchained_pages_are_kept() {
        let bytes = chained_file();
        let pdb = assert_round_trip(&bytes);
        assert_eq!(pdb.page_count(), 6);
        assert_eq!(
            page_indices(pdb.table_pages(ExportTable::Tracks)),
            [3, 1, 4]
        );
        assert_eq!(page_indices(pdb.table_pages(ExportTable::from_u32(9))), [2]);
        assert_eq!(pdb.unchained_page(5), Some(&[0xa5; LEN][..]));
        assert!(pdb.page(5).is_none());
        assert!(pdb.unchained_page(1).is_none());
        assert!(pdb.unchained_page(0).is_none() && pdb.unchained_page(6).is_none());
    }

    #[test]
    fn index_page_body_is_kept_verbatim() {
        let pdb = decode(&minimal_file());
        let PageBody::Index(body) = pdb.page(1).unwrap().body() else {
            panic!("index page");
        };
        assert_eq!(body.bytes(), &index_page(1, 2, 2)[0x20..]);
    }

    #[test]
    fn a_second_row_group_holds_rows_16_and_up() {
        let rows: Vec<(u16, bool)> = (0..18).map(|i| (i * 4, i % 3 != 1)).collect();
        let page = data_page(1, 0, 2, &[0x41; 72], &rows);
        let bytes = file(&[[0, 2, 1, 1]], &[page]);
        let pdb = assert_round_trip(&bytes);
        let data = pdb.page(1).unwrap();
        let PageBody::Data(body) = data.body() else {
            panic!("data page");
        };
        assert_eq!(body.row_groups().len(), 2);
        // A full group of 36 bytes, then a group of 2 offsets.
        assert_eq!(body.heap().len(), LEN - 0x28 - 36 - 8);
        assert_eq!(body.row_groups()[0].offsets().len(), 16);
        assert_eq!(body.row_groups()[1].offsets(), [64, 68]);
        let offsets: Vec<u16> = data.rows().map(|row| row.offset()).collect();
        let expected: Vec<u16> = rows
            .iter()
            .filter(|(_, present)| *present)
            .map(|(offset, _)| *offset)
            .collect();
        assert_eq!(offsets, expected);
        assert_eq!(data.num_row_offsets(), 18);
        assert_eq!(usize::from(data.num_rows()), expected.len());
    }

    #[test]
    fn heap_bytes_after_used_size_survive() {
        let mut heap = vec![1, 2, 3, 4];
        let mut page = data_page(1, 0, 2, &heap, &[(0, true)]);
        // Non-zero bytes after `used_size` (4) and before the row index.
        page[0x28 + 4..0x28 + 10].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef, 0, 7]);
        let bytes = file(&[[0, 2, 1, 1]], &[page]);
        let pdb = assert_round_trip(&bytes);
        let PageBody::Data(body) = pdb.page(1).unwrap().body() else {
            panic!("data page");
        };
        heap.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef, 0, 7]);
        assert_eq!(body.heap()[..10], heap[..]);
    }

    fn decode_err(bytes: &[u8]) -> (u64, &'static str, DecodeErrorKind) {
        match Pdb::from_bytes(bytes) {
            Ok(_) => panic!("decoded, expected an error"),
            Err(err) => (err.offset, err.context, err.kind),
        }
    }

    fn invalid(offset: usize, context: &'static str) -> (u64, &'static str, DecodeErrorKind) {
        (offset as u64, context, DecodeErrorKind::InvalidValue)
    }

    fn set_u32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// Offset of field `field` (0 type, 1 empty_candidate, 2 first, 3 last) of table `t`.
    fn table_field(t: usize, field: usize) -> usize {
        0x1c + 16 * t + 4 * field
    }

    #[test]
    fn len_page_zero_or_below_the_data_header_is_invalid() {
        for len_page in [0, 1, 0x1c, 0x27] {
            let mut bytes = minimal_file();
            set_u32(&mut bytes, 4, len_page);
            assert_eq!(decode_err(&bytes), invalid(4, "pdb len_page"), "{len_page}");
        }
    }

    #[test]
    fn smallest_len_page_decodes() {
        let mut bytes = header_page(&[]);
        set_u32(&mut bytes, 4, 0x28);
        bytes.truncate(0x28);
        let pdb = assert_round_trip(&bytes);
        assert_eq!(pdb.header_tail(), [0; 0x0c]);
    }

    #[test]
    fn file_length_not_a_multiple_of_len_page_is_eof_at_the_partial_page() {
        let mut bytes = minimal_file();
        bytes.extend_from_slice(&[0; 5]);
        assert_eq!(
            decode_err(&bytes),
            (
                3 * LEN as u64,
                "pdb file length",
                DecodeErrorKind::UnexpectedEof {
                    needed: LEN,
                    available: 5,
                }
            )
        );
        let mut bytes = header_page(&[]);
        bytes.truncate(100);
        assert_eq!(
            decode_err(&bytes),
            (
                0,
                "pdb file length",
                DecodeErrorKind::UnexpectedEof {
                    needed: LEN,
                    available: 100,
                }
            )
        );
    }

    #[test]
    fn truncated_file_header_is_eof_at_the_field() {
        assert_eq!(
            decode_err(&minimal_file()[..10]),
            (
                8,
                "pdb num_tables",
                DecodeErrorKind::UnexpectedEof {
                    needed: 4,
                    available: 2,
                }
            )
        );
    }

    /// Most tables that fit page 0.
    const MAX_TABLES: usize = (LEN - 0x1c) / 16;

    #[test]
    fn num_tables_overrunning_page_zero_is_invalid() {
        for num_tables in [MAX_TABLES as u32 + 1, u32::MAX] {
            let mut bytes = minimal_file();
            set_u32(&mut bytes, 8, num_tables);
            assert_eq!(decode_err(&bytes), invalid(8, "pdb num_tables"));
        }
    }

    #[test]
    fn num_tables_filling_page_zero_decodes() {
        let tables = vec![[9, 2, 1, 1]; MAX_TABLES];
        let bytes = file(&tables, &[index_page(1, 9, 2)]);
        let pdb = assert_round_trip(&bytes);
        assert_eq!(pdb.tables().len(), MAX_TABLES);
        assert_eq!(pdb.header_tail().len(), (LEN - 0x1c) % 16);
    }

    #[test]
    fn first_page_out_of_range_is_invalid_at_the_table_pointer() {
        for first in [0, 3, u32::MAX] {
            let mut bytes = minimal_file();
            set_u32(&mut bytes, table_field(0, 2), first);
            assert_eq!(
                decode_err(&bytes),
                invalid(table_field(0, 2), "pdb table first_page"),
                "{first}"
            );
        }
    }

    #[test]
    fn next_page_out_of_range_is_invalid_at_the_next_page_field() {
        for next in [0, 3, 7] {
            let mut bytes = minimal_file();
            set_u32(&mut bytes, LEN + 0x0c, next);
            assert_eq!(
                decode_err(&bytes),
                invalid(LEN + 0x0c, "pdb page next_page"),
                "{next}"
            );
        }
    }

    #[test]
    fn page_chain_cycle_is_reported_at_the_next_page_that_closes_it() {
        let bytes = file(
            &[[0, 9, 1, 3]],
            &[
                index_page(1, 0, 2),
                index_page(2, 0, 1),
                index_page(3, 0, 9),
            ],
        );
        assert_eq!(
            decode_err(&bytes),
            (
                2 * LEN as u64 + 0x0c,
                "pdb page next_page",
                DecodeErrorKind::PageChainCycle { page: 1 }
            )
        );
        let bytes = file(&[[0, 9, 1, 2]], &[index_page(1, 0, 1), index_page(2, 0, 9)]);
        assert_eq!(
            decode_err(&bytes),
            (
                LEN as u64 + 0x0c,
                "pdb page next_page",
                DecodeErrorKind::PageChainCycle { page: 1 }
            )
        );
    }

    #[test]
    fn a_page_in_two_chains_is_not_a_cycle() {
        let bytes = file(
            &[[0, 9, 1, 2], [1, 9, 2, 2]],
            &[index_page(1, 0, 2), data_page(2, 0, 9, b"x", &[(0, true)])],
        );
        let pdb = assert_round_trip(&bytes);
        assert_eq!(page_indices(pdb.table_pages(ExportTable::Genres)), [2]);
    }

    /// Heap length of a data page with two row offsets.
    const HEAP_LEN: usize = LEN - 0x28 - 8;

    #[test]
    fn a_row_may_start_where_a_full_last_group_would_be() {
        // As on page 80 of the real stick: the last group holds only its own offsets, and
        // the heap runs on into the space a full group of 16 would take.
        let at = (LEN - 0x28 - 36) as u16;
        let mut page = data_page(1, 0, 2, &[], &[(0, true), (at, true)]);
        page[0x28 + usize::from(at)..0x28 + usize::from(at) + 4].copy_from_slice(b"row1");
        let bytes = file(&[[0, 2, 1, 1]], &[page]);
        let pdb = assert_round_trip(&bytes);
        let rows: Vec<_> = pdb.page(1).unwrap().rows().collect();
        assert_eq!(rows[1].offset(), at);
        // The heap ends 8 bytes before the page end: 36 − 8 bytes from `at`.
        assert_eq!(rows[1].bytes().len(), 28);
        assert_eq!(rows[1].bytes()[..4], *b"row1");
    }

    #[test]
    fn present_row_offset_outside_the_heap_is_invalid_at_the_offset() {
        let page = data_page(
            1,
            0,
            2,
            &[],
            &[(HEAP_LEN as u16 - 1, true), (HEAP_LEN as u16, true)],
        );
        let bytes = file(&[[0, 2, 1, 1]], &[page]);
        // Row 1 of group 0: block end − 6 − 2.
        assert_eq!(decode_err(&bytes), invalid(2 * LEN - 8, "pdb row offset"));
    }

    #[test]
    fn absent_row_offset_outside_the_heap_is_kept() {
        let page = data_page(1, 0, 2, &[], &[(u16::MAX, false), (0, true)]);
        let bytes = file(&[[0, 2, 1, 1]], &[page]);
        let pdb = assert_round_trip(&bytes);
        assert_eq!(pdb.page(1).unwrap().rows().count(), 1);
    }

    #[test]
    fn row_index_overrunning_the_heap_is_invalid_at_the_row_counts() {
        // 13 full groups and one of 1 offset (474 bytes) do not fit the 472 bytes after the
        // data header.
        // Guards the test helper `row_index_len`, so the page below really overruns.
        assert_eq!(row_index_len(13 * 16 + 1), 474);
        let mut page = page_header(1, 0, 2, 13 * 16 + 1, 0, 0x34, 0, 0);
        page.resize(LEN, 0);
        let bytes = file(&[[0, 2, 1, 1]], &[page]);
        assert_eq!(
            decode_err(&bytes),
            invalid(LEN + 0x18, "pdb page num_row_offsets")
        );
        // 13 full groups (468 bytes) fit.
        let mut page = page_header(1, 0, 2, 13 * 16, 0, 0x34, 0, 0);
        page.resize(LEN, 0);
        assert_round_trip(&file(&[[0, 2, 1, 1]], &[page]));
    }

    fn data_body(pdb: &Pdb, index: u32) -> &super::super::page::DataBody {
        match pdb.page(index).unwrap().body() {
            PageBody::Data(body) => body,
            PageBody::Index(_) => panic!("data page"),
        }
    }

    #[test]
    fn pages_of_follows_the_given_table_even_when_types_repeat() {
        let bytes = file(
            &[[9, 7, 2, 2], [9, 8, 1, 3]],
            &[
                index_page(1, 9, 3),
                index_page(2, 9, 7),
                data_page(3, 9, 8, b"x", &[(0, true)]),
            ],
        );
        let pdb = assert_round_trip(&bytes);
        let [first, second] = pdb.tables() else {
            panic!("two tables");
        };
        assert_eq!(page_indices(pdb.pages_of(first)), [2]);
        assert_eq!(page_indices(pdb.pages_of(second)), [1, 3]);
    }

    #[test]
    fn exactly_16_offsets_make_one_full_group() {
        let rows: Vec<(u16, bool)> = (0..16).map(|i| (i * 2, true)).collect();
        let bytes = file(&[[0, 2, 1, 1]], &[data_page(1, 0, 2, &[0x41; 32], &rows)]);
        let pdb = assert_round_trip(&bytes);
        let body = data_body(&pdb, 1);
        assert_eq!(body.heap().len(), LEN - 0x28 - 36);
        assert_eq!(body.row_groups().len(), 1);
        assert_eq!(body.row_groups()[0].offsets().len(), 16);
        assert_eq!(pdb.page(1).unwrap().rows().count(), 16);
    }

    #[test]
    fn zero_offsets_leave_the_whole_page_to_the_heap() {
        let bytes = file(&[[0, 2, 1, 1]], &[data_page(1, 0, 2, b"x", &[])]);
        let pdb = assert_round_trip(&bytes);
        let body = data_body(&pdb, 1);
        assert_eq!(body.heap().len(), LEN - 0x28);
        assert!(body.row_groups().is_empty());
        assert_eq!(pdb.page(1).unwrap().rows().count(), 0);
    }

    #[test]
    fn present_offset_outside_the_heap_in_group_1_is_invalid_at_the_offset() {
        // 18 rows: group 0 is full (36 bytes at the page end), group 1 holds rows 16 and
        // 17. Row 17's offset is past the heap.
        let heap_len = (LEN - 0x28 - 36 - 8) as u16;
        let mut rows: Vec<(u16, bool)> = (0..17).map(|i| (i, true)).collect();
        rows.push((heap_len, true));
        let bytes = file(&[[0, 2, 1, 1]], &[data_page(1, 0, 2, &[], &rows)]);
        // Group 1's block ends 36 bytes before the page end; row 1 of it is at end − 6 − 2.
        assert_eq!(
            decode_err(&bytes),
            invalid(2 * LEN - 36 - 8, "pdb row offset")
        );
    }

    #[test]
    fn row_string_errors_are_located_in_the_file() {
        // Page 4 of the chained file holds one row at heap offset 0: "four".
        let pdb = decode(&chained_file());
        let row = pdb.page(4).unwrap().rows().next().unwrap();
        // b'r' (0x72) is not a string flag.
        let k = 3;
        let err = row.string_at(k).unwrap_err();
        assert_eq!(
            (err.offset, err.kind),
            ((4 * LEN + 0x28 + k) as u64, DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn table_pointers_past_the_file_end_decode() {
        let page = data_page(1, 0, 1000, &[], &[]);
        let bytes = file(&[[0, 1000, 1, 1]], &[page]);
        let pdb = assert_round_trip(&bytes);
        assert_eq!(pdb.tables()[0].empty_candidate(), 1000);
    }

    mod properties {
        use proptest::collection::vec;
        use proptest::prelude::*;

        use super::*;

        /// Rows of a data page, with offsets inside the smallest heap the builder makes.
        fn rows() -> impl Strategy<Value = Vec<(u16, bool)>> {
            vec((0u16..300, any::<bool>()), 0..40)
        }

        /// A valid file: one table of an index page and 0 to 3 data pages, then 0 to 2
        /// unchained pages of arbitrary bytes.
        fn valid_file() -> impl Strategy<Value = Vec<u8>> {
            (
                any::<u32>(),
                vec((vec(any::<u8>(), 0..64), rows()), 0..4),
                vec(vec(any::<u8>(), LEN), 0..3),
            )
                .prop_map(|(kind, data, unchained)| {
                    let last = data.len() as u32 + 1;
                    let empty = last + 1;
                    let mut pages = vec![index_page(1, kind, if last == 1 { empty } else { 2 })];
                    for (i, (heap, rows)) in data.iter().enumerate() {
                        let index = i as u32 + 2;
                        let next = if index == last { empty } else { index + 1 };
                        pages.push(data_page(index, kind, next, heap, rows));
                    }
                    pages.extend(unchained);
                    file(&[[kind, empty, 1, last]], &pages)
                })
        }

        /// Bytes with a plausible file header (small `len_page`, a few small table
        /// pointers) and pages that are arbitrary except for small row counts and
        /// index or data flags.
        fn plausible_file() -> impl Strategy<Value = Vec<u8>> {
            (
                0x28usize..0xa0,
                vec([0u32..6, 0..6, 0..6, 0..6], 0..3),
                1usize..6,
            )
                .prop_flat_map(|(len_page, tables, count)| {
                    let page = (
                        vec(any::<u8>(), len_page),
                        0u32..40,
                        prop_oneof![Just(0x24u8), Just(0x34), Just(0x64), any::<u8>()],
                        0u32..6,
                    );
                    (Just((len_page, tables)), vec(page, count))
                })
                .prop_map(|((len_page, tables), pages)| {
                    let mut out = Vec::new();
                    for (i, (mut bytes, rows, flags, next)) in pages.into_iter().enumerate() {
                        if i == 0 {
                            let n = tables.len().min((len_page - 0x1c) / 16);
                            set_u32(&mut bytes, 0, 0);
                            set_u32(&mut bytes, 4, len_page as u32);
                            set_u32(&mut bytes, 8, n as u32);
                            for (t, table) in tables.iter().take(n).enumerate() {
                                for (f, value) in table.iter().enumerate() {
                                    set_u32(&mut bytes, table_field(t, f), *value);
                                }
                            }
                        } else {
                            set_u32(&mut bytes, 0x0c, next);
                            bytes[0x18..0x1b].copy_from_slice(&rows.to_le_bytes()[..3]);
                            bytes[0x1b] = flags;
                        }
                        out.extend(bytes);
                    }
                    out
                })
        }

        fn assert_re_encodes(bytes: &[u8]) -> Result<(), TestCaseError> {
            if let Ok(pdb) = Pdb::from_bytes(bytes) {
                prop_assert_eq!(pdb.to_bytes().unwrap(), bytes);
            }
            Ok(())
        }

        proptest! {
            #[test]
            fn valid_files_round_trip(bytes in valid_file()) {
                let pdb = Pdb::from_bytes(&bytes).unwrap();
                prop_assert_eq!(pdb.to_bytes().unwrap(), bytes);
            }

            #[test]
            fn from_bytes_never_panics_on_arbitrary_bytes(bytes in vec(any::<u8>(), 0..1024)) {
                assert_re_encodes(&bytes)?;
            }

            #[test]
            fn from_bytes_never_panics_on_plausible_files(bytes in plausible_file()) {
                assert_re_encodes(&bytes)?;
            }

            #[test]
            fn corrupting_one_byte_never_panics(
                bytes in valid_file(),
                index in any::<prop::sample::Index>(),
                value in any::<u8>(),
            ) {
                let mut bytes = bytes;
                let i = index.index(bytes.len());
                bytes[i] = value;
                assert_re_encodes(&bytes)?;
            }
        }
    }
}
