//! Round-trips every ANLZ file on a real stick in `local-fixtures/`. Skips itself when the
//! fixture is missing.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cdj_media::anlz::AnlzFile;

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

/// Decodes and re-encodes one file; returns its tag codes, or why it failed.
fn round_trip(path: &Path) -> Result<Vec<String>, String> {
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
    let mut failures = Vec::new();
    for (path, ext) in &files {
        *per_extension.entry(ext.as_str()).or_default() += 1;
        match round_trip(path) {
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
    assert!(
        failures.is_empty(),
        "{} of {} files failed to round-trip:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
