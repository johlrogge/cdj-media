# ADR-007: Typed export.pdb rows live in the raw layer
## Status
Accepted
## Decision
Refines ADR-002 and ADR-006 for `export.pdb` rows, as ADR-004 does for ANLZ tags.
- **Typed rows decode from `RawRow`** and re-encode to exactly the row's bytes. Errors keep
  `RawRow`'s absolute offsets. `ExportRow::decode(table, &RawRow)` returns
  `Ok(None)` for tables that are not typed (yet). Every `ExportTable` variant is listed
  explicitly in that match, so adding a table forces a decision. A typed decode failure is
  an error, never `None`.
- **Ids are non-zero newtypes.** No real row has id 0. A reference of 0 means "none" and
  becomes `Option::None`.
- **A typed row ends after its last string.** The padding after it, which rekordbox sizes
  in a row-specific way (see `docs/format-notes.md`) and which can hold leftover bytes from
  rows rewritten in place, belongs to the page heap, which keeps it verbatim.
- **Fields that are not understood are stored**, named after djl-analysis or `unknownN`,
  with crate-private setters. Raw enum-like fields (artist/album `subtype`) are stored and
  interpreted by accessors. Far layout means exactly subtype 0x64 (artist) or 0x84 (album);
  any other value reads as near and still round-trips.
- **Position-dependent fields are stored and owned by the page writer.** Artist and album
  `index_shift` is 0x20 × the row's slot in the page's row index on every real row.
  Builders write 0; fresh page layout (slice 7) always overwrites it through one
  crate-private function that a local test checks against the fixture.
- **Builders reproduce rekordbox.** They always produce near rows, because a built name
  offset always fits a byte. Values that appear derived (key `id2`, colour `unknown2`) are
  computed by builders and stored on decode. Defaults live in `pdb::defaults`, each checked
  as the modal value by a local test.
## Why
Lossless loading of files from any writer (ADR-002), and rows built to look like
rekordbox's, so the hardware sees what it expects.
## Alternatives considered
- **A `Raw` variant in `ExportRow`:** would put a lifetime on an owned model type.
- **Deriving `index_shift` from the slot on decode:** proven on one writer only, like
  `free_size` (ADR-006).
- **Far when `subtype & 0x04`:** no evidence for it, and it could silently misparse.
## Consequences
- Slice 7 adds `index_shift_for_slot` and a per-row `heap_footprint` for rekordbox-like
  placement, both test-first and checked against the fixture.
- Slice 5b adds `Track` as an `ExportRow` variant, plus a `Database` model that iterates
  the typed tables explicitly.
