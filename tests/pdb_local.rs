//! Round-trips the real `export.pdb` (and `exportExt.pdb`) in `local-fixtures/` through
//! [`Pdb`], and checks the page chains against the facts the raw layer relies on. Skips
//! itself when the fixture is missing.

mod common;

use std::collections::{BTreeMap, HashSet};

use cdj_media::pdb::{ExportTable, Page, PageBody, Pdb, TablePointer};

const EXPORT: &str = "real-stick/PIONEER/rekordbox/export.pdb";
const EXPORT_EXT: &str = "real-stick/PIONEER/rekordbox/exportExt.pdb";

/// Pages of the real-stick `export.pdb` that no table chain reaches.
const REAL_STICK_UNCHAINED: usize = 18;

/// Reads and decodes a fixture, or returns `None` when it is missing.
fn load(rel: &str) -> Option<(Vec<u8>, Pdb)> {
    let path = common::local_fixture(rel)?;
    let bytes = std::fs::read(&path).expect("read fixture");
    let pdb = Pdb::from_bytes(&bytes).unwrap_or_else(|e| panic!("decode {rel}: {e}"));
    Some((bytes, pdb))
}

fn first_difference(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}

fn assert_identical(encoded: &[u8], original: &[u8], what: &str) {
    assert!(
        encoded == original,
        "{what}: re-encoded {} bytes vs {} original, first difference at {:#x}",
        encoded.len(),
        original.len(),
        first_difference(encoded, original)
    );
}

fn page_bytes<'a>(bytes: &'a [u8], pdb: &Pdb, index: usize) -> &'a [u8] {
    let len = pdb.len_page() as usize;
    &bytes[index * len..(index + 1) * len]
}

#[test]
fn real_export_pdb_round_trips_identically() {
    let Some((bytes, pdb)) = load(EXPORT) else {
        return;
    };
    let encoded = pdb.to_bytes().expect("encode");
    assert_identical(&encoded, &bytes, "export.pdb");
    eprintln!(
        "export.pdb: {} bytes, {} pages of {}, {} tables, next_unused_page {}, sequence {}",
        bytes.len(),
        pdb.page_count(),
        pdb.len_page(),
        pdb.tables().len(),
        pdb.next_unused_page(),
        pdb.sequence()
    );
    assert_eq!(pdb.page_count() * pdb.len_page() as usize, bytes.len());
    assert!(
        pdb.header_tail().iter().all(|&b| b == 0),
        "header tail is zero"
    );
}

#[test]
fn real_export_pdb_pages_re_encode_on_their_own() {
    let Some((bytes, pdb)) = load(EXPORT) else {
        return;
    };
    let mut unchained = Vec::new();
    for index in 1..pdb.page_count() {
        let original = page_bytes(&bytes, &pdb, index);
        let n = u32::try_from(index).expect("page index fits");
        match (pdb.page(n), pdb.unchained_page(n)) {
            (Some(page), None) => {
                let encoded = page.to_bytes().expect("encode page");
                assert_identical(&encoded, original, &format!("page {index}"));
            }
            (None, Some(raw)) => {
                assert_eq!(raw, original, "unchained page {index}");
                unchained.push(index);
            }
            other => panic!("page {index} is neither chained nor unchained: {other:?}"),
        }
    }
    eprintln!("unchained pages: {unchained:?}");
    for &index in &unchained {
        let raw = page_bytes(&bytes, &pdb, index);
        assert!(
            raw.iter().all(|&b| b == 0),
            "unchained page {index} is zero"
        );
    }
    assert_eq!(unchained.len(), REAL_STICK_UNCHAINED);
}

/// Follows a table's chain by hand: `(position, page)` from `first_page` to `last_page`.
fn chain<'a>(pdb: &'a Pdb, table: &TablePointer) -> Vec<(u32, &'a Page)> {
    let mut out = Vec::new();
    let mut index = table.first_page();
    loop {
        let page = pdb
            .page(index)
            .unwrap_or_else(|| panic!("type {}: page {index} not chained", table.raw_type()));
        out.push((index, page));
        if index == table.last_page() {
            return out;
        }
        index = page.next_page();
    }
}

/// Data pages in the row-index edge cases: no row offsets, and a non-zero multiple of 16
/// (a full last group).
#[derive(Default, Debug)]
struct EdgeCases {
    no_offsets: usize,
    full_last_group: usize,
    failures: Vec<String>,
}

impl EdgeCases {
    /// Counts `page` if it is an edge case, and checks `free_size + used_size == heap` on
    /// it, naming the subset in any failure.
    fn check(&mut self, place: &str, page: &Page) {
        let PageBody::Data(body) = page.body() else {
            return;
        };
        let n = page.num_row_offsets();
        let subset = match n {
            0 => {
                self.no_offsets += 1;
                "no row offsets"
            }
            n if n % 16 == 0 => {
                self.full_last_group += 1;
                "full last group"
            }
            _ => return,
        };
        let sizes = usize::from(page.used_size()) + usize::from(page.free_size());
        if sizes != body.heap().len() {
            self.failures.push(format!(
                "{place} ({subset}, {n} offsets): used + free {sizes} but heap {}",
                body.heap().len()
            ));
        }
    }

    fn report(&self, file: &str) {
        eprintln!(
            "{file} data pages with no row offsets: {}; with a full last group: {}",
            self.no_offsets, self.full_last_group
        );
        assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
    }
}

#[test]
fn real_export_pdb_chains_match_the_table_pointers() {
    let Some((_, pdb)) = load(EXPORT) else {
        return;
    };
    let types: HashSet<u32> = pdb.tables().iter().map(TablePointer::raw_type).collect();
    assert_eq!(types.len(), pdb.tables().len(), "one table per type");

    let mut rows_per_table = BTreeMap::new();
    let mut failures = Vec::new();
    let mut edges = EdgeCases::default();
    let mut total_rows = 0;
    for table in pdb.tables() {
        let kind = ExportTable::from_u32(table.raw_type());
        let pages = chain(&pdb, table);
        let positions: Vec<u32> = pages.iter().map(|(index, _)| *index).collect();
        let pages_of: Vec<u32> = pdb.pages_of(table).map(Page::page_index).collect();
        let by_kind: Vec<u32> = pdb.table_pages(kind).map(Page::page_index).collect();
        if pages_of != positions || by_kind != positions {
            failures.push(format!(
                "{kind:?}: pages_of {pages_of:?}, table_pages {by_kind:?}, chain {positions:?}"
            ));
        }

        let mut rows = 0;
        for (i, (index, page)) in pages.iter().enumerate() {
            let place = format!("{kind:?} page {index}");
            if page.page_index() != *index {
                failures.push(format!("{place}: page_index {}", page.page_index()));
            }
            if page.raw_type() != table.raw_type() {
                failures.push(format!("{place}: type {}", page.raw_type()));
            }
            let flags = page.page_flags();
            match (i, page.body()) {
                (0, PageBody::Index(_)) if flags == 0x64 => {}
                (1.., PageBody::Data(_)) if flags == 0x24 || flags == 0x34 => {}
                _ => failures.push(format!("{place}: position {i} with flags {flags:#04x}")),
            }
            // The heap is exactly `used_size + free_size` long and zero after `used_size`;
            // this holds only with a last row group that is shorter than 36 bytes.
            if let PageBody::Data(body) = page.body() {
                let used = usize::from(page.used_size());
                let sizes = used + usize::from(page.free_size());
                if sizes != body.heap().len() {
                    failures.push(format!(
                        "{place}: used + free {sizes} but heap {}",
                        body.heap().len()
                    ));
                }
                if !body
                    .heap()
                    .get(used..)
                    .is_some_and(|t| t.iter().all(|&b| b == 0))
                {
                    failures.push(format!("{place}: heap not zero after used_size"));
                }
            }
            edges.check(&place, page);
            let present = page.rows().count();
            if usize::from(page.num_rows()) != present {
                failures.push(format!(
                    "{place}: num_rows {} but {present} present rows",
                    page.num_rows()
                ));
            }
            rows += present;
        }
        let (_, last) = pages.last().expect("chain is not empty");
        if last.next_page() != table.empty_candidate() {
            failures.push(format!(
                "{kind:?}: last page's next_page {} but empty_candidate {}",
                last.next_page(),
                table.empty_candidate()
            ));
        }
        total_rows += rows;
        rows_per_table.insert(table.raw_type(), (kind, pages.len(), rows));
    }

    eprintln!("export.pdb tables: {total_rows} rows");
    for (raw, (kind, pages, rows)) in &rows_per_table {
        eprintln!("  {raw:2} {kind:?}: {pages} pages, {rows} rows");
    }
    edges.report("export.pdb");
    assert!(total_rows > 0, "no rows found");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `exportExt.pdb` has the same framing. Its table types number their own tables, so they
/// are printed as raw values and followed with `pages_of`: the `ExportTable` names are
/// those of `export.pdb`.
#[test]
fn real_export_ext_pdb_round_trips_identically() {
    let Some((bytes, pdb)) = load(EXPORT_EXT) else {
        return;
    };
    let encoded = pdb.to_bytes().expect("encode");
    assert_identical(&encoded, &bytes, "exportExt.pdb");
    eprintln!(
        "exportExt.pdb: {} bytes, {} pages of {}, {} tables",
        bytes.len(),
        pdb.page_count(),
        pdb.len_page(),
        pdb.tables().len()
    );
    let mut edges = EdgeCases::default();
    for table in pdb.tables() {
        let pages = pdb.pages_of(table).count();
        let rows: usize = pdb.pages_of(table).map(|p| p.rows().count()).sum();
        for page in pdb.pages_of(table) {
            let place = format!("type {} page {}", table.raw_type(), page.page_index());
            edges.check(&place, page);
        }
        eprintln!(
            "  type {:2}: first {} last {} empty {}; {pages} pages, {rows} rows",
            table.raw_type(),
            table.first_page(),
            table.last_page(),
            table.empty_candidate()
        );
    }
    edges.report("exportExt.pdb");
}
