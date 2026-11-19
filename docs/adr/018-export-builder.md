# ADR-018: ExportBuilder

## Status

Accepted

## Context

Slice 7f. `ExportBuilder` is the caller's domain API (ADR-008, "Layers"). It turns what MDMA
knows about a set of tracks into a complete Phase 1 `PIONEER/` tree for the CDJ-2000/NXS.

It composes existing pieces and duplicates none of them:
- `Database` (ADR-016, ADR-017);
- `AnlzFile::track_dat` (ADR-015);
- `BeatGrid::constant_tempo` (ADR-014);
- `layout` (ADR-010).

The roadmap referred to this decision as ADR-011. The ADR tool numbers sequentially, so it
is 018.

## Decision

### Output and IO

- **No file IO for audio.** The builder never reads or copies audio. For each track it
  reports the `ContentPath` the caller must copy the audio file to. The caller supplies what
  only the audio knows: file size, exact duration, sample rate, depth, bitrate and format.
- **Output in memory.** `build()` returns an `Export`, which holds:
  - a map from `StickPath` to bytes: `/PIONEER/rekordbox/export.pdb`, one `.DAT` per track,
    and the artwork JPEGs;
  - each track's `(TrackId, ContentPath)`, in input order.

  Tests work on the map.
- **Writing.** `Export::write_to(root)` is a thin helper. It creates directories and writes
  every file with `create_new`. If any target file already exists it writes nothing and
  returns `AlreadyExists`, so it is for fresh sticks only. An I/O error partway through
  leaves the files written so far in place.
- **Phase 1 files:** `export.pdb`, and an `ANLZ0000.DAT` per track with the tags
  `PPTH PVBR PQTZ PWAV PWV2 PCOB PCOB`.
- **Files not written:**
  - `.EXT` and `.2EX`;
  - `exportExt.pdb` and `exportLibrary.db`;
  - `USBMNG.DAT`, `RBFLTR.DAT`, the setting files and `djprofile.nxs`.

  The hardware run of image B (slice 7h) decides whether any of these is needed.

### Paths

Paths come from the library, never from the caller:

- **Audio:** `ContentPath::for_track(artist, album, file name)`. It goes through ADR-010's
  collision allocator, which is pure and stateful, runs in input order, and is built in this
  slice.
- **Analysis:** `AnlzPath::for_track(id)` (ADR-010).
- **Collisions:** `layout::ContentPathAllocator` applies ADR-010's rule in input order. Case
  folding is per character and one-for-one: a character folds to its upper case only when
  that is a single character, so `ö` = `Ö` but `ß` ≠ `SS`. This mimics FAT's upcase table.
  The builder allocates a track's path before it inserts the row, so slots 19/20, PPTH and
  the disk all get the allocated path.
- **Artwork:** `layout::ArtworkPath::for_artwork(id)` gives
  `/PIONEER/Artwork/{id / 20 + 1:05}/a{id}.jpg`. It was measured on both fixtures, and all
  792 rows match (format notes, "Artwork paths").
  - Each track with artwork gets its own artwork row (ADR-016).
  - The bytes are written as given. Only the JPEG SOI marker is checked, and nothing is
    resized (a VISION non-goal).
- **Ids before insertion.** The analysis and artwork paths depend on ids that `Database`
  allocates. `Database` gains crate-private `next_track_id()` and `next_artwork_id()`, and
  the builder asserts that each insert returned the id it peeked. The public `Database` API
  doesn't change.

### Track content

- **Duration:** the caller gives the exact length in ms. The grid uses it unchanged. This
  settles ADR-014's padding question: no padding. The row gets `floor(ms / 1000)`.
- **Grid** (`Grid`):
  - `None` writes an empty PQTZ and tempo 0. This is a normal case.
  - `Constant { first_beat, tempo }` goes through `constant_tempo` with the track's duration.
  - `Beats(BeatGrid)` is used as given.

  The row's tempo is the grid's tempo. For `Beats` that is the first beat's tempo, or 0 when
  there are no beats.
- **Cues:**
  - Hot cues A–C are `(HotCueSlot, CueSpan)` pairs, and memory cues are `CueSpan`s.
  - They keep the caller's order and are never sorted, because rekordbox doesn't sort.
  - They are built with `CueList::hot` and `CueList::memory`.
  - Labels and colours live in PCO2 (`.EXT`), so they are not in Phase 1.
- **Waveform** (`Waveform`):
  - `Flat` is the default. It uses `WaveformPreview::flat` and `TinyWaveformPreview::flat`,
    with heights PWAV 2 and PWV2 1, which is what rekordbox writes for a silent track.
  - `Previews(WaveformPreview, TinyWaveformPreview)` is used as given.
  - MDMA's `Envelope` input is deferred to its own slice and ADR. How it maps to PWAV and
    PWV2 can only be judged on hardware, and PWV2 can't be derived from PWAV.
- **Format:** only `AudioFormat::{Aiff, Wav}`. ADR-015's zero PVBR is wrong for MP3, so MP3
  can't be represented.
- **Derived row values** (ADR-008):
  - filename and file path from the `ContentPath`;
  - the analysis path;
  - `unknown2`, which `Track::new` sets to the track id;
  - dates. `date_added` and `analysis_date` are per track and optional, and fall back to the
    builder's `ExportDate`. The library reads no clock, so its output is deterministic.
- **Names** go through `find_or_insert_*` exactly as given (ADR-016). Normalising them is the
  caller's job.

### Errors

The builder uses `EncodeError` throughout. `Error::Io` comes only from `write_to`. Each
problem is reported where it first shows:

- **`add_track`** validates eagerly and changes nothing on error. It works in this order:
  1. check the input;
  2. peek the path and the ids;
  3. encode the `.DAT`;
  4. do a dry run of `Track::new`;
  5. only then allocate the path and insert the rows.

  It does not clone the database per track, because that would be O(n²).
- **`ValueOutOfRange`** for:
  - a file name with no extension, after sanitising;
  - a duplicate hot cue slot, or a slot other than A–C;
  - a loop whose end ≤ its start;
  - artwork that isn't JPEG;
  - a `Constant` grid with tempo 0;
  - a duration of 65,536 s or more, which overflows the row's seconds field.
- **Row and `.DAT` errors** come from `Database::add_track` and `track_dat`.
- **`build()`** runs `to_pdb`, which refuses dangling playlist references
  (`DanglingReference`) and playlists whose parent isn't a folder.

## Public API (sketch)

    ExportBuilder::new(date: ExportDate) -> Self
    .add_track(NewTrack) -> Result<TrackId, EncodeError>
    .add_playlist(name, Option<PlaylistId>) -> Result<PlaylistId, EncodeError>
    .add_folder(name, Option<PlaylistId>) -> Result<PlaylistId, EncodeError>
    .add_to_playlist(PlaylistId, TrackId)
    .build(self) -> Result<Export, EncodeError>
    Export::files() -> impl Iterator<Item = (&StickPath, &[u8])>
    Export::audio_destinations() -> impl Iterator<Item = (TrackId, &ContentPath)>
    Export::write_to(&self, root: &Path) -> Result<(), Error>

`NewTrack` has public fields and `Default`, like `TrackFields`:
- **tags:** title, artist, album, album artist, composer, remixer, original artist, lyricist,
  genre, label, key, comment, mix name and ISRC as text; year, release date, track number,
  disc number, rating, colour and play count;
- **audio metadata:** file name, `AudioFormat`, file size, duration in ms, sample rate,
  depth and bitrate;
- **analysis:** `Grid`, hot cues, memory cues, `Waveform`, and artwork as `Option<Vec<u8>>`;
- **dates:** optional.

`NewTrack::default()` has the format `Aiff`. The file extension is not checked against the
`AudioFormat`. `ExportDate::new(y, m, d)` returns `None` for a date that isn't on the
calendar.

## Why

- Paths are a format concern, and they must agree in three places (slot 20, PPTH and the
  disk), so the library owns them.
- Leaving out audio and image handling keeps to the VISION boundary.
- The in-memory result makes the slice testable without a file system and lets callers
  stream the files to any target.
- The `Beats` and `Previews` variants make the fixture acceptance test possible. They also
  let callers with real analysis use the builder without new API.

## Alternatives considered

- **The builder copies audio:** it would need audio file IO and a progress or error policy
  for large files. That belongs to the caller.
- **Writing straight to a directory:** that couples every test to a file system.
- **The caller supplies ANLZ and artwork paths:** that invites mismatches between slot 14,
  slot 20 and the disk, and ADR-010 already derives these paths.
- **Making `Database` id peeking public:** nothing outside the builder needs it.
- **Converting the envelope in this slice:** the reduction rule would be a guess, and only
  the hardware can judge it. It is deferred.

## Acceptance test (local, rekordbox-hw1)

Rebuild the fixture's 6 tracks in track-id order from the loaded `Database` and their
`.DAT`s:
- each track gets `Grid::Beats` and `Waveform::Previews` from its `.DAT`;
- each track gets its cues' slots and spans;
- the playlist is then added in entry order.

The test checks two things:
- the 6 `.DAT` byte strings come back identical (at different paths);
- the rebuilt `export.pdb` loads to a `Database` equal to the fixture's.

Documented differences:
- slot 14 is derived from the id, not hashed;
- the duration, if `floor(ms/1000)` of the supplied length differs from the row.

The fixture holds no artwork images, so the test supplies a stand-in JPEG and compares only
the artwork rows.

A second test on synthetic data covers `Constant` and `Flat`.

## Consequences

**Deferred:**
- `.EXT`: PWV3, PCO2 cue labels and colours, PWV4 and PWV5;
- `.2EX` and OneLibrary;
- the envelope input;
- MP3 (needs a real PVBR);
- incremental updates of an existing stick;
- resizing artwork;
- Windows reserved names (ADR-010).

**Hardware** (slice 7h, image B) must show whether the NXS and the CDJ-2000 need any of the
files not written, and whether they accept the flat placeholder waveform.
