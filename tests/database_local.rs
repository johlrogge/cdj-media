//! Loads the real `export.pdb` in `local-fixtures/` into a `Database` and reports how its
//! references resolve. Skips itself when the fixture is missing.
//!
//! Every set reference of the fixture resolves, so `Database::dangling_references` is
//! empty on it; `from_pdb` would load the stick either way.
//!
//! The database holds the user's library, so the tests print counts only, never library
//! text. `exportExt.pdb` has another schema and is not loaded here.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use cdj_media::pdb::model::{self, RecordId};
use cdj_media::pdb::{Database, ExportTable, Pdb, Track, TrackFields, TrackId};

const EXPORT: &str = "real-stick/PIONEER/rekordbox/export.pdb";

fn load() -> Option<Database> {
    let path = common::local_fixture(EXPORT)?;
    let bytes = std::fs::read(&path).expect("read export.pdb");
    let pdb = Pdb::from_bytes(&bytes).expect("decode export.pdb");
    Some(Database::from_pdb(&pdb).expect("load the database"))
}

#[test]
fn real_stick_database_loads_every_row() {
    let Some(db) = load() else {
        return;
    };
    let counts = [
        ("tracks", db.tracks().len(), 827),
        ("artists", db.artists().len(), 277),
        ("albums", db.albums().len(), 239),
        ("genres", db.genres().len(), 19),
        ("labels", db.labels().len(), 21),
        ("keys", db.keys().len(), 43),
        ("colors", db.colors().len(), 8),
        ("artwork", db.artworks().len(), 787),
    ];
    eprintln!("database records:");
    for (table, count, _) in counts {
        eprintln!("  {table}: {count}");
    }
    for (table, count, expected) in counts {
        assert_eq!(count, expected, "{table}");
    }
}

/// How often a reference is set, and how often it resolves.
#[derive(Debug, Default)]
struct Resolution {
    set: usize,
    resolved: usize,
}

impl Resolution {
    fn add(&mut self, set: bool, resolved: bool) {
        self.set += usize::from(set);
        self.resolved += usize::from(resolved);
    }
}

#[test]
fn real_stick_references_resolve() {
    let Some(db) = load() else {
        return;
    };
    let mut fields: BTreeMap<&str, Resolution> = BTreeMap::new();
    for t in db.tracks() {
        let refs = [
            (
                "track artist",
                t.artist_id().is_some(),
                db.artist_of(t).is_some(),
            ),
            (
                "track composer",
                t.composer_id().is_some(),
                db.composer_of(t).is_some(),
            ),
            (
                "track original artist",
                t.original_artist_id().is_some(),
                db.original_artist_of(t).is_some(),
            ),
            (
                "track remixer",
                t.remixer_id().is_some(),
                db.remixer_of(t).is_some(),
            ),
            (
                "track album",
                t.album_id().is_some(),
                db.album_of(t).is_some(),
            ),
            (
                "track genre",
                t.genre_id().is_some(),
                db.genre_of(t).is_some(),
            ),
            (
                "track label",
                t.label_id().is_some(),
                db.label_of(t).is_some(),
            ),
            ("track key", t.key_id().is_some(), db.key_of(t).is_some()),
            (
                "track color",
                t.color_id().is_some(),
                db.color_of(t).is_some(),
            ),
            (
                "track artwork",
                t.artwork_id().is_some(),
                db.artwork_of(t).is_some(),
            ),
        ];
        for (field, set, resolved) in refs {
            fields.entry(field).or_default().add(set, resolved);
        }
    }
    for album in db.albums() {
        fields.entry("album artist").or_default().add(
            album.artist_id().is_some(),
            db.album_artist_of(album).is_some(),
        );
    }
    eprintln!("references: set, resolved");
    for (field, r) in &fields {
        eprintln!("  {field}: {} set, {} resolve", r.set, r.resolved);
    }
    let dangling = db.dangling_references().count();
    eprintln!("dangling references: {dangling}");
    for (field, r) in &fields {
        assert_eq!(r.resolved, r.set, "{field}: every set reference resolves");
    }
    assert_eq!(dangling, 0, "no dangling references");
}

/// The acceptance test of `Database::to_pdb`: the real stick's database, written as a fresh
/// file, encoded, decoded and loaded again, equals the loaded one, with no dangling
/// reference. `to_pdb` writes the database's own colour table.
#[test]
fn real_stick_database_survives_to_pdb() {
    let Some(db) = load() else {
        return;
    };
    let pdb = db.to_pdb().expect("to_pdb");
    let bytes = pdb.to_bytes().expect("encode");
    let written = Pdb::from_bytes(&bytes).expect("the written file decodes");
    let back = Database::from_pdb(&written).expect("the written file loads");
    eprintln!(
        "to_pdb: {} pages of {} bytes, {} tracks",
        written.page_count(),
        written.len_page(),
        back.tracks().len()
    );
    assert!(
        back == db,
        "the written database differs from the loaded one"
    );
    assert_eq!(back.dangling_references().count(), 0);
}

/// Of `pairs` (stick id, rebuilt id): how many are equal; how many rebuilt ids equal the
/// stick id's rank (1 for the lowest stick id), which is what allocating in the same order
/// gives when the stick has gaps; and how many pairs there are.
fn same(pairs: &BTreeMap<u32, u32>) -> (usize, usize, usize) {
    let equal = pairs.iter().filter(|(old, new)| old == new).count();
    let in_rank = (1u32..)
        .zip(pairs.values())
        .filter(|(rank, new)| rank == *new)
        .count();
    (equal, in_rank, pairs.len())
}

/// The lowest and highest id of `ids`, and their number.
fn id_range(ids: impl Iterator<Item = u32>) -> (u32, u32, usize) {
    let ids: Vec<u32> = ids.collect();
    let (min, max) = (ids.iter().min(), ids.iter().max());
    (*min.unwrap_or(&0), *max.unwrap_or(&0), ids.len())
}

/// Rebuilds the stick's database from scratch through the find-or-insert API, in the order
/// of `tracks` (stick track ids), and pairs each stick id with the id `Database` allocated:
/// tracks, artists, albums, genres, labels, keys, artwork. For each track, in this order:
/// its artist, composer, original artist and remixer, its album's artist, its album, genre,
/// label, key and artwork, then the track.
fn rebuild_ids(db: &Database, tracks: &[TrackId]) -> [BTreeMap<u32, u32>; 7] {
    let mut new = Database::new();
    let mut ids: [BTreeMap<u32, u32>; 7] = Default::default();
    let mut pair = |table: usize, old: Option<u32>, new: Option<u32>| {
        if let (Some(old), Some(new)) = (old, new) {
            ids[table].insert(old, new);
        }
    };
    for &id in tracks {
        let t = db.track(id).expect("a stick track");
        let mut artist = |of: Option<&model::Artist>| {
            let found = of.and_then(|a| new.find_or_insert_artist(a.name()));
            pair(1, of.map(|a| a.id().get()), found.map(|a| a.get()));
            found
        };
        let artist_id = artist(db.artist_of(t));
        let composer_id = artist(db.composer_of(t));
        let original_artist_id = artist(db.original_artist_of(t));
        let remixer_id = artist(db.remixer_of(t));
        let album = db.album_of(t);
        let album_artist = artist(album.and_then(|a| db.album_artist_of(a)));
        let album_id = album.and_then(|a| new.find_or_insert_album(a.name(), album_artist));
        pair(2, album.map(|a| a.id().get()), album_id.map(|a| a.get()));
        let genre = db.genre_of(t);
        let genre_id = genre.and_then(|g| new.find_or_insert_genre(g.name()));
        pair(3, genre.map(|g| g.id().get()), genre_id.map(|g| g.get()));
        let label = db.label_of(t);
        let label_id = label.and_then(|l| new.find_or_insert_label(l.name()));
        pair(4, label.map(|l| l.id().get()), label_id.map(|l| l.get()));
        let key = db.key_of(t);
        let key_id = key.and_then(|k| new.find_or_insert_key(k.name()));
        pair(5, key.map(|k| k.id().get()), key_id.map(|k| k.get()));
        let artwork = db.artwork_of(t);
        let artwork_id = artwork.and_then(|a| new.find_or_insert_artwork(a.path()));
        pair(
            6,
            artwork.map(|a| a.id().get()),
            artwork_id.map(|a| a.get()),
        );
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
            ..TrackFields::default()
        };
        let track_id = new.add_track(fields).expect("add the track");
        pair(0, Some(id.get()), Some(track_id.get()));
    }
    ids
}

/// Survey: does rekordbox allocate ids the way `Database` does (last + 1, in the order
/// records first appear)? The stick's database is rebuilt from scratch in two track orders,
/// by track id and in page order (the order of the rows in the track table's pages), and
/// the allocated ids are compared with rekordbox's, table by table.
///
/// The stick's ids have gaps (tracks 1 to 881 for 827 tracks), so few are equal. In track
/// id order every rebuilt track, artwork, genre and label id equals the rank of
/// rekordbox's, and most artist and album ids do: rekordbox allocates in the order records
/// first appear, as `Database` does, without reusing the ids of deleted records. Key ids
/// follow some other order (none match). Page order matches nothing.
#[test]
fn real_stick_ids_against_a_rebuild() {
    let Some(db) = load() else {
        return;
    };
    let path = common::local_fixture(EXPORT).expect("the fixture");
    let pdb = Pdb::from_bytes(&std::fs::read(path).unwrap()).unwrap();
    let in_pages: Vec<TrackId> = pdb
        .table_pages(ExportTable::Tracks)
        .flat_map(|page| page.rows())
        .map(|raw| Track::decode(&raw).expect("a track row").id())
        .collect();
    let by_id: Vec<TrackId> = db.tracks().map(|t| t.id()).collect();
    let tables = [
        "tracks", "artists", "albums", "genres", "labels", "keys", "artwork",
    ];
    let ranges = [
        id_range(db.tracks().map(|r| r.id().get())),
        id_range(db.artists().map(|r| r.id().get())),
        id_range(db.albums().map(|r| r.id().get())),
        id_range(db.genres().map(|r| r.id().get())),
        id_range(db.labels().map(|r| r.id().get())),
        id_range(db.keys().map(|r| r.id().get())),
        id_range(db.artworks().map(|r| r.id().get())),
    ];
    eprintln!("stick ids: lowest, highest, records");
    for (table, range) in tables.iter().zip(ranges) {
        eprintln!("  {table}: {range:?}");
    }
    let mut results = Vec::new();
    for (order, tracks) in [("track id", &by_id), ("page", &in_pages)] {
        let ids = rebuild_ids(&db, tracks);
        let counts = ids.each_ref().map(same);
        eprintln!(
            "rebuild in {order} order: ids equal to rekordbox's, equal to their rank, records:"
        );
        for (table, (equal, in_rank, of)) in tables.iter().zip(counts) {
            eprintln!("  {table}: {equal}, {in_rank} of {of}");
        }
        results.push(counts);
    }
    assert_eq!(
        ranges,
        [
            (1, 881, 827),
            (1, 292, 277),
            (1, 251, 239),
            (1, 22, 19),
            (1, 21, 21),
            (1, 44, 43),
            (1, 835, 787),
        ]
    );
    assert_eq!(
        results,
        [
            [
                (27, 827, 827),
                (22, 262, 277),
                (17, 226, 239),
                (17, 19, 19),
                (21, 21, 21),
                (0, 0, 43),
                (27, 787, 787),
            ],
            [
                (1, 0, 827),
                (4, 5, 277),
                (7, 7, 239),
                (6, 6, 19),
                (5, 5, 21),
                (0, 1, 43),
                (6, 5, 787),
            ],
        ]
    );
}

/// The number of distinct values of `keys`.
fn distinct<T: Ord>(keys: impl Iterator<Item = T>) -> usize {
    keys.collect::<BTreeSet<T>>().len()
}

/// How a table's names repeat: rows, distinct names exactly, distinct case-folded, distinct
/// trimmed, empty names, names with surrounding whitespace.
fn name_shape<'a>(names: impl Iterator<Item = &'a str>) -> [usize; 6] {
    let names: Vec<&str> = names.collect();
    let names = || names.iter().copied();
    [
        names().count(),
        distinct(names()),
        distinct(names().map(str::to_lowercase)),
        distinct(names().map(str::trim)),
        names().filter(|n| n.is_empty()).count(),
        names().filter(|n| n.trim() != *n).count(),
    ]
}

/// The survey behind `Database`'s find-or-insert rules: how rekordbox keeps names apart in
/// each table. Counts only.
///
/// - Every table's names (artwork: paths) are unique exactly. 3 pairs of artists differ
///   only in case, so names are case-sensitive. No name has surrounding whitespace, so the
///   stick doesn't show whether rekordbox trims; no row has an empty name, while tracks
///   without an artist, album, genre, label, key or artwork reference 0.
/// - Album names are unique on their own, so the stick can't tell "by name" from "by name
///   and album artist". The album artist is its own tag: 125 tracks on albums with an album
///   artist have another track artist.
/// - Every record is named by some track (or album), and every artwork by exactly one track.
#[test]
fn real_stick_names_are_unique_per_table() {
    let Some(db) = load() else {
        return;
    };
    let shapes = [
        ("artists", name_shape(db.artists().map(|a| a.name()))),
        ("albums", name_shape(db.albums().map(|a| a.name()))),
        ("genres", name_shape(db.genres().map(|g| g.name()))),
        ("labels", name_shape(db.labels().map(|l| l.name()))),
        ("keys", name_shape(db.keys().map(|k| k.name()))),
        ("artwork", name_shape(db.artworks().map(|a| a.path()))),
    ];
    eprintln!("names: rows, distinct, distinct case-folded, distinct trimmed, empty, padded");
    for (table, shape) in &shapes {
        eprintln!("  {table}: {shape:?}");
    }
    let albums_by_artist = distinct(db.albums().map(|a| (a.name(), a.artist_id())));
    let without_artist = db.albums().filter(|a| a.artist_id().is_none()).count();
    // Albums of one name, and how many album rows each such name has.
    let mut per_name: BTreeMap<&str, usize> = BTreeMap::new();
    for album in db.albums() {
        *per_name.entry(album.name()).or_default() += 1;
    }
    let shared: Vec<usize> = per_name.values().copied().filter(|&n| n > 1).collect();
    eprintln!(
        "albums: {albums_by_artist} distinct (name, album artist), {without_artist} without an \
         album artist; names on more than one album row: {} (rows per name {shared:?})",
        shared.len()
    );
    // Whether an album's tracks share its album artist: albums whose tracks name more than
    // one track artist, and tracks whose artist differs from their album's artist.
    let mut artists_of_album: BTreeMap<u32, BTreeSet<Option<u32>>> = BTreeMap::new();
    let (mut other_artist, mut other_set_artist) = (0, 0);
    for t in db.tracks() {
        if let Some(album) = db.album_of(t) {
            artists_of_album
                .entry(album.id().get())
                .or_default()
                .insert(t.artist_id().map(|a| a.get()));
            let differs = album.artist_id() != t.artist_id();
            other_artist += usize::from(differs);
            other_set_artist += usize::from(differs && album.artist_id().is_some());
        }
    }
    let mixed_albums: Vec<u32> = artists_of_album
        .iter()
        .filter(|(_, artists)| artists.len() > 1)
        .map(|(&id, _)| id)
        .collect();
    let mixed = mixed_albums.len();
    let mixed_without = db
        .albums()
        .filter(|a| a.artist_id().is_none() && mixed_albums.contains(&a.id().get()))
        .count();
    let unused_albums = db.albums().len() - artists_of_album.len();
    eprintln!(
        "album tracks: {mixed} albums with more than one track artist ({mixed_without} of them \
         without an album artist), {other_artist} tracks whose artist is not the album's \
         ({other_set_artist} of them on albums with an album artist), {unused_albums} albums \
         no track names"
    );
    let unset = |f: fn(&model::Track) -> bool| db.tracks().filter(|t| f(t)).count();
    let no_refs = [
        unset(|t| t.artist_id().is_none()),
        unset(|t| t.album_id().is_none()),
        unset(|t| t.genre_id().is_none()),
        unset(|t| t.label_id().is_none()),
        unset(|t| t.key_id().is_none()),
        unset(|t| t.artwork_id().is_none()),
    ];
    eprintln!("tracks without artist, album, genre, label, key, artwork: {no_refs:?}");
    // Records no track (or album) names.
    let used: BTreeSet<RecordId> = db
        .tracks()
        .flat_map(|t| t.references())
        .chain(db.albums().flat_map(|a| a.references()))
        .filter_map(|(_, id)| id)
        .collect();
    let used_of = |table: fn(&RecordId) -> bool| used.iter().filter(|id| table(id)).count();
    let unused = [
        db.artists().len() - used_of(|id| matches!(id, RecordId::Artist(_))),
        db.genres().len() - used_of(|id| matches!(id, RecordId::Genre(_))),
        db.labels().len() - used_of(|id| matches!(id, RecordId::Label(_))),
        db.keys().len() - used_of(|id| matches!(id, RecordId::Key(_))),
        db.artworks().len() - used_of(|id| matches!(id, RecordId::Artwork(_))),
    ];
    eprintln!("records no track names (artists, genres, labels, keys, artwork): {unused:?}");
    assert_eq!(
        shapes.map(|(_, s)| s),
        [
            [277, 277, 274, 277, 0, 0],
            [239, 239, 239, 239, 0, 0],
            [19, 19, 19, 19, 0, 0],
            [21, 21, 21, 21, 0, 0],
            [43, 43, 43, 43, 0, 0],
            [787, 787, 787, 787, 0, 0],
        ]
    );
    assert_eq!(
        (albums_by_artist, without_artist, shared.len()),
        (239, 76, 0)
    );
    assert_eq!((mixed, mixed_without, unused_albums), (17, 1, 0));
    assert_eq!((other_artist, other_set_artist), (240, 125));
    assert_eq!((no_refs, unused), ([24, 51, 713, 800, 137, 40], [0; 5]));
}
