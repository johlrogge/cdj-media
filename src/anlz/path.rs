//! `PPTH`: the path of the audio file an analysis file belongs to.
//!
//! Offsets are from the start of the tag; all integers are big-endian.
//!
//! | bytes              | field        | meaning                                          |
//! |--------------------|--------------|--------------------------------------------------|
//! | 0..12              | common       | `PPTH`, `len_header` (0x10 or more), `len_tag`   |
//! | 0x0c..0x10         | `len_path`   | length of the body in bytes, including the NUL   |
//! | 0x10..`len_header` | header tail  | not documented; kept verbatim                    |
//! | `len_header`..     | path         | UTF-16BE, ending in a two-byte NUL               |
//!
//! `len_path` must equal the body length, so there are no bytes after the path to keep.

use super::FourCc;
use super::tag::{Frame, write_frame};
use crate::bytes::Writer;
use crate::encoding::utf16;
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Documented `len_header` of a `PPTH` tag.
const LEN_HEADER: u32 = 0x10;

/// The `PPTH` tag: the path of the analysed audio file on the stick, such as
/// `/Contents/Artist/Album/01 Track.mp3`.
///
/// The path's shape is not checked here.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AudioPath {
    path: String,
    header_tail: Vec<u8>,
}

impl AudioPath {
    /// Creates a path tag with no header tail.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            header_tail: Vec::new(),
        }
    }

    /// Replaces the header bytes after the documented header (`0x10..len_header`).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_header_tail(mut self, header_tail: Vec<u8>) -> Self {
        self.header_tail = header_tail;
        self
    }

    /// The path, without its terminating NUL.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Header bytes after the documented header (`0x10..len_header`); empty when built.
    pub fn header_tail(&self) -> &[u8] {
        &self.header_tail
    }

    pub(crate) fn decode(frame: Frame<'_>) -> Result<Self, DecodeError> {
        frame.require_len_header(LEN_HEADER, "PPTH len_header")?;
        let Frame {
            mut header,
            mut body,
            ..
        } = frame;
        let len_path_at = header.position();
        let len_path = header.u32_be().context("PPTH len_path")?;
        let header_tail = header.take_rest().to_vec();

        let actual = u64::try_from(body.remaining()).unwrap_or(u64::MAX);
        if u64::from(len_path) != actual {
            return Err(DecodeError::new(
                len_path_at,
                "PPTH len_path",
                DecodeErrorKind::LengthMismatch {
                    declared: u64::from(len_path),
                    actual,
                },
            ));
        }
        let path = utf16::decode_nul_terminated(&mut body, "PPTH path")?;
        Ok(Self { path, header_tail })
    }

    pub(crate) fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        let len_path = u32::try_from(utf16::encoded_len(&self.path)).map_err(|_| {
            EncodeError::LengthOverflow {
                context: "PPTH len_path",
            }
        })?;
        write_frame(
            w,
            FourCc::PPTH,
            |w| {
                w.u32_be(len_path);
                w.bytes(&self.header_tail);
                Ok(())
            },
            |w| {
                utf16::encode_nul_terminated(w, &self.path);
                Ok(())
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anlz::tag::read_frame;
    use crate::bytes::Reader;

    /// The tag starts at this offset in every test, so offsets are absolute.
    const AT: u64 = 0x40;

    fn utf16_nul(s: &str) -> Vec<u8> {
        let mut out: Vec<u8> = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
        out.extend_from_slice(&[0, 0]);
        out
    }

    /// Builds PPTH bytes by hand: `len_header` 0x10 plus `tail`, then `body`.
    fn ppth(len_path: u32, tail: &[u8], body: &[u8]) -> Vec<u8> {
        let len_header = 0x10 + tail.len() as u32;
        let mut out = b"PPTH".to_vec();
        out.extend_from_slice(&len_header.to_be_bytes());
        out.extend_from_slice(&(len_header + body.len() as u32).to_be_bytes());
        out.extend_from_slice(&len_path.to_be_bytes());
        out.extend_from_slice(tail);
        out.extend_from_slice(body);
        out
    }

    fn decode(bytes: &[u8]) -> Result<AudioPath, DecodeError> {
        let mut padded = vec![0xee; AT as usize];
        padded.extend_from_slice(bytes);
        let mut r = Reader::new(&padded);
        r.bytes(AT as usize).unwrap();
        AudioPath::decode(read_frame(&mut r)?)
    }

    fn encode(path: &AudioPath) -> Vec<u8> {
        let mut w = Writer::new();
        path.encode(&mut w).unwrap();
        w.into_inner()
    }

    fn err(bytes: &[u8]) -> (u64, &'static str, DecodeErrorKind) {
        let e = decode(bytes).unwrap_err();
        (e.offset, e.context, e.kind)
    }

    #[test]
    fn non_ascii_path_decodes_and_round_trips() {
        let path = "/Contents/Björk/Ålbum/01 – Ünïcödé.mp3";
        let body = utf16_nul(path);
        let bytes = ppth(body.len() as u32, &[], &body);
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.path(), path);
        assert_eq!(decoded.header_tail(), [0u8; 0]);
        assert_eq!(decoded, AudioPath::new(path));
        assert_eq!(encode(&decoded), bytes);
    }

    #[test]
    fn header_tail_is_kept() {
        let body = utf16_nul("/a");
        let bytes = ppth(body.len() as u32, &[1, 2, 3], &body);
        let decoded = decode(&bytes).unwrap();
        assert_eq!(
            decoded,
            AudioPath::new("/a").with_header_tail(vec![1, 2, 3])
        );
        assert_eq!(encode(&decoded), bytes);
    }

    #[test]
    fn len_path_other_than_body_length_is_mismatch() {
        let body = utf16_nul("/a");
        assert_eq!(
            err(&ppth(8, &[], &body)),
            (
                AT + 0x0c,
                "PPTH len_path",
                DecodeErrorKind::LengthMismatch {
                    declared: 8,
                    actual: 6,
                }
            )
        );
    }

    #[test]
    fn missing_nul_is_invalid_at_the_last_unit() {
        let body = [0, b'/', 0, b'a'];
        assert_eq!(
            err(&ppth(4, &[], &body)),
            (AT + 0x12, "PPTH path", DecodeErrorKind::InvalidValue)
        );
        assert_eq!(
            err(&ppth(0, &[], &[])),
            (AT + 0x10, "PPTH path", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn odd_body_is_invalid_at_its_last_byte() {
        let body = [0, b'/', 0, 0, 0];
        assert_eq!(
            err(&ppth(5, &[], &body)),
            (AT + 0x14, "PPTH path", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn invalid_utf16_is_invalid_at_the_bad_unit() {
        let body = [0, b'/', 0xdc, 0x00, 0, 0];
        assert_eq!(
            err(&ppth(6, &[], &body)),
            (AT + 0x12, "PPTH path", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn short_len_header_is_invalid() {
        let mut bytes = b"PPTH".to_vec();
        bytes.extend_from_slice(&0x0fu32.to_be_bytes());
        bytes.extend_from_slice(&0x11u32.to_be_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0, 0]);
        assert_eq!(
            err(&bytes),
            (AT + 4, "PPTH len_header", DecodeErrorKind::InvalidValue)
        );
    }
}
