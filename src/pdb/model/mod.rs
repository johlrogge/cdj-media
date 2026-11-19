//! The domain model of an `export.pdb` (ADR-002, ADR-005, ADR-008, ADR-009).
//!
//! [`Database::from_pdb`] decodes every row of the track table, the small tables and the
//! playlist tables of a [`Pdb`] through the typed rows, keys them by id, and holds their
//! domain values: text as [`String`], not
//! [`DeviceSqlString`](crate::encoding::DeviceSqlString), with the references as ids. The
//! `*_of` methods of [`Database`] resolve the references. Playlists and folders are
//! [`Playlist`]s; a playlist's tracks are [`Database::playlist_tracks`], in entry order.
//!
//! A reference that names a missing record does not stop the load, as a stick a CDJ or
//! another tool modified must still load: the record keeps the raw id, its `*_of` method
//! returns `None`, and [`Database::dangling_references`] lists it.
//!
//! A database can also be built: [`Database::new`], the `find_or_insert_*` methods for
//! names, which allocate ids, [`Database::add_track`], and [`Database::add_playlist`],
//! [`Database::add_folder`] and [`Database::add_to_playlist`]. [`Database::to_pdb`] writes
//! any database as a fresh file, and refuses one with a dangling reference.
//!
//! Only for `export.pdb`: `exportExt.pdb` uses the same table types for other tables.

mod records;
mod references;

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

pub use records::{Album, Artist, Artwork, Color, Genre, Key, Label, Playlist, Track};
pub use references::{Dangling, RecordId, Reference};

use super::{
    AlbumId, ArtistId, ArtworkId, ColorId, ExportRow, ExportTable, GenreId, KeyId, LabelId, Page,
    Pdb, PlaylistId, TrackFields, TrackId, writer,
};
use crate::error::{DecodeError, DecodeErrorKind, EncodeError};
use records::FromRow;

/// The page length [`Database::to_pdb`] writes: rekordbox's, 4,096 bytes.
const PAGE_LEN: usize = 4096;

/// The tracks, small tables and playlists of an `export.pdb`, keyed by id.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    playlists: BTreeMap<PlaylistId, Playlist>,
    /// The tracks of each playlist that has entries, in entry order. Kept apart from
    /// `playlists`, so entries of a missing playlist load and are reported; `None` is an
    /// entry row's track or playlist of 0, which loads and is reported too.
    playlist_tracks: PlaylistTracks,
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

/// Defines the find-or-insert method of a table whose records are one text field.
macro_rules! find_or_insert {
    ($($(#[$doc:meta])* $name:ident($field:ident) -> $id:ty, $map:ident of $model:ident;)*) => {
        $(
            $(#[$doc])*
            ///
            /// The text must equal the stored one exactly: case and whitespace count. An empty
            /// text is no record: `None`, a reference of 0 in the row. A new record gets the
            /// table's last id + 1, or 1 in an empty table.
            pub fn $name(&mut self, $field: &str) -> Option<$id> {
                if $field.is_empty() {
                    return None;
                }
                Some(find_or_insert(
                    &mut self.$map,
                    |record| record.$field() == $field,
                    |id| $model::new(id, $field),
                ))
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

impl Default for Database {
    /// [`Database::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl Database {
    /// A database with no tracks and no names. Its colour table holds rekordbox's eight
    /// default colours, ids 1 to 8 (pink, red, orange, yellow, green, aqua, blue and
    /// purple), which tracks reference by id; [`to_pdb`](Self::to_pdb) writes them as held.
    pub fn new() -> Self {
        let colors = writer::default_colors()
            .into_iter()
            .filter_map(|row| match row {
                ExportRow::Color(row) => Some(Color::new(row.id(), row.name().as_str())),
                _ => None,
            })
            .map(|color| (color.id(), color))
            .collect();
        Self {
            tracks: BTreeMap::new(),
            artists: BTreeMap::new(),
            albums: BTreeMap::new(),
            genres: BTreeMap::new(),
            labels: BTreeMap::new(),
            keys: BTreeMap::new(),
            colors,
            artworks: BTreeMap::new(),
            playlists: BTreeMap::new(),
            playlist_tracks: BTreeMap::new(),
        }
    }

    /// The tracks of playlist `id`, in order; empty for a folder, an empty playlist or an
    /// id the database doesn't have. A loaded entry of track 0 is skipped here and
    /// reported by [`dangling_references`](Self::dangling_references).
    pub fn playlist_tracks(&self, id: PlaylistId) -> impl Iterator<Item = TrackId> + '_ {
        self.entries_of(Some(id)).iter().flatten().copied()
    }

    /// The entries of `playlist` as held, 0 references as `None`.
    fn entries_of(&self, playlist: Option<PlaylistId>) -> &[Option<TrackId>] {
        self.playlist_tracks
            .get(&playlist)
            .map_or(&[], Vec::as_slice)
    }

    /// Adds an empty playlist named `name` to the folder `parent`, or to the root for
    /// `None`, and returns its id: the last playlist or folder id + 1, or 1 for the first.
    /// It goes after its parent's other children: its sort order is their last + 1, or 0
    /// for the first child, as in rekordbox's fresh export. Names are not deduplicated.
    /// A missing `parent` is checked by [`to_pdb`](Self::to_pdb), like a track's
    /// references.
    ///
    /// # Errors
    ///
    /// The database is unchanged on error:
    /// - [`EncodeError::ValueOutOfRange`] (`"pdb playlist parent_id"`) when `parent` is a
    ///   playlist, not a folder;
    /// - [`EncodeError::LengthOverflow`] for a name too long for a row.
    pub fn add_playlist(
        &mut self,
        name: &str,
        parent: Option<PlaylistId>,
    ) -> Result<PlaylistId, EncodeError> {
        self.add_tree_node(name, parent, false)
    }

    /// Adds an empty folder, which holds playlists and folders, as
    /// [`add_playlist`](Self::add_playlist) adds a playlist; the ids and sort orders of
    /// playlists and folders are one sequence.
    ///
    /// # Errors
    ///
    /// As for [`add_playlist`](Self::add_playlist).
    pub fn add_folder(
        &mut self,
        name: &str,
        parent: Option<PlaylistId>,
    ) -> Result<PlaylistId, EncodeError> {
        self.add_tree_node(name, parent, true)
    }

    /// Appends `track` to the tracks of `playlist`. Both are checked by
    /// [`to_pdb`](Self::to_pdb), which writes the tracks as entries 1, 2, … in order.
    pub fn add_to_playlist(&mut self, playlist: PlaylistId, track: TrackId) {
        let entries = self.playlist_tracks.entry(Some(playlist)).or_default();
        entries.push(Some(track));
    }

    fn add_tree_node(
        &mut self,
        name: &str,
        parent: Option<PlaylistId>,
        is_folder: bool,
    ) -> Result<PlaylistId, EncodeError> {
        if self.is_not_a_folder(parent) {
            return Err(parent_not_a_folder());
        }
        let id = next_id(&self.playlists);
        let sort_order = self
            .playlists
            .values()
            .filter(|p| p.parent_id() == parent)
            .map(|p| p.sort_order().saturating_add(1))
            .max()
            .unwrap_or(0);
        let playlist = Playlist::new(id, parent, sort_order, is_folder, name);
        playlist.to_row().encode()?;
        self.playlists.insert(id, playlist);
        Ok(id)
    }

    /// Whether `parent` names a playlist the database has that is not a folder. A missing
    /// parent is a dangling reference instead, and the root is no parent.
    fn is_not_a_folder(&self, parent: Option<PlaylistId>) -> bool {
        parent
            .and_then(|id| self.playlists.get(&id))
            .is_some_and(|parent| !parent.is_folder())
    }

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

    /// Decodes the track table, the small tables and the playlist tables of an
    /// `export.pdb`: tracks, artists, albums, genres, labels, keys, colours, artwork, the
    /// playlist tree and the playlist entries, each playlist's tracks in entry index order.
    /// A table the file doesn't have is empty. References are not checked; see
    /// [`dangling_references`](Self::dangling_references).
    ///
    /// # Errors
    ///
    /// A [`DecodeError`] at an absolute offset:
    /// - the typed rows' errors;
    /// - [`DuplicateId`](DecodeErrorKind::DuplicateId) at the id field of a present row
    ///   whose id an earlier present row of its table has, and at the entry index of a
    ///   playlist entry whose playlist and index an earlier present entry has.
    ///
    /// Reading is lenient (ADR-009): a playlist entry of track or playlist 0 loads and is
    /// reported by [`dangling_references`](Self::dangling_references), and a playlist or
    /// folder inside a playlist loads; [`to_pdb`](Self::to_pdb) refuses both.
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
            playlists: load(pdb)?,
            playlist_tracks: load_playlist_tracks(pdb)?,
        })
    }

    find_or_insert! {
        /// The id of the artist named `name`, inserted if the database has none.
        find_or_insert_artist(name) -> ArtistId, artists of Artist;
        /// The id of the genre named `name`, inserted if the database has none.
        find_or_insert_genre(name) -> GenreId, genres of Genre;
        /// The id of the label named `name`, inserted if the database has none.
        find_or_insert_label(name) -> LabelId, labels of Label;
        /// The id of the musical key named `name` (for example `"8A"` or `"Am"`), inserted
        /// if the database has none.
        find_or_insert_key(name) -> KeyId, keys of Key;
        /// The id of the artwork at `path` on the stick, inserted if the database has none.
        find_or_insert_artwork(path) -> ArtworkId, artworks of Artwork;
    }

    /// The id of the album named `name` with album artist `artist_id`, inserted if the
    /// database has none: albums of one name with different album artists, or one with and
    /// one without, are different albums.
    ///
    /// The name must equal the stored one exactly: case and whitespace count. An empty name
    /// is no album: `None`, a reference of 0 in the row. A new album gets the last album
    /// id + 1, or 1 for the first.
    pub fn find_or_insert_album(
        &mut self,
        name: &str,
        artist_id: Option<ArtistId>,
    ) -> Option<AlbumId> {
        if name.is_empty() {
            return None;
        }
        Some(find_or_insert(
            &mut self.albums,
            |album| album.name() == name && album.artist_id() == artist_id,
            |id| Album::new(id, name, artist_id),
        ))
    }

    /// Adds a track of `fields` and returns its id: the last track id + 1, or 1 for the
    /// first. The fields' references are ids of this database, typically from the
    /// `find_or_insert_*` methods, and colours 1 to 8 (see [`new`](Self::new)); they are
    /// checked by [`to_pdb`](Self::to_pdb). Empty ISRC and dates become `None`.
    ///
    /// # Errors
    ///
    /// The errors of [`Track::new`](super::Track::new), when the fields can't be written:
    /// [`EncodeError::ValueOutOfRange`] for an ISRC that is not ASCII without NULs or a
    /// colour id that does not fit a byte. The database is unchanged.
    pub fn add_track(&mut self, fields: TrackFields) -> Result<TrackId, EncodeError> {
        let id = next_id(&self.tracks);
        let row = super::Track::new(id, fields)?;
        self.tracks.insert(id, Track::from_row(&row));
        Ok(id)
    }

    /// Writes the database as a fresh `export.pdb` with 4,096-byte pages, as rekordbox lays
    /// one out (ADR-012): every record lowered to its typed row with the ids as they are,
    /// each table in id order, colours included, so renamed colours keep their names. A
    /// database without colours gets rekordbox's defaults, as `write_pdb` does. Playlists
    /// and folders are written in id order, then each playlist's tracks as entries 1, 2, …,
    /// by playlist id. The history tables are empty.
    ///
    /// # Errors
    ///
    /// - [`EncodeError::DanglingReference`] for the first of the
    ///   [`dangling_references`](Self::dangling_references): a written file has none;
    /// - [`EncodeError::ValueOutOfRange`] (`"pdb playlist parent_id"`) for a playlist or
    ///   folder whose parent is a playlist, which a loaded file may hold;
    /// - the errors of the typed rows and the page writer, for example
    ///   [`EncodeError::LengthOverflow`] for a row too long for a page.
    pub fn to_pdb(&self) -> Result<Pdb, EncodeError> {
        if let Some(dangling) = self.dangling_references().next() {
            return Err(EncodeError::DanglingReference(dangling));
        }
        let playlists = self.playlists.values();
        if playlists
            .clone()
            .any(|p| self.is_not_a_folder(p.parent_id()))
        {
            return Err(parent_not_a_folder());
        }
        let mut rows = Vec::new();
        for track in self.tracks.values() {
            let row = super::Track::new(track.id(), track.to_fields())?;
            rows.push(ExportRow::Track(row));
        }
        for a in self.artists.values() {
            rows.push(ExportRow::Artist(super::Artist::new(a.id(), a.name())));
        }
        for a in self.albums.values() {
            let row = super::Album::new(a.id(), a.name(), a.artist_id());
            rows.push(ExportRow::Album(row));
        }
        for g in self.genres.values() {
            rows.push(ExportRow::Genre(super::Genre::new(g.id(), g.name())));
        }
        for l in self.labels.values() {
            rows.push(ExportRow::Label(super::Label::new(l.id(), l.name())));
        }
        for k in self.keys.values() {
            rows.push(ExportRow::Key(super::Key::new(k.id(), k.name())));
        }
        for c in self.colors.values() {
            rows.push(ExportRow::Color(super::Color::new(c.id(), c.name())));
        }
        for a in self.artworks.values() {
            rows.push(ExportRow::Artwork(super::Artwork::new(a.id(), a.path())));
        }
        for p in self.playlists.values() {
            rows.push(ExportRow::PlaylistNode(p.to_row()));
        }
        for (&playlist, tracks) in &self.playlist_tracks {
            // Entries of 0 are dangling references, refused above.
            let Some(playlist) = playlist else {
                continue;
            };
            for (index, &track) in (1u32..).zip(tracks.iter().flatten()) {
                let row = super::PlaylistEntry::new(index, track, playlist);
                rows.push(ExportRow::PlaylistEntry(row));
            }
        }
        writer::write_pdb(PAGE_LEN, rows)
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
            RecordId::Playlist(id) => self.playlists.contains_key(&id),
            RecordId::PlaylistEntry { playlist, index } => usize::try_from(index)
                .ok()
                .and_then(|index| index.checked_sub(1))
                .is_some_and(|at| at < self.entries_of(playlist).len()),
        }
    }

    /// The references that name a record the database doesn't have, computed on demand:
    /// tracks first, then albums, then playlists and folders (their parent), each in id
    /// order and, within a record, in the order of its `references`; then the playlist
    /// entries, by playlist id (0 first) and place, each entry's track before its playlist.
    /// An entry's track or playlist of 0 is reported too, with no id: an entry must name
    /// both.
    pub fn dangling_references(&self) -> impl Iterator<Item = Dangling> + '_ {
        let tracks = self.tracks.values().flat_map(|t| {
            let from = RecordId::Track(t.id());
            t.references().into_iter().map(move |r| (from, r))
        });
        let albums = self.albums.values().flat_map(|a| {
            let from = RecordId::Album(a.id());
            a.references().into_iter().map(move |r| (from, r))
        });
        let playlists = self.playlists.values().flat_map(|p| {
            let from = RecordId::Playlist(p.id());
            p.references().into_iter().map(move |r| (from, r))
        });
        // A record's reference that is not set is no reference.
        let records = tracks
            .chain(albums)
            .chain(playlists)
            .filter_map(|(from, r)| {
                let (field, id) = (r.0, Some(r.1?));
                Some(Dangling { from, field, id })
            });
        let entries = self.playlist_tracks.iter().flat_map(|(&playlist, tracks)| {
            (1u32..).zip(tracks).flat_map(move |(index, &track)| {
                use Reference as R;
                let from = RecordId::PlaylistEntry { playlist, index };
                let track = (R::PlaylistEntryTrack, track.map(RecordId::Track));
                let list = (R::PlaylistEntryPlaylist, playlist.map(RecordId::Playlist));
                [track, list].map(|(field, id)| Dangling { from, field, id })
            })
        });
        records
            .chain(entries)
            .filter(|dangling| !dangling.id.is_some_and(|id| self.contains(id)))
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
        playlists, playlist(PlaylistId) -> Playlist;
    }
}

/// An id the database allocates: a non-zero `u32`.
trait Allocated: Ord + Copy {
    /// The id of raw value `raw`; `None` for 0.
    fn from_raw(raw: u32) -> Option<Self>;
    /// The raw value.
    fn raw(self) -> u32;
}

macro_rules! allocated {
    ($($id:ty),*) => {
        $(
            impl Allocated for $id {
                fn from_raw(raw: u32) -> Option<Self> {
                    Self::new(raw)
                }

                fn raw(self) -> u32 {
                    self.get()
                }
            }
        )*
    };
}

allocated! {
    TrackId, ArtistId, AlbumId, GenreId, LabelId, KeyId, ArtworkId, PlaylistId
}

/// The id of the first record of `map` that `matches`, or of a new record `make` builds
/// with the [`next_id`], inserted.
fn find_or_insert<I: Allocated, M>(
    map: &mut BTreeMap<I, M>,
    matches: impl Fn(&M) -> bool,
    make: impl FnOnce(I) -> M,
) -> I {
    if let Some((&id, _)) = map.iter().find(|(_, record)| matches(record)) {
        return id;
    }
    let id = next_id(map);
    map.insert(id, make(id));
    id
}

/// The id a new record of `map` gets: the last id + 1, or 1 when `map` is empty. After
/// `u32::MAX`, which only a loaded file can hold, the lowest id `map` doesn't have.
///
/// # Panics
///
/// When `map` holds every id, `u32::MAX` records, far more than fit in memory.
fn next_id<I: Allocated, M>(map: &BTreeMap<I, M>) -> I {
    let last = map.keys().next_back().map_or(0, |id| id.raw());
    last.checked_add(1)
        .and_then(I::from_raw)
        .or_else(|| {
            (1..=u32::MAX)
                .filter_map(I::from_raw)
                .find(|id| !map.contains_key(id))
        })
        .expect("a free id: fewer than u32::MAX records")
}

/// The error for a playlist or folder whose parent is a playlist: only folders hold
/// playlists and folders.
fn parent_not_a_folder() -> EncodeError {
    EncodeError::ValueOutOfRange {
        context: "pdb playlist parent_id",
    }
}

/// The tracks of each playlist, as [`Database`] holds them.
type PlaylistTracks = BTreeMap<Option<PlaylistId>, Vec<Option<TrackId>>>;

/// The tracks of every playlist named by a present playlist entry row, in entry index
/// order. A track or playlist of 0 loads as `None` (ADR-009: reading is lenient) and is a
/// dangling reference.
///
/// # Errors
///
/// The entry rows' errors, and [`DecodeErrorKind::DuplicateId`] at the entry index of a row
/// whose playlist and index an earlier present row has, at its absolute offset.
fn load_playlist_tracks(pdb: &Pdb) -> Result<PlaylistTracks, DecodeError> {
    let mut by_playlist: BTreeMap<Option<PlaylistId>, BTreeMap<u32, Option<TrackId>>> =
        BTreeMap::new();
    for raw in pdb
        .table_pages(ExportTable::PlaylistEntries)
        .flat_map(Page::rows)
    {
        let row = super::PlaylistEntry::decode(&raw)?;
        let index = row.entry_index();
        match by_playlist
            .entry(row.playlist_id())
            .or_default()
            .entry(index)
        {
            Entry::Vacant(slot) => {
                slot.insert(row.track_id());
            }
            Entry::Occupied(_) => {
                return Err(DecodeError::new(
                    raw.file_offset(),
                    "pdb playlist entry index",
                    DecodeErrorKind::DuplicateId { id: index },
                ));
            }
        }
    }
    Ok(by_playlist
        .into_iter()
        .map(|(playlist, entries)| (playlist, entries.into_values().collect()))
        .collect())
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
