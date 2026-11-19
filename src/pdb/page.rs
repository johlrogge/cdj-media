//! Pages: the common page header, data pages with their heap and row index, and index
//! pages, which are kept verbatim. See [`Page`] for the layout.

use crate::bytes::{Reader, Writer};
use crate::encoding::devicesql::{self, DeviceSqlString};
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// `page_flags` bit marking an index page.
const FLAG_INDEX: u8 = 0x40;

/// Offset of a data page's heap: the common header plus 8 more header bytes.
pub(crate) const HEAP_START: usize = 0x28;
/// Length of a row group's `rowpf` and `tranrf`, after its offsets.
const GROUP_FLAGS_LEN: usize = 4;
/// Most rows per row group.
pub(crate) const ROWS_PER_GROUP: usize = 16;
/// Length of a full row group: 16 offsets, `rowpf` and `tranrf`.
const FULL_GROUP_LEN: usize = 2 * ROWS_PER_GROUP + GROUP_FLAGS_LEN;
/// Offset of `next_page` in the common page header.
pub(crate) const NEXT_PAGE_AT: u64 = 0x0c;
/// Offset of the packed `num_row_offsets` and `num_rows` in the common page header.
const ROW_COUNTS_AT: u64 = 0x18;
/// Largest `num_row_offsets`: 13 bits.
const MAX_ROW_OFFSETS: u16 = 0x1fff;
/// Largest `num_rows`: 11 bits.
const MAX_ROWS: u16 = 0x7ff;

/// One page of a table, as stored.
///
/// # Common header (32 bytes)
///
/// | bytes      | field                                                                |
/// |------------|----------------------------------------------------------------------|
/// | 0x00..0x04 | unknown, 0 in real files                                             |
/// | 0x04..0x08 | `page_index`                                                         |
/// | 0x08..0x0c | `type`                                                               |
/// | 0x0c..0x10 | `next_page`                                                          |
/// | 0x10..0x14 | `sequence`                                                           |
/// | 0x14..0x18 | unknown                                                              |
/// | 0x18..0x1b | 24 bits: low 13 `num_row_offsets`, high 11 `num_rows`                |
/// | 0x1b       | `page_flags`: bit 6 index page, bit 4 holds deleted rows             |
/// | 0x1c..0x1e | `free_size`                                                          |
/// | 0x1e..0x20 | `used_size`                                                          |
///
/// The fields are stored verbatim, including `num_rows`, which equals the number of present
/// rows in every real page but is not proven derived (ADR-004), and `free_size` and
/// `used_size`, which are not checked against the heap. An index page's bytes after the
/// header are kept verbatim ([`IndexBody`]).
///
/// # Data page
///
/// | bytes                 | field                                                     |
/// |-----------------------|-----------------------------------------------------------|
/// | 0x20..0x28            | `transaction_row_count`, `transaction_row_index`, 2 × u16 |
/// | 0x28..index start     | heap: the rows, then zero in real files                   |
/// | index start..len_page | row index, growing backward from the page end             |
///
/// The row index holds `num_row_offsets` offsets in groups of up to 16, group 0 at the very
/// end of the page. A group is its offsets (row `i` of the group at block end − 6 − 2i, so
/// row 0 is nearest the end), then `rowpf` and `tranrf`. Every group but the last holds 16
/// offsets (36 bytes); the last holds only the remaining offsets, so it is
/// `4 + 2 × (num_row_offsets − 16 × (groups − 1))` bytes long, and the heap runs on into the
/// space a full group would take. That is what the real-stick fixture shows: with it,
/// `free_size + used_size` equals the heap length on every one of its 292 data pages, and
/// the heap is zero after `used_size` on all of them. Offsets are relative to 0x28.
///
/// Pages whose `num_row_offsets` is a non-zero multiple of 16 are observed (their last
/// group is full). A data page with no row offsets at all, whose heap is then the whole
/// page after 0x28, is extrapolated, not observed: real empty tables are a lone index page.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Page {
    unknown1: u32,
    page_index: u32,
    page_type: u32,
    next_page: u32,
    sequence: u32,
    unknown2: u32,
    num_row_offsets: u16,
    num_rows: u16,
    page_flags: u8,
    free_size: u16,
    used_size: u16,
    body: PageBody,
    /// Where the page was decoded from in the file, so row errors can be absolute. Not
    /// encoded; 0 for a page not decoded from a file.
    file_offset: u64,
}

/// What follows the common page header.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PageBody {
    /// A data page: rows in a heap, located by a row index.
    Data(DataBody),
    /// An index page (`page_flags` bit 6 set), kept verbatim.
    Index(IndexBody),
}

/// The part of a data page after the common header.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DataBody {
    transaction_row_count: u16,
    transaction_row_index: u16,
    unknown3: u16,
    unknown4: u16,
    heap: Vec<u8>,
    row_groups: Vec<RowGroup>,
}

/// The part of an index page after the common header, not interpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IndexBody {
    bytes: Vec<u8>,
}

/// One group of up to 16 entries of a data page's row index.
///
/// Every offset is stored, including those of rows whose presence bit is clear. Only the
/// last group of a page holds fewer than 16 (see [`Page`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RowGroup {
    offsets: Vec<u16>,
    rowpf: u16,
    tranrf: u16,
}

/// The header fields of a built page that its body does not determine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "Database::to_pdb writes files from slice 7e on")
)]
pub(crate) struct PageHeader {
    pub(crate) page_index: u32,
    pub(crate) page_type: u32,
    pub(crate) next_page: u32,
    pub(crate) sequence: u32,
    pub(crate) page_flags: u8,
    pub(crate) free_size: u16,
    pub(crate) used_size: u16,
}

/// A present row of a data page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RawRow<'a> {
    offset: u16,
    bytes: &'a [u8],
    /// Absolute file offset of the row's first byte.
    at: u64,
}

impl Page {
    /// A data page from its header fields and body. `unknown1` and `unknown2` are 0, as in
    /// every real page; `num_row_offsets` counts the body's offsets and `num_rows` the set
    /// `rowpf` bits, as on every real page. Counts that do not fit their fields saturate
    /// here and fail to encode.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the page writer builds pages from slice 7d on")
    )]
    pub(crate) fn data(header: PageHeader, body: DataBody) -> Self {
        let offsets: usize = body.row_groups.iter().map(|g| g.offsets.len()).sum();
        let present: u32 = body.row_groups.iter().map(|g| g.rowpf.count_ones()).sum();
        Self {
            unknown1: 0,
            page_index: header.page_index,
            page_type: header.page_type,
            next_page: header.next_page,
            sequence: header.sequence,
            unknown2: 0,
            num_row_offsets: u16::try_from(offsets).unwrap_or(u16::MAX),
            num_rows: u16::try_from(present).unwrap_or(u16::MAX),
            page_flags: header.page_flags,
            free_size: header.free_size,
            used_size: header.used_size,
            body: PageBody::Data(body),
            file_offset: 0,
        }
    }

    /// An index page from its header fields and body. `unknown1`, `unknown2` and the row
    /// counts are 0, as on every real index page.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "Database::to_pdb writes files from slice 7e on")
    )]
    pub(crate) fn index(header: PageHeader, body: IndexBody) -> Self {
        Self {
            unknown1: 0,
            page_index: header.page_index,
            page_type: header.page_type,
            next_page: header.next_page,
            sequence: header.sequence,
            unknown2: 0,
            num_row_offsets: 0,
            num_rows: 0,
            page_flags: header.page_flags,
            free_size: header.free_size,
            used_size: header.used_size,
            body: PageBody::Index(body),
            file_offset: 0,
        }
    }

    /// The page as if decoded from file offset `file_offset`, so row errors are absolute.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "Database::to_pdb writes files from slice 7e on")
    )]
    pub(crate) fn at(mut self, file_offset: u64) -> Self {
        self.file_offset = file_offset;
        self
    }

    /// The first `u32` of the header, 0 in real files.
    pub fn unknown1(&self) -> u32 {
        self.unknown1
    }

    /// The `page_index` field as stored. In real files it equals the page's position.
    pub fn page_index(&self) -> u32 {
        self.page_index
    }

    /// The `type` field as stored. Its meaning depends on the file: see
    /// [`ExportTable`](super::ExportTable) for `export.pdb`.
    pub fn raw_type(&self) -> u32 {
        self.page_type
    }

    /// The next page of the table's chain. For a table's last page, real files store the
    /// table's `empty_candidate` here.
    pub fn next_page(&self) -> u32 {
        self.next_page
    }

    /// The `sequence` field.
    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    /// The `u32` at header offset 0x14, not understood.
    pub fn unknown2(&self) -> u32 {
        self.unknown2
    }

    /// Number of row offsets in the row index (the low 13 bits at 0x18), present or not.
    pub fn num_row_offsets(&self) -> u16 {
        self.num_row_offsets
    }

    /// The `num_rows` field (the high 11 bits at 0x18), as stored.
    pub fn num_rows(&self) -> u16 {
        self.num_rows
    }

    /// The `page_flags` byte. Bit 6 marks an index page; bit 4 a page with deleted rows.
    pub fn page_flags(&self) -> u8 {
        self.page_flags
    }

    /// The `free_size` field, as stored.
    pub fn free_size(&self) -> u16 {
        self.free_size
    }

    /// The `used_size` field, as stored.
    pub fn used_size(&self) -> u16 {
        self.used_size
    }

    /// What follows the common header.
    pub fn body(&self) -> &PageBody {
        &self.body
    }

    /// The present rows, in row-index order; none for an index page.
    ///
    /// Rows whose presence bit is clear are skipped.
    pub fn rows(&self) -> impl Iterator<Item = RawRow<'_>> + '_ {
        let (heap, groups): (&[u8], &[RowGroup]) = match &self.body {
            PageBody::Data(body) => (&body.heap, &body.row_groups),
            PageBody::Index(_) => (&[], &[]),
        };
        let heap_at = self.file_offset.saturating_add(HEAP_START as u64);
        groups
            .iter()
            .flat_map(|group| {
                group
                    .offsets
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| group.rowpf & (1 << i) != 0)
                    .map(|(_, &offset)| offset)
            })
            .map(move |offset| {
                RawRow::new(
                    offset,
                    heap.get(usize::from(offset)..).unwrap_or_default(),
                    heap_at.saturating_add(u64::from(offset)),
                )
            })
    }

    /// Encodes the page.
    ///
    /// # Errors
    ///
    /// [`EncodeError::ValueOutOfRange`] when `num_row_offsets` or `num_rows` does not fit
    /// its packed field (13 and 11 bits). A decoded page always fits; only a built page can
    /// hold that many rows.
    pub fn to_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let mut w = Writer::new();
        self.encode(&mut w)?;
        Ok(w.into_inner())
    }

    /// Decodes one page; `data` is the whole page and `base` its offset in the file.
    pub(crate) fn decode(data: &[u8], base: u64) -> Result<Self, DecodeError> {
        let mut r = Reader::with_base(data, base);
        let unknown1 = r.u32_le().context("pdb page unknown1")?;
        let page_index = r.u32_le().context("pdb page page_index")?;
        let page_type = r.u32_le().context("pdb page type")?;
        let next_page = r.u32_le().context("pdb page next_page")?;
        let sequence = r.u32_le().context("pdb page sequence")?;
        let unknown2 = r.u32_le().context("pdb page unknown2")?;
        let [b0, b1, b2] = r.array().context("pdb page row counts")?;
        let packed = u32::from_le_bytes([b0, b1, b2, 0]);
        // Both fit: 13 and 11 bits.
        let num_row_offsets = (packed & 0x1fff) as u16;
        let num_rows = (packed >> 13) as u16;
        let page_flags = r.u8().context("pdb page page_flags")?;
        let free_size = r.u16_le().context("pdb page free_size")?;
        let used_size = r.u16_le().context("pdb page used_size")?;

        let body = if page_flags & FLAG_INDEX != 0 {
            PageBody::Index(IndexBody {
                bytes: r.take_rest().to_vec(),
            })
        } else {
            PageBody::Data(DataBody::decode(&mut r, base, num_row_offsets)?)
        };
        Ok(Self {
            unknown1,
            page_index,
            page_type,
            next_page,
            sequence,
            unknown2,
            num_row_offsets,
            num_rows,
            page_flags,
            free_size,
            used_size,
            body,
            file_offset: base,
        })
    }

    /// Writes the page, or nothing when `num_row_offsets` or `num_rows` does not fit its
    /// packed field ([`EncodeError::ValueOutOfRange`]), which only a built page can hold.
    pub(crate) fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        if self.num_row_offsets > MAX_ROW_OFFSETS {
            return Err(EncodeError::ValueOutOfRange {
                context: "pdb page num_row_offsets",
            });
        }
        if self.num_rows > MAX_ROWS {
            return Err(EncodeError::ValueOutOfRange {
                context: "pdb page num_rows",
            });
        }
        w.u32_le(self.unknown1);
        w.u32_le(self.page_index);
        w.u32_le(self.page_type);
        w.u32_le(self.next_page);
        w.u32_le(self.sequence);
        w.u32_le(self.unknown2);
        let packed = (u32::from(self.num_rows) << 13) | u32::from(self.num_row_offsets);
        w.bytes(&packed.to_le_bytes()[..3]);
        w.u8(self.page_flags);
        w.u16_le(self.free_size);
        w.u16_le(self.used_size);
        match &self.body {
            PageBody::Index(body) => w.bytes(&body.bytes),
            PageBody::Data(body) => body.encode(w),
        }
        Ok(())
    }
}

impl DataBody {
    /// A data page body; `unknown3` and `unknown4` are 0, as in every real page. `heap` is
    /// the whole heap, up to the row index.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the page writer builds pages from slice 7d on")
    )]
    pub(crate) fn new(
        transaction_row_count: u16,
        transaction_row_index: u16,
        heap: Vec<u8>,
        row_groups: Vec<RowGroup>,
    ) -> Self {
        Self {
            transaction_row_count,
            transaction_row_index,
            unknown3: 0,
            unknown4: 0,
            heap,
            row_groups,
        }
    }

    /// The `transaction_row_count` field at 0x20.
    pub fn transaction_row_count(&self) -> u16 {
        self.transaction_row_count
    }

    /// The `transaction_row_index` field at 0x22.
    pub fn transaction_row_index(&self) -> u16 {
        self.transaction_row_index
    }

    /// The `u16` at 0x24, not understood.
    pub fn unknown3(&self) -> u16 {
        self.unknown3
    }

    /// The `u16` at 0x26, not understood.
    pub fn unknown4(&self) -> u16 {
        self.unknown4
    }

    /// The whole heap, from 0x28 to the start of the row index, including any bytes after
    /// `used_size`.
    pub fn heap(&self) -> &[u8] {
        &self.heap
    }

    /// The row groups, group 0 first. Group 0 is the one at the very end of the page.
    pub fn row_groups(&self) -> &[RowGroup] {
        &self.row_groups
    }

    /// Decodes everything after the common header; `r` holds the rest of the page, which
    /// starts at absolute offset `base`.
    fn decode(r: &mut Reader<'_>, base: u64, num_row_offsets: u16) -> Result<Self, DecodeError> {
        let transaction_row_count = r.u16_le().context("pdb page transaction_row_count")?;
        let transaction_row_index = r.u16_le().context("pdb page transaction_row_index")?;
        let unknown3 = r.u16_le().context("pdb page unknown3")?;
        let unknown4 = r.u16_le().context("pdb page unknown4")?;

        let count = usize::from(num_row_offsets);
        let groups = count.div_ceil(ROWS_PER_GROUP);
        let Some(heap_len) = r.remaining().checked_sub(row_index_len(count)) else {
            return Err(DecodeError::new(
                base.saturating_add(ROW_COUNTS_AT),
                "pdb page num_row_offsets",
                DecodeErrorKind::InvalidValue,
            ));
        };
        let heap = r.bytes(heap_len).context("pdb page heap")?.to_vec();
        let page_end = r.position().saturating_add(r.remaining() as u64);

        // Group 0 is last in the page, so the groups are read in reverse.
        let mut row_groups = Vec::with_capacity(groups);
        for g in (0..groups).rev() {
            let in_group = (count - g * ROWS_PER_GROUP).min(ROWS_PER_GROUP);
            let mut offsets = vec![0u16; in_group];
            // The group's last row's offset comes first, row 0's last.
            for slot in offsets.iter_mut().rev() {
                *slot = r.u16_le().context("pdb row offset")?;
            }
            let rowpf = r.u16_le().context("pdb row group rowpf")?;
            let tranrf = r.u16_le().context("pdb row group tranrf")?;
            row_groups.push(RowGroup {
                offsets,
                rowpf,
                tranrf,
            });
        }
        row_groups.reverse();

        for (g, group) in row_groups.iter().enumerate() {
            let block_end = page_end.saturating_sub((g * FULL_GROUP_LEN) as u64);
            for (i, &offset) in group.offsets.iter().enumerate() {
                let present = group.rowpf & (1 << i) != 0;
                if present && usize::from(offset) >= heap_len {
                    return Err(DecodeError::new(
                        block_end.saturating_sub(6 + 2 * i as u64),
                        "pdb row offset",
                        DecodeErrorKind::InvalidValue,
                    ));
                }
            }
        }
        Ok(Self {
            transaction_row_count,
            transaction_row_index,
            unknown3,
            unknown4,
            heap,
            row_groups,
        })
    }

    fn encode(&self, w: &mut Writer) {
        w.u16_le(self.transaction_row_count);
        w.u16_le(self.transaction_row_index);
        w.u16_le(self.unknown3);
        w.u16_le(self.unknown4);
        w.bytes(&self.heap);
        for group in self.row_groups.iter().rev() {
            for offset in group.offsets.iter().rev() {
                w.u16_le(*offset);
            }
            w.u16_le(group.rowpf);
            w.u16_le(group.tranrf);
        }
    }
}

/// Length of the row index for `count` row offsets: 36 bytes for each full group of 16, and
/// for the last group 2 bytes per offset plus `rowpf` and `tranrf`.
pub(crate) fn row_index_len(count: usize) -> usize {
    let full = count / ROWS_PER_GROUP;
    let rest = count % ROWS_PER_GROUP;
    let last = if rest == 0 {
        0
    } else {
        2 * rest + GROUP_FLAGS_LEN
    };
    full * FULL_GROUP_LEN + last
}

impl IndexBody {
    /// An index page body: every byte after the common header.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "Database::to_pdb writes files from slice 7e on")
    )]
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// The bytes after the common header.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl RowGroup {
    /// A row group; the caller keeps to at most 16 offsets, fewer only in a page's last
    /// group.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the page writer builds pages from slice 7d on")
    )]
    pub(crate) fn new(offsets: Vec<u16>, rowpf: u16, tranrf: u16) -> Self {
        Self {
            offsets,
            rowpf,
            tranrf,
        }
    }

    /// The row offsets, row 0 of the group first, relative to the heap start: 16, or fewer
    /// in a page's last group.
    pub fn offsets(&self) -> &[u16] {
        &self.offsets
    }

    /// The `rowpf` presence flags: bit `i` set means row `i` of the group is present.
    pub fn rowpf(&self) -> u16 {
        self.rowpf
    }

    /// The `tranrf` field, not understood.
    pub fn tranrf(&self) -> u16 {
        self.tranrf
    }
}

impl<'a> RawRow<'a> {
    /// A row at heap offset `offset`, whose `bytes` start at absolute file offset `at`.
    pub(crate) fn new(offset: u16, bytes: &'a [u8], at: u64) -> Self {
        Self { offset, bytes, at }
    }

    /// A reader over the row's bytes, reporting absolute file offsets.
    pub(crate) fn reader(&self) -> Reader<'a> {
        Reader::with_base(self.bytes, self.at)
    }

    /// Absolute file offset of the row's first byte.
    pub(crate) fn file_offset(&self) -> u64 {
        self.at
    }

    /// The row's offset from the heap start, as stored in the row index.
    pub fn offset(&self) -> u16 {
        self.offset
    }

    /// The heap from the row's offset to the end of the heap.
    ///
    /// The row's length is not known at this layer, so this runs past the row into
    /// whatever follows it; typed rows ([`ExportRow`](super::ExportRow)) know where each
    /// row ends.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Decodes the DeviceSQL string at `offset` from the start of the row.
    ///
    /// Flag `0x90` is always read as UTF-16LE; use [`isrc_at`](Self::isrc_at) for a
    /// track's ISRC slot.
    ///
    /// # Errors
    ///
    /// Errors are located at absolute offsets in the file the page was decoded from:
    ///
    /// - [`DecodeErrorKind::InvalidValue`] at `offset` when it is past the end of the heap;
    /// - otherwise the errors of the string decoder, at the offending byte.
    pub fn string_at(&self, offset: usize) -> Result<DeviceSqlString, DecodeError> {
        devicesql::decode(&mut self.reader_at(offset)?)
    }

    /// Decodes the DeviceSQL string at `offset` from the start of the row, which is a
    /// track's ISRC slot: flag `0x90` content of the ISRC shape is read as
    /// [`StringForm::Isrc`](crate::encoding::StringForm::Isrc). Errors are as for
    /// [`string_at`](Self::string_at).
    pub fn isrc_at(&self, offset: usize) -> Result<DeviceSqlString, DecodeError> {
        devicesql::decode_isrc(&mut self.reader_at(offset)?)
    }

    /// A reader over the row's bytes from `offset`, reporting absolute file offsets.
    fn reader_at(&self, offset: usize) -> Result<Reader<'a>, DecodeError> {
        let at = self
            .at
            .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX));
        let bytes = self.bytes.get(offset..).ok_or_else(|| {
            DecodeError::new(at, "pdb row string offset", DecodeErrorKind::InvalidValue)
        })?;
        Ok(Reader::with_base(bytes, at))
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::encoding::StringForm;

    /// Offset of the row within the heap.
    const ROW: u16 = 4;

    /// A data page, decoded at offset 0, whose only row starts at heap offset [`ROW`] with
    /// `row` as its bytes.
    fn page_with_row(row: &[u8]) -> Page {
        page_with_row_at(row, 0)
    }

    /// Like [`page_with_row`], decoded as if the page started at file offset `base`.
    fn page_with_row_at(row: &[u8], base: u64) -> Page {
        let mut heap = vec![0xee; usize::from(ROW)];
        heap.extend_from_slice(row);
        let bytes = data_page(1, 0, 2, &heap, &[(ROW, true)]);
        Page::decode(&bytes, base).expect("decodes")
    }

    fn only_row(page: &Page) -> RawRow<'_> {
        let mut rows = page.rows();
        let row = rows.next().expect("one row");
        assert!(rows.next().is_none());
        row
    }

    #[test]
    fn string_at_decodes_at_a_row_relative_offset() {
        let page = page_with_row(&[0xaa, 0xbb, 0x09, b'h', b'i', b'!', 0xff]);
        let row = only_row(&page);
        assert_eq!(row.offset(), ROW);
        assert_eq!(row.string_at(2), Ok(DeviceSqlString::new("hi!")));
    }

    #[test]
    fn isrc_at_reads_the_isrc_form_and_string_at_does_not() {
        let page = page_with_row(&[0x90, 8, 0, 0, 0x03, b'A', b'B', 0]);
        let row = only_row(&page);
        assert_eq!(row.isrc_at(0), Ok(DeviceSqlString::isrc("AB").unwrap()));
        assert_eq!(row.string_at(0).unwrap().form(), StringForm::Utf16Le);
    }

    #[test]
    fn string_errors_are_located_at_absolute_file_offsets() {
        const BASE: u64 = 0x3000;
        let page = page_with_row_at(&[0x00, 0x00], BASE);
        let row = only_row(&page);
        let at = BASE + HEAP_START as u64 + u64::from(ROW);
        let err = row.string_at(1).unwrap_err();
        assert_eq!(
            (err.offset, err.context, err.kind),
            (
                at + 1,
                "DeviceSQL string flag",
                DecodeErrorKind::InvalidValue
            )
        );
        let err = row.isrc_at(0).unwrap_err();
        assert_eq!(err.offset, at);
    }

    #[test]
    fn string_offset_at_or_past_the_heap_end() {
        let page = page_with_row(&[]);
        let row = only_row(&page);
        let len = row.bytes().len();
        let at = (HEAP_START + usize::from(ROW) + len) as u64;
        let err = row.string_at(len).unwrap_err();
        assert_eq!(
            (err.offset, err.context, err.kind),
            (
                at,
                "DeviceSQL string flag",
                DecodeErrorKind::UnexpectedEof {
                    needed: 1,
                    available: 0
                }
            )
        );
        for offset in [len + 1, usize::MAX] {
            let err = row.isrc_at(offset).unwrap_err();
            assert_eq!(
                (err.offset, err.context, err.kind),
                (
                    ((HEAP_START + usize::from(ROW)) as u64).saturating_add(offset as u64),
                    "pdb row string offset",
                    DecodeErrorKind::InvalidValue
                ),
                "{offset}"
            );
        }
    }

    #[test]
    fn row_offsets_that_do_not_fit_13_bits_fail_to_encode() {
        let header = PageHeader {
            page_index: 1,
            page_type: 2,
            next_page: 3,
            sequence: 4,
            page_flags: 0x24,
            free_size: 0,
            used_size: 0,
        };
        let groups = |offsets: usize| {
            let mut groups = vec![RowGroup::new(vec![0; 16], 0, 0); offsets / 16];
            groups.push(RowGroup::new(vec![0; offsets % 16], 0, 0));
            groups
        };
        let page = |offsets| Page::data(header, DataBody::new(0, 0, Vec::new(), groups(offsets)));
        assert_eq!(page(0x1fff).num_row_offsets(), 0x1fff);
        assert!(page(0x1fff).to_bytes().is_ok());
        assert_eq!(
            page(0x2000).to_bytes(),
            Err(EncodeError::ValueOutOfRange {
                context: "pdb page num_row_offsets"
            })
        );
    }

    #[test]
    fn a_page_re_encodes_on_its_own() {
        let bytes = data_page(1, 0, 2, b"heap", &[(0, true), (2, false)]);
        let page = Page::decode(&bytes, 0x1000).unwrap();
        assert_eq!(page.to_bytes().unwrap(), bytes);
        let bytes = index_page(3, 4, 5);
        assert_eq!(Page::decode(&bytes, 0).unwrap().to_bytes().unwrap(), bytes);
    }
}
