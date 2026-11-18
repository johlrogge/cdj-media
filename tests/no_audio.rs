//! Guards the no-audio rule (ADR-003): nothing under `tests/fixtures` may be audio, judged
//! both by file extension and by the magic bytes a file starts with.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Kept in sync with the "Never commit audio" block in `.gitignore`.
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "mp2", "wav", "flac", "aiff", "aif", "aifc", "m4a", "mp4", "aac", "ogg", "opus", "alac",
    "wma", "caf", "dsf", "dff", "ape", "wv",
];

/// Bytes read from the start of each file; enough for every signature we check.
const HEAD_LEN: u64 = 12;

/// Returns the audio extension of `path`, compared case-insensitively.
fn audio_extension(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    AUDIO_EXTENSIONS.iter().copied().find(|&a| a == ext)
}

/// True if `head` contains `magic` at `offset`.
fn has(head: &[u8], offset: usize, magic: &[u8]) -> bool {
    let Some(rest) = head.get(offset..) else {
        return false;
    };
    rest.starts_with(magic)
}

/// True if `head` starts with a plausible MPEG audio frame header (MP1/MP2/MP3).
///
/// Beyond the 11-bit frame sync, the version, layer, bitrate and sample-rate fields must
/// not hold their reserved or invalid values. That keeps runs of `0xff`, common in
/// waveform data, from being mistaken for audio.
fn is_mpeg_audio_frame(head: &[u8]) -> bool {
    let [0xff, b1, b2, ..] = *head else {
        return false;
    };
    let sync = b1 & 0xe0 == 0xe0;
    let version = (b1 >> 3) & 0b11;
    let layer = (b1 >> 1) & 0b11;
    let bitrate = b2 >> 4;
    let sample_rate = (b2 >> 2) & 0b11;
    sync && version != 0b01 && layer != 0b00 && bitrate != 0b1111 && sample_rate != 0b11
}

/// True if `head` starts with an ADTS (raw AAC) frame header: 12-bit sync, layer `00`.
fn is_adts_frame(head: &[u8]) -> bool {
    matches!(head, [0xff, b1, ..] if b1 & 0xf6 == 0xf0)
}

/// Names the audio container whose magic bytes `head` starts with, if any.
fn audio_signature(head: &[u8]) -> Option<&'static str> {
    if has(head, 0, b"ID3") {
        return Some("ID3");
    }
    let riff_like = has(head, 0, b"RIFF") || has(head, 0, b"RF64") || has(head, 0, b"BW64");
    if riff_like && has(head, 8, b"WAVE") {
        return Some("WAVE");
    }
    if has(head, 0, b"fLaC") {
        return Some("FLAC");
    }
    if has(head, 4, b"ftyp") {
        return Some("ISO media");
    }
    if has(head, 0, b"OggS") {
        return Some("Ogg");
    }
    if has(head, 0, b"FORM") && (has(head, 8, b"AIFF") || has(head, 8, b"AIFC")) {
        return Some("AIFF");
    }
    if has(head, 0, b"FRM8") {
        return Some("DSDIFF");
    }
    if is_adts_frame(head) {
        return Some("ADTS");
    }
    if is_mpeg_audio_frame(head) {
        return Some("MPEG audio");
    }
    None
}

/// Every regular file under `root`, recursively. Symlinks are skipped, neither listed nor
/// followed.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).expect("read fixture directory") {
            let entry = entry.expect("read directory entry");
            let file_type = entry.file_type().expect("read file type");
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }
    files
}

fn head(path: &Path) -> Vec<u8> {
    let file = fs::File::open(path).expect("open fixture");
    let mut head = Vec::new();
    let mut reader = file.take(HEAD_LEN);
    reader.read_to_end(&mut head).expect("read fixture");
    head
}

#[test]
fn fixtures_contain_no_audio() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("tests").join("fixtures");
    assert!(root.is_dir(), "{} is missing", root.display());

    let mut offenders = Vec::new();
    for path in files_under(&root) {
        if let Some(ext) = audio_extension(&path) {
            offenders.push(format!("{}: audio extension .{ext}", path.display()));
        }
        if let Some(sig) = audio_signature(&head(&path)) {
            offenders.push(format!("{}: starts with {sig} magic", path.display()));
        }
    }
    assert!(
        offenders.is_empty(),
        "audio found in tests/fixtures:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn detects_every_audio_signature() {
    let cases: &[(&str, &[u8])] = &[
        ("ID3", b"ID3\x04\x00\x00\x00\x00\x00\x00"),
        ("WAVE", b"RIFF\x24\x00\x00\x00WAVE"),
        ("WAVE", b"RF64\xff\xff\xff\xffWAVE"),
        ("WAVE", b"BW64\xff\xff\xff\xffWAVE"),
        ("FLAC", b"fLaC\x00\x00\x00\x22"),
        ("ISO media", b"\x00\x00\x00\x20ftypM4A "),
        ("Ogg", b"OggS\x00\x02"),
        ("AIFF", b"FORM\x00\x00\x00\x00AIFF"),
        ("AIFF", b"FORM\x00\x00\x00\x00AIFC"),
        ("DSDIFF", b"FRM8\x00\x00\x00\x00"),
        ("ADTS", &[0xff, 0xf1, 0x50, 0x80]),
        ("MPEG audio", &[0xff, 0xfb, 0x90, 0x00]),
    ];
    for (name, head) in cases {
        assert_eq!(audio_signature(head), Some(*name), "{head:02x?}");
    }
}

#[test]
fn ignores_non_audio_heads() {
    let cases: &[&[u8]] = &[
        b"",
        b"PMAI\x00\x00\x00\x1c",
        &[0; 12],
        b"RIFF\x00\x00\x00\x00AVI ",
        b"RIFF",
        b"RF64",
        b"FORM\x00\x00\x00\x00ILBM",
        b"ftyp",
        &[0xff, 0xff, 0xff, 0xff],
        &[0xff, 0xe0],
        &[0xff, 0xd8, 0xff, 0xe0],
        &[0xff, 0x1f],
        &[0xff],
        b"# Test fixtures\n",
    ];
    for head in cases {
        assert_eq!(audio_signature(head), None, "{head:02x?}");
    }
}

#[test]
fn mpeg_header_rejects_reserved_fields() {
    // Each case starts from a valid MPEG-1 Layer III header (ff fb 90) and breaks one field.
    let cases: &[&[u8]] = &[
        &[0xff, 0xeb, 0x90],
        &[0xff, 0xf9, 0x90],
        &[0xff, 0xfb, 0xf0],
        &[0xff, 0xfb, 0x9c],
        &[0xff, 0xfb],
    ];
    assert!(is_mpeg_audio_frame(&[0xff, 0xfb, 0x90]));
    for head in cases {
        assert!(!is_mpeg_audio_frame(head), "{head:02x?}");
    }
}

#[test]
fn detects_audio_extensions_case_insensitively() {
    for ext in AUDIO_EXTENSIONS {
        let lower = format!("track.{ext}");
        let upper = lower.to_ascii_uppercase();
        assert!(audio_extension(Path::new(&lower)).is_some(), "{lower}");
        assert!(audio_extension(Path::new(&upper)).is_some(), "{upper}");
    }
    for name in ["export.pdb", "ANLZ0000.DAT", "README.md", "mp3"] {
        assert!(audio_extension(Path::new(name)).is_none(), "{name}");
    }
}
