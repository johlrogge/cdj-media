//! Builds a complete Phase 1 stick from an MDMA export, with no rekordbox involved.
//!
//! ```text
//! cargo run --example mdma_export -- <mdma-export dir> <output root> [--date YYYY-MM-DD]
//! ```
//!
//! The MDMA export directory holds:
//! - `rekordbox.xml`: the tracks' tags and the playlists;
//! - `manifest.tsv`: one line per track, in MDMA's export order, of the audio file's path
//!   and its timeline JSON's path, both relative to the directory;
//! - `timeline/<sha256>.json`: each track's exact length, cues and constant beat grid;
//! - the AIFF files.
//!
//! The tracks are added in manifest order. Each manifest line is joined to its XML `TRACK` by
//! the `Location`, percent-decoded, which must end in the line's audio path. A track gets:
//! - its tags from the XML: the key from `Tonality` (empty when there is none);
//! - its exact length, sample rate and depth from the AIFF header's `COMM` chunk, its
//!   bitrate as rekordbox computes it (rate × depth × channels / 1000), and its size from
//!   the file's metadata; no audio is decoded;
//! - its grid and cues from the timeline (cue labels are dropped in Phase 1);
//! - [`Waveform::Flat`] and no artwork.
//!
//! The XML's playlists (`NODE Type="1"`) are added at the root, folders flattened, with their
//! tracks in XML order.
//!
//! The PIONEER tree is written with [`Export::write_to`]; then each audio file is copied to
//! its [`ContentPath`] under the output root, never overwriting. An output root that already
//! holds `PIONEER/` or `Contents/` is refused. The program prints counts only, never names
//! or paths; errors name a track by its manifest line.

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cdj_media::anlz::{CueSpan, HotCueSlot};
use cdj_media::encoding::{Bpm, Millis};
use cdj_media::export::{AudioFormat, Export, ExportBuilder, ExportDate, Grid, NewTrack, Waveform};
use cdj_media::layout::ContentPath;
use cdj_media::pdb::TrackId;
use serde_json::Value;

/// The date added and analysis date when `--date` is not given.
const DEFAULT_DATE: (u16, u8, u8) = (2000, 1, 1);

/// What an AIFF file's `COMM` chunk says about its audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AiffInfo {
    /// `numChannels`.
    channels: u16,
    /// `numSampleFrames`.
    frames: u32,
    /// The sample rate in Hz, a whole number.
    sample_rate: u32,
    /// `sampleSize`, in bits.
    depth: u16,
}

impl AiffInfo {
    /// The exact length, `frames · 1000 / rate`, rounded down to whole milliseconds.
    fn duration_ms(self) -> u32 {
        let ms = u64::from(self.frames) * 1000 / u64::from(self.sample_rate.max(1));
        u32::try_from(ms).unwrap_or(u32::MAX)
    }

    /// rekordbox's bitrate in kbit/s: rate × depth × channels / 1000, rounded down.
    fn bitrate_kbps(self) -> u32 {
        let bits =
            u128::from(self.sample_rate) * u128::from(self.depth) * u128::from(self.channels);
        u32::try_from(bits / 1000).unwrap_or(u32::MAX)
    }
}

/// Reads the `COMM` chunk of an AIFF (or AIFF-C) file, walking the chunks from the start
/// and seeking past the others, so no audio is read.
fn read_aiff<R: Read + Seek>(r: &mut R) -> Result<AiffInfo, String> {
    let io = |e: io::Error| format!("cannot read the AIFF header: {e}");
    let mut form = [0; 12];
    r.read_exact(&mut form)
        .map_err(|_| "not an AIFF file".to_owned())?;
    if &form[..4] != b"FORM" || !matches!(&form[8..], b"AIFF" | b"AIFC") {
        return Err("not an AIFF file".into());
    }
    loop {
        let mut header = [0; 8];
        match r.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                return Err("the AIFF file has no COMM chunk".into());
            }
            Err(e) => return Err(io(e)),
        }
        let size = u32::from_be_bytes(header[4..].try_into().expect("4 bytes"));
        if &header[..4] != b"COMM" {
            // Chunks are padded to an even length.
            let skip = i64::from(size) + i64::from(size & 1);
            r.seek(SeekFrom::Current(skip)).map_err(io)?;
            continue;
        }
        if size < 18 {
            return Err("the AIFF COMM chunk is too short".into());
        }
        let mut comm = [0; 18];
        r.read_exact(&mut comm).map_err(io)?;
        let frames = u32::from_be_bytes(comm[2..6].try_into().expect("4 bytes"));
        let depth = u16::from_be_bytes(comm[6..8].try_into().expect("2 bytes"));
        let rate = comm[8..18].try_into().expect("10 bytes");
        let sample_rate = extended_to_u32(rate)
            .filter(|&rate| rate > 0)
            .ok_or("the AIFF sample rate is not a whole number of Hz")?;
        let channels = u16::from_be_bytes(comm[..2].try_into().expect("2 bytes"));
        return Ok(AiffInfo {
            channels,
            frames,
            sample_rate,
            depth,
        });
    }
}

/// An 80-bit IEEE 754 extended float, as AIFF stores the sample rate, as a whole number;
/// `None` when it is negative, not whole or doesn't fit a `u32`.
fn extended_to_u32(bytes: [u8; 10]) -> Option<u32> {
    if bytes[0] & 0x80 != 0 {
        return None;
    }
    let exponent = i32::from(u16::from_be_bytes([bytes[0], bytes[1]]));
    let mantissa = u64::from_be_bytes(bytes[2..].try_into().expect("8 bytes"));
    if mantissa == 0 {
        return Some(0);
    }
    // value = mantissa · 2^(exponent − 16383 − 63)
    let shift = 16383 + 63 - exponent;
    if !(1..64).contains(&shift) {
        return None;
    }
    let fraction = mantissa & ((1u64 << shift) - 1);
    (fraction == 0).then_some(())?;
    u32::try_from(mantissa >> shift).ok()
}

/// The key text of a track: its Camelot `Tonality`, as given, or empty.
fn key_of(tonality: Option<&str>) -> String {
    tonality.unwrap_or_default().to_owned()
}

/// A track's cues as the builder takes them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Cues {
    hot: Vec<(HotCueSlot, CueSpan)>,
    memory: Vec<CueSpan>,
}

/// A `POSITION_MARK` of the XML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PositionMark {
    /// `Type`: 0 is a cue point, 4 a loop.
    kind: u32,
    /// `Start`, in milliseconds.
    start: Millis,
    /// `Num`: -1 for a memory cue, 0 to 2 for hot cues A to C.
    num: i32,
}

/// The cues of a track's `POSITION_MARK`s, in XML order. The cues come from the timeline;
/// the tests check that the XML agrees.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the tests' cross-check of the timeline's cues")
)]
fn marks_to_cues(marks: &[PositionMark]) -> Result<Cues, String> {
    let mut cues = Cues::default();
    for mark in marks {
        if mark.kind != 0 {
            return Err(format!(
                "a POSITION_MARK of Type {}, not a cue point",
                mark.kind
            ));
        }
        let span = CueSpan::Point(mark.start);
        match mark.num {
            -1 => cues.memory.push(span),
            num => cues.hot.push((hot_slot(i64::from(num))?, span)),
        }
    }
    Ok(cues)
}

/// The hot cue slot of index 0 to 2.
fn hot_slot(index: i64) -> Result<HotCueSlot, String> {
    match index {
        0 => Ok(HotCueSlot::A),
        1 => Ok(HotCueSlot::B),
        2 => Ok(HotCueSlot::C),
        other => Err(format!("hot cue index {other}; only A to C are supported")),
    }
}

/// The cues of a timeline JSON, in its order; labels are dropped.
fn timeline_cues(timeline: &Value) -> Result<Cues, String> {
    let mut cues = Cues::default();
    let list = timeline["cues"]
        .as_array()
        .ok_or("the timeline has no cues")?;
    for cue in list {
        let at = cue["at_ms"]
            .as_u64()
            .and_then(|ms| u32::try_from(ms).ok())
            .ok_or("a timeline cue without a valid at_ms")?;
        let span = CueSpan::Point(Millis::new(at));
        match cue["kind"].as_str() {
            Some("memory") => cues.memory.push(span),
            Some("hot") => {
                let index = cue["index"].as_i64().ok_or("a hot cue without an index")?;
                cues.hot.push((hot_slot(index)?, span));
            }
            _ => return Err("a timeline cue that is neither hot nor memory".into()),
        }
    }
    Ok(cues)
}

/// The grid of a timeline JSON: [`Grid::Constant`] from `first_beat_ms` and `bpm` as given,
/// [`Grid::None`] when `grid` is null.
fn timeline_grid(timeline: &Value) -> Result<Grid, String> {
    let grid = &timeline["grid"];
    if grid.is_null() {
        return Ok(Grid::None);
    }
    if grid["beats_per_bar"].as_u64() != Some(4) {
        return Err("a grid that is not 4 beats to the bar".into());
    }
    let first_beat = grid["first_beat_ms"]
        .as_u64()
        .and_then(|ms| u32::try_from(ms).ok())
        .ok_or("a grid without a valid first_beat_ms")?;
    let bpm = grid["bpm"]
        .as_f64()
        .filter(|bpm| bpm.is_finite() && *bpm > 0.0 && *bpm < 10_000.0)
        .ok_or("a grid without a valid bpm")?;
    // 0 < bpm < 10,000, so the rounded centi-BPM fit a u32.
    let centi = (bpm * 100.0).round() as u32;
    Ok(Grid::Constant {
        first_beat: Millis::new(first_beat),
        tempo: Bpm::from_centi(centi),
    })
}

/// A time in seconds, as the XML writes it (`"225.013"`), in whole milliseconds.
fn seconds_to_ms(text: &str) -> Option<Millis> {
    let seconds: f64 = text.parse().ok()?;
    if !seconds.is_finite() || !(0.0..4_000_000.0).contains(&seconds) {
        return None;
    }
    // 0 <= seconds < 4,000,000, so the rounded milliseconds fit a u32.
    let ms = (seconds * 1000.0).round() as u32;
    Some(Millis::new(ms))
}

/// One line of `manifest.tsv`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestLine {
    /// The audio file's path, relative to the export directory.
    audio: String,
    /// The timeline JSON's path, relative to the export directory.
    timeline: String,
}

/// The lines of `manifest.tsv`, in order; empty lines are skipped.
fn parse_manifest(text: &str) -> Result<Vec<ManifestLine>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(|(index, line)| {
            let (audio, timeline) = line
                .split_once('\t')
                .ok_or_else(|| format!("manifest line {}: no tab", index + 1))?;
            Ok(ManifestLine {
                audio: audio.to_owned(),
                timeline: timeline.to_owned(),
            })
        })
        .collect()
}

/// `text` with its `%XX` escapes decoded; `None` for a bad escape or text that isn't UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = text.get(at + 1..at + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// For each manifest line, in order, the index of the XML track whose decoded `Location`
/// ends in `/` and the line's audio path. Each line must match exactly one track, and each
/// track at most one line.
fn join(manifest: &[ManifestLine], tracks: &[XmlTrack]) -> Result<Vec<usize>, String> {
    let locations: Vec<Option<String>> =
        tracks.iter().map(|t| percent_decode(&t.location)).collect();
    let mut joined = Vec::with_capacity(manifest.len());
    for (index, line) in manifest.iter().enumerate() {
        let number = index + 1;
        let suffix = format!("/{}", line.audio);
        let mut matches = locations
            .iter()
            .enumerate()
            .filter(|(_, l)| l.as_deref().is_some_and(|l| l.ends_with(&suffix)))
            .map(|(at, _)| at);
        let (Some(at), None) = (matches.next(), matches.next()) else {
            return Err(format!(
                "manifest line {number}: not exactly one XML track has its Location"
            ));
        };
        if joined.contains(&at) {
            return Err(format!(
                "manifest line {number}: its XML track is joined to an earlier line too"
            ));
        }
        joined.push(at);
    }
    Ok(joined)
}

/// A `TRACK` of the XML collection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct XmlTrack {
    track_id: String,
    name: String,
    artist: String,
    album: String,
    genre: String,
    label: String,
    comments: String,
    kind: String,
    bitrate: u32,
    year: u16,
    track_number: u32,
    disc_number: u16,
    /// `Rating` as the XML holds it: 0, 51, 102, 153, 204 or 255.
    rating: u32,
    colour: String,
    date_added: String,
    tonality: Option<String>,
    location: String,
    marks: Vec<PositionMark>,
}

/// A playlist of the XML: its name and its tracks' `TrackID`s, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
struct XmlPlaylist {
    name: String,
    keys: Vec<String>,
}

/// What `rekordbox.xml` holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Library {
    tracks: Vec<XmlTrack>,
    playlists: Vec<XmlPlaylist>,
}

/// Parses MDMA's `rekordbox.xml`.
fn parse_xml(text: &str) -> Result<Library, String> {
    let doc = roxmltree::Document::parse(text)
        .map_err(|e| format!("rekordbox.xml does not parse: {e}"))?;
    let root = doc.root_element();
    fn child<'a, 'i>(
        node: roxmltree::Node<'a, 'i>,
        name: &str,
    ) -> Result<roxmltree::Node<'a, 'i>, String> {
        node.children()
            .find(|c| c.has_tag_name(name))
            .ok_or_else(|| format!("rekordbox.xml has no {name}"))
    }
    let mut library = Library::default();
    let collection = child(root, "COLLECTION")?;
    for (index, track) in collection
        .children()
        .filter(|c| c.has_tag_name("TRACK"))
        .enumerate()
    {
        let fail = |what: &str| format!("XML track {}: {what}", index + 1);
        let text = |name: &str| track.attribute(name).unwrap_or_default().to_owned();
        let number = |name: &str| -> Result<u32, String> {
            match track.attribute(name).unwrap_or_default() {
                "" => Ok(0),
                value => value
                    .parse()
                    .map_err(|_| fail(&format!("{name} is no number"))),
            }
        };
        let small = |name: &str| -> Result<u16, String> {
            u16::try_from(number(name)?).map_err(|_| fail(&format!("{name} is too large")))
        };
        let marks = track
            .children()
            .filter(|c| c.has_tag_name("POSITION_MARK"))
            .map(|mark| {
                let kind = mark.attribute("Type").unwrap_or_default().parse();
                let start = mark.attribute("Start").and_then(seconds_to_ms);
                let num = mark.attribute("Num").unwrap_or_default().parse();
                match (kind, start, num) {
                    (Ok(kind), Some(start), Ok(num)) => Ok(PositionMark { kind, start, num }),
                    _ => Err(fail("a POSITION_MARK that doesn't parse")),
                }
            })
            .collect::<Result<_, _>>()?;
        library.tracks.push(XmlTrack {
            track_id: text("TrackID"),
            name: text("Name"),
            artist: text("Artist"),
            album: text("Album"),
            genre: text("Genre"),
            label: text("Label"),
            comments: text("Comments"),
            kind: text("Kind"),
            bitrate: number("BitRate")?,
            year: small("Year")?,
            track_number: number("TrackNumber")?,
            disc_number: small("DiscNumber")?,
            rating: number("Rating")?,
            colour: text("Colour"),
            date_added: text("DateAdded"),
            tonality: track
                .attribute("Tonality")
                .filter(|t| !t.is_empty())
                .map(str::to_owned),
            location: text("Location"),
            marks,
        });
    }
    let playlists = child(root, "PLAYLISTS")?;
    for node in playlists
        .descendants()
        .filter(|n| n.has_tag_name("NODE") && n.attribute("Type") == Some("1"))
    {
        let number = library.playlists.len() + 1;
        if node.attribute("KeyType").unwrap_or("0") != "0" {
            return Err(format!(
                "XML playlist {number}: its tracks are not keyed by TrackID"
            ));
        }
        let keys = node
            .children()
            .filter(|c| c.has_tag_name("TRACK"))
            .map(|t| t.attribute("Key").unwrap_or_default().to_owned())
            .collect();
        library.playlists.push(XmlPlaylist {
            name: node.attribute("Name").unwrap_or_default().to_owned(),
            keys,
        });
    }
    Ok(library)
}

/// A `YYYY-MM-DD` date.
fn parse_date(text: &str) -> Option<ExportDate> {
    let mut parts = text.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    ExportDate::new(year, month, day)
}

/// An MDMA export, ready to build: the builder with every track and playlist added, and
/// each track's audio file.
#[derive(Debug)]
struct Prepared {
    builder: ExportBuilder,
    /// Each track's id and its audio file, in manifest order.
    sources: Vec<(TrackId, PathBuf)>,
    /// The number of playlists added.
    playlists: usize,
}

/// Reads the MDMA export in `dir` and adds its tracks and playlists to a builder. Reads the
/// AIFF headers and the files' metadata, never their audio.
fn prepare(dir: &Path, date: ExportDate) -> Result<Prepared, String> {
    let manifest = fs::read_to_string(dir.join("manifest.tsv"))
        .map_err(|e| format!("cannot read manifest.tsv: {e}"))?;
    let manifest = parse_manifest(&manifest)?;
    let xml = fs::read_to_string(dir.join("rekordbox.xml"))
        .map_err(|e| format!("cannot read rekordbox.xml: {e}"))?;
    let library = parse_xml(&xml)?;
    let joined = join(&manifest, &library.tracks)?;

    let mut builder = ExportBuilder::new(date);
    let mut sources = Vec::new();
    let mut ids = HashMap::new();
    for (index, (line, &at)) in manifest.iter().zip(&joined).enumerate() {
        let number = index + 1;
        let fail = |what: String| format!("track {number}: {what}");
        let xml = &library.tracks[at];
        let audio = dir.join(&line.audio);
        let mut file =
            File::open(&audio).map_err(|e| fail(format!("cannot open the audio: {e}")))?;
        let aiff = read_aiff(&mut file).map_err(fail)?;
        let size = file
            .metadata()
            .map_err(|e| fail(format!("cannot read the audio's metadata: {e}")))?
            .len();
        let timeline = fs::read_to_string(dir.join(&line.timeline))
            .map_err(|e| fail(format!("cannot read the timeline: {e}")))?;
        let timeline: Value = serde_json::from_str(&timeline)
            .map_err(|e| fail(format!("the timeline is not JSON: {e}")))?;
        let file_name = line.audio.rsplit('/').next().unwrap_or_default();
        let track = new_track(xml, aiff, size, file_name, &timeline).map_err(fail)?;
        let id = builder
            .add_track(track)
            .map_err(|e| fail(format!("the builder refuses it: {e}")))?;
        ids.insert(xml.track_id.clone(), id);
        sources.push((id, audio));
    }
    for (index, playlist) in library.playlists.iter().enumerate() {
        let number = index + 1;
        let id = builder
            .add_playlist(&playlist.name, None)
            .map_err(|e| format!("playlist {number}: the builder refuses it: {e}"))?;
        for (entry, key) in playlist.keys.iter().enumerate() {
            let entry = entry + 1;
            let track = ids
                .get(key)
                .ok_or_else(|| format!("playlist {number} entry {entry}: no such track"))?;
            builder
                .add_to_playlist(id, *track)
                .map_err(|e| format!("playlist {number} entry {entry}: {e}"))?;
        }
    }
    Ok(Prepared {
        builder,
        sources,
        playlists: library.playlists.len(),
    })
}

/// The track the builder takes, from its XML tags, AIFF header, file size, file name and
/// timeline.
fn new_track(
    xml: &XmlTrack,
    aiff: AiffInfo,
    file_size: u64,
    file_name: &str,
    timeline: &Value,
) -> Result<NewTrack, String> {
    if xml.kind != "AIFF File" {
        return Err("only AIFF files are supported".into());
    }
    let file_size =
        u32::try_from(file_size).map_err(|_| "the audio file is 4 GiB or larger".to_owned())?;
    // rekordbox XML rates in steps of 51: 0, 51, ..., 255 for 0 to 5 stars.
    if xml.rating % 51 != 0 || xml.rating > 255 {
        return Err(format!(
            "Rating {} is not one of 0, 51, ..., 255",
            xml.rating
        ));
    }
    let rating = u8::try_from(xml.rating / 51).expect("0 to 5");
    if !matches!(xml.colour.as_str(), "" | "0") {
        return Err("track colours are not supported".into());
    }
    let date_added = match xml.date_added.as_str() {
        "" => None,
        date => Some(parse_date(date).ok_or("DateAdded is not a YYYY-MM-DD date")?),
    };
    let cues = timeline_cues(timeline)?;
    Ok(NewTrack {
        title: xml.name.clone(),
        artist: xml.artist.clone(),
        album: xml.album.clone(),
        genre: xml.genre.clone(),
        label: xml.label.clone(),
        key: key_of(xml.tonality.as_deref()),
        comment: xml.comments.clone(),
        year: xml.year,
        track_number: xml.track_number,
        disc_number: xml.disc_number,
        rating,
        file_name: file_name.to_owned(),
        format: AudioFormat::Aiff,
        file_size,
        duration: Millis::new(aiff.duration_ms()),
        sample_rate: aiff.sample_rate,
        sample_depth: aiff.depth,
        // rekordbox's formula, not the XML's BitRate (see docs/format-notes.md).
        bitrate: aiff.bitrate_kbps(),
        grid: timeline_grid(timeline)?,
        hot_cues: cues.hot,
        memory_cues: cues.memory,
        waveform: Waveform::Flat,
        artwork: None,
        date_added,
        ..NewTrack::default()
    })
}

/// The counts of a written stick.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Summary {
    tracks: usize,
    playlists: usize,
    files: usize,
    bytes: u64,
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tracks {}, playlists {}, files {}, bytes {}",
            self.tracks, self.playlists, self.files, self.bytes
        )
    }
}

/// Copies `source` to the new file `target`, creating its directories; never overwrites.
fn copy_new(source: &Path, target: &Path) -> io::Result<u64> {
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut from = File::open(source)?;
    let mut to = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let copied = io::copy(&mut from, &mut to)?;
    to.sync_all()?;
    Ok(copied)
}

/// Builds the stick for the MDMA export in `dir` under the fresh `root`.
fn run(dir: &Path, root: &Path, date: ExportDate) -> Result<Summary, String> {
    for top in ["PIONEER", "Contents"] {
        if root.join(top).exists() {
            return Err(format!("the output root already holds {top}/; refusing"));
        }
    }
    let prepared = prepare(dir, date)?;
    let export: Export = prepared
        .builder
        .build()
        .map_err(|e| format!("the export does not build: {e}"))?;
    export
        .write_to(root)
        .map_err(|e| format!("cannot write the PIONEER tree: {e}"))?;
    let mut files = export.files().count();
    let mut bytes: u64 = export.files().map(|(_, b)| b.len() as u64).sum();
    let sources: HashMap<TrackId, &PathBuf> =
        prepared.sources.iter().map(|(id, p)| (*id, p)).collect();
    for (index, (id, content)) in export.audio_destinations().enumerate() {
        let number = index + 1;
        let source = sources
            .get(&id)
            .ok_or_else(|| format!("track {number}: no audio file"))?;
        let target = ContentPath::as_stick_path(content).under(root);
        bytes += copy_new(source, &target)
            .map_err(|e| format!("track {number}: cannot copy the audio: {e}"))?;
        files += 1;
    }
    Ok(Summary {
        tracks: prepared.sources.len(),
        playlists: prepared.playlists,
        files,
        bytes,
    })
}

fn main() -> ExitCode {
    let usage = "usage: mdma_export <mdma-export dir> <output root> [--date YYYY-MM-DD]";
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (date, rest) = match &args[..] {
        [dir, root] => (
            ExportDate::new(DEFAULT_DATE.0, DEFAULT_DATE.1, DEFAULT_DATE.2),
            [dir, root],
        ),
        [dir, root, flag, date] if flag == "--date" => (parse_date(date), [dir, root]),
        _ => {
            eprintln!("{usage}");
            return ExitCode::from(2);
        }
    };
    let Some(date) = date else {
        eprintln!("mdma_export: --date must be a YYYY-MM-DD date");
        return ExitCode::from(2);
    };
    let [dir, root] = rest;
    match run(Path::new(dir), Path::new(root), date) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("mdma_export: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod common;

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// The 80-bit extended float of `rate`, a whole number of Hz.
    fn extended(rate: u32) -> [u8; 10] {
        let shift = rate.leading_zeros();
        let mantissa = u64::from(rate) << (32 + shift);
        let exponent = 16383 + 31 - u16::try_from(shift).unwrap();
        let mut out = [0; 10];
        out[..2].copy_from_slice(&exponent.to_be_bytes());
        out[2..].copy_from_slice(&mantissa.to_be_bytes());
        out
    }

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    /// A synthetic AIFF: an odd-sized chunk (with its pad byte) before `COMM`, then `SSND`.
    fn aiff(form: &[u8; 4], channels: u16, frames: u32, depth: u16, rate: u32) -> Vec<u8> {
        let mut comm = Vec::new();
        comm.extend_from_slice(&channels.to_be_bytes());
        comm.extend_from_slice(&frames.to_be_bytes());
        comm.extend_from_slice(&depth.to_be_bytes());
        comm.extend_from_slice(&extended(rate));
        if form == b"AIFC" {
            comm.extend_from_slice(b"NONE\x0enot compressed\x00");
        }
        let mut body = form.to_vec();
        body.extend(chunk(b"NAME", b"odd"));
        body.extend(chunk(b"COMM", &comm));
        body.extend(chunk(b"SSND", &[0; 16]));
        chunk(b"FORM", &body)
    }

    #[test]
    fn the_extended_sample_rate_reads_as_a_whole_number() {
        // 44,100 Hz as AIFF files store it.
        let rate = [0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0];
        assert_eq!(extended_to_u32(rate), Some(44_100));
        assert_eq!(extended_to_u32(extended(48_000)), Some(48_000));
        assert_eq!(extended_to_u32(extended(1)), Some(1));
        // 0.5 is not whole; a negative rate is refused.
        let half = [0x3f, 0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(extended_to_u32(half), None);
        let mut negative = extended(44_100);
        negative[0] |= 0x80;
        assert_eq!(extended_to_u32(negative), None);
    }

    #[test]
    fn the_comm_chunk_gives_frames_rate_and_depth() {
        let bytes = aiff(b"AIFF", 2, 441_000, 16, 44_100);
        let info = read_aiff(&mut Cursor::new(bytes)).unwrap();
        let want = AiffInfo {
            channels: 2,
            frames: 441_000,
            sample_rate: 44_100,
            depth: 16,
        };
        assert_eq!(info, want);
        assert_eq!(info.duration_ms(), 10_000);
        let aifc = aiff(b"AIFC", 1, 48_001, 24, 48_000);
        let info = read_aiff(&mut Cursor::new(aifc)).unwrap();
        let got = (info.channels, info.frames, info.sample_rate, info.depth);
        assert_eq!(got, (1, 48_001, 48_000, 24));
        // 1000.02 ms, rounded down.
        assert_eq!(info.duration_ms(), 1000);
    }

    /// rekordbox's bitrate: rate × depth × channels / 1000, rounded down.
    #[test]
    fn the_bitrate_is_rate_times_depth_times_channels() {
        let info = |channels, depth, sample_rate| AiffInfo {
            channels,
            frames: 1,
            sample_rate,
            depth,
        };
        // 1,411.2 and 2,116.8 kbit/s, as rekordbox's rows hold them.
        assert_eq!(info(2, 16, 44_100).bitrate_kbps(), 1411);
        assert_eq!(info(2, 24, 44_100).bitrate_kbps(), 2116);
        assert_eq!(info(1, 24, 48_000).bitrate_kbps(), 1152);
        assert_eq!(info(u16::MAX, u16::MAX, u32::MAX).bitrate_kbps(), u32::MAX);
    }

    #[test]
    fn the_duration_does_not_overflow() {
        let info = AiffInfo {
            channels: 2,
            frames: u32::MAX,
            sample_rate: 44_100,
            depth: 16,
        };
        // 4,294,967,295,000 / 44,100 = 97,391,548.7
        assert_eq!(info.duration_ms(), 97_391_548);
    }

    #[test]
    fn files_that_are_no_aiff_are_refused() {
        let err = read_aiff(&mut Cursor::new(b"RIFF\0\0\0\x04WAVE".to_vec())).unwrap_err();
        assert!(err.contains("not an AIFF"), "{err}");
        let mut no_comm = b"AIFF".to_vec();
        no_comm.extend(chunk(b"SSND", &[0; 8]));
        let err = read_aiff(&mut Cursor::new(chunk(b"FORM", &no_comm))).unwrap_err();
        assert!(err.contains("no COMM"), "{err}");
    }

    #[test]
    fn the_key_is_the_tonality_as_given() {
        assert_eq!(key_of(Some("3A")), "3A");
        assert_eq!(key_of(Some("12B")), "12B");
        assert_eq!(key_of(None), "");
    }

    #[test]
    fn seconds_read_as_whole_milliseconds() {
        assert_eq!(seconds_to_ms("225.013"), Some(Millis::new(225_013)));
        assert_eq!(seconds_to_ms("0.717"), Some(Millis::new(717)));
        assert_eq!(seconds_to_ms("31.999"), Some(Millis::new(31_999)));
        assert_eq!(seconds_to_ms("12"), Some(Millis::new(12_000)));
        assert_eq!(seconds_to_ms("-1.0"), None);
        assert_eq!(seconds_to_ms("soon"), None);
    }

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="6.0.0" Company="AlphaTheta"/>
  <COLLECTION Entries="2">
    <TRACK TrackID="1" Name="T &amp; 1" Artist="A" Album="" Genre="G" Kind="AIFF File" Size="100" TotalTime="288" AverageBpm="80.00" TrackNumber="3" DiscNumber="1" Year="2025" Label="L" Comments="C" Rating="102" Colour="0" DateAdded="2026-05-02" BitRate="1406" SampleRate="44100" Tonality="3A" Location="file://localhost/x/A/Unknown%20Album/T%20%25%201.aiff">
      <TEMPO Inizio="0.000" Bpm="80.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="" Type="0" Start="225.013" Num="-1"/>
      <POSITION_MARK Name="x" Type="0" Start="0.717" Num="0"/>
      <POSITION_MARK Name="" Type="0" Start="183.574" Num="2"/>
    </TRACK>
    <TRACK TrackID="2" Name="Two" Artist="B" Album="Al" Genre="" Kind="AIFF File" Size="200" TotalTime="1" TrackNumber="0" DiscNumber="0" Year="" Label="" Comments="" Rating="0" Colour="0" DateAdded="" BitRate="9" SampleRate="44100" Location="file://localhost/x/B/Al/Tw%C3%B6.aiff"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="1">
      <NODE Name="Folder" Type="0" Count="1">
        <NODE Name="pl" Type="1" KeyType="0" Entries="2">
          <TRACK Key="2"/>
          <TRACK Key="1"/>
        </NODE>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;

    #[test]
    fn the_xml_gives_tracks_marks_and_playlists() {
        let library = parse_xml(XML).unwrap();
        assert_eq!(library.tracks.len(), 2);
        let one = &library.tracks[0];
        assert_eq!(one.track_id, "1");
        assert_eq!(one.name, "T & 1");
        assert_eq!((one.artist.as_str(), one.album.as_str()), ("A", ""));
        assert_eq!((one.genre.as_str(), one.label.as_str()), ("G", "L"));
        assert_eq!(one.comments, "C");
        assert_eq!(one.kind, "AIFF File");
        assert_eq!((one.bitrate, one.year, one.rating), (1406, 2025, 102));
        assert_eq!((one.track_number, one.disc_number), (3, 1));
        assert_eq!(one.date_added, "2026-05-02");
        assert_eq!(one.tonality.as_deref(), Some("3A"));
        let marks =
            [(0, 225_013, -1), (0, 717, 0), (0, 183_574, 2)].map(|(kind, ms, num)| PositionMark {
                kind,
                start: Millis::new(ms),
                num,
            });
        assert_eq!(one.marks, marks);
        let two = &library.tracks[1];
        assert_eq!((two.year, two.tonality.as_deref()), (0, None));
        assert!(two.marks.is_empty());
        let pl = XmlPlaylist {
            name: "pl".into(),
            keys: vec!["2".into(), "1".into()],
        };
        assert_eq!(library.playlists, [pl]);
    }

    #[test]
    fn position_marks_map_to_hot_and_memory_cues_in_order() {
        let mark = |ms, num| PositionMark {
            kind: 0,
            start: Millis::new(ms),
            num,
        };
        let marks = [
            mark(717, -1),
            mark(717, 0),
            mark(500, -1),
            mark(9, 2),
            mark(8, 1),
        ];
        let cues = marks_to_cues(&marks).unwrap();
        let point = |ms| CueSpan::Point(Millis::new(ms));
        let want = Cues {
            hot: vec![
                (HotCueSlot::A, point(717)),
                (HotCueSlot::C, point(9)),
                (HotCueSlot::B, point(8)),
            ],
            memory: vec![point(717), point(500)],
        };
        assert_eq!(cues, want);
        // Hot cue D and loops are not in this export.
        assert!(marks_to_cues(&[mark(1, 3)]).is_err());
        let a_loop = PositionMark {
            kind: 4,
            ..mark(1, -1)
        };
        assert!(marks_to_cues(&[a_loop]).is_err());
    }

    #[test]
    fn timeline_cues_map_to_hot_and_memory_cues_in_order() {
        let timeline: Value = serde_json::from_str(
            r#"{"duration_ms":393375,"cues":[
                {"at_ms":717,"kind":"memory","label":null,"index":null},
                {"at_ms":717,"kind":"hot","label":"Drop","index":0},
                {"at_ms":183574,"kind":"hot","label":null,"index":2},
                {"at_ms":61670,"kind":"memory","label":null,"index":null}
            ],"grid":null}"#,
        )
        .unwrap();
        let point = |ms| CueSpan::Point(Millis::new(ms));
        let want = Cues {
            hot: vec![(HotCueSlot::A, point(717)), (HotCueSlot::C, point(183_574))],
            memory: vec![point(717), point(61_670)],
        };
        assert_eq!(timeline_cues(&timeline).unwrap(), want);
        let bad: Value =
            serde_json::from_str(r#"{"cues":[{"at_ms":1,"kind":"hot","index":3}]}"#).unwrap();
        assert!(timeline_cues(&bad).is_err());
        let bad: Value =
            serde_json::from_str(r#"{"cues":[{"at_ms":1,"kind":"loop","index":null}]}"#).unwrap();
        assert!(timeline_cues(&bad).is_err());
    }

    #[test]
    fn the_timeline_grid_is_constant_from_the_first_beat_as_given() {
        let timeline: Value = serde_json::from_str(
            r#"{"grid":{"bpm":126.0,"beats_per_bar":4,"first_beat_ms":241,"bars_ms":[241,2146]}}"#,
        )
        .unwrap();
        let want = Grid::Constant {
            first_beat: Millis::new(241),
            tempo: Bpm::from_centi(12_600),
        };
        assert_eq!(timeline_grid(&timeline).unwrap(), want);
        let fine: Value =
            serde_json::from_str(r#"{"grid":{"bpm":123.456,"beats_per_bar":4,"first_beat_ms":0}}"#)
                .unwrap();
        let Grid::Constant { tempo, .. } = timeline_grid(&fine).unwrap() else {
            panic!("a constant grid");
        };
        assert_eq!(tempo, Bpm::from_centi(12_346));
        let none: Value = serde_json::from_str(r#"{"grid":null}"#).unwrap();
        assert_eq!(timeline_grid(&none).unwrap(), Grid::None);
        let three: Value =
            serde_json::from_str(r#"{"grid":{"bpm":120.0,"beats_per_bar":3,"first_beat_ms":0}}"#)
                .unwrap();
        assert!(timeline_grid(&three).is_err(), "only 4/4 grids");
    }

    #[test]
    fn the_manifest_reads_as_audio_and_timeline_paths() {
        let lines =
            parse_manifest("A/B/c.aiff\ttimeline/1.json\n\nD/E/f.aiff\ttimeline/2.json\n").unwrap();
        let line = |audio: &str, timeline: &str| ManifestLine {
            audio: audio.into(),
            timeline: timeline.into(),
        };
        assert_eq!(
            lines,
            [
                line("A/B/c.aiff", "timeline/1.json"),
                line("D/E/f.aiff", "timeline/2.json")
            ]
        );
        assert!(parse_manifest("no tab\n").is_err());
    }

    #[test]
    fn percent_escapes_decode_to_utf8() {
        assert_eq!(percent_decode("a%20b").as_deref(), Some("a b"));
        assert_eq!(percent_decode("Tw%C3%B6").as_deref(), Some("Twö"));
        assert_eq!(percent_decode("Garçon").as_deref(), Some("Garçon"));
        assert_eq!(percent_decode("%2").as_deref(), None);
        assert_eq!(percent_decode("%FF").as_deref(), None);
    }

    #[test]
    fn manifest_lines_join_tracks_by_location() {
        let library = parse_xml(XML).unwrap();
        let line = |audio: &str| ManifestLine {
            audio: audio.into(),
            timeline: "timeline/x.json".into(),
        };
        let manifest = [line("B/Al/Twö.aiff"), line("A/Unknown Album/T % 1.aiff")];
        assert_eq!(join(&manifest, &library.tracks).unwrap(), [1, 0]);
        // A path is matched by whole components, not by a suffix of one.
        let err = join(&[line("Al/Twö.aiff"), line("l/Twö.aiff")], &library.tracks);
        assert!(err.is_err());
        let err = join(&[line("Unknown.aiff")], &library.tracks).unwrap_err();
        assert!(err.contains("line 1"), "{err}");
        let twice = [line("B/Al/Twö.aiff"), line("Al/Twö.aiff")];
        assert!(
            join(&twice, &library.tracks).is_err(),
            "one track for two lines"
        );
    }

    #[test]
    fn dates_read_as_yyyy_mm_dd() {
        assert_eq!(parse_date("2026-11-19"), ExportDate::new(2026, 11, 19));
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_date("2026-11"), None);
        assert_eq!(parse_date(""), None);
    }

    fn sample_timeline() -> Value {
        serde_json::from_str(
            r#"{"duration_ms":10000,"cues":[{"at_ms":5,"kind":"hot","label":null,"index":1}],
                "grid":{"bpm":120.0,"beats_per_bar":4,"first_beat_ms":250,"bars_ms":[250]}}"#,
        )
        .unwrap()
    }

    #[test]
    fn a_track_takes_xml_tags_the_aiff_header_and_the_timeline() {
        let library = parse_xml(XML).unwrap();
        let aiff = AiffInfo {
            channels: 2,
            frames: 441_000,
            sample_rate: 44_100,
            depth: 24,
        };
        let track = new_track(
            &library.tracks[0],
            aiff,
            100,
            "T % 1.aiff",
            &sample_timeline(),
        )
        .unwrap();
        let want = NewTrack {
            title: "T & 1".into(),
            artist: "A".into(),
            genre: "G".into(),
            label: "L".into(),
            key: "3A".into(),
            comment: "C".into(),
            year: 2025,
            track_number: 3,
            disc_number: 1,
            rating: 2,
            file_name: "T % 1.aiff".into(),
            format: AudioFormat::Aiff,
            file_size: 100,
            duration: Millis::new(10_000),
            sample_rate: 44_100,
            sample_depth: 24,
            // From the COMM chunk, not the XML's 1406.
            bitrate: 2116,
            grid: Grid::Constant {
                first_beat: Millis::new(250),
                tempo: Bpm::from_centi(12_000),
            },
            hot_cues: vec![(HotCueSlot::B, CueSpan::Point(Millis::new(5)))],
            waveform: Waveform::Flat,
            date_added: ExportDate::new(2026, 5, 2),
            ..NewTrack::default()
        };
        assert_eq!(track, want);
        let two = new_track(
            &library.tracks[1],
            aiff,
            200,
            "Twö.aiff",
            &sample_timeline(),
        )
        .unwrap();
        assert_eq!((two.key.as_str(), two.year, two.date_added), ("", 0, None));
    }

    #[test]
    fn tracks_the_builder_cannot_take_are_refused() {
        let library = parse_xml(XML).unwrap();
        let aiff = AiffInfo {
            channels: 2,
            frames: 1,
            sample_rate: 44_100,
            depth: 16,
        };
        let timeline = sample_timeline();
        let mp3 = XmlTrack {
            kind: "MP3 File".into(),
            ..library.tracks[0].clone()
        };
        assert!(new_track(&mp3, aiff, 1, "a.aiff", &timeline).is_err());
        let big = new_track(
            &library.tracks[0],
            aiff,
            u64::from(u32::MAX) + 1,
            "a.aiff",
            &timeline,
        );
        assert!(big.is_err(), "a file over 4 GiB");
        let rated = XmlTrack {
            rating: 50,
            ..library.tracks[0].clone()
        };
        assert!(new_track(&rated, aiff, 1, "a.aiff", &timeline).is_err());
        let coloured = XmlTrack {
            colour: "0xFF0000".into(),
            ..library.tracks[0].clone()
        };
        assert!(new_track(&coloured, aiff, 1, "a.aiff", &timeline).is_err());
    }

    #[test]
    fn an_output_root_with_a_stick_is_refused() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("local-fixtures/rekordbox-hw1");
        if !root.join("PIONEER").exists() {
            return;
        }
        let date = ExportDate::new(2000, 1, 1).unwrap();
        let err = run(Path::new("/nonexistent"), &root, date).unwrap_err();
        assert!(err.contains("already holds PIONEER/"), "{err}");
    }

    mod local {
        //! Builds the export of `local-fixtures/mdma-export` in memory, without writing it or
        //! its audio, and compares it with rekordbox's export of the same tracks,
        //! `local-fixtures/rekordbox-hw1`. Prints counts, times and shapes only, never names.

        use std::collections::BTreeMap;

        use cdj_media::anlz::{AnlzFile, Beat, CueList};
        use cdj_media::encoding::BeatInBar;
        use cdj_media::layout::{StickPath, export_pdb_path};
        use cdj_media::pdb::model::Track;
        use cdj_media::pdb::{Database, Pdb};

        use super::super::common;
        use super::*;

        /// Equal and differing counts of one field, and the largest difference seen.
        #[derive(Debug, Default)]
        struct Stat {
            equal: usize,
            differ: usize,
            max_delta: Option<u64>,
        }

        impl Stat {
            fn same(&mut self, equal: bool) {
                if equal {
                    self.equal += 1;
                } else {
                    self.differ += 1;
                }
            }

            /// A numeric field: equal within `tolerance`.
            fn delta(&mut self, delta: u64, tolerance: u64) {
                self.same(delta <= tolerance);
                self.max_delta = Some(self.max_delta.map_or(delta, |m| m.max(delta)));
            }
        }

        fn stat<'a>(
            table: &'a mut BTreeMap<&'static str, Stat>,
            field: &'static str,
        ) -> &'a mut Stat {
            table.entry(field).or_default()
        }

        fn load(bytes: &[u8]) -> Database {
            Database::from_pdb(&Pdb::from_bytes(bytes).expect("decode")).expect("load")
        }

        fn diff(a: u64, b: u64) -> u64 {
            a.abs_diff(b)
        }

        fn span_ms(span: CueSpan) -> u64 {
            match span {
                CueSpan::Point(at) | CueSpan::Loop { start: at, .. } => u64::from(at.get()),
                _ => panic!("a cue of an unknown type"),
            }
        }

        /// The hot cues by slot and the sorted memory cue times of a `.DAT`.
        fn cues(dat: &AnlzFile) -> (BTreeMap<u32, u64>, Vec<u64>) {
            let list = |l: Option<&CueList>| l.expect("cue list").cues().to_vec();
            let hot = list(dat.hot_cues())
                .iter()
                .map(|c| (c.hot_cue().expect("slot").to_u32(), span_ms(c.span())))
                .collect();
            let mut memory: Vec<u64> = list(dat.memory_cues())
                .iter()
                .map(|c| span_ms(c.span()))
                .collect();
            memory.sort_unstable();
            (hot, memory)
        }

        /// The beat of `beats`, sorted by time, nearest to `ms`.
        fn nearest(beats: &[Beat], ms: u32) -> &Beat {
            let at = beats.partition_point(|b| b.time().get() < ms);
            let after = beats.get(at);
            let before = at.checked_sub(1).and_then(|i| beats.get(i));
            match (before, after) {
                (Some(b), Some(a)) if ms - b.time().get() <= a.time().get() - ms => b,
                (_, Some(a)) => a,
                (Some(b), None) => b,
                (None, None) => panic!("no beats"),
            }
        }

        /// Our grid against rekordbox's.
        #[derive(Debug, PartialEq, Eq)]
        struct GridMatch {
            /// Ours has one leading beat at ≤ 1 ms that rekordbox lacks, left out of the rest.
            leading: bool,
            /// The largest |Δ| from each of our other beats to rekordbox's nearest.
            max: u64,
            /// How many of our other beats have the bar position of rekordbox's nearest.
            bars_agree: usize,
            /// How many of our beats were compared.
            compared: usize,
        }

        impl GridMatch {
            fn agrees(&self) -> bool {
                self.max <= 1 && self.bars_agree == self.compared
            }
        }

        /// Compares two non-empty grids, beat by beat against rekordbox's nearest. One leading
        /// beat of ours at ≤ 1 ms is left out when rekordbox has no beat within 1 ms of it:
        /// rounding at 0 puts it just above 0 for us and just below for rekordbox.
        fn match_grids(ours: &[Beat], rb: &[Beat]) -> GridMatch {
            let first = ours[0].time().get();
            let leading =
                first <= 1 && diff(first.into(), nearest(rb, first).time().get().into()) > 1;
            let ours = if leading { &ours[1..] } else { ours };
            let mut max = 0;
            let mut bars_agree = 0;
            for b in ours {
                let n = nearest(rb, b.time().get());
                max = max.max(diff(b.time().get().into(), n.time().get().into()));
                bars_agree += usize::from(n.bar_position() == b.bar_position());
            }
            GridMatch {
                leading,
                max,
                bars_agree,
                compared: ours.len(),
            }
        }

        /// A grid at 120 BPM (500 ms a beat) of `count` beats from `first` ms, bar position
        /// `bar` (1 to 4) first.
        fn grid(first: u32, bar: u16, count: u16) -> Vec<Beat> {
            (0..count)
                .map(|k| {
                    Beat::new(
                        BeatInBar::new((bar - 1 + k) % 4 + 1).unwrap(),
                        Bpm::from_centi(12_000),
                        Millis::new(first + 500 * u32::from(k)),
                    )
                })
                .collect()
        }

        #[test]
        fn equal_grids_agree() {
            let m = match_grids(&grid(10, 2, 8), &grid(10, 2, 8));
            assert_eq!(
                (m.leading, m.max, m.bars_agree, m.compared),
                (false, 0, 8, 8)
            );
            assert!(m.agrees());
            let off = match_grids(&grid(10, 2, 8), &grid(10, 3, 8));
            assert!(!off.agrees(), "bar positions differ");
            let late = match_grids(&grid(10, 2, 8), &grid(12, 2, 8));
            assert!(!late.agrees(), "2 ms apart");
        }

        #[test]
        fn one_leading_beat_at_0_that_rekordbox_lacks_is_left_out() {
            let ours = grid(0, 1, 9);
            let rb = grid(500, 2, 8);
            let m = match_grids(&ours, &rb);
            assert_eq!(
                (m.leading, m.max, m.bars_agree, m.compared),
                (true, 0, 8, 8)
            );
            assert!(m.agrees());
            let one = match_grids(&grid(1, 1, 9), &grid(501, 2, 8));
            assert!(one.leading && one.agrees(), "at 1 ms");
        }

        #[test]
        fn a_leading_beat_later_than_1_ms_is_refused() {
            let m = match_grids(&grid(5, 1, 9), &grid(505, 2, 8));
            assert!(!m.leading);
            assert!(!m.agrees());
        }

        #[test]
        fn only_one_leading_beat_is_left_out() {
            // Ours has two beats, at 0 and 500 ms, before rekordbox's first: the one at
            // 500 ms is compared and differs.
            let m = match_grids(&grid(0, 1, 10), &grid(1000, 3, 8));
            assert!(!m.agrees());
        }

        #[test]
        fn a_leading_beat_rekordbox_also_has_is_compared() {
            let m = match_grids(&grid(0, 1, 8), &grid(1, 1, 8));
            assert_eq!((m.leading, m.max, m.compared), (false, 1, 8));
            assert!(m.agrees());
        }

        /// The Camelot code of a key name in standard notation (`Abm`, `C`); Camelot codes
        /// and unknown names as they are.
        fn camelot(name: &str) -> &str {
            const TABLE: [(&str, &str); 34] = [
                ("Abm", "1A"),
                ("G#m", "1A"),
                ("Ebm", "2A"),
                ("D#m", "2A"),
                ("Bbm", "3A"),
                ("A#m", "3A"),
                ("Fm", "4A"),
                ("Cm", "5A"),
                ("Gm", "6A"),
                ("Dm", "7A"),
                ("Am", "8A"),
                ("Em", "9A"),
                ("Bm", "10A"),
                ("F#m", "11A"),
                ("Gbm", "11A"),
                ("Dbm", "12A"),
                ("C#m", "12A"),
                ("B", "1B"),
                ("F#", "2B"),
                ("Gb", "2B"),
                ("Db", "3B"),
                ("C#", "3B"),
                ("Ab", "4B"),
                ("G#", "4B"),
                ("Eb", "5B"),
                ("D#", "5B"),
                ("Bb", "6B"),
                ("A#", "6B"),
                ("F", "7B"),
                ("C", "8B"),
                ("G", "9B"),
                ("D", "10B"),
                ("A", "11B"),
                ("E", "12B"),
            ];
            TABLE
                .iter()
                .find(|(standard, _)| *standard == name)
                .map_or(name, |(_, code)| code)
        }

        #[test]
        fn key_names_map_to_camelot() {
            assert_eq!(camelot("Abm"), "1A");
            assert_eq!(camelot("G#m"), "1A");
            assert_eq!(camelot("Cm"), "5A");
            assert_eq!(camelot("C"), "8B");
            assert_eq!(camelot("F#"), "2B");
            assert_eq!(camelot("Gb"), "2B");
            assert_eq!(camelot("E"), "12B");
            assert_eq!(camelot("C#m"), "12A");
            assert_eq!(camelot("3A"), "3A");
            assert_eq!(camelot(""), "");
        }

        fn name<T>(of: Option<&T>, name: impl Fn(&T) -> &str) -> String {
            of.map_or(String::new(), |x| name(x).to_owned())
        }

        /// The names of a track's artist, album, genre, label and key.
        fn names(db: &Database, t: &Track) -> [String; 5] {
            [
                name(db.artist_of(t), |a| a.name()),
                name(db.album_of(t), |a| a.name()),
                name(db.genre_of(t), |g| g.name()),
                name(db.label_of(t), |l| l.name()),
                name(db.key_of(t), |k| k.name()),
            ]
        }

        #[test]
        fn the_mdma_export_builds_and_compares_with_rekordbox() {
            let (Some(dir), Some(rb_root)) = (
                common::local_fixture("mdma-export"),
                common::local_fixture("rekordbox-hw1"),
            ) else {
                return;
            };
            let date = ExportDate::new(2026, 11, 19).unwrap();
            let prepared = prepare(&dir, date).expect("prepares");
            let export = prepared.builder.build().expect("the export builds");
            let ours = load(export.file(&export_pdb_path()).expect("export.pdb"));
            let manifest =
                parse_manifest(&fs::read_to_string(dir.join("manifest.tsv")).unwrap()).unwrap();
            let library =
                parse_xml(&fs::read_to_string(dir.join("rekordbox.xml")).unwrap()).unwrap();
            assert_eq!(library.tracks.len(), manifest.len(), "XML tracks");
            let xml_entries: Vec<usize> = library.playlists.iter().map(|p| p.keys.len()).collect();
            assert_eq!(xml_entries, [manifest.len()], "XML playlist entries");
            assert_eq!(ours.tracks().len(), manifest.len(), "tracks");
            assert_eq!(ours.playlists().len(), 1, "playlists");
            let entries = ours
                .playlists()
                .map(|p| ours.playlist_tracks(p.id()).count());
            assert_eq!(
                entries.collect::<Vec<_>>(),
                [manifest.len()],
                "playlist entries"
            );
            let dat_of = |id: TrackId| {
                let row = ours.track(id).expect("the track row");
                let path = StickPath::parse(row.analysis_path()).expect("a stick path");
                let bytes = export.file(&path).expect(".DAT");
                AnlzFile::from_bytes(bytes).expect("our .DAT decodes")
            };
            for t in ours.tracks() {
                let dat = dat_of(t.id());
                assert!(
                    dat.beat_grid().is_some(),
                    "every track has a .DAT with a PQTZ"
                );
            }
            eprintln!(
                "built {} tracks, {} playlists, {} files",
                ours.tracks().len(),
                ours.playlists().len(),
                export.files().count()
            );

            // The XML's POSITION_MARKs against the timelines' cues, and the timelines'
            // durations against the AIFF headers'.
            let joined = join(&manifest, &library.tracks).unwrap();
            let mut marks_agree = 0;
            let mut duration_delta = Stat::default();
            let mut first_beats = BTreeMap::new();
            let mut durations = BTreeMap::new();
            for ((line, &at), (id, _)) in manifest.iter().zip(&joined).zip(&prepared.sources) {
                let timeline: Value =
                    serde_json::from_str(&fs::read_to_string(dir.join(&line.timeline)).unwrap())
                        .unwrap();
                let xml = marks_to_cues(&library.tracks[at].marks).unwrap();
                marks_agree += usize::from(xml == timeline_cues(&timeline).unwrap());
                let ms = timeline["duration_ms"].as_u64().unwrap();
                let aiff = read_aiff(&mut File::open(dir.join(&line.audio)).unwrap()).unwrap();
                duration_delta.delta(diff(ms, u64::from(aiff.duration_ms())), 0);
                durations.insert(*id, aiff.duration_ms());
                if let Grid::Constant { first_beat, .. } = timeline_grid(&timeline).unwrap() {
                    first_beats.insert(*id, first_beat);
                }
            }
            eprintln!(
                "XML marks equal to timeline cues: {marks_agree} of {}",
                manifest.len()
            );
            eprintln!("timeline duration_ms vs AIFF header: {duration_delta:?}");

            // Against rekordbox, matched by title.
            let rb = load(&fs::read(rb_root.join("PIONEER/rekordbox/export.pdb")).unwrap());
            let mut table: BTreeMap<&str, Stat> = BTreeMap::new();
            let mut matched = 0;
            let mut grid_rows = Vec::new();
            let mut detail_rows = Vec::new();
            let mut grids_differ = Vec::new();
            // Tracks whose grid starts with a beat at 0 ms that rekordbox lacks, under
            // "equal" (always shown).
            stat(&mut table, "leading beat at 0");
            for r in rb.tracks() {
                let Some(o) = ours.tracks().find(|o| o.title() == r.title()) else {
                    continue;
                };
                matched += 1;
                let (of, rf) = (o.to_fields(), r.to_fields());
                stat(&mut table, "duration s")
                    .delta(diff(of.duration.into(), rf.duration.into()), 0);
                stat(&mut table, "tempo (centi-BPM)")
                    .delta(diff(of.tempo.centi().into(), rf.tempo.centi().into()), 0);
                stat(&mut table, "sample rate")
                    .delta(diff(of.sample_rate.into(), rf.sample_rate.into()), 0);
                stat(&mut table, "sample depth")
                    .delta(diff(of.sample_depth.into(), rf.sample_depth.into()), 0);
                stat(&mut table, "bitrate").delta(diff(of.bitrate.into(), rf.bitrate.into()), 0);
                stat(&mut table, "file size")
                    .delta(diff(of.file_size.into(), rf.file_size.into()), 0);
                stat(&mut table, "year").delta(diff(of.year.into(), rf.year.into()), 0);
                stat(&mut table, "track number")
                    .delta(diff(of.track_number.into(), rf.track_number.into()), 0);
                let (on, rn) = (names(&ours, o), names(&rb, r));
                detail_rows.push(format!(
                    "track {}: duration ours {} s ({} ms) rekordbox {} s; depth ours {} rekordbox \
                     {}; bitrate ours {} rekordbox {}; size ours {} rekordbox {}; rekordbox file \
                     type {:?}; key ours {:?} rekordbox {:?}; label ours {} rekordbox {}",
                    o.id().get(),
                    of.duration,
                    durations[&o.id()],
                    rf.duration,
                    of.sample_depth,
                    rf.sample_depth,
                    of.bitrate,
                    rf.bitrate,
                    of.file_size,
                    rf.file_size,
                    rf.file_type,
                    on[4],
                    rn[4],
                    if on[3].is_empty() { "empty" } else { "set" },
                    if rn[3].is_empty() { "empty" } else { "set" },
                ));
                let fields = ["artist", "album", "genre", "label", "key"];
                for (field, (a, b)) in fields.into_iter().zip(on.iter().zip(rn.iter())) {
                    stat(&mut table, field).same(a == b);
                }
                stat(&mut table, "key after notation").same(camelot(&on[4]) == camelot(&rn[4]));

                let rb_dat = AnlzFile::from_bytes(
                    &fs::read(rb_root.join(r.analysis_path().trim_start_matches('/'))).unwrap(),
                )
                .unwrap();
                let our_dat = dat_of(o.id());
                let ((oh, om), (rh, rm)) = (cues(&our_dat), cues(&rb_dat));
                stat(&mut table, "hot cue count").delta(diff(oh.len() as u64, rh.len() as u64), 0);
                stat(&mut table, "memory cue count")
                    .delta(diff(om.len() as u64, rm.len() as u64), 0);
                for (slot, at) in &oh {
                    match rh.get(slot) {
                        Some(rb_at) => {
                            stat(&mut table, "hot cue ms (±1)").delta(diff(*at, *rb_at), 1)
                        }
                        None => stat(&mut table, "hot cue ms (±1)").same(false),
                    }
                }
                if om.len() == rm.len() {
                    for (a, b) in om.iter().zip(&rm) {
                        stat(&mut table, "memory cue ms (±1)").delta(diff(*a, *b), 1);
                    }
                } else {
                    stat(&mut table, "memory cue ms (±1)").same(false);
                }

                let our_beats = our_dat.beat_grid().expect("PQTZ").beats();
                let rb_beats = rb_dat.beat_grid().expect("PQTZ").beats();
                stat(&mut table, "beat count")
                    .delta(diff(our_beats.len() as u64, rb_beats.len() as u64), 0);
                if our_beats.is_empty() || rb_beats.is_empty() {
                    grid_rows.push(format!(
                        "track {}: beats ours {} rekordbox {}",
                        o.id().get(),
                        our_beats.len(),
                        rb_beats.len()
                    ));
                    continue;
                }
                let m = match_grids(our_beats, rb_beats);
                let GridMatch {
                    leading,
                    max,
                    bars_agree,
                    compared,
                } = m;
                if leading {
                    stat(&mut table, "leading beat at 0").same(true);
                }
                stat(&mut table, "beat ms (±1)").delta(max, 1);
                stat(&mut table, "beat bar positions").same(bars_agree == compared);
                if !m.agrees() {
                    grids_differ.push(o.id().get());
                }
                let first = first_beats[&o.id()];
                let at_first = nearest(rb_beats, first.get());
                grid_rows.push(format!(
                    "track {}: beats ours {} rekordbox {}; first beat ours {} ms, rekordbox {} ms \
                     (beat {}); MDMA first_beat {} ms is rekordbox beat {} ({:+} ms); max |Δ| {} ms; \
                     bar positions agree {}/{}{}",
                    o.id().get(),
                    our_beats.len(),
                    rb_beats.len(),
                    our_beats[0].time().get(),
                    rb_beats[0].time().get(),
                    rb_beats[0].bar_position().number(),
                    first.get(),
                    at_first.bar_position().number(),
                    i64::from(at_first.time().get()) - i64::from(first.get()),
                    max,
                    bars_agree,
                    compared,
                    if leading {
                        "; our leading beat at 0 left out"
                    } else {
                        ""
                    },
                ));
            }
            eprintln!(
                "matched {matched} of {} rekordbox tracks by title",
                rb.tracks().len()
            );
            eprintln!(
                "{:<22} {:>5} {:>7} {:>9}",
                "field", "equal", "differ", "max |Δ|"
            );
            for (field, s) in &table {
                let max = s.max_delta.map_or("-".to_owned(), |m| m.to_string());
                eprintln!("{field:<22} {:>5} {:>7} {max:>9}", s.equal, s.differ);
            }
            for row in detail_rows.iter().chain(&grid_rows) {
                eprintln!("{row}");
            }
            assert_eq!(
                matched, 6,
                "the rekordbox tracks are all in the MDMA export"
            );
            assert!(
                grids_differ.is_empty(),
                "tracks whose beats differ from rekordbox's by more than 1 ms or whose bar \
                 positions differ: {grids_differ:?}"
            );
        }
    }
}
