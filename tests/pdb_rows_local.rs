//! Types every row of the small tables (1 to 6 and 13) of the real `export.pdb` in
//! `local-fixtures/`, re-encodes it, rebuilds it from its domain values, and reports how the
//! rows lie in their heaps. Skips itself when the fixture is missing.

mod common;

use std::collections::BTreeMap;
use std::fmt::Debug;

use cdj_media::encoding::StringForm;
use cdj_media::pdb::{
    Album, Artist, Artwork, Color, ExportRow, ExportTable, Genre, Key, Label, Page, PageBody, Pdb,
    RawRow,
};

const EXPORT: &str = "real-stick/PIONEER/rekordbox/export.pdb";

/// The typed tables, with their row counts in the real-stick fixture.
const TABLES: [(ExportTable, usize); 7] = [
    (ExportTable::Genres, 19),
    (ExportTable::Artists, 277),
    (ExportTable::Albums, 239),
    (ExportTable::Labels, 21),
    (ExportTable::Keys, 43),
    (ExportTable::Colors, 8),
    (ExportTable::Artwork, 787),
];

fn load() -> Option<Pdb> {
    let path = common::local_fixture(EXPORT)?;
    let bytes = std::fs::read(&path).expect("read export.pdb");
    Some(Pdb::from_bytes(&bytes).expect("decode export.pdb"))
}

/// Every present row of `table`, with its page and decoded row.
fn rows(pdb: &Pdb, table: ExportTable) -> Vec<(&Page, RawRow<'_>, ExportRow)> {
    let mut out = Vec::new();
    for page in pdb.table_pages(table) {
        for raw in page.rows() {
            let row = ExportRow::decode(table, &raw)
                .unwrap_or_else(|e| {
                    panic!(
                        "{table:?} page {} row {:#x}: {e}",
                        page.page_index(),
                        raw.offset()
                    )
                })
                .unwrap_or_else(|| panic!("{table:?} is not typed"));
            out.push((page, raw, row));
        }
    }
    out
}

#[test]
fn real_export_pdb_rows_round_trip_identically() {
    let Some(pdb) = load() else {
        return;
    };
    let mut failures = Vec::new();
    let mut total = 0;
    eprintln!("export.pdb typed rows:");
    for (table, expected) in TABLES {
        let rows = rows(&pdb, table);
        let mut forms: BTreeMap<String, usize> = BTreeMap::new();
        let mut far = 0;
        for (page, raw, row) in &rows {
            let encoded = row.encode().expect("encode");
            if raw.bytes().get(..encoded.len()) != Some(&encoded[..]) {
                failures.push(format!(
                    "{table:?} page {} row {:#x}: re-encoded bytes differ",
                    page.page_index(),
                    raw.offset()
                ));
            }
            let (name, is_far) = match row {
                ExportRow::Genre(r) => (r.name(), false),
                ExportRow::Artist(r) => (r.name(), r.is_far()),
                ExportRow::Album(r) => (r.name(), r.is_far()),
                ExportRow::Label(r) => (r.name(), false),
                ExportRow::Key(r) => (r.name(), false),
                ExportRow::Color(r) => (r.name(), false),
                ExportRow::Artwork(r) => (r.path(), false),
                other => panic!("unexpected row {other:?}"),
            };
            *forms.entry(format!("{:?}", name.form())).or_default() += 1;
            far += usize::from(is_far);
        }
        eprintln!(
            "  {table:?}: {} rows, all re-encoded; {far} far; forms {forms:?}",
            rows.len()
        );
        assert_eq!(rows.len(), expected, "{table:?} rows");
        total += rows.len();
    }
    eprintln!("  {total} rows; no row id is 0 (the ids are non-zero types)");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The domain values of a row rebuilt with the public builder, encoded.
fn rebuilt(row: &ExportRow) -> Vec<u8> {
    let name = |s: &cdj_media::encoding::DeviceSqlString| s.as_str().to_owned();
    match row {
        ExportRow::Genre(r) => Genre::new(r.id(), &name(r.name())).encode(),
        ExportRow::Artist(r) => Artist::new(r.id(), &name(r.name())).encode(),
        ExportRow::Album(r) => Album::new(r.id(), &name(r.name()), r.artist_id()).encode(),
        ExportRow::Label(r) => Label::new(r.id(), &name(r.name())).encode(),
        ExportRow::Key(r) => Key::new(r.id(), &name(r.name())).encode(),
        ExportRow::Color(r) => Color::new(r.id(), &name(r.name())).encode(),
        ExportRow::Artwork(r) => Artwork::new(r.id(), &name(r.path())).encode(),
        other => panic!("unexpected row {other:?}"),
    }
    .expect("encode rebuilt row")
}

/// The row's slot in its page's row index, counting absent rows too.
fn slot_of(page: &Page, offset: u16) -> Option<usize> {
    let PageBody::Data(body) = page.body() else {
        return None;
    };
    body.row_groups()
        .iter()
        .flat_map(|g| g.offsets())
        .position(|&o| o == offset)
}

/// Rebuilds every row from its id, name and references with the public builders, and
/// checks them byte for byte against the stick. Artist and album `index_shift` depends on
/// the row's place in its page, which a builder does not know, so those two bytes are
/// compared separately; the test checks that `index_shift` is 0x20 × the row's slot in the
/// page's row index, absent rows included. The only other difference allowed is a key
/// whose `id2` is not its id (1 row on the stick).
#[test]
fn real_stick_rows_rebuild_from_their_values() {
    let Some(pdb) = load() else {
        return;
    };
    let mut failures = Vec::new();
    eprintln!("rows rebuilt with the public builders:");
    for (table, _) in TABLES {
        let rows = rows(&pdb, table);
        let (mut exact, mut but_shift) = (0, 0);
        let mut shift_is_slot = 0;
        let mut shift_is_present_index = 0;
        let mut key_id2 = 0;
        let mut present_index: BTreeMap<u32, usize> = BTreeMap::new();
        for (page, raw, row) in &rows {
            let original = row.encode().expect("encode");
            let built = rebuilt(row);
            let index = present_index.entry(page.page_index()).or_default();
            let shift = match row {
                ExportRow::Artist(r) => Some(r.index_shift()),
                ExportRow::Album(r) => Some(r.index_shift()),
                _ => None,
            };
            if let Some(shift) = shift {
                let shift = usize::from(shift);
                shift_is_slot +=
                    usize::from(Some(shift) == slot_of(page, raw.offset()).map(|s| s * 0x20));
                shift_is_present_index += usize::from(shift == *index * 0x20);
            }
            *index += 1;
            let mut masked = original.clone();
            if shift.is_some() {
                masked[2..4].copy_from_slice(&[0, 0]);
            }
            if built == original {
                exact += 1;
            }
            let odd_id2 = matches!(row, ExportRow::Key(key) if key.id2() != key.id().get());
            if built == masked {
                but_shift += 1;
            } else if odd_id2 {
                key_id2 += 1;
            } else {
                failures.push(format!(
                    "{table:?} page {} row {:#x}: rebuilt {built:02x?}, stick {original:02x?}",
                    page.page_index(),
                    raw.offset()
                ));
            }
        }
        eprint!(
            "  {table:?}: {exact} of {} identical, {but_shift} identical but for index_shift",
            rows.len()
        );
        if table == ExportTable::Artists || table == ExportTable::Albums {
            eprint!(
                "; index_shift is 0x20 × the row-index slot in {shift_is_slot}, \
                 × the present-row index in {shift_is_present_index}"
            );
            // Absent rows count: the slot, not the present-row index, gives index_shift.
            assert_eq!(shift_is_slot, rows.len(), "{table:?} index_shift");
        }
        if key_id2 > 0 {
            eprint!("; {key_id2} differ only in id2");
        }
        eprintln!();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Checks rekordbox's near/far choice: every artist and album row is near, including those
/// whose name sits at the next multiple of 4 after zero gap bytes because it is long-form.
#[test]
fn real_stick_artist_and_album_rows_are_near() {
    let Some(pdb) = load() else {
        return;
    };
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for table in [ExportTable::Artists, ExportTable::Albums] {
        for (_, raw, row) in rows(&pdb, table) {
            let (far, gap, form) = match &row {
                ExportRow::Artist(r) => (r.is_far(), r.name_gap().to_vec(), r.name().form()),
                ExportRow::Album(r) => (r.is_far(), r.name_gap().to_vec(), r.name().form()),
                other => panic!("unexpected row {other:?}"),
            };
            assert_eq!(
                raw.offset() % 4,
                0,
                "{table:?} rows start at multiples of 4"
            );
            *seen
                .entry(format!("{table:?} far {far} form {form:?} gap {gap:?}"))
                .or_default() += 1;
        }
    }
    eprintln!("artist and album layouts:");
    for (layout, count) in &seen {
        eprintln!("  {count:4} {layout}");
    }
    for layout in seen.keys() {
        let short = layout.contains("ShortAscii");
        let expected_gap = if short { "gap []" } else { "gap [0, 0]" };
        assert!(
            layout.contains("far false") && layout.ends_with(expected_gap),
            "{layout}"
        );
    }
    assert!(
        seen.keys()
            .any(|l| l.contains(&format!("{:?}", StringForm::Utf16Le)))
    );
}

/// Distribution of one field's values.
#[derive(Default)]
struct Histogram(BTreeMap<String, usize>);

impl Histogram {
    fn add(&mut self, value: impl Debug) {
        *self.0.entry(format!("{value:?}")).or_default() += 1;
    }

    /// Checks that `expected` is strictly the most common value; returns why not.
    fn check_mode(&self, field: &str, expected: impl Debug) -> Result<(), String> {
        let expected = format!("{expected:?}");
        let count = self.0.get(&expected).copied().unwrap_or(0);
        match self.0.iter().find(|(v, n)| **v != expected && **n >= count) {
            None if count > 0 => Ok(()),
            _ => Err(format!(
                "{field}: default {expected} ({count}) is not the most common value"
            )),
        }
    }
}

/// Checks that each `pdb::defaults` value, as the builders write it, is the most common
/// value of its field across the stick's rows, and prints every distribution.
#[test]
fn real_stick_row_defaults_are_modal() {
    let Some(pdb) = load() else {
        return;
    };
    let mut fields: BTreeMap<&str, Histogram> = BTreeMap::new();
    for (table, _) in TABLES {
        for (_, _, row) in rows(&pdb, table) {
            match row {
                ExportRow::Artist(r) => {
                    fields.entry("artist subtype").or_default().add(r.subtype());
                    fields
                        .entry("artist unknown1")
                        .or_default()
                        .add(r.unknown1());
                }
                ExportRow::Album(r) => {
                    fields.entry("album subtype").or_default().add(r.subtype());
                    fields
                        .entry("album unknown1")
                        .or_default()
                        .add(r.unknown1());
                    fields
                        .entry("album unknown2")
                        .or_default()
                        .add(r.unknown2());
                    fields
                        .entry("album unknown3")
                        .or_default()
                        .add(r.unknown3());
                }
                ExportRow::Color(r) => {
                    fields
                        .entry("color unknown1")
                        .or_default()
                        .add(r.unknown1());
                    fields
                        .entry("color unknown3")
                        .or_default()
                        .add(r.unknown3());
                }
                _ => {}
            }
        }
    }

    // The defaults as the builders write them; `pdb::defaults` itself is crate-private.
    let artist = Artist::new(cdj_media::pdb::ArtistId::new(1).unwrap(), "");
    let album = Album::new(cdj_media::pdb::AlbumId::new(1).unwrap(), "", None);
    let color = Color::new(cdj_media::pdb::ColorId::new(1).unwrap(), "");
    let expected: [(&str, &dyn Debug); 8] = [
        ("artist subtype", &artist.subtype()),
        ("artist unknown1", &artist.unknown1()),
        ("album subtype", &album.subtype()),
        ("album unknown1", &album.unknown1()),
        ("album unknown2", &album.unknown2()),
        ("album unknown3", &album.unknown3()),
        ("color unknown1", &color.unknown1()),
        ("color unknown3", &color.unknown3()),
    ];

    eprintln!("unknown row fields:");
    for (field, histogram) in &fields {
        eprintln!("  {field}:");
        for (value, count) in &histogram.0 {
            eprintln!("    {count:5} {value}");
        }
    }
    let failures: Vec<String> = expected
        .iter()
        .filter_map(|(field, value)| fields.get(field)?.check_mode(field, value).err())
        .collect();
    assert_eq!(
        fields.len(),
        expected.len(),
        "every surveyed field has a default"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Where the next row starts after a row that starts at heap offset `start` and is `len`
/// bytes long, if rekordbox's padding rule holds: the next multiple of 4 for rows with an
/// inline string; for artists and albums, the length the row would have with the far
/// layout's 2 extra bytes, rounded up to a multiple of 4, plus 4.
fn padded_end(row: &ExportRow, start: usize, len: usize) -> usize {
    // Near fixed part and far fixed part; every real row is near.
    let (near, far) = match row {
        ExportRow::Artist(r) => ((10 + r.name_gap().len()), 12),
        ExportRow::Album(r) => ((22 + r.name_gap().len()), 24),
        _ => return (start + len).next_multiple_of(4),
    };
    let name_len = len - near;
    start + (far + name_len).next_multiple_of(4) + 4
}

/// Reports what lies between consecutive rows of each page's heap (or between the last row
/// and the page's `used_size`), and asserts what holds on every page of the fixture: rows
/// start at multiples of 4; unless deleted rows lie in between, the next row never starts
/// before [`padded_end`], and where the bytes in between are all zero it starts exactly
/// there. The other gaps hold leftover text of longer, earlier versions of the rows (on 4
/// pages), so they are kept by the page heap, not by the rows.
#[test]
fn real_stick_inter_row_padding() {
    let Some(pdb) = load() else {
        return;
    };
    let mut failures = Vec::new();
    eprintln!("bytes between consecutive rows in the heap:");
    for (table, _) in TABLES {
        let mut gaps: BTreeMap<String, usize> = BTreeMap::new();
        for page in pdb.table_pages(table) {
            let PageBody::Data(body) = page.body() else {
                continue;
            };
            let deleted: Vec<usize> = body
                .row_groups()
                .iter()
                .flat_map(|g| {
                    g.offsets()
                        .iter()
                        .enumerate()
                        .filter(|&(i, _)| g.rowpf() & (1 << i) == 0)
                        .map(|(_, &o)| usize::from(o))
                })
                .collect();
            let mut spans: Vec<(usize, usize, ExportRow)> = page
                .rows()
                .map(|raw| {
                    let row = ExportRow::decode(table, &raw).unwrap().unwrap();
                    let len = row.encode().unwrap().len();
                    (usize::from(raw.offset()), len, row)
                })
                .collect();
            spans.sort_by_key(|s| s.0);
            let used = usize::from(page.used_size());
            for (i, (start, len, row)) in spans.iter().enumerate() {
                let end = start + len;
                let next = spans.get(i + 1).map_or(used, |s| s.0);
                let bytes = &body.heap()[end..next.max(end)];
                let zero = bytes.iter().all(|&b| b == 0);
                let holds_deleted = deleted.iter().any(|&d| d >= end && d < next);
                let rule = padded_end(row, *start, *len) == next;
                let place = if i + 1 == spans.len() {
                    "last"
                } else {
                    "between"
                };
                *gaps
                    .entry(format!(
                        "{place}: {} bytes, zero {zero}, rule {rule}, deleted rows {holds_deleted}",
                        next as i64 - end as i64
                    ))
                    .or_default() += 1;
                if start % 4 != 0 {
                    failures.push(format!(
                        "{table:?} page {}: row at {start}",
                        page.page_index()
                    ));
                }
                let room = next >= padded_end(row, *start, *len);
                if !holds_deleted && (!room || (zero && !rule)) {
                    failures.push(format!(
                        "{table:?} page {} row at {start} ({len} bytes): next at {next}, \
                         gap {bytes:02x?}",
                        page.page_index()
                    ));
                }
            }
        }
        eprintln!("  {table:?}:");
        for (gap, count) in &gaps {
            eprintln!("    {count:4} {gap}");
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
