//! [`AnlzPath`]: where a track's analysis files go.

use super::{ContentPath, StickPath};

/// The highest number of an analysis file in its folder, `ANLZ9999`: the most four digits
/// hold, keeping the name an 8.3 name.
pub const MAX_ANLZ_INDEX: u16 = 9999;

/// The paths of a track's analysis files, in the folder hashed from its audio file's path
/// ([`anlz_folder`]) and numbered within it (ADR-019):
/// `/PIONEER/USBANLZ/P{d1:03X}/{d2:08X}/ANLZ{index:04}.{DAT,EXT}`.
///
/// This is rekordbox's layout, and where the players look: every analysis file of the real
/// stick whose `PPTH` is a content path is in its path's folder. Paths that hash to the same
/// folder share it, and their files are numbered `ANLZ0000`, `ANLZ0001`, …; whoever lays out
/// a whole stick has to give each its own number, at most [`MAX_ANLZ_INDEX`], so every name is a
/// valid upper-case 8.3 name and none needs a long-name entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnlzPath {
    folder: StickPath,
    index: u16,
}

impl AnlzPath {
    /// The analysis paths of the audio file at `path`, numbered `index` in its folder;
    /// `None` when `index` is above [`MAX_ANLZ_INDEX`], whose names wouldn't be 8.3 names.
    pub fn for_content(path: &ContentPath, index: u16) -> Option<Self> {
        (index <= MAX_ANLZ_INDEX).then(|| Self {
            folder: anlz_folder(path),
            index,
        })
    }

    /// The `.DAT` file's path: what the track row holds in slot 14
    /// ([`TrackString::AnalysisPath`](crate::pdb::TrackString::AnalysisPath)).
    pub fn dat(&self) -> StickPath {
        self.file("DAT")
    }

    /// The `.EXT` file's path, next to the `.DAT` file.
    pub fn ext(&self) -> StickPath {
        self.file("EXT")
    }

    /// The analysis file with extension `extension`.
    fn file(&self, extension: &str) -> StickPath {
        let path = format!("{}/ANLZ{:04}.{extension}", self.folder, self.index);
        StickPath::parse(&path).expect("an analysis path is a valid stick path")
    }
}

/// The folder of the analysis files of the audio file at `path`, where the players look for
/// them (ADR-019): `/PIONEER/USBANLZ/P{d1:03X}/{d2:08X}`, two numbers hashed from the
/// path's UTF-16 code units. The hash is rekordbox's: it gives the folder of all 827
/// `ANLZ0000` and all 24 `ANLZ0001` files of the real stick.
///
/// Different paths can share a folder; their files are told apart by number
/// ([`AnlzPath::for_content`]) and by their `PPTH`.
pub fn anlz_folder(path: &ContentPath) -> StickPath {
    folder_of(path.as_stick_path().as_str())
}

/// The factor of the hash of the path's code units.
const PATH_FACTOR: u32 = 0x34F5_501D;
/// The factor applied to the path's hash before the modulus.
const SCATTER_FACTOR: u32 = 0x93B6;
/// The number of 8-digit folders.
const FOLDERS: u32 = 200_003;
/// The bits of the 8-digit number that make up the 3-digit one, lowest first.
const P_BITS: [u32; 7] = [0, 2, 6, 7, 9, 13, 16];

/// The analysis folder of `path`, any text: see [`anlz_folder`].
fn folder_of(path: &str) -> StickPath {
    let hash = path.encode_utf16().fold(0u32, |h, unit| {
        h.wrapping_mul(PATH_FACTOR).wrapping_add(u32::from(unit))
    });
    let number = hash.wrapping_mul(SCATTER_FACTOR) % FOLDERS;
    let p = P_BITS
        .iter()
        .enumerate()
        .fold(0, |p, (to, &from)| p | ((number >> from) & 1) << to);
    StickPath::parse(&format!("/PIONEER/USBANLZ/P{p:03X}/{number:08X}"))
        .expect("an analysis folder is a valid stick path")
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Values computed by hand from ADR-019's definition, independently of this code.
    #[test]
    fn the_folder_is_adr_019s_hash_of_the_utf16_path() {
        let cases = [
            // h = 0x2F; nothing wraps.
            ("/", "P024/0002B452"),
            // h = 0x2F * 0x34F5501D + 0x61 wraps past 2^32, and so does h * 0x93B6.
            ("/a", "P074/00017648"),
            // U+1F600 is two code units, 0xD83D 0xDE00, each hashed in turn.
            ("/\u{1F600}", "P003/00020117"),
            // From the coordinator's independent script.
            ("/Contents/a.mp3", "P068/000124BA"),
            (
                "/Contents/UnknownArtist/UnknownAlbum/x.aiff",
                "P025/0000F979",
            ),
            ("/Contents/\u{1F600}/b/c.wav", "P036/0000667C"),
        ];
        for (path, folder) in cases {
            assert_eq!(
                folder_of(path).as_str(),
                format!("/PIONEER/USBANLZ/{folder}"),
                "{path:?}"
            );
        }
    }

    #[test]
    fn anlz_folder_hashes_the_content_paths_text() {
        let path = ContentPath::for_track("Björk", "Ålbum", "01 \u{1F600}.aiff");
        assert_eq!(
            anlz_folder(&path),
            folder_of("/Contents/Björk/Ålbum/01 \u{1F600}.aiff")
        );
    }

    #[test]
    fn for_content_numbers_the_files_in_the_content_paths_folder() {
        let content = ContentPath::for_track("A", "B", "c.aiff");
        let folder = anlz_folder(&content);
        for (index, name) in [
            (0, "ANLZ0000"),
            (1, "ANLZ0001"),
            (42, "ANLZ0042"),
            (9999, "ANLZ9999"),
        ] {
            let path = AnlzPath::for_content(&content, index).unwrap();
            assert_eq!(path.dat().as_str(), format!("{folder}/{name}.DAT"));
            assert_eq!(path.ext().as_str(), format!("{folder}/{name}.EXT"));
        }
    }

    /// `ANLZ9999` is the last 8.3 name: a five-digit number is refused.
    #[test]
    fn for_content_refuses_a_number_past_9999() {
        let content = ContentPath::for_track("A", "B", "c.aiff");
        assert_eq!(MAX_ANLZ_INDEX, 9999);
        for index in [10_000, u16::MAX] {
            assert_eq!(AnlzPath::for_content(&content, index), None, "{index}");
        }
    }

    /// The two hex numbers of a path of rekordbox's shape,
    /// `^/PIONEER/USBANLZ/P[0-9A-F]{3}/[0-9A-F]{8}/ANLZ[0-9]{4}\.(DAT|EXT)$`.
    fn rekordbox_shape(path: &str) -> Option<(u32, u32)> {
        let upper_hex = |s: &str, len: usize| {
            (s.len() == len && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'A'..=b'F')))
                .then(|| u32::from_str_radix(s, 16).ok())
                .flatten()
        };
        let rest = path.strip_prefix("/PIONEER/USBANLZ/P")?;
        let [dir, sub, file] = rest.split('/').collect::<Vec<_>>()[..] else {
            return None;
        };
        let number = file
            .strip_prefix("ANLZ")?
            .strip_suffix(".DAT")
            .or_else(|| file.strip_prefix("ANLZ")?.strip_suffix(".EXT"))?;
        (number.len() == 4 && number.bytes().all(|c| c.is_ascii_digit())).then_some(())?;
        Some((upper_hex(dir, 3)?, upper_hex(sub, 8)?))
    }

    proptest! {
        /// Any path's files have rekordbox's shape: an 8-digit number below 200,003, and a
        /// 3-digit one made of its bits 0, 2, 6, 7, 9, 13 and 16.
        #[test]
        fn paths_have_rekordbox_shape(
            artist in any::<String>(),
            album in "[a\u{e9}\u{1f3b5}]{0,60}",
            file_name in "[a\u{1f3b5}]{1,50}\\.aiff",
            index in 0..=MAX_ANLZ_INDEX,
        ) {
            let content = ContentPath::for_track(&artist, &album, &file_name);
            let path = AnlzPath::for_content(&content, index).expect("a number up to 9999");
            for file in [path.dat(), path.ext()] {
                prop_assert_eq!(StickPath::parse(file.as_str()), Ok(file.clone()));
                prop_assert!(file.as_str().starts_with(anlz_folder(&content).as_str()));
                let (p, number) = rekordbox_shape(file.as_str()).expect("rekordbox's shape");
                prop_assert!(number < 200_003, "{}", file);
                let bits = [0, 2, 6, 7, 9, 13, 16].map(|bit| (number >> bit) & 1);
                let expected = bits.iter().rev().fold(0, |p, bit| p << 1 | bit);
                prop_assert_eq!(p, expected, "{}", file);
            }
            let (dat, ext) = (path.dat(), path.ext());
            prop_assert_eq!(dat.as_str().strip_suffix("DAT"), ext.as_str().strip_suffix("EXT"));
        }
    }
}
