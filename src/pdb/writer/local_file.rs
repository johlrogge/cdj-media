//! Checks the whole-file writer (index pages, the file header, page chains) against the real
//! `export.pdb` in `local-fixtures/`. Like `local`, these tests skip themselves when the
//! fixture is missing, unless `CDJ_MEDIA_REQUIRE_LOCAL_FIXTURES=1`.
//!
//! The stick holds the user's library, so the tests print counts and shapes only.

use std::collections::{BTreeMap, BTreeSet};

use super::super::{Database, ExportRow, ExportTable, Page, Pdb};
use super::file::{file_sequence, next_unused_page, write_pdb};
use super::fixed::{column_rows, default_colors};
use super::index::{EMPTY_ENTRY, build_index_page};
use super::local::{TYPED, has_tombstones, load, tables};
use super::{PageLinks, Transaction, build_data_page, build_page};

/// Fails with the first few `failures` and their count, if there are any.
fn assert_none(failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} failures, the first:\n{}",
        failures.len(),
        failures[..failures.len().min(12)].join("\n")
    );
}

/// The `u16` at `at` of `bytes`.
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

/// The acceptance test of the index page builder: the first page of every real table,
/// against `build_index_page` with the page's links, type and the table's first data page.
///
/// - Tables without deleted rows: identical bytes.
/// - Tables with deleted rows: the entries name exactly the table's data pages with deleted
///   rows (flags 0), the entry count at 0x18 of the index header is their number, and the
///   page equals the built one except for the entries, that count and the two `u16`s at
///   0x06 and 0x1a of the index header, which are not derived.
#[test]
fn real_index_pages_rebuild() {
    let Some(pdb) = load() else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let mut failures = Vec::new();
    let (mut identical, mut explained) = (0, 0);
    let mut fields: BTreeMap<String, (u16, u16, u16)> = BTreeMap::new();
    for (table, pages) in chains(&pdb) {
        let index = pages[0];
        let links = PageLinks {
            page_index: index.page_index(),
            next_page: index.next_page(),
            sequence: index.sequence(),
        };
        let first_data = pages.get(1).map(|p| p.page_index());
        let built = build_index_page(len_page, index.raw_type(), links, first_data)
            .and_then(|page| page.to_bytes())
            .unwrap();
        let real = index.to_bytes().unwrap();
        let deleted: BTreeSet<u32> = pages[1..]
            .iter()
            .filter(|p| has_tombstones(p))
            .map(|p| p.page_index())
            .collect();
        if deleted.is_empty() {
            if built == real {
                identical += 1;
            } else {
                let at = built.iter().zip(&real).position(|(a, b)| a != b);
                failures.push(format!("{table:?}: differs, first at {at:?}"));
            }
            continue;
        }
        // Offsets in the page: the index header starts at 0x20, the entries at 0x3c.
        let entries: Vec<u32> = (0x3c..len_page - 0x14)
            .step_by(4)
            .map(|at| u32_at(&real, at))
            .filter(|&e| e != EMPTY_ENTRY)
            .collect();
        let named: BTreeSet<u32> = entries.iter().map(|e| e >> 3).collect();
        let flags_zero = entries.iter().all(|e| e & 7 == 0);
        let count = u16_at(&real, 0x38);
        let mask = |bytes: &[u8]| {
            let mut bytes = bytes.to_vec();
            bytes[0x26..0x28].fill(0);
            bytes[0x38..len_page - 0x14].fill(0);
            bytes
        };
        fields.insert(
            format!("{table:?}"),
            (u16_at(&real, 0x26), count, u16_at(&real, 0x3a)),
        );
        if named == deleted
            && flags_zero
            && usize::from(count) == entries.len()
            && mask(&built) == mask(&real)
        {
            explained += 1;
        } else {
            failures.push(format!(
                "{table:?}: {} entries for {} pages with deleted rows",
                entries.len(),
                deleted.len()
            ));
        }
    }
    eprintln!(
        "index pages: {identical} identical (no deleted rows), {explained} explained (entries \
         = the pages with deleted rows)"
    );
    eprintln!("  index header (0x06, count, 0x1a): {fields:?}");
    assert_none(&failures);
    assert_eq!((identical, explained), (8, 12));
}

/// The acceptance test of the file header builder: `Pdb::new` with the real `len_page`,
/// table pointers and pages, and `next_unused_page` and `sequence` from their rules, gives
/// back the real file byte for byte. Pages no chain reaches are passed as `None`, so they
/// must be zero-filled.
#[test]
fn real_file_rebuilds_from_its_pages_and_header_rules() {
    let Some(pdb) = load() else {
        return;
    };
    let count = u32::try_from(pdb.page_count()).unwrap();
    let chained: Vec<&Page> = (1..count).filter_map(|i| pdb.page(i)).collect();
    let sequence = file_sequence(chained.iter().copied());
    let next_unused = next_unused_page(count, pdb.tables());
    eprintln!(
        "file header: sequence {sequence} (stored {}), next_unused_page {next_unused} (stored \
         {}), {} tables, {count} pages",
        pdb.sequence(),
        pdb.next_unused_page(),
        pdb.tables().len()
    );
    let pages = (1..count).map(|i| pdb.page(i).cloned()).collect();
    let built = Pdb::new(
        pdb.len_page(),
        next_unused,
        sequence,
        pdb.tables().to_vec(),
        pages,
    )
    .unwrap();
    let original = pdb.to_bytes().unwrap();
    let rebuilt = built.to_bytes().unwrap();
    let at = rebuilt.iter().zip(&original).position(|(a, b)| a != b);
    assert_eq!(at, None, "first difference");
    assert_eq!(rebuilt.len(), original.len());
    assert_eq!(built, pdb);
}

/// The links of a real page, for its rebuild.
fn links_of(page: &Page) -> PageLinks {
    PageLinks {
        page_index: page.page_index(),
        next_page: page.next_page(),
        sequence: page.sequence(),
    }
}

/// The colour and column tables of the real stick are one data page each, written in one
/// transaction, and their rows are rekordbox's defaults: each page equals the page built
/// from `default_colors` or `column_rows` with its links. Colour names can be user-set, so
/// a difference is reported without them.
#[test]
fn real_fixed_tables_hold_the_defaults() {
    let Some(pdb) = load() else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let data = |table| -> Vec<&Page> { pdb.table_pages(table).skip(1).collect() };
    let [colors] = data(ExportTable::Colors)[..] else {
        panic!("one colour data page");
    };
    let real: Vec<ExportRow> = colors
        .rows()
        .map(|raw| {
            ExportRow::decode(ExportTable::Colors, &raw)
                .unwrap()
                .unwrap()
        })
        .collect();
    assert!(
        real == default_colors(),
        "the {} colour rows differ from the defaults",
        real.len()
    );
    let whole = Transaction::WholePage;
    let built = build_data_page(
        len_page,
        ExportTable::Colors,
        default_colors(),
        links_of(colors),
        &whole,
    )
    .unwrap();
    assert!(
        built.to_bytes() == colors.to_bytes(),
        "the colour page differs from the built one"
    );

    let [columns] = data(ExportTable::Columns)[..] else {
        panic!("one column data page");
    };
    let rows = column_rows().unwrap();
    let built = build_page(len_page, 16, rows, links_of(columns), &whole).unwrap();
    let (built, real) = (built.to_bytes().unwrap(), columns.to_bytes().unwrap());
    let at = built.iter().zip(&real).position(|(a, b)| a != b);
    assert_eq!(at, None, "first difference of the column page");
    eprintln!("colour and column pages: built from the defaults");
}

/// The typed rows of `table`, in chain order, with `index_shift` 0.
fn rows_of(pdb: &Pdb, table: ExportTable) -> Vec<ExportRow> {
    pdb.table_pages(table)
        .flat_map(|page| page.rows())
        .map(|raw| ExportRow::decode(table, &raw).unwrap().unwrap())
        .map(|row| row.with_index_shift(0))
        .collect()
}

/// The acceptance test of the whole-file writer: the typed rows of the real stick, written
/// into a fresh file with `write_pdb` and decoded again, come back in the same order, every
/// table, and the fresh file loads into the same `Database` with no dangling reference.
/// Rows are compared with `index_shift` 0, as the page builder sets it from the slot.
/// The fresh layout is reported against the real one.
#[test]
fn real_rows_survive_a_fresh_file() {
    let Some(pdb) = load() else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let rows: Vec<ExportRow> = TYPED
        .iter()
        .flat_map(|&table| rows_of(&pdb, table))
        .collect();
    let bytes = write_pdb(len_page, rows).unwrap().to_bytes().unwrap();
    let fresh = Pdb::from_bytes(&bytes).expect("the fresh file decodes");
    let mut failures = Vec::new();
    eprintln!("fresh file: rows, data pages against the real ones:");
    for table in TYPED {
        let (real, written) = (rows_of(&pdb, table), rows_of(&fresh, table));
        let pages = |file: &Pdb| file.table_pages(table).count() - 1;
        eprintln!(
            "  {table:?}: {} rows, {} data pages against {}",
            written.len(),
            pages(&fresh),
            pages(&pdb)
        );
        if real != written {
            failures.push(format!(
                "{table:?}: {} rows written, {} read back or different",
                real.len(),
                written.len()
            ));
        }
    }
    eprintln!(
        "  file: {} pages against {}, next_unused_page {} against {}, sequence {} against {}",
        fresh.page_count(),
        pdb.page_count(),
        fresh.next_unused_page(),
        pdb.next_unused_page(),
        fresh.sequence(),
        pdb.sequence()
    );
    assert_none(&failures);
    let real_db = Database::from_pdb(&pdb).unwrap();
    let fresh_db = Database::from_pdb(&fresh).expect("the fresh file loads");
    assert!(
        real_db == fresh_db,
        "the fresh database differs from the real one"
    );
    assert_eq!(fresh_db.dangling_references().count(), 0);
    assert_eq!(fresh.to_bytes().unwrap(), bytes);
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The pages of every table, in chain order.
fn chains(pdb: &Pdb) -> Vec<(ExportTable, Vec<&Page>)> {
    pdb.tables()
        .iter()
        .map(|t| {
            (
                ExportTable::from_u32(t.raw_type()),
                pdb.pages_of(t).collect(),
            )
        })
        .collect()
}

/// The layout rules `write_pdb` takes from the real stick, checked on it:
///
/// - the header: `unknown1` and `unknown3` 0, `unknown2` 1, zero after the pointers;
/// - 20 tables, types 0 to 19 in order; table `t`'s index page is page 2t + 1, and page
///   2t + 2 is its first data page or, for a table without data pages, its
///   `empty_candidate`;
/// - every `empty_candidate` is its own page, in no chain and below `next_unused_page`, and
///   the pages no chain reaches are exactly the `empty_candidate`s inside the file, all
///   zero-filled;
/// - the index page of every table without deleted rows has `sequence` 1.
///
/// Reported: how many tables have contiguous data pages, and the data-page sequences of the
/// tables rekordbox filled when it created the database.
#[test]
fn real_layout_rules() {
    let Some(pdb) = load() else {
        return;
    };
    let mut failures = Vec::new();
    let header = (pdb.unknown1(), pdb.unknown2(), pdb.unknown3());
    if header != (0, 1, 0) || pdb.header_tail().iter().any(|&b| b != 0) {
        failures.push(format!("header unknowns {header:?} or a non-zero tail"));
    }
    let types: Vec<u32> = tables(&pdb).iter().map(|t| t.to_u32()).collect();
    if types != (0..20).collect::<Vec<u32>>() {
        failures.push(format!("table types {types:?}"));
    }
    let mut chained = BTreeSet::new();
    let (mut contiguous, mut initial) = (0, Vec::new());
    let pointers = (0u32..).zip(pdb.tables());
    for ((table, pages), (t, pointer)) in chains(&pdb).into_iter().zip(pointers) {
        chained.extend(pages.iter().map(|p| p.page_index()));
        if pointer.first_page() != 2 * t + 1 {
            failures.push(format!("{table:?}: first page {}", pointer.first_page()));
        }
        let second = pages
            .get(1)
            .map_or(pointer.empty_candidate(), |p| p.page_index());
        if second != 2 * t + 2 {
            failures.push(format!("{table:?}: page after the index page {second}"));
        }
        let deleted = pages.iter().any(|p| has_tombstones(p));
        if !deleted && pages[0].sequence() != 1 {
            failures.push(format!("{table:?}: index sequence {}", pages[0].sequence()));
        }
        let data: Vec<u32> = pages[1..].iter().map(|p| p.page_index()).collect();
        contiguous += usize::from(!data.is_empty() && data.windows(2).all(|w| w[1] == w[0] + 1));
        if let [only] = pages[1..] {
            if only.sequence() < 10 {
                initial.push((table, only.sequence()));
            }
        }
    }
    let candidates: BTreeSet<u32> = pdb.tables().iter().map(|t| t.empty_candidate()).collect();
    if candidates.len() != 20 || !candidates.is_disjoint(&chained) {
        failures.push("empty candidates shared or chained".to_owned());
    }
    if candidates.iter().any(|&c| c >= pdb.next_unused_page()) {
        failures.push("an empty candidate at or past next_unused_page".to_owned());
    }
    let count = u32::try_from(pdb.page_count()).unwrap();
    let unchained: BTreeSet<u32> = (1..count).filter(|&i| pdb.page(i).is_none()).collect();
    let inside: BTreeSet<u32> = candidates.iter().copied().filter(|&c| c < count).collect();
    if unchained != inside {
        failures.push(format!(
            "{} unchained pages, {} empty candidates inside the file",
            unchained.len(),
            inside.len()
        ));
    }
    let zero = unchained.iter().all(|&i| {
        pdb.unchained_page(i)
            .is_some_and(|b| b.iter().all(|&x| x == 0))
    });
    if !zero {
        failures.push("a non-zero unchained page".to_owned());
    }
    eprintln!(
        "layout: {} unchained pages, all empty candidates ({} candidates past the end); {} of \
         20 tables have contiguous data pages; one-page tables written early (table, \
         sequence): {initial:?}",
        unchained.len(),
        candidates.len() - inside.len(),
        contiguous
    );
    assert_none(&failures);
}

/// The tables whose rows are not typed: data pages, row offsets and present rows. Shapes
/// only; playlist and history rows hold library data.
#[test]
fn real_untyped_tables() {
    let Some(pdb) = load() else {
        return;
    };
    eprintln!("untyped tables: pages, offsets, present, sequences:");
    for (table, pages) in chains(&pdb) {
        if TYPED.contains(&table) {
            continue;
        }
        let data = &pages[1..];
        let offsets: usize = data.iter().map(|p| usize::from(p.num_row_offsets())).sum();
        let present: usize = data.iter().map(|p| p.rows().count()).sum();
        let sequences = data.iter().map(|p| p.sequence());
        let range = sequences.clone().min().zip(sequences.max());
        eprintln!(
            "  {table:?}: {} pages, {offsets} offsets, {present} present, {range:?}",
            data.len()
        );
    }
}
