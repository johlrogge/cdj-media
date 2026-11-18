# ADR-010: Stick path layout
## Status
Accepted
## Decision
- **Audio paths** (track slot 20, and PPTH in every ANLZ file, which always equals it on
  the real stick) are `/Contents/<artist>/<album>/<file>`, built by
  `ContentPath::for_track`, which mirrors rekordbox:
  - `/ \ : * ? " < > |` and control characters become `_`;
  - each component is cut to 48 UTF-16 units, never inside a character; a file name
    keeps an extension of up to 7 units;
  - trailing spaces are trimmed, and a trailing dot becomes `_`;
  - an empty artist or album becomes `UnknownArtist` or `UnknownAlbum` (rekordbox's
    names); an empty file name becomes `Unknown`.

  It reproduces all 827 real slot-20 paths. Callers pass metadata, never a directory
  name another tool already chose for "unknown".
- **Analysis paths** (slot 14) are derived from the track id, not from rekordbox's hash:
  `/PIONEER/USBANLZ/P{id & 0xFFF:03X}/{id:08X}/ANLZ0000.{DAT,EXT}`. The scheme is
  injective and reversible, so the hashed-directory collisions seen on the stick
  (`ANLZ0001`) can't happen. It keeps rekordbox's character set and shape. The low bits
  spread ids so no `P` directory nears FAT32's 65,536-entry limit.
- **`StickPath`** is the only path type: validated, absolute, FAT32-safe (255 units per
  component, 257 per path). `ContentPath` and `AnlzPath` wrap it, so slot 20 and slot 14
  can't be swapped. `PathError` stays separate from the crate `Error` until an API needs
  both.
- **Collisions are the builder's job** (slice 7), through a pure, stateful allocator in
  `layout` that `ExportBuilder` drives, in input order so output is deterministic:
  - directories that collide after case folding reuse the first spelling, because FAT32
    is case-insensitive and the database must match the disk;
  - file paths that still collide get ` (2)`, ` (3)`, … before the extension, re-cut to
    48 units.
- **Deferred:** Windows reserved names (`CON`, `NUL`, …), which are legal on FAT32, and
  Unicode normalisation, which FAT32 doesn't do; names pass through unchanged.
## Why
Output should look like rekordbox's, except where rekordbox's own scheme causes
collisions and missing analysis files.
## Alternatives considered
- **Copy rekordbox's hashed ANLZ directories:** the hash is unknown, and it collides.
- **MDMA's "Unknown Artist":** doesn't match what rekordbox writes.
## Consequences
- The slice 7 hardware test also covers a maximum-length path (three 48-unit
  components; the stick's longest is 140 units), non-BMP characters, and id-derived
  analysis paths, on both the CDJ-2000 and the 2000NXS.
