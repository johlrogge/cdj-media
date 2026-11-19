use super::*;
use crate::DecodeErrorKind;
use crate::encoding::{Bpm, StringForm};
use crate::pdb::testutil::{LEN, data_page, file, row_index_len};
use crate::pdb::{
    Album as AlbumRow, Artist as ArtistRow, Artwork as ArtworkRow, Color as ColorRow, ExportRow,
    ExportTable, FileType, Genre as GenreRow, Key as KeyRow, Label as LabelRow,
    PlaylistEntry as PlaylistEntryRow, PlaylistNode as PlaylistNodeRow, Track as TrackRow,
    TrackFields,
};

/// `empty_candidate` of every synthetic table, also the last page's `next_page`.
const EMPTY: u32 = 1000;

/// A file of `tables`, in order, with each row on its own data page at heap offset 0; a
/// table without rows gets one data page without rows. Also returns the absolute offset of
/// every row, by table.
fn pdb_of(tables: &[(ExportTable, Vec<ExportRow>)]) -> (Pdb, Vec<Vec<u64>>) {
    let (bytes, offsets) = file_of(tables);
    let pdb = Pdb::from_bytes(&bytes).expect("synthetic file decodes");
    (pdb, offsets)
}

/// The bytes of [`pdb_of`]'s file, and the absolute offset of every row.
fn file_of(tables: &[(ExportTable, Vec<ExportRow>)]) -> (Vec<u8>, Vec<Vec<u64>>) {
    let mut pages = Vec::new();
    let mut pointers = Vec::new();
    let mut offsets = Vec::new();
    for (table, rows) in tables {
        let kind = table.to_u32();
        let first = pages.len() as u32 + 1;
        let last = first + rows.len().max(1) as u32 - 1;
        if rows.is_empty() {
            pages.push(data_page(first, kind, EMPTY, &[], &[]));
        }
        let mut at = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let index = first + i as u32;
            let next = if index == last { EMPTY } else { index + 1 };
            let bytes = row.encode().unwrap();
            assert!(
                bytes.len() <= LEN - 0x28 - row_index_len(1),
                "row fits a page"
            );
            pages.push(data_page(index, kind, next, &bytes, &[(0, true)]));
            at.push((index as usize * LEN + 0x28) as u64);
        }
        pointers.push([kind, EMPTY, first, last]);
        offsets.push(at);
    }
    (file(&pointers, &pages), offsets)
}

fn load(tables: &[(ExportTable, Vec<ExportRow>)]) -> Database {
    Database::from_pdb(&pdb_of(tables).0).expect("database loads")
}

fn track_row(id: u32, fields: TrackFields) -> ExportRow {
    ExportRow::Track(TrackRow::new(TrackId::new(id).unwrap(), fields).unwrap())
}

fn artist_row(id: u32, name: &str) -> ExportRow {
    ExportRow::Artist(ArtistRow::new(ArtistId::new(id).unwrap(), name))
}

fn album_row(id: u32, name: &str, artist: u32) -> ExportRow {
    ExportRow::Album(AlbumRow::new(
        AlbumId::new(id).unwrap(),
        name,
        ArtistId::new(artist),
    ))
}

fn genre_row(id: u32, name: &str) -> ExportRow {
    ExportRow::Genre(GenreRow::new(GenreId::new(id).unwrap(), name))
}

fn label_row(id: u32, name: &str) -> ExportRow {
    ExportRow::Label(LabelRow::new(LabelId::new(id).unwrap(), name))
}

fn key_row(id: u32, name: &str) -> ExportRow {
    ExportRow::Key(KeyRow::new(KeyId::new(id).unwrap(), name))
}

fn color_row(id: u16, name: &str) -> ExportRow {
    ExportRow::Color(ColorRow::new(ColorId::new(id).unwrap(), name))
}

fn artwork_row(id: u32, path: &str) -> ExportRow {
    ExportRow::Artwork(ArtworkRow::new(ArtworkId::new(id).unwrap(), path))
}

/// A track with every value set and every reference to a row of [`small_tables`].
fn full_fields() -> TrackFields {
    TrackFields {
        sample_rate: 44_100,
        composer_id: ArtistId::new(3),
        file_size: 1_234_567,
        artwork_id: ArtworkId::new(70),
        key_id: KeyId::new(50),
        original_artist_id: ArtistId::new(4),
        label_id: LabelId::new(40),
        remixer_id: ArtistId::new(5),
        bitrate: 320,
        track_number: 7,
        tempo: Bpm::from_centi(12_850),
        genre_id: GenreId::new(10),
        album_id: AlbumId::new(30),
        artist_id: ArtistId::new(2),
        disc_number: 2,
        play_count: 9,
        year: 1997,
        sample_depth: 24,
        duration: 245,
        color_id: ColorId::new(6),
        rating: 4,
        file_type: FileType::Aiff,
        isrc: "GBAYE9700001".into(),
        lyricist: "Lyr".into(),
        message: "Msg".into(),
        publish_track_info: true,
        autoload_hot_cues: false,
        date_added: "2024-01-02".into(),
        release_date: "1997-09-22".into(),
        mix_name: "Extended".into(),
        analysis_path: "/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT".into(),
        analysis_date: "2024-01-03".into(),
        comment: "Nice".into(),
        title: "Jóga".into(),
        filename: "a.aiff".into(),
        file_path: "/Contents/B/a.aiff".into(),
    }
}

/// One table of each small kind, with the rows [`full_fields`] refers to.
fn small_tables() -> Vec<(ExportTable, Vec<ExportRow>)> {
    vec![
        (
            ExportTable::Artists,
            vec![
                artist_row(2, "Björk"),
                artist_row(3, "Composer"),
                artist_row(4, "Original"),
                artist_row(5, "Remixer"),
            ],
        ),
        (ExportTable::Albums, vec![album_row(30, "Homogenic", 2)]),
        (ExportTable::Genres, vec![genre_row(10, "Pop")]),
        (ExportTable::Labels, vec![label_row(40, "One Little")]),
        (ExportTable::Keys, vec![key_row(50, "8A")]),
        (ExportTable::Colors, vec![color_row(6, "Aqua")]),
        (ExportTable::Artwork, vec![artwork_row(70, "/a1.jpg")]),
    ]
}

/// [`small_tables`] and a tracks table of `tracks`.
fn with_tracks(tracks: Vec<ExportRow>) -> Vec<(ExportTable, Vec<ExportRow>)> {
    let mut tables = vec![(ExportTable::Tracks, tracks)];
    tables.extend(small_tables());
    tables
}

#[test]
fn a_track_row_maps_to_its_domain_values() {
    let db = load(&with_tracks(vec![track_row(8, full_fields())]));
    assert_eq!(db.tracks().len(), 1);
    let track = db.track(TrackId::new(8).unwrap()).expect("track 8");
    assert_eq!(track.id(), TrackId::new(8).unwrap());
    assert_eq!(
        (track.title(), track.file_path(), track.filename()),
        ("Jóga", "/Contents/B/a.aiff", "a.aiff")
    );
    assert_eq!(
        track.analysis_path(),
        "/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT"
    );
    assert_eq!(
        (
            track.comment(),
            track.mix_name(),
            track.lyricist(),
            track.message()
        ),
        ("Nice", "Extended", "Lyr", "Msg")
    );
    assert_eq!(track.isrc(), Some("GBAYE9700001"));
    assert_eq!(
        (
            track.date_added(),
            track.release_date(),
            track.analysis_date()
        ),
        (Some("2024-01-02"), Some("1997-09-22"), Some("2024-01-03"))
    );
    assert!(track.publish_track_info() && !track.autoload_hot_cues());
    assert_eq!(track.tempo(), Bpm::from_centi(12_850));
    assert_eq!(
        (track.duration(), track.bitrate(), track.sample_rate()),
        (245, 320, 44_100)
    );
    assert_eq!((track.sample_depth(), track.file_size()), (24, 1_234_567));
    assert_eq!(
        (track.track_number(), track.disc_number(), track.year()),
        (7, 2, 1997)
    );
    assert_eq!((track.rating(), track.play_count()), (4, 9));
    assert_eq!(track.file_type(), FileType::Aiff);
    assert_eq!(
        (track.artist_id(), track.composer_id()),
        (ArtistId::new(2), ArtistId::new(3))
    );
    assert_eq!(
        (track.original_artist_id(), track.remixer_id()),
        (ArtistId::new(4), ArtistId::new(5))
    );
    assert_eq!(
        (track.album_id(), track.genre_id(), track.label_id()),
        (AlbumId::new(30), GenreId::new(10), LabelId::new(40))
    );
    assert_eq!(
        (track.key_id(), track.color_id(), track.artwork_id()),
        (KeyId::new(50), ColorId::new(6), ArtworkId::new(70))
    );
}

#[test]
fn a_loaded_track_lowers_to_the_fields_it_was_built_from() {
    let empty = TrackFields {
        isrc: String::new(),
        date_added: String::new(),
        release_date: String::new(),
        analysis_date: String::new(),
        publish_track_info: false,
        ..full_fields()
    };
    for fields in [full_fields(), empty, TrackFields::default()] {
        let db = load(&with_tracks(vec![track_row(8, fields.clone())]));
        let track = db.track(TrackId::new(8).unwrap()).unwrap();
        assert_eq!(track.to_fields(), fields);
    }
}

#[test]
fn small_table_rows_map_to_their_domain_values() {
    let db = load(&small_tables());
    assert_eq!(db.tracks().len(), 0);
    let names: Vec<(u32, &str)> = db.artists().map(|a| (a.id().get(), a.name())).collect();
    assert_eq!(
        names,
        [
            (2, "Björk"),
            (3, "Composer"),
            (4, "Original"),
            (5, "Remixer")
        ]
    );
    let album = db.album(AlbumId::new(30).unwrap()).expect("album 30");
    assert_eq!(
        (album.name(), album.artist_id()),
        ("Homogenic", ArtistId::new(2))
    );
    assert_eq!(db.genre(GenreId::new(10).unwrap()).unwrap().name(), "Pop");
    assert_eq!(
        db.label(LabelId::new(40).unwrap()).unwrap().name(),
        "One Little"
    );
    assert_eq!(db.key(KeyId::new(50).unwrap()).unwrap().name(), "8A");
    assert_eq!(db.color(ColorId::new(6).unwrap()).unwrap().name(), "Aqua");
    assert_eq!(
        db.artwork(ArtworkId::new(70).unwrap()).unwrap().path(),
        "/a1.jpg"
    );
    let counts = [
        db.albums().len(),
        db.genres().len(),
        db.labels().len(),
        db.keys().len(),
        db.colors().len(),
        db.artworks().len(),
    ];
    assert_eq!(counts, [1; 6]);
}

#[test]
fn a_utf16_title_arrives_as_a_plain_string() {
    let row = TrackRow::new(TrackId::new(8).unwrap(), full_fields()).unwrap();
    assert_eq!(row.title().form(), StringForm::Utf16Le, "guards the test");
    let db = load(&with_tracks(vec![ExportRow::Track(row)]));
    let title: &str = db.track(TrackId::new(8).unwrap()).unwrap().title();
    assert_eq!(title, "Jóga");
}

#[test]
fn empty_isrc_and_dates_are_none() {
    let fields = TrackFields {
        isrc: String::new(),
        date_added: String::new(),
        release_date: String::new(),
        analysis_date: String::new(),
        ..full_fields()
    };
    let db = load(&with_tracks(vec![track_row(8, fields)]));
    let track = db.track(TrackId::new(8).unwrap()).unwrap();
    assert_eq!(track.isrc(), None);
    assert_eq!(
        (
            track.date_added(),
            track.release_date(),
            track.analysis_date()
        ),
        (None, None, None)
    );
}

#[test]
fn rows_are_keyed_by_id_across_pages() {
    let tracks = vec![
        track_row(9, full_fields()),
        track_row(8, full_fields()),
        track_row(20, full_fields()),
    ];
    let db = load(&with_tracks(tracks));
    let ids: Vec<u32> = db.tracks().map(|t| t.id().get()).collect();
    assert_eq!(ids, [8, 9, 20]);
    assert!(db.track(TrackId::new(10).unwrap()).is_none());
}

#[test]
fn every_reference_resolves() {
    let db = load(&with_tracks(vec![track_row(8, full_fields())]));
    let track = db.track(TrackId::new(8).unwrap()).unwrap();
    fn artist(r: Option<&Artist>) -> Option<&str> {
        r.map(Artist::name)
    }
    assert_eq!(artist(db.artist_of(track)), Some("Björk"));
    assert_eq!(artist(db.composer_of(track)), Some("Composer"));
    assert_eq!(artist(db.original_artist_of(track)), Some("Original"));
    assert_eq!(artist(db.remixer_of(track)), Some("Remixer"));
    let album = db.album_of(track).expect("album");
    assert_eq!(album.name(), "Homogenic");
    assert_eq!(artist(db.album_artist_of(album)), Some("Björk"));
    assert_eq!(db.genre_of(track).map(Genre::name), Some("Pop"));
    assert_eq!(db.label_of(track).map(Label::name), Some("One Little"));
    assert_eq!(db.key_of(track).map(Key::name), Some("8A"));
    assert_eq!(db.color_of(track).map(Color::name), Some("Aqua"));
    assert_eq!(db.artwork_of(track).map(Artwork::path), Some("/a1.jpg"));
}

#[test]
fn references_of_none_resolve_to_none() {
    let tables = vec![
        (
            ExportTable::Tracks,
            vec![track_row(8, TrackFields::default())],
        ),
        (ExportTable::Albums, vec![album_row(30, "No artist", 0)]),
    ];
    let db = load(&tables);
    let track = db.track(TrackId::new(8).unwrap()).unwrap();
    assert!(db.artist_of(track).is_none() && db.composer_of(track).is_none());
    assert!(db.original_artist_of(track).is_none() && db.remixer_of(track).is_none());
    assert!(db.album_of(track).is_none() && db.genre_of(track).is_none());
    assert!(db.label_of(track).is_none() && db.key_of(track).is_none());
    assert!(db.color_of(track).is_none() && db.artwork_of(track).is_none());
    let album = db.album(AlbumId::new(30).unwrap()).unwrap();
    assert!(db.album_artist_of(album).is_none());
}

#[test]
fn a_duplicate_id_is_an_error_at_the_second_rows_id() {
    // Each table with two rows of the same id, the row offset of its id field and the
    // error context.
    let cases = [
        (
            ExportTable::Tracks,
            [track_row(8, full_fields()), track_row(8, full_fields())],
            0x48,
            "pdb track id",
            8,
        ),
        (
            ExportTable::Artists,
            [artist_row(9, "A"), artist_row(9, "B")],
            4,
            "pdb artist id",
            9,
        ),
        (
            ExportTable::Albums,
            [album_row(31, "A", 2), album_row(31, "B", 2)],
            0x0c,
            "pdb album id",
            31,
        ),
        (
            ExportTable::Genres,
            [genre_row(11, "A"), genre_row(11, "B")],
            0,
            "pdb genre id",
            11,
        ),
        (
            ExportTable::Labels,
            [label_row(41, "A"), label_row(41, "B")],
            0,
            "pdb label id",
            41,
        ),
        (
            ExportTable::Keys,
            [key_row(51, "A"), key_row(51, "B")],
            0,
            "pdb key id",
            51,
        ),
        (
            ExportTable::Colors,
            [color_row(7, "A"), color_row(7, "B")],
            5,
            "pdb color id",
            7,
        ),
        (
            ExportTable::Artwork,
            [artwork_row(71, "/a"), artwork_row(71, "/b")],
            0,
            "pdb artwork id",
            71,
        ),
    ];
    for (table, duplicates, id_at, context, id) in cases {
        // Every table as in `with_tracks`, so every reference resolves, and the two rows
        // after the table's own.
        let mut tables = with_tracks(vec![track_row(1, full_fields())]);
        let t = tables.iter().position(|(kind, _)| *kind == table).unwrap();
        tables[t].1.extend(duplicates);
        let (bytes, offsets) = file_of(&tables);
        let err = Database::from_pdb(&Pdb::from_bytes(&bytes).unwrap()).unwrap_err();
        let second = *offsets[t].last().unwrap();
        assert_eq!(
            (err.offset, err.context, err.kind),
            (second + id_at, context, DecodeErrorKind::DuplicateId { id }),
            "{table:?}"
        );
        // The reported offset holds the id in the file: 2 bytes for a colour, 4 otherwise.
        let width = if table == ExportTable::Colors { 2 } else { 4 };
        let at = err.offset as usize;
        assert_eq!(
            bytes[at..at + width],
            id.to_le_bytes()[..width],
            "{table:?}: the id's bytes"
        );
    }
}

#[test]
fn a_tombstoned_old_copy_of_a_row_is_not_a_duplicate() {
    // A genre rewritten in place: the old copy's presence bit is clear, the new copy
    // with the same id is present.
    let old = genre_row(10, "Old").encode().unwrap();
    let new = genre_row(10, "New").encode().unwrap();
    assert_eq!(old.len(), 8, "the new copy starts at heap offset 8");
    let heap = [old, new].concat();
    let page = data_page(
        1,
        ExportTable::Genres.to_u32(),
        EMPTY,
        &heap,
        &[(0, false), (8, true)],
    );
    let bytes = file(&[[ExportTable::Genres.to_u32(), EMPTY, 1, 1]], &[page]);
    let db = load_bytes(&bytes);
    let names: Vec<&str> = db.genres().map(Genre::name).collect();
    assert_eq!(names, ["New"]);
}

fn load_bytes(bytes: &[u8]) -> Database {
    Database::from_pdb(&Pdb::from_bytes(bytes).unwrap()).expect("database loads")
}

#[test]
fn row_decode_errors_keep_their_absolute_offsets() {
    let tables = with_tracks(vec![track_row(8, full_fields())]);
    let (mut bytes, offsets) = file_of(&tables);
    let genres = tables
        .iter()
        .position(|(t, _)| *t == ExportTable::Genres)
        .unwrap();
    // Genre id 0, which no row may have.
    let at = offsets[genres][0] as usize;
    assert_eq!(
        bytes[at..at + 4],
        10u32.to_le_bytes(),
        "genre 10's id is there"
    );
    bytes[at..at + 4].fill(0);
    let pdb = Pdb::from_bytes(&bytes).unwrap();
    let err = Database::from_pdb(&pdb).unwrap_err();
    assert_eq!(
        (err.offset, err.context, err.kind),
        (at as u64, "pdb genre id", DecodeErrorKind::InvalidValue)
    );
}

/// Sets one reference of a track to a row that does not exist.
type Dangle = fn(&mut TrackFields);
/// Whether the reference resolves to `None`.
type Unresolved = fn(&Database, &Track) -> bool;

#[test]
fn a_dangling_track_reference_loads_and_is_reported() {
    use Reference as R;
    let artist = RecordId::Artist(ArtistId::new(99).unwrap());
    let cases: [(Dangle, Reference, RecordId, Unresolved); 10] = [
        (
            |f| f.artist_id = ArtistId::new(99),
            R::TrackArtist,
            artist,
            |db, t| db.artist_of(t).is_none(),
        ),
        (
            |f| f.composer_id = ArtistId::new(99),
            R::TrackComposer,
            artist,
            |db, t| db.composer_of(t).is_none(),
        ),
        (
            |f| f.original_artist_id = ArtistId::new(99),
            R::TrackOriginalArtist,
            artist,
            |db, t| db.original_artist_of(t).is_none(),
        ),
        (
            |f| f.remixer_id = ArtistId::new(99),
            R::TrackRemixer,
            artist,
            |db, t| db.remixer_of(t).is_none(),
        ),
        (
            |f| f.album_id = AlbumId::new(99),
            R::TrackAlbum,
            RecordId::Album(AlbumId::new(99).unwrap()),
            |db, t| db.album_of(t).is_none(),
        ),
        (
            |f| f.genre_id = GenreId::new(99),
            R::TrackGenre,
            RecordId::Genre(GenreId::new(99).unwrap()),
            |db, t| db.genre_of(t).is_none(),
        ),
        (
            |f| f.label_id = LabelId::new(99),
            R::TrackLabel,
            RecordId::Label(LabelId::new(99).unwrap()),
            |db, t| db.label_of(t).is_none(),
        ),
        (
            |f| f.key_id = KeyId::new(99),
            R::TrackKey,
            RecordId::Key(KeyId::new(99).unwrap()),
            |db, t| db.key_of(t).is_none(),
        ),
        (
            |f| f.color_id = ColorId::new(99),
            R::TrackColor,
            RecordId::Color(ColorId::new(99).unwrap()),
            |db, t| db.color_of(t).is_none(),
        ),
        (
            |f| f.artwork_id = ArtworkId::new(99),
            R::TrackArtwork,
            RecordId::Artwork(ArtworkId::new(99).unwrap()),
            |db, t| db.artwork_of(t).is_none(),
        ),
    ];
    for (dangle, field, id, unresolved) in cases {
        let mut fields = full_fields();
        dangle(&mut fields);
        let tracks = vec![track_row(1, full_fields()), track_row(8, fields)];
        let db = load(&with_tracks(tracks));
        let track = db.track(TrackId::new(8).unwrap()).unwrap();
        assert!(unresolved(&db, track), "{field:?}: resolves to None");
        let dangling: Vec<Dangling> = db.dangling_references().collect();
        let from = RecordId::Track(TrackId::new(8).unwrap());
        let id = Some(id);
        assert_eq!(dangling, [Dangling { from, field, id }], "{field:?}");
    }
}

#[test]
fn a_dangling_album_artist_loads_and_is_reported() {
    let mut tables = with_tracks(vec![track_row(8, full_fields())]);
    let albums = tables
        .iter()
        .position(|(t, _)| *t == ExportTable::Albums)
        .unwrap();
    tables[albums].1.push(album_row(31, "Lost", 99));
    let db = load(&tables);
    let album = db.album(AlbumId::new(31).unwrap()).unwrap();
    assert_eq!(album.artist_id(), ArtistId::new(99), "keeps the raw id");
    assert!(db.album_artist_of(album).is_none());
    let dangling: Vec<Dangling> = db.dangling_references().collect();
    assert_eq!(
        dangling,
        [Dangling {
            from: RecordId::Album(AlbumId::new(31).unwrap()),
            field: Reference::AlbumArtist,
            id: Some(RecordId::Artist(ArtistId::new(99).unwrap())),
        }]
    );
}

#[test]
fn contains_finds_records_of_every_table() {
    let db = load(&with_tracks(vec![track_row(8, full_fields())]));
    let present = [
        RecordId::Track(TrackId::new(8).unwrap()),
        RecordId::Artist(ArtistId::new(2).unwrap()),
        RecordId::Album(AlbumId::new(30).unwrap()),
        RecordId::Genre(GenreId::new(10).unwrap()),
        RecordId::Label(LabelId::new(40).unwrap()),
        RecordId::Key(KeyId::new(50).unwrap()),
        RecordId::Color(ColorId::new(6).unwrap()),
        RecordId::Artwork(ArtworkId::new(70).unwrap()),
    ];
    for id in present {
        assert!(db.contains(id), "{id:?}");
    }
    let absent = [
        RecordId::Track(TrackId::new(9).unwrap()),
        RecordId::Artist(ArtistId::new(99).unwrap()),
        RecordId::Album(AlbumId::new(99).unwrap()),
        RecordId::Genre(GenreId::new(99).unwrap()),
        RecordId::Label(LabelId::new(99).unwrap()),
        RecordId::Key(KeyId::new(99).unwrap()),
        RecordId::Color(ColorId::new(99).unwrap()),
        RecordId::Artwork(ArtworkId::new(99).unwrap()),
    ];
    for id in absent {
        assert!(!db.contains(id), "{id:?}");
    }
    assert_eq!(db.dangling_references().count(), 0);
}

#[test]
fn a_new_database_holds_only_the_default_colours() {
    let db = Database::new();
    let counts = [
        db.tracks().len(),
        db.artists().len(),
        db.albums().len(),
        db.genres().len(),
        db.labels().len(),
        db.keys().len(),
        db.artworks().len(),
    ];
    assert_eq!(counts, [0; 7]);
    let colors: Vec<(u16, &str)> = db.colors().map(|c| (c.id().get(), c.name())).collect();
    assert_eq!(
        colors,
        [
            (1, "Pink"),
            (2, "Red"),
            (3, "Orange"),
            (4, "Yellow"),
            (5, "Green"),
            (6, "Aqua"),
            (7, "Blue"),
            (8, "Purple")
        ]
    );
    assert_eq!(Database::default(), db);
}

#[test]
fn names_are_found_or_inserted_with_the_next_id() {
    let mut db = Database::new();
    let bjork = db.find_or_insert_artist("Björk");
    assert_eq!(bjork, ArtistId::new(1), "the first id is 1");
    assert_eq!(db.find_or_insert_artist("Björk"), bjork, "found");
    // Exact names: case-sensitive and not trimmed.
    assert_eq!(db.find_or_insert_artist("björk"), ArtistId::new(2));
    assert_eq!(db.find_or_insert_artist("Björk "), ArtistId::new(3));
    let names: Vec<(u32, &str)> = db.artists().map(|a| (a.id().get(), a.name())).collect();
    assert_eq!(names, [(1, "Björk"), (2, "björk"), (3, "Björk ")]);

    let genre = db.find_or_insert_genre("Pop");
    assert_eq!(genre, GenreId::new(1));
    assert_eq!(db.find_or_insert_genre("House"), GenreId::new(2));
    assert_eq!(db.find_or_insert_genre("Pop"), genre);
    let label = db.find_or_insert_label("One Little");
    assert_eq!(label, LabelId::new(1));
    assert_eq!(db.find_or_insert_label("One Little"), label);
    let key = db.find_or_insert_key("8A");
    assert_eq!(key, KeyId::new(1));
    assert_eq!(db.find_or_insert_key("Am"), KeyId::new(2));
    assert_eq!(db.find_or_insert_key("8A"), key);
    let artwork = db.find_or_insert_artwork("/PIONEER/Artwork/00001/a1.jpg");
    assert_eq!(artwork, ArtworkId::new(1));
    assert_eq!(
        db.find_or_insert_artwork("/PIONEER/Artwork/00001/a1.jpg"),
        artwork
    );
    let counts = [
        db.genres().len(),
        db.labels().len(),
        db.keys().len(),
        db.artworks().len(),
    ];
    assert_eq!(counts, [2, 1, 2, 1]);
    let label = db.label(LabelId::new(1).unwrap()).unwrap();
    assert_eq!(label.name(), "One Little");
}

#[test]
fn an_empty_name_is_no_record() {
    let mut db = Database::new();
    assert_eq!(db.find_or_insert_artist(""), None);
    assert_eq!(db.find_or_insert_album("", ArtistId::new(1)), None);
    assert_eq!(db.find_or_insert_genre(""), None);
    assert_eq!(db.find_or_insert_label(""), None);
    assert_eq!(db.find_or_insert_key(""), None);
    assert_eq!(db.find_or_insert_artwork(""), None);
    assert_eq!(db, Database::new());
}

#[test]
fn albums_are_found_by_name_and_album_artist() {
    let mut db = Database::new();
    let a = db.find_or_insert_artist("A");
    let b = db.find_or_insert_artist("B");
    let hits_a = db.find_or_insert_album("Hits", a);
    assert_eq!(hits_a, AlbumId::new(1));
    assert_eq!(db.find_or_insert_album("Hits", a), hits_a);
    assert_eq!(db.find_or_insert_album("Hits", b), AlbumId::new(2));
    assert_eq!(db.find_or_insert_album("Hits", None), AlbumId::new(3));
    assert_eq!(db.find_or_insert_album("Hits", None), AlbumId::new(3));
    let albums: Vec<(u32, &str, Option<ArtistId>)> = db
        .albums()
        .map(|album| (album.id().get(), album.name(), album.artist_id()))
        .collect();
    assert_eq!(
        albums,
        [(1, "Hits", a), (2, "Hits", b), (3, "Hits", None)],
        "one album per album artist"
    );
}

#[test]
fn a_loaded_database_finds_its_names_and_continues_after_its_last_id() {
    let mut db = load(&small_tables());
    assert_eq!(db.find_or_insert_artist("Björk"), ArtistId::new(2));
    assert_eq!(db.find_or_insert_artist("Sugarcubes"), ArtistId::new(6));
    assert_eq!(
        db.find_or_insert_album("Homogenic", ArtistId::new(2)),
        AlbumId::new(30)
    );
    assert_eq!(
        db.find_or_insert_album("Homogenic", ArtistId::new(3)),
        AlbumId::new(31)
    );
    assert_eq!(db.find_or_insert_genre("Pop"), GenreId::new(10));
    assert_eq!(db.find_or_insert_label("Other"), LabelId::new(41));
    assert_eq!(db.find_or_insert_key("8A"), KeyId::new(50));
    assert_eq!(db.find_or_insert_artwork("/a2.jpg"), ArtworkId::new(71));
}

#[test]
fn after_the_highest_id_the_lowest_free_one_is_taken() {
    let artists = vec![artist_row(1, "First"), artist_row(u32::MAX, "Last")];
    let mut db = load(&[(ExportTable::Artists, artists)]);
    assert_eq!(db.find_or_insert_artist("New"), ArtistId::new(2));
    let names: Vec<(u32, &str)> = db.artists().map(|a| (a.id().get(), a.name())).collect();
    assert_eq!(names, [(1, "First"), (2, "New"), (u32::MAX, "Last")]);
}

#[test]
fn tracks_are_added_with_the_next_id() {
    let mut db = load(&with_tracks(vec![track_row(8, full_fields())]));
    let first = db.add_track(full_fields()).unwrap();
    let second = db.add_track(TrackFields::default()).unwrap();
    assert_eq!((first.get(), second.get()), (9, 10));
    let track = |id| db.track(TrackId::new(id).unwrap()).unwrap().to_fields();
    assert_eq!(track(9), full_fields());
    assert_eq!(track(10), TrackFields::default());
    let mut db = Database::new();
    assert_eq!(db.add_track(full_fields()).map(TrackId::get), Ok(1));
}

/// `next_track_id` and `next_artwork_id` are the ids the next insert allocates, and peeking
/// changes nothing.
fn assert_peeks_match_inserts(mut db: Database, path: &str) {
    for _ in 0..2 {
        let (track, artwork) = (db.next_track_id(), db.next_artwork_id());
        let before = db.clone();
        assert_eq!(db.next_track_id(), track, "peeking twice");
        assert_eq!(db, before, "peeking changes nothing");
        assert_eq!(db.add_track(TrackFields::default()), Ok(track));
        let path = format!("{path}/{}", artwork.get());
        assert_eq!(db.find_or_insert_artwork(&path), Some(artwork));
    }
}

#[test]
fn the_next_ids_are_those_the_next_inserts_allocate() {
    let db = Database::new();
    assert_eq!(
        (db.next_track_id().get(), db.next_artwork_id().get()),
        (1, 1)
    );
    assert_peeks_match_inserts(db, "/new");
}

#[test]
fn a_loaded_database_peeks_the_ids_after_its_last() {
    let db = load(&with_tracks(vec![track_row(8, full_fields())]));
    assert_eq!(
        (db.next_track_id().get(), db.next_artwork_id().get()),
        (9, 71)
    );
    assert_peeks_match_inserts(db, "/new");
}

#[test]
fn after_the_highest_id_the_peek_is_the_lowest_free_one() {
    let db = load(&[
        (
            ExportTable::Tracks,
            vec![
                track_row(1, full_fields()),
                track_row(u32::MAX, full_fields()),
            ],
        ),
        (
            ExportTable::Artwork,
            vec![artwork_row(u32::MAX, "/last.jpg")],
        ),
    ]);
    assert_eq!(
        (db.next_track_id().get(), db.next_artwork_id().get()),
        (2, 1)
    );
    assert_peeks_match_inserts(db, "/new");
}

#[test]
fn a_track_the_writer_cannot_encode_is_not_added() {
    use crate::EncodeError;
    let mut db = Database::new();
    let isrc = TrackFields {
        isrc: "Björk".into(),
        ..full_fields()
    };
    let color = TrackFields {
        color_id: ColorId::new(0x100),
        ..full_fields()
    };
    let out_of_range = |context| Err(EncodeError::ValueOutOfRange { context });
    assert_eq!(db.add_track(isrc), out_of_range("DeviceSQL ISRC text"));
    assert_eq!(db.add_track(color), out_of_range("pdb track color_id"));
    assert_eq!(db, Database::new());
}

/// A database built through the find-or-insert API: two tracks sharing an artist, an
/// album, a genre and a label, with keys and artwork of their own.
fn built() -> Database {
    let mut db = Database::new();
    for (title, key, artwork) in [("Jóga", "8A", "/a1.jpg"), ("Hunter", "Am", "/a2.jpg")] {
        let artist_id = db.find_or_insert_artist("Björk");
        let fields = TrackFields {
            title: title.into(),
            artist_id,
            composer_id: db.find_or_insert_artist("Composer"),
            original_artist_id: db.find_or_insert_artist("Björk"),
            remixer_id: db.find_or_insert_artist("Remixer"),
            album_id: db.find_or_insert_album("Homogenic", artist_id),
            genre_id: db.find_or_insert_genre("Pop"),
            label_id: db.find_or_insert_label("One Little"),
            key_id: db.find_or_insert_key(key),
            artwork_id: db.find_or_insert_artwork(artwork),
            color_id: ColorId::new(6),
            ..full_fields()
        };
        db.add_track(fields).unwrap();
    }
    db
}

/// The raw values of `ids`.
fn raw<I: Into<u32>>(ids: impl Iterator<Item = I>) -> Vec<u32> {
    ids.map(Into::into).collect()
}

/// `db` written with `to_pdb`, encoded, decoded and loaded again.
fn written_and_loaded(db: &Database) -> Database {
    let pdb = db.to_pdb().expect("to_pdb");
    assert_eq!(pdb.len_page(), 4096);
    let bytes = pdb.to_bytes().unwrap();
    Database::from_pdb(&Pdb::from_bytes(&bytes).unwrap()).expect("the written file loads")
}

#[test]
fn a_built_database_survives_to_pdb_and_back() {
    let db = built();
    assert_eq!(raw(db.tracks().map(|t| t.id())), [1, 2]);
    assert_eq!(raw(db.artists().map(|a| a.id())), [1, 2, 3]);
    assert_eq!(raw(db.albums().map(|a| a.id())), [1]);
    assert_eq!(raw(db.genres().map(|g| g.id())), [1]);
    assert_eq!(raw(db.labels().map(|l| l.id())), [1]);
    assert_eq!(raw(db.keys().map(|k| k.id())), [1, 2]);
    assert_eq!(raw(db.artworks().map(|a| a.id())), [1, 2]);
    assert_eq!(db.dangling_references().count(), 0);
    assert_eq!(written_and_loaded(&db), db);
    assert_eq!(written_and_loaded(&Database::new()), Database::new());
}

#[test]
fn a_loaded_database_writes_its_ids_back_unchanged() {
    let colors = crate::pdb::writer::default_colors();
    let mut tables = with_tracks(vec![
        track_row(20, full_fields()),
        track_row(8, TrackFields::default()),
    ]);
    for (table, rows) in &mut tables {
        if *table == ExportTable::Colors {
            *rows = colors.clone();
        }
    }
    let db = load(&tables);
    assert_eq!(db.colors().len(), 8, "guards the test");
    assert_eq!(written_and_loaded(&db), db);
}

#[test]
fn a_renamed_colour_keeps_its_name_through_to_pdb() {
    // The track references colour 6, renamed from "Aqua".
    let mut colors = crate::pdb::writer::default_colors();
    colors[5] = color_row(6, "Teal");
    let mut tables = with_tracks(vec![track_row(8, full_fields())]);
    for (table, rows) in &mut tables {
        if *table == ExportTable::Colors {
            *rows = colors.clone();
        }
    }
    let db = load(&tables);
    let back = written_and_loaded(&db);
    let teal = back.color(ColorId::new(6).unwrap()).map(Color::name);
    assert_eq!(teal, Some("Teal"));
    assert_eq!(back, db);
}

#[test]
fn a_dangling_reference_fails_to_pdb() {
    use crate::EncodeError;
    let mut db = built();
    db.add_track(TrackFields {
        album_id: AlbumId::new(7),
        ..TrackFields::default()
    })
    .unwrap();
    let dangling = Dangling {
        from: RecordId::Track(TrackId::new(3).unwrap()),
        field: Reference::TrackAlbum,
        id: Some(RecordId::Album(AlbumId::new(7).unwrap())),
    };
    let err = db.to_pdb().unwrap_err();
    assert_eq!(err, EncodeError::DanglingReference(dangling));
    assert_eq!(
        err.to_string(),
        "dangling reference: TrackAlbum of Track(TrackId(3)) names a missing \
         Album(AlbumId(7))"
    );
    // A loaded album whose artist is missing.
    let db = load(&[(ExportTable::Albums, vec![album_row(30, "Lost", 99)])]);
    let dangling = Dangling {
        from: RecordId::Album(AlbumId::new(30).unwrap()),
        field: Reference::AlbumArtist,
        id: Some(RecordId::Artist(ArtistId::new(99).unwrap())),
    };
    let expected = EncodeError::DanglingReference(dangling);
    assert_eq!(db.to_pdb(), Err(expected));
}

fn playlist_id(id: u32) -> PlaylistId {
    PlaylistId::new(id).unwrap()
}

fn node_row(id: u32, parent: u32, sort_order: u32, folder: bool, name: &str) -> ExportRow {
    let parent = PlaylistId::new(parent);
    let row = PlaylistNodeRow::new(playlist_id(id), parent, sort_order, folder, name);
    ExportRow::PlaylistNode(row)
}

fn entry_row(index: u32, track: u32, playlist: u32) -> ExportRow {
    let track = TrackId::new(track).unwrap();
    ExportRow::PlaylistEntry(PlaylistEntryRow::new(index, track, playlist_id(playlist)))
}

/// The tracks of `id`, as raw ids.
fn tracks_of(db: &Database, id: u32) -> Vec<u32> {
    raw(db.playlist_tracks(playlist_id(id)))
}

#[test]
fn playlist_rows_map_to_playlists_with_ordered_tracks() {
    let tables = vec![
        (
            ExportTable::Tracks,
            vec![
                track_row(8, TrackFields::default()),
                track_row(9, TrackFields::default()),
            ],
        ),
        (
            ExportTable::PlaylistTree,
            vec![
                node_row(3, 2, 0, false, "Warmup"),
                node_row(2, 0, 1, true, "Sets"),
                node_row(5, 0, 0, false, "Empty"),
            ],
        ),
        // Entries out of index order and across pages.
        (
            ExportTable::PlaylistEntries,
            vec![entry_row(2, 8, 3), entry_row(1, 9, 3), entry_row(3, 9, 3)],
        ),
    ];
    let db = load(&tables);
    let playlists: Vec<(u32, Option<PlaylistId>, u32, bool, &str)> = db
        .playlists()
        .map(|p| {
            let id = p.id().get();
            (id, p.parent_id(), p.sort_order(), p.is_folder(), p.name())
        })
        .collect();
    assert_eq!(
        playlists,
        [
            (2, None, 1, true, "Sets"),
            (3, PlaylistId::new(2), 0, false, "Warmup"),
            (5, None, 0, false, "Empty"),
        ]
    );
    assert_eq!(tracks_of(&db, 3), [9, 8, 9]);
    assert!(tracks_of(&db, 5).is_empty() && tracks_of(&db, 2).is_empty());
    let empty = db.playlist(playlist_id(5));
    assert_eq!(empty.map(Playlist::name), Some("Empty"));
    assert_eq!(db.dangling_references().count(), 0);
}

#[test]
fn a_built_playlist_survives_to_pdb_and_back() {
    // Two tracks, 1 and 2, with every reference set.
    let mut db = built();
    let [first, second] = [1, 2].map(|id| TrackId::new(id).unwrap());
    let id = db.add_playlist("Warmup", None).unwrap();
    db.add_to_playlist(id, first);
    db.add_to_playlist(id, second);
    assert_eq!(id, playlist_id(1));
    assert_eq!(db.playlist_tracks(id).collect::<Vec<_>>(), [first, second]);
    let back = written_and_loaded(&db);
    assert_eq!(back, db);
    // The rows as rekordbox writes them.
    let pdb = db.to_pdb().unwrap();
    let rows = |table| -> Vec<ExportRow> {
        pdb.table_pages(table)
            .flat_map(|page| page.rows())
            .map(|raw| ExportRow::decode(table, &raw).unwrap().unwrap())
            .collect()
    };
    assert_eq!(
        rows(ExportTable::PlaylistTree),
        [node_row(1, 0, 0, false, "Warmup")]
    );
    assert_eq!(
        rows(ExportTable::PlaylistEntries),
        [entry_row(1, 1, 1), entry_row(2, 2, 1)]
    );
}

#[test]
fn playlists_and_folders_get_the_next_id_and_sort_order() {
    let mut db = Database::new();
    let a = db.add_playlist("A", None).unwrap();
    let sets = db.add_folder("Sets", None).unwrap();
    let b = db.add_playlist("B", Some(sets)).unwrap();
    let c = db.add_playlist("C", None).unwrap();
    let d = db.add_playlist("D", Some(sets)).unwrap();
    let shape: Vec<(u32, Option<PlaylistId>, u32, bool)> = db
        .playlists()
        .map(|p| (p.id().get(), p.parent_id(), p.sort_order(), p.is_folder()))
        .collect();
    assert_eq!(
        shape,
        [
            (1, None, 0, false),
            (2, None, 1, true),
            (3, Some(sets), 0, false),
            (4, None, 2, false),
            (5, Some(sets), 1, false),
        ]
    );
    assert_eq!(raw([a, sets, b, c, d].into_iter()), [1, 2, 3, 4, 5]);
    assert_eq!(written_and_loaded(&db), db);
    // A loaded database continues after its last id and its children's last sort order.
    let nodes = vec![node_row(7, 0, 4, false, "X"), node_row(3, 0, 9, false, "Y")];
    let mut db = load(&[(ExportTable::PlaylistTree, nodes)]);
    let next = db.add_playlist("Z", None).unwrap();
    assert_eq!(next, playlist_id(8));
    assert_eq!(db.playlist(next).map(Playlist::sort_order), Some(10));
}

#[test]
fn a_playlist_name_too_long_to_write_is_not_added() {
    use crate::EncodeError;
    let mut db = Database::new();
    // Longer than the 16-bit length of a long DeviceSQL string.
    let name = "é".repeat(0x8000);
    let err = Err(EncodeError::LengthOverflow {
        context: "DeviceSQL string length",
    });
    assert_eq!(db.add_playlist(&name, None), err);
    assert_eq!(db.add_folder(&name, None), err);
    assert_eq!(db, Database::new());
}

#[test]
fn dangling_playlist_references_are_reported_and_refused() {
    use crate::EncodeError;
    let track = track_row(8, TrackFields::default());
    let nodes = vec![node_row(1, 0, 0, false, "A"), node_row(2, 9, 0, false, "B")];
    let tables = vec![
        (ExportTable::Tracks, vec![track]),
        (ExportTable::PlaylistTree, nodes),
        (
            ExportTable::PlaylistEntries,
            vec![entry_row(1, 8, 1), entry_row(2, 7, 1), entry_row(1, 8, 4)],
        ),
    ];
    let db = load(&tables);
    let entry = |playlist, index| RecordId::PlaylistEntry {
        playlist: PlaylistId::new(playlist),
        index,
    };
    let dangling: Vec<Dangling> = db.dangling_references().collect();
    assert_eq!(
        dangling,
        [
            Dangling {
                from: RecordId::Playlist(playlist_id(2)),
                field: Reference::PlaylistParent,
                id: Some(RecordId::Playlist(playlist_id(9))),
            },
            Dangling {
                from: entry(1, 2),
                field: Reference::PlaylistEntryTrack,
                id: Some(RecordId::Track(TrackId::new(7).unwrap())),
            },
            Dangling {
                from: entry(4, 1),
                field: Reference::PlaylistEntryPlaylist,
                id: Some(RecordId::Playlist(playlist_id(4))),
            },
        ]
    );
    assert_eq!(
        db.to_pdb(),
        Err(EncodeError::DanglingReference(dangling[0]))
    );
    assert!(db.contains(entry(1, 2)) && db.contains(entry(4, 1)));
    assert!(!db.contains(entry(1, 3)) && !db.contains(entry(1, 0)));
    assert!(db.contains(RecordId::Playlist(playlist_id(2))));
    assert!(!db.contains(RecordId::Playlist(playlist_id(9))));
    // Built: a track added to a playlist the database doesn't have.
    let mut db = Database::new();
    let track = db.add_track(TrackFields::default()).unwrap();
    db.add_to_playlist(playlist_id(3), track);
    let err = db.to_pdb().unwrap_err();
    assert_eq!(
        err.to_string(),
        "dangling reference: PlaylistEntryPlaylist of PlaylistEntry { playlist: \
         Some(PlaylistId(3)), index: 1 } names a missing Playlist(PlaylistId(3))"
    );
}

#[test]
fn bad_playlist_rows_are_errors_at_the_field() {
    // A duplicate playlist id, at the second row's id.
    let nodes = vec![node_row(4, 0, 0, false, "A"), node_row(4, 0, 1, false, "B")];
    let (bytes, offsets) = file_of(&[(ExportTable::PlaylistTree, nodes)]);
    let err = Database::from_pdb(&Pdb::from_bytes(&bytes).unwrap()).unwrap_err();
    assert_eq!(
        (err.offset, err.context, err.kind),
        (
            offsets[0][1] + 0x0c,
            "pdb playlist id",
            DecodeErrorKind::DuplicateId { id: 4 }
        )
    );
    // A duplicate entry index of one playlist, at the second row's index.
    let tables = vec![(
        ExportTable::PlaylistEntries,
        vec![entry_row(1, 8, 4), entry_row(1, 9, 4)],
    )];
    let (bytes, offsets) = file_of(&tables);
    let err = Database::from_pdb(&Pdb::from_bytes(&bytes).unwrap()).unwrap_err();
    assert_eq!(
        (err.offset, err.context, err.kind),
        (
            offsets[0][1],
            "pdb playlist entry index",
            DecodeErrorKind::DuplicateId { id: 1 }
        )
    );
}

/// An entry row of the given (entry index, track, playlist), zeros included.
fn raw_entry_row(values: [u32; 3]) -> ExportRow {
    let mut bytes = Vec::new();
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let row = PlaylistEntryRow::decode(&crate::pdb::RawRow::new(0, &bytes, 0)).unwrap();
    ExportRow::PlaylistEntry(row)
}

#[test]
fn an_entry_of_track_or_playlist_0_loads_and_is_reported() {
    use crate::EncodeError;
    let track = track_row(8, TrackFields::default());
    let node = node_row(4, 0, 0, false, "A");
    let entries = vec![raw_entry_row([1, 0, 4]), raw_entry_row([2, 8, 0])];
    let tables = vec![
        (ExportTable::Tracks, vec![track]),
        (ExportTable::PlaylistTree, vec![node]),
        (ExportTable::PlaylistEntries, entries),
    ];
    let db = load(&tables);
    let entry = |playlist, index| RecordId::PlaylistEntry {
        playlist: PlaylistId::new(playlist),
        index,
    };
    let dangling: Vec<Dangling> = db.dangling_references().collect();
    // 0 means missing; the entry of playlist 0 comes first.
    let expected = [
        Dangling {
            from: entry(0, 1),
            field: Reference::PlaylistEntryPlaylist,
            id: None,
        },
        Dangling {
            from: entry(4, 1),
            field: Reference::PlaylistEntryTrack,
            id: None,
        },
    ];
    assert_eq!(dangling, expected);
    assert!(db.contains(entry(0, 1)) && db.contains(entry(4, 1)));
    assert_eq!(tracks_of(&db, 4), Vec::<u32>::new(), "track 0 is skipped");
    let err = db.to_pdb().unwrap_err();
    assert_eq!(err, EncodeError::DanglingReference(expected[0]));
    assert_eq!(
        err.to_string(),
        "dangling reference: PlaylistEntryPlaylist of PlaylistEntry { playlist: None, \
         index: 1 } is 0"
    );
}

#[test]
fn a_parent_that_is_not_a_folder_is_refused_on_write() {
    use crate::EncodeError;
    let refused = EncodeError::ValueOutOfRange {
        context: "pdb playlist parent_id",
    };
    let mut db = Database::new();
    let a = db.add_playlist("A", None).unwrap();
    let before = db.clone();
    assert_eq!(db.add_playlist("B", Some(a)), Err(refused.clone()));
    assert_eq!(db.add_folder("C", Some(a)), Err(refused.clone()));
    assert_eq!(db, before, "nothing added");
    let folder = db.add_folder("F", None).unwrap();
    assert!(db.add_playlist("G", Some(folder)).is_ok());
    // A loaded playlist inside a playlist loads, isn't dangling, and isn't written.
    let nodes = vec![node_row(1, 0, 0, false, "A"), node_row(2, 1, 0, false, "B")];
    let db = load(&[(ExportTable::PlaylistTree, nodes)]);
    let parent = db.playlist(playlist_id(2)).and_then(Playlist::parent_id);
    assert_eq!(parent, PlaylistId::new(1));
    assert_eq!(db.dangling_references().count(), 0);
    assert_eq!(db.to_pdb(), Err(refused));
}

#[test]
fn empty_and_missing_tables_have_no_rows() {
    let tables = vec![
        (ExportTable::Tracks, vec![]),
        (ExportTable::Genres, vec![]),
        (ExportTable::Artists, vec![artist_row(2, "Björk")]),
    ];
    let db = load(&tables);
    assert_eq!(db.tracks().len(), 0);
    assert_eq!(db.genres().len(), 0);
    assert_eq!(db.artists().len(), 1);
    // No albums, labels, keys, colours or artwork table at all.
    assert_eq!(db.albums().len() + db.labels().len() + db.keys().len(), 0);
    assert_eq!(db.colors().len() + db.artworks().len(), 0);
}
