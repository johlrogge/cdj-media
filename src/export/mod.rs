//! [`ExportBuilder`]: a whole Phase 1 `PIONEER/` tree for the CDJ-2000/NXS from what a caller
//! knows about its tracks (ADR-018).
//!
//! The builder is the caller's domain API (ADR-008, "Layers"). It composes the crate's
//! pieces and duplicates none of them:
//! - [`Database`] for `export.pdb` (ADR-016, ADR-017);
//! - [`AnlzFile::track_dat`](crate::anlz::AnlzFile::track_dat) for each track's `.DAT`
//!   (ADR-015);
//! - [`BeatGrid::constant_tempo`](crate::anlz::BeatGrid::constant_tempo) for constant grids
//!   (ADR-014);
//! - [`layout`](crate::layout) for every path (ADR-010).
//!
//! It never reads or copies audio: [`Export::audio_destinations`] says where the caller must
//! copy each track's audio file. The output is in memory; [`Export::write_to`] writes it to a
//! fresh stick.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::path::Path;

use crate::anlz::{
    AnlzFile, BeatGrid, ConstantGrid, CueList, CueSpan, HotCueSlot, TinyWaveformPreview,
    WaveformPreview,
};
use crate::encoding::{Bpm, Millis};
use crate::error::{EncodeError, Error};
use crate::layout::{
    AnlzPath, ArtworkPath, ContentPath, ContentPathAllocator, StickPath, audio_path_tag,
    export_pdb_path,
};
use crate::pdb::model::{Dangling, RecordId, Reference};
use crate::pdb::{ColorId, Database, ExportRow, FileType, PlaylistId, TrackFields, TrackId};

/// The JPEG start-of-image marker, the only part of an artwork image that is checked.
const JPEG_SOI: [u8; 2] = [0xff, 0xd8];

/// The column height of [`Waveform::Flat`]'s `PWAV`: 2, as rekordbox writes for the one
/// silent track on the real-stick fixture (ADR-015).
const FLAT_PREVIEW_HEIGHT: u8 = 2;

/// The column height of [`Waveform::Flat`]'s `PWV2`: 1, as rekordbox writes for the one
/// silent track on the real-stick fixture (ADR-015).
const FLAT_TINY_PREVIEW_HEIGHT: u8 = 1;

/// A calendar date, written as `YYYY-MM-DD` in the track rows' date added and analysis date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExportDate {
    year: u16,
    month: u8,
    day: u8,
}

impl ExportDate {
    /// The date `year`-`month`-`day`; `None` when it is not a date of years 0 to 9999.
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        (year <= 9999 && (1..=days).contains(&day)).then_some(Self { year, month, day })
    }
}

impl fmt::Display for ExportDate {
    /// `YYYY-MM-DD`, as the track rows hold it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// The audio file format of a track. Only formats whose `.DAT` the crate can build: their
/// `PVBR` seek index is all zeros (ADR-015), which is wrong for MP3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AudioFormat {
    /// AIFF, the default.
    #[default]
    Aiff,
    /// WAV.
    Wav,
}

impl AudioFormat {
    /// The track row's file type.
    fn file_type(self) -> FileType {
        match self {
            Self::Aiff => FileType::Aiff,
            Self::Wav => FileType::Wav,
        }
    }
}

/// A track's beat grid.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Grid {
    /// No grid: an empty `PQTZ` and tempo 0, the default.
    #[default]
    None,
    /// A constant-tempo grid, expanded with
    /// [`BeatGrid::constant_tempo`](crate::anlz::BeatGrid::constant_tempo) over the track's
    /// duration.
    Constant {
        /// A downbeat, beat 1 of its bar.
        first_beat: Millis,
        /// The tempo throughout; not 0.
        tempo: Bpm,
    },
    /// Beats as given.
    Beats(BeatGrid),
}

/// A track's waveform previews (`PWAV` and `PWV2`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Waveform {
    /// A flat placeholder, the default: [`WaveformPreview::flat`] of height 2 and
    /// [`TinyWaveformPreview::flat`] of height 1, what rekordbox writes for a silent track.
    #[default]
    Flat,
    /// Previews as given.
    Previews(WaveformPreview, TinyWaveformPreview),
}

/// What the caller knows about one track, for [`ExportBuilder::add_track`].
///
/// [`Default`] gives empty text, zeros, no colour, [`AudioFormat::Aiff`], [`Grid::None`], no
/// cues, [`Waveform::Flat`], no artwork and no dates of its own.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NewTrack {
    /// Title.
    pub title: String,
    /// Artist; also the artist directory of the audio file's path.
    pub artist: String,
    /// Album; also the album directory of the audio file's path.
    pub album: String,
    /// Album artist.
    pub album_artist: String,
    /// Composer.
    pub composer: String,
    /// Remixer.
    pub remixer: String,
    /// Original artist.
    pub original_artist: String,
    /// Genre.
    pub genre: String,
    /// Label.
    pub label: String,
    /// Musical key, e.g. `"8A"` or `"Am"`.
    pub key: String,
    /// Comment.
    pub comment: String,
    /// Mix name.
    pub mix_name: String,
    /// ISRC: empty, or ASCII without NULs.
    pub isrc: String,
    /// Year.
    pub year: u16,
    /// Track number.
    pub track_number: u32,
    /// Disc number.
    pub disc_number: u16,
    /// Rating, 0 to 5.
    pub rating: u8,
    /// Colour label, one of the colours 1 to 8 of [`Database::new`].
    pub color: Option<ColorId>,
    /// The audio file's name, with its extension; the last component of its path.
    pub file_name: String,
    /// The audio file's format.
    pub format: AudioFormat,
    /// The audio file's size in bytes.
    pub file_size: u32,
    /// The exact length of the audio.
    pub duration: Millis,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Sample depth in bits.
    pub sample_depth: u16,
    /// Bitrate in kbit/s.
    pub bitrate: u32,
    /// The beat grid.
    pub grid: Grid,
    /// Hot cues A to C, in the order given; each slot at most once.
    pub hot_cues: Vec<(HotCueSlot, CueSpan)>,
    /// Memory points and loops, in the order given.
    pub memory_cues: Vec<CueSpan>,
    /// The waveform previews.
    pub waveform: Waveform,
    /// The artwork, a JPEG image, written as given.
    pub artwork: Option<Vec<u8>>,
    /// Release date, as the row holds it.
    pub release_date: String,
    /// Lyricist.
    pub lyricist: String,
    /// Play count.
    pub play_count: u16,
    /// The date added; the builder's date when `None`.
    pub date_added: Option<ExportDate>,
    /// The analysis date; the builder's date when `None`.
    pub analysis_date: Option<ExportDate>,
}

/// Builds a whole export in memory: see the [module documentation](self).
#[derive(Debug, Clone)]
pub struct ExportBuilder {
    date: ExportDate,
    database: Database,
    /// The audio file paths allocated so far.
    paths: ContentPathAllocator,
    /// The `.DAT` files and artwork images of the tracks added, by path.
    files: BTreeMap<StickPath, Vec<u8>>,
    /// Each track's id and audio file path, in input order.
    destinations: Vec<(TrackId, ContentPath)>,
}

impl ExportBuilder {
    /// A builder with no tracks and no playlists. `date` is the date added and analysis
    /// date of every track that doesn't give its own; the library reads no clock.
    pub fn new(date: ExportDate) -> Self {
        Self {
            date,
            database: Database::new(),
            paths: ContentPathAllocator::new(),
            files: BTreeMap::new(),
            destinations: Vec::new(),
        }
    }

    /// Adds a track and returns its id, which [`Export::audio_destinations`] pairs with the
    /// path its audio file goes to.
    ///
    /// The paths come from [`layout`](crate::layout): the audio file's from
    /// [`ContentPath::for_track`] of the artist, album and file name, made distinct from
    /// earlier tracks' by [`ContentPathAllocator`] (slots 19 and 20, `PPTH` and the
    /// destination all get that path); the `.DAT`'s from [`AnlzPath::for_track`] (slot 14);
    /// the artwork's from [`ArtworkPath::for_artwork`], one artwork row per track. Names go
    /// through the `find_or_insert_*` methods of [`Database`] exactly as given. The row's
    /// tempo is the grid's: [`Grid::Constant`]'s tempo even when the track is too short for
    /// a beat, the first beat's of [`Grid::Beats`], 0 otherwise. Its duration is the whole
    /// seconds of [`NewTrack::duration`].
    ///
    /// Whatever [`build`](Self::build) would refuse about the track is refused here.
    ///
    /// # Errors
    ///
    /// Nothing changes on error. [`EncodeError::ValueOutOfRange`] for:
    /// - a hot cue slot other than A to C, or one used twice (`"hot cue slot"`);
    /// - a cue that is neither a point nor a loop (`"cue type"`);
    /// - a loop whose end is not after its start (`"cue loop end"`);
    /// - artwork that doesn't start with the JPEG SOI marker (`"artwork"`);
    /// - a file name without an extension once sanitised (`"audio file name"`);
    /// - [`Grid::Constant`] of tempo 0 (`"grid tempo"`);
    /// - a duration of 65,536 s or more (`"track duration"`);
    /// - a colour the database has no row for (`"track color"`);
    ///
    /// [`EncodeError::LengthOverflow`] (`"pdb page heap"`) when the track's row, or the row
    /// of one of its names, doesn't fit an empty page, as [`Database::to_pdb`] would refuse
    /// it; and the errors of [`Track::new`](crate::pdb::Track::new) (ISRC, a colour id
    /// above 255) and of encoding the `.DAT` (a beat's tempo above 655.35 BPM, lengths).
    pub fn add_track(&mut self, track: NewTrack) -> Result<TrackId, EncodeError> {
        check_cues(&track)?;
        check_name_rows(&track)?;
        if track
            .artwork
            .as_ref()
            .is_some_and(|image| !image.starts_with(&JPEG_SOI))
        {
            return Err(EncodeError::ValueOutOfRange { context: "artwork" });
        }
        let own = ContentPath::for_track(&track.artist, &track.album, &track.file_name);
        if !own.has_extension() {
            return Err(EncodeError::ValueOutOfRange {
                context: "audio file name",
            });
        }
        // Everything that can fail is done before anything changes: the path is peeked and
        // only allocated once the `.DAT` and the row are known to encode.
        let content = self.paths.peek(&own);
        let id = self.database.next_track_id();
        let dat_path = AnlzPath::for_track(id).dat();
        let (preview, tiny) = match track.waveform {
            Waveform::Flat => (
                WaveformPreview::flat(FLAT_PREVIEW_HEIGHT),
                TinyWaveformPreview::flat(FLAT_TINY_PREVIEW_HEIGHT),
            ),
            Waveform::Previews(preview, tiny) => (preview, tiny),
        };
        // The row's tempo is the grid's: a constant grid's tempo, even without beats, or
        // the first beat's (one tempo throughout, as rekordbox's grids have).
        let (grid, tempo) = match track.grid {
            Grid::None => (BeatGrid::new(vec![]), Bpm::from_centi(0)),
            Grid::Constant { first_beat, tempo } => {
                let grid = ConstantGrid::new(first_beat, tempo, track.duration).ok_or(
                    EncodeError::ValueOutOfRange {
                        context: "grid tempo",
                    },
                )?;
                (BeatGrid::constant_tempo(&grid), tempo)
            }
            Grid::Beats(grid) => {
                let tempo = grid
                    .beats()
                    .first()
                    .map_or(Bpm::from_centi(0), |beat| beat.tempo());
                (grid, tempo)
            }
        };
        let dat = AnlzFile::track_dat(
            audio_path_tag(&content),
            grid,
            CueList::hot(track.hot_cues),
            CueList::memory(track.memory_cues),
            preview,
            tiny,
        )?
        .to_bytes()?;
        let duration = u16::try_from(track.duration.get() / 1000).map_err(|_| {
            EncodeError::ValueOutOfRange {
                context: "track duration",
            }
        })?;

        let date = |own: Option<ExportDate>| own.unwrap_or(self.date).to_string();
        // The row without the references the database allocates, which no row error
        // depends on.
        let unreferenced = TrackFields {
            sample_rate: track.sample_rate,
            file_size: track.file_size,
            bitrate: track.bitrate,
            track_number: track.track_number,
            tempo,
            disc_number: track.disc_number,
            year: track.year,
            sample_depth: track.sample_depth,
            duration,
            color_id: track.color,
            rating: track.rating,
            file_type: track.format.file_type(),
            isrc: track.isrc.clone(),
            date_added: date(track.date_added),
            release_date: track.release_date.clone(),
            lyricist: track.lyricist.clone(),
            play_count: track.play_count,
            mix_name: track.mix_name.clone(),
            analysis_path: dat_path.as_str().to_owned(),
            analysis_date: date(track.analysis_date),
            comment: track.comment.clone(),
            title: track.title.clone(),
            filename: content.file_name().to_owned(),
            file_path: content.as_stick_path().as_str().to_owned(),
            ..TrackFields::default()
        };
        let row = crate::pdb::Track::new(id, unreferenced.clone())?;
        Database::check_fits(&ExportRow::Track(row))?;
        if track
            .color
            .is_some_and(|color| self.database.color(color).is_none())
        {
            return Err(EncodeError::ValueOutOfRange {
                context: "track color",
            });
        }

        // Nothing fails from here on.
        let allocated = self.paths.allocate(own);
        assert_eq!(allocated, content, "allocate gives the peeked path");
        let db = &mut self.database;
        let artist_id = db.find_or_insert_artist(&track.artist);
        let composer_id = db.find_or_insert_artist(&track.composer);
        let original_artist_id = db.find_or_insert_artist(&track.original_artist);
        let remixer_id = db.find_or_insert_artist(&track.remixer);
        let album_artist_id = db.find_or_insert_artist(&track.album_artist);
        let album_id = db.find_or_insert_album(&track.album, album_artist_id);
        let genre_id = db.find_or_insert_genre(&track.genre);
        let label_id = db.find_or_insert_label(&track.label);
        let key_id = db.find_or_insert_key(&track.key);
        // Each track its own artwork row (ADR-016), so its path is new.
        let artwork = track.artwork.map(|image| {
            let id = db.next_artwork_id();
            let path = ArtworkPath::for_artwork(id);
            let inserted = db.find_or_insert_artwork(path.as_str());
            assert_eq!(inserted, Some(id), "a new artwork row gets the peeked id");
            (id, path, image)
        });
        let fields = TrackFields {
            artwork_id: artwork.as_ref().map(|(id, _, _)| *id),
            composer_id,
            key_id,
            original_artist_id,
            label_id,
            remixer_id,
            genre_id,
            album_id,
            artist_id,
            ..unreferenced
        };
        let added = db
            .add_track(fields)
            .expect("the row encoded without its references");
        assert_eq!(added, id, "add_track allocates the peeked id");
        self.files.insert(dat_path, dat);
        if let Some((_, path, image)) = artwork {
            self.files.insert(path, image);
        }
        self.destinations.push((id, content));
        Ok(id)
    }

    /// Adds an empty playlist named `name` to the folder `parent`, or to the root for
    /// `None`, after its parent's other children, as [`Database::add_playlist`] does. A
    /// `parent` the export doesn't have is refused by [`build`](Self::build).
    ///
    /// # Errors
    ///
    /// The errors of [`Database::add_playlist`]; nothing changes on error.
    pub fn add_playlist(
        &mut self,
        name: &str,
        parent: Option<PlaylistId>,
    ) -> Result<PlaylistId, EncodeError> {
        self.database.add_playlist(name, parent)
    }

    /// Adds an empty folder, which holds playlists and folders, as
    /// [`add_playlist`](Self::add_playlist) adds a playlist.
    ///
    /// # Errors
    ///
    /// The errors of [`Database::add_folder`]; nothing changes on error.
    pub fn add_folder(
        &mut self,
        name: &str,
        parent: Option<PlaylistId>,
    ) -> Result<PlaylistId, EncodeError> {
        self.database.add_folder(name, parent)
    }

    /// Appends `track` to the tracks of `playlist`.
    ///
    /// # Errors
    ///
    /// Nothing changes on error:
    /// - [`EncodeError::DanglingReference`] when the export has no track `track` or no
    ///   playlist `playlist`, the reference [`Database::to_pdb`] would report for the entry
    ///   (the track is checked first);
    /// - [`EncodeError::ValueOutOfRange`] (`"pdb playlist entry playlist_id"`) when
    ///   `playlist` is a folder, which holds no tracks.
    pub fn add_to_playlist(
        &mut self,
        playlist: PlaylistId,
        track: TrackId,
    ) -> Result<(), EncodeError> {
        let db = &self.database;
        let index = db.playlist_tracks(playlist).count().saturating_add(1);
        let from = RecordId::PlaylistEntry {
            playlist: Some(playlist),
            index: u32::try_from(index).unwrap_or(u32::MAX),
        };
        let dangling = |field, id| EncodeError::DanglingReference(Dangling { from, field, id });
        if db.track(track).is_none() {
            let id = Some(RecordId::Track(track));
            return Err(dangling(Reference::PlaylistEntryTrack, id));
        }
        match db.playlist(playlist) {
            None => {
                let id = Some(RecordId::Playlist(playlist));
                return Err(dangling(Reference::PlaylistEntryPlaylist, id));
            }
            Some(node) if node.is_folder() => {
                return Err(EncodeError::ValueOutOfRange {
                    context: "pdb playlist entry playlist_id",
                });
            }
            Some(_) => {}
        }
        self.database.add_to_playlist(playlist, track);
        Ok(())
    }

    /// Builds the export: `export.pdb`, and the files of the tracks added. The builder is
    /// left as it was, so a caller can add more and build again.
    ///
    /// # Errors
    ///
    /// The errors of [`Database::to_pdb`]: a playlist or folder whose parent the export
    /// doesn't have is [`EncodeError::DanglingReference`].
    pub fn build(&self) -> Result<Export, EncodeError> {
        let pdb = self.database.to_pdb()?.to_bytes()?;
        let mut files = self.files.clone();
        files.insert(export_pdb_path(), pdb);
        Ok(Export {
            files,
            destinations: self.destinations.clone(),
        })
    }
}

/// Checks that the row of each of `track`'s names fits a page, as [`Database::to_pdb`]
/// will need: the rows a new name creates, and, harmlessly, those of names the database
/// already has, whose rows fit. A row's length doesn't depend on its ids, so any will do.
fn check_name_rows(track: &NewTrack) -> Result<(), EncodeError> {
    use crate::pdb::{
        Album, AlbumId, Artist, ArtistId, Genre, GenreId, Key, KeyId, Label, LabelId,
    };
    let artist = ArtistId::new(1).expect("not 0");
    let artists = [
        &track.artist,
        &track.composer,
        &track.original_artist,
        &track.remixer,
        &track.album_artist,
    ];
    let mut rows: Vec<ExportRow> = artists
        .into_iter()
        .filter(|name| !name.is_empty())
        .map(|name| ExportRow::Artist(Artist::new(artist, name)))
        .collect();
    if !track.album.is_empty() {
        let album_artist = (!track.album_artist.is_empty()).then_some(artist);
        let id = AlbumId::new(1).expect("not 0");
        rows.push(ExportRow::Album(Album::new(id, &track.album, album_artist)));
    }
    if !track.genre.is_empty() {
        let id = GenreId::new(1).expect("not 0");
        rows.push(ExportRow::Genre(Genre::new(id, &track.genre)));
    }
    if !track.label.is_empty() {
        let id = LabelId::new(1).expect("not 0");
        rows.push(ExportRow::Label(Label::new(id, &track.label)));
    }
    if !track.key.is_empty() {
        let id = KeyId::new(1).expect("not 0");
        rows.push(ExportRow::Key(Key::new(id, &track.key)));
    }
    rows.iter().try_for_each(Database::check_fits)
}

/// Checks `track`'s cues: hot cues in slots A to C, each at most once, every cue a point
/// or a loop, and every loop ending after it starts.
fn check_cues(track: &NewTrack) -> Result<(), EncodeError> {
    let mut slots = Vec::new();
    for &(slot, _) in &track.hot_cues {
        let known = matches!(slot, HotCueSlot::A | HotCueSlot::B | HotCueSlot::C);
        if !known || slots.contains(&slot) {
            return Err(EncodeError::ValueOutOfRange {
                context: "hot cue slot",
            });
        }
        slots.push(slot);
    }
    let spans = track.hot_cues.iter().map(|(_, span)| span);
    for span in spans.chain(&track.memory_cues) {
        let context = match span {
            CueSpan::Point(_) => continue,
            CueSpan::Loop { start, end } if start < end => continue,
            CueSpan::Loop { .. } => "cue loop end",
            _ => "cue type",
        };
        return Err(EncodeError::ValueOutOfRange { context });
    }
    Ok(())
}

/// A built export: every file to write, by its path on the stick, and where each track's
/// audio file goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    files: BTreeMap<StickPath, Vec<u8>>,
    destinations: Vec<(TrackId, ContentPath)>,
}

impl Export {
    /// Every file of the export, by path, in path order: `export.pdb`, one `.DAT` per track
    /// and the artwork images.
    pub fn files(&self) -> impl Iterator<Item = (&StickPath, &[u8])> + '_ {
        self.files
            .iter()
            .map(|(path, bytes)| (path, bytes.as_slice()))
    }

    /// The file at `path`, if the export has one.
    pub fn file(&self, path: &StickPath) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Each track's id and the path the caller must copy its audio file to, in the order
    /// the tracks were added.
    pub fn audio_destinations(&self) -> impl Iterator<Item = (TrackId, &ContentPath)> + '_ {
        self.destinations.iter().map(|(id, path)| (*id, path))
    }

    /// Writes every file under `root`, the stick's mount point, creating the directories
    /// it needs. It is for fresh sticks: if any of the files already exists it writes
    /// nothing, and every file is opened with `create_new`, so none is ever overwritten.
    ///
    /// # Errors
    ///
    /// [`Error::Io`]: of kind [`AlreadyExists`](std::io::ErrorKind::AlreadyExists), naming
    /// the stick path, when a file exists before anything is written; otherwise the error of
    /// the failing directory or file operation, after the files before it were written.
    pub fn write_to(&self, root: &Path) -> Result<(), Error> {
        for path in self.files.keys() {
            if path.under(root).try_exists()? {
                let message = format!("{path} already exists");
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, message).into());
            }
        }
        for (path, bytes) in &self.files {
            let file = path.under(root);
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)?
                .write_all(bytes)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
