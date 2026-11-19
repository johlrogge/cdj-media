use super::*;
use crate::anlz::{AnlzFile, Beat, ConstantGrid, CueList};
use crate::encoding::BeatInBar;
use crate::layout::{AnlzPath, ArtworkPath, audio_path_tag};
use crate::pdb::{ArtworkId, Pdb, PlaylistId};

/// Where rekordbox puts the database.
const EXPORT_PDB: &str = "/PIONEER/rekordbox/export.pdb";

fn date() -> ExportDate {
    ExportDate::new(2026, 11, 19).unwrap()
}

/// The file at `path` of `export`.
fn file<'a>(export: &'a Export, path: &str) -> Option<&'a [u8]> {
    export
        .files()
        .find(|(p, _)| p.as_str() == path)
        .map(|(_, bytes)| bytes)
}

/// The export's `export.pdb`, loaded.
fn database(export: &Export) -> Database {
    let bytes = file(export, EXPORT_PDB).expect("an export.pdb");
    Database::from_pdb(&Pdb::from_bytes(bytes).expect("decodes")).expect("loads")
}

#[test]
fn an_export_date_is_a_calendar_date_written_yyyy_mm_dd() {
    let date = |y, m, d| ExportDate::new(y, m, d).map(|d| d.to_string());
    assert_eq!(date(2026, 1, 5).as_deref(), Some("2026-01-05"));
    assert_eq!(date(0, 12, 31).as_deref(), Some("0000-12-31"));
    assert_eq!(date(2024, 2, 29).as_deref(), Some("2024-02-29"));
    assert_eq!(date(2000, 2, 29).as_deref(), Some("2000-02-29"));
    for (y, m, d) in [
        (2023, 2, 29),
        (1900, 2, 29),
        (2026, 4, 31),
        (2026, 13, 1),
        (2026, 0, 1),
        (2026, 1, 0),
        (2026, 1, 32),
        (10_000, 1, 1),
    ] {
        assert_eq!(date(y, m, d), None, "{y}-{m}-{d}");
    }
}

#[test]
fn an_empty_builder_gives_an_export_pdb_of_an_empty_database() {
    let export = ExportBuilder::new(date()).build().unwrap();
    let paths: Vec<&str> = export.files().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, [EXPORT_PDB]);
    assert_eq!(database(&export), Database::new());
    assert_eq!(export.audio_destinations().count(), 0);
}

/// A track with only what a file needs.
fn track(artist: &str, album: &str, file_name: &str) -> NewTrack {
    NewTrack {
        title: "Title".into(),
        artist: artist.into(),
        album: album.into(),
        file_name: file_name.into(),
        file_size: 12_345_678,
        duration: Millis::new(201_999),
        sample_rate: 44_100,
        sample_depth: 16,
        bitrate: 1_411,
        ..NewTrack::default()
    }
}

/// The `.DAT` of track `id` in `export`, decoded.
fn dat(export: &Export, id: TrackId) -> AnlzFile {
    let path = AnlzPath::for_track(id).dat();
    let bytes = file(export, path.as_str()).expect("a .DAT");
    AnlzFile::from_bytes(bytes).expect("decodes")
}

#[test]
fn a_track_gets_a_row_a_dat_and_an_audio_destination_that_agree() {
    let mut builder = ExportBuilder::new(date());
    let id = builder
        .add_track(track("Björk", "Ålbum", "01 Ü.aiff"))
        .unwrap();
    let export = builder.build().unwrap();

    let content = ContentPath::for_track("Björk", "Ålbum", "01 Ü.aiff");
    let destinations: Vec<_> = export.audio_destinations().collect();
    assert_eq!(destinations, [(id, &content)]);
    let dat_path = AnlzPath::for_track(id).dat();
    let paths: Vec<&str> = export.files().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, [dat_path.as_str(), EXPORT_PDB]);

    let db = database(&export);
    let row = db.track(id).expect("the track row");
    assert_eq!(row.file_path(), content.as_stick_path().as_str());
    assert_eq!(row.filename(), "01 Ü.aiff");
    assert_eq!(row.analysis_path(), dat_path.as_str());
    assert_eq!(row.title(), "Title");
    assert_eq!(db.artist_of(row).map(|a| a.name()), Some("Björk"));
    assert_eq!(db.album_of(row).map(|a| a.name()), Some("Ålbum"));
    assert_eq!(row.tempo(), Bpm::from_centi(0));
    assert_eq!(row.duration(), 201);
    assert_eq!(row.file_type(), FileType::Aiff);
    assert_eq!(
        (row.file_size(), row.sample_rate(), row.sample_depth()),
        (12_345_678, 44_100, 16)
    );
    assert_eq!(row.bitrate(), 1_411);
    assert_eq!(row.date_added(), Some("2026-11-19"));
    assert_eq!(row.analysis_date(), Some("2026-11-19"));

    // Grid::None and Waveform::Flat: an empty grid, no cues, the flat previews.
    let expected = AnlzFile::track_dat(
        audio_path_tag(&content),
        BeatGrid::new(vec![]),
        CueList::hot([]),
        CueList::memory([]),
        WaveformPreview::flat(2),
        TinyWaveformPreview::flat(1),
    )
    .unwrap();
    assert_eq!(dat(&export, id), expected);
    assert_eq!(dat(&export, id).path(), Some(row.file_path()));
}

/// Every tag reaches the row; names go through find-or-insert exactly as given, so one
/// name used twice is one record, and the track's own dates win over the builder's.
#[test]
fn tags_and_own_dates_reach_the_row() {
    let mut builder = ExportBuilder::new(date());
    let id = builder
        .add_track(NewTrack {
            album_artist: "Album Artist".into(),
            composer: "Composer".into(),
            remixer: "Remixer".into(),
            original_artist: "Björk".into(),
            genre: "Genre".into(),
            label: "Label".into(),
            key: "8A".into(),
            comment: "Comment".into(),
            mix_name: "Mix".into(),
            isrc: "GBAYE0601498".into(),
            year: 1997,
            track_number: 7,
            disc_number: 2,
            rating: 4,
            color: ColorId::new(3),
            date_added: ExportDate::new(2024, 1, 2),
            analysis_date: ExportDate::new(2024, 1, 3),
            release_date: "1997-09-22".into(),
            lyricist: "Lyricist".into(),
            play_count: 12,
            ..track("Björk", "Ålbum", "01.aiff")
        })
        .unwrap();
    let db = database(&builder.build().unwrap());
    let row = db.track(id).unwrap();
    let name = |a: Option<&crate::pdb::model::Artist>| a.map(|a| a.name().to_owned());
    assert_eq!(name(db.artist_of(row)).as_deref(), Some("Björk"));
    assert_eq!(name(db.original_artist_of(row)).as_deref(), Some("Björk"));
    assert_eq!(row.artist_id(), row.original_artist_id());
    assert_eq!(name(db.composer_of(row)).as_deref(), Some("Composer"));
    assert_eq!(name(db.remixer_of(row)).as_deref(), Some("Remixer"));
    let album = db.album_of(row).unwrap();
    assert_eq!(album.name(), "Ålbum");
    assert_eq!(
        name(db.album_artist_of(album)).as_deref(),
        Some("Album Artist")
    );
    assert_eq!(db.genre_of(row).map(|g| g.name()), Some("Genre"));
    assert_eq!(db.label_of(row).map(|l| l.name()), Some("Label"));
    assert_eq!(db.key_of(row).map(|k| k.name()), Some("8A"));
    assert_eq!((row.comment(), row.mix_name()), ("Comment", "Mix"));
    assert_eq!(row.isrc(), Some("GBAYE0601498"));
    assert_eq!(
        (
            row.year(),
            row.track_number(),
            row.disc_number(),
            row.rating()
        ),
        (1997, 7, 2, 4)
    );
    assert_eq!(row.color_id(), ColorId::new(3));
    assert_eq!(row.date_added(), Some("2024-01-02"));
    assert_eq!(row.analysis_date(), Some("2024-01-03"));
    assert_eq!(row.release_date(), Some("1997-09-22"));
    assert_eq!(row.lyricist(), "Lyricist");
    assert_eq!(row.play_count(), 12);
    assert_eq!((db.artists().len(), db.albums().len()), (4, 1));
    assert_eq!(db.dangling_references().count(), 0);
}

#[test]
fn a_constant_grid_is_expanded_over_the_exact_duration() {
    let mut builder = ExportBuilder::new(date());
    let id = builder
        .add_track(NewTrack {
            duration: Millis::new(1_999),
            grid: Grid::Constant {
                first_beat: Millis::new(100),
                tempo: Bpm::from_centi(12_800),
            },
            ..track("A", "B", "c.aiff")
        })
        .unwrap();
    let export = builder.build().unwrap();
    let grid = ConstantGrid::new(
        Millis::new(100),
        Bpm::from_centi(12_800),
        Millis::new(1_999),
    )
    .unwrap();
    let expected = BeatGrid::constant_tempo(&grid);
    // Not padded nor cut to whole seconds: the beat at 1,975 ms is in.
    assert_eq!(expected.beats().len(), 5);
    assert_eq!(dat(&export, id).beat_grid(), Some(&expected));
    let db = database(&export);
    let row = db.track(id).unwrap();
    assert_eq!((row.tempo(), row.duration()), (Bpm::from_centi(12_800), 1));
}

#[test]
fn given_beats_and_previews_are_used_as_given() {
    let beat = |n, ms| Beat::new(n, Bpm::from_centi(12_050), Millis::new(ms));
    let grid = BeatGrid::new(vec![beat(BeatInBar::Three, 10), beat(BeatInBar::Four, 508)]);
    let preview = WaveformPreview::new(vec![0x21, 0x42, 0x63]);
    let tiny = TinyWaveformPreview::new(vec![7, 3]);
    let mut builder = ExportBuilder::new(date());
    let id = builder
        .add_track(NewTrack {
            grid: Grid::Beats(grid.clone()),
            waveform: Waveform::Previews(preview.clone(), tiny.clone()),
            format: AudioFormat::Wav,
            ..track("A", "B", "c.wav")
        })
        .unwrap();
    let export = builder.build().unwrap();
    let file = dat(&export, id);
    assert_eq!(file.beat_grid(), Some(&grid));
    assert_eq!(file.waveform_preview(), Some(&preview));
    assert_eq!(file.tiny_waveform_preview(), Some(&tiny));
    let db = database(&export);
    let row = db.track(id).unwrap();
    assert_eq!(row.tempo(), Bpm::from_centi(12_050));
    assert_eq!(row.file_type(), FileType::Wav);
}

fn point(ms: u32) -> CueSpan {
    CueSpan::Point(Millis::new(ms))
}

fn looped(start: u32, end: u32) -> CueSpan {
    CueSpan::Loop {
        start: Millis::new(start),
        end: Millis::new(end),
    }
}

/// Cues keep the caller's order, unsorted, through `CueList::hot` and `CueList::memory`.
#[test]
fn cues_keep_the_callers_order() {
    let hot = vec![
        (HotCueSlot::C, point(300)),
        (HotCueSlot::A, looped(100, 200)),
    ];
    let memory = vec![point(900), looped(50, 60), point(100)];
    let mut builder = ExportBuilder::new(date());
    let id = builder
        .add_track(NewTrack {
            hot_cues: hot.clone(),
            memory_cues: memory.clone(),
            ..track("A", "B", "c.aiff")
        })
        .unwrap();
    let file = dat(&builder.build().unwrap(), id);
    assert_eq!(file.hot_cues(), Some(&CueList::hot(hot)));
    assert_eq!(file.memory_cues(), Some(&CueList::memory(memory)));
}

/// `bad` is refused with `ValueOutOfRange { context }`, and the builder is unchanged: it
/// builds the same export, and the next track gets the id, names and path it would have.
fn assert_refused(bad: NewTrack, context: &'static str) {
    let mut builder = ExportBuilder::new(date());
    builder.add_track(track("A", "B", "1.aiff")).unwrap();
    let before = builder.clone();
    assert_eq!(
        builder.add_track(bad),
        Err(EncodeError::ValueOutOfRange { context }),
        "{context}"
    );
    // Not `assert_eq!`: the exports' bytes would flood the output.
    let unchanged = builder.clone().build().unwrap() == before.clone().build().unwrap();
    assert!(unchanged, "{context}: the export changed");
    let next = || track("New", "B", "2.aiff");
    let mut expected = before;
    let expected_id = expected.add_track(next()).unwrap();
    assert_eq!(builder.add_track(next()), Ok(expected_id), "{context}");
    let (built, expected) = (builder.build().unwrap(), expected.build().unwrap());
    let paths = |e: &Export| -> Vec<String> {
        e.audio_destinations()
            .map(|(_, p)| p.as_stick_path().to_string())
            .collect()
    };
    assert_eq!(paths(&built), paths(&expected), "{context}");
    assert!(
        built == expected,
        "{context}: the next track's export differs"
    );
}

/// A track that `assert_refused` would spot if it were half added: a new artist, and the
/// path the next track takes.
fn bad() -> NewTrack {
    track("New", "B", "2.aiff")
}

#[test]
fn a_duplicate_hot_cue_slot_is_refused() {
    let hot_cues = vec![(HotCueSlot::B, point(1)), (HotCueSlot::B, point(2))];
    assert_refused(NewTrack { hot_cues, ..bad() }, "hot cue slot");
    // Only A to C are hot cues of Phase 1; `Other(1)` would alias A.
    for other in [0, 1, 4] {
        let hot_cues = vec![(HotCueSlot::Other(other), point(1))];
        assert_refused(NewTrack { hot_cues, ..bad() }, "hot cue slot");
    }
}

#[test]
fn a_loop_that_does_not_end_after_its_start_is_refused() {
    for span in [looped(500, 500), looped(500, 499)] {
        let hot_cues = vec![(HotCueSlot::A, span)];
        assert_refused(NewTrack { hot_cues, ..bad() }, "cue loop end");
        let memory_cues = vec![point(1), span];
        assert_refused(
            NewTrack {
                memory_cues,
                ..bad()
            },
            "cue loop end",
        );
    }
}

#[test]
fn a_file_name_without_an_extension_is_refused() {
    for file_name in ["track", "", ".aiff", "track."] {
        let file_name = file_name.to_owned();
        assert_refused(NewTrack { file_name, ..bad() }, "audio file name");
    }
}

#[test]
fn a_constant_grid_of_tempo_zero_is_refused() {
    let grid = Grid::Constant {
        first_beat: Millis::new(0),
        tempo: Bpm::from_centi(0),
    };
    assert_refused(NewTrack { grid, ..bad() }, "grid tempo");
}

#[test]
fn a_duration_past_the_rows_seconds_is_refused() {
    let duration = Millis::new(65_536_000);
    assert_refused(NewTrack { duration, ..bad() }, "track duration");
    let fits = Millis::new(65_535_999);
    let mut builder = ExportBuilder::new(date());
    let id = builder
        .add_track(NewTrack {
            duration: fits,
            ..bad()
        })
        .unwrap();
    let db = database(&builder.build().unwrap());
    assert_eq!(db.track(id).unwrap().duration(), u16::MAX);
}

/// The row's and the `.DAT`'s own errors, after the names and the path are known, still
/// leave the builder unchanged.
#[test]
fn row_and_dat_errors_leave_the_builder_unchanged() {
    let isrc = "Björk".to_owned();
    assert_refused(NewTrack { isrc, ..bad() }, "DeviceSQL ISRC text");
    let color = ColorId::new(256);
    assert_refused(NewTrack { color, ..bad() }, "pdb track color_id");
    let too_fast = Beat::new(BeatInBar::One, Bpm::from_centi(65_536), Millis::new(0));
    let grid = Grid::Beats(BeatGrid::new(vec![too_fast]));
    assert_refused(NewTrack { grid, ..bad() }, "PQTZ tempo");
}

/// The start of a JPEG file: SOI, then an APP0 marker.
const JPEG: [u8; 6] = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10];

/// Each track with artwork gets its own artwork row, even for the same image, and the image
/// goes to the row's path as given.
#[test]
fn each_track_gets_its_own_artwork_row_and_file() {
    let image = JPEG.to_vec();
    let other = [&JPEG[..], b"other"].concat();
    let with = |artwork: Option<Vec<u8>>, file: &str| NewTrack {
        artwork,
        ..track("A", "B", file)
    };
    let mut builder = ExportBuilder::new(date());
    let ids = [
        builder
            .add_track(with(Some(image.clone()), "1.aiff"))
            .unwrap(),
        builder.add_track(with(None, "2.aiff")).unwrap(),
        builder
            .add_track(with(Some(image.clone()), "3.aiff"))
            .unwrap(),
        builder
            .add_track(with(Some(other.clone()), "4.aiff"))
            .unwrap(),
    ];
    let export = builder.build().unwrap();
    let db = database(&export);
    let artwork: Vec<Option<u32>> = ids
        .iter()
        .map(|&id| db.track(id).unwrap().artwork_id().map(|a| a.get()))
        .collect();
    assert_eq!(artwork, [Some(1), None, Some(2), Some(3)]);
    assert_eq!(db.artworks().len(), 3);
    for (id, bytes) in [(1, &image), (2, &image), (3, &other)] {
        let id = ArtworkId::new(id).unwrap();
        let path = ArtworkPath::for_artwork(id);
        assert_eq!(db.artwork(id).unwrap().path(), path.as_str());
        assert_eq!(file(&export, path.as_str()), Some(bytes.as_slice()));
    }
    assert_eq!(export.files().count(), 1 + 4 + 3);
}

#[test]
fn artwork_that_is_not_a_jpeg_is_refused() {
    for artwork in [
        vec![],
        vec![0xff],
        vec![0x89, b'P', b'N', b'G'],
        vec![0xd8, 0xff],
    ] {
        let artwork = Some(artwork);
        assert_refused(NewTrack { artwork, ..bad() }, "artwork");
    }
}

#[test]
fn playlists_and_folders_hold_tracks_in_the_order_added() {
    let mut builder = ExportBuilder::new(date());
    let one = builder.add_track(track("A", "B", "1.aiff")).unwrap();
    let two = builder.add_track(track("A", "B", "2.aiff")).unwrap();
    let folder = builder.add_folder("Folder", None).unwrap();
    let playlist = builder.add_playlist("Set", Some(folder)).unwrap();
    builder.add_to_playlist(playlist, two);
    builder.add_to_playlist(playlist, one);
    builder.add_to_playlist(playlist, two);
    assert_eq!(
        builder.add_playlist("Inside a playlist", Some(playlist)),
        Err(EncodeError::ValueOutOfRange {
            context: "pdb playlist parent_id"
        })
    );
    let db = database(&builder.build().unwrap());
    let nodes: Vec<_> = db
        .playlists()
        .map(|p| (p.id(), p.parent_id(), p.is_folder(), p.name().to_owned()))
        .collect();
    assert_eq!(
        nodes,
        [
            (folder, None, true, "Folder".to_owned()),
            (playlist, Some(folder), false, "Set".to_owned()),
        ]
    );
    let tracks: Vec<_> = db.playlist_tracks(playlist).collect();
    assert_eq!(tracks, [two, one, two]);
}

#[test]
fn a_dangling_playlist_reference_is_refused_at_build() {
    let mut builder = ExportBuilder::new(date());
    let playlist = builder.add_playlist("Set", None).unwrap();
    builder.add_to_playlist(playlist, TrackId::new(9).unwrap());
    let err = builder.build().unwrap_err();
    assert!(matches!(err, EncodeError::DanglingReference(_)), "{err:?}");

    let mut builder = ExportBuilder::new(date());
    builder
        .add_playlist("Set", Some(PlaylistId::new(7).unwrap()))
        .unwrap();
    let err = builder.build().unwrap_err();
    assert!(matches!(err, EncodeError::DanglingReference(_)), "{err:?}");
}

/// A fresh directory under the system's temporary directory, removed on drop.
struct TempRoot(std::path::PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("cdj-media-export-{}-{name}", std::process::id()));
        // Left over from an aborted run of this process id: start afresh.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create a temporary directory");
        Self(dir)
    }

    /// Where `path` goes under this root.
    fn file(&self, path: &StickPath) -> std::path::PathBuf {
        self.0.join(path.as_str().trim_start_matches('/'))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An export with a track, its artwork and a playlist.
fn small_export() -> Export {
    let mut builder = ExportBuilder::new(date());
    let id = builder
        .add_track(NewTrack {
            artwork: Some(JPEG.to_vec()),
            ..track("Björk", "Ålbum", "01 Ü.aiff")
        })
        .unwrap();
    let playlist = builder.add_playlist("Set", None).unwrap();
    builder.add_to_playlist(playlist, id);
    builder.build().unwrap()
}

#[test]
fn write_to_writes_every_file_under_the_root() {
    let root = TempRoot::new("writes");
    let export = small_export();
    export.write_to(&root.0).unwrap();
    let mut written = 0;
    for (path, bytes) in export.files() {
        assert_eq!(std::fs::read(root.file(path)).unwrap(), bytes, "{path}");
        written += 1;
    }
    assert_eq!(written, 3);
}

#[test]
fn write_to_refuses_and_overwrites_nothing_when_a_file_exists() {
    let root = TempRoot::new("refuses");
    let export = small_export();
    let pdb = export_pdb_path();
    let existing = root.file(&pdb);
    std::fs::create_dir_all(existing.parent().unwrap()).unwrap();
    std::fs::write(&existing, b"old").unwrap();

    let err = export.write_to(&root.0).unwrap_err();
    let crate::Error::Io(io) = &err else {
        panic!("not an I/O error: {err:?}");
    };
    assert_eq!(io.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&existing).unwrap(), b"old");
    for (path, _) in export.files().filter(|(p, _)| **p != pdb) {
        assert!(!root.file(path).exists(), "{path} was written");
    }
}

/// Paths that are the same file on FAT32 go through the allocator, in input order, and the
/// row, the `.DAT` and the destination all get the allocated path.
#[test]
fn colliding_tracks_get_distinct_allocated_paths() {
    let mut builder = ExportBuilder::new(date());
    let first = builder.add_track(track("ABBA", "Gold", "1.aiff")).unwrap();
    let second = builder.add_track(track("ABBA", "Gold", "1.aiff")).unwrap();
    let third = builder.add_track(track("Abba", "gold", "2.aiff")).unwrap();
    let export = builder.build().unwrap();

    let expected = [
        (first, "/Contents/ABBA/Gold/1.aiff", "1.aiff"),
        (second, "/Contents/ABBA/Gold/1 (2).aiff", "1 (2).aiff"),
        (third, "/Contents/ABBA/Gold/2.aiff", "2.aiff"),
    ];
    let destinations: Vec<_> = export
        .audio_destinations()
        .map(|(id, p)| (id, p.as_stick_path().as_str()))
        .collect();
    let paths: Vec<_> = expected.iter().map(|&(id, path, _)| (id, path)).collect();
    assert_eq!(destinations, paths);
    let db = database(&export);
    for (id, path, file_name) in expected {
        let row = db.track(id).unwrap();
        assert_eq!((row.file_path(), row.filename()), (path, file_name));
        assert_eq!(dat(&export, id).path(), Some(path));
    }
    // The names stay as given; only the path is spelled as the disk has it.
    assert_eq!(
        db.artist_of(db.track(third).unwrap()).unwrap().name(),
        "Abba"
    );
}
