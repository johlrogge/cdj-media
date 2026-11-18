//! UTF-16BE strings ending in a two-byte NUL, as in the ANLZ `PPTH` tag.
//!
//! The NUL is not part of the decoded string. NULs before the final one are kept as `'\0'`
//! characters, so decoding then encoding gives back the same bytes.

use crate::bytes::{Reader, Writer};
use crate::error::{DecodeError, DecodeErrorKind};

/// Decodes everything left in `r` as UTF-16BE ending in a two-byte NUL.
///
/// On failure the reader does not advance and the error, named `context`, is
/// [`DecodeErrorKind::InvalidValue`] at:
/// - the last byte, when the length is odd;
/// - the last code unit (or the start, when empty), when it is not NUL;
/// - the offending code unit, when a surrogate is unpaired.
pub(crate) fn decode_nul_terminated(
    r: &mut Reader<'_>,
    context: &'static str,
) -> Result<String, DecodeError> {
    let mut probe = r.clone();
    let start = probe.position();
    let bytes = probe.take_rest();
    let invalid = |index: usize| {
        let offset = start.saturating_add(u64::try_from(index).unwrap_or(u64::MAX));
        DecodeError::new(offset, context, DecodeErrorKind::InvalidValue)
    };

    if bytes.len() % 2 != 0 {
        return Err(invalid(bytes.len() - 1));
    }
    let Some((text, nul)) = bytes.split_last_chunk::<2>() else {
        return Err(invalid(0));
    };
    if nul != &[0, 0] {
        return Err(invalid(text.len()));
    }

    let units = text
        .chunks_exact(2)
        .map(|unit| u16::from_be_bytes([unit[0], unit[1]]));
    let mut out = String::with_capacity(text.len() / 2);
    let mut unit_index = 0usize;
    for decoded in char::decode_utf16(units) {
        match decoded {
            Ok(c) => {
                out.push(c);
                unit_index += c.len_utf16();
            }
            Err(_) => return Err(invalid(unit_index * 2)),
        }
    }
    *r = probe;
    Ok(out)
}

/// Length in bytes of `s` encoded by [`encode_nul_terminated`], including the NUL.
///
/// Saturates at `usize::MAX`, which no caller's length field can hold.
pub(crate) fn encoded_len(s: &str) -> usize {
    s.encode_utf16().count().saturating_add(1).saturating_mul(2)
}

/// Writes `s` as UTF-16BE followed by a two-byte NUL.
pub(crate) fn encode_nul_terminated(w: &mut Writer, s: &str) {
    for unit in s.encode_utf16() {
        w.u16_be(unit);
    }
    w.u16_be(0);
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn encode(s: &str) -> Vec<u8> {
        let mut w = Writer::new();
        encode_nul_terminated(&mut w, s);
        w.into_inner()
    }

    fn decode_at(bytes: &[u8], base: u64) -> Result<String, DecodeError> {
        let mut r = Reader::with_base(bytes, base);
        let result = decode_nul_terminated(&mut r, "test string");
        let expected_pos = if result.is_ok() {
            base + bytes.len() as u64
        } else {
            base
        };
        assert_eq!(r.position(), expected_pos);
        result
    }

    fn invalid_at(offset: u64) -> DecodeError {
        DecodeError::new(offset, "test string", DecodeErrorKind::InvalidValue)
    }

    #[test]
    fn ascii_is_big_endian_with_a_nul() {
        assert_eq!(encode("ab"), [0, b'a', 0, b'b', 0, 0]);
        assert_eq!(encoded_len("ab"), 6);
        assert_eq!(decode_at(&[0, b'a', 0, b'b', 0, 0], 0), Ok("ab".into()));
    }

    #[test]
    fn empty_string_is_just_the_nul() {
        assert_eq!(encode(""), [0, 0]);
        assert_eq!(decode_at(&[0, 0], 0), Ok(String::new()));
    }

    #[test]
    fn non_bmp_characters_use_surrogate_pairs() {
        let bytes = encode("\u{1f3b5}");
        assert_eq!(bytes, [0xd8, 0x3c, 0xdf, 0xb5, 0, 0]);
        assert_eq!(encoded_len("\u{1f3b5}"), 6);
        assert_eq!(decode_at(&bytes, 0), Ok("\u{1f3b5}".into()));
    }

    #[test]
    fn odd_length_is_reported_at_the_last_byte() {
        assert_eq!(decode_at(&[0, b'a', 0], 0x20), Err(invalid_at(0x22)));
        assert_eq!(decode_at(&[0], 0x20), Err(invalid_at(0x20)));
    }

    #[test]
    fn missing_nul_is_reported_at_the_last_unit() {
        assert_eq!(decode_at(&[0, b'a', 0, b'b'], 0x20), Err(invalid_at(0x22)));
        assert_eq!(decode_at(&[], 0x20), Err(invalid_at(0x20)));
    }

    #[test]
    fn unpaired_surrogates_are_reported_where_they_are() {
        // A lone low surrogate in the second unit.
        assert_eq!(
            decode_at(&[0, b'a', 0xdc, 0x00, 0, 0], 0x20),
            Err(invalid_at(0x22))
        );
        // A high surrogate followed by a non-surrogate.
        assert_eq!(
            decode_at(&[0, b'a', 0, b'b', 0xd8, 0x00, 0, b'c', 0, 0], 0x20),
            Err(invalid_at(0x24))
        );
        // A high surrogate right before the NUL.
        assert_eq!(decode_at(&[0xd8, 0x00, 0, 0], 0x20), Err(invalid_at(0x20)));
    }

    #[test]
    fn interior_nuls_are_kept() {
        let bytes = [0, b'a', 0, 0, 0, b'b', 0, 0];
        assert_eq!(decode_at(&bytes, 0), Ok("a\0b".into()));
        assert_eq!(encode("a\0b"), bytes);
    }

    proptest! {
        #[test]
        fn any_string_round_trips(s in any::<String>()) {
            let bytes = encode(&s);
            prop_assert_eq!(bytes.len(), encoded_len(&s));
            prop_assert_eq!(decode_at(&bytes, 0), Ok(s));
        }

        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
            if let Ok(s) = decode_at(&bytes, 0) {
                prop_assert_eq!(encode(&s), bytes);
            }
        }
    }
}
