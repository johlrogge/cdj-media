//! Round-trips every DeviceSQL string in a real `export.pdb` in `local-fixtures/`, and
//! checks that `DeviceSqlString::new` picks the form rekordbox picked. Skips itself when the
//! fixture is missing.
//!
//! The page walker here is deliberately minimal and test-only; the real page codec comes in
//! a later slice. Everything in `export.pdb` is little-endian.
//!
//! Scaffolding: replaced by the `Pdb` round-trip test in slice 4, which must cover every
//! table; delete then.

mod common;

use std::collections::{BTreeMap, HashSet};

use cdj_media::encoding::{DeviceSqlString, StringForm};

const TRACKS: u32 = 0;
const GENRES: u32 = 1;
const ARTISTS: u32 = 2;
const ALBUMS: u32 = 3;
const LABELS: u32 = 4;
const KEYS: u32 = 5;

/// Number of string offsets in a track row.
const TRACK_STRINGS: usize = 21;

fn u8_at(data: &[u8], at: usize) -> Option<u8> {
    data.get(at).copied()
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(*data.get(at..)?.first_chunk()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(*data.get(at..)?.first_chunk()?))
}

fn usize_at(data: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32_at(data, at)?).ok()
}

/// A table pointer from the file header.
struct Table {
    kind: u32,
    first_page: usize,
    last_page: usize,
}

fn tables(file: &[u8]) -> Vec<Table> {
    let num_tables = usize_at(file, 8).expect("num_tables");
    (0..num_tables)
        .map(|i| {
            let at = 0x1c + 16 * i;
            Table {
                kind: u32_at(file, at).expect("table type"),
                first_page: usize_at(file, at + 8).expect("first_page"),
                last_page: usize_at(file, at + 12).expect("last_page"),
            }
        })
        .collect()
}

/// Indices of the pages in a table's chain, stopping at `last_page` or a repeated page.
fn page_chain(file: &[u8], len_page: usize, table: &Table) -> Vec<usize> {
    let mut pages = Vec::new();
    let mut seen = HashSet::new();
    let mut page = table.first_page;
    while seen.insert(page) {
        let Some(base) = page.checked_mul(len_page) else {
            break;
        };
        if base.saturating_add(len_page) > file.len() {
            break;
        }
        pages.push(page);
        if page == table.last_page {
            break;
        }
        let Some(next) = usize_at(file, base + 0x0c) else {
            break;
        };
        page = next;
    }
    pages
}

/// Absolute offsets of the present rows of the data page at `base`; empty for other pages.
fn rows(file: &[u8], base: usize, len_page: usize) -> Vec<usize> {
    let flags = u8_at(file, base + 0x1b).expect("page_flags");
    if flags & 0x40 != 0 {
        return Vec::new();
    }
    let packed = u32_at(file, base + 0x18).expect("row counts") & 0x00ff_ffff;
    let num_rows = usize::try_from(packed & 0x1fff).expect("row count fits");
    let end = base + len_page;
    let mut found = Vec::new();
    for g in 0..num_rows.div_ceil(16) {
        let group_end = end - 36 * g;
        let mask = u16_at(file, group_end - 4).expect("presence mask");
        for i in 0..16 {
            if g * 16 + i >= num_rows {
                break;
            }
            if mask & (1 << i) == 0 {
                continue;
            }
            let offset = u16_at(file, group_end - 6 - 2 * i).expect("row offset");
            found.push(base + 0x28 + usize::from(offset));
        }
    }
    found
}

/// Row-relative offsets of the strings in a row, with whether each is the ISRC slot.
fn string_offsets(file: &[u8], kind: u32, row: usize) -> Vec<(usize, bool)> {
    let u8_rel = |rel: usize| usize::from(u8_at(file, row + rel).expect("u8 in row"));
    let u16_rel = |rel: usize| usize::from(u16_at(file, row + rel).expect("u16 in row"));
    match kind {
        GENRES | LABELS => vec![(4, false)],
        KEYS => vec![(8, false)],
        ARTISTS => {
            let at = if u16_rel(0) == 0x64 {
                u16_rel(0x0a)
            } else {
                u8_rel(0x09)
            };
            vec![(at, false)]
        }
        ALBUMS => {
            let at = if u16_rel(0) == 0x84 {
                u16_rel(0x16)
            } else {
                u8_rel(0x15)
            };
            vec![(at, false)]
        }
        TRACKS => (0..TRACK_STRINGS)
            .map(|k| (u16_rel(0x5e + 2 * k), k == 0))
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the raw field has the ISRC shape: flag `0x90`, content `0x03` ... NUL.
fn has_isrc_shape(field: &[u8]) -> bool {
    let Some(len) = u16_at(field, 1).map(usize::from) else {
        return false;
    };
    field.first() == Some(&0x90)
        && len >= 6
        && field.get(4) == Some(&0x03)
        && field.get(len - 1) == Some(&0)
}

/// Decodes and re-encodes one string field; returns its form, or why it failed.
fn check(field: &[u8], isrc_slot: bool) -> Result<StringForm, String> {
    let decoded = if isrc_slot {
        DeviceSqlString::isrc_slot_from_bytes(field)
    } else {
        DeviceSqlString::from_bytes(field)
    }
    .map_err(|e| format!("decode: {e}"))?;
    let encoded = decoded.to_bytes().map_err(|e| format!("encode: {e}"))?;
    if field.get(..encoded.len()) != Some(&encoded[..]) {
        return Err(format!(
            "re-encoded bytes differ: {:02x?} vs {:02x?}",
            encoded,
            &field[..encoded.len().min(field.len())]
        ));
    }
    if decoded.form() != StringForm::Isrc {
        let picked = DeviceSqlString::new(decoded.as_str()).form();
        if picked != decoded.form() {
            return Err(format!(
                "rekordbox used {:?} for {:?}, new() picks {picked:?}",
                decoded.form(),
                decoded.as_str()
            ));
        }
    }
    Ok(decoded.form())
}

#[test]
fn real_export_pdb_strings_round_trip_identically() {
    let Some(path) = common::local_fixture("real-stick/PIONEER/rekordbox/export.pdb") else {
        return;
    };
    let file = std::fs::read(&path).expect("read export.pdb");
    let len_page = usize_at(&file, 4).expect("len_page");
    assert!(len_page > 0x28, "implausible page length {len_page}");

    let mut forms: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_table: BTreeMap<u32, usize> = BTreeMap::new();
    let mut isrc_shaped_elsewhere = 0usize;
    let mut failures = Vec::new();
    let mut total = 0usize;
    for table in tables(&file) {
        if table.kind > KEYS {
            continue;
        }
        for page in page_chain(&file, len_page, &table) {
            let base = page * len_page;
            let page_end = base + len_page;
            for row in rows(&file, base, len_page) {
                for (rel, isrc_slot) in string_offsets(&file, table.kind, row) {
                    total += 1;
                    *per_table.entry(table.kind).or_default() += 1;
                    let at = row + rel;
                    let Some(field) = file.get(at..page_end) else {
                        failures.push(format!(
                            "table {} row {row:#x}: string past page",
                            table.kind
                        ));
                        continue;
                    };
                    if !isrc_slot && has_isrc_shape(field) {
                        isrc_shaped_elsewhere += 1;
                    }
                    match check(field, isrc_slot) {
                        Ok(form) => *forms.entry(format!("{form:?}")).or_default() += 1,
                        Err(why) => failures.push(format!(
                            "table {} row {row:#x} string at {at:#x}: {why}",
                            table.kind
                        )),
                    }
                }
            }
        }
    }

    eprintln!("export.pdb strings: {total}, {} failures", failures.len());
    for (kind, count) in &per_table {
        eprintln!("  table {kind}: {count} strings");
    }
    for (form, count) in &forms {
        eprintln!("  {form}: {count}");
    }
    eprintln!("  ISRC-shaped strings outside the ISRC slot: {isrc_shaped_elsewhere}");
    assert!(total > 0, "no strings found in {}", path.display());
    assert_eq!(
        isrc_shaped_elsewhere, 0,
        "ISRC-shaped strings outside the ISRC slot would be misread by slot-blind decoding"
    );
    assert!(
        failures.is_empty(),
        "{} of {total} strings failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
