//! DeviceSQL strings, as stored in the rows of `export.pdb`.
//!
//! Everything is little-endian. The first byte is a flag:
//!
//! - **Bit 0 set: short ASCII.** `flag >> 1` is the length of the whole field, flag byte
//!   included, so the text is at most 126 bytes. The text follows with no terminator. The
//!   empty string is the single byte `0x03`.
//! - **Bit 0 clear: long form.** A 4-byte header: the flag, a `u16` length counting the
//!   whole field (header included) and a pad byte. Two flags are known:
//!   - `0x40`: ASCII, `length - 4` bytes.
//!   - `0x90`: UTF-16LE, `length - 4` bytes.
//!
//!   Any other flag is rejected. djl-analysis also names a UTF-8 bit (`0x20`) that has never
//!   been seen.
//!
//! The first string of a track row, its ISRC, is special: flag `0x90`, but the content is
//! the byte `0x03`, the ASCII text and a NUL, e.g. `\x03TCAHO2372088\x00`.
//!
//! [`DeviceSqlString`] remembers which form it was read in, so re-encoding a decoded string
//! gives back identical bytes. [`DeviceSqlString::new`] picks the form rekordbox picks.
//!
//! Strings are read from the rows of an `export.pdb` with
//! [`RawRow::string_at`](crate::pdb::RawRow::string_at) and
//! [`RawRow::isrc_at`](crate::pdb::RawRow::isrc_at); the byte-level codec is crate-private.
//!
//! # Pad byte
//!
//! The pad byte of the long form is stored verbatim; it is 0 in every string rekordbox
//! writes. [`DeviceSqlString::new`] and [`DeviceSqlString::isrc`] use 0.

use std::fmt;

use crate::bytes::{Reader, Writer};
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Longest text, in bytes, the short ASCII form can hold.
const SHORT_MAX: usize = 126;
/// Length of the long form's header: flag, `u16` length, pad.
const LONG_HEADER: usize = 4;
/// Flag of the long ASCII form.
const FLAG_LONG_ASCII: u8 = 0x40;
/// Flag of the UTF-16LE form, also used by the ISRC form.
const FLAG_UTF16LE: u8 = 0x90;
/// First content byte of the ISRC form.
const ISRC_PREFIX: u8 = 0x03;

const CTX_FLAG: &str = "DeviceSQL string flag";
const CTX_LENGTH: &str = "DeviceSQL string length";
const CTX_PAD: &str = "DeviceSQL string pad";
const CTX_TEXT: &str = "DeviceSQL string text";
const CTX_ENCODE: &str = "DeviceSQL string";
const CTX_ENCODE_LENGTH: &str = "DeviceSQL string length";
const CTX_ISRC: &str = "DeviceSQL ISRC text";

/// How a [`DeviceSqlString`] is laid out on the stick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StringForm {
    /// Bit 0 of the flag set; at most 126 ASCII bytes.
    ShortAscii,
    /// Flag `0x40`; ASCII with a 4-byte header.
    LongAscii,
    /// Flag `0x90`; UTF-16LE with a 4-byte header.
    Utf16Le,
    /// Flag `0x90`, with `0x03`, ASCII text and a NUL as content. Only in a track's ISRC slot.
    Isrc,
}

/// A string from `export.pdb`, together with the form it is stored in.
///
/// The form, pad byte and text are private, so every value is encodable as its form: short
/// ASCII text is always ASCII and at most 126 bytes, and ISRC text is always ASCII without
/// NULs. Only the length limit of the long forms (65,535 bytes for the whole field) is
/// checked when encoding, with [`EncodeError::LengthOverflow`].
///
/// Equality compares the stored layout, not just the text: the form and the pad byte take
/// part, so `DeviceSqlString::new("hi")` (short ASCII) differs from a decoded long-ASCII
/// `"hi"`. Compare [`as_str`](Self::as_str) to compare text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceSqlString {
    repr: Repr,
    text: String,
}

/// The stored layout of a [`DeviceSqlString`]. Only the long forms have a pad byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Repr {
    ShortAscii,
    LongAscii { pad: u8 },
    Utf16Le { pad: u8 },
    Isrc { pad: u8 },
}

impl DeviceSqlString {
    /// Creates a string in the form rekordbox picks for `text`:
    ///
    /// - ASCII of 126 bytes or fewer: [`StringForm::ShortAscii`];
    /// - longer ASCII: [`StringForm::LongAscii`];
    /// - anything else: [`StringForm::Utf16Le`].
    ///
    /// Long forms get pad byte 0.
    pub fn new(text: &str) -> Self {
        let repr = if !text.is_ascii() {
            Repr::Utf16Le { pad: 0 }
        } else if text.len() <= SHORT_MAX {
            Repr::ShortAscii
        } else {
            Repr::LongAscii { pad: 0 }
        };
        Self {
            repr,
            text: text.to_owned(),
        }
    }

    /// Creates a string in the ISRC form, for the first string slot of a track row.
    ///
    /// The pad byte is 0. Fails with [`EncodeError::ValueOutOfRange`] unless `text` is ASCII
    /// without NULs.
    pub fn isrc(text: &str) -> Result<Self, EncodeError> {
        if !is_isrc_text(text.as_bytes()) {
            return Err(EncodeError::ValueOutOfRange { context: CTX_ISRC });
        }
        Ok(Self {
            repr: Repr::Isrc { pad: 0 },
            text: text.to_owned(),
        })
    }

    /// The form the string is stored in.
    pub fn form(&self) -> StringForm {
        match self.repr {
            Repr::ShortAscii => StringForm::ShortAscii,
            Repr::LongAscii { .. } => StringForm::LongAscii,
            Repr::Utf16Le { .. } => StringForm::Utf16Le,
            Repr::Isrc { .. } => StringForm::Isrc,
        }
    }

    /// The pad byte of a long form, or `None` for short ASCII.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    pub(crate) fn pad(&self) -> Option<u8> {
        match self.repr {
            Repr::ShortAscii => None,
            Repr::LongAscii { pad } | Repr::Utf16Le { pad } | Repr::Isrc { pad } => Some(pad),
        }
    }

    /// The text, without any header, prefix or terminator.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Encodes the string field.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "typed rows encode strings in a later slice")
    )]
    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let mut w = Writer::new();
        self.encode(&mut w)?;
        Ok(w.into_inner())
    }

    /// Length in bytes of the encoded field, header included.
    ///
    /// Saturates at `usize::MAX`, which is far beyond what the length field can hold.
    pub(crate) fn encoded_len(&self) -> usize {
        let text = self.text.len();
        match self.repr {
            Repr::ShortAscii => text.saturating_add(1),
            Repr::LongAscii { .. } => text.saturating_add(LONG_HEADER),
            Repr::Utf16Le { .. } => self
                .text
                .encode_utf16()
                .count()
                .saturating_mul(2)
                .saturating_add(LONG_HEADER),
            // Header, prefix byte, text, NUL.
            Repr::Isrc { .. } => text.saturating_add(LONG_HEADER + 2),
        }
    }

    /// Writes the string field, with the stored pad byte for a long form.
    ///
    /// Fails with [`EncodeError::LengthOverflow`], writing nothing, when the field is longer
    /// than its length field can hold.
    pub(crate) fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        let len = u16::try_from(self.encoded_len()).map_err(|_| EncodeError::LengthOverflow {
            context: CTX_ENCODE_LENGTH,
        })?;
        match self.repr {
            Repr::ShortAscii => {
                // The constructors keep short text at most 126 bytes, so `len` is at most 127.
                let len = u8::try_from(len).ok().filter(|&len| len <= 0x7f).ok_or(
                    EncodeError::ValueOutOfRange {
                        context: CTX_ENCODE,
                    },
                )?;
                w.u8((len << 1) | 1);
                w.bytes(self.text.as_bytes());
            }
            Repr::LongAscii { pad } => {
                long_header(w, FLAG_LONG_ASCII, len, pad);
                w.bytes(self.text.as_bytes());
            }
            Repr::Utf16Le { pad } => {
                long_header(w, FLAG_UTF16LE, len, pad);
                for unit in self.text.encode_utf16() {
                    w.u16_le(unit);
                }
            }
            Repr::Isrc { pad } => {
                long_header(w, FLAG_UTF16LE, len, pad);
                w.u8(ISRC_PREFIX);
                w.bytes(self.text.as_bytes());
                w.u8(0);
            }
        }
        Ok(())
    }
}

impl fmt::Display for DeviceSqlString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

fn long_header(w: &mut Writer, flag: u8, len: u16, pad: u8) {
    w.u8(flag);
    w.u16_le(len);
    w.u8(pad);
}

/// Whether `bytes` may be the text of an ISRC string: ASCII without NULs.
fn is_isrc_text(bytes: &[u8]) -> bool {
    bytes.iter().all(|&b| b.is_ascii() && b != 0)
}

/// Which string slot is being decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Plain,
    Isrc,
}

/// Decodes one string field; flag `0x90` is always UTF-16LE.
///
/// On failure the reader does not advance. Errors, at absolute offsets:
/// - [`DecodeErrorKind::UnexpectedEof`] where the header or text runs past the input;
/// - [`DecodeErrorKind::InvalidValue`] at the flag, for an unknown flag;
/// - [`DecodeErrorKind::InvalidValue`] at the flag (short form) or the length (long form),
///   for a length shorter than the header;
/// - [`DecodeErrorKind::InvalidValue`] at the offending byte, for non-ASCII in an ASCII form;
/// - [`DecodeErrorKind::InvalidValue`] at the last byte, for an odd UTF-16 byte count;
/// - [`DecodeErrorKind::InvalidValue`] at the code unit, for an unpaired surrogate.
pub(crate) fn decode(r: &mut Reader<'_>) -> Result<DeviceSqlString, DecodeError> {
    decode_in(r, Slot::Plain)
}

/// Decodes the string field in a track's ISRC slot.
///
/// The caller says which slot this is, rather than [`decode`] guessing from content,
/// because the ISRC shape is ambiguous: `"\u{103}a"` in UTF-16LE is `03 01 61 00`, which
/// also reads as an ISRC. Within this slot, flag `0x90` content that is `0x03`, ASCII
/// without NULs and a final NUL is [`StringForm::Isrc`]; any other content is read as
/// UTF-16LE, so the slot still round-trips losslessly. Errors are those of [`decode`].
pub(crate) fn decode_isrc(r: &mut Reader<'_>) -> Result<DeviceSqlString, DecodeError> {
    decode_in(r, Slot::Isrc)
}

fn decode_in(r: &mut Reader<'_>, slot: Slot) -> Result<DeviceSqlString, DecodeError> {
    let mut probe = r.clone();
    let flag_at = probe.position();
    let flag = probe.u8().context(CTX_FLAG)?;

    let string = if flag & 1 == 1 {
        let Some(text_len) = usize::from(flag >> 1).checked_sub(1) else {
            return Err(invalid(flag_at, CTX_LENGTH));
        };
        let text_at = probe.position();
        let bytes = probe.bytes(text_len).context(CTX_TEXT)?;
        DeviceSqlString {
            repr: Repr::ShortAscii,
            text: ascii(bytes, text_at)?,
        }
    } else {
        if flag != FLAG_LONG_ASCII && flag != FLAG_UTF16LE {
            return Err(invalid(flag_at, CTX_FLAG));
        }
        let len_at = probe.position();
        let len = probe.u16_le().context(CTX_LENGTH)?;
        let pad = probe.u8().context(CTX_PAD)?;
        let Some(text_len) = usize::from(len).checked_sub(LONG_HEADER) else {
            return Err(invalid(len_at, CTX_LENGTH));
        };
        let text_at = probe.position();
        let bytes = probe.bytes(text_len).context(CTX_TEXT)?;
        if flag == FLAG_LONG_ASCII {
            DeviceSqlString {
                repr: Repr::LongAscii { pad },
                text: ascii(bytes, text_at)?,
            }
        } else if let Some(text) = isrc_content(bytes).filter(|_| slot == Slot::Isrc) {
            DeviceSqlString {
                repr: Repr::Isrc { pad },
                text,
            }
        } else {
            DeviceSqlString {
                repr: Repr::Utf16Le { pad },
                text: utf16le(bytes, text_at)?,
            }
        }
    };
    *r = probe;
    Ok(string)
}

fn invalid(offset: u64, context: &'static str) -> DecodeError {
    DecodeError::new(offset, context, DecodeErrorKind::InvalidValue)
}

fn offset_of(start: u64, index: usize) -> u64 {
    start.saturating_add(u64::try_from(index).unwrap_or(u64::MAX))
}

/// Reads ASCII text that starts at absolute offset `start`.
fn ascii(bytes: &[u8], start: u64) -> Result<String, DecodeError> {
    if let Some(index) = bytes.iter().position(|b| !b.is_ascii()) {
        return Err(invalid(offset_of(start, index), CTX_TEXT));
    }
    Ok(bytes.iter().copied().map(char::from).collect())
}

/// Returns the text of ISRC-shaped content: `0x03`, ASCII without NULs, NUL.
fn isrc_content(bytes: &[u8]) -> Option<String> {
    let [ISRC_PREFIX, text @ .., 0] = bytes else {
        return None;
    };
    is_isrc_text(text).then(|| text.iter().copied().map(char::from).collect())
}

/// Reads UTF-16LE text that starts at absolute offset `start`.
fn utf16le(bytes: &[u8], start: u64) -> Result<String, DecodeError> {
    if bytes.len() % 2 != 0 {
        return Err(invalid(offset_of(start, bytes.len() - 1), CTX_TEXT));
    }
    let units = bytes
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]));
    let mut out = String::with_capacity(bytes.len() / 2);
    let mut unit_index = 0usize;
    for decoded in char::decode_utf16(units) {
        match decoded {
            Ok(c) => {
                out.push(c);
                unit_index += c.len_utf16();
            }
            Err(_) => return Err(invalid(offset_of(start, unit_index * 2), CTX_TEXT)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::*;

    const BASE: u64 = 0x100;

    fn encode(s: &DeviceSqlString) -> Vec<u8> {
        s.to_bytes().expect("encodes")
    }

    /// Decodes `bytes` at `BASE`, checking the reader moves only on success.
    fn decode_at(bytes: &[u8], slot: Slot) -> Result<DeviceSqlString, DecodeError> {
        let mut r = Reader::with_base(bytes, BASE);
        let result = decode_in(&mut r, slot);
        match &result {
            Ok(s) => assert_eq!(r.position(), BASE + s.encoded_len() as u64),
            Err(_) => assert_eq!(r.position(), BASE),
        }
        result
    }

    fn plain(bytes: &[u8]) -> Result<DeviceSqlString, DecodeError> {
        decode_at(bytes, Slot::Plain)
    }

    fn invalid_at(rel: u64, context: &'static str) -> Result<DeviceSqlString, DecodeError> {
        Err(invalid(BASE + rel, context))
    }

    fn eof_at(
        rel: u64,
        context: &'static str,
        needed: usize,
        available: usize,
    ) -> Result<DeviceSqlString, DecodeError> {
        Err(DecodeError::new(
            BASE + rel,
            context,
            DecodeErrorKind::UnexpectedEof { needed, available },
        ))
    }

    #[test]
    fn empty_string_is_a_single_byte() {
        let s = DeviceSqlString::new("");
        assert_eq!(s.form(), StringForm::ShortAscii);
        assert_eq!(encode(&s), [0x03]);
        assert_eq!(plain(&[0x03]), Ok(s));
    }

    #[test]
    fn short_ascii_has_a_one_byte_header() {
        let s = DeviceSqlString::new("abc");
        assert_eq!(encode(&s), [0x09, b'a', b'b', b'c']);
        assert_eq!(plain(&[0x09, b'a', b'b', b'c', 0xff]), Ok(s));
    }

    #[test]
    fn ascii_up_to_126_bytes_is_short() {
        let text = "a".repeat(126);
        let s = DeviceSqlString::new(&text);
        assert_eq!(s.form(), StringForm::ShortAscii);
        let bytes = encode(&s);
        assert_eq!(bytes.len(), 127);
        assert_eq!(bytes[0], 0xff);
        assert_eq!(&bytes[1..], text.as_bytes());
    }

    #[test]
    fn ascii_of_127_bytes_is_long() {
        let text = "a".repeat(127);
        let s = DeviceSqlString::new(&text);
        assert_eq!(s.form(), StringForm::LongAscii);
        let bytes = encode(&s);
        assert_eq!(bytes[..4], [0x40, 131, 0, 0]);
        assert_eq!(&bytes[4..], text.as_bytes());
        assert_eq!(plain(&bytes), Ok(s));
    }

    #[test]
    fn non_ascii_is_utf16le() {
        let s = DeviceSqlString::new("Björk");
        assert_eq!(s.form(), StringForm::Utf16Le);
        let bytes = [
            0x90, 0x0e, 0x00, 0x00, b'B', 0, b'j', 0, 0xf6, 0, b'r', 0, b'k', 0,
        ];
        assert_eq!(encode(&s), bytes);
        assert_eq!(plain(&bytes), Ok(s));
    }

    #[test]
    fn non_bmp_utf16le_uses_surrogate_pairs() {
        let s = DeviceSqlString::new("\u{1f3b5}");
        assert_eq!(encode(&s), [0x90, 8, 0, 0, 0x3c, 0xd8, 0xb5, 0xdf]);
    }

    #[test]
    fn isrc_bytes_match_rekordbox() {
        let s = DeviceSqlString::isrc("TCAHO2372088").unwrap();
        assert_eq!(s.form(), StringForm::Isrc);
        assert_eq!(s.as_str(), "TCAHO2372088");
        let mut bytes = vec![0x90, 0x12, 0x00, 0x00, 0x03];
        bytes.extend_from_slice(b"TCAHO2372088");
        bytes.push(0);
        assert_eq!(encode(&s), bytes);
        assert_eq!(decode_at(&bytes, Slot::Isrc), Ok(s));
    }

    #[test]
    fn isrc_shape_outside_the_isrc_slot_is_utf16le() {
        let bytes = [0x90, 8, 0, 0, 0x03, 0x01, b'a', 0];
        let s = plain(&bytes).unwrap();
        assert_eq!(s, DeviceSqlString::new("\u{103}a"));
        assert_eq!(encode(&s), bytes);
    }

    #[test]
    fn isrc_slot_accepts_other_forms() {
        assert_eq!(decode_at(&[0x03], Slot::Isrc), Ok(DeviceSqlString::new("")));
        let utf16 = encode(&DeviceSqlString::new("Björk"));
        assert_eq!(
            decode_at(&utf16, Slot::Isrc),
            Ok(DeviceSqlString::new("Björk"))
        );
    }

    #[test]
    fn isrc_rejects_non_ascii_and_nul() {
        let err = EncodeError::ValueOutOfRange { context: CTX_ISRC };
        assert_eq!(DeviceSqlString::isrc("Björk"), Err(err.clone()));
        assert_eq!(DeviceSqlString::isrc("a\0b"), Err(err));
        assert!(DeviceSqlString::isrc("").is_ok());
    }

    #[test]
    fn display_and_as_str_show_the_text() {
        let s = DeviceSqlString::new("Björk");
        assert_eq!(s.as_str(), "Björk");
        assert_eq!(s.to_string(), "Björk");
    }

    #[test]
    fn too_long_strings_fail_to_encode_and_write_nothing() {
        let err = EncodeError::LengthOverflow {
            context: "DeviceSQL string length",
        };
        let longest = DeviceSqlString::new(&"a".repeat(usize::from(u16::MAX) - 4));
        assert_eq!(encode(&longest).len(), usize::from(u16::MAX));
        let s = DeviceSqlString::new(&"a".repeat(usize::from(u16::MAX) - 3));
        let mut w = Writer::new();
        assert_eq!(s.encode(&mut w), Err(err.clone()));
        assert_eq!(w.position(), 0);
        let s = DeviceSqlString::new(&"ö".repeat(usize::from(u16::MAX) / 2));
        assert_eq!(s.to_bytes(), Err(err.clone()));
        let s = DeviceSqlString::isrc(&"a".repeat(usize::from(u16::MAX) - 5)).unwrap();
        assert_eq!(s.to_bytes(), Err(err));
    }

    #[test]
    fn empty_input_is_eof_at_the_flag() {
        assert_eq!(plain(&[]), eof_at(0, CTX_FLAG, 1, 0));
    }

    #[test]
    fn unknown_flags_are_invalid_at_the_flag() {
        for flag in [0x00, 0x10, 0x20, 0x80, 0x92, 0xc0] {
            assert_eq!(
                plain(&[flag, 4, 0, 0]),
                invalid_at(0, CTX_FLAG),
                "{flag:#x}"
            );
        }
    }

    #[test]
    fn short_length_shorter_than_the_flag_is_invalid() {
        assert_eq!(plain(&[0x01]), invalid_at(0, CTX_LENGTH));
    }

    #[test]
    fn short_text_past_the_end_is_eof() {
        assert_eq!(plain(&[0x09, b'a']), eof_at(1, CTX_TEXT, 3, 1));
    }

    #[test]
    fn short_non_ascii_is_invalid_at_the_byte() {
        assert_eq!(plain(&[0x07, b'a', 0x80, b'c']), invalid_at(2, CTX_TEXT));
    }

    #[test]
    fn long_header_past_the_end_is_eof() {
        assert_eq!(plain(&[0x40, 5]), eof_at(1, CTX_LENGTH, 2, 1));
        assert_eq!(plain(&[0x90, 5, 0]), eof_at(3, CTX_PAD, 1, 0));
    }

    #[test]
    fn long_length_shorter_than_the_header_is_invalid() {
        for len in 0..4 {
            assert_eq!(plain(&[0x40, len, 0, 0]), invalid_at(1, CTX_LENGTH));
            assert_eq!(plain(&[0x90, len, 0, 0]), invalid_at(1, CTX_LENGTH));
        }
        assert_eq!(plain(&[0x40, 4, 0, 0]), Ok(long_ascii("")));
    }

    #[test]
    fn non_zero_pad_round_trips() {
        let mut isrc = vec![0x90, 8, 0, 0x7f, 0x03];
        isrc.extend_from_slice(b"AB\0");
        let cases: [(&[u8], Slot, StringForm, u8); 3] = [
            (
                &[0x40, 5, 0, 1, b'a'],
                Slot::Plain,
                StringForm::LongAscii,
                1,
            ),
            (
                &[0x90, 6, 0, 0xff, b'a', 0],
                Slot::Plain,
                StringForm::Utf16Le,
                0xff,
            ),
            (&isrc, Slot::Isrc, StringForm::Isrc, 0x7f),
        ];
        for (bytes, slot, form, pad) in cases {
            let s = decode_at(bytes, slot).unwrap();
            assert_eq!(s.form(), form);
            assert_eq!(s.pad(), Some(pad));
            assert_eq!(encode(&s), bytes);
        }
    }

    #[test]
    fn encode_writes_the_stored_pad() {
        let s = DeviceSqlString {
            repr: Repr::LongAscii { pad: 7 },
            text: "a".into(),
        };
        assert_eq!(encode(&s), [0x40, 5, 0, 7, b'a']);
    }

    #[test]
    fn pad_is_none_for_short_and_zero_from_constructors() {
        assert_eq!(DeviceSqlString::new("hi").pad(), None);
        assert_eq!(DeviceSqlString::new(&"a".repeat(127)).pad(), Some(0));
        assert_eq!(DeviceSqlString::new("Björk").pad(), Some(0));
        assert_eq!(DeviceSqlString::isrc("AB").unwrap().pad(), Some(0));
    }

    #[test]
    fn equality_includes_form_and_pad() {
        let long = plain(&[0x40, 6, 0, 0, b'h', b'i']).unwrap();
        let padded = plain(&[0x40, 6, 0, 1, b'h', b'i']).unwrap();
        assert_ne!(long, DeviceSqlString::new("hi"));
        assert_ne!(long, padded);
        assert_eq!(long.as_str(), DeviceSqlString::new("hi").as_str());
    }

    #[test]
    fn long_text_past_the_end_is_eof() {
        assert_eq!(
            plain(&[0x40, 8, 0, 0, b'a', b'b']),
            eof_at(4, CTX_TEXT, 4, 2)
        );
        assert_eq!(plain(&[0x90, 0, 1, 0]), eof_at(4, CTX_TEXT, 252, 0));
    }

    #[test]
    fn long_non_ascii_is_invalid_at_the_byte() {
        assert_eq!(plain(&[0x40, 6, 0, 0, b'a', 0xc3]), invalid_at(5, CTX_TEXT));
    }

    #[test]
    fn odd_utf16_byte_count_is_invalid_at_the_last_byte() {
        assert_eq!(
            plain(&[0x90, 7, 0, 0, b'a', 0, b'b']),
            invalid_at(6, CTX_TEXT)
        );
        assert_eq!(plain(&[0x90, 5, 0, 0, b'a']), invalid_at(4, CTX_TEXT));
    }

    #[test]
    fn unpaired_surrogates_are_invalid_at_the_unit() {
        // A lone low surrogate in the second unit.
        assert_eq!(
            plain(&[0x90, 8, 0, 0, b'a', 0, 0x00, 0xdc]),
            invalid_at(6, CTX_TEXT)
        );
        // A high surrogate at the end.
        assert_eq!(
            plain(&[0x90, 8, 0, 0, b'a', 0, 0x00, 0xd8]),
            invalid_at(6, CTX_TEXT)
        );
    }

    #[test]
    fn decoded_long_forms_keep_their_form() {
        // Short enough for the short form, but rekordbox-style decoding must keep the form.
        let bytes = [0x40, 6, 0, 0, b'h', b'i'];
        let s = plain(&bytes).unwrap();
        assert_eq!(s, long_ascii("hi"));
        assert_eq!(encode(&s), bytes);
        let bytes = [0x90, 8, 0, 0, b'h', 0, b'i', 0];
        let s = plain(&bytes).unwrap();
        assert_eq!(s.form(), StringForm::Utf16Le);
        assert_eq!(encode(&s), bytes);
    }

    fn long_ascii(text: &str) -> DeviceSqlString {
        DeviceSqlString {
            repr: Repr::LongAscii { pad: 0 },
            text: text.into(),
        }
    }

    fn expected_form(s: &str) -> StringForm {
        match (s.is_ascii(), s.len()) {
            (false, _) => StringForm::Utf16Le,
            (true, 0..=126) => StringForm::ShortAscii,
            (true, _) => StringForm::LongAscii,
        }
    }

    fn text() -> impl Strategy<Value = String> {
        prop_oneof![
            any::<String>(),
            "[ -~]{125,128}",
            "[ -~]{0,4}",
            "[ -~]{120,124}[^\\x00-\\x7f]{1,4}",
        ]
    }

    proptest! {
        #[test]
        fn new_round_trips_text_form_and_bytes(s in text()) {
            let string = DeviceSqlString::new(&s);
            prop_assert_eq!(string.form(), expected_form(&s));
            let bytes = encode(&string);
            prop_assert_eq!(bytes.len(), string.encoded_len());
            let decoded = plain(&bytes).unwrap();
            prop_assert_eq!(decoded.as_str(), s.as_str());
            prop_assert_eq!(decoded.form(), string.form());
            prop_assert_eq!(encode(&decoded), bytes);
        }

        #[test]
        fn isrc_round_trips(s in "[\\x01-\\x7f]{0,20}") {
            let string = DeviceSqlString::isrc(&s).unwrap();
            let bytes = encode(&string);
            let decoded = decode_at(&bytes, Slot::Isrc).unwrap();
            prop_assert_eq!(&decoded, &string);
            prop_assert_eq!(encode(&decoded), bytes);
        }

        #[test]
        fn decoding_arbitrary_bytes_never_panics(bytes in vec(any::<u8>(), 0..64)) {
            for slot in [Slot::Plain, Slot::Isrc] {
                if let Ok(s) = decode_at(&bytes, slot) {
                    let encoded = encode(&s);
                    prop_assert_eq!(&bytes[..encoded.len()], &encoded[..]);
                }
            }
        }

        #[test]
        fn decoding_flag_led_bytes_never_panics(
            flag in prop_oneof![Just(0x40u8), Just(0x90u8), any::<u8>()],
            len in 0u8..40,
            rest in vec(any::<u8>(), 0..48),
        ) {
            let mut bytes = vec![flag, len, 0, 0];
            bytes.extend(rest);
            for slot in [Slot::Plain, Slot::Isrc] {
                if let Ok(s) = decode_at(&bytes, slot) {
                    let encoded = encode(&s);
                    prop_assert_eq!(&bytes[..encoded.len()], &encoded[..]);
                }
            }
        }
    }
}
