//! References between records, and the ones that don't resolve.

use super::super::{AlbumId, ArtistId, ArtworkId, ColorId, GenreId, KeyId, LabelId, TrackId};
use super::{Album, Track};

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
}

/// A reference that names a record the database doesn't have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub struct Dangling {
    /// The record that holds the reference.
    pub from: RecordId,
    /// The reference field.
    pub field: Reference,
    /// The id it names.
    pub id: RecordId,
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
