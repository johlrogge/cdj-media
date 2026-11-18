//! ANLZ tags: the four-character code, the tag enum and the raw tag that keeps every byte.
//!
//! Every tag starts with a 12-byte common header, all big-endian:
//!
//! | bytes  | field        | meaning                                                  |
//! |--------|--------------|----------------------------------------------------------|
//! | 0..4   | fourcc       | tag type, e.g. `PQTZ`                                    |
//! | 4..8   | `len_header` | length of the tag header, including these 12 bytes       |
//! | 8..12  | `len_tag`    | length of the whole tag; the offset to the next tag      |
//!
//! Bytes `12..len_header` are a tag-specific header and `len_header..len_tag` is the body.
//! `len_header` is not always a multiple of four (PWVC uses 14).

use std::fmt;

use super::{AudioPath, BeatGrid, CueList};
use crate::bytes::{Reader, Writer};
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Length of the common tag header: fourcc, `len_header` and `len_tag`.
const COMMON_HEADER_LEN: u32 = 12;

/// A four-character tag code, such as `PQTZ`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FourCc([u8; 4]);

impl FourCc {
    /// Path of the audio file the analysis belongs to.
    pub const PPTH: Self = Self(*b"PPTH");
    /// VBR seek index.
    pub const PVBR: Self = Self(*b"PVBR");
    /// Beat grid.
    pub const PQTZ: Self = Self(*b"PQTZ");
    /// Monochrome waveform preview.
    pub const PWAV: Self = Self(*b"PWAV");
    /// Tiny monochrome waveform preview.
    pub const PWV2: Self = Self(*b"PWV2");
    /// Monochrome waveform detail.
    pub const PWV3: Self = Self(*b"PWV3");
    /// Colour waveform preview.
    pub const PWV4: Self = Self(*b"PWV4");
    /// Colour waveform detail.
    pub const PWV5: Self = Self(*b"PWV5");
    /// Three-band waveform preview.
    pub const PWV6: Self = Self(*b"PWV6");
    /// Three-band waveform detail.
    pub const PWV7: Self = Self(*b"PWV7");
    /// Not documented; seen in `.2EX` files.
    pub const PWVC: Self = Self(*b"PWVC");
    /// Cue list.
    pub const PCOB: Self = Self(*b"PCOB");
    /// Extended cue list.
    pub const PCO2: Self = Self(*b"PCO2");
    /// Not documented; seen in `.EXT` files.
    pub const PQT2: Self = Self(*b"PQT2");
    /// Song structure.
    pub const PSSI: Self = Self(*b"PSSI");
    /// Not documented; seen in `.2EX` files.
    pub const PVDI: Self = Self(*b"PVDI");

    /// Creates a code from its four bytes.
    pub const fn new(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }

    /// The four bytes of the code.
    pub const fn to_bytes(self) -> [u8; 4] {
        self.0
    }

    /// Whether this crate decodes tags with this code into a typed [`Tag`] variant rather
    /// than [`Tag::Raw`]: `PPTH`, `PQTZ` and `PCOB`.
    pub const fn is_typed(self) -> bool {
        // Keep in step with the match in `Tag::decode`: every code it types is listed here,
        // or `RawTag::new` would accept a code that decodes as a typed variant.
        matches!(&self.0, b"PPTH" | b"PQTZ" | b"PCOB")
    }
}

impl From<[u8; 4]> for FourCc {
    fn from(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for FourCc {
    /// Shows the code as ASCII, escaping any byte that is not printable ASCII.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.escape_ascii())
    }
}

impl fmt::Debug for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FourCc(\"{self}\")")
    }
}

/// One tag of an ANLZ file.
///
/// `PPTH`, `PQTZ` and `PCOB` are decoded into typed variants (see ADR-004); a tag with one
/// of those codes that does not parse is a decode error, never a [`Tag::Raw`]. Every other
/// tag is kept verbatim as [`Tag::Raw`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Tag {
    /// `PPTH`: the path of the audio file.
    Path(AudioPath),
    /// `PQTZ`: the beat grid.
    BeatGrid(BeatGrid),
    /// `PCOB`: a hot cue or memory point list.
    Cues(CueList),
    /// A tag this crate does not type, kept as raw bytes.
    Raw(RawTag),
}

impl Tag {
    /// The tag's four-character code.
    pub fn fourcc(&self) -> FourCc {
        match self {
            Self::Path(_) => FourCc::PPTH,
            Self::BeatGrid(_) => FourCc::PQTZ,
            Self::Cues(_) => FourCc::PCOB,
            Self::Raw(raw) => raw.fourcc(),
        }
    }

    /// Decodes one tag at the reader's position and advances past it.
    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let frame = read_frame(r)?;
        // Keep in step with `FourCc::is_typed`, which lists exactly the codes typed here.
        match frame.fourcc {
            FourCc::PPTH => AudioPath::decode(frame).map(Self::Path),
            FourCc::PQTZ => BeatGrid::decode(frame).map(Self::BeatGrid),
            FourCc::PCOB => CueList::decode(frame).map(Self::Cues),
            _ => Ok(Self::Raw(RawTag::from(frame))),
        }
    }

    /// Encodes the tag, computing `len_header`, `len_tag` and every derived length.
    pub(crate) fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        match self {
            Self::Path(path) => path.encode(w),
            Self::BeatGrid(grid) => grid.encode(w),
            Self::Cues(cues) => cues.encode(w),
            Self::Raw(raw) => raw.encode(w),
        }
    }
}

/// One framed tag, split into readers over its tag-specific header and its body.
///
/// Both readers report absolute offsets and borrow from the input.
#[derive(Debug)]
pub(crate) struct Frame<'a> {
    /// Absolute offset of the tag's first byte.
    pub(crate) start: u64,
    /// The tag's four-character code.
    pub(crate) fourcc: FourCc,
    /// Bytes `12..len_header` of the tag.
    pub(crate) header: Reader<'a>,
    /// Bytes `len_header..len_tag` of the tag.
    pub(crate) body: Reader<'a>,
}

impl Frame<'_> {
    /// Fails with [`DecodeErrorKind::BadMagic`] at the start of the tag, named `context`,
    /// unless the tag's code is `expected`.
    pub(crate) fn expect_fourcc(
        &self,
        expected: FourCc,
        context: &'static str,
    ) -> Result<(), DecodeError> {
        if self.fourcc == expected {
            return Ok(());
        }
        Err(DecodeError::new(
            self.start,
            context,
            DecodeErrorKind::BadMagic {
                expected: expected.0,
                found: self.fourcc.0,
            },
        ))
    }

    /// Fails with [`DecodeErrorKind::InvalidValue`] at the tag's `len_header` field, named
    /// `context`, when `len_header` is below the documented `min`.
    pub(crate) fn require_len_header(
        &self,
        min: u32,
        context: &'static str,
    ) -> Result<(), DecodeError> {
        let min_tail = usize::try_from(min.saturating_sub(COMMON_HEADER_LEN)).unwrap_or(usize::MAX);
        if self.header.remaining() >= min_tail {
            return Ok(());
        }
        Err(DecodeError::new(
            self.start.saturating_add(4),
            context,
            DecodeErrorKind::InvalidValue,
        ))
    }
}

/// Reads the common tag header at the reader's position, checks its lengths, and splits the
/// tag into a [`Frame`]. The reader advances past the whole tag.
pub(crate) fn read_frame<'a>(r: &mut Reader<'a>) -> Result<Frame<'a>, DecodeError> {
    let start = r.position();
    let available = r.remaining();
    if available < COMMON_HEADER_LEN as usize {
        return Err(DecodeError::new(
            start,
            "tag common header",
            DecodeErrorKind::UnexpectedEof {
                needed: COMMON_HEADER_LEN as usize,
                available,
            },
        ));
    }
    let fourcc = FourCc(r.array().context("tag fourcc")?);
    let len_header_at = r.position();
    let len_header = r.u32_be().context("tag len_header")?;
    let len_tag_at = r.position();
    let len_tag = r.u32_be().context("tag len_tag")?;

    if len_header < COMMON_HEADER_LEN || len_header > len_tag {
        return Err(DecodeError::new(
            len_header_at,
            "tag len_header",
            DecodeErrorKind::InvalidValue,
        ));
    }
    // `len_tag` counts from the start of the tag, so compare it with what was there.
    let available_u64 = u64::try_from(available).unwrap_or(u64::MAX);
    if u64::from(len_tag) > available_u64 {
        return Err(DecodeError::new(
            len_tag_at,
            "tag len_tag",
            DecodeErrorKind::LengthMismatch {
                declared: u64::from(len_tag),
                actual: available_u64,
            },
        ));
    }
    // Both fit: COMMON_HEADER_LEN <= len_header <= len_tag <= available.
    let header_len = usize::try_from(len_header - COMMON_HEADER_LEN).unwrap_or(usize::MAX);
    let body_len = usize::try_from(len_tag - len_header).unwrap_or(usize::MAX);
    let header = r.sub_reader(header_len).context("tag header bytes")?;
    let body = r.sub_reader(body_len).context("tag body")?;
    Ok(Frame {
        start,
        fourcc,
        header,
        body,
    })
}

/// Writes one tag: the common header, then `header`, then `body`, patching `len_header`
/// and `len_tag` once each part is written.
pub(crate) fn write_frame(
    w: &mut Writer,
    fourcc: FourCc,
    header: impl FnOnce(&mut Writer) -> Result<(), EncodeError>,
    body: impl FnOnce(&mut Writer) -> Result<(), EncodeError>,
) -> Result<(), EncodeError> {
    let start = w.position();
    w.bytes(&fourcc.0);
    let len_header = w.reserve_u32_be();
    let len_tag = w.reserve_u32_be();
    header(w)?;
    w.patch_len_since(len_header, start, "tag len_header")?;
    body(w)?;
    w.patch_len_since(len_tag, start, "tag len_tag")
}

/// A tag as raw bytes: its code, its tag-specific header and its body.
///
/// The common 12-byte header is not stored; `len_header` and `len_tag` are recomputed on
/// encode. A raw tag never has a typed code (see [`FourCc::is_typed`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RawTag {
    fourcc: FourCc,
    header: Vec<u8>,
    body: Vec<u8>,
}

impl RawTag {
    /// Creates a raw tag, or returns `None` when `fourcc` is one this crate types
    /// ([`FourCc::is_typed`]); build those through their typed [`Tag`] variant instead.
    ///
    /// `header` is the tag-specific header (bytes `12..len_header` of the tag) and `body`
    /// is everything after it (bytes `len_header..len_tag`).
    pub fn new(fourcc: FourCc, header: Vec<u8>, body: Vec<u8>) -> Option<Self> {
        (!fourcc.is_typed()).then_some(Self {
            fourcc,
            header,
            body,
        })
    }

    /// The tag's four-character code.
    pub fn fourcc(&self) -> FourCc {
        self.fourcc
    }

    /// The tag-specific header: bytes `12..len_header` of the tag.
    pub fn header(&self) -> &[u8] {
        &self.header
    }

    /// The body: bytes `len_header..len_tag` of the tag.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        write_frame(
            w,
            self.fourcc,
            |w| {
                w.bytes(&self.header);
                Ok(())
            },
            |w| {
                w.bytes(&self.body);
                Ok(())
            },
        )
    }
}

/// Keeps a frame verbatim. Only [`Tag::decode`] uses this, and only for untyped codes.
impl From<Frame<'_>> for RawTag {
    fn from(frame: Frame<'_>) -> Self {
        let Frame {
            fourcc,
            mut header,
            mut body,
            ..
        } = frame;
        debug_assert!(!fourcc.is_typed(), "raw tag with a typed fourcc");
        Self {
            fourcc,
            header: header.take_rest().to_vec(),
            body: body.take_rest().to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fourcc_shows_ascii() {
        assert_eq!(FourCc::PQTZ.to_string(), "PQTZ");
        assert_eq!(format!("{:?}", FourCc::PWV2), "FourCc(\"PWV2\")");
        assert_eq!(
            FourCc::new([b'P', 0, 0xff, b'Z']).to_string(),
            "P\\x00\\xffZ"
        );
    }

    #[test]
    fn fourcc_round_trips_its_bytes() {
        assert_eq!(FourCc::from(*b"PSSI"), FourCc::PSSI);
        assert_eq!(FourCc::PVDI.to_bytes(), *b"PVDI");
    }

    #[test]
    fn known_codes_match_their_names() {
        let codes = [
            (FourCc::PPTH, "PPTH"),
            (FourCc::PVBR, "PVBR"),
            (FourCc::PQTZ, "PQTZ"),
            (FourCc::PWAV, "PWAV"),
            (FourCc::PWV2, "PWV2"),
            (FourCc::PWV3, "PWV3"),
            (FourCc::PWV4, "PWV4"),
            (FourCc::PWV5, "PWV5"),
            (FourCc::PWV6, "PWV6"),
            (FourCc::PWV7, "PWV7"),
            (FourCc::PWVC, "PWVC"),
            (FourCc::PCOB, "PCOB"),
            (FourCc::PCO2, "PCO2"),
            (FourCc::PQT2, "PQT2"),
            (FourCc::PSSI, "PSSI"),
            (FourCc::PVDI, "PVDI"),
        ];
        for (code, name) in codes {
            assert_eq!(code.to_string(), name);
        }
    }

    #[test]
    fn raw_tag_getters_and_tag_fourcc() {
        let raw = RawTag::new(FourCc::PCO2, vec![1, 2], vec![3]).unwrap();
        assert_eq!(raw.fourcc(), FourCc::PCO2);
        assert_eq!(raw.header(), [1, 2]);
        assert_eq!(raw.body(), [3]);
        assert_eq!(Tag::Raw(raw).fourcc(), FourCc::PCO2);
    }

    #[test]
    fn raw_tag_rejects_typed_fourccs() {
        for typed in [FourCc::PPTH, FourCc::PQTZ, FourCc::PCOB] {
            assert!(typed.is_typed());
            assert_eq!(RawTag::new(typed, vec![], vec![]), None);
        }
        for untyped in [FourCc::PWAV, FourCc::PCO2, FourCc::new(*b"ZZZZ")] {
            assert!(!untyped.is_typed());
            assert!(RawTag::new(untyped, vec![], vec![]).is_some());
        }
    }

    #[test]
    fn typed_variants_report_their_fourcc() {
        let typed = [
            (Tag::Path(AudioPath::new("/a")), FourCc::PPTH),
            (Tag::BeatGrid(BeatGrid::new(vec![])), FourCc::PQTZ),
            (Tag::Cues(CueList::memory([])), FourCc::PCOB),
        ];
        for (tag, code) in typed {
            assert_eq!(tag.fourcc(), code);
            assert!(tag.fourcc().is_typed(), "{code} must be listed in is_typed");
        }
    }

    #[test]
    fn frame_checks_fourcc_and_minimum_len_header() {
        let mut data = vec![0xee; 2];
        data.extend_from_slice(b"PCPT");
        data.extend_from_slice(&16u32.to_be_bytes());
        data.extend_from_slice(&16u32.to_be_bytes());
        data.extend_from_slice(&[0; 4]);
        let mut r = Reader::new(&data);
        r.skip(2).unwrap();
        let frame = read_frame(&mut r).unwrap();
        assert_eq!(frame.start, 2);
        assert_eq!(frame.expect_fourcc(FourCc::new(*b"PCPT"), "x"), Ok(()));
        let err = frame
            .expect_fourcc(FourCc::PCOB, "PCPT fourcc")
            .unwrap_err();
        assert_eq!(
            (err.offset, err.context, err.kind),
            (
                2,
                "PCPT fourcc",
                DecodeErrorKind::BadMagic {
                    expected: *b"PCOB",
                    found: *b"PCPT",
                }
            )
        );
        assert_eq!(frame.require_len_header(16, "x"), Ok(()));
        let err = frame.require_len_header(17, "PCPT len_header").unwrap_err();
        assert_eq!(
            (err.offset, err.context, err.kind),
            (6, "PCPT len_header", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn read_frame_splits_header_and_body_with_absolute_offsets() {
        let mut data = vec![0xee; 3];
        data.extend_from_slice(b"PWVC");
        data.extend_from_slice(&14u32.to_be_bytes());
        data.extend_from_slice(&17u32.to_be_bytes());
        data.extend_from_slice(&[0xaa, 0xbb, 1, 2, 3, 0xff]);
        let mut r = Reader::new(&data);
        r.skip(3).unwrap();
        let mut frame = read_frame(&mut r).unwrap();
        assert_eq!(r.position(), 20);
        assert_eq!(frame.fourcc, FourCc::PWVC);
        assert_eq!(frame.header.position(), 15);
        assert_eq!(frame.header.take_rest(), [0xaa, 0xbb]);
        assert_eq!(frame.body.position(), 17);
        let err = frame.body.u32_be().unwrap_err();
        assert_eq!(err.offset, 17);
        assert_eq!(frame.body.take_rest(), [1, 2, 3]);
    }

    #[test]
    fn write_frame_patches_both_lengths() {
        let mut w = Writer::new();
        w.u8(0xee);
        write_frame(
            &mut w,
            FourCc::PWVC,
            |w| {
                w.u16_be(0xaabb);
                Ok(())
            },
            |w| {
                w.bytes(&[1, 2, 3]);
                Ok(())
            },
        )
        .unwrap();
        let mut expected = vec![0xee];
        expected.extend_from_slice(b"PWVC");
        expected.extend_from_slice(&14u32.to_be_bytes());
        expected.extend_from_slice(&17u32.to_be_bytes());
        expected.extend_from_slice(&[0xaa, 0xbb, 1, 2, 3]);
        assert_eq!(w.into_inner(), expected);
    }

    #[test]
    fn write_frame_propagates_part_errors() {
        let mut w = Writer::new();
        let err = write_frame(
            &mut w,
            FourCc::PQTZ,
            |_| Ok(()),
            |_| Err(EncodeError::ValueOutOfRange { context: "beat" }),
        )
        .unwrap_err();
        assert_eq!(err, EncodeError::ValueOutOfRange { context: "beat" });
    }
}
