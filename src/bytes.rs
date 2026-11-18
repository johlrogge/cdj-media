//! Crate-private byte primitives shared by every codec (see ADR-001).
//!
//! - [`Reader`] reads typed big- and little-endian values from a borrowed slice and reports
//!   failures at the absolute offset where the read started. Reads never panic, never
//!   overflow, and leave the reader where it was when they fail.
//! - [`Writer`] appends typed values to a `Vec<u8>`.
//! - [`Patch`] reserves a fixed-width slot in a [`Writer`] to be filled in later, once a
//!   length or offset is known.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{DecodeError, DecodeErrorKind, EncodeError};

/// Reads typed values from a byte slice, tracking an absolute offset.
///
/// A reader created with [`Reader::with_base`] or [`Reader::sub_reader`] reports offsets
/// relative to the start of the whole input, not to its own slice.
#[derive(Debug, Clone)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    base: u64,
}

impl<'a> Reader<'a> {
    /// Creates a reader whose first byte is at absolute offset 0.
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self::with_base(data, 0)
    }

    /// Creates a reader whose first byte is at absolute offset `base`.
    ///
    /// `base + data.len()` must fit in a `u64`.
    pub(crate) fn with_base(data: &'a [u8], base: u64) -> Self {
        debug_assert!(
            base.checked_add(data.len() as u64).is_some(),
            "reader end offset overflows u64"
        );
        Self { data, pos: 0, base }
    }

    /// Absolute offset of the next byte to be read.
    pub(crate) fn position(&self) -> u64 {
        self.absolute(self.pos)
    }

    /// Number of bytes left to read.
    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Reads one byte.
    pub(crate) fn u8(&mut self) -> Result<u8, DecodeError> {
        self.array_in::<1>("u8").map(|[b]| b)
    }

    /// Reads a big-endian `u16`.
    pub(crate) fn u16_be(&mut self) -> Result<u16, DecodeError> {
        self.array_in("u16_be").map(u16::from_be_bytes)
    }

    /// Reads a little-endian `u16`.
    pub(crate) fn u16_le(&mut self) -> Result<u16, DecodeError> {
        self.array_in("u16_le").map(u16::from_le_bytes)
    }

    /// Reads a big-endian `u32`.
    pub(crate) fn u32_be(&mut self) -> Result<u32, DecodeError> {
        self.array_in("u32_be").map(u32::from_be_bytes)
    }

    /// Reads a little-endian `u32`.
    pub(crate) fn u32_le(&mut self) -> Result<u32, DecodeError> {
        self.array_in("u32_le").map(u32::from_le_bytes)
    }

    /// Reads a big-endian `u64`.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    pub(crate) fn u64_be(&mut self) -> Result<u64, DecodeError> {
        self.array_in("u64_be").map(u64::from_be_bytes)
    }

    /// Reads a little-endian `u64`.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    pub(crate) fn u64_le(&mut self) -> Result<u64, DecodeError> {
        self.array_in("u64_le").map(u64::from_le_bytes)
    }

    /// Reads `n` bytes, borrowed from the input.
    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        self.take(n, "bytes")
    }

    /// Reads everything left, borrowed from the input. Never fails.
    pub(crate) fn take_rest(&mut self) -> &'a [u8] {
        let rest = self.rest();
        self.advance(rest.len());
        rest
    }

    /// Reads exactly `N` bytes into an array.
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        self.array_in("array")
    }

    /// Skips `n` bytes.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    pub(crate) fn skip(&mut self, n: usize) -> Result<(), DecodeError> {
        self.take(n, "skip").map(|_| ())
    }

    /// Moves to absolute offset `abs`.
    ///
    /// `abs` may be anywhere from the start of this reader's slice up to and including its
    /// end. Anything else fails with [`DecodeErrorKind::InvalidValue`] at offset `abs`.
    pub(crate) fn seek(&mut self, abs: u64) -> Result<(), DecodeError> {
        let target = abs
            .checked_sub(self.base)
            .and_then(|rel| usize::try_from(rel).ok())
            .filter(|&rel| rel <= self.data.len());
        match target {
            Some(rel) => {
                self.pos = rel;
                Ok(())
            }
            None => Err(DecodeError::new(
                abs,
                "seek target outside input",
                DecodeErrorKind::InvalidValue,
            )),
        }
    }

    /// Splits off the next `len` bytes as a reader of their own and advances past them.
    ///
    /// The sub-reader keeps reporting absolute offsets.
    pub(crate) fn sub_reader(&mut self, len: usize) -> Result<Reader<'a>, DecodeError> {
        let start = self.position();
        let data = self.take(len, "sub_reader")?;
        Ok(Reader::with_base(data, start))
    }

    /// Reads four bytes and checks they equal `expected`.
    ///
    /// On a mismatch the reader does not advance and the error is
    /// [`DecodeErrorKind::BadMagic`]. Like every primitive, the error's context is the
    /// method name; callers name the tag with
    /// [`DecodeContext::context`](crate::error::DecodeContext::context).
    pub(crate) fn expect_magic(&mut self, expected: &[u8; 4]) -> Result<(), DecodeError> {
        const CONTEXT: &str = "expect_magic";
        let Some(found) = self.rest().first_chunk::<4>() else {
            return Err(self.eof(4, CONTEXT));
        };
        if found != expected {
            return Err(DecodeError::new(
                self.position(),
                CONTEXT,
                DecodeErrorKind::BadMagic {
                    expected: *expected,
                    found: *found,
                },
            ));
        }
        self.advance(4);
        Ok(())
    }

    fn take(&mut self, n: usize, context: &'static str) -> Result<&'a [u8], DecodeError> {
        let Some(bytes) = self.rest().get(..n) else {
            return Err(self.eof(n, context));
        };
        self.advance(n);
        Ok(bytes)
    }

    fn array_in<const N: usize>(&mut self, ctx: &'static str) -> Result<[u8; N], DecodeError> {
        let Some(chunk) = self.rest().first_chunk::<N>() else {
            return Err(self.eof(N, ctx));
        };
        self.advance(N);
        Ok(*chunk)
    }

    fn rest(&self) -> &'a [u8] {
        self.data.get(self.pos..).unwrap_or_default()
    }

    /// Advances by `n` bytes that the caller has already checked are available.
    fn advance(&mut self, n: usize) {
        // Equivalent to `pos + n <= len`, written so the check itself cannot overflow.
        debug_assert!(n <= self.remaining(), "advance past end of input");
        self.pos = self.pos.saturating_add(n).min(self.data.len());
    }

    fn absolute(&self, pos: usize) -> u64 {
        let pos = u64::try_from(pos).unwrap_or(u64::MAX);
        self.base.saturating_add(pos)
    }

    fn eof(&self, needed: usize, context: &'static str) -> DecodeError {
        DecodeError::new(
            self.position(),
            context,
            DecodeErrorKind::UnexpectedEof {
                needed,
                available: self.remaining(),
            },
        )
    }
}

/// Source of unique writer ids, so a [`Patch`] can be tied to the [`Writer`] that made it.
static NEXT_WRITER_ID: AtomicU64 = AtomicU64::new(0);

/// Appends typed values to a growing buffer.
///
/// `Writer` must never derive or implement `Clone`: a clone would share the writer id, so a
/// [`Patch`] reserved in one copy could silently fill the other.
#[derive(Debug)]
pub(crate) struct Writer {
    buf: Vec<u8>,
    id: u64,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    /// Creates an empty writer.
    pub(crate) fn new() -> Self {
        Self {
            buf: Vec::new(),
            id: NEXT_WRITER_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Offset of the next byte to be written.
    pub(crate) fn position(&self) -> usize {
        self.buf.len()
    }

    /// Writes one byte.
    pub(crate) fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    /// Writes a big-endian `u16`.
    pub(crate) fn u16_be(&mut self, value: u16) {
        self.bytes(&value.to_be_bytes());
    }

    /// Writes a little-endian `u16`.
    pub(crate) fn u16_le(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    /// Writes a big-endian `u32`.
    pub(crate) fn u32_be(&mut self, value: u32) {
        self.bytes(&value.to_be_bytes());
    }

    /// Writes a little-endian `u32`.
    pub(crate) fn u32_le(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    /// Writes a big-endian `u64`.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    pub(crate) fn u64_be(&mut self, value: u64) {
        self.bytes(&value.to_be_bytes());
    }

    /// Writes a little-endian `u64`.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    pub(crate) fn u64_le(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    /// Writes raw bytes.
    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Writes `n` zero bytes.
    pub(crate) fn zeros(&mut self, n: usize) {
        self.buf.extend(std::iter::repeat_n(0, n));
    }

    /// Reserves a big-endian `u16` slot, written as zeros until patched.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    pub(crate) fn reserve_u16_be(&mut self) -> Patch<u16> {
        self.reserve(Endian::Big)
    }

    /// Reserves a little-endian `u16` slot, written as zeros until patched.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    pub(crate) fn reserve_u16_le(&mut self) -> Patch<u16> {
        self.reserve(Endian::Little)
    }

    /// Reserves a big-endian `u32` slot, written as zeros until patched.
    pub(crate) fn reserve_u32_be(&mut self) -> Patch<u32> {
        self.reserve(Endian::Big)
    }

    /// Reserves a little-endian `u32` slot, written as zeros until patched.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    pub(crate) fn reserve_u32_le(&mut self) -> Patch<u32> {
        self.reserve(Endian::Little)
    }

    /// Fills a slot reserved earlier with `value`, in the byte order chosen at reservation.
    ///
    /// The token is consumed, so a slot cannot be patched twice.
    ///
    /// # Panics
    ///
    /// Panics if `token` was reserved by a different writer. That is a programming error,
    /// never a consequence of input data.
    pub(crate) fn patch<T: PatchValue>(&mut self, token: Patch<T>, value: T) {
        assert_eq!(
            token.writer, self.id,
            "Patch token used with a different Writer than the one that reserved it"
        );
        let encoded = value.encode(token.endian);
        let encoded = encoded.as_ref();
        // The slot was written by `reserve` on this writer, and a writer never shrinks.
        let end = token.offset.saturating_add(encoded.len());
        let slot = self
            .buf
            .get_mut(token.offset..end)
            .expect("reserved slot is within the buffer");
        slot.copy_from_slice(encoded);
    }

    /// Fills a reserved length slot with the number of bytes written since offset `start`.
    ///
    /// Fails with [`EncodeError::LengthOverflow`] naming `context` when that length does not
    /// fit the slot's type, or when `start` is past the current position. The slot is left
    /// zero on failure.
    ///
    /// # Panics
    ///
    /// Panics if `token` was reserved by a different writer, like [`Writer::patch`].
    pub(crate) fn patch_len_since<T>(
        &mut self,
        token: Patch<T>,
        start: usize,
        context: &'static str,
    ) -> Result<(), EncodeError>
    where
        T: PatchValue + TryFrom<usize>,
    {
        let len = self
            .position()
            .checked_sub(start)
            .and_then(|len| T::try_from(len).ok())
            .ok_or(EncodeError::LengthOverflow { context })?;
        self.patch(token, len);
        Ok(())
    }

    /// Returns the written bytes.
    pub(crate) fn into_inner(self) -> Vec<u8> {
        self.buf
    }

    fn reserve<T: PatchValue>(&mut self, endian: Endian) -> Patch<T> {
        let offset = self.position();
        self.zeros(T::WIDTH);
        Patch {
            writer: self.id,
            offset,
            endian,
            _value: PhantomData,
        }
    }
}

/// Byte order of a reserved slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Endian {
    Big,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    Little,
}

/// A value that can fill a reserved [`Patch`] slot.
pub(crate) trait PatchValue: Copy {
    /// Encoded width in bytes.
    const WIDTH: usize;
    /// The encoded bytes.
    type Bytes: AsRef<[u8]>;
    /// Encodes the value in the given byte order.
    fn encode(self, endian: Endian) -> Self::Bytes;
}

impl PatchValue for u16 {
    const WIDTH: usize = 2;
    type Bytes = [u8; 2];
    fn encode(self, endian: Endian) -> [u8; 2] {
        match endian {
            Endian::Big => self.to_be_bytes(),
            Endian::Little => self.to_le_bytes(),
        }
    }
}

impl PatchValue for u32 {
    const WIDTH: usize = 4;
    type Bytes = [u8; 4];
    fn encode(self, endian: Endian) -> [u8; 4] {
        match endian {
            Endian::Big => self.to_be_bytes(),
            Endian::Little => self.to_le_bytes(),
        }
    }
}

/// A reserved slot in a [`Writer`], to be filled with [`Writer::patch`].
///
/// The token is neither `Clone` nor `Copy`, so it can be used at most once, and it remembers
/// which writer made it, so patching another writer panics.
#[derive(Debug)]
#[must_use = "a reserved slot stays zero unless it is filled with Writer::patch"]
pub(crate) struct Patch<T: PatchValue> {
    writer: u64,
    offset: usize,
    endian: Endian,
    _value: PhantomData<T>,
}

impl<T: PatchValue> Patch<T> {
    /// Offset of the reserved slot in its writer.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the pdb codec in a later slice")
    )]
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::*;
    use crate::error::DecodeContext;

    fn eof(offset: u64, needed: usize, available: usize) -> (u64, DecodeErrorKind) {
        (offset, DecodeErrorKind::UnexpectedEof { needed, available })
    }

    fn located(err: DecodeError) -> (u64, DecodeErrorKind) {
        (err.offset, err.kind)
    }

    #[test]
    fn every_width_and_endianness_round_trips() {
        let mut w = Writer::new();
        w.u8(0xab);
        w.u16_be(0x0102);
        w.u16_le(0x0102);
        w.u32_be(0x0102_0304);
        w.u32_le(0x0102_0304);
        w.u64_be(0x0102_0304_0506_0708);
        w.u64_le(0x0102_0304_0506_0708);
        let buf = w.into_inner();

        let mut r = Reader::new(&buf);
        assert_eq!(r.u8(), Ok(0xab));
        assert_eq!(r.u16_be(), Ok(0x0102));
        assert_eq!(r.u16_le(), Ok(0x0102));
        assert_eq!(r.u32_be(), Ok(0x0102_0304));
        assert_eq!(r.u32_le(), Ok(0x0102_0304));
        assert_eq!(r.u64_be(), Ok(0x0102_0304_0506_0708));
        assert_eq!(r.u64_le(), Ok(0x0102_0304_0506_0708));
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn byte_order_is_laid_out_as_named() {
        let mut w = Writer::new();
        w.u16_be(0x0102);
        w.u16_le(0x0102);
        w.u32_be(0x0102_0304);
        w.u32_le(0x0102_0304);
        w.u64_be(0x0102_0304_0506_0708);
        w.u64_le(0x0102_0304_0506_0708);
        let buf = w.into_inner();
        assert_eq!(buf[..4], [1, 2, 2, 1]);
        assert_eq!(buf[4..12], [1, 2, 3, 4, 4, 3, 2, 1]);
        assert_eq!(buf[12..20], [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(buf[20..], [8, 7, 6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn bytes_array_skip_and_remaining() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut r = Reader::new(&data);
        assert_eq!(r.bytes(2), Ok(&data[..2]));
        assert_eq!(r.array::<3>(), Ok([3, 4, 5]));
        assert_eq!(r.remaining(), 3);
        assert_eq!(r.skip(2), Ok(()));
        assert_eq!(r.position(), 7);
        assert_eq!(r.bytes(0), Ok(&[][..]));
        assert_eq!(r.u8(), Ok(8));
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn take_rest_reads_everything_left() {
        let data = [1, 2, 3, 4];
        let mut r = Reader::with_base(&data, 0x10);
        r.skip(1).unwrap();
        assert_eq!(r.take_rest(), [2, 3, 4]);
        assert_eq!(r.position(), 0x14);
        assert_eq!(r.take_rest(), [0u8; 0]);
    }

    #[test]
    fn read_at_eof_reports_absolute_offset_and_does_not_advance() {
        let data = [0u8; 3];
        let mut r = Reader::new(&data);
        r.u8().unwrap();
        let err = r.u32_be().unwrap_err();
        assert_eq!(located(err.clone()), eof(1, 4, 2));
        assert_eq!(err.context, "u32_be");
        assert_eq!(r.position(), 1);
        assert_eq!(located(r.bytes(3).unwrap_err()), eof(1, 3, 2));
        let err = r.skip(usize::MAX).unwrap_err();
        assert_eq!(located(err), eof(1, usize::MAX, 2));
        assert_eq!(located(r.array::<8>().unwrap_err()), eof(1, 8, 2));
        assert_eq!(r.u16_le(), Ok(0));
        assert_eq!(located(r.u8().unwrap_err()), eof(3, 1, 0));
    }

    #[test]
    fn reader_with_base_reports_offsets_from_base() {
        let data = [0u8; 2];
        let mut r = Reader::with_base(&data, 0x1000);
        assert_eq!(r.position(), 0x1000);
        assert_eq!(located(r.u64_le().unwrap_err()), eof(0x1000, 8, 2));
    }

    #[test]
    fn sub_reader_keeps_absolute_offsets() {
        let data: Vec<u8> = (0..16).collect();
        let mut r = Reader::new(&data);
        r.skip(4).unwrap();
        let mut sub = r.sub_reader(8).unwrap();
        assert_eq!(r.position(), 12);
        assert_eq!(sub.position(), 4);
        assert_eq!(sub.remaining(), 8);
        sub.skip(6).unwrap();
        let err = sub.u32_le().unwrap_err();
        assert_eq!(located(err), eof(10, 4, 2));

        let mut nested = sub.sub_reader(2).unwrap();
        assert_eq!(nested.position(), 10);
        assert_eq!(nested.u16_be(), Ok(0x0a0b));
        assert_eq!(located(nested.u8().unwrap_err()), eof(12, 1, 0));
    }

    #[test]
    fn sub_reader_past_end_fails_without_advancing() {
        let data = [0u8; 4];
        let mut r = Reader::new(&data);
        r.skip(1).unwrap();
        assert_eq!(located(r.sub_reader(4).unwrap_err()), eof(1, 4, 3));
        assert_eq!(r.position(), 1);
    }

    #[test]
    fn seek_moves_within_bounds_and_rejects_outside() {
        let data: Vec<u8> = (0..8).collect();
        let mut r = Reader::new(&data);
        r.skip(2).unwrap();
        let mut sub = r.sub_reader(4).unwrap();

        sub.seek(4).unwrap();
        assert_eq!(sub.u8(), Ok(4));
        sub.seek(6).unwrap();
        assert_eq!(sub.remaining(), 0);

        for bad in [0, 1, 7, u64::MAX] {
            let err = sub.seek(bad).unwrap_err();
            assert_eq!(located(err), (bad, DecodeErrorKind::InvalidValue));
            assert_eq!(sub.position(), 6);
        }
    }

    #[test]
    fn expect_magic_accepts_match() {
        let mut r = Reader::new(b"PMAIrest");
        assert_eq!(r.expect_magic(b"PMAI"), Ok(()));
        assert_eq!(r.position(), 4);
    }

    #[test]
    fn expect_magic_fails_with_bad_magic() {
        let data = b"xxPQTZ";
        let mut r = Reader::with_base(data, 0x20);
        r.skip(2).unwrap();
        let err = r.expect_magic(b"PCOB").unwrap_err();
        assert_eq!(err.offset, 0x22);
        assert_eq!(err.context, "expect_magic");
        assert_eq!(
            err.kind,
            DecodeErrorKind::BadMagic {
                expected: *b"PCOB",
                found: *b"PQTZ",
            }
        );
        assert_eq!(r.position(), 0x22);
    }

    #[test]
    fn expect_magic_at_eof_is_unexpected_eof() {
        let mut r = Reader::new(b"PM");
        let err = r.expect_magic(b"PMAI").unwrap_err();
        assert_eq!(located(err), eof(0, 4, 2));
    }

    #[test]
    fn context_names_the_field_and_keeps_the_offset() {
        let data = [0u8; 6];
        let mut r = Reader::with_base(&data, 0x100);
        r.skip(4).unwrap();
        let err = r.u32_be().context("PQTZ entry count").unwrap_err();
        assert_eq!(err.context, "PQTZ entry count");
        assert_eq!(located(err), eof(0x104, 4, 2));

        let err = r
            .expect_magic(b"PQTZ")
            .context("beat grid tag")
            .unwrap_err();
        assert_eq!(err.context, "beat grid tag");
        assert_eq!(located(err), eof(0x104, 4, 2));
    }

    #[test]
    fn patch_fills_reserved_slot() {
        let mut w = Writer::new();
        w.u8(0xee);
        let len_be = w.reserve_u32_be();
        let len_le = w.reserve_u32_le();
        let short_be = w.reserve_u16_be();
        let short_le = w.reserve_u16_le();
        assert_eq!(len_be.offset(), 1);
        assert_eq!(short_le.offset(), 11);
        w.u8(0xff);
        assert_eq!(w.position(), 14);

        w.patch(len_be, 0x0102_0304);
        w.patch(len_le, 0x0102_0304);
        w.patch(short_be, 0x0506);
        w.patch(short_le, 0x0506);
        let buf = w.into_inner();
        assert_eq!(buf, [0xee, 1, 2, 3, 4, 4, 3, 2, 1, 5, 6, 6, 5, 0xff]);
    }

    #[test]
    fn reserved_slot_is_zero_until_patched() {
        let mut w = Writer::new();
        let _slot = w.reserve_u32_le();
        w.zeros(2);
        w.bytes(&[9]);
        assert_eq!(w.into_inner(), [0, 0, 0, 0, 0, 0, 9]);
    }

    #[test]
    #[should_panic(expected = "different Writer")]
    fn patch_with_another_writer_panics() {
        let mut a = Writer::new();
        let mut b = Writer::default();
        b.zeros(8);
        let slot = a.reserve_u32_be();
        b.patch(slot, 1);
    }

    #[test]
    fn patch_len_since_writes_bytes_written_since_start() {
        let mut w = Writer::new();
        w.zeros(3);
        let start = w.position();
        w.bytes(b"TAG!");
        let len = w.reserve_u32_be();
        w.zeros(5);
        assert_eq!(w.patch_len_since(len, start, "tag len"), Ok(()));
        assert_eq!(
            w.into_inner(),
            [0, 0, 0, b'T', b'A', b'G', b'!', 0, 0, 0, 13, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn patch_len_since_reports_overflow_and_leaves_slot_zero() {
        let mut w = Writer::new();
        let len = w.reserve_u16_be();
        w.zeros(usize::from(u16::MAX) - 1);
        let err = w.patch_len_since(len, 0, "big tag").unwrap_err();
        assert_eq!(err, EncodeError::LengthOverflow { context: "big tag" });
        let buf = w.into_inner();
        assert_eq!(buf.len(), usize::from(u16::MAX) + 1);
        assert_eq!(buf[..2], [0, 0]);
    }

    #[test]
    fn patch_len_since_fits_the_largest_value() {
        let mut w = Writer::new();
        let len = w.reserve_u16_le();
        w.zeros(usize::from(u16::MAX) - 2);
        assert_eq!(w.patch_len_since(len, 0, "big tag"), Ok(()));
        assert_eq!(w.into_inner()[..2], [0xff, 0xff]);
    }

    #[test]
    fn patch_len_since_rejects_start_past_position() {
        let mut w = Writer::new();
        let len = w.reserve_u32_be();
        let err = w.patch_len_since(len, 10, "tag len").unwrap_err();
        assert_eq!(err, EncodeError::LengthOverflow { context: "tag len" });
    }

    #[derive(Debug, Clone)]
    enum Item {
        U8(u8),
        U16Be(u16),
        U16Le(u16),
        U32Be(u32),
        U32Le(u32),
        U64Be(u64),
        U64Le(u64),
        Bytes(Vec<u8>),
        Zeros(usize),
    }

    fn item() -> impl Strategy<Value = Item> {
        prop_oneof![
            any::<u8>().prop_map(Item::U8),
            any::<u16>().prop_map(Item::U16Be),
            any::<u16>().prop_map(Item::U16Le),
            any::<u32>().prop_map(Item::U32Be),
            any::<u32>().prop_map(Item::U32Le),
            any::<u64>().prop_map(Item::U64Be),
            any::<u64>().prop_map(Item::U64Le),
            vec(any::<u8>(), 0..32).prop_map(Item::Bytes),
            (0usize..32).prop_map(Item::Zeros),
        ]
    }

    #[derive(Debug, Clone)]
    enum ReadOp {
        U8,
        U16Be,
        U16Le,
        U32Be,
        U32Le,
        U64Be,
        U64Le,
        Bytes(usize),
        Array4,
        Array16,
        Skip(usize),
        SeekAbs(u64),
        SeekFromBase(u64),
        Sub(usize),
        Magic([u8; 4]),
    }

    fn length() -> impl Strategy<Value = usize> {
        prop_oneof![0usize..80, any::<usize>()]
    }

    fn read_op() -> impl Strategy<Value = ReadOp> {
        prop_oneof![
            Just(ReadOp::U8),
            Just(ReadOp::U16Be),
            Just(ReadOp::U16Le),
            Just(ReadOp::U32Be),
            Just(ReadOp::U32Le),
            Just(ReadOp::U64Be),
            Just(ReadOp::U64Le),
            length().prop_map(ReadOp::Bytes),
            Just(ReadOp::Array4),
            Just(ReadOp::Array16),
            length().prop_map(ReadOp::Skip),
            any::<u64>().prop_map(ReadOp::SeekAbs),
            (0u64..80).prop_map(ReadOp::SeekFromBase),
            length().prop_map(ReadOp::Sub),
            any::<[u8; 4]>().prop_map(ReadOp::Magic),
        ]
    }

    /// Longest input the arbitrary-bytes proptest generates.
    const MAX_DATA: usize = 64;
    const MAX_BASE: u64 = u64::MAX - MAX_DATA as u64;

    fn base() -> impl Strategy<Value = u64> {
        // Leave room for the data, so `base + data.len()` never overflows.
        prop_oneof![Just(0), 0..=MAX_BASE, Just(MAX_BASE)]
    }

    /// Applies `op`; returns the error and the offset it is expected to report.
    fn apply(r: &mut Reader<'_>, op: &ReadOp) -> Option<(DecodeError, u64)> {
        let before = r.position();
        let result = match op {
            ReadOp::U8 => r.u8().map(|_| ()),
            ReadOp::U16Be => r.u16_be().map(|_| ()),
            ReadOp::U16Le => r.u16_le().map(|_| ()),
            ReadOp::U32Be => r.u32_be().map(|_| ()),
            ReadOp::U32Le => r.u32_le().map(|_| ()),
            ReadOp::U64Be => r.u64_be().map(|_| ()),
            ReadOp::U64Le => r.u64_le().map(|_| ()),
            ReadOp::Bytes(n) => r.bytes(*n).map(|_| ()),
            ReadOp::Array4 => r.array::<4>().map(|_| ()),
            ReadOp::Array16 => r.array::<16>().map(|_| ()),
            ReadOp::Skip(n) => r.skip(*n),
            ReadOp::SeekAbs(abs) => {
                return r.seek(*abs).err().map(|e| (e, *abs));
            }
            ReadOp::SeekFromBase(rel) => {
                let abs = r.base.saturating_add(*rel);
                return r.seek(abs).err().map(|e| (e, abs));
            }
            ReadOp::Sub(n) => r.sub_reader(*n).map(|mut sub| {
                while sub.u32_be().is_ok() {}
                let _ = sub.u8();
            }),
            ReadOp::Magic(m) => r.expect_magic(m),
        };
        result.err().map(|e| (e, before))
    }

    proptest! {
        #[test]
        fn typed_writes_read_back_in_order(items in vec(item(), 0..64)) {
            let mut w = Writer::new();
            for it in &items {
                match it {
                    Item::U8(v) => w.u8(*v),
                    Item::U16Be(v) => w.u16_be(*v),
                    Item::U16Le(v) => w.u16_le(*v),
                    Item::U32Be(v) => w.u32_be(*v),
                    Item::U32Le(v) => w.u32_le(*v),
                    Item::U64Be(v) => w.u64_be(*v),
                    Item::U64Le(v) => w.u64_le(*v),
                    Item::Bytes(v) => w.bytes(v),
                    Item::Zeros(n) => w.zeros(*n),
                }
            }
            let buf = w.into_inner();
            let mut r = Reader::new(&buf);
            for it in &items {
                match it {
                    Item::U8(v) => prop_assert_eq!(r.u8(), Ok(*v)),
                    Item::U16Be(v) => prop_assert_eq!(r.u16_be(), Ok(*v)),
                    Item::U16Le(v) => prop_assert_eq!(r.u16_le(), Ok(*v)),
                    Item::U32Be(v) => prop_assert_eq!(r.u32_be(), Ok(*v)),
                    Item::U32Le(v) => prop_assert_eq!(r.u32_le(), Ok(*v)),
                    Item::U64Be(v) => prop_assert_eq!(r.u64_be(), Ok(*v)),
                    Item::U64Le(v) => prop_assert_eq!(r.u64_le(), Ok(*v)),
                    Item::Bytes(v) => prop_assert_eq!(r.bytes(v.len()), Ok(&v[..])),
                    Item::Zeros(n) => {
                        let zeros = r.bytes(*n).unwrap();
                        prop_assert!(zeros.iter().all(|&b| b == 0));
                    }
                }
            }
            prop_assert_eq!(r.remaining(), 0);
        }

        #[test]
        fn reader_over_arbitrary_bytes_never_panics(
            data in vec(any::<u8>(), 0..=MAX_DATA),
            base in base(),
            ops in vec(read_op(), 0..32),
        ) {
            let mut r = Reader::with_base(&data, base);
            for op in &ops {
                let before = r.position();
                if let Some((err, expected_offset)) = apply(&mut r, op) {
                    prop_assert_eq!(err.offset, expected_offset);
                    prop_assert_eq!(r.position(), before, "failed read moved the reader");
                }
                let consumed = usize::try_from(r.position() - base).unwrap();
                prop_assert_eq!(consumed + r.remaining(), data.len());
            }
        }
    }
}
