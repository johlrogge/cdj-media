//! `PWAV`, `PWV2` and `PWV3`: monochrome waveforms.
//!
//! Offsets are from the start of the tag; all integers are big-endian.
//!
//! `PWAV` (preview, 400 columns, in `.DAT`) and `PWV2` (tiny preview, 100 columns, in
//! `.DAT`):
//!
//! | bytes              | field         | meaning                                         |
//! |--------------------|---------------|-------------------------------------------------|
//! | 0..12              | common        | fourcc, `len_header` (0x14 or more), `len_tag`  |
//! | 0x0c..0x10         | `len_preview` | number of columns, one byte each                |
//! | 0x10..0x14         | `unknown1`    | not understood; `0x00010000` in real files      |
//! | 0x14..`len_header` | header tail   | not documented; kept verbatim                   |
//! | `len_header`..     | columns       | `len_preview` bytes, then a body tail           |
//!
//! A `PWAV` column has its height in the low five bits (0 to 31) and its whiteness in the
//! high three; a `PWV2` column has its height in the low four bits (0 to 15). Columns are
//! stored as read, so every byte round-trips; the accessors split them.

use super::FourCc;
use super::defaults;
use super::tag::{Frame, write_frame};
use crate::bytes::Writer;
use crate::encoding::Millis;
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Documented `len_header` of a `PWAV` or `PWV2` tag.
const PREVIEW_LEN_HEADER: u32 = 0x14;

/// Documented `len_header` of a `PWV3` tag.
const DETAIL_LEN_HEADER: u32 = 0x18;

/// Number of columns in a `PWAV` preview written by rekordbox (all 848 real `.DAT` files).
pub const PREVIEW_COLUMNS: usize = 400;

/// Number of columns in a `PWV2` tiny preview written by rekordbox (all 848 real `.DAT`
/// files).
pub const TINY_PREVIEW_COLUMNS: usize = 100;

/// `PWV3` columns per second of audio: 150, half a CD frame each.
const DETAIL_PER_SECOND: u64 = 150;

/// Largest `PWAV` and `PWV3` column height: five bits.
const MAX_HEIGHT: u8 = 0x1f;

/// Largest `PWV2` column height: four bits.
const MAX_TINY_HEIGHT: u8 = 0x0f;

/// Field names of one preview tag, for error contexts.
struct PreviewNames {
    len_header: &'static str,
    len_preview: &'static str,
    unknown1: &'static str,
    columns: &'static str,
}

const PWAV_NAMES: PreviewNames = PreviewNames {
    len_header: "PWAV len_header",
    len_preview: "PWAV len_preview",
    unknown1: "PWAV unknown1",
    columns: "PWAV columns",
};

const PWV2_NAMES: PreviewNames = PreviewNames {
    len_header: "PWV2 len_header",
    len_preview: "PWV2 len_preview",
    unknown1: "PWV2 unknown1",
    columns: "PWV2 columns",
};

/// The shared layout of `PWAV` and `PWV2`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Preview {
    unknown1: u32,
    header_tail: Vec<u8>,
    columns: Vec<u8>,
    body_tail: Vec<u8>,
}

impl Preview {
    fn new(columns: Vec<u8>, unknown1: u32) -> Self {
        Self {
            unknown1,
            header_tail: Vec::new(),
            columns,
            body_tail: Vec::new(),
        }
    }

    fn decode(frame: Frame<'_>, names: &PreviewNames) -> Result<Self, DecodeError> {
        frame.require_len_header(PREVIEW_LEN_HEADER, names.len_header)?;
        let Frame {
            mut header,
            mut body,
            ..
        } = frame;
        let len_preview_at = header.position();
        let len_preview = header.u32_be().context(names.len_preview)?;
        let unknown1 = header.u32_be().context(names.unknown1)?;
        let header_tail = header.take_rest().to_vec();

        let available = body.remaining();
        let count = usize::try_from(len_preview)
            .ok()
            .filter(|&count| count <= available)
            .ok_or_else(|| {
                DecodeError::new(
                    len_preview_at,
                    names.len_preview,
                    DecodeErrorKind::LengthMismatch {
                        declared: u64::from(len_preview),
                        actual: u64::try_from(available).unwrap_or(u64::MAX),
                    },
                )
            })?;
        let columns = body.bytes(count).context(names.columns)?.to_vec();
        let body_tail = body.take_rest().to_vec();
        Ok(Self {
            unknown1,
            header_tail,
            columns,
            body_tail,
        })
    }

    fn encode(
        &self,
        w: &mut Writer,
        fourcc: FourCc,
        names: &PreviewNames,
    ) -> Result<(), EncodeError> {
        let len_preview =
            u32::try_from(self.columns.len()).map_err(|_| EncodeError::LengthOverflow {
                context: names.len_preview,
            })?;
        write_frame(
            w,
            fourcc,
            |w| {
                w.u32_be(len_preview);
                w.u32_be(self.unknown1);
                w.bytes(&self.header_tail);
                Ok(())
            },
            |w| {
                w.bytes(&self.columns);
                w.bytes(&self.body_tail);
                Ok(())
            },
        )
    }
}

/// Height of a `PWAV` or `PWV3` column: its low five bits.
fn height5(column: u8) -> u8 {
    column & MAX_HEIGHT
}

/// Whiteness of a `PWAV` or `PWV3` column: its high three bits.
fn whiteness(column: u8) -> u8 {
    column >> 5
}

/// The `PWAV` tag: the waveform preview, 400 one-byte columns in real files.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WaveformPreview(Preview);

impl WaveformPreview {
    /// Creates a preview from its column bytes, stored as given, with rekordbox's default
    /// `unknown1`. Real previews have 400 columns.
    pub fn new(columns: Vec<u8>) -> Self {
        Self(Preview::new(columns, defaults::PWAV_UNKNOWN1))
    }

    /// A placeholder for callers without waveform data: [`PREVIEW_COLUMNS`] columns, all of
    /// `height` (capped at 31) and of whiteness 5.
    ///
    /// This is what rekordbox writes for a silent track at height 2: every column `0xa2`.
    /// The whiteness is rekordbox's; the caller chooses the height.
    pub fn flat(height: u8) -> Self {
        let column = (defaults::PWAV_FLAT_WHITENESS << 5) | height.min(MAX_HEIGHT);
        Self::new(vec![column; PREVIEW_COLUMNS])
    }

    /// Replaces `unknown1` (offset 0x10).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_unknown1(mut self, unknown1: u32) -> Self {
        self.0.unknown1 = unknown1;
        self
    }

    /// Replaces the header bytes after the documented header (`0x14..len_header`).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_header_tail(mut self, header_tail: Vec<u8>) -> Self {
        self.0.header_tail = header_tail;
        self
    }

    /// Replaces the body bytes after the last column.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_body_tail(mut self, body_tail: Vec<u8>) -> Self {
        self.0.body_tail = body_tail;
        self
    }

    /// The column bytes, in file order.
    pub fn data(&self) -> &[u8] {
        &self.0.columns
    }

    /// The height of each column (0 to 31): its low five bits.
    pub fn heights(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.columns.iter().copied().map(height5)
    }

    /// The whiteness of each column (0 to 7, higher is whiter): its high three bits.
    pub fn whitenesses(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.columns.iter().copied().map(whiteness)
    }

    /// `unknown1` (offset 0x10), not understood.
    pub fn unknown1(&self) -> u32 {
        self.0.unknown1
    }

    /// Header bytes after the documented header (`0x14..len_header`); empty when built.
    pub fn header_tail(&self) -> &[u8] {
        &self.0.header_tail
    }

    /// Body bytes after the last column; empty when built.
    pub fn body_tail(&self) -> &[u8] {
        &self.0.body_tail
    }

    pub(crate) fn decode(frame: Frame<'_>) -> Result<Self, DecodeError> {
        Preview::decode(frame, &PWAV_NAMES).map(Self)
    }

    pub(crate) fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        self.0.encode(w, FourCc::PWAV, &PWAV_NAMES)
    }
}

/// The `PWV2` tag: the tiny waveform preview, 100 one-byte columns in real files.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TinyWaveformPreview(Preview);

impl TinyWaveformPreview {
    /// Creates a tiny preview from its column bytes, stored as given, with rekordbox's
    /// default `unknown1`. Real tiny previews have 100 columns.
    pub fn new(columns: Vec<u8>) -> Self {
        Self(Preview::new(columns, defaults::PWV2_UNKNOWN1))
    }

    /// A placeholder for callers without waveform data: [`TINY_PREVIEW_COLUMNS`] columns,
    /// all of `height`, capped at 15.
    ///
    /// rekordbox's tiny preview of a silent track is all height 1.
    pub fn flat(height: u8) -> Self {
        Self::new(vec![height.min(MAX_TINY_HEIGHT); TINY_PREVIEW_COLUMNS])
    }

    /// Replaces `unknown1` (offset 0x10).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_unknown1(mut self, unknown1: u32) -> Self {
        self.0.unknown1 = unknown1;
        self
    }

    /// Replaces the header bytes after the documented header (`0x14..len_header`).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_header_tail(mut self, header_tail: Vec<u8>) -> Self {
        self.0.header_tail = header_tail;
        self
    }

    /// Replaces the body bytes after the last column.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_body_tail(mut self, body_tail: Vec<u8>) -> Self {
        self.0.body_tail = body_tail;
        self
    }

    /// The column bytes, in file order.
    pub fn data(&self) -> &[u8] {
        &self.0.columns
    }

    /// The height of each column (0 to 15): its low four bits. Real files use no others.
    pub fn heights(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.columns.iter().map(|column| column & MAX_TINY_HEIGHT)
    }

    /// `unknown1` (offset 0x10), not understood.
    pub fn unknown1(&self) -> u32 {
        self.0.unknown1
    }

    /// Header bytes after the documented header (`0x14..len_header`); empty when built.
    pub fn header_tail(&self) -> &[u8] {
        &self.0.header_tail
    }

    /// Body bytes after the last column; empty when built.
    pub fn body_tail(&self) -> &[u8] {
        &self.0.body_tail
    }

    pub(crate) fn decode(frame: Frame<'_>) -> Result<Self, DecodeError> {
        Preview::decode(frame, &PWV2_NAMES).map(Self)
    }

    pub(crate) fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        self.0.encode(w, FourCc::PWV2, &PWV2_NAMES)
    }
}

/// The `PWV3` tag: the waveform detail, one one-byte column per half frame (150 per
/// second). It lives in `.EXT` files.
///
/// | bytes              | field             | meaning                                  |
/// |--------------------|-------------------|------------------------------------------|
/// | 0..12              | common            | `PWV3`, `len_header` (0x18 or more), ... |
/// | 0x0c..0x10         | `len_entry_bytes` | bytes per column; must be 1              |
/// | 0x10..0x14         | `len_entries`     | number of columns                        |
/// | 0x14..0x18         | `unknown1`        | not understood; `0x00960000` in real files |
/// | 0x18..`len_header` | header tail       | not documented; kept verbatim            |
/// | `len_header`..     | columns           | `len_entries` bytes, then a body tail    |
///
/// Columns split into height and whiteness like [`WaveformPreview`] columns.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WaveformDetail {
    unknown1: u32,
    header_tail: Vec<u8>,
    columns: Vec<u8>,
    body_tail: Vec<u8>,
}

impl WaveformDetail {
    /// Creates a detail waveform from its column bytes, stored as given, with rekordbox's
    /// default `unknown1`.
    pub fn new(columns: Vec<u8>) -> Self {
        Self {
            unknown1: defaults::PWV3_UNKNOWN1,
            header_tail: Vec::new(),
            columns,
            body_tail: Vec::new(),
        }
    }

    /// The number of columns rekordbox writes for audio `duration` long: 150 a second,
    /// rounded up, `ceil(ms · 3 / 20)` (ADR-020).
    ///
    /// rekordbox counts from the exact length, `ceil(frames · 150 / rate)`; from the length
    /// in whole milliseconds this is the same except when the exact length lies just past a
    /// multiple of 20 ms, where it gives one column fewer. It matched all 6 tracks of the
    /// rekordbox-hw1 fixture.
    pub fn entries_for(duration: Millis) -> usize {
        let entries = (u64::from(duration.get()) * DETAIL_PER_SECOND).div_ceil(1000);
        usize::try_from(entries).expect("at most 644,245,095 entries")
    }

    /// A placeholder for callers without waveform data:
    /// [`entries_for(duration)`](Self::entries_for) columns, all of `height` (capped at 31)
    /// and of whiteness 7.
    ///
    /// rekordbox writes a silent track's detail at height 0: every column `0xe0`. The
    /// whiteness is rekordbox's; the caller chooses the height.
    pub fn flat(height: u8, duration: Millis) -> Self {
        let column = (defaults::PWV3_FLAT_WHITENESS << 5) | height.min(MAX_HEIGHT);
        Self::new(vec![column; Self::entries_for(duration)])
    }

    /// Replaces `unknown1` (offset 0x14).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_unknown1(mut self, unknown1: u32) -> Self {
        self.unknown1 = unknown1;
        self
    }

    /// Replaces the header bytes after the documented header (`0x18..len_header`).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_header_tail(mut self, header_tail: Vec<u8>) -> Self {
        self.header_tail = header_tail;
        self
    }

    /// Replaces the body bytes after the last column.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_body_tail(mut self, body_tail: Vec<u8>) -> Self {
        self.body_tail = body_tail;
        self
    }

    /// The column bytes, in file order.
    pub fn data(&self) -> &[u8] {
        &self.columns
    }

    /// The height of each column (0 to 31): its low five bits.
    pub fn heights(&self) -> impl Iterator<Item = u8> + '_ {
        self.columns.iter().copied().map(height5)
    }

    /// The whiteness of each column (0 to 7, higher is whiter): its high three bits.
    pub fn whitenesses(&self) -> impl Iterator<Item = u8> + '_ {
        self.columns.iter().copied().map(whiteness)
    }

    /// `unknown1` (offset 0x14), not understood.
    pub fn unknown1(&self) -> u32 {
        self.unknown1
    }

    /// Header bytes after the documented header (`0x18..len_header`); empty when built.
    pub fn header_tail(&self) -> &[u8] {
        &self.header_tail
    }

    /// Body bytes after the last column; empty when built.
    pub fn body_tail(&self) -> &[u8] {
        &self.body_tail
    }

    pub(crate) fn decode(frame: Frame<'_>) -> Result<Self, DecodeError> {
        frame.require_len_header(DETAIL_LEN_HEADER, "PWV3 len_header")?;
        let Frame {
            mut header,
            mut body,
            ..
        } = frame;
        let entry_len_at = header.position();
        let entry_len = header.u32_be().context("PWV3 len_entry_bytes")?;
        if entry_len != 1 {
            return Err(DecodeError::new(
                entry_len_at,
                "PWV3 len_entry_bytes",
                DecodeErrorKind::InvalidValue,
            ));
        }
        let len_entries_at = header.position();
        let len_entries = header.u32_be().context("PWV3 len_entries")?;
        let unknown1 = header.u32_be().context("PWV3 unknown1")?;
        let header_tail = header.take_rest().to_vec();

        let available = body.remaining();
        let count = usize::try_from(len_entries)
            .ok()
            .filter(|&count| count <= available)
            .ok_or_else(|| {
                DecodeError::new(
                    len_entries_at,
                    "PWV3 len_entries",
                    DecodeErrorKind::LengthMismatch {
                        declared: u64::from(len_entries),
                        actual: u64::try_from(available).unwrap_or(u64::MAX),
                    },
                )
            })?;
        let columns = body.bytes(count).context("PWV3 columns")?.to_vec();
        let body_tail = body.take_rest().to_vec();
        Ok(Self {
            unknown1,
            header_tail,
            columns,
            body_tail,
        })
    }

    pub(crate) fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        let len_entries =
            u32::try_from(self.columns.len()).map_err(|_| EncodeError::LengthOverflow {
                context: "PWV3 len_entries",
            })?;
        write_frame(
            w,
            FourCc::PWV3,
            |w| {
                w.u32_be(1);
                w.u32_be(len_entries);
                w.u32_be(self.unknown1);
                w.bytes(&self.header_tail);
                Ok(())
            },
            |w| {
                w.bytes(&self.columns);
                w.bytes(&self.body_tail);
                Ok(())
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anlz::tag::read_frame;
    use crate::bytes::{Reader, Writer};
    use crate::error::{DecodeError, DecodeErrorKind};

    /// The tag starts at this offset in every test, so offsets are absolute.
    const AT: u64 = 0x40;

    /// Builds preview bytes by hand: `len_header` 0x14 plus `tail`, then `body`.
    fn preview_bytes(
        fourcc: &[u8; 4],
        len_preview: u32,
        unknown1: u32,
        tail: &[u8],
        body: &[u8],
    ) -> Vec<u8> {
        let len_header = 0x14 + tail.len() as u32;
        let mut out = fourcc.to_vec();
        out.extend_from_slice(&len_header.to_be_bytes());
        out.extend_from_slice(&(len_header + body.len() as u32).to_be_bytes());
        out.extend_from_slice(&len_preview.to_be_bytes());
        out.extend_from_slice(&unknown1.to_be_bytes());
        out.extend_from_slice(tail);
        out.extend_from_slice(body);
        out
    }

    fn frame_of(bytes: &[u8]) -> Vec<u8> {
        let mut padded = vec![0xee; AT as usize];
        padded.extend_from_slice(bytes);
        padded
    }

    fn decode_preview(bytes: &[u8]) -> Result<WaveformPreview, DecodeError> {
        let padded = frame_of(bytes);
        let mut r = Reader::new(&padded);
        r.bytes(AT as usize).unwrap();
        WaveformPreview::decode(read_frame(&mut r)?)
    }

    fn encode_with(f: impl FnOnce(&mut Writer) -> Result<(), EncodeError>) -> Vec<u8> {
        let mut w = Writer::new();
        f(&mut w).unwrap();
        w.into_inner()
    }

    #[test]
    fn preview_decodes_and_round_trips() {
        let body = [0xa2, 0x17, 0x00];
        // Deliberately not rekordbox's unknown1; see `defaults` for the real value.
        let bytes = preview_bytes(b"PWAV", 3, 0x1234_5678, &[], &body);
        let preview = decode_preview(&bytes).unwrap();
        assert_eq!(preview.data(), body);
        assert_eq!(preview.unknown1(), 0x1234_5678);
        assert_eq!(encode_with(|w| preview.encode(w)), bytes);
    }

    #[test]
    fn preview_splits_height_and_whiteness() {
        let preview = WaveformPreview::new(vec![0xa2, 0x17, 0xff]);
        assert_eq!(preview.heights().collect::<Vec<_>>(), [2, 0x17, 31]);
        assert_eq!(preview.whitenesses().collect::<Vec<_>>(), [5, 0, 7]);
    }

    #[test]
    fn preview_tails_are_kept() {
        let body = [1, 2, 0xb1, 0xb2];
        let bytes = preview_bytes(b"PWAV", 2, 0x0001_0000, &[0xd1], &body);
        let preview = decode_preview(&bytes).unwrap();
        assert_eq!(preview.data(), [1, 2]);
        assert_eq!(preview.header_tail(), [0xd1]);
        assert_eq!(preview.body_tail(), [0xb1, 0xb2]);
        assert_eq!(encode_with(|w| preview.encode(w)), bytes);
    }

    #[test]
    fn preview_len_past_the_body_is_mismatch() {
        let bytes = preview_bytes(b"PWAV", 5, 0x0001_0000, &[], &[1, 2, 3]);
        let err = decode_preview(&bytes).unwrap_err();
        assert_eq!(
            (err.offset, err.context, err.kind),
            (
                AT + 0x0c,
                "PWAV len_preview",
                DecodeErrorKind::LengthMismatch {
                    declared: 5,
                    actual: 3,
                }
            )
        );
    }

    #[test]
    fn flat_preview_is_rekordbox_silence_at_the_callers_height() {
        let flat = WaveformPreview::flat(2);
        assert_eq!(flat.data(), [0xa2; 400]);
        assert_eq!(flat.unknown1(), defaults::PWAV_UNKNOWN1);
        assert!(flat.heights().all(|h| h == 2));
        assert!(flat.whitenesses().all(|w| w == 5));
        assert!(WaveformPreview::flat(200).heights().all(|h| h == 31));
    }

    fn decode_tiny(bytes: &[u8]) -> Result<TinyWaveformPreview, DecodeError> {
        let padded = frame_of(bytes);
        let mut r = Reader::new(&padded);
        r.bytes(AT as usize).unwrap();
        TinyWaveformPreview::decode(read_frame(&mut r)?)
    }

    #[test]
    fn tiny_preview_decodes_and_round_trips() {
        let body = [0x0f, 0x01, 0xf3, 9];
        let bytes = preview_bytes(b"PWV2", 3, 0x0001_0000, &[0xd1], &body);
        let tiny = decode_tiny(&bytes).unwrap();
        assert_eq!(tiny.data(), &body[..3]);
        assert_eq!(tiny.heights().collect::<Vec<_>>(), [15, 1, 3]);
        assert_eq!(tiny.unknown1(), 0x0001_0000);
        assert_eq!(tiny.header_tail(), [0xd1]);
        assert_eq!(tiny.body_tail(), [9]);
        assert_eq!(encode_with(|w| tiny.encode(w)), bytes);
    }

    #[test]
    fn tiny_preview_errors_name_pwv2() {
        let bytes = preview_bytes(b"PWV2", 2, 0, &[], &[1]);
        let err = decode_tiny(&bytes).unwrap_err();
        assert_eq!((err.offset, err.context), (AT + 0x0c, "PWV2 len_preview"));
    }

    #[test]
    fn flat_tiny_preview_has_100_columns_at_the_callers_height() {
        let flat = TinyWaveformPreview::flat(1);
        assert_eq!(flat.data(), [1; 100]);
        assert_eq!(flat.unknown1(), defaults::PWV2_UNKNOWN1);
        assert_eq!(TinyWaveformPreview::flat(16).data(), [15; 100]);
        assert_eq!(TinyWaveformPreview::new(vec![3]).data(), [3]);
    }

    /// Builds PWV3 bytes by hand: `len_header` 0x18, then `body`.
    fn detail_bytes(entry_len: u32, len_entries: u32, unknown1: u32, body: &[u8]) -> Vec<u8> {
        let mut out = b"PWV3".to_vec();
        out.extend_from_slice(&0x18u32.to_be_bytes());
        out.extend_from_slice(&(0x18 + body.len() as u32).to_be_bytes());
        out.extend_from_slice(&entry_len.to_be_bytes());
        out.extend_from_slice(&len_entries.to_be_bytes());
        out.extend_from_slice(&unknown1.to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    fn decode_detail(bytes: &[u8]) -> Result<WaveformDetail, DecodeError> {
        let padded = frame_of(bytes);
        let mut r = Reader::new(&padded);
        r.bytes(AT as usize).unwrap();
        WaveformDetail::decode(read_frame(&mut r)?)
    }

    #[test]
    fn detail_decodes_and_round_trips() {
        let body = [0xff, 0x00, 0x45, 0xbb];
        let bytes = detail_bytes(1, 3, 0x0096_0000, &body);
        let detail = decode_detail(&bytes).unwrap();
        assert_eq!(detail.data(), &body[..3]);
        assert_eq!(detail.body_tail(), [0xbb]);
        assert_eq!(detail.heights().collect::<Vec<_>>(), [31, 0, 5]);
        assert_eq!(detail.whitenesses().collect::<Vec<_>>(), [7, 0, 2]);
        assert_eq!(detail.unknown1(), 0x0096_0000);
        assert_eq!(encode_with(|w| detail.encode(w)), bytes);
        assert_eq!(
            WaveformDetail::new(body[..3].to_vec()),
            WaveformDetail {
                body_tail: vec![],
                ..detail
            }
        );
    }

    /// rekordbox's PWV3 length: ceil(ms · 150 / 1000), measured on the 6 tracks of
    /// rekordbox-hw1 against their exact lengths (the first two values are two of them).
    #[test]
    fn detail_entries_are_150_a_second_rounded_up() {
        let entries = |ms| WaveformDetail::entries_for(Millis::new(ms));
        assert_eq!(entries(393_375), 59_007);
        assert_eq!(entries(456_000), 68_400);
        assert_eq!(entries(0), 0);
        assert_eq!(entries(1), 1);
        assert_eq!(entries(20), 3);
        assert_eq!(entries(21), 4);
        assert_eq!(entries(u32::MAX), 644_245_095);
    }

    #[test]
    fn flat_detail_is_rekordbox_silence_at_the_callers_height() {
        let flat = WaveformDetail::flat(0, Millis::new(1_000));
        assert_eq!(flat.data(), [0xe0; 150]);
        assert_eq!(flat.unknown1(), defaults::PWV3_UNKNOWN1);
        let low = WaveformDetail::flat(2, Millis::new(7));
        assert_eq!(low.data(), [0xe2; 2]);
        assert!(
            WaveformDetail::flat(200, Millis::new(10))
                .heights()
                .all(|h| h == 31)
        );
        assert!(WaveformDetail::flat(5, Millis::new(0)).data().is_empty());
    }

    #[test]
    fn detail_entry_length_other_than_one_is_invalid() {
        let bytes = detail_bytes(2, 1, 0x0096_0000, &[0, 0]);
        let err = decode_detail(&bytes).unwrap_err();
        assert_eq!(
            (err.offset, err.context, err.kind),
            (
                AT + 0x0c,
                "PWV3 len_entry_bytes",
                DecodeErrorKind::InvalidValue
            )
        );
    }

    #[test]
    fn detail_entries_past_the_body_is_mismatch() {
        let bytes = detail_bytes(1, 4, 0x0096_0000, &[0, 0]);
        let err = decode_detail(&bytes).unwrap_err();
        assert_eq!(
            (err.offset, err.context, err.kind),
            (
                AT + 0x10,
                "PWV3 len_entries",
                DecodeErrorKind::LengthMismatch {
                    declared: 4,
                    actual: 2,
                }
            )
        );
    }

    #[test]
    fn detail_short_len_header_is_invalid() {
        let mut bytes = detail_bytes(1, 0, 0, &[]);
        bytes[4..8].copy_from_slice(&0x17u32.to_be_bytes());
        bytes[8..12].copy_from_slice(&0x17u32.to_be_bytes());
        bytes.pop();
        let err = decode_detail(&bytes).unwrap_err();
        assert_eq!(
            (err.offset, err.context, err.kind),
            (AT + 4, "PWV3 len_header", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn preview_short_len_header_is_invalid() {
        let mut bytes = preview_bytes(b"PWAV", 0, 0, &[], &[]);
        bytes[4..8].copy_from_slice(&0x13u32.to_be_bytes());
        let err = decode_preview(&bytes).unwrap_err();
        assert_eq!(
            (err.offset, err.context, err.kind),
            (AT + 4, "PWAV len_header", DecodeErrorKind::InvalidValue)
        );
    }
}
