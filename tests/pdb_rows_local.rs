//! Types every row of the track table (0) and the small tables (1 to 6 and 13) of the real
//! `export.pdb` in `local-fixtures/`, re-encodes it, rebuilds it from its domain values, and
//! reports how the rows lie in their heaps. Skips itself when the fixture is missing.
//!
//! Track rows hold the user's library, so the tests print shapes and counts only, never
//! track text.

mod common;

use std::collections::BTreeMap;
use std::fmt::Debug;

use cdj_media::encoding::StringForm;
use cdj_media::pdb::{
    Album, Artist, Artwork, Color, ExportRow, ExportTable, Genre, Key, Label, Page, PageBody, Pdb,
    RawRow, Track, TrackFields, TrackId, TrackString,
};

/// Track rows in the real-stick fixture.
const TRACK_ROWS: usize = 827;

/// The track string slots that are not understood.
const UNKNOWN_STRING_SLOTS: [TrackString; 7] = [
    TrackString::Unknown2,
    TrackString::Unknown3,
    TrackString::Unknown4,
    TrackString::Unknown8,
    TrackString::Unknown9,
    TrackString::Unknown13,
    TrackString::Unknown18,
];

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

/// Every track row, with its page and raw row.
fn tracks(pdb: &Pdb) -> Vec<(&Page, RawRow<'_>, Track)> {
    rows(pdb, ExportTable::Tracks)
        .into_iter()
        .map(|(page, raw, row)| match row {
            ExportRow::Track(track) => (page, raw, track),
            other => panic!("unexpected row {other:?}"),
        })
        .collect()
}

/// Where a row's re-encoding first differs from the stick, without showing any bytes.
fn first_difference(a: &[u8], b: &[u8]) -> String {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(at) => format!("first difference at row offset {at:#x}"),
        None => format!("lengths {} and {}", a.len(), b.len()),
    }
}

/// The shape of an analysis path: `h` for each hex digit of the two directory names, the
/// rest as is when it is rekordbox's layout; otherwise only its length.
fn analysis_path_shape(path: &str) -> String {
    let b = path.as_bytes();
    let hex = |r: std::ops::Range<usize>| {
        b.get(r).is_some_and(|s| {
            s.iter()
                .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(c))
        })
    };
    let file = path.get(31..).unwrap_or_default();
    let anlz_file = file.len() == 12
        && file.starts_with("ANLZ")
        && file[4..8].bytes().all(|c| c.is_ascii_digit())
        && file.ends_with(".DAT");
    if b.len() == 43
        && path.starts_with("/PIONEER/USBANLZ/P")
        && hex(18..21)
        && b[21] == b'/'
        && hex(22..30)
        && b[30] == b'/'
        && anlz_file
    {
        format!("/PIONEER/USBANLZ/Phhh/hhhhhhhh/{file}")
    } else {
        format!("other, {} bytes", path.len())
    }
}

/// Decodes every track row and checks it re-encodes to exactly its bytes; reports the
/// shapes of the analysis paths.
#[test]
fn real_stick_track_rows_round_trip_identically() {
    let Some(pdb) = load() else {
        return;
    };
    let tracks = tracks(&pdb);
    let mut failures = Vec::new();
    let mut paths: BTreeMap<String, usize> = BTreeMap::new();
    let mut titles: BTreeMap<String, usize> = BTreeMap::new();
    for (page, raw, track) in &tracks {
        let encoded = track.encode().expect("encode");
        let stick = raw.bytes().get(..encoded.len());
        if stick != Some(&encoded[..]) {
            failures.push(format!(
                "page {} row {:#x}: {}",
                page.page_index(),
                raw.offset(),
                first_difference(&encoded, stick.unwrap_or(raw.bytes()))
            ));
        }
        assert_eq!(raw.offset() % 4, 0, "track rows start at multiples of 4");
        *paths
            .entry(analysis_path_shape(track.analysis_path().as_str()))
            .or_default() += 1;
        *titles
            .entry(format!("{:?}", track.title().form()))
            .or_default() += 1;
    }
    eprintln!(
        "export.pdb track rows: {}, all re-encoded identically: {}; no id is 0 (TrackId is \
         non-zero); title forms {titles:?}",
        tracks.len(),
        failures.is_empty()
    );
    eprintln!("  analysis paths:");
    for (shape, count) in &paths {
        eprintln!("    {count:4} {shape}");
    }
    assert_eq!(tracks.len(), TRACK_ROWS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A track's domain values, as the builder takes them.
fn fields_of(t: &Track) -> TrackFields {
    let text = |s: &cdj_media::encoding::DeviceSqlString| s.as_str().to_owned();
    TrackFields {
        sample_rate: t.sample_rate(),
        composer_id: t.composer_id(),
        file_size: t.file_size(),
        artwork_id: t.artwork_id(),
        key_id: t.key_id(),
        original_artist_id: t.original_artist_id(),
        label_id: t.label_id(),
        remixer_id: t.remixer_id(),
        bitrate: t.bitrate(),
        track_number: t.track_number(),
        tempo: t.tempo(),
        genre_id: t.genre_id(),
        album_id: t.album_id(),
        artist_id: t.artist_id(),
        disc_number: t.disc_number(),
        play_count: t.play_count(),
        year: t.year(),
        sample_depth: t.sample_depth(),
        duration: t.duration(),
        color_id: t.color_id(),
        rating: t.rating(),
        file_type: t.file_type(),
        isrc: text(t.isrc()),
        lyricist: text(t.lyricist()),
        message: text(t.message()),
        publish_track_info: t.is_publish_track_info_on(),
        autoload_hot_cues: t.is_autoload_hot_cues_on(),
        date_added: text(t.date_added()),
        release_date: text(t.release_date()),
        mix_name: text(t.mix_name()),
        analysis_path: text(t.analysis_path()),
        analysis_date: text(t.analysis_date()),
        comment: text(t.comment()),
        title: text(t.title()),
        filename: text(t.filename()),
        file_path: text(t.file_path()),
    }
}

/// The named string slots: the ones `TrackFields` sets.
const NAMED_STRINGS: [TrackString; 14] = [
    TrackString::Isrc,
    TrackString::Lyricist,
    TrackString::Message,
    TrackString::PublishTrackInfo,
    TrackString::AutoloadHotCues,
    TrackString::DateAdded,
    TrackString::ReleaseDate,
    TrackString::MixName,
    TrackString::AnalysisPath,
    TrackString::AnalysisDate,
    TrackString::Comment,
    TrackString::Title,
    TrackString::Filename,
    TrackString::FilePath,
];

/// Rows of the real stick whose unknowns are all the builder's: the `bitmask`, the unknown
/// string slots and zero gap bytes.
const REKORDBOX_LIKE_TRACKS: usize = 272;

/// Rebuilds every track from its domain values with `Track::new` and compares it with the
/// stick. On every row: the fixed part from 0x08 to 0x14 and from 0x18 to the string offsets
/// is identical (that leaves out `subtype`, `index_shift`, `bitmask` and `unknown2`), and
/// every named string has the stick's text and form. Rows whose unknowns rekordbox writes
/// differently from the builder's defaults are counted by reason; the other 272 must be
/// identical byte for byte once `index_shift` (the page writer's) and `unknown2` (the id in
/// built rows, see ADR-008) are masked in both. Also checks that `index_shift` is 0x20 ×
/// the row's slot in its page's row index, absent rows included, on every row.
#[test]
fn real_stick_tracks_rebuild_from_their_values() {
    let Some(pdb) = load() else {
        return;
    };
    let tracks = tracks(&pdb);
    let defaults = Track::new(TrackId::new(1).unwrap(), TrackFields::default()).unwrap();
    let (mut exact, mut but_shift, mut but_shift_unknown2) = (0, 0, 0);
    let (mut fixed_match, mut strings_match) = (0, 0);
    let (mut shift_is_slot, mut shift_is_present_index) = (0, 0);
    let mut present_index: BTreeMap<u32, usize> = BTreeMap::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for (page, raw, track) in &tracks {
        let place = format!("page {} row {:#x}", page.page_index(), raw.offset());
        let original = track.encode().expect("encode");
        let rebuilt = Track::new(track.id(), fields_of(track)).expect("build");
        let built = rebuilt.encode().expect("encode built");
        let shift = usize::from(track.index_shift());
        shift_is_slot += usize::from(slot_of(page, raw.offset()).map(|s| s * 0x20) == Some(shift));
        let index = present_index.entry(page.page_index()).or_default();
        shift_is_present_index += usize::from(shift == *index * 0x20);
        *index += 1;

        // The fixed fields the builder takes from `TrackFields` or its constant defaults.
        let fixed = |b: &[u8]| [b[0x08..0x14].to_vec(), b[0x18..0x5e].to_vec()].concat();
        if fixed(&built) == fixed(&original) {
            fixed_match += 1;
        } else {
            failures.push(format!("{place}: fixed fields differ"));
        }
        let same_strings = NAMED_STRINGS.iter().all(|&s| {
            let (a, b) = (rebuilt.string(s), track.string(s));
            a.as_str() == b.as_str() && a.form() == b.form()
        });
        if same_strings {
            strings_match += 1;
        } else {
            failures.push(format!("{place}: a named string differs in text or form"));
        }

        exact += usize::from(built == original);
        let mask = |b: &[u8], unknown2: bool| {
            let mut b = b.to_vec();
            b[2..4].fill(0);
            if unknown2 {
                b[0x14..0x18].fill(0);
            }
            b
        };
        but_shift += usize::from(mask(&built, false) == mask(&original, false));
        let both_masked = mask(&built, true) == mask(&original, true);
        but_shift_unknown2 += usize::from(both_masked);

        let mut why = Vec::new();
        if track.bitmask() != defaults.bitmask() {
            why.push(format!("bitmask {:#x}", track.bitmask()));
        }
        for slot in UNKNOWN_STRING_SLOTS {
            if track.string(slot) != defaults.string(slot) {
                why.push(format!("{slot:?}"));
            }
        }
        if TrackString::ALL
            .iter()
            .any(|&s| track.string_gap(s).iter().any(|&b| b != 0))
        {
            why.push("non-zero gap bytes".to_owned());
        }
        if why.is_empty() && !both_masked {
            failures.push(format!(
                "{place}: {}",
                first_difference(&mask(&built, true), &mask(&original, true))
            ));
        }
        let why = if why.is_empty() {
            "none".to_owned()
        } else {
            why.join(", ")
        };
        *reasons.entry(why).or_default() += 1;
    }
    eprintln!(
        "track rows rebuilt with Track::new: {exact} of {} identical, {but_shift} identical \
         but for index_shift, {but_shift_unknown2} but for index_shift and unknown2; fixed \
         fields equal in {fixed_match}, named strings equal in text and form in \
         {strings_match}; index_shift is 0x20 × the row-index slot in {shift_is_slot}, × the \
         present-row index in {shift_is_present_index}",
        tracks.len()
    );
    eprintln!("  where rekordbox's row differs from the builder's defaults:");
    for (why, count) in &reasons {
        eprintln!("    {count:4} {why}");
    }
    assert_eq!(shift_is_slot, tracks.len(), "track index_shift");
    assert_eq!(
        reasons.get("none").copied(),
        Some(REKORDBOX_LIKE_TRACKS),
        "rows with no differing unknowns"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The filename (slot 19) equals the last component of the file path (slot 20) on every
/// track row. ADR-008: `ExportBuilder` derives the filename from the path, which relies on
/// this holding for rekordbox's rows.
#[test]
fn real_stick_filename_is_the_last_path_component() {
    let Some(pdb) = load() else {
        return;
    };
    let tracks = tracks(&pdb);
    let matching = tracks
        .iter()
        .filter(|(_, _, t)| {
            t.file_path().as_str().rsplit('/').next() == Some(t.filename().as_str())
        })
        .count();
    eprintln!(
        "filename equals the last file path component on {matching} of {} tracks",
        tracks.len()
    );
    assert_eq!(matching, TRACK_ROWS);
}

/// `unknown2` is different in every track row, so no constant can stand in for it
/// (ADR-008).
#[test]
fn real_stick_track_unknown2_is_unique() {
    let Some(pdb) = load() else {
        return;
    };
    let values: std::collections::BTreeSet<u32> =
        tracks(&pdb).iter().map(|(_, _, t)| t.unknown2()).collect();
    eprintln!("track unknown2: {} distinct values", values.len());
    assert_eq!(values.len(), TRACK_ROWS);
}

/// The two upper-case hex numbers of a rekordbox analysis path,
/// `/PIONEER/USBANLZ/Phhh/hhhhhhhh/ANLZnnnn.DAT`.
fn analysis_path_numbers(path: &str) -> Option<(u32, u32)> {
    let upper_hex = |s: &str| {
        s.bytes()
            .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
            .then(|| u32::from_str_radix(s, 16).ok())
            .flatten()
    };
    let rest = path.strip_prefix("/PIONEER/USBANLZ/P")?;
    let (dir, rest) = rest.split_once('/')?;
    let (sub, _) = rest.split_once('/')?;
    if dir.len() != 3 || sub.len() != 8 {
        return None;
    }
    Some((upper_hex(dir)?, upper_hex(sub)?))
}

/// Evidence for ADR-008 about `unknown2`: its range, a log2 histogram, whether it rises with
/// the track id, and whether it matches the hex numbers of the track's analysis path. Prints
/// numbers only; asserts nothing beyond what holds.
#[test]
fn real_stick_track_unknown2_evidence() {
    let Some(pdb) = load() else {
        return;
    };
    let mut tracks: Vec<Track> = tracks(&pdb).into_iter().map(|(_, _, t)| t).collect();
    tracks.sort_by_key(|t| t.id());
    let values: Vec<u32> = tracks.iter().map(Track::unknown2).collect();
    let (min, max) = (values.iter().min().unwrap(), values.iter().max().unwrap());
    let mut log2: BTreeMap<u32, usize> = BTreeMap::new();
    for v in &values {
        *log2.entry(32 - v.leading_zeros()).or_default() += 1;
    }
    let rising = values.windows(2).filter(|w| w[1] > w[0]).count();
    let mut matches: BTreeMap<&str, usize> = BTreeMap::new();
    let mut with_path = 0;
    for t in &tracks {
        let Some((dir, sub)) = analysis_path_numbers(t.analysis_path().as_str()) else {
            continue;
        };
        with_path += 1;
        let u = t.unknown2();
        for (name, hit) in [
            ("equals the 8-digit number", u == sub),
            ("equals the 3-digit number", u == dir),
            (
                "equals the low 28 bits of the 8-digit number",
                u == sub & 0x0fff_ffff,
            ),
            (
                "equals the 8-digit number xor the 3-digit number",
                u == sub ^ dir,
            ),
            (
                "low 16 bits equal those of the 8-digit number",
                u & 0xffff == sub & 0xffff,
            ),
            ("equals the track id", u == t.id().get()),
        ] {
            *matches.entry(name).or_default() += usize::from(hit);
        }
    }
    eprintln!(
        "track unknown2: min {min:#x}, max {max:#x}; rises from one track id to the next in \
         {rising} of {} steps",
        values.len() - 1
    );
    eprintln!("  bit length: count");
    for (bits, count) in &log2 {
        eprintln!("    {bits:2}: {count}");
    }
    eprintln!("  against the analysis path numbers ({with_path} rows with a rekordbox path):");
    for (name, count) in &matches {
        eprintln!("    {count:4} {name}");
    }
    assert!(*max < 1 << 28, "unknown2 fits 28 bits");
}

/// Evidence for ADR-008 about the analysis paths that end in `ANLZ0001.DAT`: whether another
/// track's path names `ANLZ0000.DAT` in the same directory, and which analysis files exist
/// on the stick for both names. Prints counts and booleans only.
#[test]
fn real_stick_anlz0001_evidence() {
    let Some(pdb) = load() else {
        return;
    };
    let Some(stick) = common::local_fixture("real-stick") else {
        return;
    };
    let paths: Vec<String> = tracks(&pdb)
        .iter()
        .map(|(_, _, t)| t.analysis_path().as_str().to_owned())
        .collect();
    let dir_of = |p: &str| p.rsplit_once('/').map(|(d, _)| d.to_owned());
    let mut seen = 0;
    for path in paths.iter().filter(|p| p.ends_with("/ANLZ0001.DAT")) {
        seen += 1;
        let dir = dir_of(path).expect("a directory");
        let shared = paths
            .iter()
            .filter(|p| *p != path && dir_of(p).as_deref() == Some(dir.as_str()))
            .count();
        let shared_0000 = paths.iter().any(|p| *p == format!("{dir}/ANLZ0000.DAT"));
        let exists = |name: &str| stick.join(dir.trim_start_matches('/')).join(name).exists();
        eprintln!(
            "ANLZ0001 path {seen}: other tracks in its directory {shared}, one of them names \
             ANLZ0000.DAT: {shared_0000}; files: ANLZ0000.DAT {}, ANLZ0000.EXT {}, \
             ANLZ0001.DAT {}, ANLZ0001.EXT {}",
            exists("ANLZ0000.DAT"),
            exists("ANLZ0000.EXT"),
            exists("ANLZ0001.DAT"),
            exists("ANLZ0001.EXT"),
        );
        assert!(
            shared_0000,
            "ANLZ0001 path {seen} shares its directory with ANLZ0000"
        );
        assert!(
            exists("ANLZ0001.DAT") && exists("ANLZ0001.EXT"),
            "ANLZ0001 path {seen}: its files exist"
        );
    }
    eprintln!("{seen} track(s) with an ANLZ0001.DAT analysis path");
    // What holds on the stick: every ANLZ0001 analysis file exists, and its directory is
    // shared with a track whose path names ANLZ0000.DAT.
    assert_eq!(seen, 2);
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
    // Track unknowns; `unknown2` differs in every row, so it has no default to check. The
    // unknown string slots hold only numbers or nothing, so their values are printed.
    let mut track_fields: BTreeMap<String, Histogram> = BTreeMap::new();
    for (_, _, t) in tracks(&pdb) {
        let mut add = |field: &str, value: &dyn Debug| {
            track_fields
                .entry(format!("track {field}"))
                .or_default()
                .add(value);
        };
        add("subtype", &t.subtype());
        add("bitmask", &t.bitmask());
        add("unknown3", &t.unknown3());
        add("unknown4", &t.unknown4());
        add("unknown5", &t.unknown5());
        add("unknown7", &t.unknown7());
        for slot in UNKNOWN_STRING_SLOTS {
            add(&format!("string {slot:?}"), &t.string(slot).as_str());
        }
    }

    // The defaults as the builders write them; `pdb::defaults` itself is crate-private.
    let track = Track::new(TrackId::new(1).unwrap(), TrackFields::default()).unwrap();
    let track_strings: Vec<(String, &str)> = UNKNOWN_STRING_SLOTS
        .iter()
        .map(|&slot| {
            (
                format!("track string {slot:?}"),
                track.string(slot).as_str(),
            )
        })
        .collect();
    let track_expected: [(&str, &dyn Debug); 6] = [
        ("track subtype", &track.subtype()),
        ("track bitmask", &track.bitmask()),
        ("track unknown3", &track.unknown3()),
        ("track unknown4", &track.unknown4()),
        ("track unknown5", &track.unknown5()),
        ("track unknown7", &track.unknown7()),
    ];
    let mut track_failures: Vec<String> = track_expected
        .iter()
        .map(|(field, value)| (field.to_string(), *value))
        .chain(
            track_strings
                .iter()
                .map(|(f, v)| (f.clone(), v as &dyn Debug)),
        )
        .filter_map(|(field, value)| track_fields.get(&field)?.check_mode(&field, value).err())
        .collect();
    assert_eq!(
        track_fields.len(),
        track_expected.len() + track_strings.len(),
        "every surveyed track field has a default"
    );
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
    let all = fields
        .iter()
        .map(|(f, h)| (f.to_string(), h))
        .chain(track_fields.iter().map(|(f, h)| (f.clone(), h)));
    for (field, histogram) in all {
        eprintln!("  {field}:");
        for (value, count) in &histogram.0 {
            eprintln!("    {count:5} {value}");
        }
    }
    let mut failures: Vec<String> = expected
        .iter()
        .filter_map(|(field, value)| fields.get(field)?.check_mode(field, value).err())
        .collect();
    assert_eq!(
        fields.len(),
        expected.len(),
        "every surveyed field has a default"
    );
    failures.append(&mut track_failures);
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
