//! [`Envelope`]: a caller's waveform envelope, and its mapping onto `PWV3`, `PWAV` and
//! `PWV2` (ADR-021).

use std::ops::Range;

use super::defaults;
use super::waveform::{MAX_HEIGHT, MAX_TINY_HEIGHT};
use super::{
    PREVIEW_COLUMNS, TINY_PREVIEW_COLUMNS, TinyWaveformPreview, WaveformDetail, WaveformPreview,
};
use crate::error::EncodeError;

/// A track's waveform envelope: 150 entries a second over the whole track, each a linear
/// full-band peak from 0 to 1, with optional band peaks (ADR-021).
///
/// The crate analyses no audio: the caller supplies the envelope, whose length is
/// [`WaveformDetail::entries_for`] the track's duration. Every value is within 0..=1 and
/// none is NaN, and the bands, if any, have one entry per peak: [`new`](Self::new) refuses
/// anything else.
///
/// # Mapping
///
/// The mapping onto Pioneer's bytes is provisional until it is calibrated against
/// rekordbox's waveforms of the same tracks:
///
/// - **`PWV3`** ([`WaveformDetail::from_envelope`]): one column per entry, of height
///   `round(peak · 31)`; its whiteness is `round(7 · high / (low + mid + high))` when bands
///   are given and their sum is above 0, and 5 otherwise.
/// - **`PWAV`** ([`WaveformPreview::from_envelope`]): column `j` of 400 covers the entries
///   `[j·n/400, (j+1)·n/400)`, in integer arithmetic; its height is `round(max peak · 31)`,
///   and its whiteness the rule above over the column's summed bands.
/// - **`PWV2`** ([`TinyWaveformPreview::from_envelope`]): the same windows over 100
///   columns, of height `round(max peak · 15)`, without whiteness.
/// - **Short tracks**: a column whose window is empty, when the track has fewer entries
///   than columns, takes the previous column's byte, or 0 for the first. So the first
///   columns of a track shorter than 400 entries (2.67 s) are 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    peak: Vec<f32>,
    bands: Option<Bands>,
}

/// No value of an [`Envelope`] is NaN, so its `PartialEq` is an equivalence.
impl Eq for Envelope {}

/// The band peaks of an [`Envelope`]: one linear 0..1 peak per entry in each band.
#[derive(Debug, Clone, PartialEq)]
pub struct Bands {
    /// Low band peaks.
    pub low: Vec<f32>,
    /// Mid band peaks.
    pub mid: Vec<f32>,
    /// High band peaks.
    pub high: Vec<f32>,
}

impl Envelope {
    /// An envelope of the full-band peaks `peak` and, optionally, the band peaks `bands`.
    ///
    /// # Errors
    ///
    /// [`EncodeError::ValueOutOfRange`]:
    /// - `"waveform envelope band length"` when a band's length differs from `peak`'s;
    /// - `"waveform envelope value"` when a value is outside 0..=1 or is NaN.
    pub fn new(peak: Vec<f32>, bands: Option<Bands>) -> Result<Self, EncodeError> {
        let band_values: Vec<&[f32]> = bands
            .iter()
            .flat_map(|bands| [&bands.low[..], &bands.mid[..], &bands.high[..]])
            .collect();
        if band_values.iter().any(|band| band.len() != peak.len()) {
            return Err(EncodeError::ValueOutOfRange {
                context: "waveform envelope band length",
            });
        }
        // `contains` is false for NaN.
        let valid = |values: &[f32]| values.iter().all(|v| (0.0..=1.0).contains(v));
        if !valid(&peak) || !band_values.iter().all(|band| valid(band)) {
            return Err(EncodeError::ValueOutOfRange {
                context: "waveform envelope value",
            });
        }
        Ok(Self { peak, bands })
    }

    /// The full-band peaks, one per entry.
    pub fn peak(&self) -> &[f32] {
        &self.peak
    }

    /// The band peaks, if given.
    pub fn bands(&self) -> Option<&Bands> {
        self.bands.as_ref()
    }
}

/// The whiteness of a column without band energy: 5, rekordbox's modal overview whiteness.
const NO_BANDS_WHITENESS: u8 = defaults::PWAV_FLAT_WHITENESS;

/// Largest whiteness: three bits.
const MAX_WHITENESS: u8 = 7;

/// `round(peak · max)`; `peak` is within 0..=1, so the height is within 0..=`max`.
fn height(peak: f32, max: u8) -> u8 {
    // Within 0..=max, so the cast neither truncates nor saturates.
    (peak * f32::from(max)).round() as u8
}

/// `round(7 · high / (low + mid + high))`, or [`NO_BANDS_WHITENESS`] when the sum is 0.
fn band_whiteness(low: f32, mid: f32, high: f32) -> u8 {
    let sum = low + mid + high;
    if sum > 0.0 {
        // `high <= sum`, so the share is within 0..=1.
        height(high / sum, MAX_WHITENESS)
    } else {
        NO_BANDS_WHITENESS
    }
}

/// A `PWAV` or `PWV3` column byte: whiteness in the high three bits, height in the low five.
fn column(whiteness: u8, height: u8) -> u8 {
    (whiteness << 5) | height
}

impl Envelope {
    /// The whiteness of the entries in `window`, from their summed bands.
    fn whiteness(&self, window: Range<usize>) -> u8 {
        let Some(bands) = &self.bands else {
            return NO_BANDS_WHITENESS;
        };
        let sum = |band: &[f32]| band[window.clone()].iter().sum::<f32>();
        band_whiteness(sum(&bands.low), sum(&bands.mid), sum(&bands.high))
    }

    /// The largest full-band peak in `window`, which is not empty.
    fn max_peak(&self, window: Range<usize>) -> f32 {
        self.peak[window].iter().copied().fold(0.0, f32::max)
    }

    /// `count` columns, column `j` made by `make` from the entries
    /// `[j·n/count, (j+1)·n/count)`; a column whose window is empty takes the previous
    /// column's byte, or 0 for the first.
    fn columns(&self, count: usize, make: impl Fn(Range<usize>) -> u8) -> Vec<u8> {
        let n = self.peak.len();
        let bound = |j: usize| {
            let at = j as u64 * n as u64 / count as u64;
            usize::try_from(at).expect("at most n")
        };
        let mut previous = 0;
        (0..count)
            .map(|j| {
                let window = bound(j)..bound(j + 1);
                if !window.is_empty() {
                    previous = make(window);
                }
                previous
            })
            .collect()
    }
}

impl WaveformDetail {
    /// The detail of `envelope`: one column per entry, of height `round(peak · 31)` and
    /// the band whiteness (see the [mapping](Envelope#mapping)).
    pub fn from_envelope(envelope: &Envelope) -> Self {
        let columns = (0..envelope.peak.len())
            .map(|i| {
                let whiteness = envelope.whiteness(i..i + 1);
                column(whiteness, height(envelope.peak[i], MAX_HEIGHT))
            })
            .collect();
        Self::new(columns)
    }
}

impl WaveformPreview {
    /// The preview of `envelope`: [`PREVIEW_COLUMNS`] columns, each of height
    /// `round(max peak · 31)` over its window and the whiteness of its summed bands (see the
    /// [mapping](Envelope#mapping)).
    pub fn from_envelope(envelope: &Envelope) -> Self {
        Self::new(envelope.columns(PREVIEW_COLUMNS, |window| {
            let whiteness = envelope.whiteness(window.clone());
            column(whiteness, height(envelope.max_peak(window), MAX_HEIGHT))
        }))
    }
}

impl TinyWaveformPreview {
    /// The tiny preview of `envelope`: [`TINY_PREVIEW_COLUMNS`] columns, each of height
    /// `round(max peak · 15)` over its window (see the [mapping](Envelope#mapping)).
    pub fn from_envelope(envelope: &Envelope) -> Self {
        Self::new(envelope.columns(TINY_PREVIEW_COLUMNS, |window| {
            height(envelope.max_peak(window), MAX_TINY_HEIGHT)
        }))
    }
}

#[cfg(test)]
mod tests {
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::*;

    fn value_error() -> EncodeError {
        EncodeError::ValueOutOfRange {
            context: "waveform envelope value",
        }
    }

    fn bands(low: Vec<f32>, mid: Vec<f32>, high: Vec<f32>) -> Option<Bands> {
        Some(Bands { low, mid, high })
    }

    fn peaks(peak: Vec<f32>) -> Envelope {
        Envelope::new(peak, None).expect("a valid envelope")
    }

    #[test]
    fn values_within_zero_to_one_are_accepted() {
        let envelope = Envelope::new(
            vec![0.0, 0.5, 1.0],
            bands(vec![1.0; 3], vec![0.0; 3], vec![0.25; 3]),
        )
        .unwrap();
        assert_eq!(envelope.peak(), [0.0, 0.5, 1.0]);
        assert_eq!(envelope.bands().unwrap().high, [0.25; 3]);
        assert!(Envelope::new(vec![], None).unwrap().bands().is_none());
    }

    #[test]
    fn peaks_outside_zero_to_one_or_nan_are_refused() {
        for bad in [-0.01, 1.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                Envelope::new(vec![0.5, bad], None),
                Err(value_error()),
                "{bad}"
            );
        }
    }

    #[test]
    fn band_values_outside_zero_to_one_or_nan_are_refused() {
        for bad in [-0.01, 1.01, f32::NAN] {
            let ok = || vec![0.5; 2];
            for bands in [
                bands(vec![0.5, bad], ok(), ok()),
                bands(ok(), vec![bad, 0.5], ok()),
                bands(ok(), ok(), vec![0.5, bad]),
            ] {
                assert_eq!(Envelope::new(ok(), bands), Err(value_error()), "{bad}");
            }
        }
    }

    #[test]
    fn bands_of_another_length_than_the_peaks_are_refused() {
        let error = Err(EncodeError::ValueOutOfRange {
            context: "waveform envelope band length",
        });
        let (two, three) = (|| vec![0.5; 2], || vec![0.5; 3]);
        assert_eq!(Envelope::new(two(), bands(three(), two(), two())), error);
        assert_eq!(Envelope::new(two(), bands(two(), three(), two())), error);
        assert_eq!(Envelope::new(two(), bands(two(), two(), vec![])), error);
    }

    /// Whiteness 5, rekordbox's modal overview whiteness, in the high three bits.
    const NO_BANDS: u8 = 5 << 5;

    #[test]
    fn a_silent_envelope_maps_to_height_zero() {
        let envelope = peaks(vec![0.0; 600]);
        assert_eq!(
            WaveformDetail::from_envelope(&envelope).data(),
            [NO_BANDS; 600]
        );
        assert_eq!(
            WaveformPreview::from_envelope(&envelope).data(),
            [NO_BANDS; 400]
        );
        assert_eq!(
            TinyWaveformPreview::from_envelope(&envelope).data(),
            [0; 100]
        );
    }

    #[test]
    fn a_full_envelope_maps_to_the_largest_heights() {
        let envelope = peaks(vec![1.0; 600]);
        assert_eq!(
            WaveformDetail::from_envelope(&envelope).data(),
            [NO_BANDS | 31; 600]
        );
        assert_eq!(
            WaveformPreview::from_envelope(&envelope).data(),
            [NO_BANDS | 31; 400]
        );
        assert_eq!(
            TinyWaveformPreview::from_envelope(&envelope).data(),
            [15; 100]
        );
    }

    /// `n` entries rising linearly from 0 to 1.
    fn ramp(n: u16) -> Envelope {
        let last = f32::from(n - 1);
        peaks((0..n).map(|i| f32::from(i) / last).collect())
    }

    #[test]
    fn a_ramp_maps_to_rounded_heights_of_each_windows_largest_peak() {
        // 800 entries: two a PWAV column, eight a PWV2 column.
        let envelope = ramp(800);
        let detail: Vec<u8> = WaveformDetail::from_envelope(&envelope).heights().collect();
        assert_eq!(detail.len(), 800);
        // round(31 · i / 799): 0.466 rounds down, 0.504 up.
        assert_eq!(
            [detail[0], detail[12], detail[13], detail[400], detail[799]],
            [0, 0, 1, 16, 31]
        );
        let preview: Vec<u8> = WaveformPreview::from_envelope(&envelope)
            .heights()
            .collect();
        // Column j's largest peak is entry 2j + 1's: round(31 · (2j + 1) / 799).
        assert_eq!(
            [preview[0], preview[100], preview[200], preview[399]],
            [0, 8, 16, 31]
        );
        let tiny: Vec<u8> = TinyWaveformPreview::from_envelope(&envelope)
            .heights()
            .collect();
        // Column k's largest peak is entry 8k + 7's: round(15 · (8k + 7) / 799).
        assert_eq!([tiny[0], tiny[50], tiny[99]], [0, 8, 15]);
    }

    #[test]
    fn windows_are_cut_in_integer_arithmetic() {
        // 1,000 entries, 2.5 a column: column 0 is [0, 2), column 1 [2, 5), column 2 [5, 7).
        let mut peak = vec![0.0; 1_000];
        peak[2] = 1.0;
        let preview: Vec<u8> = WaveformPreview::from_envelope(&peaks(peak.clone()))
            .heights()
            .collect();
        assert_eq!(preview[..3], [0, 31, 0]);
        peak[2] = 0.0;
        peak[5] = 1.0;
        let preview: Vec<u8> = WaveformPreview::from_envelope(&peaks(peak))
            .heights()
            .collect();
        assert_eq!(preview[..3], [0, 0, 31]);
    }

    #[test]
    fn whiteness_is_the_high_bands_share() {
        let entry = |low, mid, high| {
            let envelope = Envelope::new(vec![1.0], bands(vec![low], vec![mid], vec![high]));
            let detail = WaveformDetail::from_envelope(&envelope.unwrap());
            detail.whitenesses().next().unwrap()
        };
        assert_eq!(entry(1.0, 0.5, 0.0), 0);
        assert_eq!(entry(0.0, 0.0, 0.5), 7);
        // round(7 / 3) = 2; round(3.5) = 4.
        assert_eq!(entry(0.3, 0.3, 0.3), 2);
        assert_eq!(entry(0.2, 0.2, 0.4), 4);
        // No band energy: the no-bands whiteness.
        assert_eq!(entry(0.0, 0.0, 0.0), 5);
    }

    #[test]
    fn preview_whiteness_is_the_share_of_the_windows_summed_bands() {
        // Two entries a column. Column 0: low 1 then high 0.1, summed 0.1 / 1.1 of 7 = 0.64,
        // so 1 (averaging the entries' whitenesses, 0 and 7, would give 4). Column 1 has no
        // band energy, so 5. The rest: all high, 7.
        let mut low = vec![0.0; 800];
        let mut high = vec![1.0; 800];
        low[0] = 1.0;
        high[0] = 0.0;
        high[1] = 0.1;
        high[2] = 0.0;
        high[3] = 0.0;
        let envelope = Envelope::new(vec![0.5; 800], bands(low, vec![0.0; 800], high)).unwrap();
        let preview = WaveformPreview::from_envelope(&envelope);
        let whiteness: Vec<u8> = preview.whitenesses().collect();
        assert_eq!(whiteness[..3], [1, 5, 7]);
        assert!(preview.heights().all(|h| h == 16));
        // PWV2 has no whiteness: its high bits stay 0.
        assert_eq!(
            TinyWaveformPreview::from_envelope(&envelope).data(),
            [8; 100]
        );
    }

    #[test]
    fn a_track_shorter_than_the_columns_repeats_the_previous_column() {
        // 50 entries falling from 1 to 0. PWAV column j covers [50j/400, 50(j+1)/400):
        // columns 0 to 6 are empty, column 7 is entry 0, 8 to 14 empty, 15 entry 1, ...
        let envelope = peaks((0..50u16).map(|i| 1.0 - f32::from(i) / 49.0).collect());
        let preview = WaveformPreview::from_envelope(&envelope);
        let data = preview.data();
        assert_eq!(data.len(), 400);
        // Empty before any entry: 0, not even the no-bands whiteness.
        assert_eq!(data[..7], [0; 7]);
        assert_eq!(data[7..15], [NO_BANDS | 31; 8]);
        assert_eq!(data[399], NO_BANDS);
        // PWV2 column k covers [k/2, (k+1)/2): odd columns hold entry (k-1)/2, even ones
        // are empty and take the previous column's.
        let tiny = TinyWaveformPreview::from_envelope(&envelope);
        assert_eq!(tiny.data().len(), 100);
        assert_eq!(tiny.data()[..4], [0, 15, 15, 15]);
        assert_eq!(tiny.data()[99], 0);
        assert_eq!(WaveformDetail::from_envelope(&envelope).data().len(), 50);
    }

    #[test]
    fn a_track_shorter_than_400_but_not_100_entries() {
        // 150 entries of 1: every PWV2 window holds one or two entries; PWAV columns 0 and 1
        // are empty, column 2 is [0, 1).
        let envelope = peaks(vec![1.0; 150]);
        assert_eq!(
            TinyWaveformPreview::from_envelope(&envelope).data(),
            [15; 100]
        );
        let preview = WaveformPreview::from_envelope(&envelope);
        assert_eq!(preview.data()[..3], [0, 0, NO_BANDS | 31]);
        assert!(preview.data()[2..].iter().all(|&c| c == NO_BANDS | 31));
    }

    #[test]
    fn an_empty_envelope_maps_to_zero_columns() {
        let envelope = peaks(vec![]);
        assert!(WaveformDetail::from_envelope(&envelope).data().is_empty());
        assert_eq!(WaveformPreview::from_envelope(&envelope).data(), [0; 400]);
        assert_eq!(
            TinyWaveformPreview::from_envelope(&envelope).data(),
            [0; 100]
        );
    }

    fn envelope() -> impl Strategy<Value = Envelope> {
        (0usize..1_500, any::<bool>()).prop_flat_map(|(n, banded)| {
            let values = move || vec(0.0f32..=1.0, n);
            let bands = (values(), values(), values())
                .prop_map(move |(low, mid, high)| banded.then_some(Bands { low, mid, high }));
            (values(), bands).prop_map(|(peak, bands)| Envelope::new(peak, bands).unwrap())
        })
    }

    proptest! {
        #[test]
        fn mapped_waveforms_have_rekordboxs_columns_and_heights(envelope in envelope()) {
            let detail = WaveformDetail::from_envelope(&envelope);
            prop_assert_eq!(detail.data().len(), envelope.peak().len());
            let expected = envelope.peak().iter().map(|p| (p * 31.0).round() as u8);
            prop_assert!(detail.heights().eq(expected));
            prop_assert_eq!(WaveformPreview::from_envelope(&envelope).data().len(), 400);
            let tiny = TinyWaveformPreview::from_envelope(&envelope);
            prop_assert_eq!(tiny.data().len(), 100);
            prop_assert!(tiny.data().iter().all(|&c| c <= 15));
        }
    }
}
