//! Rows of the playlist tree (table 7) and the playlist entries (table 8).

use super::super::defaults;
use super::super::{PlaylistId, RawRow, TrackId};
use super::read_id;
use crate::bytes::Writer;
use crate::encoding::DeviceSqlString;
use crate::encoding::devicesql;
use crate::error::{DecodeContext, DecodeError, EncodeError};

/// A playlist tree row (table 7): a playlist or a folder.
///
/// | bytes     | field           | meaning                                              |
/// |-----------|-----------------|------------------------------------------------------|
/// | 0x00..04  | `parent_id`     | the folder it is in; 0 for the root                  |
/// | 0x04..08  | `unknown`       | not understood                                       |
/// | 0x08..0c  | `sort_order`    | its place among the parent's children                |
/// | 0x0c..10  | `id`            | [`PlaylistId`], never 0                              |
/// | 0x10..14  | `raw_is_folder` | non-zero for a folder                                |
/// | 0x14..    | `name`          | DeviceSQL string                                     |
///
/// The row ends after its name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlaylistNode {
    parent_id: Option<PlaylistId>,
    unknown: u32,
    sort_order: u32,
    id: PlaylistId,
    raw_is_folder: u32,
    name: DeviceSqlString,
}

impl PlaylistNode {
    /// Creates a tree row as rekordbox does: `unknown` is rekordbox's default, a folder's
    /// `raw_is_folder` is 1, and the name gets rekordbox's string form.
    pub fn new(
        id: PlaylistId,
        parent_id: Option<PlaylistId>,
        sort_order: u32,
        is_folder: bool,
        name: &str,
    ) -> Self {
        Self {
            parent_id,
            unknown: defaults::PLAYLIST_UNKNOWN,
            sort_order,
            id,
            raw_is_folder: u32::from(is_folder),
            name: DeviceSqlString::new(name),
        }
    }

    /// Replaces `unknown` (offset 4).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_unknown(mut self, unknown: u32) -> Self {
        self.unknown = unknown;
        self
    }

    /// Replaces `raw_is_folder` (offset 0x10).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-007)")
    )]
    pub(crate) fn with_raw_is_folder(mut self, raw_is_folder: u32) -> Self {
        self.raw_is_folder = raw_is_folder;
        self
    }

    /// The row's id.
    pub fn id(&self) -> PlaylistId {
        self.id
    }

    /// The folder the playlist or folder is in; `None` for the root.
    pub fn parent_id(&self) -> Option<PlaylistId> {
        self.parent_id
    }

    /// `unknown` (offset 4), not understood.
    pub fn unknown(&self) -> u32 {
        self.unknown
    }

    /// The row's place among its parent's children.
    pub fn sort_order(&self) -> u32 {
        self.sort_order
    }

    /// The folder flag (offset 0x10), as stored.
    pub fn raw_is_folder(&self) -> u32 {
        self.raw_is_folder
    }

    /// Whether the row is a folder: a non-zero `raw_is_folder`.
    pub fn is_folder(&self) -> bool {
        self.raw_is_folder != 0
    }

    /// The name, with its stored form.
    pub fn name(&self) -> &DeviceSqlString {
        &self.name
    }

    /// Decodes a playlist tree row.
    ///
    /// # Errors
    ///
    /// [`UnexpectedEof`](crate::DecodeErrorKind::UnexpectedEof) where the row is cut off,
    /// [`InvalidValue`](crate::DecodeErrorKind::InvalidValue) at an `id` of 0, and the
    /// string decoder's errors, all at absolute offsets.
    pub fn decode(row: &RawRow<'_>) -> Result<Self, DecodeError> {
        let mut r = row.reader();
        let parent_id = PlaylistId::new(r.u32_le().context("pdb playlist parent_id")?);
        let unknown = r.u32_le().context("pdb playlist unknown")?;
        let sort_order = r.u32_le().context("pdb playlist sort_order")?;
        let id = read_id(&mut r, "pdb playlist id", PlaylistId::new)?;
        let raw_is_folder = r.u32_le().context("pdb playlist is_folder")?;
        let name = devicesql::decode(&mut r)?;
        Ok(Self {
            parent_id,
            unknown,
            sort_order,
            id,
            raw_is_folder,
            name,
        })
    }

    /// Encodes the row: the row's bytes.
    ///
    /// # Errors
    ///
    /// [`EncodeError::LengthOverflow`] for a name too long for its form.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut w = Writer::new();
        w.u32_le(self.parent_id.map_or(0, PlaylistId::get));
        w.u32_le(self.unknown);
        w.u32_le(self.sort_order);
        w.u32_le(self.id.get());
        w.u32_le(self.raw_is_folder);
        self.name.encode(&mut w)?;
        Ok(w.into_inner())
    }
}

/// A playlist entry row (table 8): one track of a playlist.
///
/// | bytes     | field         | meaning                               |
/// |-----------|---------------|---------------------------------------|
/// | 0x00..04  | `entry_index` | the track's place in the playlist     |
/// | 0x04..08  | `track_id`    | the track; 0 for none                 |
/// | 0x08..0c  | `playlist_id` | the playlist; 0 for none              |
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlaylistEntry {
    entry_index: u32,
    track_id: Option<TrackId>,
    playlist_id: Option<PlaylistId>,
}

impl PlaylistEntry {
    /// Creates an entry row: track `track_id` at place `entry_index` of playlist
    /// `playlist_id`.
    pub fn new(entry_index: u32, track_id: TrackId, playlist_id: PlaylistId) -> Self {
        Self {
            entry_index,
            track_id: Some(track_id),
            playlist_id: Some(playlist_id),
        }
    }

    /// The track's place in the playlist, as stored.
    pub fn entry_index(&self) -> u32 {
        self.entry_index
    }

    /// The track; `None` for a reference of 0.
    pub fn track_id(&self) -> Option<TrackId> {
        self.track_id
    }

    /// The playlist; `None` for a reference of 0.
    pub fn playlist_id(&self) -> Option<PlaylistId> {
        self.playlist_id
    }

    /// Decodes a playlist entry row.
    ///
    /// # Errors
    ///
    /// [`UnexpectedEof`](crate::DecodeErrorKind::UnexpectedEof) at the field where the row
    /// is cut off, at its absolute offset.
    pub fn decode(row: &RawRow<'_>) -> Result<Self, DecodeError> {
        let mut r = row.reader();
        let entry_index = r.u32_le().context("pdb playlist entry index")?;
        let track_id = TrackId::new(r.u32_le().context("pdb playlist entry track_id")?);
        let playlist_id = r.u32_le().context("pdb playlist entry playlist_id")?;
        Ok(Self {
            entry_index,
            track_id,
            playlist_id: PlaylistId::new(playlist_id),
        })
    }

    /// Encodes the row: its 12 bytes.
    ///
    /// # Errors
    ///
    /// None; the result matches the other row types.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut w = Writer::new();
        w.u32_le(self.entry_index);
        w.u32_le(self.track_id.map_or(0, TrackId::get));
        w.u32_le(self.playlist_id.map_or(0, PlaylistId::get));
        Ok(w.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{AT, raw};
    use super::*;
    use crate::error::DecodeErrorKind;

    fn playlist(id: u32) -> PlaylistId {
        PlaylistId::new(id).unwrap()
    }

    /// Playlist 1 at the root, "Warmup", as the fresh export's one row lies (the name is
    /// made up), followed by a byte of the next row.
    const WARMUP: [u8; 28] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0x0f, b'W', b'a', b'r', b'm',
        b'u', b'p', 0xee,
    ];

    fn located(e: DecodeError) -> (u64, &'static str, DecodeErrorKind) {
        (e.offset, e.context, e.kind)
    }

    #[test]
    fn a_tree_row_decodes_and_re_encodes_its_bytes() {
        let node = PlaylistNode::decode(&raw(&WARMUP)).unwrap();
        assert_eq!(node.id(), playlist(1));
        assert_eq!(node.parent_id(), None);
        assert_eq!((node.unknown(), node.sort_order()), (0, 0));
        assert_eq!((node.raw_is_folder(), node.is_folder()), (0, false));
        assert_eq!(node.name().as_str(), "Warmup");
        assert_eq!(node.encode().unwrap(), WARMUP[..27]);
    }

    #[test]
    fn a_tree_row_keeps_its_unknowns() {
        let mut bytes = Vec::new();
        for value in [7u32, 0x1234_5678, 3, 9, 0x100] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        // "Ö" as UTF-16LE.
        bytes.extend_from_slice(&[0x90, 6, 0, 0, 0xd6, 0]);
        let node = PlaylistNode::decode(&raw(&bytes)).unwrap();
        assert_eq!(node.parent_id(), Some(playlist(7)));
        assert_eq!((node.unknown(), node.sort_order()), (0x1234_5678, 3));
        assert_eq!(node.id(), playlist(9));
        assert_eq!((node.raw_is_folder(), node.is_folder()), (0x100, true));
        assert_eq!(node.name().as_str(), "Ö");
        assert_eq!(node.encode().unwrap(), bytes);
        let built = PlaylistNode::new(playlist(9), Some(playlist(7)), 3, true, "Ö")
            .with_unknown(0x1234_5678)
            .with_raw_is_folder(0x100);
        assert_eq!(built, node);
    }

    #[test]
    fn the_tree_row_builder_gives_rekordbox_layout() {
        let built = PlaylistNode::new(playlist(1), None, 0, false, "Warmup");
        assert_eq!(built.encode().unwrap(), WARMUP[..27]);
        let folder = PlaylistNode::new(playlist(2), Some(playlist(1)), 4, true, "");
        let bytes = folder.encode().unwrap();
        let fixed = [1, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0];
        assert_eq!(bytes[..0x14], fixed);
        assert_eq!(bytes[0x14..], [0x03]);
    }

    #[test]
    fn tree_row_errors_are_at_the_field() {
        let mut bytes = WARMUP;
        bytes[0x0c] = 0;
        let e = PlaylistNode::decode(&raw(&bytes)).unwrap_err();
        assert_eq!(
            located(e),
            (AT + 0x0c, "pdb playlist id", DecodeErrorKind::InvalidValue)
        );
        let e = PlaylistNode::decode(&raw(&WARMUP[..6])).unwrap_err();
        assert_eq!(place(e), (AT + 4, "pdb playlist unknown"));
        let e = PlaylistNode::decode(&raw(&WARMUP[..0x12])).unwrap_err();
        assert_eq!(place(e), (AT + 0x10, "pdb playlist is_folder"));
        let e = PlaylistNode::decode(&raw(&WARMUP[..0x16])).unwrap_err();
        assert_eq!(e.offset, AT + 0x15);
    }

    fn place(e: DecodeError) -> (u64, &'static str) {
        (e.offset, e.context)
    }

    /// Entry 3 of playlist 1, track 6, followed by a byte of the next row.
    const ENTRY: [u8; 13] = [3, 0, 0, 0, 6, 0, 0, 0, 1, 0, 0, 0, 0xee];

    #[test]
    fn an_entry_row_is_three_u32s() {
        let entry = PlaylistEntry::decode(&raw(&ENTRY)).unwrap();
        assert_eq!(entry.entry_index(), 3);
        assert_eq!(entry.track_id(), TrackId::new(6));
        assert_eq!(entry.playlist_id(), PlaylistId::new(1));
        assert_eq!(entry.encode().unwrap(), ENTRY[..12]);
        let built = PlaylistEntry::new(3, TrackId::new(6).unwrap(), playlist(1));
        assert_eq!(built, entry);
    }

    #[test]
    fn an_entry_of_no_track_or_playlist_keeps_its_zeros() {
        let bytes = [0u8; 12];
        let entry = PlaylistEntry::decode(&raw(&bytes)).unwrap();
        assert_eq!((entry.track_id(), entry.playlist_id()), (None, None));
        assert_eq!(entry.encode().unwrap(), bytes);
    }

    #[test]
    fn entry_row_errors_are_at_the_field() {
        let e = PlaylistEntry::decode(&raw(&ENTRY[..2])).unwrap_err();
        assert_eq!(place(e), (AT, "pdb playlist entry index"));
        let e = PlaylistEntry::decode(&raw(&ENTRY[..6])).unwrap_err();
        assert_eq!(place(e), (AT + 4, "pdb playlist entry track_id"));
        let e = PlaylistEntry::decode(&raw(&ENTRY[..10])).unwrap_err();
        assert_eq!(place(e), (AT + 8, "pdb playlist entry playlist_id"));
    }
}
