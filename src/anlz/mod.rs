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

mod tag;

pub use tag::{FourCc, RawTag, Tag};

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
        Tag::Raw(RawTag::new(fourcc, header.to_vec(), body.to_vec()))
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

    #[test]
    fn multi_tag_file_round_trips_in_order() {
        let bytes = file_bytes(
            &UNKNOWN,
            &[
                tag_bytes(b"PPTH", &[0, 0, 0, 4], b"a\0b\0"),
                tag_bytes(b"PQTZ", &[0; 12], &[1, 2, 3, 4, 5, 6, 7, 8]),
                tag_bytes(b"PCOB", &[0; 12], &[]),
                tag_bytes(b"PCOB", &[1; 12], &[9]),
                tag_bytes(b"ZZZZ", &[], &[]),
            ],
        );
        let expected = AnlzFile::new(
            UNKNOWN.to_vec(),
            vec![
                raw(FourCc::PPTH, &[0, 0, 0, 4], b"a\0b\0"),
                raw(FourCc::PQTZ, &[0; 12], &[1, 2, 3, 4, 5, 6, 7, 8]),
                raw(FourCc::PCOB, &[0; 12], &[]),
                raw(FourCc::PCOB, &[1; 12], &[9]),
                raw(FourCc::new(*b"ZZZZ"), &[], &[]),
            ],
        );
        assert_round_trip(&bytes, &expected);

        let decoded = AnlzFile::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.unknown_header(), UNKNOWN);
        assert_eq!(decoded.tags().len(), 5);
        let cob_headers: Vec<&[u8]> = decoded
            .tags_with(FourCc::PCOB)
            .map(|tag| match tag {
                Tag::Raw(raw) => raw.header(),
            })
            .collect();
        assert_eq!(cob_headers, [[0; 12], [1; 12]]);
        assert_eq!(decoded.tags_with(FourCc::PWV3).count(), 0);
        assert_eq!(decoded.tags_with(FourCc::PPTH).count(), 1);
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
        tag_bytes(b"PPTH", &[0; 2], &[1])
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

    fn fourcc() -> impl Strategy<Value = FourCc> {
        prop_oneof![
            Just(FourCc::PPTH),
            Just(FourCc::PQTZ),
            Just(FourCc::PCOB),
            any::<[u8; 4]>().prop_map(FourCc::new),
        ]
    }

    fn tag() -> impl Strategy<Value = Tag> {
        (fourcc(), vec(any::<u8>(), 0..24), vec(any::<u8>(), 0..64))
            .prop_map(|(fourcc, header, body)| Tag::Raw(RawTag::new(fourcc, header, body)))
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
