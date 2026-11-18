# ADR-004: Typed ANLZ tags live in the raw layer
## Status
Accepted
## Decision
Typed ANLZ tags (PPTH, PQTZ, PCOB first) are variants of `anlz::Tag`, next to `Tag::Raw`.
They are lossless raw-layer structs that refine ADR-002 for ANLZ:
- Fields that are not understood are stored but not asserted. They keep djl-analysis'
  names (for example `status`, `memory_count`), or `unknownN` when it has none. Setters for
  them stay crate-private until a real caller needs them, so they can be renamed.
- Fields rekordbox appears to derive, but which are not proven derived (for example the
  PCPT order links and PCOB `memory_count`), are stored verbatim. Builders compute
  rekordbox's values, and a local test checks the builders against real files.
- Enum-like fields with an `Other` arm store the raw value and interpret it in an
  accessor, so encode then decode is always the identity.
- Derived fields (path length, beat count, cue count, entry lengths) are not stored. On
  decode they are checked against the data and a disagreement is `LengthMismatch` at the
  field's offset; on encode they are recomputed.
- Bytes beyond the documented header length (`header_tail`) or after the last parsed entry
  (`body_tail`) are kept verbatim; both are empty for files built from scratch.
- A `len_header` shorter than the documented minimum is `InvalidValue`.
- `Tag::Raw` is only for fourccs this crate does not type. A typed decode failure is an
  error, never a silent fallback to `Raw`. `RawTag` cannot be built with a typed fourcc.
- Rekordbox's default values for unknown fields live in `anlz::defaults`, each citing the
  fixture it came from.

The ANLZ model layer is therefore thin: domain accessors over `AnlzFile` (for example
`hot_cues()` and `memory_cues()`, chosen by the PCOB list type, not by position).
## Why
One decode pass, errors with absolute offsets at load time, and files built directly from
typed values (`AnlzFile::new(h, vec![Tag::BeatGrid(..), ..])`). Keeping unknowns in the
typed structs preserves identical-bytes round-trips.
## Alternatives considered
- **Keep `AnlzFile` fully raw** with typed tags converted from and to `RawTag`: matches
  ADR-002 literally, but parses twice and loses absolute error offsets unless `RawTag`
  stores one.
## Consequences
- The real-stick round-trip test must also assert that no PPTH, PQTZ or PCOB decodes as
  `Raw`, so a fallback can never make it pass trivially.
- Proptest generators produce raw tags only for untyped fourccs, plus typed generators.
