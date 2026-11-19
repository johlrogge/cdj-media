//! Track rows (table 0).

use super::super::defaults;
use super::super::{
    AlbumId, ArtistId, ArtworkId, ColorId, GenreId, KeyId, LabelId, RawRow, TrackId,
};
use super::read_id;
use crate::bytes::Writer;
use crate::encoding::devicesql;
use crate::encoding::{Bpm, DeviceSqlString, StringForm};
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Error context of the string offsets.
const CTX_OFFSET: &str = "pdb track string offset";

/// Number of string slots in a track row.
const STRING_SLOTS: usize = 21;
/// Row offset of the first string offset.
const OFFSETS_AT: usize = 0x5e;
/// Length of the fixed part: the fields and the 21 string offsets.
const FIXED_LEN: usize = OFFSETS_AT + 2 * STRING_SLOTS;

/// A track's audio file type (track row offset 0x5a), as djl-analysis names the values.
///
/// ```compile_fail
/// // `Other` cannot be built by hand, so it can never alias a named type.
/// use cdj_media::pdb::{FileType, OtherFileType};
/// let _ = FileType::Other(OtherFileType(1));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FileType {
    /// 0, which djl-analysis calls unknown.
    Unknown,
    /// 1.
    Mp3,
    /// 4.
    M4a,
    /// 5.
    Flac,
    /// 0x0b.
    Wav,
    /// 0x0c.
    Aiff,
    /// Any other value. Only [`FileType::from_u16`] makes one, so it never holds a named
    /// type.
    Other(OtherFileType),
}

/// The value of a [`FileType::Other`]; never that of a named type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OtherFileType(u16);

impl OtherFileType {
    /// The stored value.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl FileType {
    /// The file type for a stored value.
    pub const fn from_u16(value: u16) -> Self {
        match value {
            0 => Self::Unknown,
            1 => Self::Mp3,
            4 => Self::M4a,
            5 => Self::Flac,
            0x0b => Self::Wav,
            0x0c => Self::Aiff,
            other => Self::Other(OtherFileType(other)),
        }
    }

    /// The stored value.
    pub const fn to_u16(self) -> u16 {
        match self {
            Self::Unknown => 0,
            Self::Mp3 => 1,
            Self::M4a => 4,
            Self::Flac => 5,
            Self::Wav => 0x0b,
            Self::Aiff => 0x0c,
            Self::Other(other) => other.get(),
        }
    }
}

/// One string slot: the bytes between the previous string (or the fixed part) and this
/// one, verbatim, then the string.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Slot {
    gap: Vec<u8>,
    string: DeviceSqlString,
}

/// The domain values of a track row, for [`Track::new`].
///
/// [`Default`] gives zeros, no references, [`FileType::Unknown`], empty strings, and the
/// two flags on, as in every row of the real stick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackFields {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// The composer.
    pub composer_id: Option<ArtistId>,
    /// The audio file's size in bytes.
    pub file_size: u32,
    /// The artwork.
    pub artwork_id: Option<ArtworkId>,
    /// The musical key.
    pub key_id: Option<KeyId>,
    /// The original artist.
    pub original_artist_id: Option<ArtistId>,
    /// The label.
    pub label_id: Option<LabelId>,
    /// The remixer.
    pub remixer_id: Option<ArtistId>,
    /// Bitrate in kbit/s.
    pub bitrate: u32,
    /// Track number.
    pub track_number: u32,
    /// Tempo.
    pub tempo: Bpm,
    /// The genre.
    pub genre_id: Option<GenreId>,
    /// The album.
    pub album_id: Option<AlbumId>,
    /// The artist.
    pub artist_id: Option<ArtistId>,
    /// Disc number.
    pub disc_number: u16,
    /// Play count.
    pub play_count: u16,
    /// Year.
    pub year: u16,
    /// Sample depth in bits.
    pub sample_depth: u16,
    /// Duration in seconds.
    pub duration: u16,
    /// The colour label; it must fit a byte.
    pub color_id: Option<ColorId>,
    /// Rating, 0 to 5.
    pub rating: u8,
    /// Audio file type.
    pub file_type: FileType,
    /// ISRC: empty, or ASCII without NULs.
    pub isrc: String,
    /// Lyricist.
    pub lyricist: String,
    /// Message.
    pub message: String,
    /// Publish track info: written as `"ON"`, or empty when off.
    pub publish_track_info: bool,
    /// Autoload hot cues: written as `"ON"`, or empty when off.
    pub autoload_hot_cues: bool,
    /// Date added, `YYYY-MM-DD`.
    pub date_added: String,
    /// Release date.
    pub release_date: String,
    /// Mix name.
    pub mix_name: String,
    /// Path of the analysis file on the stick, e.g.
    /// `/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT`.
    pub analysis_path: String,
    /// Analysis date, `YYYY-MM-DD`.
    pub analysis_date: String,
    /// Comment.
    pub comment: String,
    /// Title.
    pub title: String,
    /// The audio file's name.
    pub filename: String,
    /// The audio file's path on the stick.
    pub file_path: String,
}

impl Default for TrackFields {
    fn default() -> Self {
        Self {
            sample_rate: 0,
            composer_id: None,
            file_size: 0,
            artwork_id: None,
            key_id: None,
            original_artist_id: None,
            label_id: None,
            remixer_id: None,
            bitrate: 0,
            track_number: 0,
            tempo: Bpm::from_centi(0),
            genre_id: None,
            album_id: None,
            artist_id: None,
            disc_number: 0,
            play_count: 0,
            year: 0,
            sample_depth: 0,
            duration: 0,
            color_id: None,
            rating: 0,
            file_type: FileType::Unknown,
            isrc: String::new(),
            lyricist: String::new(),
            message: String::new(),
            publish_track_info: true,
            autoload_hot_cues: true,
            date_added: String::new(),
            release_date: String::new(),
            mix_name: String::new(),
            analysis_path: String::new(),
            analysis_date: String::new(),
            comment: String::new(),
            title: String::new(),
            filename: String::new(),
            file_path: String::new(),
        }
    }
}

/// The text rekordbox writes for a flag that is on.
const ON: &str = "ON";

/// The `color_id` byte: 0 for none, otherwise the id, which must fit a byte.
fn color_byte(color_id: Option<ColorId>) -> Result<u8, EncodeError> {
    color_id.map_or(Ok(0), |c| {
        u8::try_from(c.get()).map_err(|_| EncodeError::ValueOutOfRange {
            context: "pdb track color_id",
        })
    })
}

/// A track row (table 0).
///
/// | bytes      | field                | meaning                                         |
/// |------------|----------------------|-------------------------------------------------|
/// | 0x00..0x02 | `subtype`            | not understood; 0x24 in real files              |
/// | 0x02..0x04 | `index_shift`        | 0x20 × the row's slot in its page's row index   |
/// | 0x04..0x08 | `bitmask`            | not understood                                  |
/// | 0x08..0x0c | `sample_rate`        | Hz                                              |
/// | 0x0c..0x10 | `composer_id`        | artist, 0 for none                              |
/// | 0x10..0x14 | `file_size`          | bytes                                           |
/// | 0x14..0x18 | `unknown2`           | not understood; different in every real row; the id in built rows |
/// | 0x18..0x1a | `unknown3`           | not understood                                  |
/// | 0x1a..0x1c | `unknown4`           | not understood                                  |
/// | 0x1c..0x20 | `artwork_id`         | 0 for none                                      |
/// | 0x20..0x24 | `key_id`             | 0 for none                                      |
/// | 0x24..0x28 | `original_artist_id` | artist, 0 for none                              |
/// | 0x28..0x2c | `label_id`           | 0 for none                                      |
/// | 0x2c..0x30 | `remixer_id`         | artist, 0 for none                              |
/// | 0x30..0x34 | `bitrate`            | kbit/s                                          |
/// | 0x34..0x38 | `track_number`       |                                                 |
/// | 0x38..0x3c | `tempo`              | [`Bpm`], hundredths of a BPM                    |
/// | 0x3c..0x40 | `genre_id`           | 0 for none                                      |
/// | 0x40..0x44 | `album_id`           | 0 for none                                      |
/// | 0x44..0x48 | `artist_id`          | 0 for none                                      |
/// | 0x48..0x4c | `id`                 | [`TrackId`], never 0                            |
/// | 0x4c..0x4e | `disc_number`        |                                                 |
/// | 0x4e..0x50 | `play_count`         |                                                 |
/// | 0x50..0x52 | `year`               |                                                 |
/// | 0x52..0x54 | `sample_depth`       | bits                                            |
/// | 0x54..0x56 | `duration`           | seconds                                         |
/// | 0x56..0x58 | `unknown5`           | not understood; 0x29 in real files              |
/// | 0x58       | `color_id`           | 0 for none                                      |
/// | 0x59       | `rating`             | 0 to 5                                          |
/// | 0x5a..0x5c | `file_type`          | [`FileType`], stored raw                        |
/// | 0x5c..0x5e | `unknown7`           | not understood; 3 in real files                 |
/// | 0x5e..0x88 | string offsets       | 21 × `u16`, from the row start                  |
/// | 0x88..     | strings              | each after its gap bytes, in slot order         |
///
/// The string slots, by number: 0 ISRC, 1 lyricist, 2–4 unknown, 5 message, 6 publish
/// track info, 7 autoload hot cues, 8–9 unknown, 10 date added, 11 release date, 12 mix
/// name, 13 unknown, 14 analysis file path, 15 analysis date, 16 comment, 17 title,
/// 18 unknown, 19 file name, 20 file path. Slot 0 is decoded as an ISRC (ADR-005).
///
/// The string offsets are not stored: each is where its string lands after the previous
/// string and its own gap bytes. So a row decodes only when the offsets increase in slot
/// order and the first is at or after 0x88, which holds on every real row. The gap bytes
/// are kept verbatim, as they are not always zero. The row ends after its last string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Track {
    subtype: u16,
    index_shift: u16,
    bitmask: u32,
    sample_rate: u32,
    composer_id: Option<ArtistId>,
    file_size: u32,
    unknown2: u32,
    unknown3: u16,
    unknown4: u16,
    artwork_id: Option<ArtworkId>,
    key_id: Option<KeyId>,
    original_artist_id: Option<ArtistId>,
    label_id: Option<LabelId>,
    remixer_id: Option<ArtistId>,
    bitrate: u32,
    track_number: u32,
    tempo: Bpm,
    genre_id: Option<GenreId>,
    album_id: Option<AlbumId>,
    artist_id: Option<ArtistId>,
    id: TrackId,
    disc_number: u16,
    play_count: u16,
    year: u16,
    sample_depth: u16,
    duration: u16,
    unknown5: u16,
    color_id: Option<ColorId>,
    rating: u8,
    file_type: u16,
    unknown7: u16,
    /// Boxed, so that `ExportRow` stays small.
    strings: Box<[Slot; STRING_SLOTS]>,
}

/// Defines crate-private setters for fields that are not understood (ADR-007).
macro_rules! unknown_setters {
    ($($(#[$doc:meta])* $name:ident($field:ident: $ty:ty)),* $(,)?) => {
        $(
            $(#[$doc])*
            #[must_use]
            #[cfg_attr(
                not(test),
                expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
            )]
            pub(crate) fn $name(mut self, $field: $ty) -> Self {
                self.$field = $field;
                self
            }
        )*
    };
}

/// The 21 string slots of a track row, in slot order. `UnknownN` is slot N, which is not
/// understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrackString {
    /// Slot 0: the ISRC, decoded in the ISRC form (ADR-005).
    Isrc,
    /// Slot 1: the lyricist.
    Lyricist,
    /// Slot 2: not understood; a number of 1 or 2 digits in real rows.
    Unknown2,
    /// Slot 3: not understood; a single digit in real rows.
    Unknown3,
    /// Slot 4: not understood; empty or a number in real rows.
    Unknown4,
    /// Slot 5: the message.
    Message,
    /// Slot 6: publish track info, `"ON"` or empty.
    PublishTrackInfo,
    /// Slot 7: autoload hot cues, `"ON"` or empty.
    AutoloadHotCues,
    /// Slot 8: not understood; empty in real rows.
    Unknown8,
    /// Slot 9: not understood; empty in real rows.
    Unknown9,
    /// Slot 10: the date added.
    DateAdded,
    /// Slot 11: the release date.
    ReleaseDate,
    /// Slot 12: the mix name.
    MixName,
    /// Slot 13: not understood; empty in real rows.
    Unknown13,
    /// Slot 14: the analysis file path.
    AnalysisPath,
    /// Slot 15: the analysis date.
    AnalysisDate,
    /// Slot 16: the comment.
    Comment,
    /// Slot 17: the title.
    Title,
    /// Slot 18: not understood; empty in real rows.
    Unknown18,
    /// Slot 19: the audio file's name.
    Filename,
    /// Slot 20: the audio file's path.
    FilePath,
}

impl TrackString {
    /// Every slot, in slot order.
    pub const ALL: [Self; STRING_SLOTS] = [
        Self::Isrc,
        Self::Lyricist,
        Self::Unknown2,
        Self::Unknown3,
        Self::Unknown4,
        Self::Message,
        Self::PublishTrackInfo,
        Self::AutoloadHotCues,
        Self::Unknown8,
        Self::Unknown9,
        Self::DateAdded,
        Self::ReleaseDate,
        Self::MixName,
        Self::Unknown13,
        Self::AnalysisPath,
        Self::AnalysisDate,
        Self::Comment,
        Self::Title,
        Self::Unknown18,
        Self::Filename,
        Self::FilePath,
    ];

    /// The slot number, 0 to 20: the string's place in the row's offset table.
    pub const fn slot(self) -> usize {
        self as usize
    }
}

impl Track {
    /// Creates a track row as rekordbox does: `index_shift` 0 (the page writer sets it),
    /// rekordbox's unknowns and unknown strings from `pdb::defaults`, `unknown2` the id (it
    /// is different in every rekordbox row, see ADR-008), and each string in rekordbox's
    /// form: right after the
    /// previous one when short ASCII, otherwise at the next multiple of 4 after zero gap
    /// bytes. Rows start at multiples of 4 in the heap, so this is the heap alignment of the
    /// format-notes string layout rule. The ISRC gets the ISRC form, or short ASCII when
    /// empty.
    ///
    /// # Errors
    ///
    /// [`EncodeError::ValueOutOfRange`] when the ISRC is not ASCII without NULs, or the
    /// colour id does not fit a byte.
    pub fn new(id: TrackId, fields: TrackFields) -> Result<Self, EncodeError> {
        use TrackString as S;
        color_byte(fields.color_id)?;
        let flag = |on: bool| if on { ON } else { "" }.to_owned();
        let mut texts: [String; STRING_SLOTS] = Default::default();
        for (slot, text) in defaults::TRACK_UNKNOWN_STRINGS {
            texts[slot.slot()] = text.to_owned();
        }
        let named = [
            (S::Lyricist, fields.lyricist),
            (S::Message, fields.message),
            (S::PublishTrackInfo, flag(fields.publish_track_info)),
            (S::AutoloadHotCues, flag(fields.autoload_hot_cues)),
            (S::DateAdded, fields.date_added),
            (S::ReleaseDate, fields.release_date),
            (S::MixName, fields.mix_name),
            (S::AnalysisPath, fields.analysis_path),
            (S::AnalysisDate, fields.analysis_date),
            (S::Comment, fields.comment),
            (S::Title, fields.title),
            (S::Filename, fields.filename),
            (S::FilePath, fields.file_path),
        ];
        for (slot, text) in named {
            texts[slot.slot()] = text;
        }
        let isrc = if fields.isrc.is_empty() {
            DeviceSqlString::new("")
        } else {
            DeviceSqlString::isrc(&fields.isrc)?
        };
        let mut end = FIXED_LEN;
        let strings = texts.map(|text| DeviceSqlString::new(&text));
        let mut strings = strings.map(|string| Slot {
            gap: Vec::new(),
            string,
        });
        strings[S::Isrc.slot()].string = isrc;
        for slot in &mut strings {
            let at = if slot.string.form() == StringForm::ShortAscii {
                end
            } else {
                end.next_multiple_of(4)
            };
            slot.gap = vec![0; at - end];
            end = at.saturating_add(slot.string.encoded_len());
        }
        Ok(Self {
            subtype: defaults::TRACK_SUBTYPE,
            index_shift: 0,
            bitmask: defaults::TRACK_BITMASK,
            sample_rate: fields.sample_rate,
            composer_id: fields.composer_id,
            file_size: fields.file_size,
            unknown2: defaults::track_unknown2(id),
            unknown3: defaults::TRACK_UNKNOWN3,
            unknown4: defaults::TRACK_UNKNOWN4,
            artwork_id: fields.artwork_id,
            key_id: fields.key_id,
            original_artist_id: fields.original_artist_id,
            label_id: fields.label_id,
            remixer_id: fields.remixer_id,
            bitrate: fields.bitrate,
            track_number: fields.track_number,
            tempo: fields.tempo,
            genre_id: fields.genre_id,
            album_id: fields.album_id,
            artist_id: fields.artist_id,
            id,
            disc_number: fields.disc_number,
            play_count: fields.play_count,
            year: fields.year,
            sample_depth: fields.sample_depth,
            duration: fields.duration,
            unknown5: defaults::TRACK_UNKNOWN5,
            color_id: fields.color_id,
            rating: fields.rating,
            file_type: fields.file_type.to_u16(),
            unknown7: defaults::TRACK_UNKNOWN7,
            strings: Box::new(strings),
        })
    }

    unknown_setters! {
        /// Replaces `subtype` (offset 0).
        with_subtype(subtype: u16),
        /// Replaces `bitmask` (offset 4).
        with_bitmask(bitmask: u32),
        /// Replaces `unknown2` (offset 0x14).
        with_unknown2(unknown2: u32),
        /// Replaces `unknown3` (offset 0x18).
        with_unknown3(unknown3: u16),
        /// Replaces `unknown4` (offset 0x1a).
        with_unknown4(unknown4: u16),
        /// Replaces `unknown5` (offset 0x56).
        with_unknown5(unknown5: u16),
        /// Replaces `unknown7` (offset 0x5c).
        with_unknown7(unknown7: u16)
    }

    /// Replaces `index_shift` (offset 2); the page writer sets it.
    #[must_use]
    pub(crate) fn with_index_shift(mut self, index_shift: u16) -> Self {
        self.index_shift = index_shift;
        self
    }

    /// Replaces the string in `slot`, keeping its gap. [`TrackString::Isrc`] is decoded as
    /// an ISRC, so a string there round-trips only in the ISRC form, as short ASCII, or as
    /// UTF-16LE without the ISRC shape.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_string(mut self, slot: TrackString, string: DeviceSqlString) -> Self {
        self.strings[slot.slot()].string = string;
        self
    }

    /// Replaces the gap bytes before the string in `slot`.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_string_gap(mut self, slot: TrackString, gap: Vec<u8>) -> Self {
        self.strings[slot.slot()].gap = gap;
        self
    }

    /// The row's id.
    pub fn id(&self) -> TrackId {
        self.id
    }

    /// `subtype` (offset 0), as stored.
    pub fn subtype(&self) -> u16 {
        self.subtype
    }

    /// `index_shift` (offset 2), as stored.
    pub fn index_shift(&self) -> u16 {
        self.index_shift
    }

    /// `bitmask` (offset 4), not understood.
    pub fn bitmask(&self) -> u32 {
        self.bitmask
    }

    /// The sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The composer, or `None` when the field is 0.
    pub fn composer_id(&self) -> Option<ArtistId> {
        self.composer_id
    }

    /// The audio file's size in bytes.
    pub fn file_size(&self) -> u32 {
        self.file_size
    }

    /// `unknown2` (offset 0x14), not understood.
    pub fn unknown2(&self) -> u32 {
        self.unknown2
    }

    /// `unknown3` (offset 0x18), not understood.
    pub fn unknown3(&self) -> u16 {
        self.unknown3
    }

    /// `unknown4` (offset 0x1a), not understood.
    pub fn unknown4(&self) -> u16 {
        self.unknown4
    }

    /// The artwork, or `None` when the field is 0.
    pub fn artwork_id(&self) -> Option<ArtworkId> {
        self.artwork_id
    }

    /// The musical key, or `None` when the field is 0.
    pub fn key_id(&self) -> Option<KeyId> {
        self.key_id
    }

    /// The original artist, or `None` when the field is 0.
    pub fn original_artist_id(&self) -> Option<ArtistId> {
        self.original_artist_id
    }

    /// The label, or `None` when the field is 0.
    pub fn label_id(&self) -> Option<LabelId> {
        self.label_id
    }

    /// The remixer, or `None` when the field is 0.
    pub fn remixer_id(&self) -> Option<ArtistId> {
        self.remixer_id
    }

    /// The bitrate in kbit/s.
    pub fn bitrate(&self) -> u32 {
        self.bitrate
    }

    /// The track number.
    pub fn track_number(&self) -> u32 {
        self.track_number
    }

    /// The tempo.
    pub fn tempo(&self) -> Bpm {
        self.tempo
    }

    /// The genre, or `None` when the field is 0.
    pub fn genre_id(&self) -> Option<GenreId> {
        self.genre_id
    }

    /// The album, or `None` when the field is 0.
    pub fn album_id(&self) -> Option<AlbumId> {
        self.album_id
    }

    /// The artist, or `None` when the field is 0.
    pub fn artist_id(&self) -> Option<ArtistId> {
        self.artist_id
    }

    /// The disc number.
    pub fn disc_number(&self) -> u16 {
        self.disc_number
    }

    /// The play count.
    pub fn play_count(&self) -> u16 {
        self.play_count
    }

    /// The year.
    pub fn year(&self) -> u16 {
        self.year
    }

    /// The sample depth in bits.
    pub fn sample_depth(&self) -> u16 {
        self.sample_depth
    }

    /// The duration in seconds.
    pub fn duration(&self) -> u16 {
        self.duration
    }

    /// `unknown5` (offset 0x56), not understood.
    pub fn unknown5(&self) -> u16 {
        self.unknown5
    }

    /// The colour label, or `None` when the field is 0.
    pub fn color_id(&self) -> Option<ColorId> {
        self.color_id
    }

    /// The rating, 0 to 5 in rekordbox's rows; stored as is.
    pub fn rating(&self) -> u8 {
        self.rating
    }

    /// The audio file type.
    pub fn file_type(&self) -> FileType {
        FileType::from_u16(self.file_type)
    }

    /// `unknown7` (offset 0x5c), not understood.
    pub fn unknown7(&self) -> u16 {
        self.unknown7
    }

    /// The string in `slot`, with its stored form. The slots that are not understood
    /// (`TrackString::UnknownN`) have no named getter.
    pub fn string(&self, slot: TrackString) -> &DeviceSqlString {
        &self.strings[slot.slot()].string
    }

    /// The bytes before the string in `slot`, after the previous string (or, for
    /// [`TrackString::Isrc`], after the fixed part). Zero or empty in rekordbox's rows,
    /// except for leftover bytes in some gaps before a long-form string.
    pub fn string_gap(&self, slot: TrackString) -> &[u8] {
        &self.strings[slot.slot()].gap
    }

    /// The row's length as if every string started at a multiple of 4, without other gap
    /// bytes: what rekordbox's heap layout sizes a track row by (see
    /// [`ExportRow::heap_footprint`]).
    ///
    /// [`ExportRow::heap_footprint`]: super::ExportRow::heap_footprint
    pub(super) fn aligned_len(&self) -> usize {
        self.strings.iter().fold(FIXED_LEN, |len, s| {
            len.checked_next_multiple_of(4)
                .unwrap_or(usize::MAX)
                .saturating_add(s.string.encoded_len())
        })
    }

    /// Slot 0: the ISRC, in the ISRC form, or empty.
    pub fn isrc(&self) -> &DeviceSqlString {
        self.string(TrackString::Isrc)
    }

    /// Slot 1: the lyricist.
    pub fn lyricist(&self) -> &DeviceSqlString {
        self.string(TrackString::Lyricist)
    }

    /// Slot 5: the message.
    pub fn message(&self) -> &DeviceSqlString {
        self.string(TrackString::Message)
    }

    /// Slot 6: publish track info, `"ON"` or empty.
    pub fn publish_track_info(&self) -> &DeviceSqlString {
        self.string(TrackString::PublishTrackInfo)
    }

    /// Whether publish track info is on: slot 6 is exactly `"ON"`.
    pub fn is_publish_track_info_on(&self) -> bool {
        self.publish_track_info().as_str() == ON
    }

    /// Slot 7: autoload hot cues, `"ON"` or empty.
    pub fn autoload_hot_cues(&self) -> &DeviceSqlString {
        self.string(TrackString::AutoloadHotCues)
    }

    /// Whether autoload hot cues is on: slot 7 is exactly `"ON"`.
    pub fn is_autoload_hot_cues_on(&self) -> bool {
        self.autoload_hot_cues().as_str() == ON
    }

    /// Slot 10: the date added, `YYYY-MM-DD`.
    pub fn date_added(&self) -> &DeviceSqlString {
        self.string(TrackString::DateAdded)
    }

    /// Slot 11: the release date.
    pub fn release_date(&self) -> &DeviceSqlString {
        self.string(TrackString::ReleaseDate)
    }

    /// Slot 12: the mix name.
    pub fn mix_name(&self) -> &DeviceSqlString {
        self.string(TrackString::MixName)
    }

    /// Slot 14: the path of the track's analysis file on the stick.
    pub fn analysis_path(&self) -> &DeviceSqlString {
        self.string(TrackString::AnalysisPath)
    }

    /// Slot 15: the analysis date, `YYYY-MM-DD`.
    pub fn analysis_date(&self) -> &DeviceSqlString {
        self.string(TrackString::AnalysisDate)
    }

    /// Slot 16: the comment.
    pub fn comment(&self) -> &DeviceSqlString {
        self.string(TrackString::Comment)
    }

    /// Slot 17: the title.
    pub fn title(&self) -> &DeviceSqlString {
        self.string(TrackString::Title)
    }

    /// Slot 19: the audio file's name.
    pub fn filename(&self) -> &DeviceSqlString {
        self.string(TrackString::Filename)
    }

    /// Slot 20: the audio file's path on the stick.
    pub fn file_path(&self) -> &DeviceSqlString {
        self.string(TrackString::FilePath)
    }

    /// Decodes a track row.
    ///
    /// # Errors
    ///
    /// Errors are at absolute offsets: [`UnexpectedEof`] where the row is cut off,
    /// [`InvalidValue`] at an `id` of 0 and at a string offset that is before the end of
    /// the previous string (or, for slot 0, inside the fixed part) or past the end of the
    /// heap, and the string decoder's errors in the strings.
    ///
    /// [`UnexpectedEof`]: crate::DecodeErrorKind::UnexpectedEof
    /// [`InvalidValue`]: crate::DecodeErrorKind::InvalidValue
    pub fn decode(row: &RawRow<'_>) -> Result<Self, DecodeError> {
        let mut r = row.reader();
        let row_at = r.position();
        let subtype = r.u16_le().context("pdb track subtype")?;
        let index_shift = r.u16_le().context("pdb track index_shift")?;
        let bitmask = r.u32_le().context("pdb track bitmask")?;
        let sample_rate = r.u32_le().context("pdb track sample_rate")?;
        let composer_id = ArtistId::new(r.u32_le().context("pdb track composer_id")?);
        let file_size = r.u32_le().context("pdb track file_size")?;
        let unknown2 = r.u32_le().context("pdb track unknown2")?;
        let unknown3 = r.u16_le().context("pdb track unknown3")?;
        let unknown4 = r.u16_le().context("pdb track unknown4")?;
        let artwork_id = ArtworkId::new(r.u32_le().context("pdb track artwork_id")?);
        let key_id = KeyId::new(r.u32_le().context("pdb track key_id")?);
        let original_artist_id = ArtistId::new(r.u32_le().context("pdb track original_artist_id")?);
        let label_id = LabelId::new(r.u32_le().context("pdb track label_id")?);
        let remixer_id = ArtistId::new(r.u32_le().context("pdb track remixer_id")?);
        let bitrate = r.u32_le().context("pdb track bitrate")?;
        let track_number = r.u32_le().context("pdb track track_number")?;
        let tempo = Bpm::from_centi(r.u32_le().context("pdb track tempo")?);
        let genre_id = GenreId::new(r.u32_le().context("pdb track genre_id")?);
        let album_id = AlbumId::new(r.u32_le().context("pdb track album_id")?);
        let artist_id = ArtistId::new(r.u32_le().context("pdb track artist_id")?);
        let id = read_id(&mut r, "pdb track id", TrackId::new)?;
        let disc_number = r.u16_le().context("pdb track disc_number")?;
        let play_count = r.u16_le().context("pdb track play_count")?;
        let year = r.u16_le().context("pdb track year")?;
        let sample_depth = r.u16_le().context("pdb track sample_depth")?;
        let duration = r.u16_le().context("pdb track duration")?;
        let unknown5 = r.u16_le().context("pdb track unknown5")?;
        let color_id = ColorId::new(u16::from(r.u8().context("pdb track color_id")?));
        let rating = r.u8().context("pdb track rating")?;
        let file_type = r.u16_le().context("pdb track file_type")?;
        let unknown7 = r.u16_le().context("pdb track unknown7")?;
        let mut offsets = [(0u64, 0u16); STRING_SLOTS];
        for (at, offset) in &mut offsets {
            *at = r.position();
            *offset = r.u16_le().context(CTX_OFFSET)?;
        }
        let row_len = row.bytes().len() as u64;
        let mut strings = Vec::with_capacity(STRING_SLOTS);
        for (slot, &(offset_at, offset)) in TrackString::ALL.iter().zip(&offsets) {
            let end = r.position() - row_at;
            let gap_len = u64::from(offset)
                .checked_sub(end)
                .filter(|_| u64::from(offset) <= row_len)
                .and_then(|len| usize::try_from(len).ok())
                .ok_or_else(|| {
                    DecodeError::new(offset_at, CTX_OFFSET, DecodeErrorKind::InvalidValue)
                })?;
            let gap = r.bytes(gap_len).context("pdb track string gap")?.to_vec();
            let string = if *slot == TrackString::Isrc {
                devicesql::decode_isrc(&mut r)?
            } else {
                devicesql::decode(&mut r)?
            };
            strings.push(Slot { gap, string });
        }
        let strings = strings
            .into_boxed_slice()
            .try_into()
            .expect("one string per slot");
        Ok(Self {
            subtype,
            index_shift,
            bitmask,
            sample_rate,
            composer_id,
            file_size,
            unknown2,
            unknown3,
            unknown4,
            artwork_id,
            key_id,
            original_artist_id,
            label_id,
            remixer_id,
            bitrate,
            track_number,
            tempo,
            genre_id,
            album_id,
            artist_id,
            id,
            disc_number,
            play_count,
            year,
            sample_depth,
            duration,
            unknown5,
            color_id,
            rating,
            file_type,
            unknown7,
            strings,
        })
    }

    /// Encodes the row: the row's bytes.
    ///
    /// # Errors
    ///
    /// - [`EncodeError::ValueOutOfRange`] when the colour id does not fit a byte;
    /// - [`EncodeError::LengthOverflow`] when a string offset does not fit a `u16`, or a
    ///   string is too long for its form.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let color = color_byte(self.color_id)?;
        let mut offsets = [0u16; STRING_SLOTS];
        let mut at = FIXED_LEN;
        for (offset, slot) in offsets.iter_mut().zip(self.strings.iter()) {
            at = at.saturating_add(slot.gap.len());
            *offset = u16::try_from(at).map_err(|_| EncodeError::LengthOverflow {
                context: CTX_OFFSET,
            })?;
            at = at.saturating_add(slot.string.encoded_len());
        }
        let mut w = Writer::new();
        w.u16_le(self.subtype);
        w.u16_le(self.index_shift);
        w.u32_le(self.bitmask);
        w.u32_le(self.sample_rate);
        w.u32_le(self.composer_id.map_or(0, ArtistId::get));
        w.u32_le(self.file_size);
        w.u32_le(self.unknown2);
        w.u16_le(self.unknown3);
        w.u16_le(self.unknown4);
        w.u32_le(self.artwork_id.map_or(0, ArtworkId::get));
        w.u32_le(self.key_id.map_or(0, KeyId::get));
        w.u32_le(self.original_artist_id.map_or(0, ArtistId::get));
        w.u32_le(self.label_id.map_or(0, LabelId::get));
        w.u32_le(self.remixer_id.map_or(0, ArtistId::get));
        w.u32_le(self.bitrate);
        w.u32_le(self.track_number);
        w.u32_le(self.tempo.centi());
        w.u32_le(self.genre_id.map_or(0, GenreId::get));
        w.u32_le(self.album_id.map_or(0, AlbumId::get));
        w.u32_le(self.artist_id.map_or(0, ArtistId::get));
        w.u32_le(self.id.get());
        w.u16_le(self.disc_number);
        w.u16_le(self.play_count);
        w.u16_le(self.year);
        w.u16_le(self.sample_depth);
        w.u16_le(self.duration);
        w.u16_le(self.unknown5);
        w.u8(color);
        w.u8(self.rating);
        w.u16_le(self.file_type);
        w.u16_le(self.unknown7);
        for offset in offsets {
            w.u16_le(offset);
        }
        for slot in self.strings.iter() {
            w.bytes(&slot.gap);
            slot.string.encode(&mut w)?;
        }
        Ok(w.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{AT, raw};
    use super::*;
    use crate::DecodeErrorKind;
    use crate::encoding::{DeviceSqlString, StringForm};
    use crate::pdb::{AlbumId, ArtistId, ArtworkId, ColorId, GenreId, KeyId, LabelId};

    fn put16(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// The fixed part of track 0x2a with a distinct value in every field; the string offsets
    /// are 0 until [`row_bytes`] fills them.
    fn fixed() -> Vec<u8> {
        let mut b = vec![0; 0x88];
        put16(&mut b, 0x00, 0x24);
        put16(&mut b, 0x02, 0x60);
        put32(&mut b, 0x04, 0xc0700);
        put32(&mut b, 0x08, 44_100);
        put32(&mut b, 0x0c, 11);
        put32(&mut b, 0x10, 0x0123_4567);
        put32(&mut b, 0x14, 0x0abc_def0);
        put16(&mut b, 0x18, 0x3520);
        put16(&mut b, 0x1a, 0x1f58);
        put32(&mut b, 0x1c, 12);
        put32(&mut b, 0x20, 13);
        put32(&mut b, 0x24, 14);
        put32(&mut b, 0x28, 15);
        put32(&mut b, 0x2c, 16);
        put32(&mut b, 0x30, 320);
        put32(&mut b, 0x34, 7);
        put32(&mut b, 0x38, 12_800);
        put32(&mut b, 0x3c, 17);
        put32(&mut b, 0x40, 18);
        put32(&mut b, 0x44, 19);
        put32(&mut b, 0x48, 0x2a);
        put16(&mut b, 0x4c, 2);
        put16(&mut b, 0x4e, 9);
        put16(&mut b, 0x50, 2024);
        put16(&mut b, 0x52, 24);
        put16(&mut b, 0x54, 245);
        put16(&mut b, 0x56, 0x29);
        b[0x58] = 6;
        b[0x59] = 4;
        put16(&mut b, 0x5a, 5);
        put16(&mut b, 0x5c, 3);
        b
    }

    /// The ISRC field of "GBAYE0601498", as rekordbox writes it.
    const ISRC: &[u8] = b"\x90\x12\x00\x00\x03GBAYE0601498\x00";

    /// "Björk" as UTF-16LE.
    const BJORK: &[u8] = &[
        0x90, 0x0e, 0, 0, b'B', 0, b'j', 0, 0xf6, 0, b'r', 0, b'k', 0,
    ];

    /// The text of each string slot of the test track; slot 0 and 17 are [`ISRC`] and
    /// [`BJORK`].
    const TEXTS: [&str; 21] = [
        "GBAYE0601498",
        "Lyr",
        "2",
        "1",
        "",
        "Msg",
        "ON",
        "ON",
        "",
        "",
        "2024-01-02",
        "2023-12-31",
        "Extended",
        "",
        "/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT",
        "2024-01-03",
        "Nice",
        "Björk",
        "",
        "a.flac",
        "/Contents/B/a.flac",
    ];

    fn field(k: usize) -> Vec<u8> {
        match k {
            0 => ISRC.to_vec(),
            17 => BJORK.to_vec(),
            _ => DeviceSqlString::new(TEXTS[k]).to_bytes().unwrap(),
        }
    }

    /// The test track's strings, each with its gap: none before short ASCII, zeros up to the
    /// next multiple of 4 before a long form (the format-notes alignment rule).
    fn aligned() -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut end: usize = 0x88;
        (0..21)
            .map(|k| {
                let field = field(k);
                let gap = if field[0] & 1 == 1 {
                    0
                } else {
                    end.next_multiple_of(4) - end
                };
                end += gap + field.len();
                (vec![0; gap], field)
            })
            .collect()
    }

    /// The fixed part, the string offsets and the strings with their gaps.
    fn row_bytes(strings: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = fixed();
        for (k, (gap, field)) in strings.iter().enumerate() {
            bytes.extend_from_slice(gap);
            let at = u16::try_from(bytes.len()).unwrap();
            put16(&mut bytes, 0x5e + 2 * k, at);
            bytes.extend_from_slice(field);
        }
        bytes
    }

    /// The test track, then a byte of the next row.
    fn track_bytes() -> Vec<u8> {
        let mut bytes = row_bytes(&aligned());
        bytes.push(0xee);
        bytes
    }

    fn decode(bytes: &[u8]) -> Track {
        Track::decode(&raw(bytes)).unwrap()
    }

    #[test]
    fn track_decodes_every_fixed_field() {
        let track = decode(&track_bytes());
        assert_eq!(track.subtype(), 0x24);
        assert_eq!(track.index_shift(), 0x60);
        assert_eq!(track.bitmask(), 0xc0700);
        assert_eq!(track.sample_rate(), 44_100);
        assert_eq!(track.composer_id(), ArtistId::new(11));
        assert_eq!(track.file_size(), 0x0123_4567);
        assert_eq!(track.unknown2(), 0x0abc_def0);
        assert_eq!(track.unknown3(), 0x3520);
        assert_eq!(track.unknown4(), 0x1f58);
        assert_eq!(track.artwork_id(), ArtworkId::new(12));
        assert_eq!(track.key_id(), KeyId::new(13));
        assert_eq!(track.original_artist_id(), ArtistId::new(14));
        assert_eq!(track.label_id(), LabelId::new(15));
        assert_eq!(track.remixer_id(), ArtistId::new(16));
        assert_eq!(track.bitrate(), 320);
        assert_eq!(track.track_number(), 7);
        assert_eq!(track.tempo(), Bpm::from_centi(12_800));
        assert_eq!(track.genre_id(), GenreId::new(17));
        assert_eq!(track.album_id(), AlbumId::new(18));
        assert_eq!(track.artist_id(), ArtistId::new(19));
        assert_eq!(track.id(), TrackId::new(0x2a).unwrap());
        assert_eq!(track.disc_number(), 2);
        assert_eq!(track.play_count(), 9);
        assert_eq!(track.year(), 2024);
        assert_eq!(track.sample_depth(), 24);
        assert_eq!(track.duration(), 245);
        assert_eq!(track.unknown5(), 0x29);
        assert_eq!(track.color_id(), ColorId::new(6));
        assert_eq!(track.rating(), 4);
        assert_eq!(track.file_type(), FileType::Flac);
        assert_eq!(track.unknown7(), 3);
    }

    #[test]
    fn track_decodes_every_string_slot() {
        let track = decode(&track_bytes());
        let isrc = DeviceSqlString::isrc("GBAYE0601498").unwrap();
        assert_eq!(track.isrc(), &isrc);
        assert_eq!(track.isrc().form(), StringForm::Isrc);
        assert_eq!(track.title().form(), StringForm::Utf16Le);
        let named = [
            (TrackString::Isrc, track.isrc()),
            (TrackString::Lyricist, track.lyricist()),
            (TrackString::Message, track.message()),
            (TrackString::PublishTrackInfo, track.publish_track_info()),
            (TrackString::AutoloadHotCues, track.autoload_hot_cues()),
            (TrackString::DateAdded, track.date_added()),
            (TrackString::ReleaseDate, track.release_date()),
            (TrackString::MixName, track.mix_name()),
            (TrackString::AnalysisPath, track.analysis_path()),
            (TrackString::AnalysisDate, track.analysis_date()),
            (TrackString::Comment, track.comment()),
            (TrackString::Title, track.title()),
            (TrackString::Filename, track.filename()),
            (TrackString::FilePath, track.file_path()),
        ];
        for (slot, string) in named {
            assert_eq!(string.as_str(), TEXTS[slot.slot()], "{slot:?}");
            assert_eq!(track.string(slot), string, "{slot:?}");
        }
        assert!(track.is_publish_track_info_on());
        assert!(track.is_autoload_hot_cues_on());
        // Every slot, in slot order, including the ones that are not understood.
        assert_eq!(TrackString::ALL.len(), TEXTS.len());
        for (k, slot) in TrackString::ALL.into_iter().enumerate() {
            assert_eq!(slot.slot(), k);
            assert_eq!(track.string(slot).as_str(), TEXTS[k], "{slot:?}");
        }
        let unknown = [
            (TrackString::Unknown2, 2),
            (TrackString::Unknown3, 3),
            (TrackString::Unknown4, 4),
            (TrackString::Unknown8, 8),
            (TrackString::Unknown9, 9),
            (TrackString::Unknown13, 13),
            (TrackString::Unknown18, 18),
        ];
        for (slot, k) in unknown {
            assert_eq!(slot.slot(), k);
        }
        // The title is UTF-16LE, so it starts at the next multiple of 4.
        let gaps: Vec<usize> = TrackString::ALL
            .into_iter()
            .map(|slot| track.string_gap(slot).len())
            .collect();
        assert_eq!(gaps.iter().filter(|&&g| g > 0).count(), 1);
        assert_eq!(track.string_gap(TrackString::Title), [0]);
    }

    #[test]
    fn flag_accessors_read_on_only() {
        let off = TrackFields {
            publish_track_info: false,
            autoload_hot_cues: false,
            ..test_fields()
        };
        let track = Track::new(track_id(), off).unwrap();
        assert!(!track.is_publish_track_info_on());
        assert!(!track.is_autoload_hot_cues_on());
        let odd = track
            .with_string(TrackString::PublishTrackInfo, DeviceSqlString::new("on"))
            .with_string(TrackString::AutoloadHotCues, DeviceSqlString::new("ON"));
        assert!(!odd.is_publish_track_info_on());
        assert!(odd.is_autoload_hot_cues_on());
    }

    #[test]
    fn track_re_encodes_its_bytes() {
        let bytes = track_bytes();
        assert_eq!(decode(&bytes).encode().unwrap(), bytes[..bytes.len() - 1]);
    }

    #[test]
    fn file_type_names_djl_analysis_values() {
        let named = [
            (0, FileType::Unknown),
            (1, FileType::Mp3),
            (4, FileType::M4a),
            (5, FileType::Flac),
            (0x0b, FileType::Wav),
            (0x0c, FileType::Aiff),
        ];
        for (value, file_type) in named {
            assert_eq!(FileType::from_u16(value), file_type, "{value}");
            assert_eq!(file_type.to_u16(), value, "{file_type:?}");
        }
        for value in [2, 3, 6, 0x0a, 0x0d, u16::MAX] {
            let other = FileType::from_u16(value);
            assert!(
                matches!(other, FileType::Other(o) if o.get() == value),
                "{value}"
            );
            assert_eq!(other.to_u16(), value);
        }
    }

    /// The domain values of the test track.
    fn test_fields() -> TrackFields {
        let text = |k: usize| TEXTS[k].to_owned();
        TrackFields {
            sample_rate: 44_100,
            composer_id: ArtistId::new(11),
            file_size: 0x0123_4567,
            artwork_id: ArtworkId::new(12),
            key_id: KeyId::new(13),
            original_artist_id: ArtistId::new(14),
            label_id: LabelId::new(15),
            remixer_id: ArtistId::new(16),
            bitrate: 320,
            track_number: 7,
            tempo: Bpm::from_centi(12_800),
            genre_id: GenreId::new(17),
            album_id: AlbumId::new(18),
            artist_id: ArtistId::new(19),
            disc_number: 2,
            play_count: 9,
            year: 2024,
            sample_depth: 24,
            duration: 245,
            color_id: ColorId::new(6),
            rating: 4,
            file_type: FileType::Flac,
            isrc: text(0),
            lyricist: text(1),
            message: text(5),
            publish_track_info: true,
            autoload_hot_cues: true,
            date_added: text(10),
            release_date: text(11),
            mix_name: text(12),
            analysis_path: text(14),
            analysis_date: text(15),
            comment: text(16),
            title: text(17),
            filename: text(19),
            file_path: text(20),
        }
    }

    fn track_id() -> TrackId {
        TrackId::new(0x2a).unwrap()
    }

    #[test]
    fn builder_gives_rekordbox_layout() {
        // The test track has rekordbox's unknowns and unknown strings, so only
        // `index_shift` (set by the page writer) and `unknown2` (the id in built rows)
        // differ.
        let mut expected = row_bytes(&aligned());
        put16(&mut expected, 0x02, 0);
        put32(&mut expected, 0x14, 0x2a);
        let built = Track::new(track_id(), test_fields()).unwrap();
        assert_eq!(built.encode().unwrap(), expected);
        assert_eq!(built.title().form(), StringForm::Utf16Le);
        assert_eq!(built.string_gap(TrackString::Title), [0]);
    }

    #[test]
    fn builder_aligns_long_forms_to_4() {
        let fields = TrackFields {
            isrc: String::new(),
            title: "a".repeat(127),
            filename: "é".into(),
            ..test_fields()
        };
        let built = Track::new(track_id(), fields).unwrap();
        let bytes = built.encode().unwrap();
        assert_eq!(built.isrc(), &DeviceSqlString::new(""));
        assert_eq!(bytes[0x88], 0x03);
        let offset = |k: usize| {
            usize::from(u16::from_le_bytes([
                bytes[0x5e + 2 * k],
                bytes[0x5e + 2 * k + 1],
            ]))
        };
        for pair in TrackString::ALL.windows(2) {
            let (prev, slot) = (pair[0], pair[1]);
            let prev_end = offset(prev.slot()) + built.string(prev).encoded_len();
            let string = built.string(slot);
            let expected = if string.form() == StringForm::ShortAscii {
                prev_end
            } else {
                prev_end.next_multiple_of(4)
            };
            assert_eq!(offset(slot.slot()), expected, "{slot:?}");
            assert!(built.string_gap(slot).iter().all(|&b| b == 0));
        }
        assert_eq!(built.title().form(), StringForm::LongAscii);
        assert_eq!(built.filename().form(), StringForm::Utf16Le);
        assert_eq!(decode(&bytes), built);
    }

    #[test]
    fn builder_writes_on_flags_and_rejects_a_bad_isrc() {
        let fields = TrackFields {
            publish_track_info: false,
            autoload_hot_cues: true,
            ..test_fields()
        };
        let built = Track::new(track_id(), fields).unwrap();
        assert_eq!(built.publish_track_info(), &DeviceSqlString::new(""));
        assert_eq!(built.autoload_hot_cues(), &DeviceSqlString::new("ON"));
        let bad = TrackFields {
            isrc: "Björk".into(),
            ..test_fields()
        };
        assert_eq!(
            Track::new(track_id(), bad),
            Err(EncodeError::ValueOutOfRange {
                context: "DeviceSQL ISRC text"
            })
        );
    }

    #[test]
    fn default_fields_get_rekordbox_defaults() {
        let built = Track::new(track_id(), TrackFields::default()).unwrap();
        assert_eq!(built.subtype(), 0x24);
        assert_eq!(built.index_shift(), 0);
        assert_eq!(built.bitmask(), 0xc0700);
        assert_eq!(built.unknown2(), 0x2a, "unknown2 is the id");
        assert_eq!((built.unknown3(), built.unknown4()), (0x3520, 0x1f58));
        assert_eq!((built.unknown5(), built.unknown7()), (0x29, 3));
        assert_eq!(built.publish_track_info().as_str(), "ON");
        assert_eq!(built.autoload_hot_cues().as_str(), "ON");
        let unknown: Vec<&str> = [
            TrackString::Unknown2,
            TrackString::Unknown3,
            TrackString::Unknown4,
            TrackString::Unknown8,
            TrackString::Unknown9,
            TrackString::Unknown13,
            TrackString::Unknown18,
        ]
        .into_iter()
        .map(|slot| built.string(slot).as_str())
        .collect();
        assert_eq!(unknown, ["2", "1", "", "", "", "", ""]);
        assert_eq!(built.file_type(), FileType::Unknown);
        assert_eq!(built.tempo(), Bpm::from_centi(0));
        assert_eq!(built.title().as_str(), "");
    }

    #[test]
    fn colour_ids_that_do_not_fit_a_byte_are_rejected() {
        let err = EncodeError::ValueOutOfRange {
            context: "pdb track color_id",
        };
        let fields = TrackFields {
            color_id: ColorId::new(0x100),
            ..test_fields()
        };
        assert_eq!(Track::new(track_id(), fields), Err(err.clone()));
        let fits = TrackFields {
            color_id: ColorId::new(0xff),
            ..test_fields()
        };
        assert!(Track::new(track_id(), fits).is_ok());
        // `encode` checks too, for a row made some other way.
        let wide = Track {
            color_id: ColorId::new(0x100),
            ..Track::new(track_id(), test_fields()).unwrap()
        };
        assert_eq!(wide.encode(), Err(err));
    }

    #[test]
    fn unknown_file_type_and_subtype_round_trip() {
        let mut bytes = track_bytes();
        put16(&mut bytes, 0x00, 0x99);
        put16(&mut bytes, 0x5a, 0x0d);
        let track = decode(&bytes);
        assert_eq!(track.subtype(), 0x99);
        assert!(matches!(track.file_type(), FileType::Other(o) if o.get() == 0x0d));
        assert_eq!(track.encode().unwrap(), bytes[..bytes.len() - 1]);
    }

    #[test]
    fn non_zero_gap_bytes_round_trip() {
        let mut strings = aligned();
        strings[2].0 = vec![0xab];
        strings[17].0 = vec![b'x', b'y', b'z'];
        strings[20].0 = vec![1, 2, 3, 4, 5];
        let bytes = row_bytes(&strings);
        let track = decode(&bytes);
        assert_eq!(track.string_gap(TrackString::Unknown2), [0xab]);
        assert_eq!(track.string_gap(TrackString::Title), b"xyz");
        assert_eq!(track.string_gap(TrackString::FilePath), [1, 2, 3, 4, 5]);
        assert_eq!(track.title().as_str(), "Björk");
        assert_eq!(track.file_path().as_str(), TEXTS[20]);
        assert_eq!(track.encode().unwrap(), bytes);
    }

    fn err(bytes: &[u8]) -> (u64, &'static str, DecodeErrorKind) {
        let e = Track::decode(&raw(bytes)).unwrap_err();
        (e.offset, e.context, e.kind)
    }

    fn eof(needed: usize, available: usize) -> DecodeErrorKind {
        DecodeErrorKind::UnexpectedEof { needed, available }
    }

    #[test]
    fn truncated_fixed_part_is_eof_at_the_field() {
        let fixed = fixed();
        assert_eq!(
            err(&fixed[..0x0a]),
            (AT + 8, "pdb track sample_rate", eof(4, 2))
        );
        assert_eq!(
            err(&fixed[..0x59]),
            (AT + 0x59, "pdb track rating", eof(1, 0))
        );
        assert_eq!(
            err(&fixed[..0x87]),
            (AT + 0x86, "pdb track string offset", eof(2, 1))
        );
    }

    #[test]
    fn id_zero_is_invalid_at_the_id() {
        let mut bytes = track_bytes();
        put32(&mut bytes, 0x48, 0);
        assert_eq!(
            err(&bytes),
            (AT + 0x48, "pdb track id", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn string_offsets_out_of_order_are_invalid_at_the_offset() {
        let invalid = |slot: u64| {
            (
                AT + 0x5e + 2 * slot,
                "pdb track string offset",
                DecodeErrorKind::InvalidValue,
            )
        };
        // Slot 0 inside the fixed part.
        let mut bytes = track_bytes();
        put16(&mut bytes, 0x5e, 0x86);
        assert_eq!(err(&bytes), invalid(0));
        // Slot 5 at slot 4's string, before slot 4 ends.
        let mut bytes = track_bytes();
        let slot4 = u16::from_le_bytes([bytes[0x5e + 8], bytes[0x5e + 9]]);
        put16(&mut bytes, 0x5e + 10, slot4);
        assert_eq!(err(&bytes), invalid(5));
    }

    #[test]
    fn string_offsets_past_the_row_are_invalid_at_the_offset() {
        let mut bytes = track_bytes();
        let past = u16::try_from(bytes.len() + 1).unwrap();
        put16(&mut bytes, 0x5e + 40, past);
        assert_eq!(
            err(&bytes),
            (
                AT + 0x5e + 40,
                "pdb track string offset",
                DecodeErrorKind::InvalidValue
            )
        );
    }

    mod properties {
        use proptest::collection::vec;
        use proptest::prelude::*;

        use super::*;

        fn name() -> impl Strategy<Value = String> {
            prop_oneof![any::<String>(), "[ -~]{0,8}", "[ -~]{120,130}"]
        }

        fn id<T>(new: fn(u32) -> Option<T>) -> impl Fn(u32) -> Option<T> {
            move |v| new(v % 4)
        }

        /// Any track the builder and the crate-private setters can make.
        fn track() -> impl Strategy<Value = Track> {
            (
                (1u32.., any::<(u32, u32, u32, u32, u32)>(), vec(0u32..4, 9)),
                any::<(u16, u16, u16, u16, u16, u8, u8, u16)>(),
                (
                    vec(name(), 13),
                    any::<(bool, bool)>(),
                    "[\\x01-\\x7f]{0,20}",
                ),
                any::<(u16, u16, u32, u32, u16, u16, u16, u16)>(),
                vec(name(), 7),
                vec(vec(any::<u8>(), 0..6), STRING_SLOTS),
            )
                .prop_map(|(ids, small, texts, unknowns, unknown_texts, gaps)| {
                    let (track_id, nums, refs) = ids;
                    let artist = id(ArtistId::new);
                    let (strings, (publish, autoload), isrc) = texts;
                    let text = |k: usize| strings[k].clone();
                    let fields = TrackFields {
                        sample_rate: nums.0,
                        composer_id: artist(refs[0]),
                        file_size: nums.1,
                        artwork_id: id(ArtworkId::new)(refs[1]),
                        key_id: id(KeyId::new)(refs[2]),
                        original_artist_id: artist(refs[3]),
                        label_id: id(LabelId::new)(refs[4]),
                        remixer_id: artist(refs[5]),
                        bitrate: nums.2,
                        track_number: nums.3,
                        tempo: Bpm::from_centi(nums.4),
                        genre_id: id(GenreId::new)(refs[6]),
                        album_id: id(AlbumId::new)(refs[7]),
                        artist_id: artist(refs[8]),
                        disc_number: small.0,
                        play_count: small.1,
                        year: small.2,
                        sample_depth: small.3,
                        duration: small.4,
                        color_id: ColorId::new(u16::from(small.5)),
                        rating: small.6,
                        file_type: FileType::from_u16(small.7),
                        isrc,
                        lyricist: text(0),
                        message: text(1),
                        publish_track_info: publish,
                        autoload_hot_cues: autoload,
                        date_added: text(2),
                        release_date: text(3),
                        mix_name: text(4),
                        analysis_path: text(5),
                        analysis_date: text(6),
                        comment: text(7),
                        title: text(8),
                        filename: text(9),
                        file_path: text(10),
                    };
                    let mut track = Track::new(TrackId::new(track_id).unwrap(), fields)
                        .unwrap()
                        .with_subtype(unknowns.0)
                        .with_index_shift(unknowns.1)
                        .with_bitmask(unknowns.2)
                        .with_unknown2(unknowns.3)
                        .with_unknown3(unknowns.4)
                        .with_unknown4(unknowns.5)
                        .with_unknown5(unknowns.6)
                        .with_unknown7(unknowns.7);
                    let unknown_slots = [
                        TrackString::Unknown2,
                        TrackString::Unknown3,
                        TrackString::Unknown4,
                        TrackString::Unknown8,
                        TrackString::Unknown9,
                        TrackString::Unknown13,
                        TrackString::Unknown18,
                    ];
                    for (slot, text) in unknown_slots.into_iter().zip(&unknown_texts) {
                        track = track.with_string(slot, DeviceSqlString::new(text));
                    }
                    for (slot, gap) in TrackString::ALL.into_iter().zip(gaps) {
                        track = track.with_string_gap(slot, gap);
                    }
                    track
                })
        }

        /// Whatever decodes from `bytes` must re-encode to a prefix of `bytes`.
        fn check_any_bytes(bytes: &[u8]) -> Result<(), TestCaseError> {
            if let Ok(track) = Track::decode(&raw(bytes)) {
                let encoded = track.encode().unwrap();
                prop_assert_eq!(&bytes[..encoded.len()], &encoded[..]);
            }
            Ok(())
        }

        proptest! {
            #[test]
            fn built_tracks_encode_and_decode_to_themselves(track in track()) {
                let bytes = track.encode().unwrap();
                prop_assert_eq!(decode(&bytes), track);
            }

            #[test]
            fn decoding_arbitrary_bytes_never_panics(bytes in vec(any::<u8>(), 0..256)) {
                check_any_bytes(&bytes)?;
            }

            #[test]
            fn decoding_plausible_tracks_never_panics(
                id in 0u32..3,
                strings in vec((vec(any::<u8>(), 0..3), "[ -~]{0,3}"), STRING_SLOTS),
                flips in vec((any::<prop::sample::Index>(), any::<u8>()), 0..4),
                cut in any::<prop::sample::Index>(),
            ) {
                // A valid row of short strings with random gaps and id, then a few bytes
                // anywhere changed, and sometimes cut short.
                let fields: Vec<(Vec<u8>, Vec<u8>)> = strings
                    .into_iter()
                    .map(|(gap, s)| (gap, DeviceSqlString::new(&s).to_bytes().unwrap()))
                    .collect();
                let mut bytes = row_bytes(&fields);
                put32(&mut bytes, 0x48, id);
                for (at, value) in flips {
                    let at = at.index(bytes.len());
                    bytes[at] = value;
                }
                check_any_bytes(&bytes)?;
                check_any_bytes(&bytes[..cut.index(bytes.len())])?;
            }
        }
    }

    #[test]
    fn string_errors_are_at_the_string() {
        let mut bytes = track_bytes();
        let slot3 = usize::from(u16::from_le_bytes([bytes[0x5e + 6], bytes[0x5e + 7]]));
        bytes[slot3] = 0x00;
        assert_eq!(
            err(&bytes),
            (
                AT + slot3 as u64,
                "DeviceSQL string flag",
                DecodeErrorKind::InvalidValue
            )
        );
    }
}
