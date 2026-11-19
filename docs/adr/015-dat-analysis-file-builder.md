# ADR-015: Building `.DAT` analysis files
## Status
Accepted
## Decision
`AnlzFile::track_dat` builds a complete `.DAT` from typed values: the audio path tag, a
`BeatGrid`, a hot-cue list, a memory-cue list, a `WaveformPreview` (PWAV) and a
`TinyWaveformPreview` (PWV2).

- **Tag order** is fixed to rekordbox's: `PPTH PVBR PQTZ PWAV PWV2 PCOB(hot) PCOB(memory)`,
  as in all 848 `.DAT` files on the fixture.
- **PMAI header:** always the modal value (793 of 848 files). Variants on the fixture
  correlate with stale analyses and MP3s, but with 8 AIFFs too, so no rule is derived.
- **PVBR** (MP3 seek index) is written as a zero 4-byte header and a zero 1,604-byte body.
  That is what rekordbox writes for AIFF and WAV. MP3 files would need a real index, which
  needs audio analysis and is out of scope, so Phase 1 builds for AIFF/WAV.
- **Cue lists** come from the existing `CueList` builders, which compute rekordbox's order
  links and `memory_count`. The builder is strict: a hot list that isn't of kind Hot, or a
  memory list that isn't Memory, is an error.
- **Waveform placeholder** for callers without waveform data:
  `WaveformPreview::flat(height)` and `TinyWaveformPreview::flat(height)`. The caller picks
  the height, capped at the field width (31, 15). PWAV whiteness is fixed at 5, the value
  rekordbox writes for the silent track on the fixture. PWV2 is a separate input because
  no simple rule derives it from PWAV.
- **PWV3** is typed (decode and encode) but `.EXT` is not built yet. A minimal `.EXT` also
  needs PCO2, PWV4 and PWV5, none of them typed, and whether the NXS needs `.EXT` at all is
  a hardware question (slice 7h).
## Why
Rebuilding every real `.DAT` from its typed contents with `track_dat` gives 793 of 848
files back byte for byte. The other 55 differ only in the PMAI header (30) or in the PMAI
header and PVBR (25 MP3s). See `docs/format-notes.md`.
## Consequences
- MP3 export needs a caller-supplied PVBR or a decision to transcode, which MDMA already does
  (`--format aiff`).
- The placeholder height and whiteness are checked on hardware in slice 7h.
