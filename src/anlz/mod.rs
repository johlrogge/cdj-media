//! ANLZ analysis files (`.DAT`, `.EXT`, `.2EX`): the `PMAI` container and its tags.
//!
//! Every multi-byte integer is big-endian. The file header is:
//!
//! | bytes            | field        | meaning                                        |
//! |------------------|--------------|------------------------------------------------|
//! | 0..4             | magic        | `PMAI`                                         |
//! | 4..8             | `len_header` | length of the file header (28 in real files)   |
//! | 8..12            | `len_file`   | length of the whole file                       |
//! | 12..`len_header` | unknown      | not understood; kept verbatim                  |
//!
//! Tags follow from offset `len_header` and tile the rest of the file exactly. See
//! [`Tag`] for the tag framing.
//!
//! `PPTH`, `PQTZ`, `PCOB`, `PWAV`, `PWV2` and `PWV3` are decoded into [`AudioPath`],
//! [`BeatGrid`], [`CueList`], [`WaveformPreview`], [`TinyWaveformPreview`] and
//! [`WaveformDetail`] (see ADR-004); every other tag is kept as a [`RawTag`].
//! [`AnlzFile::track_dat`] builds a whole `.DAT` file from typed values, and
//! [`AnlzFile::track_ext`] the Phase 1 part of an `.EXT` file.

mod beat_grid;
mod cues;
pub(crate) mod defaults;
mod path;
mod tag;
mod waveform;

pub use beat_grid::{Beat, BeatGrid, ConstantGrid};
pub use cues::{Cue, CueList, CueListKind, CueSpan, HotCueSlot};
pub use path::AudioPath;
pub use tag::{FourCc, RawTag, Tag};
pub use waveform::{
    PREVIEW_COLUMNS, TINY_PREVIEW_COLUMNS, TinyWaveformPreview, WaveformDetail, WaveformPreview,
};

use crate::bytes::{Reader, Writer};
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Magic at the start of every ANLZ file.
const MAGIC: &[u8; 4] = b"PMAI";

/// Length of the understood part of the file header: magic, `len_header` and `len_file`.
const FIXED_HEADER_LEN: u32 = 12;

/// An ANLZ file: its file header and its tags, in file order.
///
/// Decoding then encoding an unchanged value gives back identical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AnlzFile {
    unknown_header: Vec<u8>,
    tags: Vec<Tag>,
}

impl AnlzFile {
    /// Creates a file from the not-understood header bytes (`12..len_header`) and its tags.
    pub fn new(unknown_header: Vec<u8>, tags: Vec<Tag>) -> Self {
        Self {
            unknown_header,
            tags,
        }
    }

    /// Builds a track's `.DAT` file the way rekordbox writes it: the tags `PPTH`, `PVBR`,
    /// `PQTZ`, `PWAV`, `PWV2` and `PCOB` twice (hot cues, then memory points), in that
    /// order, after rekordbox's file header.
    ///
    /// `path` is the audio file path tag (see
    /// [`layout::audio_path_tag`](crate::layout::audio_path_tag)). Every value is written as
    /// given; build the cue lists with [`CueList::hot`] and [`CueList::memory`], which
    /// compute rekordbox's order links and `memory_count`. Callers without waveform data
    /// can pass [`WaveformPreview::flat`] and [`TinyWaveformPreview::flat`].
    ///
    /// The `PVBR` seek index is all zeros, as rekordbox writes it for AIFF and WAV files
    /// (all 823 on the fixture); MP3 files get a real index, which this crate can't compute.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::ValueOutOfRange`] when `hot_cues` is not of kind
    /// [`CueListKind::Hot`] or `memory_cues` is not of kind [`CueListKind::Memory`].
    pub fn track_dat(
        path: AudioPath,
        beat_grid: BeatGrid,
        hot_cues: CueList,
        memory_cues: CueList,
        preview: WaveformPreview,
        tiny_preview: TinyWaveformPreview,
    ) -> Result<Self, EncodeError> {
        check_cue_kinds(&hot_cues, &memory_cues)?;
        let vbr = RawTag::new(
            FourCc::PVBR,
            defaults::PVBR_HEADER.to_vec(),
            vec![0; defaults::PVBR_BODY_LEN],
        )
        .expect("PVBR is not a typed fourcc");
        Ok(Self::new(
            defaults::PMAI_UNKNOWN_HEADER.to_vec(),
            vec![
                Tag::Path(path),
                Tag::Raw(vbr),
                Tag::BeatGrid(beat_grid),
                Tag::WaveformPreview(preview),
                Tag::TinyWaveformPreview(tiny_preview),
                Tag::Cues(hot_cues),
                Tag::Cues(memory_cues),
            ],
        ))
    }

    /// Builds a track's `.EXT` file with the tags of Phase 1 (ADR-020): `PPTH`, `PWV3` and
    /// `PCOB` twice (an empty hot cue list, then an empty memory point list), in rekordbox's
    /// order, after rekordbox's `.EXT` file header. rekordbox's files go on with `PCO2`,
    /// `PWV4`, `PWV5` and others, which this crate doesn't write.
    ///
    /// Pass the `path` given to [`track_dat`](Self::track_dat), so both files hold the same
    /// `PPTH`. The cue lists are always empty: rekordbox's `.EXT` lists hold no cues in any
    /// fixture file, and the players read the cues from the `.DAT`. Callers without waveform
    /// data can pass [`WaveformDetail::flat`].
    pub fn track_ext(path: AudioPath, detail: WaveformDetail) -> Self {
        Self::new(
            defaults::PMAI_EXT_UNKNOWN_HEADER.to_vec(),
            vec![
                Tag::Path(path),
                Tag::WaveformDetail(detail),
                Tag::Cues(CueList::hot([])),
                Tag::Cues(CueList::memory([])),
            ],
        )
    }

    /// The file-header bytes `12..len_header`, which are not understood.
    pub fn unknown_header(&self) -> &[u8] {
        &self.unknown_header
    }

    /// The tags, in file order.
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Every tag with code `fourcc`, in file order.
    ///
    /// Some codes occur more than once in a file: `.DAT` files have two PCOB tags.
    pub fn tags_with(&self, fourcc: FourCc) -> impl Iterator<Item = &Tag> + '_ {
        self.tags.iter().filter(move |tag| tag.fourcc() == fourcc)
    }

    /// The path of the analysed audio file: the first `PPTH` tag. Real files have one.
    pub fn path(&self) -> Option<&str> {
        self.tags.iter().find_map(|tag| match tag {
            Tag::Path(path) => Some(path.path()),
            _ => None,
        })
    }

    /// The beat grid: the first `PQTZ` tag.
    pub fn beat_grid(&self) -> Option<&BeatGrid> {
        self.tags.iter().find_map(|tag| match tag {
            Tag::BeatGrid(grid) => Some(grid),
            _ => None,
        })
    }

    /// The hot cues: the first `PCOB` tag whose kind is [`CueListKind::Hot`].
    pub fn hot_cues(&self) -> Option<&CueList> {
        self.cue_list(CueListKind::Hot)
    }

    /// The memory points: the first `PCOB` tag whose kind is [`CueListKind::Memory`].
    pub fn memory_cues(&self) -> Option<&CueList> {
        self.cue_list(CueListKind::Memory)
    }

    /// The waveform preview: the first `PWAV` tag. Real `.DAT` files have one.
    pub fn waveform_preview(&self) -> Option<&WaveformPreview> {
        self.tags.iter().find_map(|tag| match tag {
            Tag::WaveformPreview(preview) => Some(preview),
            _ => None,
        })
    }

    /// The tiny waveform preview: the first `PWV2` tag. Real `.DAT` files have one.
    pub fn tiny_waveform_preview(&self) -> Option<&TinyWaveformPreview> {
        self.tags.iter().find_map(|tag| match tag {
            Tag::TinyWaveformPreview(preview) => Some(preview),
            _ => None,
        })
    }

    /// The waveform detail: the first `PWV3` tag. Real `.EXT` files have one.
    pub fn waveform_detail(&self) -> Option<&WaveformDetail> {
        self.tags.iter().find_map(|tag| match tag {
            Tag::WaveformDetail(detail) => Some(detail),
            _ => None,
        })
    }

    fn cue_list(&self, kind: CueListKind) -> Option<&CueList> {
        self.tags.iter().find_map(|tag| match tag {
            Tag::Cues(cues) if cues.kind() == kind => Some(cues),
            _ => None,
        })
    }

    /// Decodes a whole ANLZ file.
    ///
    /// # Errors
    ///
    /// Returns a [`DecodeError`] located at the offending field when the magic is wrong,
    /// a declared length is inconsistent, `len_file` differs from the input length, or a
    /// tag does not fit.
    pub fn from_bytes(data: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(data);
        r.expect_magic(MAGIC).context("PMAI magic")?;
        let len_header_at = r.position();
        let len_header = r.u32_be().context("PMAI len_header")?;
        let len_file_at = r.position();
        let len_file = r.u32_be().context("PMAI len_file")?;

        if len_header < FIXED_HEADER_LEN {
            return Err(DecodeError::new(
                len_header_at,
                "PMAI len_header",
                DecodeErrorKind::InvalidValue,
            ));
        }
        let actual = u64::try_from(data.len()).unwrap_or(u64::MAX);
        if u64::from(len_file) != actual {
            return Err(DecodeError::new(
                len_file_at,
                "PMAI len_file",
                DecodeErrorKind::LengthMismatch {
                    declared: u64::from(len_file),
                    actual,
                },
            ));
        }
        if len_header > len_file {
            return Err(DecodeError::new(
                len_header_at,
                "PMAI len_header",
                DecodeErrorKind::InvalidValue,
            ));
        }

        // Fits: FIXED_HEADER_LEN <= len_header <= len_file == data.len().
        let unknown_len = usize::try_from(len_header - FIXED_HEADER_LEN).unwrap_or(usize::MAX);
        let unknown_header = r.bytes(unknown_len).context("PMAI header")?.to_vec();

        let mut tags = Vec::new();
        while r.remaining() > 0 {
            tags.push(Tag::decode(&mut r)?);
        }
        Ok(Self {
            unknown_header,
            tags,
        })
    }

    /// Encodes the file, computing `len_header`, `len_file` and every tag's lengths.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::LengthOverflow`] when a length does not fit in a `u32`.
    pub fn to_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let mut w = Writer::new();
        let start = w.position();
        w.bytes(MAGIC);
        let len_header = w.reserve_u32_be();
        let len_file = w.reserve_u32_be();
        w.bytes(&self.unknown_header);
        w.patch_len_since(len_header, start, "PMAI len_header")?;
        for tag in &self.tags {
            tag.encode(&mut w)?;
        }
        w.patch_len_since(len_file, start, "PMAI len_file")?;
        Ok(w.into_inner())
    }
}

/// Checks that `hot` is a hot cue list and `memory` a memory point list, as the builders
/// write them.
fn check_cue_kinds(hot: &CueList, memory: &CueList) -> Result<(), EncodeError> {
    if hot.kind() != CueListKind::Hot {
        return Err(EncodeError::ValueOutOfRange {
            context: "PCOB hot cue list kind",
        });
    }
    if memory.kind() != CueListKind::Memory {
        return Err(EncodeError::ValueOutOfRange {
            context: "PCOB memory point list kind",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::*;

    /// Header bytes 12..28 as seen in real files: 16 bytes, here made up.
    const UNKNOWN: [u8; 16] = [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];

    /// Builds tag bytes by hand, independent of the encoder.
    fn tag_bytes(fourcc: &[u8; 4], header: &[u8], body: &[u8]) -> Vec<u8> {
        let len_header = 12 + header.len() as u32;
        let len_tag = len_header + body.len() as u32;
        tag_bytes_with(fourcc, len_header, len_tag, &[header, body].concat())
    }

    /// Builds tag bytes with arbitrary declared lengths.
    fn tag_bytes_with(fourcc: &[u8; 4], len_header: u32, len_tag: u32, rest: &[u8]) -> Vec<u8> {
        let mut out = fourcc.to_vec();
        out.extend_from_slice(&len_header.to_be_bytes());
        out.extend_from_slice(&len_tag.to_be_bytes());
        out.extend_from_slice(rest);
        out
    }

    /// Builds file bytes by hand with a correct `len_header` and `len_file`.
    fn file_bytes(unknown: &[u8], tags: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"PMAI".to_vec();
        out.extend_from_slice(&(12 + unknown.len() as u32).to_be_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(unknown);
        for tag in tags {
            out.extend_from_slice(tag);
        }
        let len_file = out.len() as u32;
        out[8..12].copy_from_slice(&len_file.to_be_bytes());
        out
    }

    fn raw(fourcc: FourCc, header: &[u8], body: &[u8]) -> Tag {
        Tag::Raw(RawTag::new(fourcc, header.to_vec(), body.to_vec()).expect("untyped fourcc"))
    }

    /// A PCPT entry by hand: hot cue `slot`, a point at `ms`.
    ///
    /// Deliberately not rekordbox's values: `status` 1 and order `(0, 0xFFFF)` are rare or
    /// never seen in real files, so the test proves they are kept. See `anlz::defaults` for
    /// the real values; do not copy these.
    fn pcpt_point(slot: u32, ms: u32) -> Vec<u8> {
        let header = [
            &slot.to_be_bytes()[..],
            &1u32.to_be_bytes(),
            &0x0001_0000u32.to_be_bytes(),
            &0u16.to_be_bytes(),
            &0xffffu16.to_be_bytes(),
        ]
        .concat();
        let body = [
            &[1, 0, 3, 0xe8][..],
            &ms.to_be_bytes(),
            &u32::MAX.to_be_bytes(),
            &[0; 16],
        ]
        .concat();
        tag_bytes(b"PCPT", &header, &body)
    }

    /// A PCOB header by hand, always with `memory_count` `0xFFFFFFFF`, which rekordbox only
    /// writes for hot cue lists and empty memory point lists.
    fn pcob_header(kind: u32, len_cues: u16) -> Vec<u8> {
        [
            &kind.to_be_bytes()[..],
            &0u16.to_be_bytes(),
            &len_cues.to_be_bytes(),
            &u32::MAX.to_be_bytes(),
        ]
        .concat()
    }

    fn assert_round_trip(bytes: &[u8], expected: &AnlzFile) {
        let decoded = AnlzFile::from_bytes(bytes).expect("decodes");
        assert_eq!(&decoded, expected);
        assert_eq!(decoded.to_bytes().expect("encodes"), bytes);
    }

    fn decode_err(bytes: &[u8]) -> (u64, &'static str, DecodeErrorKind) {
        let err = AnlzFile::from_bytes(bytes).unwrap_err();
        (err.offset, err.context, err.kind)
    }

    #[test]
    fn file_without_tags_round_trips() {
        let bytes = file_bytes(&UNKNOWN, &[]);
        assert_eq!(bytes.len(), 28);
        assert_round_trip(&bytes, &AnlzFile::new(UNKNOWN.to_vec(), vec![]));
    }

    #[test]
    fn bare_header_round_trips() {
        let bytes = file_bytes(&[], &[]);
        assert_eq!(bytes.len(), 12);
        assert_round_trip(&bytes, &AnlzFile::new(vec![], vec![]));
    }

    /// A file shaped like a real `.DAT`: path, beat grid, hot cues, memory points, and a
    /// tag this crate does not type.
    fn dat_like_bytes() -> Vec<u8> {
        let beat = [
            &1u16.to_be_bytes()[..],
            &12_000u16.to_be_bytes(),
            &500u32.to_be_bytes(),
        ]
        .concat();
        file_bytes(
            &UNKNOWN,
            &[
                tag_bytes(b"PPTH", &6u32.to_be_bytes(), &[0, b'/', 0, b'a', 0, 0]),
                tag_bytes(b"PQTZ", &[0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 1], &beat),
                tag_bytes(b"PCOB", &pcob_header(1, 1), &pcpt_point(2, 12_345)),
                tag_bytes(b"PCOB", &pcob_header(0, 1), &pcpt_point(0, 700)),
                tag_bytes(b"ZZZZ", &[], &[]),
            ],
        )
    }

    fn dat_like() -> AnlzFile {
        let point = |ms| CueSpan::Point(crate::encoding::Millis::new(ms));
        // Matches `pcpt_point`, whose status and order are deliberately not rekordbox's.
        let cue = |cue: Cue| {
            cue.with_status(1)
                .with_unknown1(0x0001_0000)
                .with_order(0, 0xffff)
        };
        AnlzFile::new(
            UNKNOWN.to_vec(),
            vec![
                Tag::Path(AudioPath::new("/a")),
                Tag::BeatGrid(BeatGrid::new(vec![Beat::new(
                    crate::encoding::BeatInBar::One,
                    crate::encoding::Bpm::from_centi(12_000),
                    crate::encoding::Millis::new(500),
                )])),
                Tag::Cues(CueList::new(
                    CueListKind::Hot,
                    vec![cue(Cue::hot(HotCueSlot::B, point(12_345)))],
                )),
                Tag::Cues(CueList::new(
                    CueListKind::Memory,
                    vec![cue(Cue::memory(point(700)))],
                )),
                raw(FourCc::new(*b"ZZZZ"), &[], &[]),
            ],
        )
    }

    #[test]
    fn multi_tag_file_round_trips_in_order() {
        let bytes = dat_like_bytes();
        assert_round_trip(&bytes, &dat_like());

        let decoded = AnlzFile::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.unknown_header(), UNKNOWN);
        assert_eq!(decoded.tags().len(), 5);
        assert_eq!(decoded.tags_with(FourCc::PCOB).count(), 2);
        assert_eq!(decoded.tags_with(FourCc::PWV3).count(), 0);
        assert_eq!(decoded.tags_with(FourCc::PPTH).count(), 1);
    }

    #[test]
    fn accessors_find_typed_tags() {
        let file = dat_like();
        assert_eq!(file.path(), Some("/a"));
        assert_eq!(file.beat_grid().map(|g| g.beats().len()), Some(1));
        let hot = file.hot_cues().unwrap();
        assert_eq!(hot.cues()[0].hot_cue(), Some(HotCueSlot::B));
        let memory = file.memory_cues().unwrap();
        assert_eq!(memory.cues()[0].hot_cue(), None);

        let empty = AnlzFile::new(vec![], vec![]);
        assert_eq!(empty.path(), None);
        assert!(empty.beat_grid().is_none());
        assert!(empty.hot_cues().is_none() && empty.memory_cues().is_none());
    }

    #[test]
    fn cue_lists_are_chosen_by_kind_not_position() {
        let file = AnlzFile::new(
            vec![],
            vec![
                Tag::Cues(CueList::memory([])),
                Tag::Cues(CueList::new(CueListKind::Other(7), vec![])),
                Tag::Cues(CueList::hot([])),
            ],
        );
        assert_eq!(file.hot_cues().unwrap().kind(), CueListKind::Hot);
        assert_eq!(file.memory_cues().unwrap().kind(), CueListKind::Memory);
    }

    #[test]
    fn typed_tag_that_fails_to_parse_is_an_error_not_raw() {
        // A PQTZ whose single beat has beat_number 0.
        let beat = [0u8, 0, 0x2e, 0xe0, 0, 0, 1, 0xf4];
        let bytes = file_bytes(
            &UNKNOWN,
            &[tag_bytes(
                b"PQTZ",
                &[0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 1],
                &beat,
            )],
        );
        assert_eq!(
            decode_err(&bytes),
            (28 + 0x18, "PQTZ beat_number", DecodeErrorKind::InvalidValue)
        );
    }

    /// A waveform tag header by hand: a count, then `unknown`.
    fn counted_header(count: u32, unknown: u32) -> Vec<u8> {
        [count.to_be_bytes(), unknown.to_be_bytes()].concat()
    }

    #[test]
    fn waveform_tags_decode_typed_and_round_trip() {
        let mut detail_header = 1u32.to_be_bytes().to_vec();
        detail_header.extend(counted_header(2, 0x0096_0000));
        let bytes = file_bytes(
            &UNKNOWN,
            &[
                tag_bytes(b"PWAV", &counted_header(2, 0x0001_0000), &[0xa2, 0x17]),
                tag_bytes(b"PWV2", &counted_header(1, 0x0001_0000), &[0x0f]),
                tag_bytes(b"PWV3", &detail_header, &[0xff, 0x01]),
            ],
        );
        let expected = AnlzFile::new(
            UNKNOWN.to_vec(),
            vec![
                Tag::WaveformPreview(WaveformPreview::new(vec![0xa2, 0x17])),
                Tag::TinyWaveformPreview(TinyWaveformPreview::new(vec![0x0f])),
                Tag::WaveformDetail(WaveformDetail::new(vec![0xff, 0x01])),
            ],
        );
        assert_round_trip(&bytes, &expected);
        let preview = expected.waveform_preview().unwrap();
        assert_eq!(preview.data(), [0xa2, 0x17]);
        let tiny = expected.tiny_waveform_preview().unwrap();
        assert_eq!(tiny.data(), [0x0f]);
        let detail = expected.waveform_detail().unwrap();
        assert_eq!(detail.data(), [0xff, 0x01]);
        let empty = AnlzFile::new(vec![], vec![]);
        assert!(empty.waveform_preview().is_none());
        assert!(empty.tiny_waveform_preview().is_none());
        assert!(empty.waveform_detail().is_none());
    }

    #[test]
    fn track_dat_writes_rekordbox_tag_order_and_defaults() {
        let point = |ms| CueSpan::Point(crate::encoding::Millis::new(ms));
        let grid = BeatGrid::new(vec![Beat::new(
            crate::encoding::BeatInBar::One,
            crate::encoding::Bpm::from_centi(12_000),
            crate::encoding::Millis::new(500),
        )]);
        let hot = CueList::hot([(HotCueSlot::A, point(1))]);
        let memory = CueList::memory([point(2), point(3)]);
        let file = AnlzFile::track_dat(
            AudioPath::new("/Contents/a/b/c.aiff"),
            grid.clone(),
            hot.clone(),
            memory.clone(),
            WaveformPreview::flat(2),
            TinyWaveformPreview::flat(1),
        )
        .unwrap();

        assert_eq!(
            file.unknown_header(),
            [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]
        );
        let codes: Vec<FourCc> = file.tags().iter().map(Tag::fourcc).collect();
        assert_eq!(
            codes,
            [
                FourCc::PPTH,
                FourCc::PVBR,
                FourCc::PQTZ,
                FourCc::PWAV,
                FourCc::PWV2,
                FourCc::PCOB,
                FourCc::PCOB,
            ]
        );
        let Tag::Raw(vbr) = &file.tags()[1] else {
            panic!("PVBR is raw");
        };
        assert_eq!(vbr.header(), [0; 4]);
        assert_eq!(vbr.body(), [0; 1604]);
        assert_eq!(file.path(), Some("/Contents/a/b/c.aiff"));
        assert_eq!(file.beat_grid(), Some(&grid));
        assert_eq!(file.hot_cues(), Some(&hot));
        assert_eq!(file.memory_cues(), Some(&memory));
        assert_eq!(file.waveform_preview(), Some(&WaveformPreview::flat(2)));
        assert_eq!(
            file.tiny_waveform_preview(),
            Some(&TinyWaveformPreview::flat(1))
        );

        let bytes = file.to_bytes().unwrap();
        assert_round_trip(&bytes, &file);
    }

    #[test]
    fn track_ext_writes_rekordbox_prefix_and_header() {
        let point = |ms| CueSpan::Point(crate::encoding::Millis::new(ms));
        let detail = WaveformDetail::new(vec![0xe0, 0x45, 0xff]);
        let file = AnlzFile::track_ext(AudioPath::new("/Contents/a/b/c.aiff"), detail.clone());

        assert_eq!(
            file.unknown_header(),
            [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]
        );
        let codes: Vec<FourCc> = file.tags().iter().map(Tag::fourcc).collect();
        assert_eq!(
            codes,
            [FourCc::PPTH, FourCc::PWV3, FourCc::PCOB, FourCc::PCOB]
        );
        assert_eq!(file.path(), Some("/Contents/a/b/c.aiff"));
        assert_eq!(file.waveform_detail(), Some(&detail));
        // Empty cue lists, as rekordbox writes in every `.EXT`: the cues are in the `.DAT`.
        assert_eq!(file.tags()[2], Tag::Cues(CueList::hot([])));
        assert_eq!(file.tags()[3], Tag::Cues(CueList::memory([])));
        assert_round_trip(&file.to_bytes().unwrap(), &file);

        // The same PPTH bytes as the track's `.DAT`.
        let dat = AnlzFile::track_dat(
            AudioPath::new("/Contents/a/b/c.aiff"),
            BeatGrid::new(vec![]),
            CueList::hot([(HotCueSlot::B, point(5))]),
            CueList::memory([point(2), point(3)]),
            WaveformPreview::flat(2),
            TinyWaveformPreview::flat(1),
        )
        .unwrap();
        assert_eq!(file.tags()[0], dat.tags()[0]);
    }

    #[test]
    fn track_dat_rejects_cue_lists_of_the_wrong_kind() {
        let build = |hot: CueList, memory: CueList| {
            AnlzFile::track_dat(
                AudioPath::new("/a"),
                BeatGrid::new(vec![]),
                hot,
                memory,
                WaveformPreview::flat(0),
                TinyWaveformPreview::flat(0),
            )
        };
        let hot_error = Err(EncodeError::ValueOutOfRange {
            context: "PCOB hot cue list kind",
        });
        let memory_error = Err(EncodeError::ValueOutOfRange {
            context: "PCOB memory point list kind",
        });
        let other = || CueList::new(CueListKind::Other(7), vec![]);

        assert_eq!(build(CueList::memory([]), CueList::memory([])), hot_error);
        assert_eq!(build(other(), CueList::memory([])), hot_error);
        assert_eq!(build(CueList::hot([]), CueList::hot([])), memory_error);
        assert_eq!(build(CueList::hot([]), other()), memory_error);
        assert!(build(CueList::hot([]), CueList::memory([])).is_ok());
    }

    #[test]
    fn waveform_tag_that_fails_to_parse_is_an_error_not_raw() {
        // A PWAV that declares more columns than its body holds.
        let bytes = file_bytes(
            &UNKNOWN,
            &[tag_bytes(b"PWAV", &counted_header(3, 0x0001_0000), &[1])],
        );
        assert_eq!(
            decode_err(&bytes),
            (
                28 + 0x0c,
                "PWAV len_preview",
                DecodeErrorKind::LengthMismatch {
                    declared: 3,
                    actual: 1,
                }
            )
        );
    }

    #[test]
    fn tag_with_odd_len_header_round_trips() {
        let tag = tag_bytes(b"PWVC", &[0xaa, 0xbb], &[1, 2, 3]);
        assert_eq!(tag[4..8], 14u32.to_be_bytes());
        assert_eq!(tag[8..12], 17u32.to_be_bytes());
        let bytes = file_bytes(&UNKNOWN, &[tag, tag_bytes(b"PVDI", &[0; 4], &[7])]);
        let expected = AnlzFile::new(
            UNKNOWN.to_vec(),
            vec![
                raw(FourCc::PWVC, &[0xaa, 0xbb], &[1, 2, 3]),
                raw(FourCc::PVDI, &[0; 4], &[7]),
            ],
        );
        assert_round_trip(&bytes, &expected);
    }

    #[test]
    fn bad_magic_is_reported_at_zero() {
        let mut bytes = file_bytes(&UNKNOWN, &[]);
        bytes[3] = b'X';
        assert_eq!(
            decode_err(&bytes),
            (
                0,
                "PMAI magic",
                DecodeErrorKind::BadMagic {
                    expected: *b"PMAI",
                    found: *b"PMAX",
                }
            )
        );
    }

    #[test]
    fn truncated_file_header_is_unexpected_eof() {
        assert_eq!(
            decode_err(b"PM"),
            (
                0,
                "PMAI magic",
                DecodeErrorKind::UnexpectedEof {
                    needed: 4,
                    available: 2
                }
            )
        );
        assert_eq!(
            decode_err(b"PMAI\0\0\0\x0c\0\0"),
            (
                8,
                "PMAI len_file",
                DecodeErrorKind::UnexpectedEof {
                    needed: 4,
                    available: 2
                }
            )
        );
    }

    #[test]
    fn file_len_header_below_twelve_is_invalid() {
        let mut bytes = file_bytes(&UNKNOWN, &[]);
        bytes[4..8].copy_from_slice(&11u32.to_be_bytes());
        assert_eq!(
            decode_err(&bytes),
            (4, "PMAI len_header", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn file_len_header_past_len_file_is_invalid() {
        let mut bytes = file_bytes(&UNKNOWN, &[]);
        bytes[4..8].copy_from_slice(&29u32.to_be_bytes());
        assert_eq!(
            decode_err(&bytes),
            (4, "PMAI len_header", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn len_file_other_than_input_length_is_mismatch() {
        let mut bytes = file_bytes(&UNKNOWN, &[tag_bytes(b"PQTZ", &[], &[1])]);
        bytes.push(0);
        assert_eq!(
            decode_err(&bytes),
            (
                8,
                "PMAI len_file",
                DecodeErrorKind::LengthMismatch {
                    declared: 41,
                    actual: 42,
                }
            )
        );
        bytes.truncate(40);
        assert_eq!(
            decode_err(&bytes),
            (
                8,
                "PMAI len_file",
                DecodeErrorKind::LengthMismatch {
                    declared: 41,
                    actual: 40,
                }
            )
        );
    }

    /// A valid first tag, so the failing tag starts at a non-trivial offset: 28 + 15 = 43.
    fn first_tag() -> Vec<u8> {
        tag_bytes(b"PVBR", &[0; 2], &[1])
    }

    const SECOND_TAG_AT: u64 = 43;

    #[test]
    fn tag_running_past_the_end_is_reported_at_its_len_tag() {
        let bad = tag_bytes_with(b"PQTZ", 12, 20, &[0; 4]);
        let bytes = file_bytes(&UNKNOWN, &[first_tag(), bad]);
        assert_eq!(
            decode_err(&bytes),
            (
                SECOND_TAG_AT + 8,
                "tag len_tag",
                DecodeErrorKind::LengthMismatch {
                    declared: 20,
                    actual: 16,
                }
            )
        );
    }

    #[test]
    fn tag_len_tag_at_u32_max_is_reported_without_overflow() {
        let bad = tag_bytes_with(b"PQTZ", 12, u32::MAX, &[]);
        let bytes = file_bytes(&UNKNOWN, &[first_tag(), bad]);
        assert_eq!(
            decode_err(&bytes),
            (
                SECOND_TAG_AT + 8,
                "tag len_tag",
                DecodeErrorKind::LengthMismatch {
                    declared: u64::from(u32::MAX),
                    actual: 12,
                }
            )
        );
    }

    #[test]
    fn tag_len_header_below_twelve_is_invalid() {
        let bad = tag_bytes_with(b"PQTZ", 8, 12, &[]);
        let bytes = file_bytes(&UNKNOWN, &[first_tag(), bad]);
        assert_eq!(
            decode_err(&bytes),
            (
                SECOND_TAG_AT + 4,
                "tag len_header",
                DecodeErrorKind::InvalidValue
            )
        );
    }

    #[test]
    fn tag_len_header_past_len_tag_is_invalid() {
        let bad = tag_bytes_with(b"PQTZ", 16, 14, &[0; 4]);
        let bytes = file_bytes(&UNKNOWN, &[first_tag(), bad]);
        assert_eq!(
            decode_err(&bytes),
            (
                SECOND_TAG_AT + 4,
                "tag len_header",
                DecodeErrorKind::InvalidValue
            )
        );
    }

    #[test]
    fn fewer_than_twelve_bytes_where_a_tag_starts_is_unexpected_eof() {
        let bytes = file_bytes(&UNKNOWN, &[first_tag(), b"PQTZ\0\0\0\x0c\0\0\0".to_vec()]);
        assert_eq!(
            decode_err(&bytes),
            (
                SECOND_TAG_AT,
                "tag common header",
                DecodeErrorKind::UnexpectedEof {
                    needed: 12,
                    available: 11,
                }
            )
        );
    }

    /// Codes this crate keeps raw; typed codes are generated by the typed strategies.
    fn untyped_fourcc() -> impl Strategy<Value = FourCc> {
        prop_oneof![
            Just(FourCc::PVBR),
            Just(FourCc::PCO2),
            Just(FourCc::new(*b"PCPT")),
            any::<[u8; 4]>()
                .prop_map(FourCc::new)
                .prop_filter("untyped", |code| !code.is_typed()),
        ]
    }

    fn raw_tag() -> impl Strategy<Value = Tag> {
        (
            untyped_fourcc(),
            vec(any::<u8>(), 0..24),
            vec(any::<u8>(), 0..64),
        )
            .prop_map(|(fourcc, header, body)| Tag::Raw(RawTag::new(fourcc, header, body).unwrap()))
    }

    fn tail() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![Just(vec![]), vec(any::<u8>(), 0..24)]
    }

    /// Raw enum values biased towards the named ones (0 to 3), so `Memory`, `Hot`, no slot
    /// and slots A to C are all exercised, not only `Other`.
    fn enum_value() -> impl Strategy<Value = u32> {
        prop_oneof![
            3 => Just(0u32),
            3 => Just(1),
            2 => Just(2),
            2 => Just(3),
            1 => any::<u32>(),
        ]
    }

    fn path_tag() -> impl Strategy<Value = Tag> {
        (any::<String>(), tail())
            .prop_map(|(path, tail)| Tag::Path(AudioPath::new(path).with_header_tail(tail)))
    }

    fn beat() -> impl Strategy<Value = Beat> {
        (1u16..=4, any::<u16>(), any::<u32>()).prop_map(|(n, tempo, ms)| {
            Beat::new(
                crate::encoding::BeatInBar::new(n).unwrap(),
                crate::encoding::Bpm::from_centi(u32::from(tempo)),
                crate::encoding::Millis::new(ms),
            )
        })
    }

    fn beat_grid_tag() -> impl Strategy<Value = Tag> {
        (
            any::<u32>(),
            any::<u32>(),
            tail(),
            vec(beat(), 0..8),
            tail(),
        )
            .prop_map(|(u1, u2, header_tail, beats, body_tail)| {
                Tag::BeatGrid(
                    BeatGrid::new(beats)
                        .with_unknown1(u1)
                        .with_unknown2(u2)
                        .with_header_tail(header_tail)
                        .with_body_tail(body_tail),
                )
            })
    }

    fn span() -> impl Strategy<Value = CueSpan> {
        use crate::encoding::Millis;
        prop_oneof![
            any::<u32>().prop_map(|ms| CueSpan::Point(Millis::new(ms))),
            (any::<u32>(), any::<u32>()).prop_map(|(s, e)| CueSpan::Loop {
                start: Millis::new(s),
                end: Millis::new(e),
            }),
            (any::<u8>(), any::<u32>(), any::<u32>()).prop_map(|(cue_type, time, loop_time)| {
                CueSpan::Other {
                    cue_type,
                    time,
                    loop_time,
                }
            }),
        ]
    }

    fn cue() -> impl Strategy<Value = Cue> {
        (
            (
                prop_oneof![
                    4 => enum_value().prop_map(HotCueSlot::from_u32),
                    // Hand-built `Other` slots, including the non-canonical `Other(0..=3)`.
                    1 => enum_value().prop_map(|v| Some(HotCueSlot::Other(v))),
                ],
                span(),
            ),
            (any::<u32>(), any::<u32>(), any::<u16>(), any::<u16>()),
            (any::<[u8; 3]>(), any::<[u8; 16]>(), tail(), tail()),
        )
            .prop_map(
                |((slot, span), (status, u1, first, last), (u2, u3, ht, bt))| {
                    let cue = match slot {
                        Some(slot) => Cue::hot(slot, span),
                        None => Cue::memory(span),
                    };
                    cue.with_status(status)
                        .with_unknown1(u1)
                        .with_order(first, last)
                        .with_unknown2(u2)
                        .with_unknown3(u3)
                        .with_header_tail(ht)
                        .with_body_tail(bt)
                },
            )
    }

    fn cue_list_tag() -> impl Strategy<Value = Tag> {
        (
            prop_oneof![
                4 => enum_value().prop_map(CueListKind::from_u32),
                // Hand-built `Other` values, including the non-canonical `Other(0 | 1)`.
                1 => enum_value().prop_map(CueListKind::Other),
            ],
            any::<u16>(),
            any::<u32>(),
            tail(),
            vec(cue(), 0..4),
            tail(),
        )
            .prop_map(|(kind, u1, memory_count, ht, cues, bt)| {
                Tag::Cues(
                    CueList::new(kind, cues)
                        .with_unknown1(u1)
                        .with_memory_count(memory_count)
                        .with_header_tail(ht)
                        .with_body_tail(bt),
                )
            })
    }

    fn waveform_tag() -> impl Strategy<Value = Tag> {
        (
            0u8..3,
            any::<u32>(),
            tail(),
            vec(any::<u8>(), 0..32),
            tail(),
        )
            .prop_map(|(which, u1, header_tail, columns, body_tail)| match which {
                0 => Tag::WaveformPreview(
                    WaveformPreview::new(columns)
                        .with_unknown1(u1)
                        .with_header_tail(header_tail)
                        .with_body_tail(body_tail),
                ),
                1 => Tag::TinyWaveformPreview(
                    TinyWaveformPreview::new(columns)
                        .with_unknown1(u1)
                        .with_header_tail(header_tail)
                        .with_body_tail(body_tail),
                ),
                _ => Tag::WaveformDetail(
                    WaveformDetail::new(columns)
                        .with_unknown1(u1)
                        .with_header_tail(header_tail)
                        .with_body_tail(body_tail),
                ),
            })
    }

    fn tag() -> impl Strategy<Value = Tag> {
        prop_oneof![
            raw_tag(),
            path_tag(),
            beat_grid_tag(),
            cue_list_tag(),
            waveform_tag(),
        ]
    }

    fn anlz_file() -> impl Strategy<Value = AnlzFile> {
        (vec(any::<u8>(), 0..32), vec(tag(), 0..8))
            .prop_map(|(unknown, tags)| AnlzFile::new(unknown, tags))
    }

    /// Bytes that get past the file-header checks: `PMAI`, a plausible `len_header` and a
    /// correct `len_file`, followed by arbitrary bytes.
    fn plausible_bytes() -> impl Strategy<Value = Vec<u8>> {
        (12u32..40, vec(any::<u8>(), 0..256)).prop_map(|(len_header, rest)| {
            let mut out = b"PMAI".to_vec();
            out.extend_from_slice(&len_header.to_be_bytes());
            out.extend_from_slice(&((12 + rest.len()) as u32).to_be_bytes());
            out.extend_from_slice(&rest);
            out
        })
    }

    proptest! {
        #[test]
        fn encode_then_decode_is_identity(file in anlz_file()) {
            let bytes = file.to_bytes().unwrap();
            let decoded = AnlzFile::from_bytes(&bytes).unwrap();
            prop_assert_eq!(&decoded, &file);
            prop_assert_eq!(decoded.to_bytes().unwrap(), bytes);
        }

        #[test]
        fn from_bytes_never_panics_on_arbitrary_bytes(data in vec(any::<u8>(), 0..256)) {
            let _ = AnlzFile::from_bytes(&data);
        }

        #[test]
        fn from_bytes_never_panics_past_the_file_header(data in plausible_bytes()) {
            if let Ok(file) = AnlzFile::from_bytes(&data) {
                prop_assert_eq!(file.to_bytes().unwrap(), data);
            }
        }

        #[test]
        fn corrupting_one_byte_never_panics(
            file in anlz_file(),
            index in any::<prop::sample::Index>(),
            value in any::<u8>(),
        ) {
            let mut bytes = file.to_bytes().unwrap();
            let i = index.index(bytes.len());
            bytes[i] = value;
            if let Ok(decoded) = AnlzFile::from_bytes(&bytes) {
                prop_assert_eq!(decoded.to_bytes().unwrap(), bytes);
            }
        }
    }
}
