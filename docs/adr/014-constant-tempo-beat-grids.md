# ADR-014: Constant-tempo beat grids
## Status
Accepted
## Decision
`BeatGrid::constant_tempo(&ConstantGrid)` builds a PQTZ grid from what callers such as
MDMA know: a downbeat in whole ms, a tempo in hundredths of a BPM (not 0) and the track
length in ms, with four beats to the bar.

- One tempo for every beat, as in all 824 non-empty grids on the real-stick fixture.
- Beat `k` from the downbeat is at `first_beat + round(k · 60000 / BPM)` ms, ties up, each
  beat rounded on its own in integer arithmetic, so rounding never drifts.
- The grid extends backwards to the earliest beat at or after 0 (every rekordbox grid
  starts less than one interval from 0) and ends at the last beat before the length.
- Bar positions count 1 to 4 with the downbeat at 1, backwards too (…, 3, 4, 1, 2, …).
- Unknown PQTZ fields take the `BeatGrid::new` defaults.

The caller's length is used as given. Padding a whole-second duration (MDMA, pdb) is a
policy for the export builder (slice 7f), not for this function.
## Why
Measured on the fixture (`docs/format-notes.md`, `tests/beat_grid_local.rs`). Among fixed
sub-ms offsets from the stored first beat, rounding matched the most grids (147 of 483
that use the stored tempo exactly, against 127 for floor).
## Accepted inaccuracy
rekordbox anchors its beats on a sub-ms phase the stick doesn't store, and in about 40%
of grids on a tempo finer than centi-BPM or on several phase segments. No rule built from
(whole-ms first beat, centi-BPM, length) recovers that, so built grids equal rekordbox's
for 121 of 824 fixture grids. For grids at the stored tempo they are never more than
1 ms off. A whole-second length can end the grid up to 4 beats early.
## Alternatives considered
- **Floor instead of round:** indistinguishable from round with a shifted phase on the
  fixture, and matched fewer grids at the stored first beat.
- **Accept a finer first beat or tempo:** might recover rekordbox's phase, but MDMA
  supplies neither. Revisit if a caller does, and settle floor against round by comparing
  MDMA and rekordbox grids of the same tracks.
## Consequences
- The local rebuild test pins the 121 exact matches to the current fixture. A re-exported
  fixture means re-measuring.
- Open: whether MDMA's `first_beat_ms` is always a downbeat (beat 1). The slice 7g
  comparison checks it.
