//! Surveys a fresh rekordbox export in `local-fixtures/rekordbox-hw1`: one playlist of 6
//! tracks, exported by rekordbox to an empty stick and played on real CDJs. Checks the track
//! row builder, `Database`, the `.DAT` builder and `BeatGrid::constant_tempo` against it, and
//! reports the shapes of the other files on the stick. Skips itself when the fixture is
//! missing. The page layout of its `export.pdb` is compared with the writer in
//! `src/pdb/writer/local_fresh.rs`.
//!
//! The export holds the user's library, so the tests print and pin counts and shapes only,
//! never titles, artists, paths or playlist names.

mod common;

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use cdj_media::anlz::{AnlzFile, Beat, BeatGrid, ConstantGrid, FourCc, Tag};
use cdj_media::encoding::{BeatInBar, DeviceSqlString, Millis};
use cdj_media::pdb::model::Artist;
use cdj_media::pdb::{
    Database, ExportRow, ExportTable, Pdb, TablePointer, Track, TrackFields, TrackString,
};

const ROOT: &str = "rekordbox-hw1/PIONEER";
const OLD_STICK: &str = "real-stick/PIONEER/rekordbox/export.pdb";

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

fn read_pdb(path: &Path) -> Pdb {
    let bytes = std::fs::read(path).expect("read");
    Pdb::from_bytes(&bytes).expect("decode")
}

/// The present track rows, in chain order.
fn tracks(pdb: &Pdb) -> Vec<Track> {
    pdb.table_pages(ExportTable::Tracks)
        .flat_map(|p| p.rows())
        .map(|raw| match ExportRow::decode(ExportTable::Tracks, &raw) {
            Ok(Some(ExportRow::Track(t))) => t,
            other => panic!("not a track row: {other:?}"),
        })
        .collect()
}

/// A track's domain values, as the builder takes them.
fn fields_of(t: &Track) -> TrackFields {
    let text = |s: &DeviceSqlString| s.as_str().to_owned();
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

/// The track string slots that are not understood.
const UNKNOWN_SLOTS: [TrackString; 7] = [
    TrackString::Unknown2,
    TrackString::Unknown3,
    TrackString::Unknown4,
    TrackString::Unknown8,
    TrackString::Unknown9,
    TrackString::Unknown13,
    TrackString::Unknown18,
];

/// The number of significant bits of `value`.
fn bits(value: u32) -> u32 {
    u32::BITS - value.leading_zeros()
}

/// Whether any gap byte before a string of `t` is non-zero.
fn has_gap_bytes(t: &Track) -> bool {
    let gaps = TrackString::ALL.iter().map(|&s| t.string_gap(s));
    gaps.flatten().any(|&b| b != 0)
}

/// Whether `a` and `b` have the same text and form in every named string slot.
fn same_named_strings(a: &Track, b: &Track) -> bool {
    TrackString::ALL
        .iter()
        .filter(|s| !UNKNOWN_SLOTS.contains(s))
        .all(|&s| a.string(s) == b.string(s))
}

/// The fresh export's track rows against ADR-008's defaults and the builder:
///
/// - `subtype`, `unknown3`, `unknown4`, `unknown5`, `unknown7` and `bitmask` are the
///   builder's defaults on all 6 rows;
/// - `unknown2` is not the track id, and it is the value the old stick has for the same audio
///   file (6 of 6): a stable per-track value of the rekordbox library, not random per export;
/// - the unknown string slots 2, 3 and 4 are numbers that differ per track; none equals the
///   builder's defaults ("2", "1", ""); slots 8, 9, 13 and 18 are empty;
/// - `Track::new` from the row's values gives the same fixed fields and named strings (text
///   and form) on every row, but no row is identical with `index_shift` and `unknown2`
///   masked, because of slots 2 to 4.
#[test]
fn fresh_track_rows_against_the_builder() {
    let Some(root) = common::local_fixture(ROOT) else {
        return;
    };
    let pdb = read_pdb(&root.join("rekordbox/export.pdb"));
    let tracks = tracks(&pdb);
    let defaults = Track::new(tracks[0].id(), TrackFields::default()).unwrap();
    let mut pins = Pins::default();
    let ids: Vec<u32> = tracks.iter().map(|t| t.id().get()).collect();
    pins.eq("track ids in page order", ids, vec![4, 1, 6, 2, 3, 5]);
    let constants = |t: &Track| {
        (
            t.subtype(),
            t.bitmask(),
            t.unknown3(),
            t.unknown4(),
            t.unknown5(),
            t.unknown7(),
        )
    };
    let with_defaults = tracks
        .iter()
        .filter(|t| constants(t) == constants(&defaults))
        .count();
    pins.eq(
        "rows with the builder's constants and bitmask",
        with_defaults,
        6,
    );
    let unknown2: Vec<(bool, u32)> = tracks
        .iter()
        .map(|t| (t.unknown2() == t.id().get(), bits(t.unknown2())))
        .collect();
    let pinned: Vec<(bool, u32)> = [28, 26, 28, 28, 28, 27].map(|b| (false, b)).into();
    pins.eq("unknown2 (equal to the id, bits)", unknown2, pinned);
    let strings: Vec<[&str; 3]> = tracks
        .iter()
        .map(|t| [0, 1, 2].map(|k| t.string(UNKNOWN_SLOTS[k]).as_str()))
        .collect();
    let pinned = vec![
        ["10", "2", "3"],
        ["19", "1", "19"],
        ["6", "1", ""],
        ["9", "1", "3"],
        ["13", "4", "5"],
        ["6", "4", ""],
    ];
    pins.eq("unknown string slots 2, 3 and 4", strings, pinned);
    let mut empty = 0;
    for t in &tracks {
        let texts = UNKNOWN_SLOTS[3..].iter().map(|&s| t.string(s).as_str());
        empty += usize::from(texts.collect::<String>().is_empty());
    }
    pins.eq("rows with slots 8, 9, 13 and 18 empty", empty, 6);
    let gaps = tracks.iter().filter(|t| has_gap_bytes(t)).count();
    pins.eq("rows with non-zero gap bytes", gaps, 0);
    let (mut fixed, mut named, mut identical) = (0, 0, 0);
    for t in &tracks {
        let rebuilt = Track::new(t.id(), fields_of(t)).unwrap();
        let (built, real) = (rebuilt.encode().unwrap(), t.encode().unwrap());
        let part = |b: &[u8]| [b[0x08..0x14].to_vec(), b[0x18..0x5e].to_vec()].concat();
        fixed += usize::from(part(&built) == part(&real));
        named += usize::from(same_named_strings(&rebuilt, t));
        let mask = |mut b: Vec<u8>| {
            b[2..4].fill(0);
            b[0x14..0x18].fill(0);
            b
        };
        identical += usize::from(mask(built) == mask(real));
    }
    pins.eq(
        "rebuilt rows: fixed fields equal, named strings equal, identical but index_shift and \
         unknown2",
        (fixed, named, identical),
        (6, 6, 0),
    );
    if let Some(old) = common::local_fixture(OLD_STICK) {
        let old = self::tracks(&read_pdb(&old));
        let mut same = Vec::new();
        for t in &tracks {
            let Some(o) = old.iter().find(|o| o.file_path() == t.file_path()) else {
                continue;
            };
            let unknown2 = o.unknown2() == t.unknown2();
            let path = o.analysis_path() == t.analysis_path();
            same.push((unknown2, path));
        }
        pins.eq(
            "on the old stick: (same unknown2, same analysis path) per track",
            same,
            vec![(true, true); 6],
        );
    }
    pins.check();
}

/// The fresh export through `Database`: it loads with no dangling reference, and rebuilding
/// it from scratch with find-or-insert in track id order (each track's artist, album artist,
/// album, genre, label, key and artwork, then the track) gives every record rekordbox's id,
/// so the rebuilt database equals the loaded one (ADR-016). Each artwork row belongs to one
/// track; one track has none.
#[test]
fn fresh_export_database_rebuilds_with_rekordbox_ids() {
    let Some(root) = common::local_fixture(ROOT) else {
        return;
    };
    let pdb = read_pdb(&root.join("rekordbox/export.pdb"));
    let db = Database::from_pdb(&pdb).unwrap();
    let mut rows = tracks(&pdb);
    rows.sort_by_key(Track::id);
    let mut pins = Pins::default();
    let counts = [
        db.tracks().len(),
        db.artists().len(),
        db.albums().len(),
        db.genres().len(),
        db.labels().len(),
        db.keys().len(),
        db.colors().len(),
        db.artworks().len(),
    ];
    let dangling = db.dangling_references().count();
    pins.eq(
        "tracks, artists, albums, genres, labels, keys, colours, artwork; dangling",
        (counts, dangling),
        ([6, 7, 4, 1, 0, 6, 8, 5], 0),
    );
    let mut new = Database::new();
    let mut moved = 0;
    for row in &rows {
        let t = db.track(row.id()).expect("a loaded track");
        let artist = |new: &mut Database, a: Option<&Artist>| {
            a.and_then(|a| new.find_or_insert_artist(a.name()))
        };
        let artist_id = artist(&mut new, db.artist_of(t));
        let composer_id = artist(&mut new, db.composer_of(t));
        let original_artist_id = artist(&mut new, db.original_artist_of(t));
        let remixer_id = artist(&mut new, db.remixer_of(t));
        let album = db.album_of(t);
        let album_artist = artist(&mut new, album.and_then(|a| db.album_artist_of(a)));
        let album_id = album.and_then(|a| new.find_or_insert_album(a.name(), album_artist));
        let genre = db.genre_of(t);
        let genre_id = genre.and_then(|g| new.find_or_insert_genre(g.name()));
        let label = db.label_of(t);
        let label_id = label.and_then(|l| new.find_or_insert_label(l.name()));
        let key = db.key_of(t);
        let key_id = key.and_then(|k| new.find_or_insert_key(k.name()));
        let artwork = db.artwork_of(t);
        let artwork_id = artwork.and_then(|a| new.find_or_insert_artwork(a.path()));
        let same = [
            artist_id == t.artist_id(),
            album_id == t.album_id(),
            genre_id == t.genre_id(),
            label_id == t.label_id(),
            key_id == t.key_id(),
            artwork_id == t.artwork_id(),
        ];
        moved += same.iter().filter(|&&same| !same).count();
        let fields = TrackFields {
            artist_id,
            composer_id,
            original_artist_id,
            remixer_id,
            album_id,
            genre_id,
            label_id,
            key_id,
            artwork_id,
            ..fields_of(row)
        };
        let id = new.add_track(fields).unwrap();
        moved += usize::from(id != t.id());
    }
    pins.eq("references or ids that differ from rekordbox's", moved, 0);
    // The playlist, through the public API in rekordbox's order: the playlist, then its
    // tracks in entry order.
    let playlists: Vec<_> = db.playlists().collect();
    let [playlist] = playlists[..] else {
        panic!("one playlist");
    };
    let id = new.add_playlist(playlist.name(), playlist.parent_id());
    pins.eq("the rebuilt playlist's id", id, Ok(playlist.id()));
    for track in db.playlist_tracks(playlist.id()) {
        new.add_to_playlist(playlist.id(), track);
    }
    pins.eq(
        "playlists, playlist tracks",
        (
            db.playlists().len(),
            db.playlist_tracks(playlist.id()).count(),
        ),
        (1, 6),
    );
    pins.eq(
        "the rebuilt database equals the loaded one",
        new == db,
        true,
    );
    let written = new.to_pdb().unwrap();
    for table in [ExportTable::PlaylistTree, ExportTable::PlaylistEntries] {
        pins.eq(
            &format!("{table:?}: our rows equal rekordbox's, byte for byte"),
            row_bytes(&written, table) == row_bytes(&pdb, table),
            true,
        );
    }
    let with_artwork = db.tracks().filter(|t| t.artwork_id().is_some()).count();
    pins.eq("tracks with artwork", with_artwork, 5);
    pins.check();
}

/// The bytes of every present row of `table`, in chain order, as long as its typed row.
fn row_bytes(pdb: &Pdb, table: ExportTable) -> Vec<Vec<u8>> {
    pdb.table_pages(table)
        .flat_map(|p| p.rows())
        .map(|raw| {
            let row = ExportRow::decode(table, &raw)
                .unwrap()
                .expect("a typed row");
            let len = row.encode().unwrap().len();
            raw.bytes()[..len].to_vec()
        })
        .collect()
}

/// The fresh export's database, written with `to_pdb`, encoded, decoded and loaded again,
/// equals the loaded one, playlists included, with no dangling reference.
#[test]
fn fresh_export_database_survives_to_pdb() {
    let Some(root) = common::local_fixture(ROOT) else {
        return;
    };
    let db = Database::from_pdb(&read_pdb(&root.join("rekordbox/export.pdb"))).unwrap();
    let bytes = db.to_pdb().unwrap().to_bytes().unwrap();
    let back = Database::from_pdb(&Pdb::from_bytes(&bytes).unwrap()).unwrap();
    let mut pins = Pins::default();
    pins.eq("written and loaded again, equal", back == db, true);
    pins.eq(
        "playlists, dangling references",
        (back.playlists().len(), back.dangling_references().count()),
        (1, 0),
    );
    pins.check();
}

/// Every ANLZ file under `dir`, sorted.
fn anlz_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("read directory") {
            let path = entry.expect("directory entry").path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if path.is_dir() {
                pending.push(path);
            } else if name.starts_with("ANLZ") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn extension(path: &Path) -> String {
    path.extension().unwrap().to_string_lossy().to_uppercase()
}

fn decode(path: &Path) -> (Vec<u8>, AnlzFile) {
    let bytes = std::fs::read(path).expect("read");
    let file = AnlzFile::from_bytes(&bytes).expect("decode");
    (bytes, file)
}

/// Whether a PVBR tag is what `track_dat` writes: a zero 4-byte header and a zero 1,604-byte
/// body.
fn is_zero_pvbr(tag: &Tag) -> bool {
    let Tag::Raw(raw) = tag else {
        return false;
    };
    let zero = raw.body().iter().all(|&b| b == 0);
    raw.header() == [0; 4] && raw.body().len() == 1604 && zero
}

/// The fresh export's analysis files: `AnlzFile::track_dat` rebuilds all 6 `.DAT` files byte
/// for byte; every file has the modal PMAI header (ADR-015) and every PVBR is the zero
/// header and zero 1,604-byte body `track_dat` writes. The tag sets of `.EXT` and `.2EX` are
/// reported; every `.2EX` here has PVDI (99 of 826 on the old stick).
#[test]
fn fresh_anlz_files() {
    let Some(root) = common::local_fixture(ROOT) else {
        return;
    };
    let mut pins = Pins::default();
    let files = anlz_files(&root.join("USBANLZ"));
    let mut headers: BTreeMap<(String, Vec<u8>), usize> = BTreeMap::new();
    let mut orders: BTreeMap<(String, String), usize> = BTreeMap::new();
    let (mut identical, mut pvbr_zero) = (0, 0);
    for path in &files {
        let (bytes, file) = decode(path);
        let ext = extension(path);
        *headers
            .entry((ext.clone(), file.unknown_header().to_vec()))
            .or_default() += 1;
        let order: Vec<String> = file.tags().iter().map(|t| t.fourcc().to_string()).collect();
        *orders.entry((ext.clone(), order.join(" "))).or_default() += 1;
        if ext != "DAT" {
            continue;
        }
        let audio_path = file
            .tags()
            .iter()
            .find_map(|t| match t {
                Tag::Path(p) => Some(p.clone()),
                _ => None,
            })
            .expect("PPTH");
        let built = AnlzFile::track_dat(
            audio_path,
            file.beat_grid().expect("PQTZ").clone(),
            file.hot_cues().expect("hot cues").clone(),
            file.memory_cues().expect("memory cues").clone(),
            file.waveform_preview().expect("PWAV").clone(),
            file.tiny_waveform_preview().expect("PWV2").clone(),
        )
        .expect("builds");
        identical += usize::from(built.to_bytes().unwrap() == bytes);
        pvbr_zero += usize::from(file.tags_with(FourCc::PVBR).all(is_zero_pvbr));
    }
    let modal = vec![0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    let header_kinds: Vec<(String, bool, usize)> = headers
        .iter()
        .map(|((ext, h), n)| (ext.clone(), *h == modal, *n))
        .collect();
    let pinned: Vec<(String, bool, usize)> = ["2EX", "DAT", "EXT"]
        .iter()
        .map(|ext| ((*ext).to_owned(), true, 6))
        .collect();
    pins.eq(
        "PMAI headers (extension, modal, files)",
        header_kinds,
        pinned,
    );
    pins.eq(
        ".DAT rebuilt identically, PVBR zero",
        (identical, pvbr_zero),
        (6, 6),
    );
    let orders: Vec<(String, String, usize)> = orders
        .into_iter()
        .map(|((ext, order), n)| (ext, order, n))
        .collect();
    let ext = "PPTH PWV3 PCOB PCOB PCO2 PCO2 PQT2 PWV5 PWV4";
    let pinned: Vec<(String, String, usize)> = [
        ("2EX", "PPTH PWV7 PWV6 PWVC PVDI".to_owned(), 6),
        ("DAT", "PPTH PVBR PQTZ PWAV PWV2 PCOB PCOB".to_owned(), 6),
        ("EXT", ext.to_owned(), 4),
        ("EXT", format!("{ext} PSSI"), 2),
    ]
    .into_iter()
    .map(|(e, order, n)| (e.to_owned(), order, n))
    .collect();
    pins.eq("tag orders (extension, order, files)", orders, pinned);
    pins.check();
}

/// How `BeatGrid::constant_tempo` rebuilds a grid from its first downbeat, its tempo and the
/// PWV3 length: (equal, beat counts equal, beats equal, beats, largest deviation in ms).
type GridRebuild = (bool, bool, usize, usize, u32);

fn rebuild_grid(beats: &[Beat], end: Millis) -> GridRebuild {
    let anchor = beats
        .iter()
        .find(|b| b.bar_position() == BeatInBar::One)
        .expect("a downbeat");
    let grid = ConstantGrid::new(anchor.time(), anchor.tempo(), end);
    let built = BeatGrid::constant_tempo(&grid.expect("a tempo"));
    let built = built.beats();
    let deviation = beats
        .iter()
        .zip(built)
        .map(|(a, b)| a.time().get().abs_diff(b.time().get()))
        .max()
        .unwrap_or(0);
    let equal = beats.iter().zip(built).filter(|(a, b)| a == b).count();
    (
        built == beats,
        built.len() == beats.len(),
        equal,
        beats.len(),
        deviation,
    )
}

/// The fresh export's beat grids, rebuilt with `BeatGrid::constant_tempo` from each grid's
/// first downbeat, its tempo and the `.EXT` PWV3 length (ADR-014). Every grid has one tempo;
/// every rebuild has the right beat count, 2 of 6 are exact, and no beat is more than 1 ms
/// off. Of 4,427 beats, 3,622 are equal.
#[test]
fn fresh_beat_grids_rebuild() {
    let Some(root) = common::local_fixture(ROOT) else {
        return;
    };
    let mut pins = Pins::default();
    let mut grids = Vec::new();
    for path in anlz_files(&root.join("USBANLZ")) {
        if extension(&path) != "DAT" {
            continue;
        }
        let (_, dat) = decode(&path);
        let (_, ext) = decode(&path.with_extension("EXT"));
        let entries = ext.waveform_detail().expect("PWV3").data().len();
        let end = Millis::new(u32::try_from(entries).unwrap() * 20 / 3);
        let beats = dat.beat_grid().expect("PQTZ").beats();
        let constant = beats.windows(2).all(|w| w[0].tempo() == w[1].tempo());
        let first = beats[0].bar_position().number();
        let rebuilt = rebuild_grid(beats, end);
        grids.push((constant, first, rebuilt));
    }
    let pinned = vec![
        (true, 4, (false, true, 591, 826, 1)),
        (true, 2, (false, true, 509, 880, 1)),
        (true, 2, (false, true, 618, 776, 1)),
        (true, 1, (false, true, 608, 649, 1)),
        (true, 1, (true, true, 384, 384, 0)),
        (true, 2, (true, true, 912, 912, 0)),
    ];
    pins.eq(
        "grids (one tempo, first beat number, (equal, count equal, beats equal, beats, max ms \
         off))",
        grids,
        pinned,
    );
    pins.check();
}

/// A big-endian `u32` at `at`.
fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The four-letter code at `at`.
fn code(bytes: &[u8], at: usize) -> String {
    String::from_utf8_lossy(&bytes[at..at + 4]).into_owned()
}

/// The top level of an ANLZ-like container: its code and its first section's, then the file
/// size, the header length, the length field, and the first section's header length and
/// length.
fn container(bytes: &[u8]) -> (String, [u32; 5]) {
    let header = usize::try_from(be32(bytes, 4)).unwrap();
    let codes = format!("{} {}", code(bytes, 0), code(bytes, header));
    let size = u32::try_from(bytes.len()).unwrap();
    let (header_len, length) = (be32(bytes, 4), be32(bytes, 8));
    let section = (be32(bytes, header + 4), be32(bytes, header + 8));
    (codes, [size, header_len, length, section.0, section.1])
}

/// The shape of an `exportExt.pdb` table: type, `empty_candidate`, first page, last page,
/// pages and present rows.
fn ext_table(pdb: &Pdb, t: &TablePointer) -> [u32; 6] {
    let pages: Vec<_> = pdb.pages_of(t).collect();
    let rows: usize = pages.iter().map(|p| p.rows().count()).sum();
    let count = |n: usize| u32::try_from(n).unwrap();
    let (pages, rows) = (count(pages.len()), count(rows));
    let (first, last) = (t.first_page(), t.last_page());
    [t.raw_type(), t.empty_candidate(), first, last, pages, rows]
}

/// The setting files at the top of `PIONEER/`.
const SETTINGS: [&str; 3] = ["DJMMYSETTING.DAT", "MYSETTING.DAT", "MYSETTING2.DAT"];

/// The other files of a fresh export, by size and top-level shape, for deciding what a stick
/// image needs:
///
/// - `USBANLZ/USBMNG.DAT` (400,058 bytes, `PMNG` with one `PTBL` section) and
///   `rekordbox/RBFLTR.DAT` (232 bytes, `FMAI` starting with an `FCND` section) are ANLZ-like
///   containers: a four-letter code, a big-endian header length and the file length, then
///   tagged sections;
/// - `MYSETTING.DAT`, `MYSETTING2.DAT` and `DJMMYSETTING.DAT` start with a `u32` string-block
///   length of 0x60, as djl-analysis documents;
/// - `djprofile.nxs` is 160 bytes;
/// - `exportLibrary.db` is whole 4,096-byte pages but not a plain SQLite file;
/// - `exportExt.pdb` has 9 tables of types 0 to 8 and 18 pages.
#[test]
fn fresh_other_files() {
    let Some(root) = common::local_fixture(ROOT) else {
        return;
    };
    let mut pins = Pins::default();
    let read = |rel: &str| std::fs::read(root.join(rel)).expect("read");
    pins.eq(
        "USBMNG.DAT (codes, [size, header, length, section header, section length])",
        container(&read("USBANLZ/USBMNG.DAT")),
        ("PMNG PTBL".into(), [400_058, 32, 400_058, 20, 400_026]),
    );
    pins.eq(
        "RBFLTR.DAT (codes, [size, header, length, section header, section length])",
        container(&read("rekordbox/RBFLTR.DAT")),
        ("FMAI FCND".into(), [232, 116, 232, 20, 28]),
    );
    let mut settings = Vec::new();
    for rel in SETTINGS {
        let b = read(rel);
        settings.push((b.len(), u32::from_le_bytes([b[0], b[1], b[2], b[3]])));
    }
    pins.eq(
        "setting files (size, u32 at 0)",
        settings,
        vec![(160, 0x60), (148, 0x60), (148, 0x60)],
    );
    pins.eq("djprofile.nxs size", read("djprofile.nxs").len(), 160);
    let library = read("rekordbox/exportLibrary.db");
    let pages = library.len() % 4096 == 0;
    let sqlite = library.starts_with(b"SQLite format 3\0");
    pins.eq(
        "exportLibrary.db (size, whole 4,096-byte pages, SQLite magic)",
        (library.len(), pages, sqlite),
        (118_784, true, false),
    );
    let ext = read_pdb(&root.join("rekordbox/exportExt.pdb"));
    let header = (ext.next_unused_page(), ext.unknown2(), ext.sequence());
    pins.eq(
        "exportExt.pdb pages, (next_unused_page, unknown2, sequence)",
        (ext.page_count(), header),
        (18, (22, 5, 7)),
    );
    let tables: Vec<[u32; 6]> = ext.tables().iter().map(|t| ext_table(&ext, t)).collect();
    let pinned = vec![
        [0, 2, 1, 1, 1, 0],
        [1, 4, 3, 3, 1, 0],
        [2, 6, 5, 5, 1, 0],
        [3, 20, 7, 8, 2, 24],
        [4, 21, 9, 10, 2, 1],
        [5, 12, 11, 11, 1, 0],
        [6, 14, 13, 13, 1, 0],
        [7, 19, 15, 16, 2, 1],
        [8, 18, 17, 17, 1, 0],
    ];
    pins.eq(
        "exportExt.pdb tables (type, empty_candidate, first, last, pages, present rows)",
        tables,
        pinned,
    );
    pins.check();
}
