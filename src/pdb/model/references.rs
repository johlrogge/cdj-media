//! References between records, and the ones that don't resolve.

use std::fmt;

use super::super::{
    AlbumId, ArtistId, ArtworkId, ColorId, GenreId, KeyId, LabelId, PlaylistId, TrackId,
};
use super::{Album, Playlist, Track};

/// The id of a record of any table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum RecordId {
    /// A track.
    Track(TrackId),
    /// An artist.
    Artist(ArtistId),
    /// An album.
    Album(AlbumId),
    /// A genre.
    Genre(GenreId),
    /// A label.
    Label(LabelId),
    /// A musical key.
    Key(KeyId),
    /// A colour label.
    Color(ColorId),
    /// An artwork.
    Artwork(ArtworkId),
    /// A playlist or a folder.
    Playlist(PlaylistId),
    /// Entry `index` of a playlist's tracks: its place, from 1, as `to_pdb` writes it.
    PlaylistEntry {
        /// The playlist the entry names; `None` for an entry row whose playlist is 0.
        playlist: Option<PlaylistId>,
        /// The entry's place, from 1.
        index: u32,
    },
}

/// A reference field of a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Reference {
    /// [`Track::artist_id`].
    TrackArtist,
    /// [`Track::composer_id`].
    TrackComposer,
    /// [`Track::original_artist_id`].
    TrackOriginalArtist,
    /// [`Track::remixer_id`].
    TrackRemixer,
    /// [`Track::album_id`].
    TrackAlbum,
    /// [`Track::genre_id`].
    TrackGenre,
    /// [`Track::label_id`].
    TrackLabel,
    /// [`Track::key_id`].
    TrackKey,
    /// [`Track::color_id`].
    TrackColor,
    /// [`Track::artwork_id`].
    TrackArtwork,
    /// [`Album::artist_id`].
    AlbumArtist,
    /// [`Playlist::parent_id`].
    PlaylistParent,
    /// The track of a playlist entry.
    PlaylistEntryTrack,
    /// The playlist of a playlist entry.
    PlaylistEntryPlaylist,
}

/// A reference that names a record the database doesn't have, or a required reference
/// that is 0 (a playlist entry's track or playlist).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub struct Dangling {
    /// The record that holds the reference.
    pub from: RecordId,
    /// The reference field.
    pub field: Reference,
    /// The id it names; `None` for a reference of 0 where one is required.
    pub id: Option<RecordId>,
}

impl fmt::Display for Dangling {
    /// `TrackAlbum of Track(TrackId(3)) names a missing Album(AlbumId(7))`, or `… is 0`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} of {:?} ", self.field, self.from)?;
        match self.id {
            Some(id) => write!(f, "names a missing {id:?}"),
            None => f.write_str("is 0"),
        }
    }
}

impl Track {
    /// Every reference field, with the id it names, if set.
    pub fn references(&self) -> [(Reference, Option<RecordId>); 10] {
        use Reference as R;
        [
            (R::TrackArtist, self.artist_id().map(RecordId::Artist)),
            (R::TrackComposer, self.composer_id().map(RecordId::Artist)),
            (
                R::TrackOriginalArtist,
                self.original_artist_id().map(RecordId::Artist),
            ),
            (R::TrackRemixer, self.remixer_id().map(RecordId::Artist)),
            (R::TrackAlbum, self.album_id().map(RecordId::Album)),
            (R::TrackGenre, self.genre_id().map(RecordId::Genre)),
            (R::TrackLabel, self.label_id().map(RecordId::Label)),
            (R::TrackKey, self.key_id().map(RecordId::Key)),
            (R::TrackColor, self.color_id().map(RecordId::Color)),
            (R::TrackArtwork, self.artwork_id().map(RecordId::Artwork)),
        ]
    }
}

impl Album {
    /// Every reference field, with the id it names, if set.
    pub fn references(&self) -> [(Reference, Option<RecordId>); 1] {
        [(
            Reference::AlbumArtist,
            self.artist_id().map(RecordId::Artist),
        )]
    }
}

impl Playlist {
    /// Every reference field, with the id it names, if set.
    pub fn references(&self) -> [(Reference, Option<RecordId>); 1] {
        [(
            Reference::PlaylistParent,
            self.parent_id().map(RecordId::Playlist),
        )]
    }
}
