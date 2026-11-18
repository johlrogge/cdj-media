# ADR-008: Track builder policy for unknown and per-row unique fields
## Status
Accepted
## Decision
Extends ADR-007 for track rows.
- **`index_shift`** follows the same slot rule for tracks as for artists and albums
  (0x20 × slot, counting absent rows; all 827 real rows). Builders write 0, and the page
  writer owns the value.
- **`unknown2` keeps the property rekordbox shows: every row has a different value.**
  Its meaning is unknown (an opaque library id or a hash). Builders write the track id,
  which is deterministic, non-zero and unique. This is not a modal default and doesn't
  claim to be rekordbox's value. The setter stays crate-private, and no public field
  exposes it, so the policy can change once its meaning is known.
- **`bitmask` and the unknown string slots 2, 3 and 4 use their modal defaults** until a
  correlation with known data is found. Then they are derived in `Track::new` or promoted
  to named fields, as key `id2` was.
- **Layers:** `Track` and `TrackFields` are the row layer, with raw ids. The `Database`
  model (slice 5c) allocates ids and deduplicates names. `ExportBuilder` (slice 7) is the
  caller's domain API and owns derived values: filename, analysis path, dates and
  `unknown2`.
## Why
A value class rekordbox never produces (every row equal) is a more likely trap for the
firmware than any particular unique value.
## Alternatives considered
- **Write 0:** every built row equal, a pattern that never occurs in real files.
- **Compute a hash:** we can't confirm which, if any.
- **A public caller-supplied field:** freezes an API around a guess.
## Consequences
- The slice 7 hardware test includes an A/B run with `unknown2` = 0 against
  `unknown2` = id on both players. It checks browsing, loading, beatgrid and cues, hot cue
  autoload, track info, and what the player writes back (history, play count).
