//! Measures the artwork path scheme of the local fixtures (`rekordbox-hw1`, a fresh export,
//! and `real-stick`, a long-used one): the shape of each artwork row's path, whether its
//! numbers come from the row's id, and whether any `_m` variant appears, in the rows or on
//! disk. Checks `ArtworkPath` against every row. Skips a fixture that is missing.
//!
//! The paths come from the user's library, so the tests print shapes and counts only:
//! letters outside the structural `/PIONEER/Artwork/` prefix are masked (`a` for lower case,
//! `A` for upper case), a number equal to the artwork id is shown as `{id}`, one equal to
//! `id / 20 + 1` as `{dir}` (when it has 5 digits), and other digits are kept.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use cdj_media::layout::ArtworkPath;
use cdj_media::pdb::{Database, Pdb};

const FIXTURES: [&str; 2] = ["rekordbox-hw1", "real-stick"];

/// The structural prefix the shape keeps unmasked.
const PREFIX: &str = "/PIONEER/Artwork/";

fn load(root: &Path) -> Database {
    let bytes = std::fs::read(root.join("PIONEER/rekordbox/export.pdb")).expect("read");
    Database::from_pdb(&Pdb::from_bytes(&bytes).expect("decode")).expect("load")
}

/// The shape of `path`, the path of artwork `id`: see the module documentation.
fn shape(path: &str, id: u32) -> String {
    let (prefix, rest) = match path.strip_prefix(PREFIX) {
        Some(rest) => (PREFIX, rest),
        None => ("", path),
    };
    let mut out = String::from(prefix);
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut number = String::from(c);
            while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                number.push(d);
                chars.next();
            }
            let value = number.parse::<u32>();
            if !number.starts_with('0') && value == Ok(id) {
                out.push_str("{id}");
            } else if number.len() == 5 && value == Ok(id / 20 + 1) {
                out.push_str("{dir}");
            } else {
                out.push_str(&number);
            }
        } else if c.is_ascii_lowercase() {
            out.push('a');
        } else if c.is_ascii_uppercase() {
            out.push('A');
        } else if c.is_ascii() {
            out.push(c);
        } else {
            out.push('?');
        }
    }
    out
}

/// What a fixture shows: its artwork rows by shape, the distinct directories, the rows whose
/// path names `_m`, the files under `PIONEER/Artwork/` on disk and those named `_m`.
#[derive(Debug, PartialEq)]
struct Survey {
    shapes: BTreeMap<String, usize>,
    directories: usize,
    m_rows: usize,
    files_on_disk: usize,
    m_files_on_disk: usize,
}

fn survey(root: &Path) -> Survey {
    let db = load(root);
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();
    let mut directories: BTreeMap<&str, ()> = BTreeMap::new();
    let mut m_rows = 0;
    for artwork in db.artworks() {
        let path = artwork.path();
        *shapes.entry(shape(path, artwork.id().get())).or_default() += 1;
        m_rows += usize::from(path.contains("_m"));
        if let Some((dir, _)) = path.rsplit_once('/') {
            directories.insert(dir, ());
        }
    }
    let (mut files_on_disk, mut m_files_on_disk) = (0, 0);
    let mut pending = vec![root.join("PIONEER/Artwork")];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files_on_disk += 1;
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                m_files_on_disk += usize::from(name.contains("_m"));
            }
        }
    }
    Survey {
        shapes,
        directories: directories.len(),
        m_rows,
        files_on_disk,
        m_files_on_disk,
    }
}

#[test]
fn the_shape_masks_letters_and_marks_the_numbers() {
    assert_eq!(
        shape("/PIONEER/Artwork/00001/a12.jpg", 12),
        "/PIONEER/Artwork/{dir}/a{id}.aaa"
    );
    assert_eq!(
        shape("/PIONEER/Artwork/00002/a12_m.jpg", 3),
        "/PIONEER/Artwork/00002/a12_a.aaa"
    );
    assert_eq!(
        shape("/PIONEER/Artwork/00002/a20.jpg", 20),
        "/PIONEER/Artwork/{dir}/a{id}.aaa"
    );
    assert_eq!(shape("/Other/Xy1/ö", 1), "/Aaaaa/Aa{id}/?");
}

/// The artwork rows of both fixtures have one shape,
/// `/PIONEER/Artwork/{dir}/a{id}.jpg` with `dir = id / 20 + 1` in 5 digits: directory
/// 00001 holds ids 1 to 19 and each further one 20 ids. The fresh export's 5 rows are all
/// in 00001; the real stick's 787 rows (ids 1 to 835 with gaps) are in 42 directories. No
/// row names an `_m` file, and neither fixture holds the image files.
#[test]
fn artwork_path_scheme() {
    let one_shape = |rows| BTreeMap::from([("/PIONEER/Artwork/{dir}/a{id}.aaa".to_owned(), rows)]);
    let pinned = [
        (
            "rekordbox-hw1",
            Survey {
                shapes: one_shape(5),
                directories: 1,
                m_rows: 0,
                files_on_disk: 0,
                m_files_on_disk: 0,
            },
        ),
        (
            "real-stick",
            Survey {
                shapes: one_shape(787),
                directories: 42,
                m_rows: 0,
                files_on_disk: 0,
                m_files_on_disk: 0,
            },
        ),
    ];
    for (fixture, pinned) in pinned {
        let Some(root) = common::local_fixture(fixture) else {
            continue;
        };
        let survey = survey(&root);
        eprintln!("{fixture}: {survey:?}");
        assert_eq!(survey, pinned, "{fixture}");
    }
}

/// `ArtworkPath::for_artwork` gives back the path of every artwork row of both fixtures.
#[test]
fn artwork_path_reproduces_every_fixture_row() {
    for fixture in FIXTURES {
        let Some(root) = common::local_fixture(fixture) else {
            continue;
        };
        let db = load(&root);
        let (mut same, mut other) = (0, 0);
        for artwork in db.artworks() {
            if ArtworkPath::for_artwork(artwork.id()).as_str() == artwork.path() {
                same += 1;
            } else {
                other += 1;
            }
        }
        eprintln!("{fixture}: ArtworkPath equals {same} artwork rows, differs on {other}");
        assert_eq!((other, same), (0, db.artworks().len()), "{fixture}");
    }
}
