//! Round-trips every ANLZ file on a real stick in `local-fixtures/`, and checks the typed
//! tags' builders and defaults against it. Skips itself when the fixture is missing.

mod common;

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use cdj_media::anlz::{AnlzFile, BeatGrid, Cue, CueList, CueListKind, CueSpan, HotCueSlot, Tag};
use cdj_media::encoding::Millis;

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
            _ => {}
        }
    }

    if ext == "DAT" {
        if file.path().is_none() {
            return Err("no PPTH path".into());
        }
        if file.beat_grid().is_none() {
            return Err("no PQTZ beat grid".into());
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
    let expected: [(&str, &dyn Debug); 11] = [
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
