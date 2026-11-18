use super::*;
use crate::DecodeErrorKind;
use crate::encoding::{Bpm, StringForm};
use crate::pdb::testutil::{LEN, data_page, file, row_index_len};
use crate::pdb::{
    Album as AlbumRow, Artist as ArtistRow, Artwork as ArtworkRow, Color as ColorRow, ExportRow,
    ExportTable, FileType, Genre as GenreRow, Key as KeyRow, Label as LabelRow, Track as TrackRow,
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
            id: RecordId::Artist(ArtistId::new(99).unwrap()),
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
