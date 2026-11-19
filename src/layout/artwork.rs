//! [`ArtworkPath`]: where a track's artwork image goes.

use super::StickPath;
use crate::pdb::ArtworkId;

/// The path of an artwork image, derived from its artwork id, as rekordbox lays it out:
/// `/PIONEER/Artwork/{id / 20 + 1:05}/a{id}.jpg`. Directory `00001` holds ids 1 to 19 and
/// each further directory 20 ids: artwork 20 is `/PIONEER/Artwork/00002/a20.jpg`.
///
/// This is rekordbox's scheme: it gives back the path of every artwork row of both local
/// fixtures, the 5 of a fresh export and the 787 of a long-used stick (ids 1 to 835 with
/// gaps, in 42 directories). Neither has an `_m` variant.
///
/// It holds no value: [`for_artwork`](Self::for_artwork) is the path, a plain
/// [`StickPath`], which is what the artwork row holds.
#[derive(Debug)]
pub enum ArtworkPath {}

impl ArtworkPath {
    /// The path of artwork `id`'s image.
    pub fn for_artwork(id: ArtworkId) -> StickPath {
        let id = id.get();
        let path = format!("/PIONEER/Artwork/{:05}/a{id}.jpg", id / 20 + 1);
        StickPath::parse(&path).expect("an artwork path is a valid stick path")
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn path(id: u32) -> String {
        ArtworkPath::for_artwork(ArtworkId::new(id).unwrap()).to_string()
    }

    #[test]
    fn twenty_ids_per_directory_the_first_holding_1_to_19() {
        let cases = [
            (1, "00001/a1.jpg"),
            (19, "00001/a19.jpg"),
            (20, "00002/a20.jpg"),
            (39, "00002/a39.jpg"),
            (40, "00003/a40.jpg"),
            (835, "00042/a835.jpg"),
            (1_999_999, "100000/a1999999.jpg"),
            (u32::MAX, "214748365/a4294967295.jpg"),
        ];
        for (id, rest) in cases {
            assert_eq!(path(id), format!("/PIONEER/Artwork/{rest}"), "{id}");
        }
    }

    proptest! {
        #[test]
        fn every_id_gives_a_valid_path_that_names_it(id in 1..=u32::MAX) {
            let path = ArtworkPath::for_artwork(ArtworkId::new(id).unwrap());
            prop_assert_eq!(StickPath::parse(path.as_str()), Ok(path.clone()));
            prop_assert_eq!(path.file_name(), format!("a{id}.jpg"));
        }
    }
}
