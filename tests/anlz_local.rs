//! Round-trips every ANLZ file on a real stick in `local-fixtures/`, and checks the typed
//! tags' builders and defaults against it. Skips itself when the fixture is missing.

mod common;

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use cdj_media::anlz::{
    AnlzFile, AudioPath, BeatGrid, Cue, CueList, CueListKind, CueSpan, FourCc, HotCueSlot, Tag,
    TinyWaveformPreview, WaveformDetail, WaveformPreview,
};
use cdj_media::encoding::Millis;
use cdj_media::pdb::{Database, Pdb};

/// Extensions of ANLZ files, upper case.
const EXTENSIONS: [&str; 3] = ["DAT", "EXT", "2EX"];

/// Returns the upper-case extension of `path` if it names an ANLZ file.
fn anlz_extension(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?.to_ascii_uppercase();
    let ext = path.extension()?.to_str()?.to_ascii_uppercase();
    let is_anlz = EXTENSIONS.contains(&ext.as_str()) && (ext != "DAT" || name.starts_with("ANLZ"));
    is_anlz.then_some(ext)
}

/// Collects every ANLZ file under `dir`, sorted, so failures are reported in a stable order.
fn anlz_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read directory {}: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if let Some(ext) = anlz_extension(&path) {
                found.push((path, ext));
            }
        }
    }
    found.sort();
    found
}

/// Counts of typed tags and cues across the stick.
#[derive(Default)]
struct Counts {
    paths: usize,
    beat_grids: usize,
    cue_lists: usize,
    beats: usize,
    hot_cues: usize,
    memory_cues: usize,
    loops: usize,
    previews: usize,
    tiny_previews: usize,
    details: usize,
}

/// Decodes and re-encodes one file and checks its typed tags; returns its tag codes, or why
/// it failed.
fn round_trip(path: &Path, ext: &str, counts: &mut Counts) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let file = AnlzFile::from_bytes(&bytes).map_err(|e| format!("decode: {e}"))?;
    let encoded = file.to_bytes().map_err(|e| format!("encode: {e}"))?;
    if encoded != bytes {
        let first_diff = encoded
            .iter()
            .zip(&bytes)
            .position(|(a, b)| a != b)
            .unwrap_or(encoded.len().min(bytes.len()));
        return Err(format!(
            "re-encoded bytes differ: {} bytes vs {} original, first difference at {first_diff:#x}",
            encoded.len(),
            bytes.len()
        ));
    }

    // A typed code must never fall back to `Raw`, or this test could pass trivially.
    for tag in file.tags() {
        match tag {
            Tag::Raw(raw) if raw.fourcc().is_typed() => {
                return Err(format!("{} decoded as Raw", raw.fourcc()));
            }
            Tag::Path(_) => counts.paths += 1,
            Tag::BeatGrid(grid) => {
                counts.beat_grids += 1;
                counts.beats += grid.beats().len();
            }
            Tag::Cues(_) => counts.cue_lists += 1,
            Tag::WaveformPreview(_) => counts.previews += 1,
            Tag::TinyWaveformPreview(_) => counts.tiny_previews += 1,
            Tag::WaveformDetail(_) => counts.details += 1,
            _ => {}
        }
    }
    if ext == "EXT" && file.waveform_detail().is_none() {
        return Err("no PWV3 waveform detail".into());
    }

    if ext == "DAT" {
        if file.path().is_none() {
            return Err("no PPTH path".into());
        }
        if file.beat_grid().is_none() {
            return Err("no PQTZ beat grid".into());
        }
        if file.waveform_preview().is_none() || file.tiny_waveform_preview().is_none() {
            return Err("no PWAV or PWV2 waveform preview".into());
        }
        let hot = file.hot_cues().ok_or("no hot cue list")?;
        let memory = file.memory_cues().ok_or("no memory cue list")?;
        counts.hot_cues += hot.cues().len();
        counts.memory_cues += memory.cues().len();
        counts.loops += hot
            .cues()
            .iter()
            .chain(memory.cues())
            .filter(|cue| matches!(cue.span(), CueSpan::Loop { .. }))
            .count();
    }
    Ok(file.tags().iter().map(|t| t.fourcc().to_string()).collect())
}

#[test]
fn real_stick_anlz_files_round_trip_identically() {
    let Some(dir) = common::local_fixture("real-stick/PIONEER/USBANLZ") else {
        return;
    };
    let files = anlz_files(&dir);
    assert!(!files.is_empty(), "no ANLZ files under {}", dir.display());

    let mut per_extension: BTreeMap<&str, usize> = BTreeMap::new();
    let mut tags_per_extension: BTreeMap<(&str, String), usize> = BTreeMap::new();
    let mut counts = Counts::default();
    let mut failures = Vec::new();
    for (path, ext) in &files {
        *per_extension.entry(ext.as_str()).or_default() += 1;
        match round_trip(path, ext, &mut counts) {
            Ok(codes) => {
                for code in codes {
                    *tags_per_extension.entry((ext.as_str(), code)).or_default() += 1;
                }
            }
            Err(why) => failures.push(format!("{}: {why}", path.display())),
        }
    }

    eprintln!(
        "ANLZ round-trip: {} files, {} failures",
        files.len(),
        failures.len()
    );
    for (ext, count) in &per_extension {
        eprintln!("  .{ext}: {count} files");
    }
    for ((ext, code), count) in &tags_per_extension {
        eprintln!("  .{ext} {code}: {count}");
    }
    eprintln!(
        "typed tags: {} PPTH, {} PQTZ ({} beats), {} PCOB",
        counts.paths, counts.beat_grids, counts.beats, counts.cue_lists
    );
    eprintln!(
        ".DAT cues: {} hot, {} memory, {} loops",
        counts.hot_cues, counts.memory_cues, counts.loops
    );
    eprintln!(
        "typed waveforms: {} PWAV, {} PWV2, {} PWV3",
        counts.previews, counts.tiny_previews, counts.details
    );
    // One of each per file, and none fell back to `Raw` (checked per tag above).
    let files_with = |ext| per_extension.get(ext).copied().unwrap_or(0);
    assert_eq!(
        (counts.previews, counts.tiny_previews, counts.details),
        (files_with("DAT"), files_with("DAT"), files_with("EXT")),
        "typed waveform tags per file"
    );
    assert!(
        failures.is_empty(),
        "{} of {} files failed to round-trip:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

/// Decodes every `.DAT` file on the stick, or returns `None` when the fixture is missing.
fn dat_files() -> Option<Vec<(PathBuf, AnlzFile)>> {
    let dir = common::local_fixture("real-stick/PIONEER/USBANLZ")?;
    let files: Vec<_> = anlz_files(&dir)
        .into_iter()
        .filter(|(_, ext)| ext == "DAT")
        .map(|(path, _)| {
            let bytes = std::fs::read(&path).expect("read");
            let file = AnlzFile::from_bytes(&bytes).expect("decode");
            (path, file)
        })
        .collect();
    assert!(!files.is_empty(), "no .DAT files under {}", dir.display());
    Some(files)
}

/// The start time of a cue, for checking the order of a list.
fn start_ms(span: CueSpan) -> u32 {
    match span {
        CueSpan::Point(time) | CueSpan::Loop { start: time, .. } => time.get(),
        CueSpan::Other { time, .. } => time,
        other => panic!("unexpected span {other:?}"),
    }
}

/// Order links and `memory_count` of a list, the fields its builder computes.
fn links(list: &CueList) -> (u32, Vec<(u16, u16)>) {
    let orders = list
        .cues()
        .iter()
        .map(|cue| (cue.order_first(), cue.order_last()))
        .collect();
    (list.memory_count(), orders)
}

/// Rebuilds every hot cue and memory point list from domain data only (slots and spans) and
/// checks that the builders compute rekordbox's order links and `memory_count`.
#[test]
fn real_stick_cue_list_builders_match_rekordbox() {
    let Some(files) = dat_files() else {
        return;
    };
    let mut failures = Vec::new();
    let (mut lists, mut cues, mut cues_equal, mut lists_equal) = (0, 0, 0, 0);
    let (mut ordered, mut unordered) = (0, 0);
    for (path, file) in &files {
        let hot = file.hot_cues().expect("hot cue list");
        let memory = file.memory_cues().expect("memory cue list");
        let slots: Option<Vec<(HotCueSlot, CueSpan)>> = hot
            .cues()
            .iter()
            .map(|cue| Some((cue.hot_cue()?, cue.span())))
            .collect();
        let Some(slots) = slots else {
            failures.push(format!("{}: hot cue without a slot", path.display()));
            continue;
        };
        let rebuilt_hot = CueList::hot(slots);
        let rebuilt_memory = CueList::memory(memory.cues().iter().map(Cue::span));

        for (decoded, rebuilt) in [(hot, &rebuilt_hot), (memory, &rebuilt_memory)] {
            lists += 1;
            cues += decoded.cues().len();
            if links(decoded) != links(rebuilt) {
                failures.push(format!(
                    "{}: {:?} list links {:?}, rebuilt {:?}",
                    path.display(),
                    decoded.kind(),
                    links(decoded),
                    links(rebuilt)
                ));
            }
            cues_equal += decoded
                .cues()
                .iter()
                .zip(rebuilt.cues())
                .filter(|(a, b)| a == b)
                .count();
            lists_equal += usize::from(decoded == rebuilt);
        }

        if memory.cues().len() > 1 {
            let times: Vec<u32> = memory.cues().iter().map(|c| start_ms(c.span())).collect();
            if times.windows(2).all(|w| w[0] <= w[1]) {
                ordered += 1;
            } else {
                unordered += 1;
            }
        }
    }

    eprintln!(
        "rebuilt {lists} cue lists from slots and spans: {lists_equal} equal in full; \
         {cues_equal} of {cues} cues equal in full, defaults included"
    );
    eprintln!("memory point lists with 2+ cues: {ordered} in time order, {unordered} not");
    assert!(
        failures.is_empty(),
        "{} lists differ from their rebuilt links:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The typed contents [`AnlzFile::track_dat`] takes, cloned from a decoded `.DAT` file.
struct DatParts {
    path: AudioPath,
    grid: BeatGrid,
    hot: CueList,
    memory: CueList,
    preview: WaveformPreview,
    tiny: TinyWaveformPreview,
}

impl DatParts {
    fn of(file: &AnlzFile) -> Option<Self> {
        let path = file.tags().iter().find_map(|tag| match tag {
            Tag::Path(path) => Some(path.clone()),
            _ => None,
        })?;
        Some(Self {
            path,
            grid: file.beat_grid()?.clone(),
            hot: file.hot_cues()?.clone(),
            memory: file.memory_cues()?.clone(),
            preview: file.waveform_preview()?.clone(),
            tiny: file.tiny_waveform_preview()?.clone(),
        })
    }

    fn build(self) -> AnlzFile {
        AnlzFile::track_dat(
            self.path,
            self.grid,
            self.hot,
            self.memory,
            self.preview,
            self.tiny,
        )
        .expect("hot and memory lists")
    }
}

/// Why a rebuilt `.DAT` differs from rekordbox's, or that it doesn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rebuild {
    Identical,
    /// Only the not-understood `PMAI` header bytes differ.
    PmaiHeader,
    /// Only the `PVBR` seek index differs (MP3 tracks have one).
    Pvbr,
    PmaiHeaderAndPvbr,
    Other,
}

/// Rebuilds `file` from its typed contents with [`AnlzFile::track_dat`] and classifies the
/// result against `bytes`, the original.
fn rebuild(file: &AnlzFile, bytes: &[u8]) -> Rebuild {
    let Some(parts) = DatParts::of(file) else {
        return Rebuild::Other;
    };
    let built = parts.build();
    if built.to_bytes().expect("encodes") == bytes {
        return Rebuild::Identical;
    }
    let vbr = |f: &AnlzFile| f.tags_with(FourCc::PVBR).next().cloned();
    let header_differs = built.unknown_header() != file.unknown_header();
    let vbr_differs = vbr(&built) != vbr(file);
    // With rekordbox's header and seek index put back, nothing else may differ.
    let patched: Vec<Tag> = built
        .tags()
        .iter()
        .map(|tag| match tag.fourcc() {
            FourCc::PVBR => vbr(file).expect("original PVBR"),
            _ => tag.clone(),
        })
        .collect();
    let patched = AnlzFile::new(file.unknown_header().to_vec(), patched);
    if patched.to_bytes().expect("encodes") != bytes {
        return Rebuild::Other;
    }
    match (header_differs, vbr_differs) {
        (true, false) => Rebuild::PmaiHeader,
        (false, true) => Rebuild::Pvbr,
        (true, true) => Rebuild::PmaiHeaderAndPvbr,
        (false, false) => Rebuild::Other,
    }
}

/// Acceptance test for [`AnlzFile::track_dat`]: rebuilds every `.DAT` from its typed
/// contents and compares bytes. Prints counts by category only.
#[test]
fn real_stick_dat_files_rebuild_identically() {
    let Some(dir) = common::local_fixture("real-stick/PIONEER/USBANLZ") else {
        return;
    };
    let mut outcomes: BTreeMap<Rebuild, usize> = BTreeMap::new();
    let mut from_domain = 0;
    let mut total = 0;
    for (path, ext) in anlz_files(&dir) {
        if ext != "DAT" {
            continue;
        }
        total += 1;
        let bytes = std::fs::read(&path).expect("read");
        let file = AnlzFile::from_bytes(&bytes).expect("decode");
        *outcomes.entry(rebuild(&file, &bytes)).or_default() += 1;

        // Also from domain data only: cue slots and spans, not the decoded lists.
        if let Some(mut parts) = DatParts::of(&file) {
            let slots: Option<Vec<_>> = parts
                .hot
                .cues()
                .iter()
                .map(|cue| Some((cue.hot_cue()?, cue.span())))
                .collect();
            parts.hot = CueList::hot(slots.expect("hot cues have slots"));
            parts.memory = CueList::memory(parts.memory.cues().iter().map(Cue::span));
            from_domain += usize::from(parts.build().to_bytes().expect("encodes") == bytes);
        }
    }
    eprintln!("rebuilt {total} .DAT files with AnlzFile::track_dat:");
    for (outcome, count) in &outcomes {
        eprintln!("  {count:5} {outcome:?}");
    }
    eprintln!("  {from_domain} identical with cue lists rebuilt from slots and spans");
    // Pinned to the current fixture. The 30 header-only files are the 22 stale `.DAT`
    // files without an `.EXT` plus 8 AIFF tracks whose first header word is 0; the 25 are
    // the MP3 tracks, whose first header word is 0 and whose PVBR holds a seek index.
    let expected: BTreeMap<Rebuild, usize> = [
        (Rebuild::Identical, 793),
        (Rebuild::PmaiHeader, 30),
        (Rebuild::PmaiHeaderAndPvbr, 25),
    ]
    .into();
    assert_eq!(outcomes, expected, "rebuild outcomes");
    assert_eq!(
        from_domain, 793,
        "identical with cue lists from slots and spans"
    );
}

/// Survey of tag order and file headers by extension; checks that every `.DAT` has the
/// order [`AnlzFile::track_dat`] writes. Prints counts and shapes only.
#[test]
fn real_stick_anlz_tag_orders() {
    let Some(dir) = common::local_fixture("real-stick/PIONEER/USBANLZ") else {
        return;
    };
    let built = DatParts {
        path: AudioPath::new("/a"),
        grid: BeatGrid::new(vec![]),
        hot: CueList::hot([]),
        memory: CueList::memory([]),
        preview: WaveformPreview::flat(0),
        tiny: TinyWaveformPreview::flat(0),
    }
    .build();
    let order = |file: &AnlzFile| {
        let codes: Vec<String> = file.tags().iter().map(|t| t.fourcc().to_string()).collect();
        codes.join(",")
    };
    let built_order = order(&built);

    let mut orders: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut headers: BTreeMap<(String, Vec<u8>), usize> = BTreeMap::new();
    let mut dat_in_built_order = 0;
    let mut dat_files = 0;
    let detail_default = WaveformDetail::new(vec![]).unknown1();
    let (mut ext_files, mut details_with_default) = (0, 0);
    for (path, ext) in anlz_files(&dir) {
        let bytes = std::fs::read(&path).expect("read");
        let file = AnlzFile::from_bytes(&bytes).expect("decode");
        let file_order = order(&file);
        if ext == "DAT" {
            dat_files += 1;
            dat_in_built_order += usize::from(file_order == built_order);
        }
        if ext == "EXT" {
            ext_files += 1;
            let unknown1 = file.waveform_detail().map(WaveformDetail::unknown1);
            details_with_default += usize::from(unknown1 == Some(detail_default));
        }
        *orders.entry((ext.clone(), file_order)).or_default() += 1;
        *headers
            .entry((ext, file.unknown_header().to_vec()))
            .or_default() += 1;
    }
    eprintln!("tag orders:");
    for ((ext, order), count) in &orders {
        eprintln!("  {count:5} .{ext} {order}");
    }
    eprintln!("PMAI header bytes 12..:");
    for ((ext, header), count) in &headers {
        eprintln!("  {count:5} .{ext} {header:02x?}");
    }
    eprintln!("PWV3 unknown1 {detail_default:#010x} in {details_with_default} of {ext_files} .EXT");
    assert_eq!(
        dat_in_built_order, dat_files,
        ".DAT files in the order track_dat writes ({built_order})"
    );
    assert_eq!(details_with_default, ext_files, "PWV3 unknown1 default");
}

/// A track of a stick with both a `.DAT` and an `.EXT`, matched through its row's analysis
/// path.
struct ExtPair {
    /// The row's duration, whole seconds.
    seconds: u16,
    dat: AnlzFile,
    ext: AnlzFile,
}

/// Every track of the stick at `root` (under `local-fixtures/`) with a `.DAT` and an `.EXT`,
/// or `None` when the fixture is missing.
fn ext_pairs(root: &str) -> Option<Vec<ExtPair>> {
    let root = common::local_fixture(root)?;
    let pdb = std::fs::read(root.join("PIONEER/rekordbox/export.pdb")).expect("read pdb");
    let db = Database::from_pdb(&Pdb::from_bytes(&pdb).expect("decode pdb")).expect("load");
    let mut out = Vec::new();
    for track in db.tracks() {
        let dat_path = root.join(track.analysis_path().trim_start_matches('/'));
        let (Ok(dat), Ok(ext)) = (
            std::fs::read(&dat_path),
            std::fs::read(dat_path.with_extension("EXT")),
        ) else {
            continue;
        };
        out.push(ExtPair {
            seconds: track.duration(),
            dat: AnlzFile::from_bytes(&dat).expect("decode .DAT"),
            ext: AnlzFile::from_bytes(&ext).expect("decode .EXT"),
        });
    }
    assert!(!out.is_empty(), "no tracks with a .DAT and an .EXT");
    Some(out)
}

/// The `.EXT` survey of one stick, counts only.
#[derive(Debug, Default, PartialEq, Eq)]
struct ExtSurvey {
    pairs: usize,
    /// `(.DAT header, .EXT header)` → files.
    headers: BTreeMap<(Vec<u8>, Vec<u8>), usize>,
    /// PWV3 `(unknown1, header tail length, body tail length)` → files.
    pwv3_fields: BTreeMap<(u32, usize, usize), usize>,
    /// PWV3 entries minus 150 × the row's seconds → files.
    count_minus_seconds: BTreeMap<i64, usize>,
    /// The first and the last PWV3 byte → files.
    first_bytes: BTreeMap<u8, usize>,
    last_bytes: BTreeMap<u8, usize>,
    /// `(height, whiteness)` of bytes of height 0 to 3 → occurrences, over every PWV3.
    low_heights: BTreeMap<(u8, u8), usize>,
    /// PWV3 bytes of the tracks whose PWAV is all `0xa2` (rekordbox's silence) → occurrences.
    silent_track_bytes: BTreeMap<u8, usize>,
    silent_tracks: usize,
    /// PWV5 entries (u32 at tag offset 0x10) minus PWV3 entries → files.
    pwv5_minus_pwv3: BTreeMap<i64, usize>,
    /// `(.DAT cues, .EXT cues, .EXT PCOBs equal to the .DAT's, .EXT PCOBs equal to the
    /// builders' empty lists)`, cues counted over both lists → files.
    cues: BTreeMap<(usize, usize, bool, bool), usize>,
}

fn ext_survey(pairs: &[ExtPair]) -> ExtSurvey {
    let mut s = ExtSurvey {
        pairs: pairs.len(),
        ..ExtSurvey::default()
    };
    for pair in pairs {
        *s.headers
            .entry((
                pair.dat.unknown_header().to_vec(),
                pair.ext.unknown_header().to_vec(),
            ))
            .or_default() += 1;
        let detail = pair.ext.waveform_detail().expect("PWV3");
        *s.pwv3_fields
            .entry((
                detail.unknown1(),
                detail.header_tail().len(),
                detail.body_tail().len(),
            ))
            .or_default() += 1;
        let count = detail.data().len() as i64;
        *s.count_minus_seconds
            .entry(count - 150 * i64::from(pair.seconds))
            .or_default() += 1;
        if let (Some(first), Some(last)) = (detail.data().first(), detail.data().last()) {
            *s.first_bytes.entry(*first).or_default() += 1;
            *s.last_bytes.entry(*last).or_default() += 1;
        }
        let low = detail.heights().zip(detail.whitenesses());
        for (height, whiteness) in low.filter(|(h, _)| *h <= 3) {
            *s.low_heights.entry((height, whiteness)).or_default() += 1;
        }
        let preview = pair.dat.waveform_preview().expect("PWAV");
        if preview.data().iter().all(|&c| c == 0xa2) {
            s.silent_tracks += 1;
            for &byte in detail.data() {
                *s.silent_track_bytes.entry(byte).or_default() += 1;
            }
        }
        let cue_count = |f: &AnlzFile| {
            f.tags_with(FourCc::PCOB)
                .map(|t| match t {
                    Tag::Cues(list) => list.cues().len(),
                    _ => 0,
                })
                .sum::<usize>()
        };
        let lists = |f: &AnlzFile| f.tags_with(FourCc::PCOB).cloned().collect::<Vec<_>>();
        let empty = vec![Tag::Cues(CueList::hot([])), Tag::Cues(CueList::memory([]))];
        *s.cues
            .entry((
                cue_count(&pair.dat),
                cue_count(&pair.ext),
                lists(&pair.dat) == lists(&pair.ext),
                lists(&pair.ext) == empty,
            ))
            .or_default() += 1;
        if let Some(Tag::Raw(pwv5)) = pair.ext.tags_with(FourCc::PWV5).next() {
            let entries = u32::from_be_bytes(pwv5.header()[4..8].try_into().expect("4 bytes"));
            *s.pwv5_minus_pwv3
                .entry(i64::from(entries) - count)
                .or_default() += 1;
        }
    }
    s
}

/// Measures the `.EXT` files of both sticks for ADR-020: the PMAI header against the
/// `.DAT`'s, the PWV3 header fields, the PWV3 length against the row's whole seconds, and
/// the bytes rekordbox writes for silence. Counts only.
#[test]
fn ext_files_survey() {
    let (Some(real), Some(hw1)) = (ext_pairs("real-stick"), ext_pairs("rekordbox-hw1")) else {
        return;
    };
    let (real, hw1) = (ext_survey(&real), ext_survey(&hw1));
    let report = format!("real-stick: {real:?}\nrekordbox-hw1: {hw1:?}");
    assert_eq!((real.pairs, hw1.pairs), (826, 6), "{report}");

    // The `.EXT` header is the modal `.DAT` header, `00000001 00010000 00010000 00000000`,
    // in all but 3 files, whose `.DAT` and `.EXT` both have first word 0. The 30 `.DAT`
    // files with first word 0 and a modal `.EXT` are the AIFF and MP3 ones of
    // `real_stick_defaults_are_modal`.
    let modal = [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0].to_vec();
    let first_zero = [0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0].to_vec();
    let headers: BTreeMap<(Vec<u8>, Vec<u8>), usize> = [
        ((modal.clone(), modal.clone()), 793),
        ((first_zero.clone(), modal.clone()), 30),
        ((first_zero.clone(), first_zero), 3),
    ]
    .into();
    assert_eq!(real.headers, headers, "{report}");
    assert_eq!(
        hw1.headers,
        [((modal.clone(), modal), 6)].into(),
        "{report}"
    );

    // PWV3: unknown1 0x00960000, no header or body tail, in every file.
    assert_eq!(real.pwv3_fields, [((0x0096_0000, 0, 0), 826)].into());
    assert_eq!(hw1.pwv3_fields, [((0x0096_0000, 0, 0), 6)].into());

    // The row's whole seconds only bound the length: 0 to 150 entries past 150 × seconds,
    // spread over the whole range; exactly 150 past in 91 files (and 2 of hw1's 6), the
    // tracks whose exact length is whole seconds (see the exact rule in
    // `examples/mdma_export.rs`).
    let range = |s: &ExtSurvey| {
        let keys = &s.count_minus_seconds;
        (keys.keys().next().copied(), keys.keys().last().copied())
    };
    assert_eq!(range(&real), (Some(0), Some(150)), "{report}");
    assert_eq!(range(&hw1), (Some(42), Some(150)), "{report}");
    assert_eq!(real.count_minus_seconds.len(), 148, "{report}");
    assert_eq!(real.count_minus_seconds[&150], 91, "{report}");
    assert_eq!(hw1.count_minus_seconds[&150], 2, "{report}");

    // Silence is 0xe0, height 0 and whiteness 7: the whole PWV3 of the one silent track
    // (the one whose PWAV is all 0xa2), and the last byte of 732 of 826 files.
    assert_eq!(real.silent_tracks, 1, "{report}");
    assert_eq!(real.silent_track_bytes, [(0xe0, 154)].into(), "{report}");
    assert_eq!(real.last_bytes.get(&0xe0), Some(&732), "{report}");
    assert_eq!(real.first_bytes.get(&0xe0), Some(&523), "{report}");
    // Low columns have no modal whiteness: at height 0 whiteness 6 is the most common.
    let at_height_0 = |w| real.low_heights[&(0, w)];
    assert!((0..8).all(|w| at_height_0(w) <= at_height_0(6)), "{report}");

    // PWV5 (the colour detail) has as many entries as PWV3 in 812 of 826 files, one more
    // in 14.
    assert_eq!(real.pwv5_minus_pwv3, [(0, 812), (1, 14)].into(), "{report}");
    assert_eq!(hw1.pwv5_minus_pwv3, [(0, 6)].into(), "{report}");

    // rekordbox's `.EXT` PCOB lists hold no cues: in every file they are the builders'
    // empty hot cue and memory point lists (`CueList::hot([])`, `CueList::memory([])`), so
    // they equal the `.DAT`'s exactly when the `.DAT` has no cues (739 and 2 files).
    let cues = |s: &ExtSurvey| {
        let mut out = (0, 0, 0, 0);
        for (&(dat, ext, same, empty), &n) in &s.cues {
            out.0 += usize::from(dat > 0) * n;
            out.1 += ext * n;
            out.2 += usize::from(same) * n;
            out.3 += usize::from(empty) * n;
        }
        out
    };
    assert_eq!(cues(&real), (87, 0, 739, 826), "{report}");
    assert_eq!(cues(&hw1), (4, 0, 2, 6), "{report}");
}

/// How an `.EXT` rebuilt with [`AnlzFile::track_ext`] compares with rekordbox's, limited to
/// the tags it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ExtRebuild {
    /// The `PPTH PWV3 PCOB PCOB` prefix and the header are rekordbox's.
    Identical,
    /// The prefix is rekordbox's; the not-understood `PMAI` header bytes differ.
    PmaiHeader,
    Other,
}

/// Rebuilds the `.EXT` `file` from its own `PPTH` and `PWV3` columns with
/// [`AnlzFile::track_ext`], and compares it with rekordbox's prefix of the same tags.
fn rebuild_ext(file: &AnlzFile) -> ExtRebuild {
    let tags = file.tags();
    let (Some(Tag::Path(path)), Some(Tag::WaveformDetail(detail)), Some(_), Some(_)) =
        (tags.first(), tags.get(1), tags.get(2), tags.get(3))
    else {
        return ExtRebuild::Other;
    };
    // The PWV3 from its columns only, so its other fields are the builder's; the PCOBs
    // are the builder's empty lists.
    let detail = WaveformDetail::new(detail.data().to_vec());
    let built = AnlzFile::track_ext(path.clone(), detail);
    let prefix = |header: &[u8]| {
        AnlzFile::new(header.to_vec(), tags[..4].to_vec())
            .to_bytes()
            .expect("encodes")
    };
    let bytes = built.to_bytes().expect("encodes");
    if bytes != prefix(built.unknown_header()) {
        ExtRebuild::Other
    } else if built.unknown_header() == file.unknown_header() {
        ExtRebuild::Identical
    } else {
        ExtRebuild::PmaiHeader
    }
}

/// Acceptance test for [`AnlzFile::track_ext`] (ADR-020): rebuilds every `.EXT` of both
/// sticks from its own tags and compares the `PPTH PWV3 PCOB PCOB` prefix and the header.
/// Counts only.
#[test]
fn ext_prefixes_rebuild_identically() {
    let (Some(real), Some(hw1)) = (
        common::local_fixture("real-stick/PIONEER/USBANLZ"),
        common::local_fixture("rekordbox-hw1/PIONEER/USBANLZ"),
    ) else {
        return;
    };
    let outcomes = |dir: &Path| {
        let mut out: BTreeMap<ExtRebuild, usize> = BTreeMap::new();
        for (path, ext) in anlz_files(dir) {
            if ext == "EXT" {
                let file = AnlzFile::from_bytes(&std::fs::read(path).expect("read"));
                *out.entry(rebuild_ext(&file.expect("decode"))).or_default() += 1;
            }
        }
        out
    };
    // The 3 are the `.EXT` files whose header has first word 0.
    let expected: BTreeMap<ExtRebuild, usize> =
        [(ExtRebuild::Identical, 823), (ExtRebuild::PmaiHeader, 3)].into();
    assert_eq!(outcomes(&real), expected, "real-stick");
    assert_eq!(
        outcomes(&hw1),
        [(ExtRebuild::Identical, 6)].into(),
        "rekordbox-hw1"
    );
}

/// Distribution of one field's values.
#[derive(Default)]
struct Histogram(BTreeMap<String, usize>);

impl Histogram {
    fn add(&mut self, value: impl Debug) {
        *self.0.entry(format!("{value:?}")).or_default() += 1;
    }

    /// Checks that `expected` is strictly the most common value; returns why not.
    fn check_mode(&self, field: &str, expected: impl Debug) -> Result<(), String> {
        let expected = format!("{expected:?}");
        let count = self.0.get(&expected).copied().unwrap_or(0);
        match self.0.iter().find(|(v, n)| **v != expected && **n >= count) {
            None if count > 0 => Ok(()),
            _ => Err(format!(
                "{field}: default {expected} ({count}) is not the most common value"
            )),
        }
    }
}

/// Checks that each `anlz::defaults` value, as the builders write it, is the most common
/// value of its field across the stick's `.DAT` files, and prints every distribution.
#[test]
fn real_stick_defaults_are_modal() {
    let Some(files) = dat_files() else {
        return;
    };
    let mut fields: BTreeMap<&str, Histogram> = BTreeMap::new();
    for (_, file) in &files {
        let grid = file.beat_grid().expect("beat grid");
        fields
            .entry("PMAI unknown header")
            .or_default()
            .add(file.unknown_header());
        fields
            .entry("PWAV unknown1")
            .or_default()
            .add(file.waveform_preview().expect("PWAV").unknown1());
        fields
            .entry("PWV2 unknown1")
            .or_default()
            .add(file.tiny_waveform_preview().expect("PWV2").unknown1());
        fields
            .entry("PQTZ unknown1")
            .or_default()
            .add(grid.unknown1());
        fields
            .entry("PQTZ unknown2")
            .or_default()
            .add(grid.unknown2());
        for tag in file.tags() {
            let Tag::Cues(list) = tag else {
                continue;
            };
            fields
                .entry("PCOB unknown1")
                .or_default()
                .add(list.unknown1());
            fields
                .entry("PCOB memory_count")
                .or_default()
                .add(list.memory_count());
            for cue in list.cues() {
                fields.entry("PCPT status").or_default().add(cue.status());
                fields
                    .entry("PCPT unknown1")
                    .or_default()
                    .add(cue.unknown1());
                fields
                    .entry("PCPT order_first")
                    .or_default()
                    .add(cue.order_first());
                fields
                    .entry("PCPT order_last")
                    .or_default()
                    .add(cue.order_last());
                fields
                    .entry("PCPT unknown2")
                    .or_default()
                    .add(cue.unknown2());
                fields
                    .entry("PCPT unknown3")
                    .or_default()
                    .add(cue.unknown3());
                if cue.cue_type() == 1 {
                    let loop_time = cue.raw_loop_time();
                    fields
                        .entry("PCPT point loop_time")
                        .or_default()
                        .add(loop_time);
                }
            }
        }
    }

    // The defaults as the builders write them; `anlz::defaults` itself is crate-private.
    let grid = BeatGrid::new(vec![]);
    let list = CueList::new(CueListKind::Hot, vec![]);
    let cue = Cue::hot(HotCueSlot::A, CueSpan::Point(Millis::new(0)));
    let dat = AnlzFile::track_dat(
        AudioPath::new("/a"),
        grid.clone(),
        list.clone(),
        CueList::memory([]),
        WaveformPreview::new(vec![]),
        TinyWaveformPreview::new(vec![]),
    )
    .expect("hot and memory lists");
    let preview = dat.waveform_preview().expect("PWAV").unknown1();
    let tiny = dat.tiny_waveform_preview().expect("PWV2").unknown1();
    let expected: [(&str, &dyn Debug); 14] = [
        ("PMAI unknown header", &dat.unknown_header()),
        ("PWAV unknown1", &preview),
        ("PWV2 unknown1", &tiny),
        ("PQTZ unknown1", &grid.unknown1()),
        ("PQTZ unknown2", &grid.unknown2()),
        ("PCOB unknown1", &list.unknown1()),
        ("PCOB memory_count", &list.memory_count()),
        ("PCPT status", &cue.status()),
        ("PCPT unknown1", &cue.unknown1()),
        ("PCPT order_first", &cue.order_first()),
        ("PCPT order_last", &cue.order_last()),
        ("PCPT unknown2", &cue.unknown2()),
        ("PCPT unknown3", &cue.unknown3()),
        ("PCPT point loop_time", &cue.raw_loop_time()),
    ];

    eprintln!("unknown fields across {} .DAT files:", files.len());
    for (field, histogram) in &fields {
        eprintln!("  {field}:");
        for (value, count) in &histogram.0 {
            eprintln!("    {count:5} {value}");
        }
    }
    let failures: Vec<String> = expected
        .iter()
        .filter_map(|(field, value)| {
            let histogram = fields.get(field)?;
            histogram.check_mode(field, value).err()
        })
        .collect();
    assert_eq!(
        fields.len(),
        expected.len(),
        "every surveyed field has a default"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
