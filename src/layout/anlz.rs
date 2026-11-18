//! [`AnlzPath`]: where a track's analysis files go.

use super::StickPath;
use crate::pdb::TrackId;

/// The paths of a track's analysis files, derived from its id:
/// `/PIONEER/USBANLZ/P{low 12 bits}/{id}/ANLZ0000.{DAT,EXT}`, both numbers in upper-case
/// hex, 3 and 8 digits. Track 0x0abc_def0 gets `/PIONEER/USBANLZ/PEF0/0ABCDEF0/`.
///
/// This is rekordbox's shape: all 827 analysis paths of the real stick are
/// `/PIONEER/USBANLZ/P[0-9A-F]{3}/[0-9A-F]{8}/ANLZ[0-9]{4}.DAT`. rekordbox's numbers look
/// hashed, and 2 of its paths are numbered `ANLZ0001` because another track's analysis
/// is in the same directory. Here the 8-digit directory is the whole id, so distinct ids
/// always get distinct directories, every file is `ANLZ0000`, and the id can be read back
/// from the path.
///
/// The 3-digit directory uses the id's low bits rather than its high ones, so tracks spread
/// over up to 4,096 `P` directories instead of all landing in `P000`: a FAT32 directory
/// holds at most 65,536 entries. Every name is a valid upper-case 8.3 name, so none needs a
/// long-name entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnlzPath {
    id: TrackId,
}

impl AnlzPath {
    /// The analysis paths of track `id`.
    pub fn for_track(id: TrackId) -> Self {
        Self { id }
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
        let id = self.id.get();
        let path = format!(
            "/PIONEER/USBANLZ/P{:03X}/{id:08X}/ANLZ0000.{extension}",
            id & 0xfff
        );
        StickPath::parse(&path).expect("an analysis path is a valid stick path")
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn paths(id: u32) -> (String, String) {
        let path = AnlzPath::for_track(TrackId::new(id).unwrap());
        (path.dat().to_string(), path.ext().to_string())
    }

    #[test]
    fn derives_both_directories_from_the_track_id() {
        let cases = [
            (1, "P001/00000001"),
            (0xfff, "PFFF/00000FFF"),
            (0x1000, "P000/00001000"),
            (0xabc_def0, "PEF0/0ABCDEF0"),
            (u32::MAX, "PFFF/FFFFFFFF"),
        ];
        for (id, dirs) in cases {
            assert_eq!(
                paths(id).0,
                format!("/PIONEER/USBANLZ/{dirs}/ANLZ0000.DAT"),
                "{id:#x}"
            );
        }
    }

    #[test]
    fn ext_is_next_to_dat() {
        assert_eq!(
            paths(0xfff),
            (
                "/PIONEER/USBANLZ/PFFF/00000FFF/ANLZ0000.DAT".to_owned(),
                "/PIONEER/USBANLZ/PFFF/00000FFF/ANLZ0000.EXT".to_owned()
            )
        );
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
        #[test]
        fn paths_have_rekordbox_shape_and_give_back_the_id(id in 1..=u32::MAX) {
            let path = AnlzPath::for_track(TrackId::new(id).unwrap());
            for file in [path.dat(), path.ext()] {
                prop_assert_eq!(StickPath::parse(file.as_str()), Ok(file.clone()));
                let numbers = rekordbox_shape(file.as_str());
                prop_assert_eq!(numbers, Some((id & 0xfff, id)), "{}", file);
            }
            let (dat, ext) = (path.dat(), path.ext());
            prop_assert_eq!(dat.as_str().strip_suffix("DAT"), ext.as_str().strip_suffix("EXT"));
        }

        #[test]
        fn distinct_ids_give_distinct_paths(
            a in 1..=u32::MAX,
            other in 1..=u32::MAX,
            bit in 0..32u32,
        ) {
            // Any other id, and the ids one bit away, which catch a dropped bit.
            for b in [other, a ^ (1 << bit)] {
                if b == 0 || b == a {
                    continue;
                }
                let (pa, pb) = (paths(a), paths(b));
                prop_assert_ne!(&pa.0, &pb.0);
                prop_assert_ne!(&pa.1, &pb.1);
            }
        }
    }
}
