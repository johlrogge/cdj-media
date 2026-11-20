# ADR-019: Hashed analysis paths

## Status

Accepted. Supersedes ADR-010's "Analysis paths" decision.

## Context

ADR-010 derived analysis paths from the track id (`P{id & 0xFFF:03X}/{id:08X}`), because
rekordbox's hash was unknown. Hardware run B (`docs/hardware/b.md`) disproved that choice
on the CDJ-2000NXS:

- **The row is ignored.** The NXS ignores a track row's analysis path (slot 14). It looks
  for the analysis in a folder computed from the audio file's path. When that folder had no
  `.DAT`, it analysed the track itself and wrote its own `.DAT` there.
- **The same hash as rekordbox.** For a path present on both sticks, the folder the NXS
  chose equals the one rekordbox wrote.
- **Our `.DAT`s are accepted.** With our `.DAT`s copied into those folders (run B2), the NXS
  showed our BPM, grid, hot cues, memory cues and flat waveform. The tracks left at
  id-derived folders stayed unanalysed.

The function was recovered from the data alone, without reading other implementations.
It matches every rekordbox- and player-written `.DAT` on the fixtures: 843 files on four
sticks, including the `ANLZ0001` files.

## Decision

### The folder hash

The input is the audio file's stick path exactly as in slot 20 and PPTH: the leading `/`,
no NUL, no case folding, as UTF-16 code units.

    h = 0
    for each UTF-16 code unit c:  h = (h * 0x34F5501D + c) mod 2^32
    d2 = ((h * 0x93B6) mod 2^32) mod 200003
    d1 = d2's bits 0, 2, 6, 7, 9, 13, 16, packed as bits 0..6
    folder = /PIONEER/USBANLZ/P{d1:03X}/{d2:08X}/

The multiplier could equally be 0xB4F5501D, because 0x93B6 masks bit 31. Both give the
same folders.

### Collisions

Paths that hash to the same folder share it, and the files in it are numbered `ANLZ0000`,
`ANLZ0001`, …. The player presumably tells them apart by their PPTH.

The evidence for this is thin. On the fixtures:
- **One real collision:** only one folder holds both `ANLZ0000` and `ANLZ0001`.
- **`ANLZ0001` on its own:** 23 folders hold `ANLZ0001` with no `ANLZ0000`. In 22 of them
  the PPTH is outside `/Contents/` (files rekordbox analysed elsewhere on the stick). So a
  lone `0001` is not a collision, and rekordbox's numbers have gaps.

The builder numbers each folder from 0 in input order within one export: the first track
gets `0000` and the next `0001`. Each `.DAT`'s PPTH names its own audio file. Indexes stop
at 9999, so names stay 8.3.

Whether the player picks a collision's `ANLZ0001` by PPTH is still to be shown on hardware.

### API

- `AnlzPath::for_content(&ContentPath, index: u16) -> Option<AnlzPath>` replaces
  `AnlzPath::for_track(TrackId)`. It gives `None` above `MAX_ANLZ_INDEX` (9999).
- `layout::anlz_folder(&ContentPath)` exposes the folder.
- `ExportBuilder` keeps a per-folder counter. A folder's counter only grows within one
  export.
- Slot 14 still holds the analysis path, as rekordbox writes it.

## Why

The player finds analysis only at the hashed folder, so an export written any other way
has no grid, no cues and no waveform on the NXS.

## Alternatives considered

- **Keep id-derived folders:** the hardware showed the NXS never finds them.
- **Write the analysis to both folders:** that doubles the files, and nothing reads the
  id-derived copy.

## Consequences

- **Acceptance test:** ADR-018's documented `analysis_path` difference should disappear.
  The rebuilt export's slot 14 should equal rekordbox's for every track that doesn't
  collide.
- **ADR-010 updates:**
  - its "Analysis paths" bullet and its alternative "Copy rekordbox's hashed ANLZ
    directories" are superseded;
  - the FAT32 limit on entries per `P` directory still holds (at most 128 `P` folders, each
    holding at most about 1,563 `d2` values).
- **Hardware still needed:**
  - a rebuilt image B on the NXS;
  - a collision on hardware, to confirm the player picks `ANLZ0001` by PPTH;
  - the CDJ-2000.
