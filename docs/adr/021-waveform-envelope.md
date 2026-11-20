# ADR-021: Waveform envelope input

## Status

Accepted. The value mapping is provisional until it is calibrated (see Consequences).
Amends ADR-018 and ADR-020, which deferred the envelope.

## Context

Since run B5 every waveform we write is a flat placeholder: PWAV and PWV2 in the `.DAT`,
PWV3 in the `.EXT`. MDMA will serve waveform data (its ADR-036) but doesn't yet. The user
decided cdj-media keeps its boundary and does no audio analysis, so the library takes the
envelope as input and waits for MDMA to provide it.

What we asked MDMA to serve (2026-11-20):
- 150 entries per second over the whole track;
- per entry, a linear 0..1 full-band `peak` and optional `low`/`mid`/`high` band peaks;
- `ceil(frames·150/rate)` entries, which is exactly rekordbox's PWV3 count (ADR-020).

## Decision

### The input

`Waveform::Envelope(Envelope)` joins `Flat` and `Previews`, where:

    Envelope { peak: Vec<f32>, bands: Option<Bands> }
    Bands { low: Vec<f32>, mid: Vec<f32>, high: Vec<f32> }

The rate is fixed at 150 per second, so it isn't a field.

Both are refused with `ValueOutOfRange`, at different points:
- `Envelope::new` refuses band lengths that differ from `peak`'s, and any value outside
  0..=1 or NaN. The fields are private, so an invalid envelope can't exist.
- `add_track` refuses an envelope whose length isn't the track's PWV3 count,
  `ceil(ms·3/20)`, before anything changes.

### Mapping (provisional)

- **PWV3** (150/s, one entry per entry):
  - height = `round(peak · 31)`;
  - whiteness = `round(7 · high / (low + mid + high))` when bands are given and their sum is
    above 0, otherwise 5 (rekordbox's modal overview whiteness).
- **PWAV** (400 columns): column j covers entries `[j·n/400, (j+1)·n/400)` in integer
  arithmetic. Its height is `round(max peak · 31)`. Its whiteness uses the band rule over
  the column's summed bands, or 5 without bands or when the bands sum to 0.
- **PWV2** (100 columns): the same windows over 100 columns; height = `round(max peak · 15)`.
  It has no whiteness bits.
- **Short tracks:** if a track has fewer entries than columns, empty windows take the
  previous column's value. Leading empty windows are byte 0, which affects only tracks
  under about 2.7 s.

The mapping lives in `anlz` as pure functions from `Envelope` to the three waveform types,
so the mapping can be tested without the builder.

## Why

- **Linear peaks:** linear 0..1 peaks keep MDMA free of Pioneer specifics. cdj-media owns the
  Pioneer encoding, and can recalibrate without changing MDMA.
- **One input:** a single 150/s envelope covers all three tags, and later PWV5, PWV4 and the
  CDJ-3000's 3-band data.

## Alternatives considered

- **Analysing audio in the library or in an example:** declined by the user. It stays MDMA's
  job.
- **Accepting other rates and resampling:** that adds a resampling policy for no current
  caller.

## Consequences

- **Calibration:** once MDMA serves envelopes for the hw1 tracks, compare the mapped
  PWAV/PWV2/PWV3 with rekordbox's for the same tracks (local fixtures). Then fit the height
  curve (rekordbox may not be linear) and the whiteness rule. Until then the mapping is a
  documented guess.
- **Rounding:** `f32::round`, so halves round away from zero.
- **The MDMA example** gains an envelope source once the endpoint exists. Until then it keeps
  `Flat`.
