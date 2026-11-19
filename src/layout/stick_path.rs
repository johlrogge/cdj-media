//! [`StickPath`]: a validated absolute path on the stick.

use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error as ThisError;

/// The longest component a [`StickPath`] can have, in UTF-16 code units: the length of a
/// VFAT long name, which FAT32 stores in UTF-16.
pub const MAX_COMPONENT_UNITS: usize = 255;

/// The longest [`StickPath`], in UTF-16 code units, counted from the leading `/`.
///
/// FAT32 itself has no limit on a whole path; the limit comes from the systems that read
/// and write the stick. rekordbox runs on Windows, whose `MAX_PATH` is 260 UTF-16 units
/// including the drive (`E:`) and the terminating NUL; that leaves 260 − 2 − 1 = 257 for
/// the path from the root's `\`. A path that fits is reachable on every system rekordbox
/// exports from. The real stick's longest audio path is 140 units, its analysis paths 43.
pub const MAX_PATH_UNITS: usize = 257;

/// The printable characters FAT32 long names can't hold, besides the separator `/`.
const FORBIDDEN: [char; 8] = ['\\', ':', '*', '?', '"', '<', '>', '|'];

/// Whether a path component can't hold `c`: one of [`FORBIDDEN`] or a control character.
/// The separator `/` is not included.
pub(crate) fn is_forbidden(c: char) -> bool {
    c < '\u{20}' || FORBIDDEN.contains(&c)
}

/// The length of `s` in UTF-16 code units.
pub(crate) fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Why a string is not a [`StickPath`].
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
#[non_exhaustive]
pub enum PathError {
    /// The path does not start with `/`.
    #[error("path is not absolute")]
    NotAbsolute,
    /// A component is empty: the path is `/`, has `//` or ends in `/`.
    #[error("path has an empty component")]
    EmptyComponent,
    /// A component is `.` or `..`.
    #[error("path has a `.` or `..` component")]
    DotComponent,
    /// A component has a character FAT32 long names can't hold: one of
    /// `\ : * ? " < > |`, or a control character (below U+0020).
    #[error("path has a character FAT32 forbids: {0:?}")]
    ForbiddenCharacter(char),
    /// A component ends in a dot or a space, which Windows drops from names on FAT32.
    #[error("path has a component that ends in a dot or a space")]
    TrailingDotOrSpace,
    /// A component is longer than [`MAX_COMPONENT_UNITS`] UTF-16 code units.
    #[error("path component is {units} UTF-16 units long, more than {MAX_COMPONENT_UNITS}")]
    #[non_exhaustive]
    ComponentTooLong {
        /// The component's length in UTF-16 code units.
        units: usize,
    },
    /// The path is longer than [`MAX_PATH_UNITS`] UTF-16 code units.
    #[error("path is {units} UTF-16 units long, more than {MAX_PATH_UNITS}")]
    #[non_exhaustive]
    PathTooLong {
        /// The path's length in UTF-16 code units.
        units: usize,
    },
}

/// An absolute path on the stick, such as `/Contents/Artist/Album/01 Track.mp3`: what track
/// rows hold in slots 14 (analysis path) and 20 (file path), and `PPTH` holds.
///
/// [`parse`](Self::parse) is the only way to make one, so every `StickPath` is:
/// - rooted at `/`, with `/` separating its components;
/// - free of empty, `.` and `..` components (so it has at least one component);
/// - FAT32-safe in every component: none of `\ : * ? " < > |` or a control character, no
///   trailing dot or space, at most [`MAX_COMPONENT_UNITS`] UTF-16 code units;
/// - at most [`MAX_PATH_UNITS`] UTF-16 code units long.
///
/// Equality compares the text exactly. FAT32 compares names case-insensitively, so two
/// paths that differ only in case (`/Contents/ABBA` and `/Contents/Abba`) are different
/// `StickPath`s but the same file on the stick.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StickPath(String);

impl StickPath {
    /// Checks `path` and returns it as a stick path.
    ///
    /// # Errors
    ///
    /// A [`PathError`] naming the first rule `path` breaks, checking the components from
    /// the first, each against the rules in the order of [`PathError`]'s variants, then the
    /// whole path's length. The error never holds the path's text.
    pub fn parse(path: &str) -> Result<Self, PathError> {
        let Some(rest) = path.strip_prefix('/') else {
            return Err(PathError::NotAbsolute);
        };
        for component in rest.split('/') {
            if component.is_empty() {
                return Err(PathError::EmptyComponent);
            }
            if component == "." || component == ".." {
                return Err(PathError::DotComponent);
            }
            if let Some(c) = component.chars().find(|&c| is_forbidden(c)) {
                return Err(PathError::ForbiddenCharacter(c));
            }
            if component.ends_with(['.', ' ']) {
                return Err(PathError::TrailingDotOrSpace);
            }
            let units = utf16_len(component);
            if units > MAX_COMPONENT_UNITS {
                return Err(PathError::ComponentTooLong { units });
            }
        }
        let units = utf16_len(path);
        if units > MAX_PATH_UNITS {
            return Err(PathError::PathTooLong { units });
        }
        Ok(Self(path.to_owned()))
    }

    /// The path.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Where the path is on a file system whose stick is mounted at `root`: `root` joined
    /// with each component in turn.
    pub fn under(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        path.extend(self.0.split('/').skip(1));
        path
    }

    /// The last component: the file's name.
    pub fn file_name(&self) -> &str {
        // A stick path has at least one non-empty component after its leading `/`.
        self.0.rsplit('/').next().unwrap_or_default()
    }
}

impl fmt::Display for StickPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(path: &str) -> Result<String, PathError> {
        StickPath::parse(path).map(|p| p.as_str().to_owned())
    }

    #[test]
    fn accepts_an_absolute_path() {
        let path = "/Contents/Björk/Ålbum/01 Ünïcödé.mp3";
        assert_eq!(parse(path), Ok(path.to_owned()));
    }

    #[test]
    fn rejects_a_relative_path() {
        assert_eq!(parse("Contents/a.mp3"), Err(PathError::NotAbsolute));
        assert_eq!(parse(""), Err(PathError::NotAbsolute));
    }

    #[test]
    fn rejects_an_empty_component() {
        for path in ["/", "//a", "/a//b", "/a/"] {
            assert_eq!(parse(path), Err(PathError::EmptyComponent), "{path}");
        }
    }

    #[test]
    fn rejects_a_dot_component() {
        for path in ["/.", "/a/./b", "/..", "/a/../b"] {
            assert_eq!(parse(path), Err(PathError::DotComponent), "{path}");
        }
        // Dots inside a name are fine.
        assert!(parse("/.a/..b/c.d").is_ok());
    }

    #[test]
    fn rejects_a_character_fat32_forbids() {
        for c in ['\\', ':', '*', '?', '"', '<', '>', '|'] {
            let path = format!("/Contents/a{c}b.mp3");
            assert_eq!(parse(&path), Err(PathError::ForbiddenCharacter(c)), "{c:?}");
        }
    }

    #[test]
    fn rejects_a_control_character() {
        for c in ['\0', '\t', '\n', '\u{1f}'] {
            let path = format!("/Contents/a{c}b.mp3");
            assert_eq!(parse(&path), Err(PathError::ForbiddenCharacter(c)), "{c:?}");
        }
        // DEL and everything above U+001F are fine.
        assert!(parse("/a\u{7f}b/\u{20}x").is_ok());
    }

    #[test]
    fn rejects_a_trailing_dot_or_space() {
        for path in ["/a./b", "/a/b.", "/a /b", "/a/b ", "/a/b. ."] {
            assert_eq!(parse(path), Err(PathError::TrailingDotOrSpace), "{path:?}");
        }
        // Leading ones are fine.
        assert!(parse("/ a/.b").is_ok());
    }

    #[test]
    fn rejects_a_component_over_255_utf16_units() {
        let ascii = "a".repeat(255);
        assert!(parse(&format!("/{ascii}")).is_ok());
        assert_eq!(
            parse(&format!("/{ascii}b")),
            Err(PathError::ComponentTooLong { units: 256 })
        );
        // U+1F3B5 is one char but two UTF-16 units: 127 of them plus one more unit fit.
        let notes = "\u{1f3b5}".repeat(127);
        assert!(parse(&format!("/{notes}a")).is_ok());
        assert_eq!(
            parse(&format!("/{notes}\u{1f3b5}")),
            Err(PathError::ComponentTooLong { units: 256 })
        );
    }

    #[test]
    fn rejects_a_path_over_257_utf16_units() {
        let a = "a".repeat(254);
        assert_eq!(parse(&format!("/{a}/b")).map(|p| utf16_len(&p)), Ok(257));
        assert_eq!(
            parse(&format!("/{a}a/b")),
            Err(PathError::PathTooLong { units: 258 })
        );
        // Counted in UTF-16 units: ö is one unit, U+1F3B5 two.
        assert_eq!(
            parse(&format!("/{}/\u{1f3b5}", "ö".repeat(254))),
            Err(PathError::PathTooLong { units: 258 })
        );
    }

    #[test]
    fn under_places_the_path_below_a_root() {
        use std::path::{Path, PathBuf};

        let path = StickPath::parse("/Contents/Björk/01 a b.mp3").unwrap();
        assert_eq!(
            path.under(Path::new("/mnt/stick")),
            PathBuf::from("/mnt/stick/Contents/Björk/01 a b.mp3")
        );
        let relative = Path::new("stick");
        let expected = relative.join("Contents").join("Björk").join("01 a b.mp3");
        assert_eq!(path.under(relative), expected);
    }

    #[test]
    fn file_name_is_the_last_component_and_display_is_the_path() {
        let path = StickPath::parse("/Contents/Björk/Ålbum/01 Ünïcödé.mp3").unwrap();
        assert_eq!(path.file_name(), "01 Ünïcödé.mp3");
        assert_eq!(path.to_string(), path.as_str());
        assert_eq!(StickPath::parse("/a").unwrap().file_name(), "a");
    }
}
