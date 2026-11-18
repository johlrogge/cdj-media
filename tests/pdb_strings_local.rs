//! Decodes every DeviceSQL string in the rows of a real `export.pdb` in `local-fixtures/`,
//! checks that `DeviceSqlString::new` picks the form rekordbox picked, and that the string
//! bytes are exactly what the format description gives for the decoded value. Skips itself
//! when the fixture is missing.
//!
//! Rows are found through [`Pdb`]; only the string offsets within each row are read here,
//! as typed rows come in a later slice. Everything in `export.pdb` is little-endian.

mod common;

use std::collections::BTreeMap;

use cdj_media::encoding::{DeviceSqlString, StringForm};
use cdj_media::pdb::{ExportTable, Pdb, RawRow};

/// The tables whose rows hold strings checked here.
const KINDS: [ExportTable; 6] = [
    ExportTable::Tracks,
    ExportTable::Genres,
    ExportTable::Artists,
    ExportTable::Albums,
    ExportTable::Labels,
    ExportTable::Keys,
];

/// Number of string offsets in a track row.
const TRACK_STRINGS: usize = 21;

/// Strings in the real-stick fixture's tracks, genres, artists, albums, labels and keys.
const REAL_STICK_STRINGS: usize = 17_966;

fn u8_at(row: &[u8], at: usize) -> usize {
    usize::from(*row.get(at).expect("u8 in row"))
}

fn u16_at(row: &[u8], at: usize) -> usize {
    let bytes = row
        .get(at..)
        .and_then(|b| b.first_chunk())
        .expect("u16 in row");
    usize::from(u16::from_le_bytes(*bytes))
}

/// Row-relative offsets of the strings in a row, with whether each is the ISRC slot.
fn string_offsets(kind: ExportTable, row: &[u8]) -> Vec<(usize, bool)> {
    match kind {
        ExportTable::Genres | ExportTable::Labels => vec![(4, false)],
        ExportTable::Keys => vec![(8, false)],
        ExportTable::Artists => {
            let at = if u16_at(row, 0) == 0x64 {
                u16_at(row, 0x0a)
            } else {
                u8_at(row, 0x09)
            };
            vec![(at, false)]
        }
        ExportTable::Albums => {
            let at = if u16_at(row, 0) == 0x84 {
                u16_at(row, 0x16)
            } else {
                u8_at(row, 0x15)
            };
            vec![(at, false)]
        }
        ExportTable::Tracks => (0..TRACK_STRINGS)
            .map(|k| (u16_at(row, 0x5e + 2 * k), k == 0))
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the raw field has the ISRC shape: flag `0x90`, content `0x03` ... NUL.
fn has_isrc_shape(field: &[u8]) -> bool {
    let Some(len) = field
        .get(1..3)
        .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    else {
        return false;
    };
    field.first() == Some(&0x90)
        && len >= 6
        && field.get(4) == Some(&0x03)
        && field.get(len - 1) == Some(&0)
}

/// The bytes of a string field with pad byte 0, built from the format description rather
/// than the crate's encoder.
fn field_bytes(string: &DeviceSqlString) -> Vec<u8> {
    let text = string.as_str();
    let long = |flag: u8, content: &[u8]| {
        let len = u16::try_from(content.len() + 4).expect("field fits");
        let mut out = vec![flag];
        out.extend_from_slice(&len.to_le_bytes());
        out.push(0);
        out.extend_from_slice(content);
        out
    };
    match string.form() {
        StringForm::ShortAscii => {
            let mut out = vec![u8::try_from(((text.len() + 1) << 1) | 1).expect("short")];
            out.extend_from_slice(text.as_bytes());
            out
        }
        StringForm::LongAscii => long(0x40, text.as_bytes()),
        StringForm::Utf16Le => {
            let units: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
            long(0x90, &units)
        }
        StringForm::Isrc => {
            let content = [&[0x03][..], text.as_bytes(), &[0]].concat();
            long(0x90, &content)
        }
        other => panic!("unexpected form {other:?}"),
    }
}

/// Decodes one string field and checks it; returns its form, or why it failed.
fn check(row: &RawRow<'_>, at: usize, isrc_slot: bool) -> Result<StringForm, String> {
    let decoded = if isrc_slot {
        row.isrc_at(at)
    } else {
        row.string_at(at)
    }
    .map_err(|e| format!("decode: {e}"))?;
    // Rekordbox's form, with pad byte 0, as the constructors build it.
    let rekordbox = match decoded.form() {
        StringForm::Isrc => DeviceSqlString::isrc(decoded.as_str()).map_err(|e| e.to_string())?,
        _ => DeviceSqlString::new(decoded.as_str()),
    };
    if rekordbox != decoded {
        return Err(format!(
            "stored as {decoded:?}, rekordbox's form is {rekordbox:?}"
        ));
    }
    let expected = field_bytes(&decoded);
    let field = &row.bytes()[at..];
    if field.get(..expected.len()) != Some(&expected[..]) {
        return Err(format!(
            "bytes differ from the format description: {:02x?} vs {:02x?}",
            expected,
            &field[..expected.len().min(field.len())]
        ));
    }
    Ok(decoded.form())
}

#[test]
fn real_export_pdb_strings_round_trip_identically() {
    let Some(path) = common::local_fixture("real-stick/PIONEER/rekordbox/export.pdb") else {
        return;
    };
    let file = std::fs::read(&path).expect("read export.pdb");
    let pdb = Pdb::from_bytes(&file).expect("decode export.pdb");

    let mut forms: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_table: BTreeMap<String, usize> = BTreeMap::new();
    let mut isrc_shaped_elsewhere = 0usize;
    let mut failures = Vec::new();
    let mut total = 0usize;
    for kind in KINDS {
        for page in pdb.table_pages(kind) {
            for row in page.rows() {
                for (at, isrc_slot) in string_offsets(kind, row.bytes()) {
                    total += 1;
                    *per_table.entry(format!("{kind:?}")).or_default() += 1;
                    let place = format!(
                        "{kind:?} page {} row {:#x} string at {at:#x}",
                        page.page_index(),
                        row.offset()
                    );
                    let Some(field) = row.bytes().get(at..) else {
                        failures.push(format!("{place}: string past the heap"));
                        continue;
                    };
                    if !isrc_slot && has_isrc_shape(field) {
                        isrc_shaped_elsewhere += 1;
                    }
                    match check(&row, at, isrc_slot) {
                        Ok(form) => *forms.entry(format!("{form:?}")).or_default() += 1,
                        Err(why) => failures.push(format!("{place}: {why}")),
                    }
                }
            }
        }
    }

    eprintln!("export.pdb strings: {total}, {} failures", failures.len());
    for (kind, count) in &per_table {
        eprintln!("  {kind}: {count} strings");
    }
    for (form, count) in &forms {
        eprintln!("  {form}: {count}");
    }
    eprintln!("  ISRC-shaped strings outside the ISRC slot: {isrc_shaped_elsewhere}");
    assert_eq!(total, REAL_STICK_STRINGS, "strings in {}", path.display());
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

/// String layout facts for one table, across its rows.
#[derive(Default)]
struct Layout {
    rows: usize,
    /// Rows whose string offsets increase strictly in slot order.
    increasing: usize,
    /// Gap from the end of one string to the start of the next, in slot order.
    gaps: BTreeMap<i64, usize>,
    /// Row-relative offset of the first string.
    first_at: BTreeMap<usize, usize>,
    /// `(gap before it, form, heap offset mod 4)` of every string after the first.
    next_string: BTreeMap<(i64, String, usize), usize>,
    /// Strings not where the alignment rule puts them.
    rule_breaks: Vec<String>,
    /// Values of the gap bytes between strings.
    padding: BTreeMap<u8, usize>,
}

/// Reports, for ADR-005, whether each row's strings lie in slot order and back to back.
/// Asserts only what holds on every row of the fixture: offsets increase in slot order, and
/// each string after the first starts where the previous one ends, except that a long-form
/// string (long ASCII or UTF-16LE) starts at the next heap offset that is a multiple of 4.
#[test]
fn real_export_pdb_string_heap_layout() {
    let Some(path) = common::local_fixture("real-stick/PIONEER/rekordbox/export.pdb") else {
        return;
    };
    let file = std::fs::read(&path).expect("read export.pdb");
    let pdb = Pdb::from_bytes(&file).expect("decode export.pdb");

    let mut layouts: BTreeMap<String, Layout> = BTreeMap::new();
    for kind in KINDS {
        let layout = layouts.entry(format!("{kind:?}")).or_default();
        for page in pdb.table_pages(kind) {
            for row in page.rows() {
                let decoded: Vec<(usize, DeviceSqlString)> = string_offsets(kind, row.bytes())
                    .into_iter()
                    .map(|(at, isrc_slot)| {
                        let decoded = if isrc_slot {
                            row.isrc_at(at)
                        } else {
                            row.string_at(at)
                        }
                        .expect("string decodes");
                        (at, decoded)
                    })
                    .collect();
                let strings: Vec<(usize, usize)> = decoded
                    .iter()
                    .map(|(at, s)| (*at, field_bytes(s).len()))
                    .collect();
                let row_at = usize::from(row.offset());
                for (w, next) in strings.windows(2).zip(&decoded[1..]) {
                    let prev_end = w[0].0 + w[0].1;
                    let gap = w[1].0 as i64 - prev_end as i64;
                    let heap_at = row_at + w[1].0;
                    *layout
                        .next_string
                        .entry((gap, format!("{:?}", next.1.form()), heap_at % 4))
                        .or_default() += 1;
                    // Rule: short ASCII follows directly; a long form starts at the next
                    // heap offset that is a multiple of 4.
                    let expected = if next.1.form() == StringForm::ShortAscii {
                        prev_end
                    } else {
                        (row_at + prev_end).next_multiple_of(4) - row_at
                    };
                    if w[1].0 != expected {
                        layout.rule_breaks.push(format!(
                            "page {} row {:#x}: string at {:#x}, rule gives {expected:#x}",
                            page.page_index(),
                            row.offset(),
                            w[1].0
                        ));
                    }
                    for &b in &row.bytes()[prev_end..w[1].0.max(prev_end)] {
                        *layout.padding.entry(b).or_default() += 1;
                    }
                }
                layout.rows += 1;
                *layout.first_at.entry(strings[0].0).or_default() += 1;
                if strings.windows(2).all(|w| w[0].0 < w[1].0) {
                    layout.increasing += 1;
                }
                for w in strings.windows(2) {
                    let gap = w[1].0 as i64 - (w[0].0 + w[0].1) as i64;
                    *layout.gaps.entry(gap).or_default() += 1;
                }
            }
        }
    }

    eprintln!("export.pdb string heap layout, per table:");
    for (kind, layout) in &layouts {
        eprintln!(
            "  {kind}: {} rows, {} with offsets increasing in slot order",
            layout.rows, layout.increasing
        );
        eprintln!("    first string at (row-relative): {:?}", layout.first_at);
        if !layout.gaps.is_empty() {
            eprintln!("    gaps between consecutive strings: {:?}", layout.gaps);
            eprintln!("    (gap, next form, next heap offset mod 4): count");
            for (key, count) in &layout.next_string {
                eprintln!("      {key:?}: {count}");
            }
            eprintln!("    gap byte values: {:?}", layout.padding);
            eprintln!("    alignment rule breaks: {}", layout.rule_breaks.len());
        }
    }

    // What holds on every row of the fixture. The gap bytes are reported, not asserted:
    // most are 0, but some are leftover text, so typed rows must keep them.
    for (kind, layout) in &layouts {
        assert_eq!(
            layout.increasing, layout.rows,
            "{kind}: offsets in slot order"
        );
        assert!(
            layout.rule_breaks.is_empty(),
            "{kind}: strings off the alignment rule:\n{}",
            layout.rule_breaks.join("\n")
        );
    }
    let tracks = &layouts["Tracks"];
    assert_eq!(
        tracks.first_at.keys().collect::<Vec<_>>(),
        [&0x88],
        "track strings start right after the 21 string offsets"
    );
}
