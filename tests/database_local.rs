//! Loads the real `export.pdb` in `local-fixtures/` into a `Database` and reports how its
//! references resolve. Skips itself when the fixture is missing.
//!
//! Every set reference of the fixture resolves, so `Database::dangling_references` is
//! empty on it; `from_pdb` would load the stick either way.
//!
//! The database holds the user's library, so the tests print counts only, never library
//! text. `exportExt.pdb` has another schema and is not loaded here.

mod common;

use std::collections::BTreeMap;

use cdj_media::pdb::{Database, Pdb};

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
