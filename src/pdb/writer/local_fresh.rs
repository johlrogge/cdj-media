//! Compares the page writer with a fresh rekordbox export in `local-fixtures/rekordbox-hw1`:
//! one playlist of 6 tracks, exported by rekordbox to an empty stick and played on real
//! CDJs. Like `local`, these tests skip themselves when the fixture is missing, unless
//! `CDJ_MEDIA_REQUIRE_LOCAL_FIXTURES=1`.
//!
//! The export holds the user's library, so the tests print and pin counts and shapes only:
//! page numbers, sequences, ids, lengths and field values, never names or paths.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::ops::Range;

use super::super::{ExportRow, ExportTable, Page, PageBody, Pdb, RawRow, TablePointer};
use super::file::{file_sequence, next_unused_page, write_pdb};
use super::fixed::{column_rows, default_colors};
use super::index::{EMPTY_ENTRY, build_index_page};
use super::local::{TYPED, has_tombstones, links_of, load, load_from, slots, transaction_of};
use super::{Transaction, build_data_page, build_page, pack, page_sizes};

const EXPORT: &str = "local-fixtures/rekordbox-hw1/PIONEER/rekordbox/export.pdb";

/// Collects pinned values that differ, so one run shows every one of them.
#[derive(Default)]
struct Pins(Vec<String>);

impl Pins {
    fn eq<T: Debug + PartialEq>(&mut self, what: &str, actual: T, pinned: T) {
        if actual != pinned {
            self.0
                .push(format!("{what}: {actual:?}\n    pinned {pinned:?}"));
        }
    }

    fn check(self) {
        assert!(
            self.0.is_empty(),
            "{} pins differ:\n  {}",
            self.0.len(),
            self.0.join("\n  ")
        );
    }
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

/// The data pages of the table of type `t`, in chain order.
fn data_pages(pdb: &Pdb, t: u32) -> Vec<&Page> {
    pdb.tables()
        .iter()
        .filter(|p| p.raw_type() == t)
        .flat_map(|p| pdb.pages_of(p))
        .filter(|p| matches!(p.body(), PageBody::Data(_)))
        .collect()
}

fn heap(page: &Page) -> &[u8] {
    match page.body() {
        PageBody::Data(body) => body.heap(),
        PageBody::Index(_) => &[],
    }
}

/// The typed rows of `table`, in chain order, with `index_shift` 0.
fn rows_of(pdb: &Pdb, table: ExportTable) -> Vec<ExportRow> {
    pdb.table_pages(table)
        .flat_map(|page| page.rows())
        .map(|raw| ExportRow::decode(table, &raw).unwrap().unwrap())
        .map(|row| row.with_index_shift(0))
        .collect()
}

/// The chained pages of `pdb`, in page order.
fn chained(pdb: &Pdb) -> Vec<&Page> {
    let count = u32::try_from(pdb.page_count()).unwrap();
    (1..count).filter_map(|i| pdb.page(i)).collect()
}

/// A table pointer's type, `empty_candidate`, first page and last page.
fn pointer_fields(t: &TablePointer) -> [u32; 4] {
    let (kind, candidate) = (t.raw_type(), t.empty_candidate());
    [kind, candidate, t.first_page(), t.last_page()]
}

/// The file header and the table pointers of the fresh export, against the rules
/// `write_pdb` follows (ADR-012).
///
/// - The header's `unknown2` (0x10) is 5, where the old stick has 1 and `Pdb::new` writes 1.
/// - Its `sequence` is 49, 4 more than the highest page `sequence` (45), not 1 more.
/// - `next_unused_page` (55) is one past the highest `empty_candidate`, as the rule says.
#[test]
fn fresh_export_header() {
    let Some(pdb) = load_from(EXPORT) else {
        return;
    };
    let mut pins = Pins::default();
    let header = (
        pdb.unknown1(),
        pdb.len_page(),
        pdb.unknown2(),
        pdb.unknown3(),
        pdb.header_tail().iter().all(|&b| b == 0),
    );
    pins.eq(
        "header (unknown1, len_page, unknown2, unknown3, zero tail)",
        header,
        (0, 4096, 5, 0, true),
    );
    let count = u32::try_from(pdb.page_count()).unwrap();
    pins.eq("page count", count, 51);
    let rule = next_unused_page(count, pdb.tables());
    pins.eq(
        "next_unused_page, and the rule's",
        (pdb.next_unused_page(), rule),
        (55, 55),
    );
    let pages = chained(&pdb);
    let highest = pages.iter().map(|p| p.sequence()).max();
    let rule = file_sequence(pages);
    pins.eq(
        "sequence, highest page sequence, the rule's",
        (pdb.sequence(), highest, rule),
        (49, Some(45), 46),
    );
    let pointers: Vec<[u32; 4]> = pdb.tables().iter().map(pointer_fields).collect();
    let pinned = vec![
        [0, 54, 1, 50],
        [1, 53, 3, 4],
        [2, 47, 5, 6],
        [3, 48, 7, 8],
        [4, 10, 9, 9],
        [5, 49, 11, 12],
        [6, 42, 13, 14],
        [7, 46, 15, 16],
        [8, 52, 17, 18],
        [9, 20, 19, 19],
        [10, 22, 21, 21],
        [11, 24, 23, 23],
        [12, 26, 25, 25],
        [13, 51, 27, 28],
        [14, 30, 29, 29],
        [15, 32, 31, 31],
        [16, 43, 33, 34],
        [17, 44, 35, 36],
        [18, 45, 37, 38],
        [19, 41, 39, 40],
    ];
    pins.eq(
        "table pointers (type, empty_candidate, first, last)",
        pointers,
        pinned,
    );
    pins.check();
}

/// Where the fresh export puts its pages: index pages at 2t + 1 and each table's first data
/// page or `empty_candidate` at 2t + 2, as `write_pdb` does. The difference is in the pages
/// after 40: rekordbox hands out `empty_candidate`s from page 41 in no type order (in the
/// order the tables were written, by their sequences), and the one later data page (tracks,
/// page 50) sits among them. `write_pdb` puts the further data pages first, then the
/// candidates in type order. Both files have candidates past the end.
#[test]
fn fresh_export_page_layout() {
    let Some(pdb) = load_from(EXPORT) else {
        return;
    };
    let mut pins = Pins::default();
    let count = u32::try_from(pdb.page_count()).unwrap();
    let (mut at_2t1, mut at_2t2) = (0, 0);
    let mut further = Vec::new();
    for (t, pointer) in (0u32..).zip(pdb.tables()) {
        let pages: Vec<&Page> = pdb.pages_of(pointer).collect();
        at_2t1 += usize::from(pointer.first_page() == 2 * t + 1);
        let candidate = pointer.empty_candidate();
        let second = pages.get(1).map_or(candidate, |p| p.page_index());
        at_2t2 += usize::from(second == 2 * t + 2);
        for page in pages.iter().skip(2) {
            further.push((pointer.raw_type(), page.page_index()));
        }
    }
    pins.eq(
        "tables with the index page at 2t + 1, and 2t + 2 next",
        (at_2t1, at_2t2),
        (20, 20),
    );
    pins.eq("further data pages (type, page)", further, vec![(0, 50)]);
    let mut candidates: Vec<(u32, u32)> = pdb
        .tables()
        .iter()
        .map(|t| (t.empty_candidate(), t.raw_type()))
        .collect();
    candidates.sort_unstable();
    let pinned = vec![
        (10, 4),
        (20, 9),
        (22, 10),
        (24, 11),
        (26, 12),
        (30, 14),
        (32, 15),
        (41, 19),
        (42, 6),
        (43, 16),
        (44, 17),
        (45, 18),
        (46, 7),
        (47, 2),
        (48, 3),
        (49, 5),
        (51, 13),
        (52, 8),
        (53, 1),
        (54, 0),
    ];
    pins.eq(
        "empty candidates (page, type), in page order",
        candidates.clone(),
        pinned,
    );
    let (inside, past): (BTreeSet<u32>, BTreeSet<u32>) =
        candidates.iter().map(|c| c.0).partition(|&c| c < count);
    pins.eq(
        "empty candidates past the end of the file",
        past,
        [51, 52, 53, 54].into(),
    );
    let unchained: BTreeSet<u32> = (1..count).filter(|&i| pdb.page(i).is_none()).collect();
    let zero = unchained.iter().all(|&i| {
        pdb.unchained_page(i)
            .is_some_and(|b| b.iter().all(|&x| x == 0))
    });
    pins.eq(
        "unchained pages are the candidates inside the file, all zero",
        (unchained == inside, zero, unchained.len()),
        (true, true, 16),
    );
    pins.check();
}

/// The fresh export's page `sequence`s are not 1, 2, 3, … in page order, as `write_pdb`
/// writes them: they look like a write counter. Index pages have 1, except those of the two
/// tables with deleted rows. Colours, columns and tables 17 and 18 have 2 to 5, then the
/// playlist tree 6; the other pages carry the sequence of their last write.
#[test]
fn fresh_export_sequences() {
    let Some(pdb) = load_from(EXPORT) else {
        return;
    };
    let mut pins = Pins::default();
    let mut index = Vec::new();
    let mut data = Vec::new();
    for page in chained(&pdb) {
        let at = (page.raw_type(), page.sequence());
        match page.body() {
            PageBody::Index(_) if page.sequence() != 1 => index.push(at),
            PageBody::Index(_) => {}
            PageBody::Data(_) => data.push((page.page_index(), page.sequence())),
        }
    }
    pins.eq(
        "index pages whose sequence is not 1 (type, sequence)",
        index,
        vec![(0, 44), (19, 17)],
    );
    let pinned = vec![
        (2, 44),
        (4, 22),
        (6, 42),
        (8, 36),
        (12, 43),
        (14, 2),
        (16, 6),
        (18, 45),
        (28, 39),
        (34, 3),
        (36, 4),
        (38, 5),
        (40, 44),
        (50, 40),
    ];
    pins.eq("data pages (page, sequence), in page order", data, pinned);
    pins.check();
}

/// What an index page with entries holds: (type, its entries as (page, flags), the table's
/// data pages with deleted rows, the u16 at 0x06, the entry count, equal to the built page
/// otherwise).
type IndexEntries = (u32, Vec<(u32, u32)>, Vec<u32>, u16, u16, bool);

/// The index pages of the fresh export follow the old stick's rules: empty, and byte for byte
/// `build_index_page`, for the 18 tables without deleted rows; for tracks and history, the
/// entries name exactly the data pages with deleted rows, the u16 at 0x06 equals their count,
/// and the rest is the built page.
#[test]
fn fresh_export_index_pages() {
    let Some(pdb) = load_from(EXPORT) else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let mut pins = Pins::default();
    let mut identical = 0;
    let mut with_entries: Vec<IndexEntries> = Vec::new();
    for pointer in pdb.tables() {
        let pages: Vec<&Page> = pdb.pages_of(pointer).collect();
        let index = pages[0];
        let first_data = pages.get(1).map(|p| p.page_index());
        let built = build_index_page(len_page, index.raw_type(), links_of(index), first_data)
            .and_then(|page| page.to_bytes())
            .unwrap();
        let real = index.to_bytes().unwrap();
        if built == real {
            identical += 1;
            continue;
        }
        let entries: Vec<(u32, u32)> = (0x3c..len_page - 0x14)
            .step_by(4)
            .map(|at| u32_at(&real, at))
            .filter(|&e| e != EMPTY_ENTRY)
            .map(|e| (e >> 3, e & 7))
            .collect();
        let deleted: Vec<u32> = pages[1..]
            .iter()
            .filter(|p| has_tombstones(p))
            .map(|p| p.page_index())
            .collect();
        let mask = |bytes: &[u8]| {
            let mut bytes = bytes.to_vec();
            bytes[0x26..0x28].fill(0);
            bytes[0x38..len_page - 0x14].fill(0);
            bytes
        };
        with_entries.push((
            pointer.raw_type(),
            entries,
            deleted,
            u16_at(&real, 0x26),
            u16_at(&real, 0x38),
            mask(&built) == mask(&real),
        ));
    }
    pins.eq(
        "empty index pages identical to the built ones",
        identical,
        18,
    );
    let pinned: Vec<IndexEntries> = vec![
        (0, vec![(2, 0), (50, 0)], vec![2, 50], 2, 2, true),
        (19, vec![(40, 0)], vec![40], 1, 1, true),
    ];
    pins.eq("index pages with entries", with_entries, pinned);
    pins.check();
}

/// A data page's transaction fields: (page, `transaction_row_count`,
/// `transaction_row_index`, each group's `tranrf`).
type TransactionFields = (u32, u16, u16, Vec<u16>);

/// Whether a data page holds the values `write_pdb` writes for the transaction fields: all
/// rows, from row 0, each `tranrf` equal to its `rowpf`.
fn is_whole_page(page: &Page) -> bool {
    let PageBody::Data(body) = page.body() else {
        return false;
    };
    body.transaction_row_count() == page.num_row_offsets()
        && body.transaction_row_index() == 0
        && body.row_groups().iter().all(|g| g.tranrf() == g.rowpf())
}

/// Whether a data page's unknowns are 0, its heap is zero after `used_size` and its
/// `free_size` is what `page_sizes` computes.
fn is_plain(page: &Page, len_page: usize) -> bool {
    let PageBody::Data(body) = page.body() else {
        return false;
    };
    let unknowns = (
        page.unknown1(),
        page.unknown2(),
        body.unknown3(),
        body.unknown4(),
    );
    let used = usize::from(page.used_size());
    let rows = usize::from(page.num_row_offsets());
    let sizes = page_sizes(len_page, used, rows).map(|s| (s.free, s.used));
    let zero_after = heap(page)[used..].iter().all(|&b| b == 0);
    unknowns == (0, 0, 0, 0) && zero_after && sizes == Ok((page.free_size(), page.used_size()))
}

/// The header fields of the fresh export's data pages:
///
/// - `page_flags` 0x24 without deleted rows and 0x34 with them, the unknowns 0, the heap
///   zero after `used_size` and `free_size` as `page_sizes` computes it, on every data page,
///   as on the old stick;
/// - the transaction fields record the last write, one row on most pages. Only colours,
///   columns and tables 17 and 18 hold whole-page values (all rows, from row 0), and the
///   genre and playlist tree pages, which have one row each. `write_pdb` writes whole-page
///   values on every page.
///
/// Every typed page without deleted rows rebuilds byte for byte from its rows with its
/// recorded transaction fields, so rows and heap layout match `build_data_page`.
#[test]
fn fresh_export_data_pages() {
    let Some(pdb) = load_from(EXPORT) else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let mut pins = Pins::default();
    let mut flags: BTreeMap<(u8, bool), usize> = BTreeMap::new();
    let mut odd = Vec::new();
    let mut whole_page = Vec::new();
    let mut recorded: Vec<TransactionFields> = Vec::new();
    for pointer in pdb.tables() {
        for page in data_pages(&pdb, pointer.raw_type()) {
            let PageBody::Data(body) = page.body() else {
                continue;
            };
            let key = (page.page_flags(), has_tombstones(page));
            *flags.entry(key).or_default() += 1;
            if !is_plain(page, len_page) {
                odd.push(page.page_index());
            }
            if is_whole_page(page) {
                whole_page.push(page.page_index());
                continue;
            }
            let tranrf: Vec<u16> = body.row_groups().iter().map(|g| g.tranrf()).collect();
            let count = body.transaction_row_count();
            let index = body.transaction_row_index();
            recorded.push((page.page_index(), count, index, tranrf));
        }
    }
    pins.eq(
        "(flags, has deleted rows) counts",
        flags,
        [((0x24, false), 11), ((0x34, true), 3)].into(),
    );
    pins.eq("pages with odd unknowns, heap or sizes", odd, vec![]);
    pins.eq(
        "pages with whole-page transaction values",
        whole_page,
        vec![4, 14, 16, 34, 36, 38],
    );
    let pinned: Vec<TransactionFields> = vec![
        (2, 1, 2, vec![0x04]),
        (50, 2, 0, vec![0x03]),
        (6, 1, 6, vec![0x40]),
        (8, 1, 3, vec![0x08]),
        (12, 1, 5, vec![0x20]),
        (18, 1, 5, vec![0x20]),
        (28, 1, 4, vec![0x10]),
        (40, 2, 5, vec![0x60]),
    ];
    pins.eq(
        "other transaction values (page, count, index, tranrf)",
        recorded,
        pinned,
    );

    let mut rebuilt = Vec::new();
    for table in TYPED {
        for page in data_pages(&pdb, table.to_u32()) {
            if has_tombstones(page) {
                continue;
            }
            let rows: Vec<ExportRow> = page
                .rows()
                .map(|raw| ExportRow::decode(table, &raw).unwrap().unwrap())
                .map(|row| row.with_index_shift(0))
                .collect();
            let build = |transaction: &Transaction| {
                build_data_page(len_page, table, rows.clone(), links_of(page), transaction)
                    .and_then(|built| built.to_bytes())
            };
            let real = page.to_bytes();
            let as_recorded = build(&transaction_of(page)) == real;
            let as_whole = build(&Transaction::WholePage) == real;
            rebuilt.push((table.to_u32(), as_recorded, as_whole));
        }
    }
    let pinned = vec![
        (1, true, true),
        (2, true, false),
        (3, true, false),
        (5, true, false),
        (6, true, true),
        (13, true, false),
    ];
    pins.eq(
        "typed pages without deleted rows (type, identical with the recorded transaction, \
         with whole-page values)",
        rebuilt,
        pinned,
    );
    pins.check();
}

/// Where a track row lies: (page, slot, id, present, heap bytes to the next row, footprint,
/// padding zero).
type TrackSlot = (u32, usize, u32, bool, usize, usize, bool);

/// The track pages of the fresh export: 6 tracks in 9 row versions on 2 pages. Three tracks
/// were written twice: the deleted version differs from the present one only in
/// `index_shift` and `artwork_id`, so rekordbox added the artwork after the track. Two rows
/// sit in the slots of earlier, longer rows, with leftover bytes after them. Without the
/// rewrites the 6 tracks fit one page, as `pack` puts them.
#[test]
fn fresh_export_track_pages() {
    let Some(pdb) = load_from(EXPORT) else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let mut pins = Pins::default();
    let mut layout: Vec<TrackSlot> = Vec::new();
    let mut versions: BTreeMap<u32, Vec<(bool, Vec<u8>)>> = BTreeMap::new();
    for page in data_pages(&pdb, 0) {
        let slots = slots(page);
        let page_index = page.page_index();
        let used = usize::from(page.used_size());
        for (k, &(slot, offset, present)) in slots.iter().enumerate() {
            let start = usize::from(offset);
            let raw = RawRow::new(offset, &heap(page)[start..], 0);
            let Ok(Some(row)) = ExportRow::decode(ExportTable::Tracks, &raw) else {
                panic!("a track row");
            };
            let ExportRow::Track(track) = &row else {
                panic!("a track row");
            };
            let id = track.id().get();
            let bytes = row.encode().unwrap();
            let footprint = row.heap_footprint().unwrap();
            let next = slots.get(k + 1).map_or(used, |s| usize::from(s.1));
            let padding = &heap(page)[start + bytes.len()..next];
            let zero = padding.iter().all(|&b| b == 0);
            let span = next - start;
            layout.push((page_index, slot, id, present, span, footprint, zero));
            versions.entry(id).or_default().push((present, bytes));
        }
    }
    let pinned: Vec<TrackSlot> = vec![
        (2, 0, 4, true, 556, 548, false),
        (2, 1, 1, true, 556, 556, true),
        (2, 2, 6, true, 500, 368, false),
        (2, 3, 2, true, 500, 500, true),
        (2, 4, 3, false, 564, 564, true),
        (2, 5, 3, true, 564, 564, true),
        (2, 6, 4, false, 548, 548, true),
        (50, 0, 5, false, 572, 572, true),
        (50, 1, 5, true, 572, 572, true),
    ];
    pins.eq(
        "track slots (page, slot, id, present, heap span, footprint, zero padding)",
        layout,
        pinned,
    );
    let mut differences = BTreeSet::new();
    let mut rewritten = Vec::new();
    for (id, rows) in &versions {
        let Some((_, live)) = rows.iter().find(|(present, _)| *present) else {
            continue;
        };
        for (_, old) in rows.iter().filter(|(present, _)| !present) {
            rewritten.push(*id);
            let pairs = old.iter().zip(live).enumerate();
            differences.extend(pairs.filter(|(_, (a, b))| a != b).map(|(at, _)| at));
        }
    }
    pins.eq("rewritten track ids", rewritten, vec![3, 4, 5]);
    // 2: index_shift (low byte); 0x1c: artwork_id (low byte).
    pins.eq(
        "bytes the versions differ in",
        differences,
        [2, 0x1c].into(),
    );
    let rows = rows_of(&pdb, ExportTable::Tracks);
    let packed = pack(len_page, &rows);
    pins.eq("present tracks packed into pages", packed, Ok(vec![6]));
    pins.check();
}

/// The rows (number, `used_size`, the distinct steps between row offsets) of each data page
/// of the table of type `t`.
type RowShape = (u16, u16, Vec<usize>);

fn row_shapes(pdb: &Pdb, t: u32) -> Vec<RowShape> {
    let mut out = Vec::new();
    for page in data_pages(pdb, t) {
        let offsets: Vec<u16> = slots(page).iter().map(|s| s.1).collect();
        let steps: BTreeSet<usize> = offsets
            .windows(2)
            .map(|w| usize::from(w[1] - w[0]))
            .collect();
        let steps = steps.into_iter().collect();
        out.push((page.num_rows(), page.used_size(), steps));
    }
    out
}

/// The first 8 bytes of every present row of the table of type `t`.
fn short_rows(pdb: &Pdb, t: u32) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for page in data_pages(pdb, t) {
        out.extend(page.rows().map(|r| r.bytes()[..8].to_vec()));
    }
    out
}

/// The fixed tables: colours and columns are byte for byte `write_pdb`'s; tables 17 and 18,
/// which `write_pdb` leaves empty, have rows in a fresh export, written whole at creation
/// (sequences 4 and 5). Table 18 is the old stick's, byte for byte; table 17 shares 13 of
/// its 22 rows with the old stick, so it holds settings that changed on the old stick.
#[test]
fn fresh_export_fixed_tables() {
    let Some(pdb) = load_from(EXPORT) else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let mut pins = Pins::default();
    let whole = Transaction::WholePage;
    let [colors] = data_pages(&pdb, 6)[..] else {
        panic!("one colour page");
    };
    let rows = default_colors();
    let table = ExportTable::Colors;
    let built = build_data_page(len_page, table, rows, links_of(colors), &whole);
    let colors_equal = built.and_then(|p| p.to_bytes()) == colors.to_bytes();
    let [columns] = data_pages(&pdb, 16)[..] else {
        panic!("one column page");
    };
    let rows = column_rows().unwrap();
    let built = build_page(len_page, 16, rows, links_of(columns), &whole);
    let columns_equal = built.and_then(|p| p.to_bytes()) == columns.to_bytes();
    pins.eq(
        "colour and column pages equal the built ones",
        (colors_equal, columns_equal),
        (true, true),
    );
    pins.eq(
        "tables 17 and 18: (rows, used_size, row spacing) per data page",
        (row_shapes(&pdb, 17), row_shapes(&pdb, 18)),
        (vec![(22, 176, vec![8])], vec![(17, 136, vec![8])]),
    );
    let Some(old) = load() else {
        return pins.check();
    };
    let (fresh17, old17) = (short_rows(&pdb, 17), short_rows(&old, 17));
    let shared17 = fresh17.iter().filter(|r| old17.contains(r)).count();
    let heaps = |file: &Pdb| -> Vec<Vec<u8>> {
        let pages = data_pages(file, 18);
        pages.iter().map(|p| heap(p).to_vec()).collect()
    };
    pins.eq(
        "table 17 rows shared with the old stick, of; table 18 heap equal to the old stick's",
        (shared17, fresh17.len(), heaps(&pdb) == heaps(&old)),
        (13, 22, true),
    );
    pins.check();
}

/// A playlist tree row: the u32s at 0 to 0x14, the name's form and characters, the row's
/// length and its page's `used_size`.
type TreeRow = (Vec<u32>, String, usize, usize, u16);

/// The playlist and history tables of the fresh export, by shape:
///
/// - playlist tree: one row of 0x14 bytes of u32s (parent 0, unknown 0, sort order 0, id 1,
///   not a folder) and a short-ASCII name, padded to 4;
/// - playlist entries: 6 rows of 12 bytes (entry index, track id, playlist id), entry k
///   holding track k;
/// - history playlists and history entries: empty, an index page only;
/// - table 19 (history): 7 row versions of 40 bytes, the last present.
#[test]
fn fresh_export_playlists_and_history() {
    let Some(pdb) = load_from(EXPORT) else {
        return;
    };
    let mut pins = Pins::default();
    let mut tree: Vec<TreeRow> = Vec::new();
    for page in data_pages(&pdb, 7) {
        for raw in page.rows() {
            let name = raw.string_at(0x14).unwrap();
            let words = (0..5).map(|k| u32_at(raw.bytes(), 4 * k)).collect();
            let form = format!("{:?}", name.form());
            let chars = name.as_str().chars().count();
            let len = 0x14 + name.encoded_len();
            tree.push((words, form, chars, len, page.used_size()));
        }
    }
    let pinned: Vec<TreeRow> = vec![(vec![0, 0, 0, 1, 0], "ShortAscii".to_owned(), 13, 34, 36)];
    pins.eq("playlist tree rows", tree, pinned);
    let mut entries: Vec<Vec<u32>> = Vec::new();
    let mut used = Vec::new();
    for page in data_pages(&pdb, 8) {
        used.push(page.used_size());
        for raw in page.rows() {
            entries.push((0..3).map(|k| u32_at(raw.bytes(), 4 * k)).collect());
        }
    }
    let pinned: Vec<Vec<u32>> = (1..=6).map(|k| vec![k, k, 1]).collect();
    pins.eq(
        "playlist entries (entry index, track id, playlist id)",
        entries,
        pinned,
    );
    pins.eq("playlist entries used_size", used, vec![72]);
    let empty: Vec<(u32, usize)> = [11, 12]
        .iter()
        .map(|&t| (t, data_pages(&pdb, t).len()))
        .collect();
    pins.eq(
        "history playlists and entries: data pages",
        empty,
        vec![(11, 0), (12, 0)],
    );
    // Table 19: every slot, absent rows too. Bytes 2..8 vary; the rest is compared.
    let mut history = Vec::new();
    let mut rest = BTreeSet::new();
    for page in data_pages(&pdb, 19) {
        for (slot, offset, present) in slots(page) {
            let start = usize::from(offset);
            let row = &heap(page)[start..start + 40];
            let fields = (u16_at(row, 0), u16_at(row, 2), u32_at(row, 4));
            history.push((slot, present, fields));
            rest.insert(row[8..].to_vec());
        }
    }
    let pinned: Vec<(usize, bool, (u16, u16, u32))> = (0..7u16)
        .map(|k| (usize::from(k), k == 6, (0x280, 0x20 * k, u32::from(k))))
        .collect();
    pins.eq(
        "table 19 slots (slot, present, (u16 0, u16 2, u32 4))",
        history,
        pinned,
    );
    pins.eq("table 19 rows alike after byte 8", rest.len(), 1);
    pins.check();
}

/// Which header fields or body parts of two pages differ.
fn page_differences(a: &[u8], b: &[u8], index: bool) -> Vec<&'static str> {
    const FIELDS: [(&str, Range<usize>); 9] = [
        ("index", 0x04..0x08),
        ("type", 0x08..0x0c),
        ("next", 0x0c..0x10),
        ("seq", 0x10..0x14),
        ("unknown2", 0x14..0x18),
        ("rows", 0x18..0x1b),
        ("flags", 0x1b..0x1c),
        ("sizes", 0x1c..0x20),
        ("transaction", 0x20..0x24),
    ];
    let mut out: Vec<&str> = FIELDS
        .iter()
        .filter(|(name, _)| !index || *name != "transaction")
        .filter(|(_, r)| a[r.clone()] != b[r.clone()])
        .map(|(name, _)| *name)
        .collect();
    let body = if index { 0x20 } else { 0x24 };
    if a[body..] != b[body..] {
        out.push("body");
    }
    out
}

/// What a page position holds: whether it is an index page, and its bytes.
fn kind(pdb: &Pdb, i: u32) -> Option<(bool, Vec<u8>)> {
    if let Some(page) = pdb.page(i) {
        let index = matches!(page.body(), PageBody::Index(_));
        return Some((index, page.to_bytes().unwrap()));
    }
    pdb.unchained_page(i).map(|b| (false, b.to_vec()))
}

/// How two pages at the same position differ.
fn describe(a: &[u8], b: &[u8], index: bool) -> String {
    let zero = |x: &[u8]| x.iter().all(|&c| c == 0);
    match (zero(a), zero(b)) {
        (true, _) => "zero in rekordbox's".to_owned(),
        (_, true) => "zero in ours".to_owned(),
        _ => page_differences(a, b, index).join(" "),
    }
}

/// The fresh export against `write_pdb` from its own typed rows, page by page. The same: the
/// empty index pages and the zero pages of the tables both leave empty. Different: links and
/// sequences elsewhere, the transaction fields, the track pages, the index and data pages of
/// the playlist, history and 17/18 tables, which `write_pdb` leaves empty, and every page
/// from 40 on, past the end of ours.
#[test]
fn fresh_export_against_write_pdb() {
    let Some(pdb) = load_from(EXPORT) else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let mut pins = Pins::default();
    let rows: Vec<ExportRow> = TYPED.iter().flat_map(|&t| rows_of(&pdb, t)).collect();
    let ours = write_pdb(len_page, rows).unwrap();
    let header = (ours.next_unused_page(), ours.sequence(), ours.unknown2());
    pins.eq(
        "ours: pages, (next_unused_page, sequence, unknown2)",
        (ours.page_count(), header),
        (40, (49, 10, 1)),
    );
    let count = u32::try_from(pdb.page_count().max(ours.page_count())).unwrap();
    let mut same = Vec::new();
    let mut differ: Vec<(u32, String)> = Vec::new();
    for i in 1..count {
        let how = match (kind(&pdb, i), kind(&ours, i)) {
            (Some((_, a)), Some((_, b))) if a == b => {
                same.push(i);
                continue;
            }
            (Some((index, a)), Some((_, b))) => describe(&a, &b, index),
            (Some(_), None) => "past our end".to_owned(),
            (None, Some(_)) => "past rekordbox's end".to_owned(),
            (None, None) => continue,
        };
        differ.push((i, how));
    }
    let pinned = vec![
        3, 5, 7, 9, 10, 11, 13, 19, 20, 21, 22, 23, 24, 25, 26, 27, 29, 30, 31, 32, 33,
    ];
    pins.eq("pages identical", same, pinned);
    let pinned: Vec<(u32, String)> = [
        (1, "seq body"),
        (2, "next seq rows flags sizes transaction body"),
        (4, "next seq"),
        (6, "next seq transaction body"),
        (8, "next seq transaction body"),
        (12, "next seq transaction body"),
        (14, "next seq"),
        (15, "body"),
        (16, "zero in ours"),
        (17, "body"),
        (18, "zero in ours"),
        (28, "next seq transaction body"),
        (34, "next seq"),
        (35, "body"),
        (36, "zero in ours"),
        (37, "body"),
        (38, "zero in ours"),
        (39, "seq body"),
    ]
    .into_iter()
    .chain((40..=50).map(|i| (i, "past our end")))
    .map(|(i, how)| (i, how.to_owned()))
    .collect();
    pins.eq("pages that differ, and how", differ, pinned);
    pins.check();
}
