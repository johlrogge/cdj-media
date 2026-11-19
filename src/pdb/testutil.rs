//! Hand-built `export.pdb` bytes for tests, independent of the encoder.
//!
//! The values are made up, not rekordbox's.

/// Page length of the synthetic files; real files use 4,096.
pub(crate) const LEN: usize = 512;
/// `next_unused_page` of the synthetic files.
pub(crate) const NEXT_UNUSED: u32 = 9;
/// `sequence` of the synthetic files.
pub(crate) const SEQUENCE: u32 = 77;

fn push_u32s(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// Page 0: the file header with `tables` as `[type, empty_candidate, first, last]`.
pub(crate) fn header_page(tables: &[[u32; 4]]) -> Vec<u8> {
    let mut out = Vec::new();
    push_u32s(
        &mut out,
        &[
            0,
            LEN as u32,
            tables.len() as u32,
            NEXT_UNUSED,
            5,
            SEQUENCE,
            0,
        ],
    );
    for table in tables {
        push_u32s(&mut out, table);
    }
    out.resize(LEN, 0);
    out
}

/// The 32-byte common page header.
#[expect(clippy::too_many_arguments, reason = "mirrors the header layout")]
pub(crate) fn page_header(
    index: u32,
    kind: u32,
    next: u32,
    num_row_offsets: u32,
    num_rows: u32,
    flags: u8,
    free: u16,
    used: u16,
) -> Vec<u8> {
    let mut out = Vec::new();
    push_u32s(&mut out, &[0, index, kind, next, 0x11, 0x22]);
    let packed = (num_rows << 13) | num_row_offsets;
    out.extend_from_slice(&packed.to_le_bytes()[..3]);
    out.push(flags);
    out.extend_from_slice(&free.to_le_bytes());
    out.extend_from_slice(&used.to_le_bytes());
    out
}

/// An index page with a made-up body.
pub(crate) fn index_page(index: u32, kind: u32, next: u32) -> Vec<u8> {
    let mut out = page_header(index, kind, next, 0, 0, 0x64, 0, 0);
    out.extend_from_slice(&[0x1f, 0, 0xec, 0x03]);
    out.extend((0..).map(|i: u32| (i % 251) as u8).take(LEN - out.len()));
    out
}

/// Length of the row index for `rows` row offsets: 36 bytes per full group, and for the
/// last group only its offsets plus `rowpf` and `tranrf`.
pub(crate) fn row_index_len(rows: usize) -> usize {
    match rows.div_ceil(16) {
        0 => 0,
        groups => 36 * (groups - 1) + 4 + 2 * (rows - 16 * (groups - 1)),
    }
}

/// A data page with `heap` at the start of its heap, zero after it, and `rows` as
/// `(offset, present)`.
pub(crate) fn data_page(
    index: u32,
    kind: u32,
    next: u32,
    heap: &[u8],
    rows: &[(u16, bool)],
) -> Vec<u8> {
    let groups = rows.len().div_ceil(16);
    let present = rows.iter().filter(|(_, p)| *p).count();
    let mut out = page_header(
        index,
        kind,
        next,
        rows.len() as u32,
        present as u32,
        0x34,
        0x33,
        heap.len() as u16,
    );
    for value in [3u16, 5, 7, 9] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(heap);
    out.resize(LEN - row_index_len(rows.len()), 0);
    for g in (0..groups).rev() {
        let mut rowpf = 0u16;
        let in_group = rows.len().saturating_sub(g * 16).min(16);
        for i in (0..in_group).rev() {
            let (offset, present) = rows[g * 16 + i];
            if present {
                rowpf |= 1 << i;
            }
            out.extend_from_slice(&offset.to_le_bytes());
        }
        out.extend_from_slice(&rowpf.to_le_bytes());
        out.extend_from_slice(&0x1234u16.to_le_bytes());
    }
    assert_eq!(out.len(), LEN);
    out
}

/// A file: the header page followed by `pages`.
pub(crate) fn file(tables: &[[u32; 4]], pages: &[Vec<u8>]) -> Vec<u8> {
    let mut out = header_page(tables);
    for page in pages {
        assert_eq!(page.len(), LEN);
        out.extend_from_slice(page);
    }
    out
}

/// One artists table: index page 1, then data page 2 with two rows, row 1 not present.
pub(crate) fn minimal_file() -> Vec<u8> {
    // Row 0 at heap offset 0 (8 bytes), row 1 at offset 8.
    let mut heap = b"\x01row 0\0\0".to_vec();
    heap.extend_from_slice(b"\x02deleted");
    file(
        &[[2, 3, 1, 2]],
        &[
            index_page(1, 2, 2),
            data_page(2, 2, 3, &heap, &[(0, true), (8, false)]),
        ],
    )
}

/// A tracks table chained 3 → 1 → 4, an unknown-9 table on page 2, and page 5 that no
/// chain reaches, filled with `0xa5`.
pub(crate) fn chained_file() -> Vec<u8> {
    file(
        &[[0, 6, 3, 4], [9, 7, 2, 2]],
        &[
            data_page(1, 0, 4, b"one", &[(0, true)]),
            index_page(2, 9, 7),
            index_page(3, 0, 1),
            data_page(4, 0, 6, b"four", &[(0, true)]),
            vec![0xa5; LEN],
        ],
    )
}
