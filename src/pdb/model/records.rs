//! The model's records: the domain values of the typed rows, text as `String`.

use super::super::{
    AlbumId, ArtistId, ArtworkId, ColorId, ExportTable, FileType, GenreId, KeyId, LabelId, RawRow,
    TrackFields, TrackId, rows,
};
use crate::encoding::{Bpm, DeviceSqlString};
use crate::error::DecodeError;

/// A record decoded from a row of one table.
pub(super) trait FromRow: Sized {
    /// The id type of the table.
    type Id: Ord + Copy + Into<u32>;
    /// The table the rows are in.
    const TABLE: ExportTable;
    /// The row offset of the id field, and the error context of the id.
    const ID_FIELD: (u64, &'static str);

    /// Decodes the row through its typed row.
    fn decode(raw: &RawRow<'_>) -> Result<Self, DecodeError>;

    /// The record's id.
    fn id(&self) -> Self::Id;
}

/// Defines getters that return a copy of a field.
macro_rules! copy_getters {
    ($($(#[$doc:meta])* $name:ident: $ty:ty),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $name(&self) -> $ty {
                self.$name
            }
        )*
    };
}

/// Defines getters that return a `String` field as `&str`.
macro_rules! str_getters {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $name(&self) -> &str {
                &self.$name
            }
        )*
    };
}

/// Defines getters that return an `Option<String>` field as `Option<&str>`.
macro_rules! opt_str_getters {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$doc])*
            pub fn $name(&self) -> Option<&str> {
                self.$name.as_deref()
            }
        )*
    };
}

/// A DeviceSQL string's text.
fn text(string: &DeviceSqlString) -> String {
    string.as_str().to_owned()
}

/// A DeviceSQL string's text, or `None` when it is empty.
fn non_empty(string: &DeviceSqlString) -> Option<String> {
    Some(text(string)).filter(|s| !s.is_empty())
}

/// A track: the domain values of a track row.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Track {
    id: TrackId,
    title: String,
    artist_id: Option<ArtistId>,
    composer_id: Option<ArtistId>,
    original_artist_id: Option<ArtistId>,
    remixer_id: Option<ArtistId>,
    album_id: Option<AlbumId>,
    genre_id: Option<GenreId>,
    label_id: Option<LabelId>,
    key_id: Option<KeyId>,
    color_id: Option<ColorId>,
    artwork_id: Option<ArtworkId>,
    file_path: String,
    filename: String,
    analysis_path: String,
    comment: String,
    mix_name: String,
    lyricist: String,
    message: String,
    isrc: Option<String>,
    date_added: Option<String>,
    release_date: Option<String>,
    analysis_date: Option<String>,
    publish_track_info: bool,
    autoload_hot_cues: bool,
    tempo: Bpm,
    duration: u16,
    bitrate: u32,
    sample_rate: u32,
    sample_depth: u16,
    file_size: u32,
    track_number: u32,
    disc_number: u16,
    year: u16,
    rating: u8,
    play_count: u16,
    file_type: FileType,
}

impl Track {
    copy_getters! {
        /// The track's id.
        id: TrackId,
        /// The artist, if any.
        artist_id: Option<ArtistId>,
        /// The composer, if any.
        composer_id: Option<ArtistId>,
        /// The original artist, if any.
        original_artist_id: Option<ArtistId>,
        /// The remixer, if any.
        remixer_id: Option<ArtistId>,
        /// The album, if any.
        album_id: Option<AlbumId>,
        /// The genre, if any.
        genre_id: Option<GenreId>,
        /// The label, if any.
        label_id: Option<LabelId>,
        /// The musical key, if any.
        key_id: Option<KeyId>,
        /// The colour label, if any.
        color_id: Option<ColorId>,
        /// The artwork, if any.
        artwork_id: Option<ArtworkId>,
        /// Whether publish track info is on.
        publish_track_info: bool,
        /// Whether autoload hot cues is on.
        autoload_hot_cues: bool,
        /// The tempo.
        tempo: Bpm,
        /// The duration in seconds.
        duration: u16,
        /// The bitrate in kbit/s.
        bitrate: u32,
        /// The sample rate in Hz.
        sample_rate: u32,
        /// The sample depth in bits.
        sample_depth: u16,
        /// The audio file's size in bytes.
        file_size: u32,
        /// The track number.
        track_number: u32,
        /// The disc number.
        disc_number: u16,
        /// The year.
        year: u16,
        /// The rating, 0 to 5 in rekordbox's rows.
        rating: u8,
        /// The play count.
        play_count: u16,
        /// The audio file type.
        file_type: FileType,
    }

    str_getters! {
        /// The title.
        title,
        /// The audio file's path on the stick.
        file_path,
        /// The audio file's name.
        filename,
        /// The path of the analysis file on the stick (track string slot 14).
        analysis_path,
        /// The comment.
        comment,
        /// The mix name.
        mix_name,
        /// The lyricist.
        lyricist,
        /// The message.
        message,
    }

    opt_str_getters! {
        /// The ISRC; `None` when empty.
        isrc,
        /// The date added, `YYYY-MM-DD` as stored; `None` when empty.
        date_added,
        /// The release date as stored; `None` when empty.
        release_date,
        /// The analysis date, `YYYY-MM-DD` as stored; `None` when empty.
        analysis_date,
    }

    /// The track's values as the fields of a track row, the ids unchanged: the lowering of
    /// [`Database::to_pdb`](super::Database::to_pdb). `None` text is empty.
    pub(crate) fn to_fields(&self) -> TrackFields {
        let text = |value: &Option<String>| value.clone().unwrap_or_default();
        TrackFields {
            sample_rate: self.sample_rate,
            composer_id: self.composer_id,
            file_size: self.file_size,
            artwork_id: self.artwork_id,
            key_id: self.key_id,
            original_artist_id: self.original_artist_id,
            label_id: self.label_id,
            remixer_id: self.remixer_id,
            bitrate: self.bitrate,
            track_number: self.track_number,
            tempo: self.tempo,
            genre_id: self.genre_id,
            album_id: self.album_id,
            artist_id: self.artist_id,
            disc_number: self.disc_number,
            play_count: self.play_count,
            year: self.year,
            sample_depth: self.sample_depth,
            duration: self.duration,
            color_id: self.color_id,
            rating: self.rating,
            file_type: self.file_type,
            isrc: text(&self.isrc),
            lyricist: self.lyricist.clone(),
            message: self.message.clone(),
            publish_track_info: self.publish_track_info,
            autoload_hot_cues: self.autoload_hot_cues,
            date_added: text(&self.date_added),
            release_date: text(&self.release_date),
            mix_name: self.mix_name.clone(),
            analysis_path: self.analysis_path.clone(),
            analysis_date: text(&self.analysis_date),
            comment: self.comment.clone(),
            title: self.title.clone(),
            filename: self.filename.clone(),
            file_path: self.file_path.clone(),
        }
    }
}

impl FromRow for Track {
    type Id = TrackId;
    const TABLE: ExportTable = ExportTable::Tracks;
    const ID_FIELD: (u64, &'static str) = (0x48, "pdb track id");

    fn decode(raw: &RawRow<'_>) -> Result<Self, DecodeError> {
        Ok(Self::from_row(&rows::Track::decode(raw)?))
    }

    fn id(&self) -> TrackId {
        self.id
    }
}

impl Track {
    /// The domain values of a track row.
    pub(super) fn from_row(row: &rows::Track) -> Self {
        Self {
            id: row.id(),
            title: text(row.title()),
            artist_id: row.artist_id(),
            composer_id: row.composer_id(),
            original_artist_id: row.original_artist_id(),
            remixer_id: row.remixer_id(),
            album_id: row.album_id(),
            genre_id: row.genre_id(),
            label_id: row.label_id(),
            key_id: row.key_id(),
            color_id: row.color_id(),
            artwork_id: row.artwork_id(),
            file_path: text(row.file_path()),
            filename: text(row.filename()),
            analysis_path: text(row.analysis_path()),
            comment: text(row.comment()),
            mix_name: text(row.mix_name()),
            lyricist: text(row.lyricist()),
            message: text(row.message()),
            isrc: non_empty(row.isrc()),
            date_added: non_empty(row.date_added()),
            release_date: non_empty(row.release_date()),
            analysis_date: non_empty(row.analysis_date()),
            publish_track_info: row.is_publish_track_info_on(),
            autoload_hot_cues: row.is_autoload_hot_cues_on(),
            tempo: row.tempo(),
            duration: row.duration(),
            bitrate: row.bitrate(),
            sample_rate: row.sample_rate(),
            sample_depth: row.sample_depth(),
            file_size: row.file_size(),
            track_number: row.track_number(),
            disc_number: row.disc_number(),
            year: row.year(),
            rating: row.rating(),
            play_count: row.play_count(),
            file_type: row.file_type(),
        }
    }
}

/// An album.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Album {
    id: AlbumId,
    name: String,
    artist_id: Option<ArtistId>,
}

impl Album {
    /// The album with id `id`.
    pub(super) fn new(id: AlbumId, name: &str, artist_id: Option<ArtistId>) -> Self {
        Self {
            id,
            name: name.to_owned(),
            artist_id,
        }
    }

    copy_getters! {
        /// The album's id.
        id: AlbumId,
        /// The album artist, if any.
        artist_id: Option<ArtistId>,
    }
    str_getters! {
        /// The name.
        name,
    }
}

impl FromRow for Album {
    type Id = AlbumId;
    const TABLE: ExportTable = ExportTable::Albums;
    const ID_FIELD: (u64, &'static str) = (0x0c, "pdb album id");

    fn decode(raw: &RawRow<'_>) -> Result<Self, DecodeError> {
        let row = rows::Album::decode(raw)?;
        Ok(Self {
            id: row.id(),
            name: text(row.name()),
            artist_id: row.artist_id(),
        })
    }

    fn id(&self) -> AlbumId {
        self.id
    }
}

/// Defines a record of an id and one text field, decoded from a row with the same
/// accessor.
macro_rules! id_text_record {
    (
        $(#[$doc:meta])*
        $name:ident($id:ident, $field:ident) from $row:ident in $table:ident,
        id at $id_at:literal, $id_context:literal,
        $field_doc:literal
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        #[non_exhaustive]
        pub struct $name {
            id: $id,
            $field: String,
        }

        impl $name {
            /// The record with id `id`.
            pub(super) fn new(id: $id, $field: &str) -> Self {
                Self {
                    id,
                    $field: $field.to_owned(),
                }
            }

            copy_getters! {
                /// The id.
                id: $id,
            }
            str_getters! {
                #[doc = $field_doc]
                $field,
            }
        }

        impl FromRow for $name {
            type Id = $id;
            const TABLE: ExportTable = ExportTable::$table;
            const ID_FIELD: (u64, &'static str) = ($id_at, $id_context);

            fn decode(raw: &RawRow<'_>) -> Result<Self, DecodeError> {
                let row = rows::$row::decode(raw)?;
                Ok(Self {
                    id: row.id(),
                    $field: text(row.$field()),
                })
            }

            fn id(&self) -> $id {
                self.id
            }
        }
    };
}

id_text_record! {
    /// An artist.
    Artist(ArtistId, name) from Artist in Artists,
    id at 4, "pdb artist id",
    "The name."
}

id_text_record! {
    /// A genre.
    Genre(GenreId, name) from Genre in Genres,
    id at 0, "pdb genre id",
    "The name."
}

id_text_record! {
    /// A label.
    Label(LabelId, name) from Label in Labels,
    id at 0, "pdb label id",
    "The name."
}

id_text_record! {
    /// A musical key.
    Key(KeyId, name) from Key in Keys,
    id at 0, "pdb key id",
    "The name, e.g. `\"8A\"` or `\"Am\"`."
}

id_text_record! {
    /// A colour label.
    Color(ColorId, name) from Color in Colors,
    id at 5, "pdb color id",
    "The name."
}

id_text_record! {
    /// Album artwork: where the image is on the stick.
    Artwork(ArtworkId, path) from Artwork in Artwork,
    id at 0, "pdb artwork id",
    "The path of the 80×80 image; the 240×240 one has `_m` before the extension."
}
