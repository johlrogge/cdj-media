//! ADR-018's acceptance test: `ExportBuilder` rebuilds the fresh rekordbox export in
//! `local-fixtures/rekordbox-hw1` (one playlist of 6 tracks) from its loaded `Database` and
//! its `.DAT` files. Skips itself when the fixture is missing.
//!
//! Each track, in track id order, gets its row's tags and audio values, its `.DAT`'s beat
//! grid (`Grid::Beats`), previews (`Waveform::Previews`) and cues, and artwork when its row
//! has some; then the playlist is added in entry order. The fixture holds no artwork images,
//! so a stand-in JPEG is used, which only the artwork rows see.
//!
//! The export holds the user's library, so the test prints counts and field names only,
//! never titles, artists, paths or playlist names.

mod common;

use std::collections::BTreeMap;

use cdj_media::anlz::{AnlzFile, CueSpan, HotCueSlot};
use cdj_media::encoding::Millis;
use cdj_media::export::{AudioFormat, ExportBuilder, ExportDate, Grid, NewTrack, Waveform};
use cdj_media::layout::{AnlzPath, export_pdb_path};
use cdj_media::pdb::model::Track;
use cdj_media::pdb::{Database, FileType, Pdb, TrackFields};

const ROOT: &str = "rekordbox-hw1";

/// A stand-in artwork image: the JPEG start-of-image marker, then filler.
const JPEG: [u8; 4] = [0xff, 0xd8, 0xff, 0xd9];

fn load(bytes: &[u8]) -> Database {
    Database::from_pdb(&Pdb::from_bytes(bytes).expect("decode")).expect("load")
}

/// A `YYYY-MM-DD` date as stored.
fn date(text: Option<&str>) -> Option<ExportDate> {
    let mut parts = text?.split('-');
    let mut next = || parts.next()?.parse::<u16>().ok();
    let (year, month, day) = (next()?, next()?, next()?);
    ExportDate::new(year, u8::try_from(month).ok()?, u8::try_from(day).ok()?)
}

/// The track's values as the builder takes them, with its `.DAT`'s analysis.
fn new_track(db: &Database, t: &Track, dat: &AnlzFile) -> NewTrack {
    let name = |a: Option<&cdj_media::pdb::model::Artist>| a.map_or("", |a| a.name()).to_owned();
    let album = db.album_of(t);
    let format = match t.file_type() {
        FileType::Aiff => AudioFormat::Aiff,
        FileType::Wav => AudioFormat::Wav,
        other => panic!("a track of file type {other:?}, which the builder can't write"),
    };
    let hot_cues: Vec<(HotCueSlot, CueSpan)> = dat
        .hot_cues()
        .expect("hot cues")
        .cues()
        .iter()
        .map(|c| (c.hot_cue().expect("a hot cue slot"), c.span()))
        .collect();
    let memory_cues = dat
        .memory_cues()
        .expect("memory cues")
        .cues()
        .iter()
        .map(|c| c.span())
        .collect();
    NewTrack {
        title: t.title().to_owned(),
        artist: name(db.artist_of(t)),
        album: album.map_or("", |a| a.name()).to_owned(),
        album_artist: name(album.and_then(|a| db.album_artist_of(a))),
        composer: name(db.composer_of(t)),
        remixer: name(db.remixer_of(t)),
        original_artist: name(db.original_artist_of(t)),
        genre: db.genre_of(t).map_or("", |g| g.name()).to_owned(),
        label: db.label_of(t).map_or("", |l| l.name()).to_owned(),
        key: db.key_of(t).map_or("", |k| k.name()).to_owned(),
        comment: t.comment().to_owned(),
        mix_name: t.mix_name().to_owned(),
        isrc: t.isrc().unwrap_or_default().to_owned(),
        year: t.year(),
        track_number: t.track_number(),
        disc_number: t.disc_number(),
        rating: t.rating(),
        color: t.color_id(),
        file_name: t.filename().to_owned(),
        format,
        file_size: t.file_size(),
        duration: Millis::new(u32::from(t.duration()) * 1000),
        sample_rate: t.sample_rate(),
        sample_depth: t.sample_depth(),
        bitrate: t.bitrate(),
        grid: Grid::Beats(dat.beat_grid().expect("PQTZ").clone()),
        hot_cues,
        memory_cues,
        waveform: Waveform::Previews(
            dat.waveform_preview().expect("PWAV").clone(),
            dat.tiny_waveform_preview().expect("PWV2").clone(),
        ),
        release_date: t.release_date().unwrap_or_default().to_owned(),
        lyricist: t.lyricist().to_owned(),
        play_count: t.play_count(),
        artwork: t.artwork_id().map(|_| JPEG.to_vec()),
        date_added: date(t.date_added()),
        analysis_date: date(t.analysis_date()),
    }
}

/// Counts, by field name, the tracks of `a` whose field differs from the same track's in `b`.
macro_rules! differing_fields {
    ($a:expr, $b:expr, $out:expr; $($field:ident),* $(,)?) => {
        $(
            if $a.$field != $b.$field {
                *$out.entry(stringify!($field)).or_insert(0usize) += 1;
            }
        )*
        // Exhaustive: a field of `TrackFields` missing from the list doesn't compile.
        let TrackFields { $($field: _),* } = &$a;
    };
}

/// The fields of the tracks that differ between `rebuilt` and `fixture`, by name, with the
/// number of tracks; a track one of them lacks counts as `missing track`. Each track is
/// compared whole, as the fields of its row, once the documented difference, the analysis
/// path, is counted and taken out; the names only say where a difference is.
fn track_differences(rebuilt: &Database, fixture: &Database) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for f in fixture.tracks() {
        let Some(r) = rebuilt.track(f.id()) else {
            *out.entry("missing track").or_insert(0) += 1;
            continue;
        };
        let (r, mut f) = (r.to_fields(), f.to_fields());
        if r.analysis_path != f.analysis_path {
            *out.entry("analysis_path").or_insert(0) += 1;
            f.analysis_path.clone_from(&r.analysis_path);
        }
        if r != f {
            differing_fields!(r, f, out;
                sample_rate, composer_id, file_size, artwork_id, key_id, original_artist_id,
                label_id, remixer_id, bitrate, track_number, tempo, genre_id, album_id,
                artist_id, disc_number, play_count, year, sample_depth, duration, color_id,
                rating, file_type, isrc, lyricist, message, publish_track_info,
                autoload_hot_cues, date_added, release_date, mix_name, analysis_path,
                analysis_date, comment, title, filename, file_path,
            );
        }
    }
    let extra = rebuilt
        .tracks()
        .len()
        .saturating_sub(fixture.tracks().len());
    if extra > 0 {
        out.insert("extra track", extra);
    }
    out
}

/// The tables other than tracks that differ, by name.
fn table_differences(rebuilt: &Database, fixture: &Database) -> Vec<&'static str> {
    let mut out = Vec::new();
    let mut check = |name, same: bool| {
        if !same {
            out.push(name);
        }
    };
    check("artists", rebuilt.artists().eq(fixture.artists()));
    check("albums", rebuilt.albums().eq(fixture.albums()));
    check("genres", rebuilt.genres().eq(fixture.genres()));
    check("labels", rebuilt.labels().eq(fixture.labels()));
    check("keys", rebuilt.keys().eq(fixture.keys()));
    check("colors", rebuilt.colors().eq(fixture.colors()));
    check("artworks", rebuilt.artworks().eq(fixture.artworks()));
    check("playlists", rebuilt.playlists().eq(fixture.playlists()));
    let entries = fixture.playlists().all(|p| {
        rebuilt
            .playlist_tracks(p.id())
            .eq(fixture.playlist_tracks(p.id()))
    });
    check("playlist tracks", entries);
    out
}

/// ADR-018's acceptance test:
/// - the builder gives every track rekordbox's id;
/// - the 6 `.DAT` files come back byte for byte, at the builder's paths;
/// - the rebuilt `export.pdb` loads to a `Database` equal to the fixture's except for the
///   analysis path (slot 14), derived from the id instead of hashed. The duration, the
///   other documented difference, can't differ: each track is given its row's whole
///   seconds.
#[test]
fn export_builder_rebuilds_the_fresh_export() {
    let Some(root) = common::local_fixture(ROOT) else {
        return;
    };
    let pdb = std::fs::read(root.join("PIONEER/rekordbox/export.pdb")).expect("read");
    let fixture = load(&pdb);
    let mut builder = ExportBuilder::new(ExportDate::new(2000, 1, 1).unwrap());
    let mut dats = Vec::new();
    let mut moved_ids = 0;
    for t in fixture.tracks() {
        let path = t.analysis_path().trim_start_matches('/');
        let bytes = std::fs::read(root.join(path)).expect("read a .DAT");
        let dat = AnlzFile::from_bytes(&bytes).expect("decode a .DAT");
        let id = builder
            .add_track(new_track(&fixture, t, &dat))
            .expect("the builder takes the track");
        moved_ids += usize::from(id != t.id());
        dats.push((id, bytes));
    }
    for playlist in fixture.playlists() {
        let name = playlist.name();
        let id = if playlist.is_folder() {
            builder.add_folder(name, playlist.parent_id())
        } else {
            builder.add_playlist(name, playlist.parent_id())
        };
        assert_eq!(id, Ok(playlist.id()), "the playlist gets rekordbox's id");
        for track in fixture.playlist_tracks(playlist.id()) {
            builder
                .add_to_playlist(playlist.id(), track)
                .expect("the playlist takes the track");
        }
    }
    let export = builder.build().expect("builds");

    let identical = dats
        .iter()
        .filter(|(id, bytes)| {
            export.file(&AnlzPath::for_track(*id).dat()) == Some(bytes.as_slice())
        })
        .count();
    let rebuilt = load(export.file(&export_pdb_path()).expect("export.pdb"));
    let tracks = track_differences(&rebuilt, &fixture);
    let tables = table_differences(&rebuilt, &fixture);
    eprintln!(
        "rebuilt {} tracks ({moved_ids} with another id), {} playlists; {identical} .DAT \
         files identical; {} files in the export",
        fixture.tracks().len(),
        fixture.playlists().len(),
        export.files().count(),
    );
    eprintln!("track fields that differ (field: tracks): {tracks:?}");
    eprintln!("other tables that differ: {tables:?}");

    assert_eq!(moved_ids, 0, "track ids");
    assert_eq!((dats.len(), identical), (6, 6), ".DAT files identical");
    let documented = BTreeMap::from([("analysis_path", 6)]);
    assert_eq!(tracks, documented, "only ADR-018's documented differences");
    assert!(tables.is_empty(), "tables that differ: {tables:?}");
    // 1 export.pdb, 6 .DAT files, 5 artwork images.
    assert_eq!(export.files().count(), 1 + 6 + 5);
}
