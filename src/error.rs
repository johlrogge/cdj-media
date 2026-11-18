//! Error types for decoding and encoding Pioneer media.
//!
//! Decoding errors carry the absolute byte offset where the failing read started, so a
//! problem in a large `export.pdb` or ANLZ file can be located with a hex viewer.

use thiserror::Error as ThisError;

/// Any error produced by this crate.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// The input bytes could not be decoded.
    #[error(transparent)]
    Decode(#[from] DecodeError),
    /// A value could not be encoded.
    #[error(transparent)]
    Encode(#[from] EncodeError),
    /// Reading or writing a file failed. The underlying error is the
    /// [`source`](std::error::Error::source), so it is not repeated in the message.
    #[error("I/O error")]
    Io(#[from] std::io::Error),
}

/// Failure to decode bytes, located at an absolute byte offset in the input.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
#[error("{context} at offset {offset:#x}: {kind}")]
#[non_exhaustive]
pub struct DecodeError {
    /// Absolute offset in the input where the failing read started.
    pub offset: u64,
    /// What was being decoded when the failure happened.
    pub context: &'static str,
    /// What went wrong.
    pub kind: DecodeErrorKind,
}

impl DecodeError {
    /// Creates a decode error at `offset`.
    pub(crate) fn new(offset: u64, context: &'static str, kind: DecodeErrorKind) -> Self {
        Self {
            offset,
            context,
            kind,
        }
    }
}

/// Names what was being decoded when a read failed, e.g. `.context("PQTZ entry count")`.
///
/// Primitive reads report their own method name (`"u32_be"`) as context. Codecs replace
/// it with the field they were reading. The offset and kind are kept.
pub(crate) trait DecodeContext<T> {
    /// Replaces the context of an error; leaves `Ok` untouched.
    fn context(self, context: &'static str) -> Result<T, DecodeError>;
}

impl<T> DecodeContext<T> for Result<T, DecodeError> {
    fn context(self, context: &'static str) -> Result<T, DecodeError> {
        self.map_err(|err| DecodeError { context, ..err })
    }
}

/// The kind of a [`DecodeError`].
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
#[non_exhaustive]
pub enum DecodeErrorKind {
    /// The input ended before a read could complete.
    #[error("unexpected end of data: needed {needed} bytes, {available} available")]
    #[non_exhaustive]
    UnexpectedEof {
        /// Bytes the read needed.
        needed: usize,
        /// Bytes that were left in the input.
        available: usize,
    },
    /// A four-byte magic tag did not match.
    #[error(
        "bad magic: expected \"{}\", found \"{}\"",
        .expected.escape_ascii(),
        .found.escape_ascii()
    )]
    #[non_exhaustive]
    BadMagic {
        /// The magic that was expected.
        expected: [u8; 4],
        /// The bytes that were found instead.
        found: [u8; 4],
    },
    /// A declared length disagrees with the actual length.
    #[error("length mismatch: declared {declared}, actual {actual}")]
    #[non_exhaustive]
    LengthMismatch {
        /// The length the input declared.
        declared: u64,
        /// The length that was actually available or computed.
        actual: u64,
    },
    /// A value is not valid at this position.
    #[error("invalid value")]
    InvalidValue,
}

/// Failure to encode a value into bytes.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
#[non_exhaustive]
pub enum EncodeError {
    /// A value does not fit the field it is written to.
    #[error("value out of range: {context}")]
    ValueOutOfRange {
        /// The field being encoded.
        context: &'static str,
    },
    /// A length or offset does not fit the field it is written to.
    #[error("length overflow: {context}")]
    LengthOverflow {
        /// The field being encoded.
        context: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_error_message_includes_hex_offset() {
        let err = DecodeError::new(
            0x1234,
            "page header",
            DecodeErrorKind::UnexpectedEof {
                needed: 4,
                available: 2,
            },
        );
        assert_eq!(
            err.to_string(),
            "page header at offset 0x1234: unexpected end of data: needed 4 bytes, 2 available"
        );
    }

    #[test]
    fn bad_magic_message_shows_both_tags() {
        let err = DecodeError::new(
            0x10,
            "anlz header",
            DecodeErrorKind::BadMagic {
                expected: *b"PMAI",
                found: [b'P', b'M', 0x00, 0xff],
            },
        );
        assert_eq!(
            err.to_string(),
            "anlz header at offset 0x10: bad magic: expected \"PMAI\", found \"PM\\x00\\xff\""
        );
    }

    #[test]
    fn top_level_error_wraps_decode_transparently() {
        let decode = DecodeError::new(0x0, "x", DecodeErrorKind::InvalidValue);
        let err = Error::from(decode.clone());
        assert_eq!(err.to_string(), decode.to_string());
    }

    #[test]
    fn io_error_message_does_not_repeat_its_source() {
        use std::error::Error as _;

        let io = std::io::Error::other("disk on fire");
        let err = Error::from(io);
        assert_eq!(err.to_string(), "I/O error");
        let source = err.source().expect("io error is the source");
        assert_eq!(source.to_string(), "disk on fire");
    }

    #[test]
    fn context_replaces_context_and_keeps_offset_and_kind() {
        let kind = DecodeErrorKind::LengthMismatch {
            declared: 8,
            actual: 4,
        };
        let result: Result<(), DecodeError> = Err(DecodeError::new(0x42, "u32_be", kind.clone()));
        let err = result.context("PQTZ entry count").unwrap_err();
        assert_eq!(err.offset, 0x42);
        assert_eq!(err.context, "PQTZ entry count");
        assert_eq!(err.kind, kind);
    }

    #[test]
    fn context_leaves_ok_untouched() {
        let result: Result<u8, DecodeError> = Ok(7);
        assert_eq!(result.context("anything"), Ok(7));
    }

    #[test]
    fn encode_error_messages_name_the_field() {
        let err = EncodeError::LengthOverflow {
            context: "tag length",
        };
        assert_eq!(err.to_string(), "length overflow: tag length");
        let err = EncodeError::ValueOutOfRange { context: "tempo" };
        assert_eq!(err.to_string(), "value out of range: tempo");
    }
}
