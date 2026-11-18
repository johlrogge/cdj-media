//! The read-only domain model of an `export.pdb` (ADR-002, ADR-005, ADR-008).
//!
//! [`Database::from_pdb`] decodes every row of the track table and the small tables of a
//! [`Pdb`] through the typed rows, keys them by id, and holds their domain values: text
//! as [`String`], not [`DeviceSqlString`](crate::encoding::DeviceSqlString), with the
//! references as ids. The `*_of` methods of [`Database`] resolve the references.
//!
//! A reference that names a missing record does not stop the load, as a stick a CDJ or
//! another tool modified must still load: the record keeps the raw id, its `*_of` method
//! returns `None`, and [`Database::dangling_references`] lists it.
//!
//! Only for `export.pdb`: `exportExt.pdb` uses the same table types for other tables.

mod records;
mod references;

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

pub use records::{Album, Artist, Artwork, Color, Genre, Key, Label, Track};
pub use references::{Dangling, RecordId, Reference};

use super::{AlbumId, ArtistId, ArtworkId, ColorId, GenreId, KeyId, LabelId, Page, Pdb, TrackId};
use crate::error::{DecodeError, DecodeErrorKind};
use records::FromRow;

/// The tracks and small tables of an `export.pdb`, keyed by id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Database {
    tracks: BTreeMap<TrackId, Track>,
    artists: BTreeMap<ArtistId, Artist>,
    albums: BTreeMap<AlbumId, Album>,
    genres: BTreeMap<GenreId, Genre>,
    labels: BTreeMap<LabelId, Label>,
    keys: BTreeMap<KeyId, Key>,
    colors: BTreeMap<ColorId, Color>,
    artworks: BTreeMap<ArtworkId, Artwork>,
}

/// Defines the iterator and lookup by id of one table.
macro_rules! table_access {
    ($($all:ident, $one:ident($id:ty) -> $model:ty;)*) => {
        $(
            #[doc = concat!("Every [`", stringify!($model), "`], in id order.")]
            pub fn $all(&self) -> impl ExactSizeIterator<Item = &$model> + '_ {
                self.$all.values()
            }

            #[doc = concat!("The [`", stringify!($model), "`] with id `id`.")]
            pub fn $one(&self, id: $id) -> Option<&$model> {
                self.$all.get(&id)
            }
        )*
    };
}

/// Defines the methods that resolve a reference of a record.
macro_rules! resolve {
    ($($(#[$doc:meta])* $name:ident($of:ident: $from:ty) -> $model:ty, $map:ident by $field:ident;)*) => {
        $(
            $(#[$doc])*
            ///
            /// `None` when the reference is not set, or dangling: it names a record the
            /// database doesn't have (see [`dangling_references`](Self::dangling_references)).
            pub fn $name(&self, $of: &$from) -> Option<&$model> {
                self.$map.get(&$of.$field()?)
            }
        )*
    };
}

impl Database {
    resolve! {
        /// The track's artist.
        artist_of(track: Track) -> Artist, artists by artist_id;
        /// The track's composer.
        composer_of(track: Track) -> Artist, artists by composer_id;
        /// The track's original artist.
        original_artist_of(track: Track) -> Artist, artists by original_artist_id;
        /// The track's remixer.
        remixer_of(track: Track) -> Artist, artists by remixer_id;
        /// The track's album.
        album_of(track: Track) -> Album, albums by album_id;
        /// The track's genre.
        genre_of(track: Track) -> Genre, genres by genre_id;
        /// The track's label.
        label_of(track: Track) -> Label, labels by label_id;
        /// The track's musical key.
        key_of(track: Track) -> Key, keys by key_id;
        /// The track's colour label.
        color_of(track: Track) -> Color, colors by color_id;
        /// The track's artwork.
        artwork_of(track: Track) -> Artwork, artworks by artwork_id;
        /// The album's artist.
        album_artist_of(album: Album) -> Artist, artists by artist_id;
    }

    /// Decodes the track table and the small tables of an `export.pdb`: tracks, artists,
    /// albums, genres, labels, keys, colours and artwork. A table the file doesn't have is
    /// empty. References are not checked; see
    /// [`dangling_references`](Self::dangling_references).
    ///
    /// # Errors
    ///
    /// A [`DecodeError`] at an absolute offset:
    /// - the typed rows' errors;
    /// - [`DuplicateId`](DecodeErrorKind::DuplicateId) at the id field of a present row
    ///   whose id an earlier present row of its table has.
    pub fn from_pdb(pdb: &Pdb) -> Result<Self, DecodeError> {
        Ok(Self {
            tracks: load(pdb)?,
            artists: load(pdb)?,
            albums: load(pdb)?,
            genres: load(pdb)?,
            labels: load(pdb)?,
            keys: load(pdb)?,
            colors: load(pdb)?,
            artworks: load(pdb)?,
        })
    }

    /// Whether the database has the record `id`.
    pub fn contains(&self, id: RecordId) -> bool {
        match id {
            RecordId::Track(id) => self.tracks.contains_key(&id),
            RecordId::Artist(id) => self.artists.contains_key(&id),
            RecordId::Album(id) => self.albums.contains_key(&id),
            RecordId::Genre(id) => self.genres.contains_key(&id),
            RecordId::Label(id) => self.labels.contains_key(&id),
            RecordId::Key(id) => self.keys.contains_key(&id),
            RecordId::Color(id) => self.colors.contains_key(&id),
            RecordId::Artwork(id) => self.artworks.contains_key(&id),
        }
    }

    /// The references that name a record the database doesn't have, computed on demand:
    /// tracks first, then albums, each in id order and, within a record, in the order of
    /// its `references`.
    pub fn dangling_references(&self) -> impl Iterator<Item = Dangling> + '_ {
        let tracks = self.tracks.values().flat_map(|t| {
            let from = RecordId::Track(t.id());
            t.references().into_iter().map(move |r| (from, r))
        });
        let albums = self.albums.values().flat_map(|a| {
            let from = RecordId::Album(a.id());
            a.references().into_iter().map(move |r| (from, r))
        });
        tracks
            .chain(albums)
            .filter_map(|(from, (field, id))| {
                Some(Dangling {
                    from,
                    field,
                    id: id?,
                })
            })
            .filter(|dangling| !self.contains(dangling.id))
    }

    table_access! {
        tracks, track(TrackId) -> Track;
        artists, artist(ArtistId) -> Artist;
        albums, album(AlbumId) -> Album;
        genres, genre(GenreId) -> Genre;
        labels, label(LabelId) -> Label;
        keys, key(KeyId) -> Key;
        colors, color(ColorId) -> Color;
        artworks, artwork(ArtworkId) -> Artwork;
    }
}

/// Every present row of `M`'s table, keyed by id.
fn load<M: FromRow>(pdb: &Pdb) -> Result<BTreeMap<M::Id, M>, DecodeError> {
    let mut records = BTreeMap::new();
    for raw in pdb.table_pages(M::TABLE).flat_map(Page::rows) {
        let record = M::decode(&raw)?;
        match records.entry(record.id()) {
            Entry::Vacant(slot) => {
                slot.insert(record);
            }
            Entry::Occupied(_) => {
                let (id_at, context) = M::ID_FIELD;
                return Err(DecodeError::new(
                    raw.file_offset().saturating_add(id_at),
                    context,
                    DecodeErrorKind::DuplicateId {
                        id: record.id().into(),
                    },
                ));
            }
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests;
