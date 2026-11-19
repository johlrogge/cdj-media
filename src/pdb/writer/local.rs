//! Checks the page writer against the real `export.pdb` in `local-fixtures/`. These tests
//! live in the crate, not in `tests/`, because the writer is crate-private (ADR-006,
//! ADR-007). Like `tests/common`, they skip themselves when the fixture is missing, unless
//! `CDJ_MEDIA_REQUIRE_LOCAL_FIXTURES=1`.
//!
//! The stick holds the user's library, so the tests print counts and shapes only.

use std::collections::BTreeMap;
use std::path::Path;

use super::super::{ExportRow, ExportTable, Page, PageBody, Pdb, RawRow};
use super::*;

const EXPORT: &str = "local-fixtures/real-stick/PIONEER/rekordbox/export.pdb";
/// Set to `1` to make a missing local fixture fail the test instead of skipping it.
const REQUIRE_ENV: &str = "CDJ_MEDIA_REQUIRE_LOCAL_FIXTURES";

pub(super) fn load() -> Option<Pdb> {
    load_from(EXPORT)
}

/// Decodes the `export.pdb` at `rel` (from the crate root), or skips as [`load`] does.
pub(super) fn load_from(rel: &str) -> Option<Pdb> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    if !path.exists() {
        assert!(
            std::env::var(REQUIRE_ENV).as_deref() != Ok("1"),
            "local fixture {} not found and {REQUIRE_ENV}=1",
            path.display()
        );
        eprintln!("skipping: local fixture {} not found", path.display());
        return None;
    }
    let bytes = std::fs::read(&path).expect("read export.pdb");
    Some(Pdb::from_bytes(&bytes).expect("decode export.pdb"))
}

/// Every table of the file, in file order.
pub(super) fn tables(pdb: &Pdb) -> Vec<ExportTable> {
    pdb.tables()
        .iter()
        .map(|t| ExportTable::from_u32(t.raw_type()))
        .collect()
}

/// The data pages of `table`, in chain order.
fn data_pages(pdb: &Pdb, table: ExportTable) -> Vec<&Page> {
    pdb.table_pages(table)
        .filter(|p| matches!(p.body(), PageBody::Data(_)))
        .collect()
}

/// Fails with the first few `failures` and their count, if there are any.
fn assert_none(failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} failures, the first:\n{}",
        failures.len(),
        failures[..failures.len().min(12)].join("\n")
    );
}

/// Every row offset of a data page with its slot and presence, in slot order.
pub(super) fn slots(page: &Page) -> Vec<(usize, u16, bool)> {
    let PageBody::Data(body) = page.body() else {
        return Vec::new();
    };
    body.row_groups()
        .iter()
        .flat_map(|g| {
            g.offsets()
                .iter()
                .enumerate()
                .map(move |(i, &o)| (o, g.rowpf() & (1 << i) != 0))
        })
        .enumerate()
        .map(|(slot, (offset, present))| (slot, offset, present))
        .collect()
}

/// The present row of `page` at heap offset `offset`.
fn present_row(page: &Page, offset: u16) -> RawRow<'_> {
    let mut rows = page.rows();
    rows.find(|r| r.offset() == offset).expect("present row")
}

/// The `u16` at `at` of a data page's heap, if it is inside the heap.
fn heap_u16(page: &Page, at: usize) -> Option<u16> {
    let PageBody::Data(body) = page.body() else {
        return None;
    };
    let bytes = body.heap().get(at..at + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// `index_shift_for_slot` gives the `index_shift` of every track, artist and album row from
/// its slot: the decoded field of present rows, and the same two bytes of absent ones (whose
/// row bytes stay in the heap). Genres, labels, keys, colours and artwork have no such field.
#[test]
fn real_index_shift_is_computed_from_the_slot() {
    let Some(pdb) = load() else {
        return;
    };
    let mut failures = Vec::new();
    eprintln!("index_shift = index_shift_for_slot(slot):");
    for table in [
        ExportTable::Tracks,
        ExportTable::Artists,
        ExportTable::Albums,
    ] {
        let (mut present, mut matching, mut absent, mut absent_matching) = (0, 0, 0, 0);
        for page in data_pages(&pdb, table) {
            for (slot, offset, is_present) in slots(page) {
                let expected = index_shift_for_slot(slot).ok();
                if !is_present {
                    absent += 1;
                    if heap_u16(page, usize::from(offset) + 2) == expected {
                        absent_matching += 1;
                    } else {
                        failures.push(format!(
                            "{table:?} page {} absent slot {slot}",
                            page.page_index()
                        ));
                    }
                    continue;
                }
                present += 1;
                let shift = match ExportRow::decode(table, &present_row(page, offset)) {
                    Ok(Some(ExportRow::Track(r))) => r.index_shift(),
                    Ok(Some(ExportRow::Artist(r))) => r.index_shift(),
                    Ok(Some(ExportRow::Album(r))) => r.index_shift(),
                    other => panic!("{table:?}: {other:?}"),
                };
                if Some(shift) == expected {
                    matching += 1;
                } else {
                    failures.push(format!(
                        "{table:?} page {} slot {slot}: index_shift {shift:#x}",
                        page.page_index()
                    ));
                }
            }
        }
        eprintln!(
            "  {table:?}: {matching} of {present} present rows; the bytes of {absent_matching} \
             of {absent} absent rows too"
        );
    }
    assert_none(&failures);
}

pub(super) fn has_tombstones(page: &Page) -> bool {
    usize::from(page.num_row_offsets()) != page.rows().count()
}

/// The tables whose rows are typed.
pub(super) const TYPED: [ExportTable; 8] = [
    ExportTable::Tracks,
    ExportTable::Genres,
    ExportTable::Artists,
    ExportTable::Albums,
    ExportTable::Labels,
    ExportTable::Keys,
    ExportTable::Colors,
    ExportTable::Artwork,
];

/// Track rows of the real stick followed by 4 more zero bytes than their footprint, all on
/// pages with deleted rows.
const FOUR_FURTHER: usize = 5;

/// How a present row lies in its page's heap, against its footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layout {
    /// The next row starts exactly a footprint on, and the padding is zero.
    Exact,
    /// At the exact place, but the padding holds leftover bytes.
    LeftoverInPadding,
    /// The next row starts further on, and the bytes in between hold leftover bytes.
    LeftoverFurtherOn,
    /// The next row starts 4 bytes further on, with zero bytes in between.
    FourZeroFurtherOn,
    /// The next row starts further on still, with zero bytes in between.
    ZeroFurtherOn,
    /// The next row starts inside the footprint.
    Overlaps,
}

/// How each present row of a page of a typed table lies, by slot, with the bytes from its
/// start to the next row's and its footprint. Rows lie in the heap in slot order (checked by
/// `real_heap_footprints_match_the_gaps`), so a row ends where the next slot's row starts,
/// present or absent, or the last at `used_size`.
fn layouts(table: ExportTable, page: &Page) -> Vec<(usize, Layout, usize, usize)> {
    let PageBody::Data(body) = page.body() else {
        return Vec::new();
    };
    let slots = slots(page);
    let mut out = Vec::new();
    for (k, &(slot, offset, present)) in slots.iter().enumerate() {
        if !present {
            continue;
        }
        let row = ExportRow::decode(table, &present_row(page, offset))
            .unwrap()
            .unwrap();
        let start = usize::from(offset);
        let end = start + row.encode().unwrap().len();
        let next = slots
            .get(k + 1)
            .map_or(usize::from(page.used_size()), |s| usize::from(s.1));
        let footprint = row.heap_footprint().unwrap();
        let between = body.heap().get(end..next).unwrap_or_default();
        let zero = between.iter().all(|&b| b == 0);
        let span = next.saturating_sub(start);
        let layout = match (span.checked_sub(footprint), zero) {
            (Some(0), true) => Layout::Exact,
            (Some(0), false) => Layout::LeftoverInPadding,
            (Some(_), false) => Layout::LeftoverFurtherOn,
            (Some(4), true) => Layout::FourZeroFurtherOn,
            (Some(_), true) => Layout::ZeroFurtherOn,
            (None, _) => Layout::Overlaps,
        };
        out.push((slot, layout, span, footprint));
    }
    out
}

/// Checks `ExportRow::heap_footprint` against the real heaps: each present row is counted by
/// its [`Layout`], separately on pages with deleted rows. The leftover layouts are rows
/// rewritten in place, whose earlier, longer version left bytes behind. The 4 zero bytes
/// further on occur only on pages with deleted rows, which rekordbox rewrote: not explained,
/// but no fresh layout is involved. Any other zero gap, a row that overlaps its footprint,
/// or rows out of slot order, is a failure.
#[test]
fn real_heap_footprints_match_the_gaps() {
    let Some(pdb) = load() else {
        return;
    };
    let mut failures = Vec::new();
    let mut four_further = 0;
    eprintln!("heap footprints against the gaps between rows:");
    for table in TYPED {
        let mut categories: BTreeMap<(Layout, bool), usize> = BTreeMap::new();
        for page in data_pages(&pdb, table) {
            let slots = slots(page);
            if !slots.windows(2).all(|w| w[0].1 < w[1].1) {
                failures.push(format!(
                    "{table:?} page {}: not in slot order",
                    page.page_index()
                ));
            }
            let deleted = has_tombstones(page);
            for (slot, layout, next, footprint) in layouts(table, page) {
                *categories.entry((layout, deleted)).or_default() += 1;
                let allowed = match layout {
                    Layout::FourZeroFurtherOn => deleted,
                    Layout::ZeroFurtherOn | Layout::Overlaps => false,
                    _ => true,
                };
                four_further += usize::from(layout == Layout::FourZeroFurtherOn);
                if !allowed {
                    failures.push(format!(
                        "{table:?} page {} slot {slot}: {layout:?}, next at {next} for \
                         {footprint}",
                        page.page_index()
                    ));
                }
            }
        }
        eprintln!("  {table:?}:");
        for ((layout, deleted), count) in &categories {
            let on = if *deleted {
                " (page with deleted rows)"
            } else {
                ""
            };
            eprintln!("    {count:4} {layout:?}{on}");
        }
    }
    assert_none(&failures);
    assert_eq!(four_further, FOUR_FURTHER, "rows 4 zero bytes further on");
}

/// The links of a real page, for its rebuild.
pub(super) fn links_of(page: &Page) -> PageLinks {
    PageLinks {
        page_index: page.page_index(),
        next_page: page.next_page(),
        sequence: page.sequence(),
    }
}

/// The transaction fields of a real data page, as stored.
pub(super) fn transaction_of(page: &Page) -> Transaction {
    let PageBody::Data(body) = page.body() else {
        panic!("data page");
    };
    Transaction::Recorded {
        row_count: body.transaction_row_count(),
        row_index: body.transaction_row_index(),
        tranrf: body.row_groups().iter().map(|g| g.tranrf()).collect(),
    }
}

/// Why a real data page does not come back identical from its typed rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rebuild {
    /// Identical, with its stored links and transaction fields.
    Identical,
    /// Identical even with whole-page transaction fields: written in one transaction.
    IdenticalAsWholePage,
    /// The table's rows are not typed (yet).
    NotTyped,
    /// The page has deleted rows, which a fresh layout never has.
    DeletedRows,
    /// Rows were rewritten in place: leftover bytes, so rows are not where a fresh layout
    /// puts them.
    RewrittenInPlace,
}

/// The acceptance test of the page builder: every real data page of every table, rebuilt
/// from its decoded typed rows with `build_data_page`, gives identical bytes, or falls in an
/// explained [`Rebuild`] category. The links (`page_index`, `next_page`, `sequence`) and the
/// transaction fields (`transaction_row_count`, `transaction_row_index`, each `tranrf`) are
/// taken from the real page; whether the whole-page transaction values would do as well is
/// counted too. A page that has typed rows, no deleted rows and only exact row layouts but
/// still differs is a failure.
#[test]
fn real_data_pages_rebuild_from_their_typed_rows() {
    let Some(pdb) = load() else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let mut counts: BTreeMap<String, BTreeMap<Rebuild, usize>> = BTreeMap::new();
    let mut failures = Vec::new();
    for table in tables(&pdb) {
        for page in data_pages(&pdb, table) {
            let category = if !TYPED.contains(&table) {
                Rebuild::NotTyped
            } else if has_tombstones(page) {
                Rebuild::DeletedRows
            } else {
                // With `index_shift` 0, as the row builders make them: the page builder
                // must set it.
                let rows: Vec<ExportRow> = typed_rows(table, page)
                    .into_iter()
                    .map(|row| row.with_index_shift(0))
                    .collect();
                let build = |transaction: &Transaction| {
                    build_data_page(len_page, table, rows.clone(), links_of(page), transaction)
                        .and_then(|built| built.to_bytes())
                };
                let original = page.to_bytes().unwrap();
                let built = build(&transaction_of(page));
                let rewritten = layouts(table, page)
                    .iter()
                    .any(|&(_, layout, _, _)| layout != Layout::Exact);
                if built.as_ref() == Ok(&original) {
                    if build(&Transaction::WholePage) == Ok(original) {
                        Rebuild::IdenticalAsWholePage
                    } else {
                        Rebuild::Identical
                    }
                } else if rewritten {
                    Rebuild::RewrittenInPlace
                } else {
                    let first_difference =
                        |b: &Vec<u8>| b.iter().zip(&original).position(|(x, y)| x != y);
                    let at = built.as_ref().map(first_difference);
                    failures.push(format!(
                        "{table:?} page {}: rebuilt differs, first at {at:?}",
                        page.page_index()
                    ));
                    continue;
                }
            };
            *counts
                .entry(format!("{table:?}"))
                .or_default()
                .entry(category)
                .or_default() += 1;
        }
    }
    eprintln!("real data pages rebuilt from their typed rows:");
    let mut totals: BTreeMap<Rebuild, usize> = BTreeMap::new();
    for (table, categories) in &counts {
        eprintln!("  {table}: {categories:?}");
        for (category, count) in categories {
            *totals.entry(*category).or_default() += count;
        }
    }
    eprintln!("  all: {totals:?}");
    assert_none(&failures);
}

/// The decoded present rows of a page, in slot order.
fn typed_rows(table: ExportTable, page: &Page) -> Vec<ExportRow> {
    page.rows()
        .map(|raw| ExportRow::decode(table, &raw).unwrap().unwrap())
        .collect()
}

/// How rekordbox decides a page break. For each data page P of a typed table that another
/// data page Q follows, and whose slot 0 row of Q is present: would that row, with its
/// footprint and row index entry, have fit in P's free space? If not, P was full. If it
/// would, rekordbox started Q although P had room (on this stick, which rekordbox updated
/// over many sessions, Q holds rows added later).
///
/// For every P without deleted rows whose rows all lie exactly ([`Layout::Exact`]), `pack`
/// of P's rows and Q's first row must put them on one page when the row fits and break
/// before it when it does not, which reproduces every full-page break. Whole-table packing
/// is reported against the real page counts; it cannot match where pages are not full.
#[test]
fn real_page_breaks_are_full_pages_or_later_writes() {
    let Some(pdb) = load() else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let mut failures = Vec::new();
    eprintln!("page breaks: did the next page's first row fit?");
    for table in TYPED {
        let pages = data_pages(&pdb, table);
        let mut breaks: BTreeMap<&str, usize> = BTreeMap::new();
        let most_rows = pages.iter().map(|p| p.num_row_offsets()).max();
        for pair in pages.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            let Some(&(0, offset, true)) = slots(q).first() else {
                *breaks.entry("next page's first row deleted").or_default() += 1;
                continue;
            };
            let first = ExportRow::decode(table, &present_row(q, offset))
                .unwrap()
                .unwrap();
            let used = usize::from(p.used_size()) + first.heap_footprint().unwrap();
            let n = usize::from(p.num_row_offsets());
            let fits = page_sizes(len_page, used, n + 1).is_ok();
            let fresh = !has_tombstones(p)
                && layouts(table, p)
                    .iter()
                    .all(|&(_, layout, _, _)| layout == Layout::Exact);
            let kind = match (fits, fresh) {
                (false, true) => "full",
                (false, false) => "full (deleted rows or rewritten rows)",
                (true, true) => "room left",
                (true, false) => "room left (deleted rows or rewritten rows)",
            };
            *breaks.entry(kind).or_default() += 1;
            if fits {
                let later = if q.sequence() > p.sequence() {
                    "room left: next page's sequence higher"
                } else {
                    "room left: next page's sequence not higher"
                };
                *breaks.entry(later).or_default() += 1;
            }
            if fresh {
                let mut rows = typed_rows(table, p);
                rows.push(first);
                let expected = if fits { vec![n + 1] } else { vec![n, 1] };
                let packed = pack(len_page, &rows);
                if packed.as_ref() != Ok(&expected) {
                    failures.push(format!(
                        "{table:?} page {}: packed {packed:?}, expected {expected:?}",
                        p.page_index()
                    ));
                }
            }
        }
        let all: Vec<ExportRow> = pages.iter().flat_map(|p| typed_rows(table, p)).collect();
        let packed = pack(len_page, &all).unwrap();
        eprintln!(
            "  {table:?}: {breaks:?}; most row offsets on a page {most_rows:?}; {} rows pack \
             into {} pages ({:?} rows each), rekordbox has {}",
            all.len(),
            packed.len(),
            packed,
            pages.len()
        );
    }
    assert_none(&failures);
}

/// The header fields the page builder sets to constants, on every real data page:
/// `unknown1`, `unknown2`, `unknown3` and `unknown4` are 0, and `page_flags` is
/// [`DATA_PAGE_FLAGS`] without deleted rows and 0x34 with them. The transaction fields are
/// reported: how many pages hold whole-page values (all rows, from row 0, `tranrf` equal to
/// `rowpf`), and the other shapes.
#[test]
fn real_data_page_header_fields() {
    let Some(pdb) = load() else {
        return;
    };
    let mut failures = Vec::new();
    let mut flags: BTreeMap<(u8, bool), usize> = BTreeMap::new();
    let mut transactions: BTreeMap<String, usize> = BTreeMap::new();
    for table in tables(&pdb) {
        for page in data_pages(&pdb, table) {
            let PageBody::Data(body) = page.body() else {
                continue;
            };
            let place = format!("{table:?} page {}", page.page_index());
            let unknowns = (
                page.unknown1(),
                page.unknown2(),
                body.unknown3(),
                body.unknown4(),
            );
            if unknowns != (0, 0, 0, 0) {
                failures.push(format!("{place}: unknowns {unknowns:?}"));
            }
            let deleted = has_tombstones(page);
            *flags.entry((page.page_flags(), deleted)).or_default() += 1;
            let expected = if deleted { 0x34 } else { DATA_PAGE_FLAGS };
            if page.page_flags() != expected {
                failures.push(format!("{place}: flags {:#04x}", page.page_flags()));
            }
            let whole = body.transaction_row_count() == page.num_row_offsets()
                && body.transaction_row_index() == 0
                && body.row_groups().iter().all(|g| g.tranrf() == g.rowpf());
            let shape = if whole {
                "whole page: all rows from row 0, tranrf = rowpf".to_owned()
            } else {
                let groups = body.row_groups().iter();
                let set: u32 = groups.map(|g| g.tranrf().count_ones()).sum();
                format!(
                    "count {}, {set} tranrf bits set",
                    body.transaction_row_count()
                )
            };
            *transactions.entry(shape).or_default() += 1;
        }
    }
    eprintln!("data page flags (flags, has deleted rows): {flags:?}");
    eprintln!("data page transaction fields:");
    for (shape, count) in &transactions {
        eprintln!("  {count:4} {shape}");
    }
    assert_none(&failures);
}

/// `page_sizes` gives back `free_size` and `used_size` from `used_size` and the row count on
/// every data page of every table.
#[test]
fn real_page_sizes_are_computed() {
    let Some(pdb) = load() else {
        return;
    };
    let len_page = pdb.len_page() as usize;
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();
    for table in tables(&pdb) {
        for page in data_pages(&pdb, table) {
            let computed = page_sizes(
                len_page,
                usize::from(page.used_size()),
                usize::from(page.num_row_offsets()),
            );
            let stored = PageSizes {
                free: page.free_size(),
                used: page.used_size(),
            };
            let count = counts.entry(format!("{table:?}")).or_default();
            count.1 += 1;
            if computed == Ok(stored) {
                count.0 += 1;
            } else {
                failures.push(format!(
                    "{table:?} page {}: computed {computed:?}, stored {stored:?}",
                    page.page_index()
                ));
            }
        }
    }
    eprintln!("free_size and used_size computed (matching of data pages):");
    for (table, (matching, pages)) in &counts {
        eprintln!("  {table}: {matching} of {pages}");
    }
    assert_none(&failures);
}
