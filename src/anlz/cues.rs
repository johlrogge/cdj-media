//! `PCOB`: a list of hot cues or memory points, each a nested `PCPT` entry.
//!
//! Offsets are from the start of the tag or entry; all integers are big-endian.
//!
//! `PCOB`:
//!
//! | bytes              | field          | meaning                                         |
//! |--------------------|----------------|-------------------------------------------------|
//! | 0..12              | common         | `PCOB`, `len_header` (0x18 or more), `len_tag`  |
//! | 0x0c..0x10         | `type`         | 0: memory points, 1: hot cues                   |
//! | 0x10..0x12         | `unknown1`     | not understood; usually 0                       |
//! | 0x12..0x14         | `len_cues`     | number of `PCPT` entries                        |
//! | 0x14..0x18         | `memory_count` | not understood; see [`CueList::memory`]         |
//! | 0x18..`len_header` | header tail    | not documented; kept verbatim                   |
//! | `len_header`..     | cues           | `len_cues` `PCPT` entries, then a body tail     |
//!
//! `PCPT`, framed like a tag. The documented offsets assume `len_header` is 0x1c; the body
//! fields are read from `len_header` on, after any header tail.
//!
//! | bytes              | field         | meaning                                          |
//! |--------------------|---------------|--------------------------------------------------|
//! | 0..12              | common        | `PCPT`, `len_header` (0x1c or more), `len_entry` |
//! | 0x0c..0x10         | `hot_cue`     | 0: memory point, 1: A, 2: B, 3: C                |
//! | 0x10..0x14         | `status`      | not understood; 0 or 1 in real files             |
//! | 0x14..0x18         | `unknown1`    | not understood                                   |
//! | 0x18..0x1a         | `order_first` | linked-list order; see [`CueList::memory`]       |
//! | 0x1a..0x1c         | `order_last`  | linked-list order; see [`CueList::memory`]       |
//! | 0x1c               | `type`        | 1: point, 2: loop                                |
//! | 0x1d..0x20         | `unknown2`    | not understood                                   |
//! | 0x20..0x24         | `time`        | position in milliseconds                         |
//! | 0x24..0x28         | `loop_time`   | loop end in milliseconds; for points, not used   |
//! | 0x28..0x38         | `unknown3`    | not understood; zero in real files               |
//! | 0x38..`len_entry`  | body tail     | not documented; kept verbatim                    |
//!
//! Enum-like fields (`PCOB` `type`, `PCPT` `hot_cue` and `type`) are stored as raw values
//! and interpreted by accessors, so encoding then decoding always gives back the same value.

use super::FourCc;
use super::defaults;
use super::tag::{Frame, read_frame, write_frame};
use crate::bytes::{Reader, Writer};
use crate::encoding::Millis;
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Documented `len_header` of a `PCOB` tag.
const LEN_HEADER: u32 = 0x18;

/// Code of a cue entry nested in a `PCOB` tag.
const PCPT: FourCc = FourCc::new(*b"PCPT");

/// Documented `len_header` of a `PCPT` entry.
const PCPT_LEN_HEADER: u32 = 0x1c;

/// Which cues a [`CueList`] holds: its `type` field.
///
/// `Other` values may become named variants later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CueListKind {
    /// Memory points: `type` 0.
    Memory,
    /// Hot cues: `type` 1.
    Hot,
    /// Any other `type`. [`CueListKind::from_u32`] never returns `Other(0)` or `Other(1)`;
    /// if built by hand they are stored as 0 or 1 and read back as `Memory` or `Hot`.
    Other(u32),
}

impl CueListKind {
    /// The kind for a `type` value.
    pub const fn from_u32(value: u32) -> Self {
        match value {
            0 => Self::Memory,
            1 => Self::Hot,
            other => Self::Other(other),
        }
    }

    /// The `type` value.
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::Memory => 0,
            Self::Hot => 1,
            Self::Other(other) => other,
        }
    }
}

/// The hot cue slot a [`Cue`] is assigned to.
///
/// `Other` values may become named variants later, e.g. hot cues D to H.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HotCueSlot {
    /// Hot cue A: `hot_cue` 1.
    A,
    /// Hot cue B: `hot_cue` 2.
    B,
    /// Hot cue C: `hot_cue` 3.
    C,
    /// Any other `hot_cue`. Real `.DAT` files only use A to C. [`HotCueSlot::from_u32`]
    /// never returns `Other(0..=3)`; if built by hand, `Other(0)` is stored as 0 and reads
    /// back as no slot, and `Other(1..=3)` read back as `A` to `C`.
    Other(u32),
}

impl HotCueSlot {
    /// The slot for a `hot_cue` value, or `None` for 0, which marks a memory point.
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => None,
            1 => Some(Self::A),
            2 => Some(Self::B),
            3 => Some(Self::C),
            other => Some(Self::Other(other)),
        }
    }

    /// The `hot_cue` value.
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::A => 1,
            Self::B => 2,
            Self::C => 3,
            Self::Other(other) => other,
        }
    }
}

/// Where a [`Cue`] is: a point, a loop, or a `type` this crate does not know.
///
/// `Other` values may become named variants later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CueSpan {
    /// A single position: `type` 1.
    Point(Millis),
    /// A loop from `start` to `end`: `type` 2.
    Loop {
        /// Where the loop starts: the `time` field.
        start: Millis,
        /// Where the loop ends: the `loop_time` field.
        end: Millis,
    },
    /// Any other `type`, with its raw `time` and `loop_time` fields. [`Cue::span`] never
    /// returns `type` 1 or 2 here.
    Other {
        /// The `type` byte.
        cue_type: u8,
        /// The `time` field.
        time: u32,
        /// The `loop_time` field.
        loop_time: u32,
    },
}

/// One cue: a `PCPT` entry of a [`CueList`].
///
/// The `hot_cue`, `type`, `time` and `loop_time` fields are stored as read, so every entry
/// round-trips, including unknown `type` bytes and the unused `loop_time` of points;
/// [`Cue::hot_cue`] and [`Cue::span`] interpret them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Cue {
    hot_cue: u32,
    status: u32,
    unknown1: u32,
    order_first: u16,
    order_last: u16,
    cue_type: u8,
    unknown2: [u8; 3],
    time: u32,
    loop_time: u32,
    unknown3: [u8; 16],
    header_tail: Vec<u8>,
    body_tail: Vec<u8>,
}

impl Cue {
    /// Creates a memory point or memory loop (`hot_cue` 0) with rekordbox's default unknown
    /// values. [`CueList::memory`] also sets the order links.
    pub fn memory(span: CueSpan) -> Self {
        Self::with_raw_slot(0, span)
    }

    /// Creates a hot cue in `slot` with rekordbox's default unknown values.
    pub fn hot(slot: HotCueSlot, span: CueSpan) -> Self {
        Self::with_raw_slot(slot.to_u32(), span)
    }

    fn with_raw_slot(hot_cue: u32, span: CueSpan) -> Self {
        Self {
            hot_cue,
            status: defaults::PCPT_STATUS,
            unknown1: defaults::PCPT_UNKNOWN1,
            order_first: defaults::PCPT_ORDER_FIRST,
            order_last: defaults::PCPT_ORDER_LAST,
            cue_type: 0,
            unknown2: defaults::PCPT_UNKNOWN2,
            time: 0,
            loop_time: 0,
            unknown3: defaults::PCPT_UNKNOWN3,
            header_tail: Vec::new(),
            body_tail: Vec::new(),
        }
        .with_span(span)
    }

    /// Replaces the span. A point gets rekordbox's default unused `loop_time`.
    #[must_use]
    pub fn with_span(mut self, span: CueSpan) -> Self {
        (self.cue_type, self.time, self.loop_time) = match span {
            CueSpan::Point(time) => (1, time.get(), defaults::PCPT_POINT_LOOP_TIME),
            CueSpan::Loop { start, end } => (2, start.get(), end.get()),
            CueSpan::Other {
                cue_type,
                time,
                loop_time,
            } => (cue_type, time, loop_time),
        };
        self
    }

    /// Replaces `status` (offset 0x10).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_status(mut self, status: u32) -> Self {
        self.status = status;
        self
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

    /// Replaces the linked-list order fields `order_first` and `order_last` (0x18, 0x1a).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_order(mut self, order_first: u16, order_last: u16) -> Self {
        self.order_first = order_first;
        self.order_last = order_last;
        self
    }

    /// Replaces `unknown2` (offsets 0x1d to 0x1f).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_unknown2(mut self, unknown2: [u8; 3]) -> Self {
        self.unknown2 = unknown2;
        self
    }

    /// Replaces `unknown3` (offsets 0x28 to 0x37).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_unknown3(mut self, unknown3: [u8; 16]) -> Self {
        self.unknown3 = unknown3;
        self
    }

    /// Replaces the entry header bytes after the documented header (`0x1c..len_header`).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_header_tail(mut self, header_tail: Vec<u8>) -> Self {
        self.header_tail = header_tail;
        self
    }

    /// Replaces the entry bytes after `unknown3`.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_body_tail(mut self, body_tail: Vec<u8>) -> Self {
        self.body_tail = body_tail;
        self
    }

    /// The hot cue slot, or `None` for a memory point (`hot_cue` 0).
    pub fn hot_cue(&self) -> Option<HotCueSlot> {
        HotCueSlot::from_u32(self.hot_cue)
    }

    /// The raw `hot_cue` field (offset 0x0c).
    pub fn raw_hot_cue(&self) -> u32 {
        self.hot_cue
    }

    /// Where the cue is, interpreted from the `type`, `time` and `loop_time` fields.
    pub fn span(&self) -> CueSpan {
        match self.cue_type {
            1 => CueSpan::Point(Millis::new(self.time)),
            2 => CueSpan::Loop {
                start: Millis::new(self.time),
                end: Millis::new(self.loop_time),
            },
            cue_type => CueSpan::Other {
                cue_type,
                time: self.time,
                loop_time: self.loop_time,
            },
        }
    }

    /// The raw `type` byte (offset 0x1c): 1 for a point, 2 for a loop.
    pub fn cue_type(&self) -> u8 {
        self.cue_type
    }

    /// The raw `loop_time` field (offset 0x24), also for points, where it is not used.
    pub fn raw_loop_time(&self) -> u32 {
        self.loop_time
    }

    /// `status` (offset 0x10), not understood.
    pub fn status(&self) -> u32 {
        self.status
    }

    /// `unknown1` (offset 0x14), not understood.
    pub fn unknown1(&self) -> u32 {
        self.unknown1
    }

    /// `order_first` (offset 0x18), a linked-list order field.
    pub fn order_first(&self) -> u16 {
        self.order_first
    }

    /// `order_last` (offset 0x1a), a linked-list order field.
    pub fn order_last(&self) -> u16 {
        self.order_last
    }

    /// `unknown2` (offsets 0x1d to 0x1f), not understood.
    pub fn unknown2(&self) -> [u8; 3] {
        self.unknown2
    }

    /// `unknown3` (offsets 0x28 to 0x37), not understood.
    pub fn unknown3(&self) -> [u8; 16] {
        self.unknown3
    }

    /// Entry header bytes after the documented header (`0x1c..len_header`); empty when built.
    pub fn header_tail(&self) -> &[u8] {
        &self.header_tail
    }

    /// Entry bytes after `unknown3`; empty when built.
    pub fn body_tail(&self) -> &[u8] {
        &self.body_tail
    }

    fn decode(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let frame = read_frame(r)?;
        frame.expect_fourcc(PCPT, "PCPT fourcc")?;
        frame.require_len_header(PCPT_LEN_HEADER, "PCPT len_header")?;
        let Frame {
            mut header,
            mut body,
            ..
        } = frame;
        let hot_cue = header.u32_be().context("PCPT hot_cue")?;
        let status = header.u32_be().context("PCPT status")?;
        let unknown1 = header.u32_be().context("PCPT unknown1")?;
        let order_first = header.u16_be().context("PCPT order_first")?;
        let order_last = header.u16_be().context("PCPT order_last")?;
        let header_tail = header.take_rest().to_vec();

        let cue_type = body.u8().context("PCPT type")?;
        let unknown2 = body.array().context("PCPT unknown2")?;
        let time = body.u32_be().context("PCPT time")?;
        let loop_time = body.u32_be().context("PCPT loop_time")?;
        let unknown3 = body.array().context("PCPT unknown3")?;
        let body_tail = body.take_rest().to_vec();
        Ok(Self {
            hot_cue,
            status,
            unknown1,
            order_first,
            order_last,
            cue_type,
            unknown2,
            time,
            loop_time,
            unknown3,
            header_tail,
            body_tail,
        })
    }

    fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        write_frame(
            w,
            PCPT,
            |w| {
                w.u32_be(self.hot_cue);
                w.u32_be(self.status);
                w.u32_be(self.unknown1);
                w.u16_be(self.order_first);
                w.u16_be(self.order_last);
                w.bytes(&self.header_tail);
                Ok(())
            },
            |w| {
                w.u8(self.cue_type);
                w.bytes(&self.unknown2);
                w.u32_be(self.time);
                w.u32_be(self.loop_time);
                w.bytes(&self.unknown3);
                w.bytes(&self.body_tail);
                Ok(())
            },
        )
    }
}

/// The `PCOB` tag: a list of hot cues or of memory points.
///
/// `.DAT` and `.EXT` files have two: hot cues first, then memory points. Select them with
/// [`AnlzFile::hot_cues`](super::AnlzFile::hot_cues) and
/// [`AnlzFile::memory_cues`](super::AnlzFile::memory_cues), which go by [`CueListKind`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CueList {
    list_type: u32,
    unknown1: u16,
    memory_count: u32,
    header_tail: Vec<u8>,
    cues: Vec<Cue>,
    body_tail: Vec<u8>,
}

impl CueList {
    /// Creates a list of `kind` holding `cues` exactly as given, with rekordbox's default
    /// `unknown1` and `memory_count`.
    ///
    /// Nothing is computed: the cues keep their order links, and `memory_count` is the
    /// default `0xFFFFFFFF`. Use [`CueList::memory`] to build a memory point list the way
    /// rekordbox does.
    pub fn new(kind: CueListKind, cues: Vec<Cue>) -> Self {
        Self {
            list_type: kind.to_u32(),
            unknown1: defaults::PCOB_UNKNOWN1,
            memory_count: defaults::PCOB_MEMORY_COUNT,
            header_tail: Vec::new(),
            cues,
            body_tail: Vec::new(),
        }
    }

    /// Creates a hot cue list from slots and spans, in the order given.
    ///
    /// Like rekordbox, hot cues get no order links (`0xFFFF` in both fields) and the list's
    /// `memory_count` is `0xFFFFFFFF`.
    pub fn hot(cues: impl IntoIterator<Item = (HotCueSlot, CueSpan)>) -> Self {
        let cues = cues
            .into_iter()
            .map(|(slot, span)| Cue::hot(slot, span))
            .collect();
        Self::new(CueListKind::Hot, cues)
    }

    /// Creates a memory point list from spans, computing the fields rekordbox writes for
    /// one. Every cue has `hot_cue` 0.
    ///
    /// Entries are indexed in the order the spans are given; they are not sorted. Entry `i`
    /// of `n` links to its neighbours: `order_first` is `i - 1` (`0xFFFF` for the first)
    /// and `order_last` is `i + 1` (`0xFFFF` for the last). The list's `memory_count` is
    /// `n - 1`, or `0xFFFFFFFF` when empty.
    ///
    /// A list of more than 65,535 entries cannot be encoded (`len_cues` is a `u16`, so
    /// encoding fails with [`EncodeError::LengthOverflow`]); indices past `u16::MAX` are
    /// saturated here and never written.
    pub fn memory(spans: impl IntoIterator<Item = CueSpan>) -> Self {
        let mut cues: Vec<Cue> = spans.into_iter().map(Cue::memory).collect();
        let n = cues.len();
        let index = |i: usize| u16::try_from(i).unwrap_or(u16::MAX);
        for (i, cue) in cues.iter_mut().enumerate() {
            cue.order_first = match i.checked_sub(1) {
                Some(previous) => index(previous),
                None => defaults::PCPT_ORDER_FIRST,
            };
            cue.order_last = if i + 1 == n {
                defaults::PCPT_ORDER_LAST
            } else {
                index(i + 1)
            };
        }
        let memory_count = match n.checked_sub(1) {
            Some(last) => u32::try_from(last).unwrap_or(u32::MAX),
            None => defaults::PCOB_MEMORY_COUNT,
        };
        Self {
            memory_count,
            ..Self::new(CueListKind::Memory, cues)
        }
    }

    /// Replaces `unknown1` (offset 0x10).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_unknown1(mut self, unknown1: u16) -> Self {
        self.unknown1 = unknown1;
        self
    }

    /// Replaces `memory_count` (offset 0x14).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_memory_count(mut self, memory_count: u32) -> Self {
        self.memory_count = memory_count;
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

    /// Replaces the body bytes after the last cue.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "crate-private until a caller needs it (ADR-004)")
    )]
    pub(crate) fn with_body_tail(mut self, body_tail: Vec<u8>) -> Self {
        self.body_tail = body_tail;
        self
    }

    /// Whether the list holds hot cues or memory points, interpreted from `type`.
    pub fn kind(&self) -> CueListKind {
        CueListKind::from_u32(self.list_type)
    }

    /// The raw `type` field (offset 0x0c).
    pub fn raw_type(&self) -> u32 {
        self.list_type
    }

    /// The cues, in file order.
    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }

    /// `unknown1` (offset 0x10), not understood.
    pub fn unknown1(&self) -> u16 {
        self.unknown1
    }

    /// `memory_count` (offset 0x14), kept verbatim. See [`CueList::memory`] for the values
    /// rekordbox writes.
    pub fn memory_count(&self) -> u32 {
        self.memory_count
    }

    /// Header bytes after the documented header (`0x18..len_header`); empty when built.
    pub fn header_tail(&self) -> &[u8] {
        &self.header_tail
    }

    /// Body bytes after the last cue; empty when built.
    pub fn body_tail(&self) -> &[u8] {
        &self.body_tail
    }

    pub(crate) fn decode(frame: Frame<'_>) -> Result<Self, DecodeError> {
        frame.require_len_header(LEN_HEADER, "PCOB len_header")?;
        let Frame {
            mut header,
            mut body,
            ..
        } = frame;
        let list_type = header.u32_be().context("PCOB type")?;
        let unknown1 = header.u16_be().context("PCOB unknown1")?;
        let len_cues_at = header.position();
        let len_cues = header.u16_be().context("PCOB len_cues")?;
        let memory_count = header.u32_be().context("PCOB memory_count")?;
        let header_tail = header.take_rest().to_vec();

        let mut cues = Vec::new();
        for parsed in 0..len_cues {
            if body.remaining() == 0 {
                return Err(DecodeError::new(
                    len_cues_at,
                    "PCOB len_cues",
                    DecodeErrorKind::LengthMismatch {
                        declared: u64::from(len_cues),
                        actual: u64::from(parsed),
                    },
                ));
            }
            cues.push(Cue::decode(&mut body)?);
        }
        let body_tail = body.take_rest().to_vec();
        Ok(Self {
            list_type,
            unknown1,
            memory_count,
            header_tail,
            cues,
            body_tail,
        })
    }

    pub(crate) fn encode(&self, w: &mut Writer) -> Result<(), EncodeError> {
        let len_cues = u16::try_from(self.cues.len()).map_err(|_| EncodeError::LengthOverflow {
            context: "PCOB len_cues",
        })?;
        write_frame(
            w,
            FourCc::PCOB,
            |w| {
                w.u32_be(self.list_type);
                w.u16_be(self.unknown1);
                w.u16_be(len_cues);
                w.u32_be(self.memory_count);
                w.bytes(&self.header_tail);
                Ok(())
            },
            |w| {
                for cue in &self.cues {
                    cue.encode(w)?;
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

    /// The tag starts at this offset in every test, so offsets are absolute.
    const AT: u64 = 0x40;

    fn ms(ms: u32) -> Millis {
        Millis::new(ms)
    }

    /// Fields of a hand-built PCPT entry.
    struct Entry {
        hot_cue: u32,
        status: u32,
        unknown1: u32,
        order: (u16, u16),
        cue_type: u8,
        unknown2: [u8; 3],
        time: u32,
        loop_time: u32,
    }

    impl Entry {
        /// A point entry. Deliberately not rekordbox's values: `status` 1 and order
        /// `(0, 0xFFFF)` are rare or never seen for hot cues, so the test proves they are
        /// kept. See `anlz::defaults` for the real values; do not copy these.
        fn point(hot_cue: u32, time: u32) -> Self {
            Self {
                hot_cue,
                status: 1,
                unknown1: 0x0001_0000,
                order: (0, 0xffff),
                cue_type: 1,
                unknown2: [0, 3, 0xe8],
                time,
                loop_time: 0xffff_ffff,
            }
        }

        /// Builds the entry by hand with `len_header` 0x1c and `len_entry` 0x38.
        fn bytes(&self) -> Vec<u8> {
            let mut out = b"PCPT".to_vec();
            out.extend_from_slice(&0x1cu32.to_be_bytes());
            out.extend_from_slice(&0x38u32.to_be_bytes());
            out.extend_from_slice(&self.hot_cue.to_be_bytes());
            out.extend_from_slice(&self.status.to_be_bytes());
            out.extend_from_slice(&self.unknown1.to_be_bytes());
            out.extend_from_slice(&self.order.0.to_be_bytes());
            out.extend_from_slice(&self.order.1.to_be_bytes());
            out.push(self.cue_type);
            out.extend_from_slice(&self.unknown2);
            out.extend_from_slice(&self.time.to_be_bytes());
            out.extend_from_slice(&self.loop_time.to_be_bytes());
            out.extend_from_slice(&[0; 16]);
            assert_eq!(out.len(), 0x38);
            out
        }
    }

    /// Builds PCOB bytes by hand with `len_header` 0x18 and the given body.
    fn pcob(kind: u32, len_cues: u16, memory_count: u32, body: &[u8]) -> Vec<u8> {
        let mut out = b"PCOB".to_vec();
        out.extend_from_slice(&0x18u32.to_be_bytes());
        out.extend_from_slice(&(0x18 + body.len() as u32).to_be_bytes());
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&len_cues.to_be_bytes());
        out.extend_from_slice(&memory_count.to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    fn decode(bytes: &[u8]) -> Result<CueList, DecodeError> {
        let mut padded = vec![0xee; AT as usize];
        padded.extend_from_slice(bytes);
        let mut r = Reader::new(&padded);
        r.bytes(AT as usize).unwrap();
        CueList::decode(read_frame(&mut r)?)
    }

    fn encode(list: &CueList) -> Vec<u8> {
        let mut w = Writer::new();
        list.encode(&mut w).unwrap();
        w.into_inner()
    }

    fn err(bytes: &[u8]) -> (u64, &'static str, DecodeErrorKind) {
        let e = decode(bytes).unwrap_err();
        (e.offset, e.context, e.kind)
    }

    #[test]
    fn hot_cue_b_decodes_and_round_trips() {
        let bytes = pcob(1, 1, 0xffff_ffff, &Entry::point(2, 12_345).bytes());
        let list = decode(&bytes).unwrap();
        assert_eq!(list.kind(), CueListKind::Hot);
        assert_eq!(list.raw_type(), 1);
        assert_eq!(list.memory_count(), 0xffff_ffff);
        let [cue] = list.cues() else {
            panic!("one cue expected");
        };
        assert_eq!(cue.hot_cue(), Some(HotCueSlot::B));
        assert_eq!(cue.raw_hot_cue(), 2);
        assert_eq!(cue.span(), CueSpan::Point(ms(12_345)));
        assert_eq!(cue.status(), 1);
        assert_eq!(cue.unknown1(), 0x0001_0000);
        assert_eq!((cue.order_first(), cue.order_last()), (0, 0xffff));
        assert_eq!(cue.unknown2(), [0, 3, 0xe8]);
        assert_eq!(cue.raw_loop_time(), 0xffff_ffff);
        assert_eq!(encode(&list), bytes);
    }

    #[test]
    fn memory_loop_decodes_and_round_trips() {
        let entry = Entry {
            cue_type: 2,
            time: 1_000,
            loop_time: 3_000,
            ..Entry::point(0, 0)
        };
        let bytes = pcob(0, 1, 6, &entry.bytes());
        let list = decode(&bytes).unwrap();
        assert_eq!(list.kind(), CueListKind::Memory);
        assert_eq!(list.memory_count(), 6);
        assert_eq!(list.cues()[0].hot_cue(), None);
        assert_eq!(
            list.cues()[0].span(),
            CueSpan::Loop {
                start: ms(1_000),
                end: ms(3_000),
            }
        );
        assert_eq!(encode(&list), bytes);
    }

    #[test]
    fn unknown_type_byte_and_slot_round_trip() {
        let entry = Entry {
            cue_type: 7,
            hot_cue: 9,
            time: 5,
            loop_time: 6,
            ..Entry::point(0, 0)
        };
        let bytes = pcob(5, 1, 0, &entry.bytes());
        let list = decode(&bytes).unwrap();
        assert_eq!(list.kind(), CueListKind::Other(5));
        let cue = &list.cues()[0];
        assert_eq!(cue.hot_cue(), Some(HotCueSlot::Other(9)));
        assert_eq!(cue.cue_type(), 7);
        assert_eq!(
            cue.span(),
            CueSpan::Other {
                cue_type: 7,
                time: 5,
                loop_time: 6,
            }
        );
        assert_eq!(encode(&list), bytes);
    }

    #[test]
    fn non_canonical_other_values_round_trip_equal() {
        let list = CueList::new(
            CueListKind::Other(0),
            vec![Cue::hot(HotCueSlot::Other(0), CueSpan::Point(ms(1)))],
        );
        assert_eq!(list.raw_type(), 0);
        assert_eq!(list.kind(), CueListKind::Memory);
        assert_eq!(list.cues()[0].raw_hot_cue(), 0);
        assert_eq!(list.cues()[0].hot_cue(), None);
        assert_eq!(decode(&encode(&list)).unwrap(), list);

        let list = CueList::new(
            CueListKind::Other(1),
            vec![Cue::hot(HotCueSlot::Other(1), CueSpan::Point(ms(1)))],
        );
        assert_eq!(list.kind(), CueListKind::Hot);
        assert_eq!(list.cues()[0].hot_cue(), Some(HotCueSlot::A));
        assert_eq!(decode(&encode(&list)).unwrap(), list);
    }

    #[test]
    fn tails_are_kept() {
        let mut entry = Entry::point(1, 0).bytes();
        // Grow the entry header by 2 bytes and its body by 1.
        entry[4..8].copy_from_slice(&0x1eu32.to_be_bytes());
        entry[8..12].copy_from_slice(&0x3bu32.to_be_bytes());
        entry.splice(0x1c..0x1c, [0xa1, 0xa2]);
        entry.push(0xb1);
        let body = [entry, vec![0xc1, 0xc2]].concat();
        let mut bytes = pcob(1, 1, 0xffff_ffff, &body);
        bytes[4..8].copy_from_slice(&0x19u32.to_be_bytes());
        bytes.insert(0x18, 0xd1);
        let len_tag = bytes.len() as u32;
        bytes[8..12].copy_from_slice(&len_tag.to_be_bytes());

        let list = decode(&bytes).unwrap();
        assert_eq!(list.header_tail(), [0xd1]);
        assert_eq!(list.body_tail(), [0xc1, 0xc2]);
        let cue = &list.cues()[0];
        assert_eq!(cue.header_tail(), [0xa1, 0xa2]);
        assert_eq!(cue.body_tail(), [0xb1]);
        assert_eq!(cue.span(), CueSpan::Point(ms(0)));
        assert_eq!(encode(&list), bytes);
    }

    #[test]
    fn len_cues_past_the_entries_is_mismatch() {
        let bytes = pcob(1, 3, 0xffff_ffff, &Entry::point(1, 0).bytes());
        assert_eq!(
            err(&bytes),
            (
                AT + 0x12,
                "PCOB len_cues",
                DecodeErrorKind::LengthMismatch {
                    declared: 3,
                    actual: 1,
                }
            )
        );
    }

    #[test]
    fn pcpt_with_short_len_header_is_invalid() {
        let mut entry = Entry::point(1, 0).bytes();
        entry[4..8].copy_from_slice(&0x1bu32.to_be_bytes());
        let bytes = pcob(1, 1, 0xffff_ffff, &entry);
        assert_eq!(
            err(&bytes),
            (
                AT + 0x18 + 4,
                "PCPT len_header",
                DecodeErrorKind::InvalidValue
            )
        );
    }

    #[test]
    fn entry_with_another_fourcc_is_bad_magic() {
        let mut entry = Entry::point(1, 0).bytes();
        entry[..4].copy_from_slice(b"PCP2");
        let bytes = pcob(1, 1, 0xffff_ffff, &entry);
        assert_eq!(
            err(&bytes),
            (
                AT + 0x18,
                "PCPT fourcc",
                DecodeErrorKind::BadMagic {
                    expected: *b"PCPT",
                    found: *b"PCP2",
                }
            )
        );
    }

    #[test]
    fn short_entry_body_is_unexpected_eof_at_the_field() {
        let mut entry = Entry::point(1, 0).bytes();
        entry.truncate(0x24);
        entry[8..12].copy_from_slice(&0x24u32.to_be_bytes());
        let bytes = pcob(1, 1, 0xffff_ffff, &entry);
        assert_eq!(
            err(&bytes),
            (
                AT + 0x18 + 0x24,
                "PCPT loop_time",
                DecodeErrorKind::UnexpectedEof {
                    needed: 4,
                    available: 0,
                }
            )
        );
    }

    #[test]
    fn short_len_header_is_invalid() {
        let mut bytes = pcob(1, 0, 0xffff_ffff, &[]);
        bytes[4..8].copy_from_slice(&0x17u32.to_be_bytes());
        assert_eq!(
            err(&bytes),
            (AT + 4, "PCOB len_header", DecodeErrorKind::InvalidValue)
        );
    }

    #[test]
    fn built_hot_lists_use_rekordbox_defaults() {
        let hot = CueList::hot([
            (HotCueSlot::A, CueSpan::Point(ms(42))),
            (HotCueSlot::C, CueSpan::Point(ms(7))),
        ]);
        assert_eq!(hot.kind(), CueListKind::Hot);
        assert_eq!(hot.unknown1(), defaults::PCOB_UNKNOWN1);
        assert_eq!(hot.memory_count(), 0xffff_ffff);
        for cue in hot.cues() {
            assert_eq!(cue.cue_type(), 1);
            assert_eq!(cue.raw_loop_time(), defaults::PCPT_POINT_LOOP_TIME);
            assert_eq!(cue.status(), defaults::PCPT_STATUS);
            assert_eq!(cue.unknown1(), defaults::PCPT_UNKNOWN1);
            assert_eq!((cue.order_first(), cue.order_last()), (0xffff, 0xffff));
            assert_eq!(cue.unknown2(), defaults::PCPT_UNKNOWN2);
            assert_eq!(cue.unknown3(), [0; 16]);
        }
        assert_eq!(hot.cues()[1].hot_cue(), Some(HotCueSlot::C));
        assert_eq!(decode(&encode(&hot)).unwrap(), hot);
    }

    #[test]
    fn built_memory_lists_link_their_entries_in_given_order() {
        let looped = CueSpan::Loop {
            start: ms(1_000),
            end: ms(3_000),
        };
        let memory = CueList::memory([CueSpan::Point(ms(900)), looped, CueSpan::Point(ms(100))]);
        assert_eq!(memory.kind(), CueListKind::Memory);
        assert_eq!(memory.memory_count(), 2);
        let orders: Vec<_> = memory
            .cues()
            .iter()
            .map(|c| (c.order_first(), c.order_last()))
            .collect();
        assert_eq!(orders, [(0xffff, 1), (0, 2), (1, 0xffff)]);
        assert!(memory.cues().iter().all(|c| c.hot_cue().is_none()));
        assert_eq!(memory.cues()[1].span(), looped);
        assert_eq!(memory.cues()[2].span(), CueSpan::Point(ms(100)));
        assert_eq!(decode(&encode(&memory)).unwrap(), memory);

        let single = CueList::memory([CueSpan::Point(ms(5))]);
        assert_eq!(single.memory_count(), 0);
        let cue = &single.cues()[0];
        assert_eq!((cue.order_first(), cue.order_last()), (0xffff, 0xffff));

        assert_eq!(CueList::memory([]).memory_count(), 0xffff_ffff);
    }

    #[test]
    fn new_keeps_cues_as_given() {
        let cue = Cue::memory(CueSpan::Point(ms(1))).with_order(4, 5);
        let list = CueList::new(CueListKind::Memory, vec![cue.clone(), cue.clone()]);
        assert_eq!(list.memory_count(), 0xffff_ffff);
        assert_eq!(list.cues(), [cue.clone(), cue]);
    }

    #[test]
    fn kind_and_slot_values_map_both_ways() {
        for v in [0, 1, 2, u32::MAX] {
            assert_eq!(CueListKind::from_u32(v).to_u32(), v);
        }
        assert_eq!(HotCueSlot::from_u32(0), None);
        assert_eq!(HotCueSlot::from_u32(1), Some(HotCueSlot::A));
        for v in [1, 2, 3, 4, u32::MAX] {
            assert_eq!(HotCueSlot::from_u32(v).map(HotCueSlot::to_u32), Some(v));
        }
    }
}
