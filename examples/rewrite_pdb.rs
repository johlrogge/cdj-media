//! Rewrites an `export.pdb` with this crate's writer, for hardware test image V0: the same
//! stick content, with only `export.pdb` rewritten.
//!
//! ```text
//! cargo run --example rewrite_pdb -- <input export.pdb> <output export.pdb>
//! ```
//!
//! The input is loaded as a [`Database`] and written with [`Database::to_pdb`]. The output
//! is checked by loading it again: it must equal the input's database, with no dangling
//! reference. The program refuses an input with dangling references, an existing output
//! file and an output that is the input. It prints counts only, never text from the data.

use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use cdj_media::pdb::{Database, Pdb};

/// The counts of a rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Summary {
    tracks: usize,
    playlists: usize,
    entries: usize,
    pages_in: usize,
    pages_out: usize,
    bytes_out: usize,
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tracks {}, playlists {}, entries {}, pages {} in, {} out, output {} bytes",
            self.tracks,
            self.playlists,
            self.entries,
            self.pages_in,
            self.pages_out,
            self.bytes_out
        )
    }
}

/// A rewritten, verified `export.pdb` and its counts.
#[derive(Debug)]
struct Rewrite {
    bytes: Vec<u8>,
    summary: Summary,
}

/// Decodes and loads `export.pdb` bytes; `what` names them in errors.
fn load(bytes: &[u8], what: &str) -> Result<(Pdb, Database), String> {
    let pdb = Pdb::from_bytes(bytes).map_err(|e| format!("{what} does not decode: {e}"))?;
    let db = Database::from_pdb(&pdb).map_err(|e| format!("{what} does not load: {e}"))?;
    let dangling = db.dangling_references().count();
    if dangling != 0 {
        return Err(format!("{what} has {dangling} dangling references"));
    }
    Ok((pdb, db))
}

/// Rewrites the `export.pdb` bytes `input` with [`Database::to_pdb`], and checks that the
/// result loads again as an equal database with no dangling reference.
fn rewrite(input: &[u8]) -> Result<Rewrite, String> {
    let (pdb_in, db) = load(input, "the input")?;
    let bytes = db
        .to_pdb()
        .and_then(|pdb| pdb.to_bytes())
        .map_err(|e| format!("the database does not write: {e}"))?;
    let (pdb_out, back) = load(&bytes, "the output")?;
    if back != db {
        return Err("the output's database differs from the input's".into());
    }
    let summary = Summary {
        tracks: db.tracks().len(),
        playlists: db.playlists().len(),
        entries: db
            .playlists()
            .map(|p| db.playlist_tracks(p.id()).count())
            .sum(),
        pages_in: pdb_in.page_count(),
        pages_out: pdb_out.page_count(),
        bytes_out: bytes.len(),
    };
    Ok(Rewrite { bytes, summary })
}

/// Rewrites the file `input` to the new file `output`, then reads the output back and
/// checks it holds what was written.
fn run(input: &Path, output: &Path) -> Result<Summary, String> {
    if output.exists() {
        let same = std::fs::canonicalize(input).ok() == std::fs::canonicalize(output).ok();
        return Err(if same {
            "the output is the input; refusing to overwrite it".into()
        } else {
            "the output already exists; refusing to overwrite it".into()
        });
    }
    let bytes = std::fs::read(input).map_err(|e| format!("cannot read the input: {e}"))?;
    let rewritten = rewrite(&bytes)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| format!("cannot create the output: {e}"))?;
    file.write_all(&rewritten.bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("cannot write the output: {e}"))?;
    let back = std::fs::read(output).map_err(|e| format!("cannot read the output back: {e}"))?;
    if back != rewritten.bytes {
        return Err("the output file differs from the bytes written".into());
    }
    Ok(rewritten.summary)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = &args[..] else {
        eprintln!("usage: rewrite_pdb <input export.pdb> <output export.pdb>");
        return ExitCode::from(2);
    };
    match run(Path::new(input), Path::new(output)) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("rewrite_pdb: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod common;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use cdj_media::pdb::TrackFields;

    use super::*;

    const FRESH: &str = "rekordbox-hw1/PIONEER/rekordbox/export.pdb";

    fn manifest(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    fn loaded(bytes: &[u8]) -> (Pdb, Database) {
        load(bytes, "the test file").unwrap()
    }

    /// A database of one track with an artist, in one playlist.
    fn built() -> Database {
        let mut db = Database::new();
        let artist_id = db.find_or_insert_artist("An Artist");
        let fields = TrackFields {
            title: "A Title".into(),
            artist_id,
            file_path: "/Contents/An Artist/a.mp3".into(),
            ..TrackFields::default()
        };
        let track = db.add_track(fields).unwrap();
        let playlist = db.add_playlist("A Playlist", None).unwrap();
        db.add_to_playlist(playlist, track);
        db
    }

    #[test]
    fn rewriting_a_built_database_gives_it_back() {
        let db = built();
        let input = db.to_pdb().unwrap().to_bytes().unwrap();
        let out = rewrite(&input).unwrap();
        assert_eq!(loaded(&out.bytes).1, db);
        let s = &out.summary;
        assert_eq!((s.tracks, s.playlists, s.entries), (1, 1, 1));
        assert_eq!(s.bytes_out, out.bytes.len());
        assert_eq!(s.pages_in, loaded(&input).0.page_count());
        assert_eq!(s.pages_out, loaded(&out.bytes).0.page_count());
    }

    #[test]
    fn bytes_that_are_no_pdb_are_refused() {
        let err = rewrite(&[0; 10]).unwrap_err();
        assert!(err.contains("does not decode"), "{err}");
    }

    #[test]
    fn the_input_as_output_is_refused() {
        let path = manifest("Cargo.toml");
        let err = run(&path, &path).unwrap_err();
        assert!(err.contains("is the input"), "{err}");
    }

    #[test]
    fn an_existing_output_is_refused() {
        let err = run(&manifest("src/lib.rs"), &manifest("Cargo.toml")).unwrap_err();
        assert!(err.contains("already exists"), "{err}");
    }

    /// The fresh rekordbox export, rewritten: an equal database, and the file header's
    /// `unknown2` is 5, as in a fresh export.
    #[test]
    fn rewriting_the_fresh_export_gives_an_equal_database() {
        let Some(input) = common::local_fixture(FRESH) else {
            return;
        };
        let bytes = std::fs::read(input).unwrap();
        let out = rewrite(&bytes).unwrap();
        let (written, back) = loaded(&out.bytes);
        assert_eq!(back, loaded(&bytes).1);
        assert_eq!(written.unknown2(), 5);
        let pinned = Summary {
            tracks: 6,
            playlists: 1,
            entries: 6,
            pages_in: 51,
            pages_out: 40,
            bytes_out: 40 * 4096,
        };
        assert_eq!(out.summary, pinned);
    }
}
