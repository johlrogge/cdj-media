//! The stick's path layout: where audio, analysis and artwork files go.
//!
//! Pure path logic: nothing here touches a file system.
//!
//! - [`StickPath`]: a validated, FAT32-safe absolute path on the stick.
//! - [`ContentPath`]: a track's audio file, `/Contents/<artist>/<album>/<file name>`, the
//!   track row's file path (slot 20). Its last component is the filename (slot 19).
//! - [`AnlzPath`]: a track's analysis files under `/PIONEER/USBANLZ/`, derived from the
//!   track id. Its `.DAT` path is the analysis path (slot 14).
//! - [`ArtworkPath`]: an artwork image under `/PIONEER/Artwork/`, derived from the artwork
//!   id: what the artwork row holds.
//! - [`ContentPathAllocator`]: makes the content paths of one stick distinct, as FAT32
//!   compares them (ADR-010).
//! - [`audio_path_tag`]: the `PPTH` tag of the analysis files, which repeats the file path.

mod allocator;
mod anlz;
mod artwork;
mod content;
mod stick_path;

pub use allocator::ContentPathAllocator;
pub use anlz::AnlzPath;
pub use artwork::ArtworkPath;
pub use content::ContentPath;
pub use stick_path::{MAX_COMPONENT_UNITS, MAX_PATH_UNITS, PathError, StickPath};

use crate::anlz::AudioPath;

/// The `PPTH` tag of a track's analysis files (`.DAT` and `.EXT`): the track's audio file
/// path, `file_path`, verbatim, so it equals the track row's file path (slot 20).
///
/// This holds on the real stick: for each of the 826 tracks whose analysis files exist
/// (one track's are missing), `PPTH` in both its `.DAT` and its `.EXT` equals slot 20.
pub fn audio_path_tag(file_path: &ContentPath) -> AudioPath {
    AudioPath::new(file_path.as_stick_path().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ppth_tag_holds_the_audio_file_path() {
        let path = ContentPath::for_track("Björk", "Ålbum", "01.mp3");
        assert_eq!(
            audio_path_tag(&path),
            AudioPath::new(path.as_stick_path().as_str())
        );
        assert_eq!(audio_path_tag(&path).path(), "/Contents/Björk/Ålbum/01.mp3");
    }
}
