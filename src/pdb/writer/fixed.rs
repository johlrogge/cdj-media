//! The tables rekordbox fills when it creates a database: colours (table 6) and browse
//! columns (table 16). Both hold rekordbox's built-in rows, the same in every library; the
//! real stick's pages of both were written once, with the database (`sequence` 2 and 3),
//! and never again.

use super::super::{Color, ColorId, ExportRow};
use super::HeapRow;
use crate::bytes::Writer;
use crate::encoding::DeviceSqlString;
use crate::error::EncodeError;

/// rekordbox's colour names, for ids 1 to 8. djl-analysis gives the colours; the names are
/// rekordbox's defaults, which users may rename.
const COLOR_NAMES: [&str; 8] = [
    "Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple",
];

/// rekordbox's browse columns: id, the `u16` after it (not understood) and the label, which
/// is stored between U+FFFA and U+FFFB. Ids run 1 to 27; the second numbers do not follow
/// them.
const COLUMNS: [(u16, u16, &str); 27] = [
    (1, 0x80, "GENRE"),
    (2, 0x81, "ARTIST"),
    (3, 0x82, "ALBUM"),
    (4, 0x83, "TRACK"),
    (5, 0x85, "BPM"),
    (6, 0x86, "RATING"),
    (7, 0x87, "YEAR"),
    (8, 0x88, "REMIXER"),
    (9, 0x89, "LABEL"),
    (10, 0x8a, "ORIGINAL ARTIST"),
    (11, 0x8b, "KEY"),
    (12, 0x8d, "CUE"),
    (13, 0x8e, "COLOR"),
    (14, 0x92, "TIME"),
    (15, 0x93, "BITRATE"),
    (16, 0x94, "FILE NAME"),
    (17, 0x84, "PLAYLIST"),
    (18, 0x98, "HOT CUE BANK"),
    (19, 0x95, "HISTORY"),
    (20, 0x91, "SEARCH"),
    (21, 0x96, "COMMENTS"),
    (22, 0x8c, "DATE ADDED"),
    (23, 0x97, "DJ PLAY COUNT"),
    (24, 0x90, "FOLDER"),
    (25, 0xa1, "DEFAULT"),
    (26, 0xa2, "ALPHABET"),
    (27, 0xaa, "MATCHING"),
];

/// rekordbox's eight colour rows, in id order.
pub(crate) fn default_colors() -> Vec<ExportRow> {
    (1u16..)
        .zip(COLOR_NAMES)
        .filter_map(|(id, name)| Some(ExportRow::Color(Color::new(ColorId::new(id)?, name))))
        .collect()
}

/// rekordbox's browse column rows, in id order. A row is its id (`u16`), the second `u16`
/// and the label as a DeviceSQL string; each takes its length rounded up to 4 in the heap,
/// like the other one-string rows.
///
/// # Errors
///
/// None in practice: the labels are short. The string encoder's errors otherwise.
pub(crate) fn column_rows() -> Result<Vec<HeapRow>, EncodeError> {
    COLUMNS
        .iter()
        .map(|&(id, number, label)| {
            let mut w = Writer::new();
            w.u16_le(id);
            w.u16_le(number);
            DeviceSqlString::new(&format!("\u{fffa}{label}\u{fffb}")).encode(&mut w)?;
            let bytes = w.into_inner();
            Ok(HeapRow {
                footprint: bytes.len().next_multiple_of(4),
                bytes,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_rekordbox_defaults() {
        let colors = default_colors();
        assert_eq!(colors.len(), 8);
        // Colour 1, "Pink", as rekordbox writes it.
        let pink = [0, 0, 0, 0, 1, 1, 0, 0, 0x0b, b'P', b'i', b'n', b'k'];
        assert_eq!(colors[0].encode().unwrap(), pink);
        let ExportRow::Color(purple) = &colors[7] else {
            panic!("colour row");
        };
        assert_eq!(purple.id(), ColorId::new(8).unwrap());
        assert_eq!(purple.name().as_str(), "Purple");
    }

    #[test]
    fn a_column_row_is_its_ids_and_a_bracketed_utf16_label() {
        let rows = column_rows().unwrap();
        assert_eq!(rows.len(), 27);
        let mut bpm = vec![5, 0, 0x85, 0, 0x90, 0x0e, 0, 0, 0xfa, 0xff];
        bpm.extend_from_slice(&[b'B', 0, b'P', 0, b'M', 0, 0xfb, 0xff]);
        assert_eq!(rows[4].bytes, bpm);
        assert_eq!(rows[4].footprint, 20);
        // 4 + 4 + 2 × 7 = 22 bytes, rounded up to 24.
        assert_eq!((rows[0].bytes.len(), rows[0].footprint), (22, 24));
    }
}
