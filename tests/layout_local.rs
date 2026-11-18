//! Checks the stick path layout against the real stick in `local-fixtures/`: the shapes of
//! the audio file paths (track slot 20), the analysis paths (slot 14), and the `PPTH` path
//! in each track's analysis file. Skips itself when the fixture is missing.
//!
//! The paths hold the user's library, so the tests print shapes and counts only, never
//! path text.

mod common;

use std::collections::BTreeMap;

use cdj_media::anlz::AnlzFile;
use cdj_media::layout::{AnlzPath, ContentPath, StickPath, audio_path_tag};
use cdj_media::pdb::model::Track;
use cdj_media::pdb::{Database, Pdb, TrackId};

const EXPORT: &str = "real-stick/PIONEER/rekordbox/export.pdb";

/// Track rows in the real-stick fixture.
const TRACKS: usize = 827;

fn load() -> Option<Database> {
    let path = common::local_fixture(EXPORT)?;
    let bytes = std::fs::read(&path).expect("read export.pdb");
    let pdb = Pdb::from_bytes(&bytes).expect("decode export.pdb");
    Some(Database::from_pdb(&pdb).expect("load the database"))
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The class of a character that matters for FAT32 or for sanitising, if any.
fn char_class(c: char) -> Option<String> {
    match c {
        '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => Some(format!("forbidden {c:?}")),
        c if c < '\u{20}' => Some("control".to_owned()),
        '_' => Some("underscore".to_owned()),
        '\u{0300}'..='\u{036f}' => Some("non-ASCII: combining diacritic".to_owned()),
        '\u{80}'..='\u{ff}' => Some("non-ASCII: Latin-1".to_owned()),
        c if c > '\u{ffff}' => Some("non-ASCII: outside the BMP".to_owned()),
        c if !c.is_ascii() => Some("non-ASCII: other BMP".to_owned()),
        _ => None,
    }
}

/// How a path directory relates to the name it is made from.
fn relation(dir: &str, name: Option<&str>) -> &'static str {
    match name {
        None if dir == "UnknownArtist" || dir == "UnknownAlbum" => "no name; dir is UnknownX",
        None if dir == "Unknown Artist" || dir == "Unknown Album" => "no name; dir is Unknown X",
        None => "no name; dir is something else",
        Some(name) if name == dir => "equal to the name",
        Some("") => "empty name",
        Some(name) if name.starts_with(dir) => "a prefix of the name",
        Some(name) if name.chars().count() == dir.chars().count() => "same length, other text",
        Some(_) => "other",
    }
}

/// Every audio file path (slot 20) and analysis path (slot 14) on the stick is a valid
/// `StickPath`. Prints the rule each failing path breaks, never the path.
#[test]
fn real_stick_paths_parse_as_stick_paths() {
    let Some(db) = load() else {
        return;
    };
    let mut errors: BTreeMap<String, usize> = BTreeMap::new();
    let mut ok = 0;
    for t in db.tracks() {
        for (slot, path) in [("slot 20", t.file_path()), ("slot 14", t.analysis_path())] {
            match StickPath::parse(path) {
                Ok(parsed) => {
                    ok += 1;
                    assert_eq!(parsed.as_str(), path);
                }
                Err(e) => *errors.entry(format!("{slot}: {e}")).or_default() += 1,
            }
        }
    }
    eprintln!(
        "stick paths: {ok} of {} parse; errors {errors:?}",
        2 * TRACKS
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(ok, 2 * TRACKS);
}

/// The ANLZ file number of a path of rekordbox's analysis shape,
/// `^/PIONEER/USBANLZ/P[0-9A-F]{3}/[0-9A-F]{8}/ANLZ[0-9]{4}\.(DAT|EXT)$`, and its
/// extension.
fn rekordbox_anlz_shape(path: &str) -> Option<(&str, &str)> {
    let upper_hex = |s: &str, len: usize| {
        s.len() == len && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'A'..=b'F'))
    };
    let rest = path.strip_prefix("/PIONEER/USBANLZ/P")?;
    let [dir, sub, file] = rest.split('/').collect::<Vec<_>>()[..] else {
        return None;
    };
    let (name, ext) = file.split_once('.')?;
    let number = name.strip_prefix("ANLZ")?;
    let ok = upper_hex(dir, 3)
        && upper_hex(sub, 8)
        && number.len() == 4
        && number.bytes().all(|c| c.is_ascii_digit())
        && (ext == "DAT" || ext == "EXT");
    ok.then_some((number, ext))
}

/// Every analysis path (slot 14) has rekordbox's shape, the shape `AnlzPath` writes:
/// 825 name `ANLZ0000.DAT`, 2 `ANLZ0001.DAT`.
#[test]
fn real_stick_analysis_paths_have_the_anlz_path_shape() {
    let Some(db) = load() else {
        return;
    };
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();
    for t in db.tracks() {
        let shape = rekordbox_anlz_shape(t.analysis_path())
            .map_or("other".to_owned(), |(n, e)| format!("ANLZ{n}.{e}"));
        *shapes.entry(shape).or_default() += 1;
    }
    eprintln!("analysis paths (slot 14): {shapes:?}");
    let expected = BTreeMap::from([
        ("ANLZ0000.DAT".to_owned(), 825),
        ("ANLZ0001.DAT".to_owned(), 2),
    ]);
    assert_eq!(shapes, expected);

    // The shape check accepts what `AnlzPath` writes, and only that shape.
    let path = AnlzPath::for_track(TrackId::new(0xabc_def0).unwrap());
    assert_eq!(
        rekordbox_anlz_shape(path.dat().as_str()),
        Some(("0000", "DAT"))
    );
    assert_eq!(
        rekordbox_anlz_shape(path.ext().as_str()),
        Some(("0000", "EXT"))
    );
    for bad in [
        "/PIONEER/USBANLZ/Pabc/0ABCDEF0/ANLZ0000.DAT",
        "/PIONEER/USBANLZ/PABC/0ABCDEF/ANLZ0000.DAT",
        "/PIONEER/USBANLZ/PABC/0ABCDEF0/ANLZ000.DAT",
        "/PIONEER/USBANLZ/PABC/0ABCDEF0/ANLZ0000.2EX",
        "/PIONEER/USBANLZ/PABC/0ABCDEF0/x/ANLZ0000.DAT",
    ] {
        assert_eq!(rekordbox_anlz_shape(bad), None, "{bad}");
    }
}

/// A track's `ContentPath`, from its artist and album names (empty when it has none) and
/// its file name.
fn content_path(db: &Database, t: &Track) -> ContentPath {
    ContentPath::for_track(
        db.artist_of(t).map_or("", |a| a.name()),
        db.album_of(t).map_or("", |a| a.name()),
        t.filename(),
    )
}

/// The `PPTH` path of each track's analysis files equals the track's audio file path
/// (slot 20) and the tag [`audio_path_tag`] builds from the track's `ContentPath`. Loads
/// each `.DAT` through slot 14 and the `.EXT` next to it; skips files missing from the
/// stick and counts them.
#[test]
fn real_stick_ppth_is_the_audio_file_path() {
    let Some(db) = load() else {
        return;
    };
    let Some(stick) = common::local_fixture("real-stick") else {
        return;
    };
    let mut counts: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    for t in db.tracks() {
        let expected = audio_path_tag(&content_path(&db, t));
        assert_eq!(expected.path(), t.file_path(), "the tag is slot 20");
        let dat = t.analysis_path();
        let ext = format!(
            "{}.EXT",
            dat.strip_suffix(".DAT").expect("slot 14 ends in .DAT")
        );
        for (kind, path) in [("DAT", dat), ("EXT", ext.as_str())] {
            let (equal, differ, missing) = counts.entry(kind).or_default();
            let file = stick.join(path.trim_start_matches('/'));
            let Ok(bytes) = std::fs::read(&file) else {
                *missing += 1;
                continue;
            };
            let anlz = AnlzFile::from_bytes(&bytes).expect("decode an analysis file");
            if anlz.path() == Some(expected.path()) {
                *equal += 1;
            } else {
                *differ += 1;
            }
        }
    }
    for (kind, (equal, differ, missing)) in &counts {
        eprintln!(
            "PPTH of .{kind} equals slot 20 on {equal} tracks, differs on {differ}; \
             {missing} files missing"
        );
    }
    // One track's analysis files are missing from the stick (see docs/format-notes.md).
    assert_eq!(counts["DAT"], (TRACKS - 1, 0, 1));
    assert_eq!(counts["EXT"], (TRACKS - 1, 0, 1));
}

/// `ContentPath::for_track`, given a track's artist and album names and its file name,
/// gives back the stick's audio file path (slot 20) on all 827 tracks, including those
/// without an artist or album, which go under `UnknownArtist` and `UnknownAlbum`. Counts
/// the tracks by whether they have an artist and an album. Prints counts only.
#[test]
fn real_stick_content_paths_match_rekordbox() {
    let Some(db) = load() else {
        return;
    };
    // (same, different)
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for t in db.tracks() {
        let built = content_path(&db, t);
        let class = match (db.artist_of(t), db.album_of(t)) {
            (Some(_), Some(_)) => "artist and album".to_owned(),
            (a, b) => format!("artist {}, album {}", a.is_some(), b.is_some()),
        };
        let (same, other) = counts.entry(class).or_default();
        let filename_is_slot_19 = built.file_name() == t.filename();
        if built.as_stick_path().as_str() == t.file_path() && filename_is_slot_19 {
            *same += 1;
        } else {
            *other += 1;
        }
    }
    eprintln!("ContentPath against slot 20 (same, different):");
    for (class, counts) in &counts {
        eprintln!("  {class}: {counts:?}");
    }
    let expected = BTreeMap::from([
        ("artist and album".to_owned(), (776, 0)),
        ("artist false, album false".to_owned(), (24, 0)),
        ("artist true, album false".to_owned(), (27, 0)),
    ]);
    assert_eq!(counts, expected);
}

/// A character as the survey may show it: ASCII punctuation as is, the rest by class.
fn shown(c: char) -> String {
    match c {
        c if c.is_ascii_punctuation() || c == ' ' => format!("{c:?}"),
        c if c.is_ascii_alphanumeric() => "ASCII alphanumeric".to_owned(),
        c if c < '\u{20}' => "control".to_owned(),
        c => char_class(c).unwrap_or_else(|| "other".to_owned()),
    }
}

/// Compares a directory with the name it is made from, without showing either: which
/// characters differ position by position (same length), or the lengths otherwise.
fn differences(dir: &str, name: &str, out: &mut BTreeMap<String, usize>) {
    let (d, n): (Vec<char>, Vec<char>) = (dir.chars().collect(), name.chars().collect());
    if d.len() == n.len() {
        let last = n.len() - 1;
        for (i, (a, b)) in n.iter().zip(&d).enumerate().filter(|(_, (a, b))| a != b) {
            let at = if i == last { "last" } else { "inner" };
            *out.entry(format!("{} -> {} ({at})", shown(*a), shown(*b)))
                .or_default() += 1;
        }
    } else {
        // Where the two first differ, and the directory's and name's lengths.
        let common = n.iter().zip(&d).take_while(|(a, b)| a == b).count();
        let replaced = n
            .iter()
            .zip(&d)
            .filter(|(a, b)| a != b)
            .map(|(a, b)| format!("{} -> {}", shown(*a), shown(*b)))
            .collect::<Vec<_>>();
        *out.entry(format!(
            "lengths: dir {} units, name {} units; first {common} chars equal; \
             pairwise differences {replaced:?}; dir ends with {:?}",
            utf16_len(dir),
            utf16_len(name),
            d.last().map(|&c| shown(c))
        ))
        .or_default() += 1;
    }
}

/// Surveys how rekordbox turns artist and album names into directories, and how long the
/// file names are. Prints character classes and counts only.
#[test]
fn real_stick_file_path_sanitising_survey() {
    let Some(db) = load() else {
        return;
    };
    let mut artist: BTreeMap<String, usize> = BTreeMap::new();
    let mut album: BTreeMap<String, usize> = BTreeMap::new();
    let mut file_lengths: BTreeMap<usize, usize> = BTreeMap::new();
    let mut long_files: BTreeMap<String, usize> = BTreeMap::new();
    for t in db.tracks() {
        let parts: Vec<&str> = t.file_path().split('/').skip(1).collect();
        let [_, a, b, f] = parts[..] else {
            continue;
        };
        if let Some(name) = db.artist_of(t).map(|r| r.name()).filter(|n| *n != a) {
            differences(a, name, &mut artist);
        }
        if let Some(name) = db.album_of(t).map(|r| r.name()).filter(|n| *n != b) {
            differences(b, name, &mut album);
        }
        let units = utf16_len(f);
        *file_lengths.entry(units / 8 * 8).or_default() += 1;
        if units >= 44 {
            let ext = f.rsplit_once('.').map(|(_, e)| e.len());
            let shape = f
                .chars()
                .rev()
                .take(6)
                .map(shown)
                .collect::<Vec<_>>()
                .join(", ");
            *long_files
                .entry(format!(
                    "{units} units, extension length {ext:?}, last chars reversed [{shape}]"
                ))
                .or_default() += 1;
        }
    }
    eprintln!("artist directory differences from the artist name:");
    for (d, n) in &artist {
        eprintln!("  {n:4} {d}");
    }
    eprintln!("album directory differences from the album name:");
    for (d, n) in &album {
        eprintln!("  {n:4} {d}");
    }
    eprintln!("file name lengths in UTF-16 units, by 8s: {file_lengths:?}");
    eprintln!("file names of 44 units or more:");
    for (d, n) in &long_files {
        eprintln!("  {n:4} {d}");
    }
    let longest = file_lengths.keys().max().copied().unwrap_or(0);
    assert!(longest <= 48, "no file name is longer than 48 units");
}

/// Surveys the shapes of every audio file path (slot 20). Prints counts only.
#[test]
fn real_stick_file_path_survey() {
    let Some(db) = load() else {
        return;
    };
    let mut components: BTreeMap<usize, usize> = BTreeMap::new();
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    let mut max_component: BTreeMap<usize, usize> = BTreeMap::new();
    let mut artist_dirs: BTreeMap<&str, usize> = BTreeMap::new();
    let mut album_dirs: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut contents, mut trailing, mut leading, mut filename_last) = (0, 0, 0, 0);
    let mut max_total = 0;
    for t in db.tracks() {
        let path = t.file_path();
        contents += usize::from(path.starts_with("/Contents/"));
        max_total = max_total.max(utf16_len(path));
        let parts: Vec<&str> = path.split('/').skip(1).collect();
        *components.entry(parts.len()).or_default() += 1;
        for (i, part) in parts.iter().enumerate() {
            let max = max_component.entry(i + 1).or_default();
            *max = (*max).max(utf16_len(part));
            trailing += usize::from(part.ends_with(['.', ' ']));
            leading += usize::from(part.starts_with(['.', ' ']));
        }
        let mut seen: BTreeMap<String, ()> = BTreeMap::new();
        for class in path.chars().filter_map(char_class) {
            seen.insert(class, ());
        }
        for class in seen.into_keys() {
            *classes.entry(class).or_default() += 1;
        }
        filename_last += usize::from(parts.last() == Some(&t.filename()));
        if let [_, artist, album, _] = parts[..] {
            let name = db.artist_of(t).map(|a| a.name());
            *artist_dirs.entry(relation(artist, name)).or_default() += 1;
            let name = db.album_of(t).map(|a| a.name());
            *album_dirs.entry(relation(album, name)).or_default() += 1;
        }
    }
    eprintln!("file paths (slot 20) of {} tracks:", db.tracks().len());
    eprintln!("  start with /Contents/: {contents}");
    eprintln!("  components: count {components:?}");
    eprintln!("  longest component by position, UTF-16 units: {max_component:?}");
    eprintln!("  longest path, UTF-16 units: {max_total}");
    eprintln!("  components ending in a dot or space: {trailing}; starting with one: {leading}");
    eprintln!("  filename equals the last component: {filename_last}");
    eprintln!("  paths with a character of each class:");
    for (class, count) in &classes {
        eprintln!("    {count:4} {class}");
    }
    eprintln!("  artist directory (4-component paths): {artist_dirs:?}");
    eprintln!("  album directory (4-component paths): {album_dirs:?}");
    assert_eq!(db.tracks().len(), TRACKS);
    assert_eq!(contents, TRACKS);
    assert_eq!(components, BTreeMap::from([(4, TRACKS)]));
    assert!(max_component.values().all(|&units| units <= 48));
    assert_eq!((trailing, leading), (0, 0));
    assert_eq!(filename_last, TRACKS);
}
