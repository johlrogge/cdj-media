# ADR-001: Hand-rolled binary codecs with thiserror errors (no binrw/deku/nom)
## Status
Proposed
## Decision
All formats (ANLZ and DeviceSQL `export.pdb`) are encoded and decoded by hand-rolled codecs
built on a crate-private `bytes` layer: `Reader` (typed big- and little-endian reads that
record the absolute offset), `Writer` (into a `Vec<u8>`) and `Patch` (reserve a slot and
fill it in once lengths and offsets are known). `thiserror` is the only runtime dependency.
Errors carry byte offsets. One approach for every format.
## Why
The hard part of these formats is layout: DeviceSQL pages with a heap growing forward and a
row index growing backward, row groups with presence masks, row-relative string offsets,
page chains, and ANLZ tag lengths known only after the body is written. Derive libraries
handle fixed headers but don't solve layout, and their writers handle offsets poorly.
Hand-rolling keeps layout explicit and testable, makes unknown fields trivial to keep, and
gives exact error offsets.
## Alternatives considered
- **binrw**: bidirectional, but offset reads (`FilePtr`) have no write side and derive
  errors are weak. rekordcrate's write support lagging its read support is a warning sign.
- **deku**: bit-level and bidirectional, but heavy compile times and awkward for heap and
  offset layouts. The bit-level work here is a handful of small functions.
- **nom**: read-only.
## Consequences
- About 200 lines of primitives to own and test.
- Fixed headers take more boilerplate. `zerocopy` with byte-order types stays an option if
  that becomes tedious.
