//! [`ContentPath`]: where an audio file goes, `/Contents/<artist>/<album>/<file name>`.

use super::StickPath;
use super::stick_path::{is_forbidden, utf16_len};

/// The path of a track's audio file, `/Contents/<artist>/<album>/<file name>`, as
/// rekordbox lays it out: what the track row holds in slot 20
/// ([`TrackString::FilePath`](crate::pdb::TrackString::FilePath)).
///
/// It is its own type, not a bare [`StickPath`], so it can't be confused with an analysis
/// path ([`AnlzPath`](super::AnlzPath), slot 14). Convert it with
/// [`as_stick_path`](Self::as_stick_path) or `StickPath::from`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentPath(StickPath);

impl ContentPath {
    /// The path of a track's audio file: `/Contents/<artist>/<album>/<file_name>`.
    ///
    /// Each name is sanitised, never rejected, so any three strings give a path:
    /// 1. each of `/ \ : * ? " < > |` and each control character becomes `_`;
    /// 2. a name longer than 48 UTF-16 code units is cut to 48, never inside a character.
    ///    A file name keeps its extension (up to 7 units after the last dot that isn't
    ///    the first character): its stem is cut instead;
    /// 3. trailing spaces are dropped, then trailing dots become `_`;
    /// 4. a name that is left empty becomes `UnknownArtist`, `UnknownAlbum` or `Unknown`.
    ///
    /// Other characters, non-ASCII included, are kept as they are, in the form given (no
    /// Unicode normalisation). Sanitising is idempotent: the components of a result give
    /// back the same path, so names that are already sanitised pass through unchanged.
    ///
    /// These are rekordbox's rules and names: on the real stick, given each track's artist
    /// and album names (empty when it has none) and its file name, this gives back the file
    /// path (slot 20) of all 827 tracks.
    ///
    /// # File names
    ///
    /// The fallback `Unknown` has no extension, and a file name without one is kept
    /// without one; the players need the extension to know the file's type. Callers that
    /// lay out a stick must reject an empty or extension-less file name before calling
    /// this.
    ///
    /// Trailing spaces are dropped after the cut, so an extension that ends in spaces
    /// (`….mp3 `) is cut with its spaces counted and then loses them: the name ends up
    /// shorter than 48 units by the number of spaces.
    ///
    /// # Collisions
    ///
    /// Different tracks can get the same path: names that differ only in replaced
    /// characters (`AC/DC` and `AC:DC`), only after 48 units, or only in case (FAT32
    /// ignores case, see [`StickPath`]), and tracks with the same file name in the same
    /// album. This function doesn't resolve them; whoever lays out a whole stick has to
    /// compare the paths it gets.
    pub fn for_track(artist: &str, album: &str, file_name: &str) -> Self {
        let artist = or_unknown(sanitize(artist), UNKNOWN_ARTIST);
        let album = or_unknown(sanitize(album), UNKNOWN_ALBUM);
        let file_name = or_unknown(sanitize_file(file_name), UNKNOWN_FILE);
        let path = format!("/Contents/{artist}/{album}/{file_name}");
        Self(StickPath::parse(&path).expect("sanitised components make a valid path"))
    }

    /// The path as a [`StickPath`].
    pub fn as_stick_path(&self) -> &StickPath {
        &self.0
    }

    /// The audio file's name, the last component: what the track row holds in slot 19
    /// ([`TrackString::Filename`](crate::pdb::TrackString::Filename)).
    pub fn file_name(&self) -> &str {
        self.0.file_name()
    }
}

impl From<ContentPath> for StickPath {
    fn from(path: ContentPath) -> Self {
        path.0
    }
}

/// The longest component under `/Contents/`, in UTF-16 code units. No component of the
/// real stick's 827 audio paths is longer, and rekordbox cuts longer artist, album and
/// file names to this length (277 paths have a component of exactly 48). A whole content
/// path is then at most 10 + 3 × 48 + 2 = 156 units, well within
/// [`MAX_PATH_UNITS`](super::MAX_PATH_UNITS).
const MAX_UNITS: usize = 48;

/// The artist directory of a track whose artist sanitises to nothing.
const UNKNOWN_ARTIST: &str = "UnknownArtist";
/// The album directory of a track whose album sanitises to nothing.
const UNKNOWN_ALBUM: &str = "UnknownAlbum";
/// The file name of a track whose file name sanitises to nothing.
const UNKNOWN_FILE: &str = "Unknown";

/// `name`, or `fallback` when it is empty.
fn or_unknown(name: String, fallback: &str) -> String {
    if name.is_empty() {
        fallback.to_owned()
    } else {
        name
    }
}

/// Makes `name` a valid directory name: see [`ContentPath::for_track`].
fn sanitize(name: &str) -> String {
    fix_end(cut(&replace_forbidden(name), MAX_UNITS).to_owned())
}

/// Makes `name` a valid file name: as [`sanitize`], but a cut keeps the extension.
fn sanitize_file(name: &str) -> String {
    let name = replace_forbidden(name);
    let cut = match extension_at(&name) {
        Some(at) if utf16_len(&name) > MAX_UNITS => {
            let (stem, extension) = name.split_at(at);
            format!("{}{extension}", cut(stem, MAX_UNITS - utf16_len(extension)))
        }
        _ => cut(&name, MAX_UNITS).to_owned(),
    };
    fix_end(cut)
}

/// The longest extension a cut keeps, in UTF-16 code units without its dot. Audio
/// extensions are 3 or 4; a longer "extension" is part of the name.
const MAX_EXTENSION_UNITS: usize = 7;

/// Where `name`'s extension starts (its dot), if it has one: a dot that is not the first
/// character, followed by 1 to [`MAX_EXTENSION_UNITS`] units.
fn extension_at(name: &str) -> Option<usize> {
    let at = name.rfind('.').filter(|&at| at > 0)?;
    let units = utf16_len(&name[at + 1..]);
    (1..=MAX_EXTENSION_UNITS).contains(&units).then_some(at)
}

/// Replaces each character a component can't hold, `/` included, with `_`.
fn replace_forbidden(name: &str) -> String {
    name.chars()
        .map(|c| if c == '/' || is_forbidden(c) { '_' } else { c })
        .collect()
}

/// The longest prefix of `s` that is at most `max` UTF-16 code units long.
fn cut(s: &str, max: usize) -> &str {
    let mut units = 0;
    for (at, c) in s.char_indices() {
        units += c.len_utf16();
        if units > max {
            return &s[..at];
        }
    }
    s
}

/// Drops trailing spaces and turns the trailing dots before them into `_`, as rekordbox
/// does, so the name ends in neither.
fn fix_end(name: String) -> String {
    let trimmed = name.trim_end_matches(' ');
    let stem = trimmed.trim_end_matches('.');
    let dots = trimmed.len() - stem.len();
    format!("{stem}{}", "_".repeat(dots))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn builds_contents_artist_album_file() {
        let path = ContentPath::for_track("Björk", "Ålbum", "01 Ünïcödé.mp3");
        assert_eq!(
            path.as_stick_path().as_str(),
            "/Contents/Björk/Ålbum/01 Ünïcödé.mp3"
        );
        assert_eq!(path.file_name(), "01 Ünïcödé.mp3");
    }

    #[test]
    fn a_content_path_is_a_stick_path_with_a_file_name() {
        let path: ContentPath = ContentPath::for_track("A", "B", "c.mp3");
        let stick: &StickPath = path.as_stick_path();
        assert_eq!(stick.as_str(), "/Contents/A/B/c.mp3");
        assert_eq!(path.file_name(), "c.mp3");
        assert_eq!(StickPath::from(path.clone()), *path.as_stick_path());
    }

    #[test]
    fn replaces_each_forbidden_or_control_character_with_an_underscore() {
        let cases = [
            ("AC/DC", "AC_DC"),
            ("Re: Mix", "Re_ Mix"),
            ("Why?", "Why_"),
            ("\"Quoted\"", "_Quoted_"),
            ("<>", "__"),
            ("A|B", "A_B"),
            ("C:\\Path", "C__Path"),
            ("*", "_"),
            ("tab\there", "tab_here"),
            ("new\nline\0", "new_line_"),
            ("\u{1f}x", "_x"),
            // Kept as is.
            ("Björk", "Björk"),
            ("Ålbum", "Ålbum"),
            ("Ünïcödé", "Ünïcödé"),
            ("音楽 \u{1f3b5}", "音楽 \u{1f3b5}"),
            ("a_b-c (d) [e] & f'g", "a_b-c (d) [e] & f'g"),
        ];
        for (name, expected) in cases {
            assert_eq!(sanitize(name), expected, "{name:?}");
        }
    }

    /// As rekordbox does: a trailing dot becomes `_`, a trailing space is dropped.
    #[test]
    fn trims_trailing_spaces_and_replaces_trailing_dots() {
        let cases = [
            ("Inc.", "Inc_"),
            ("Wait...", "Wait___"),
            ("Space ", "Space"),
            ("Spaces \t ", "Spaces _"),
            ("Dot then space. ", "Dot then space_"),
            ("Mixed . . ", "Mixed . _"),
            (".", "_"),
            ("..", "__"),
            // Leading and inner ones stay.
            (" .Lead", " .Lead"),
            ("Mr. X", "Mr. X"),
        ];
        for (name, expected) in cases {
            assert_eq!(sanitize(name), expected, "{name:?}");
        }
    }

    #[test]
    fn empty_names_become_the_unknown_names() {
        for empty in ["", " ", "   "] {
            assert_eq!(
                ContentPath::for_track(empty, empty, empty)
                    .as_stick_path()
                    .as_str(),
                "/Contents/UnknownArtist/UnknownAlbum/Unknown",
                "{empty:?}"
            );
        }
        assert_eq!(
            ContentPath::for_track("", "B", "c.mp3")
                .as_stick_path()
                .as_str(),
            "/Contents/UnknownArtist/B/c.mp3"
        );
    }

    fn components(path: &ContentPath) -> Vec<String> {
        path.as_stick_path()
            .as_str()
            .split('/')
            .skip(2)
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn cuts_long_directory_names_to_48_utf16_units() {
        let long = "a".repeat(100);
        let path = ContentPath::for_track(&long, &long, "c.mp3");
        assert_eq!(
            components(&path),
            ["a".repeat(48), "a".repeat(48), "c.mp3".into()]
        );
        // Counted in UTF-16 units; a character is never split.
        let notes = "\u{1f3b5}".repeat(30);
        let path = ContentPath::for_track(&notes, &format!("ö{notes}"), "c.mp3");
        assert_eq!(
            components(&path)[..2],
            [
                "\u{1f3b5}".repeat(24),
                format!("ö{}", "\u{1f3b5}".repeat(23))
            ]
        );
        // The cut name loses a trailing space, as on the stick, and fixes a trailing dot.
        let spaced = format!("{} b", "a".repeat(47));
        let dotted = format!("{}.b", "a".repeat(47));
        let path = ContentPath::for_track(&spaced, &dotted, "c.mp3");
        assert_eq!(
            components(&path)[..2],
            ["a".repeat(47), format!("{}_", "a".repeat(47))]
        );
    }

    fn file(name: &str) -> String {
        ContentPath::for_track("A", "B", name)
            .file_name()
            .to_owned()
    }

    #[test]
    fn cuts_long_file_names_to_48_utf16_units_keeping_the_extension() {
        let a = |n: usize| "a".repeat(n);
        let cases = [
            (format!("{}.mp3", a(100)), format!("{}.mp3", a(44))),
            (format!("{}.flac", a(100)), format!("{}.flac", a(43))),
            (format!("{}.mp3", a(44)), format!("{}.mp3", a(44))),
            // A cut stem may end in a space, as 28 names on the stick do.
            (
                format!("{} {}.aiff", a(42), a(9)),
                format!("{} .aiff", a(42)),
            ),
            // Spaces after the extension count in the cut, then are dropped: 47 units.
            (format!("{}.mp3 ", a(45)), format!("{}.mp3", a(43))),
            // No extension: cut as a whole.
            (a(100), a(48)),
            // A "extension" longer than 7 units is not one.
            (
                format!("a.{}", "b".repeat(60)),
                format!("a.{}", "b".repeat(46)),
            ),
            // A leading dot does not start an extension.
            (format!(".{}", a(60)), format!(".{}", a(47))),
            // Non-ASCII stem, counted in UTF-16 units.
            (
                format!("{}.mp3", "\u{1f3b5}".repeat(30)),
                format!("{}.mp3", "\u{1f3b5}".repeat(22)),
            ),
        ];
        for (name, expected) in cases {
            assert_eq!(file(&name), expected, "{name:?}");
        }
        // Directories have no extension.
        let album = format!("{}.2", "x".repeat(60));
        let path = ContentPath::for_track("A", &album, "c.mp3");
        assert_eq!(components(&path)[1], "x".repeat(48));
    }

    /// Names rich in what sanitising handles: forbidden and control characters, dots,
    /// spaces, characters of 1 and 2 UTF-16 units, and lengths around the limit.
    fn tricky_name() -> impl Strategy<Value = String> {
        prop_oneof![
            any::<String>(),
            "[a. /\\\\:*?\"<>|\\x00-\\x1f\u{e9}\u{1f3b5}]{0,120}",
            "[a\u{1f3b5}]{40,60}(\\.[a-z ]{0,9})?[ .]{0,3}",
        ]
    }

    proptest! {
        #[test]
        fn any_names_give_a_valid_contents_path(
            artist in tricky_name(),
            album in tricky_name(),
            file_name in tricky_name(),
        ) {
            let path = ContentPath::for_track(&artist, &album, &file_name);
            let stick = path.as_stick_path();
            prop_assert_eq!(StickPath::parse(stick.as_str()), Ok(stick.clone()));
            prop_assert!(stick.as_str().starts_with("/Contents/"));
            let parts = components(&path);
            prop_assert_eq!(parts.len(), 3);
            for part in &parts {
                prop_assert!(utf16_len(part) <= MAX_UNITS, "{part:?}");
            }
        }

        /// Names that are already sanitised come back unchanged.
        #[test]
        fn sanitising_twice_changes_nothing(
            artist in tricky_name(),
            album in tricky_name(),
            file_name in tricky_name(),
        ) {
            let path = ContentPath::for_track(&artist, &album, &file_name);
            let parts = components(&path);
            let again = ContentPath::for_track(&parts[0], &parts[1], &parts[2]);
            prop_assert_eq!(again, path);
        }
    }
}
