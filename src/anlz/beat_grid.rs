//! `PQTZ`: the beat grid.
//!
//! Offsets are from the start of the tag; all integers are big-endian.
//!
//! | bytes              | field        | meaning                                           |
//! |--------------------|--------------|---------------------------------------------------|
//! | 0..12              | common       | `PQTZ`, `len_header` (0x18 or more), `len_tag`    |
//! | 0x0c..0x10         | `unknown1`   | not understood; 0 in real files                   |
//! | 0x10..0x14         | `unknown2`   | not understood; `0x00080000` in real files        |
//! | 0x14..0x18         | `len_beats`  | number of beats                                   |
//! | 0x18..`len_header` | header tail  | not documented; kept verbatim                     |
//! | `len_header`..     | beats        | `len_beats` entries of 8 bytes, then a body tail  |
//!
//! Each beat is `beat_number` (u16, 1 to 4), `tempo` (u16, BPM × 100) and `time` (u32,
//! milliseconds). Bytes after the last beat are kept verbatim as the body tail; real files
//! have none.

use super::FourCc;
use super::defaults;
use super::tag::{Frame, write_frame};
use crate::bytes::{Reader, Writer};
use crate::encoding::{BeatInBar, Bpm, Millis};
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Documented `len_header` of a `PQTZ` tag.
const LEN_HEADER: u32 = 0x18;

/// Length of one beat entry.
const BEAT_LEN: usize = 8;

/// One beat of a [`BeatGrid`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Beat {
    bar_position: BeatInBar,
    tempo: Bpm,
    time: Millis,
}

impl Beat {
    /// Creates a beat at `time`, at `bar_position` in its bar, with the tempo from here on.
    ///
    /// The beat grid stores tempos as `u16`, so a tempo above 655.35 BPM makes encoding
    /// fail with [`EncodeError::ValueOutOfRange`].
    pub const fn new(bar_position: BeatInBar, tempo: Bpm, time: Millis) -> Self {
        Self {
            bar_position,
            tempo,
            time,
        }
    }

    /// The beat's position in its bar.
    pub const fn bar_position(&self) -> BeatInBar {
        self.bar_position
    }

    /// The tempo at this beat.
    pub const fn tempo(&self) -> Bpm {
        self.tempo
    }

    /// The time of the beat from the start of the track.
    pub const fn time(&self) -> Millis {
        self.time
    }

    fn decode(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let number_at = r.position();
        let number = r.u16_be().context("PQTZ beat_number")?;
        let bar_position = BeatInBar::new(number).ok_or_else(|| {
            DecodeError::new(number_at, "PQTZ beat_number", DecodeErrorKind::InvalidValue)
        })?;
        let tempo = Bpm::from_centi(u32::from(r.u16_be().context("PQTZ tempo")?));
        let time = Millis::new(r.u32_be().context("PQTZ time")?);
        Ok(Self::new(bar_position, tempo, time))
    }

    fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        let tempo =
            u16::try_from(self.tempo.centi()).map_err(|_| EncodeError::ValueOutOfRange {
                context: "PQTZ tempo",
            })?;
        w.u16_be(self.bar_position.number());
        w.u16_be(tempo);
        w.u32_be(self.time.get());
        Ok(())
    }
}

/// A constant-tempo beat grid as a caller knows it: one downbeat, a tempo and the track's
/// length, with four beats to the bar. [`BeatGrid::constant_tempo`] expands it into beats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ConstantGrid {
    first_beat: Millis,
    tempo: Bpm,
    duration: Millis,
}

impl ConstantGrid {
    /// A grid with a downbeat at `first_beat`, `tempo` throughout, for a track `duration`
    /// long; `None` when `tempo` is 0, which has no beat interval.
    pub const fn new(first_beat: Millis, tempo: Bpm, duration: Millis) -> Option<Self> {
        if tempo.centi() == 0 {
            return None;
        }
        Some(Self {
            first_beat,
            tempo,
            duration,
        })
    }

    /// The downbeat the grid is anchored at.
    pub const fn first_beat(&self) -> Millis {
        self.first_beat
    }

    /// The tempo.
    pub const fn tempo(&self) -> Bpm {
        self.tempo
    }

    /// The length of the track.
    pub const fn duration(&self) -> Millis {
        self.duration
    }
}

/// The `PQTZ` tag: the beat grid of a track.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BeatGrid {
    unknown1: u32,
    unknown2: u32,
    header_tail: Vec<u8>,
    beats: Vec<Beat>,
    body_tail: Vec<u8>,
}

impl BeatGrid {
    /// Creates a beat grid with rekordbox's default unknown values and no tails.
    pub fn new(beats: Vec<Beat>) -> Self {
        Self {
            unknown1: defaults::PQTZ_UNKNOWN1,
            unknown2: defaults::PQTZ_UNKNOWN2,
            header_tail: Vec::new(),
            beats,
            body_tail: Vec::new(),
        }
    }

    /// Expands a constant-tempo grid into its beats, with rekordbox's default unknown values.
    ///
    /// Beat `k` from the first beat is at `first_beat + round(k · 60000 / BPM)` ms, rounded
    /// half up, each beat on its own so rounding never drifts. The grid runs from the
    /// earliest such beat at or after 0 to the last one before `duration`; bar positions
    /// count 1 to 4 with the first beat at 1, backwards too. Every grid on the real-stick
    /// fixture has one tempo and starts within one beat of 0.
    ///
    /// rekordbox works from a phase finer than the stored whole milliseconds, so it can
    /// place any beat 1 ms either way of this one, and some of its grids use a tempo finer
    /// than the stored hundredths of a BPM. See `docs/format-notes.md`.
    pub fn constant_tempo(grid: &ConstantGrid) -> Self {
        let centi = i64::from(grid.tempo.centi());
        let first = i64::from(grid.first_beat.get());
        let end = i64::from(grid.duration.get());
        // Beat k from the first: first + round(k · 60000 / BPM), half up, in whole numbers.
        let time_of = |k: i64| first + (k * 12_000_000 + centi).div_euclid(2 * centi);
        let mut start = 0;
        while time_of(start - 1) >= 0 {
            start -= 1;
        }
        let mut beats = Vec::new();
        for k in start.. {
            let time = time_of(k);
            if time >= end {
                break;
            }
            let bar_position = match k.rem_euclid(4) {
                0 => BeatInBar::One,
                1 => BeatInBar::Two,
                2 => BeatInBar::Three,
                _ => BeatInBar::Four,
            };
            // In range: 0 <= time < end <= u32::MAX.
            let time = Millis::new(u32::try_from(time).unwrap_or(u32::MAX));
            beats.push(Beat::new(bar_position, grid.tempo, time));
        }
        Self::new(beats)
    }

    /// Replaces `unknown1` (offset 0x0c).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_unknown1(mut self, unknown1: u32) -> Self {
        self.unknown1 = unknown1;
        self
    }

    /// Replaces `unknown2` (offset 0x10).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_unknown2(mut self, unknown2: u32) -> Self {
        self.unknown2 = unknown2;
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

    /// Replaces the body bytes after the last beat.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_body_tail(mut self, body_tail: Vec<u8>) -> Self {
        self.body_tail = body_tail;
        self
    }

    /// The beats, in file order.
    pub fn beats(&self) -> &[Beat] {
        &self.beats
    }

    /// `unknown1` (offset 0x0c), not understood.
    pub fn unknown1(&self) -> u32 {
        self.unknown1
    }

    /// `unknown2` (offset 0x10), not understood.
    pub fn unknown2(&self) -> u32 {
        self.unknown2
    }

    /// Header bytes after the documented header (`0x18..len_header`); empty when built.
    pub fn header_tail(&self) -> &[u8] {
        &self.header_tail
    }

    /// Body bytes after the last beat; empty when built.
    pub fn body_tail(&self) -> &[u8] {
        &self.body_tail
    }

    pub(crate) fn decode(frame: Frame<'_>) -> Result<Self, DecodeError> {
        frame.require_len_header(LEN_HEADER, "PQTZ len_header")?;
        let Frame {
            mut header,
            mut body,
            ..
        } = frame;
        let unknown1 = header.u32_be().context("PQTZ unknown1")?;
        let unknown2 = header.u32_be().context("PQTZ unknown2")?;
        let len_beats_at = header.position();
        let len_beats = header.u32_be().context("PQTZ len_beats")?;
        let header_tail = header.take_rest().to_vec();

        let available = body.remaining() / BEAT_LEN;
        let count = usize::try_from(len_beats)
            .ok()
            .filter(|&count| count <= available)
            .ok_or_else(|| {
                DecodeError::new(
                    len_beats_at,
                    "PQTZ len_beats",
                    DecodeErrorKind::LengthMismatch {
                        declared: u64::from(len_beats),
                        actual: u64::try_from(available).unwrap_or(u64::MAX),
                    },
                )
            })?;
        let beats = (0..count)
            .map(|_| Beat::decode(&mut body))
            .collect::<Result<Vec<_>, _>>()?;
        let body_tail = body.take_rest().to_vec();
        Ok(Self {
            unknown1,
            unknown2,
            header_tail,
            beats,
            body_tail,
        })
    }

    pub(crate) fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        let len_beats =
            u32::try_from(self.beats.len()).map_err(|_| EncodeError::LengthOverflow {
                context: "PQTZ len_beats",
            })?;
        write_frame(
            w,
            FourCc::PQTZ,
            |w| {
                w.u32_be(self.unknown1);
                w.u32_be(self.unknown2);
                w.u32_be(len_beats);
                w.bytes(&self.header_tail);
                Ok(())
            },
            |w| {
                for beat in &self.beats {
                    beat.encode(w)?;
                }
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

    /// The tag starts at this offset in every test, so offsets are absolute.
    const AT: u64 = 0x40;

    /// Builds PQTZ bytes by hand: the documented header plus `tail`, then `body`.
    fn pqtz(unknown2: u32, len_beats: u32, tail: &[u8], body: &[u8]) -> Vec<u8> {
        let len_header = 0x18 + tail.len() as u32;
        let mut out = b"PQTZ".to_vec();
        out.extend_from_slice(&len_header.to_be_bytes());
        out.extend_from_slice(&(len_header + body.len() as u32).to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&unknown2.to_be_bytes());
        out.extend_from_slice(&len_beats.to_be_bytes());
        out.extend_from_slice(tail);
        out.extend_from_slice(body);
        out
    }

    fn beat_bytes(number: u16, centi_bpm: u16, ms: u32) -> Vec<u8> {
        [
            &number.to_be_bytes()[..],
            &centi_bpm.to_be_bytes(),
            &ms.to_be_bytes(),
        ]
        .concat()
    }

    fn decode(bytes: &[u8]) -> Result<BeatGrid, DecodeError> {
        let mut padded = vec![0xee; AT as usize];
        padded.extend_from_slice(bytes);
        let mut r = Reader::new(&padded);
        r.bytes(AT as usize).unwrap();
        BeatGrid::decode(read_frame(&mut r)?)
    }

    fn encode(grid: &BeatGrid) -> Vec<u8> {
        let mut w = Writer::new();
        grid.encode(&mut w).unwrap();
        w.into_inner()
    }

    fn err(bytes: &[u8]) -> (u64, &'static str, DecodeErrorKind) {
        let e = decode(bytes).unwrap_err();
        (e.offset, e.context, e.kind)
    }

    #[test]
    fn beats_decode_and_round_trip() {
        let body = [beat_bytes(1, 12_000, 500), beat_bytes(2, 12_000, 1_000)].concat();
        let bytes = pqtz(0x0008_0000, 2, &[], &body);
        let grid = decode(&bytes).unwrap();
        let expected = BeatGrid::new(vec![
            Beat::new(BeatInBar::One, Bpm::from_centi(12_000), Millis::new(500)),
            Beat::new(BeatInBar::Two, Bpm::from_centi(12_000), Millis::new(1_000)),
        ]);
        assert_eq!(grid, expected);
        let first = grid.beats()[0];
        assert_eq!(first.bar_position(), BeatInBar::One);
        assert_eq!(first.tempo().to_string(), "120.00");
        assert_eq!(first.time(), Millis::new(500));
        assert_eq!(grid.unknown1(), 0);
        assert_eq!(grid.unknown2(), 0x0008_0000);
        assert_eq!(encode(&grid), bytes);
    }

    #[test]
    fn unknowns_and_tails_are_kept() {
        let body = [beat_bytes(4, 9_000, 7), vec![0xaa, 0xbb]].concat();
        // Deliberately not rekordbox's unknown2; see `defaults` for the real value.
        let bytes = pqtz(0x1234_5678, 1, &[9, 8], &body);
        let grid = decode(&bytes).unwrap();
        assert_eq!(grid.unknown2(), 0x1234_5678);
        assert_eq!(grid.header_tail(), [9, 8]);
        assert_eq!(grid.body_tail(), [0xaa, 0xbb]);
        assert_eq!(encode(&grid), bytes);
    }

    #[test]
    fn tempo_above_u16_is_out_of_range_on_encode() {
        let fits = Beat::new(BeatInBar::One, Bpm::from_centi(65_535), Millis::new(0));
        let mut w = Writer::new();
        assert_eq!(BeatGrid::new(vec![fits]).encode(&mut w), Ok(()));

        let too_fast = Beat::new(BeatInBar::One, Bpm::from_centi(65_536), Millis::new(0));
        let mut w = Writer::new();
        assert_eq!(
            BeatGrid::new(vec![fits, too_fast]).encode(&mut w),
            Err(EncodeError::ValueOutOfRange {
                context: "PQTZ tempo"
            })
        );
    }

    #[test]
    fn beat_number_outside_one_to_four_is_invalid() {
        for bad in [0, 5] {
            let body = [beat_bytes(1, 12_000, 0), beat_bytes(bad, 12_000, 500)].concat();
            assert_eq!(
                err(&pqtz(0x0008_0000, 2, &[], &body)),
                (
                    AT + 0x18 + 8,
                    "PQTZ beat_number",
                    DecodeErrorKind::InvalidValue
                )
            );
        }
    }

    #[test]
    fn len_beats_past_the_body_is_mismatch() {
        let body = [beat_bytes(1, 12_000, 0), vec![0; 7]].concat();
        assert_eq!(
            err(&pqtz(0x0008_0000, 2, &[], &body)),
            (
                AT + 0x14,
                "PQTZ len_beats",
                DecodeErrorKind::LengthMismatch {
                    declared: 2,
                    actual: 1,
                }
            )
        );
        assert_eq!(
            err(&pqtz(0x0008_0000, u32::MAX, &[], &[])),
            (
                AT + 0x14,
                "PQTZ len_beats",
                DecodeErrorKind::LengthMismatch {
                    declared: u64::from(u32::MAX),
                    actual: 0,
                }
            )
        );
    }

    #[test]
    fn short_len_header_is_invalid() {
        let mut bytes = b"PQTZ".to_vec();
        bytes.extend_from_slice(&0x14u32.to_be_bytes());
        bytes.extend_from_slice(&0x14u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 8]);
        assert_eq!(
            err(&bytes),
            (AT + 4, "PQTZ len_header", DecodeErrorKind::InvalidValue)
        );
    }

    /// `(beat_number, time)` of each beat, all at `centi` BPM × 100.
    fn shape(grid: &BeatGrid, centi: u32) -> Vec<(u16, u32)> {
        assert!(grid.beats().iter().all(|b| b.tempo().centi() == centi));
        grid.beats()
            .iter()
            .map(|b| (b.bar_position().number(), b.time().get()))
            .collect()
    }

    fn constant(first_beat: u32, centi: u32, duration: u32) -> BeatGrid {
        let grid = ConstantGrid::new(
            Millis::new(first_beat),
            Bpm::from_centi(centi),
            Millis::new(duration),
        )
        .expect("non-zero tempo");
        BeatGrid::constant_tempo(&grid)
    }

    #[test]
    fn constant_tempo_counts_bars_from_the_first_beat_until_the_end() {
        // 120 BPM: a beat every 500 ms. The beat at the end itself is past the track.
        let grid = constant(0, 12_000, 2_500);
        assert_eq!(
            shape(&grid, 12_000),
            [(1, 0), (2, 500), (3, 1_000), (4, 1_500), (1, 2_000)]
        );
        assert_eq!(grid, BeatGrid::new(grid.beats().to_vec()));
    }

    #[test]
    fn constant_tempo_rounds_each_beat_from_the_first_half_up() {
        // 128 BPM: 468.75 ms. Each beat is rounded on its own, so errors don't add up.
        let grid = constant(100, 12_800, 1_900);
        assert_eq!(
            shape(&grid, 12_800),
            [(1, 100), (2, 569), (3, 1_038), (4, 1_506)]
        );
        // 132 BPM: 454.54… ms. Beats 11 and 1,100 fall on whole ms, with no drift.
        let grid = constant(0, 13_200, 500_001);
        let times: Vec<u32> = grid.beats().iter().map(|b| b.time().get()).collect();
        assert_eq!((times[1], times[11], times[1_100]), (455, 5_000, 500_000));
    }

    #[test]
    fn constant_tempo_extends_back_to_the_earliest_beat_at_or_after_zero() {
        let grid = constant(1_100, 12_000, 2_000);
        assert_eq!(
            shape(&grid, 12_000),
            [(3, 100), (4, 600), (1, 1_100), (2, 1_600)]
        );
        // A beat at exactly 0 is kept, and a whole bar back wraps the numbering.
        let grid = constant(2_500, 12_000, 3_000);
        assert_eq!(
            shape(&grid, 12_000),
            [
                (4, 0),
                (1, 500),
                (2, 1_000),
                (3, 1_500),
                (4, 2_000),
                (1, 2_500)
            ]
        );
        // 128 BPM back from 1,000 ms: −468.75 rounds to −469 and −937.5 to −937.
        let grid = constant(1_000, 12_800, 1_400);
        assert_eq!(shape(&grid, 12_800), [(3, 63), (4, 531), (1, 1_000)]);
    }

    #[test]
    fn constant_grid_needs_a_tempo() {
        let zero = ConstantGrid::new(Millis::new(0), Bpm::from_centi(0), Millis::new(1_000));
        assert_eq!(zero, None);
        let grid = ConstantGrid::new(Millis::new(7), Bpm::from_centi(1), Millis::new(9)).unwrap();
        assert_eq!(
            (grid.first_beat(), grid.tempo(), grid.duration()),
            (Millis::new(7), Bpm::from_centi(1), Millis::new(9))
        );
    }

    #[test]
    fn new_uses_rekordbox_defaults() {
        let grid = BeatGrid::new(vec![]);
        assert_eq!(grid.unknown1(), defaults::PQTZ_UNKNOWN1);
        assert_eq!(grid.unknown2(), 0x0008_0000);
        assert!(grid.header_tail().is_empty() && grid.body_tail().is_empty());
        let grid = grid
            .with_unknown1(1)
            .with_header_tail(vec![1])
            .with_body_tail(vec![2]);
        assert_eq!(grid.unknown1(), 1);
        assert_eq!((grid.header_tail(), grid.body_tail()), (&[1][..], &[2][..]));
    }
}
