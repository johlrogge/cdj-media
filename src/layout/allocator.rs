//! [`ContentPathAllocator`]: distinct audio file paths for the tracks of one stick.

use std::collections::{BTreeMap, BTreeSet};

use super::ContentPath;
use super::content::{MAX_UNITS, cut, extension_at};
use super::stick_path::utf16_len;

/// Gives the tracks of one stick distinct audio file paths (ADR-010). It is pure and
/// stateful: each [`allocate`](Self::allocate) sees the paths allocated before it, so the
/// same paths in the same order always come out the same.
///
/// FAT32 compares names case-insensitively, so names are compared case folded, and the
/// database must name each file and directory the way the disk does:
/// - an artist directory that equals an earlier one once case folded is spelled as the
///   first one was, and so is an album directory under the same artist directory;
/// - a file path that is then still taken keeps its own spelling and gets ` (2)`, ` (3)`, …
///   before its extension (at the end if it has none), the first number whose path is
///   free. The stem is cut so the name stays within 48 UTF-16 units, never inside a
///   character; the extension is kept.
///
/// Case folding maps each character to its upper case when that is one character (`ö` and
/// `Ö` are one name), and keeps it otherwise (`ß` is not `SS`), as FAT32's upper-case table
/// does: one character for one.
#[derive(Debug, Clone, Default)]
pub struct ContentPathAllocator {
    /// The first spelling of each artist directory, by its folded name.
    artists: BTreeMap<String, String>,
    /// The first spelling of each album directory, by the folded names of its artist
    /// directory and itself.
    albums: BTreeMap<(String, String), String>,
    /// Every path allocated, folded.
    files: BTreeSet<String>,
}

impl ContentPathAllocator {
    /// An allocator that has allocated nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The path the track whose own path is `path` (from
    /// [`ContentPath::for_track`]) goes to: `path` with its directories spelled as before
    /// and, if that path is taken, a numbered file name. The result is taken from then on.
    pub fn allocate(&mut self, path: ContentPath) -> ContentPath {
        let text = path.as_stick_path().as_str();
        let [_, artist, album, file] = text.splitn(5, '/').skip(1).collect::<Vec<_>>()[..] else {
            unreachable!("a content path is /Contents/<artist>/<album>/<file name>");
        };
        let artist = self
            .artists
            .entry(fold(artist))
            .or_insert_with(|| artist.to_owned())
            .clone();
        let album = self
            .albums
            .entry((fold(&artist), fold(album)))
            .or_insert_with(|| album.to_owned())
            .clone();
        let dir = fold(&format!("{artist}/{album}/"));
        let mut name = file.to_owned();
        let mut number = 1u32;
        while !self.files.insert(format!("{dir}{}", fold(&name))) {
            number += 1;
            name = numbered(file, number);
        }
        // The components are already sanitised and the name within 48 units, so this
        // gives them back unchanged.
        ContentPath::for_track(&artist, &album, &name)
    }
}

/// `name` case folded: each character in upper case, when that is one character.
fn fold(name: &str) -> String {
    name.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(u), None) => u,
                _ => c,
            }
        })
        .collect()
}

/// File name `name` numbered `number`: ` (number)` before its extension, the stem cut so
/// the whole is at most 48 UTF-16 units.
fn numbered(name: &str, number: u32) -> String {
    let (stem, extension) = name.split_at(extension_at(name).unwrap_or(name.len()));
    let suffix = format!(" ({number}){extension}");
    let stem = cut(stem, MAX_UNITS - utf16_len(&suffix));
    format!("{stem}{suffix}")
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::layout::stick_path::utf16_len;

    fn allocate_all(tracks: &[(&str, &str, &str)]) -> Vec<String> {
        let mut allocator = ContentPathAllocator::new();
        tracks
            .iter()
            .map(|&(artist, album, file)| {
                let path = ContentPath::for_track(artist, album, file);
                allocator.allocate(path).as_stick_path().to_string()
            })
            .collect()
    }

    #[test]
    fn distinct_paths_pass_through_unchanged() {
        let paths = allocate_all(&[
            ("A", "B", "1.mp3"),
            ("A", "B", "2.mp3"),
            ("C", "D", "1.mp3"),
        ]);
        assert_eq!(
            paths,
            [
                "/Contents/A/B/1.mp3",
                "/Contents/A/B/2.mp3",
                "/Contents/C/D/1.mp3"
            ]
        );
    }

    #[test]
    fn a_taken_file_path_gets_a_number_before_the_extension() {
        let paths = allocate_all(&[
            ("A", "B", "1.mp3"),
            ("A", "B", "1.mp3"),
            ("A", "B", "1.mp3"),
        ]);
        assert_eq!(
            paths,
            [
                "/Contents/A/B/1.mp3",
                "/Contents/A/B/1 (2).mp3",
                "/Contents/A/B/1 (3).mp3"
            ]
        );
    }

    /// Different names that sanitise to one path collide too.
    #[test]
    fn names_that_sanitise_alike_collide() {
        let paths = allocate_all(&[("AC/DC", "B", "1.mp3"), ("AC:DC", "B", "1.mp3")]);
        assert_eq!(
            paths,
            ["/Contents/AC_DC/B/1.mp3", "/Contents/AC_DC/B/1 (2).mp3"]
        );
    }

    #[test]
    fn a_file_name_without_an_extension_gets_the_number_at_the_end() {
        let paths = allocate_all(&[("", "", ""), ("", "", "")]);
        assert_eq!(
            paths,
            [
                "/Contents/UnknownArtist/UnknownAlbum/Unknown",
                "/Contents/UnknownArtist/UnknownAlbum/Unknown (2)"
            ]
        );
    }

    /// FAT32 compares names case-insensitively: a file name that differs only in case is the
    /// same file. It keeps its own spelling and gets a number.
    #[test]
    fn file_names_that_differ_only_in_case_collide() {
        let paths = allocate_all(&[("A", "B", "track.mp3"), ("A", "B", "TRACK.MP3")]);
        assert_eq!(
            paths,
            ["/Contents/A/B/track.mp3", "/Contents/A/B/TRACK (2).MP3"]
        );
    }

    /// A directory that differs only in case is the same directory on FAT32, so later
    /// tracks reuse the first spelling and the database matches the disk.
    #[test]
    fn directories_that_differ_only_in_case_reuse_the_first_spelling() {
        let paths = allocate_all(&[
            ("ABBA", "Gold", "1.mp3"),
            ("Abba", "Gold", "2.mp3"),
            ("abba", "GOLD", "3.mp3"),
            ("Other", "GOLD", "4.mp3"),
            ("Other", "gold", "5.mp3"),
        ]);
        assert_eq!(
            paths,
            [
                "/Contents/ABBA/Gold/1.mp3",
                "/Contents/ABBA/Gold/2.mp3",
                "/Contents/ABBA/Gold/3.mp3",
                "/Contents/Other/GOLD/4.mp3",
                "/Contents/Other/GOLD/5.mp3"
            ]
        );
    }

    /// Directories folded together, then a file name that collides in the folded directory.
    #[test]
    fn a_file_collides_in_a_directory_spelled_differently() {
        let paths = allocate_all(&[("ABBA", "Gold", "1.mp3"), ("Abba", "gold", "1.mp3")]);
        assert_eq!(
            paths,
            ["/Contents/ABBA/Gold/1.mp3", "/Contents/ABBA/Gold/1 (2).mp3"]
        );
    }

    /// Case folds per character, beyond ASCII: `ö` and `Ö` collide.
    #[test]
    fn case_folds_beyond_ascii() {
        let paths = allocate_all(&[("Björk", "Ö", "ä.mp3"), ("BJÖRK", "ö", "Ä.mp3")]);
        assert_eq!(
            paths,
            ["/Contents/Björk/Ö/ä.mp3", "/Contents/Björk/Ö/Ä (2).mp3"]
        );
    }

    /// A character whose upper case is more than one character folds to itself, as in
    /// FAT32's one-for-one upper-case table: `ß` is not `SS`.
    #[test]
    fn case_folds_one_character_for_one() {
        let paths = allocate_all(&[("A", "B", "ß.mp3"), ("A", "B", "SS.mp3")]);
        assert_eq!(paths, ["/Contents/A/B/ß.mp3", "/Contents/A/B/SS.mp3"]);
    }

    /// A number can't take a name that is already taken: it counts on.
    #[test]
    fn a_number_skips_names_already_taken() {
        let paths = allocate_all(&[
            ("A", "B", "1.mp3"),
            ("A", "B", "1 (2).mp3"),
            ("A", "B", "1.mp3"),
        ]);
        assert_eq!(
            paths,
            [
                "/Contents/A/B/1.mp3",
                "/Contents/A/B/1 (2).mp3",
                "/Contents/A/B/1 (3).mp3"
            ]
        );
    }

    /// A numbered name is cut again to 48 UTF-16 units, its stem cut and its extension kept.
    #[test]
    fn a_numbered_name_is_cut_to_48_units() {
        let a = |n: usize| "a".repeat(n);
        let long = format!("{}.mp3", a(44));
        let tracks = vec![("A", "B", long.as_str()); 11];
        let paths = allocate_all(&tracks);
        assert_eq!(paths[0], format!("/Contents/A/B/{}.mp3", a(44)));
        assert_eq!(paths[1], format!("/Contents/A/B/{} (2).mp3", a(40)));
        assert_eq!(paths[10], format!("/Contents/A/B/{} (11).mp3", a(39)));
        // Without an extension, the whole name is cut; never inside a character.
        let notes = "\u{1f3b5}".repeat(24);
        let paths = allocate_all(&[("A", "B", &notes), ("A", "B", &notes)]);
        assert_eq!(
            paths[1],
            format!("/Contents/A/B/{} (2)", "\u{1f3b5}".repeat(22))
        );
    }

    /// The same tracks in the same order always get the same paths.
    #[test]
    fn allocation_is_deterministic() {
        let tracks = [
            ("A", "B", "1.mp3"),
            ("a", "b", "1.MP3"),
            ("A", "B", "1.mp3"),
        ];
        assert_eq!(allocate_all(&tracks), allocate_all(&tracks));
    }

    /// FAT32's view of a path: case folded.
    fn folded(path: &str) -> String {
        path.chars().flat_map(char::to_uppercase).collect()
    }

    proptest! {
        /// Whatever the names, the paths allocated are distinct even ignoring case, every
        /// component is at most 48 units long, and each directory is spelled one way.
        #[test]
        fn allocated_paths_are_distinct_on_fat32(
            tracks in prop::collection::vec(
                ("[aAbB]{0,2}", "[aAbB/:]{0,2}", "[aA. ]{0,3}(\\.[mM][pP]3)?"),
                0..40,
            ),
        ) {
            let tracks: Vec<(&str, &str, &str)> = tracks
                .iter()
                .map(|(a, b, f)| (a.as_str(), b.as_str(), f.as_str()))
                .collect();
            let paths = allocate_all(&tracks);
            let mut seen = std::collections::BTreeSet::new();
            let mut spellings = std::collections::BTreeMap::new();
            for path in &paths {
                prop_assert!(seen.insert(folded(path)), "{} taken twice", path);
                let parts: Vec<&str> = path.split('/').skip(2).collect();
                for part in &parts {
                    prop_assert!(utf16_len(part) <= 48, "{:?}", part);
                }
                let dir = parts[..2].join("/");
                let first = spellings.entry(folded(&dir)).or_insert_with(|| dir.clone());
                prop_assert_eq!(&*first, &dir);
            }
        }
    }
}
