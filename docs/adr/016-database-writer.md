# ADR-016: Building and writing a Database
## Status
Accepted. Amends ADR-009.
## Decision
`Database` can be built from scratch and written as export.pdb.

- **Public API:**
  - `Database::new()`, also used by `Default`;
  - `find_or_insert_{artist,genre,label,key}(name)`, `find_or_insert_album(name, artist)`
    and `find_or_insert_artwork(path)`, each returning `Option<Id>`;
  - `add_track(TrackFields) -> Result<TrackId, EncodeError>`;
  - `to_pdb() -> Result<Pdb, EncodeError>`.
- **Dedupe:**
  - Names are compared exactly: case-sensitive and untrimmed. On the fixture, 3 artist
    pairs differ only in case. Normalising tags is the caller's or `ExportBuilder`'s job.
  - An empty name or path creates no record and returns `None`, which is a 0 reference.
    rekordbox writes 0 for missing values, and the fixture has no empty-name rows.
  - Albums are keyed by name plus album artist. Album names are unique on the fixture, so
    it can't confirm this, and nothing contradicts it.
  - Artwork is keyed by path.
  - Records loaded from a file are found the same way.
- **Id allocation:** a new record gets the table's last id + 1 (1 when the table is empty),
  and deleted ids are never reused. Rebuilt in track-id order, the fixture's track,
  artwork, genre and label ids all match by rank, as do 262 of 277 artist ids and 226 of
  239 album ids. Key ids follow a different, unknown order. After `u32::MAX`, which only
  a loaded file can hold, the lowest free id is taken.
- **Colours:** `Database::new()` holds rekordbox's 8 default colours (ids 1–8), and callers
  don't add colours. `to_pdb` writes the database's own colour table, so a loaded database
  keeps its renamed colours. A database with no colour rows at all gets the defaults,
  because a player expects the table to be there.
- **Strictness:** `to_pdb` refuses a database with a dangling reference and reports the
  first one as `EncodeError::DanglingReference(Dangling)` (record, field and id). Row
  encoding errors (an ISRC that isn't ASCII, a colour id over 255) are caught at
  `add_track`, and the database is left unchanged. Loaded ids are written back unchanged.
- **Lowering:** each record becomes its typed row via the existing constructors and
  ADR-008's defaults, in id order, through `write_pdb` (ADR-012) with 4,096-byte pages.
## Why
ADR-009 put id allocation and dedupe inside `Database`, which owns the key space. The
fixture shows that rekordbox allocates in first-appearance order without reusing ids. The
stick's database, written with `to_pdb` and loaded again, is equal to the loaded one.
## Consequences
- Find-or-insert scans the table on every call. That is fine at fixture scale; add an
  index when a caller needs one.
- Row and page errors from `to_pdb` don't name the record.
- `Track::to_fields` stays crate-private until an editing API is needed.
- rekordbox writes one artwork row per track, so `ExportBuilder` should give each track its
  own artwork path.
