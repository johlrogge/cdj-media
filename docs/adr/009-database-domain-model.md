# ADR-009: Database domain model: contents, strictness and lowering
## Status
Accepted
## Decision
- **`Database::from_pdb` loads leniently.** A reference to a missing row loads. The
  record keeps the raw id, and the `*_of` resolvers return `None` ("not set, or
  dangling"). `Database::dangling_references()` reports each one at the domain level as
  (record, field, id), computed on demand. File offsets aren't used, because they stop
  meaning anything once a database is edited, and the raw `Pdb` remains for byte-level
  debugging.
- **Duplicate ids are strict** (`DecodeErrorKind::DuplicateId`, at the second row's id
  field). A map keyed by id can't hold both, and "first one wins" would drop data
  silently. Rows whose presence bit is clear (tombstones) don't count. This matters on
  real data: the stick's tracks table has 1,431 row offsets but 827 present rows, and its
  tombstones include older copies of present ids (for example 838).
- **The model holds domain values only:** `String` text (ADR-005), empty ISRC and dates
  as `None`, references as ids, resolved by accessors (this is what ADR-002's "resolved
  IDs" means). Raw-layer unknowns, string forms, heap padding and untyped tables are not
  part of the model. `Database::to_pdb` (slice 7) produces a fresh, rekordbox-like layout
  from ADR-008's defaults. Lossless editing of a foreign file goes through the raw `Pdb`.
- **Id allocation and name deduplication move to slice 7,** inside `Database`, which owns
  the key space (find-or-insert by name; next id = last key + 1). This amends ADR-008's
  Layers bullet, which placed them in slice 5c.
- **Writers are strict where readers are lenient.** `to_pdb` and `ExportBuilder` refuse to
  write a database they built with dangling references, since in built output that is a
  bug. Lowering a loaded database writes raw ids back unchanged.
- **Naming:** row types stay at `pdb::` (for example `pdb::Track`). Domain types live
  under `pdb::model`, with `Database` also re-exported as `pdb::Database`. Revisit before
  slice 7 grows the public writer API.
## Why
Reading back what a CDJ writes is a project goal (VISION), and CDJ- or third-party-written
sticks may contain references rekordbox would never write. They must load (ADR-002),
the same reasoning ADR-005 applies to string pad bytes.
## Alternatives considered
- **Strict dangling references:** one odd reference would make the whole stick unreadable.
- **Two modes (strict and lenient):** more API for no observed need. The on-demand report
  covers validation.
- **Keeping raw unknowns in the model:** leaks raw-layer concerns into the domain API. A
  private `origin` field can be added later without breaking callers, if a lossless model
  round-trip is ever needed.
## Consequences
- Playlist entries and history (slices 9 and 10) have no ids of their own. They get their
  own loaders, grouping by playlist and ordering by entry index, and they report dangling
  track references and parent cycles through the same `dangling_references()`.
- Slice 7's first test: build a row from fully populated `TrackFields`, load it, lower it,
  and get the same fields back.
