//! Helpers shared by the row types: the row id reader, and the name part of artist and
//! album rows with its near and far layouts.

use crate::bytes::{Reader, Writer};
use crate::encoding::devicesql;
use crate::encoding::{DeviceSqlString, StringForm};
use crate::error::{DecodeContext, DecodeError, DecodeErrorKind, EncodeError};

/// Length of the near marker and name offset fields: a byte each.
const NEAR_FIELDS_LEN: usize = 2;
/// Length of the far marker and name offset fields: a `u16` each.
const FAR_FIELDS_LEN: usize = 4;

/// Reads a row's own id: a non-zero `u32`. 0 is [`DecodeErrorKind::InvalidValue`] at the
/// field, as a row id of 0 cannot be represented.
pub(super) fn read_id<T>(
    r: &mut Reader<'_>,
    context: &'static str,
    new: fn(u32) -> Option<T>,
) -> Result<T, DecodeError> {
    let at = r.position();
    let value = r.u32_le().context(context)?;
    new(value).ok_or_else(|| DecodeError::new(at, context, DecodeErrorKind::InvalidValue))
}

/// Which layout an artist or album row's marker and name offset use, chosen by its
/// `subtype` (ADR-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Layout {
    /// A byte each.
    Near,
    /// A `u16` each.
    Far,
}

impl Layout {
    /// Length of the marker and name offset fields.
    fn fields_len(self) -> usize {
        match self {
            Self::Near => NEAR_FIELDS_LEN,
            Self::Far => FAR_FIELDS_LEN,
        }
    }
}

/// The part of an artist or album row from its marker field on: the marker, the name
/// offset (not stored; see [`NameTail::encode`]), the gap bytes and the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NameTail {
    marker: u16,
    gap: Vec<u8>,
    name: DeviceSqlString,
}

/// Error contexts of a [`NameTail`]'s fields, per row type.
pub(super) struct TailContexts {
    pub(super) marker: &'static str,
    pub(super) offset: &'static str,
    pub(super) gap: &'static str,
}

impl NameTail {
    /// Rekordbox's near tail for a row whose marker is at row offset `marker_at`: the name
    /// in rekordbox's form, right after the fixed part when short ASCII, otherwise at the
    /// next multiple of 4 after zero gap bytes. Rows start at multiples of 4 in the heap,
    /// so this is the heap alignment the string layout rule asks for (ADR-005).
    pub(super) fn built(marker: u16, marker_at: usize, name: &str) -> Self {
        let name = DeviceSqlString::new(name);
        let fixed = marker_at + NEAR_FIELDS_LEN;
        let at = if name.form() == StringForm::ShortAscii {
            fixed
        } else {
            fixed.next_multiple_of(4)
        };
        Self {
            marker,
            gap: vec![0; at - fixed],
            name,
        }
    }

    pub(super) fn marker(&self) -> u16 {
        self.marker
    }

    pub(super) fn gap(&self) -> &[u8] {
        &self.gap
    }

    pub(super) fn name(&self) -> &DeviceSqlString {
        &self.name
    }

    /// The length the tail would have in the far layout, without a gap: the marker and name
    /// offset fields and the name.
    pub(super) fn far_len(&self) -> usize {
        FAR_FIELDS_LEN.saturating_add(self.name.encoded_len())
    }

    pub(super) fn with_marker(self, marker: u16) -> Self {
        Self { marker, ..self }
    }

    pub(super) fn with_gap(self, gap: Vec<u8>) -> Self {
        Self { gap, ..self }
    }

    /// Decodes the tail from `r`, positioned at the marker of a row that starts at
    /// absolute offset `row_at`.
    pub(super) fn decode(
        r: &mut Reader<'_>,
        row_at: u64,
        layout: Layout,
        ctx: &TailContexts,
    ) -> Result<Self, DecodeError> {
        let field = |r: &mut Reader<'_>, context: &'static str| match layout {
            Layout::Far => r.u16_le().context(context),
            Layout::Near => r.u8().map(u16::from).context(context),
        };
        let marker = field(r, ctx.marker)?;
        let offset_at = r.position();
        let offset = field(r, ctx.offset)?;
        let fixed = r.position().saturating_sub(row_at);
        let gap_len = u64::from(offset)
            .checked_sub(fixed)
            .and_then(|len| usize::try_from(len).ok())
            .ok_or_else(|| {
                DecodeError::new(offset_at, ctx.offset, DecodeErrorKind::InvalidValue)
            })?;
        let gap = r.bytes(gap_len).context(ctx.gap)?.to_vec();
        let name = devicesql::decode(r)?;
        Ok(Self { marker, gap, name })
    }

    /// Writes the tail into `w`, which holds the row so far: the name offset is the
    /// fixed part's length plus the gap's. Writes nothing on failure.
    pub(super) fn encode(
        &self,
        w: &mut Writer,
        layout: Layout,
        ctx: &TailContexts,
    ) -> Result<(), EncodeError> {
        let offset = w.position() + layout.fields_len() + self.gap.len();
        let overflow = EncodeError::LengthOverflow {
            context: ctx.offset,
        };
        match layout {
            Layout::Far => {
                let offset = u16::try_from(offset).map_err(|_| overflow)?;
                w.u16_le(self.marker);
                w.u16_le(offset);
            }
            Layout::Near => {
                let marker =
                    u8::try_from(self.marker).map_err(|_| EncodeError::ValueOutOfRange {
                        context: ctx.marker,
                    })?;
                let offset = u8::try_from(offset).map_err(|_| overflow)?;
                w.u8(marker);
                w.u8(offset);
            }
        }
        w.bytes(&self.gap);
        self.name.encode(w)
    }
}
