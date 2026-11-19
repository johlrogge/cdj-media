# Format notes

Observations about Pioneer's formats that add to or differ from Deep Symmetry's
djl-analysis, with the evidence behind them. The evidence comes from local real-stick
fixtures (`local-fixtures/`, never committed; see ADR-003). The counts below are from one
rekordbox-exported, CDJ-played stick.

## ANLZ

- Every `len_file` equals the file size, and tags tile the file exactly, with no padding
  and no trailing bytes (2,500 files across `.DAT`, `.EXT` and `.2EX`).
- PQTZ `unknown2` is `0x00080000` in all 848 files. djl-analysis gives `00800000`.
- PPTH `len_path` counts the body including the UTF-16BE NUL terminator.
- PCPT: the order fields sit where djl-analysis' diagram puts them (`order_first` at
  0x18, `order_last` at 0x1a), not where its prose puts them. In memory lists they form a
  doubly linked list of indices with 0xFFFF at both ends, and PCOB `memory_count` is the
  entry count minus one (0xFFFFFFFF when empty). Hot lists use 0xFFFF/0xFFFF and
  0xFFFFFFFF.
- PCPT `status` is 0 or 1 in practice. djl-analysis mentions 4 for an active loop, which
  was not seen.
- Memory cues are not stored in time order: 58 of 68 lists with two or more cues aren't.
- Undocumented tags seen: PQT2 (`.EXT`), PWVC and PVDI (`.2EX`). PWVC has
  `len_header` 14.
- **PQTZ beat grids** (826 tracks with a `.DAT`, matched to their track row through the
  analysis path; `tests/beat_grid_local.rs`). Below, `t_n` is the time of beat `n` (0 for
  the first) and `I` = 60000 / BPM ms, from the stored tempo:
  - 2 grids are empty. In the other 824 every beat has the same tempo, equal to the
    track row's tempo; no grid on the stick changes tempo.
  - The first beat is always before `I` (all 824), so the grid reaches back to the
    earliest beat at or after 0. 85 grids start at exactly 0, 75 of them on beat 1.
  - `beat_number` counts 1 to 4 from the first beat without a break (all 824). The first
    beat is beat 1 in 532 grids, 2 in 185, 3 in 40 and 4 in 67.
  - In 801 grids every interval is `floor(I)` or `ceil(I)`; in 23 some are 1 to 2 ms
    further off.
  - Beat times are not computed from the stored whole-ms first beat. In 483 of the 801,
    some real phase φ gives every beat as `floor(φ + n·I)` (equivalently `round`, with φ
    shifted by ½), but φ falls anywhere in the first beat's millisecond: the anchored
    rule `floor(t0 + c + n·I)` matches every beat for 127 to 147 of the 483 grids,
    whichever fraction `c` is used, best at `c` = ½ (rounding). A two-stage rule through
    whole samples (round or floor to a sample, then floor or round to a ms) fits 489, not
    more.
  - The other 318 regular grids need a tempo finer than the stored hundredths. 144 fit one
    finer tempo, nearly all within ±0.005 BPM of the stored one (stored = rounded);
    174 fit none: they split into runs at the stored tempo, each with its own phase (108
    into 2 runs, 37 into 3–5, 16 into 6–10, 157 into 11 or more). Over a whole track the
    last beat sits within ±5 ms of `t0 + last·I` in 731 grids, and up to 190 ms off in
    others.
  - The last beat is the last one before the end of the track: with the end taken from
    the `.EXT` PWV3 length (1/150 s entries), the beat count equals the number of beats
    `t0 + n·I` before the end for 743 of 815 grids, is one fewer for 69, and is 1 or 2
    off for 12.
    The pdb duration is whole seconds, rounded down: PWV3 is 0 to 1.1 s longer. From
    the pdb seconds the count is right for only 191 of 815 grids, short by 1 to 4 beats
    for the rest.
  - Rebuilding each grid from its first beat 1, its tempo and the PWV3 length with
    `round(t_a + k·I)` gives 121 of 824 grids back exactly and 479,003 of 702,606 beats.
    Of the 483 exact grids: 121 equal, 335 within 1 ms everywhere, 22 differ only in the
    last beat or two, and 5 start on another beat because the beat nearest 0 lands within
    1 ms of it, on the other side. With pdb seconds as the length, 41 come back exactly.

## export.pdb

- **The last row group is partial.** Every row group holds 16 offsets in 36 bytes except
  the last, which holds only its own k = `num_row_offsets − 16·(groups−1)` offsets in
  4 + 2k bytes. The heap extends into the rest. Evidence: page 80 has 116 offsets, and
  present row 115 sits at heap offset 3768 with `used_size` 3792, inside where a full
  last group would be. Under this layout `free_size + used_size` equals the heap length
  on all 292 data pages of `export.pdb` and all 11 of `exportExt.pdb`, against 2 of 292
  with full groups.
  - Observed: pages whose `num_row_offsets` is a non-zero multiple of 16 (4 pages; the
    last group is a full 36 bytes).
  - Extrapolated: data pages with `num_row_offsets` = 0. None exist, because empty tables
    consist of an index page only.
- The `next_page` of a table's last page equals that table's `empty_candidate`.
  `empty_candidate` and the header's `next_unused_page` may point past the end of the
  file.
- Pages that belong to no table chain are zero-filled (18 of 331).
- Each table's first page is an index page (flags 0x64); the rest are data pages (0x24
  or 0x34).
- String forms: rekordbox writes ASCII of at most 126 bytes as short strings, longer
  ASCII as long ASCII (0x40), and anything non-ASCII as UTF-16LE (0x90), without
  exception across 17,966 strings. ISRC slots use 0x90 with `0x03`, ASCII and a NUL.
- **String heap layout within a row:** string offsets increase strictly in slot order,
  and the first string starts right after the row's fixed part (track rows at 0x88).
  A short-ASCII string follows the previous string directly. A long-form string
  (UTF-16LE or long ASCII) starts at the next heap offset that is a multiple of 4. This
  rule holds without exception across the 16,540 consecutive string pairs in track rows.
  The 0–3 gap bytes are **not** always zero (245 zero, about 100 leftover ASCII), so a
  lossless raw row has to store them.
- **Small-table rows** (genres, artists, albums, labels, keys, colours, artwork; 1,394
  rows, checked by `tests/pdb_rows_local.rs`):
  - No row has id 0.
  - Every artist row is near (subtype 0x60, all 277) and every album row is near (0x80,
    all 239); the far layouts never occur. The rows whose name starts at +12 (17
    artists) or +24 (14 albums) are near rows with a long-form (UTF-16LE) name, which the
    alignment rule above puts at the next multiple of 4 after two zero gap bytes. The byte before the name offset is 3 in all of them.
  - Artist and album `index_shift` is 0x20 × the row's slot in its page's row index,
    absent rows included (all 516 rows; the present-row index gives it in only 480).
  - Album `unknown1` and `unknown2` are 0 in all rows.
  - Colour rows match djl-analysis' offsets: `unknown1` (u32) 0, the byte at +4 equal to
    the id, the id as a u16 at +5 (1 to 8, no row for 0), the byte at +7 0, the name at
    +8.
  - Key `id2` equals the id in 42 of 43 rows; key 44 ("2B") has `id2` 43.
  - Every row starts at a heap offset that is a multiple of 4. After a genre, label,
    key, colour or artwork row, the next row starts at the next multiple of 4. After an
    artist or album row there are 4 more bytes: the next row starts at
    `align4(far fixed length + name length) + 4` from the row start, where the far fixed
    length is 12 (artist) or 24 (album), as if the row were far. The last row's padding
    is inside `used_size`. Padding bytes are zero, except on 4 pages (77, 78, 80, 81)
    where rows were rewritten in place: there the space after a row holds leftover text
    of a longer, earlier version, and the next row may start further on.
- **Track rows** (827 on the stick):
  - No track id is 0. `index_shift` is 0x20 × the row's slot in the page's row index,
    counting absent rows, on every row (the same rule as artist and album rows).
  - `subtype` 0x24, `unknown3` 0x3520, `unknown4` 0x1f58, `unknown5` 0x29 and `unknown7`
    3 on every row. `bitmask` is 0xc0700 on 816 rows and 0xd0700 on 11 (they differ in
    bit 0x10000).
  - `unknown2` (u32 at 0x14) is different on every row and fits in 28 bits (0x3cd9 to
    0xffb3bb9, mostly 24–28 bits). It doesn't grow with the track id, and it never matches
    the track id or the hex numbers of the analysis path, so it looks like an opaque
    random id.
  - `file_type`: 765 AIFF (0x0c), 37 WAV (0x0b), 25 MP3 (1). Composer, original artist,
    remixer and colour references are 0 on every row.
  - String slots: 2 holds 1–2 digits and is never empty; 3 holds one digit; 4 is usually
    empty, otherwise 1–2 digits; 8, 9, 13 and 18 are always empty; 6 and 7 are "ON" on
    every row; 10 and 15 are `YYYY-MM-DD`; the filename (19) always equals the last
    component of the file path (20). The ISRC slot holds the ISRC form on 10 rows and an
    empty short string on the rest.
  - Slot 14 (analysis path) is `/PIONEER/USBANLZ/Phhh/hhhhhhhh/ANLZ0000.DAT` (upper-case
    hex) on 825 rows; 2 rows end in `ANLZ0001.DAT`. Each of those shares its directory
    with exactly one other track whose path names `ANLZ0000.DAT`, consistent with a
    hashed directory name plus numbered collisions. In one of the two directories
    `ANLZ0000.DAT`/`.EXT` don't exist on the stick, so a track row can point at analysis
    files that are missing.
  - Slot 20 (file path) is `/Contents/<artist>/<album>/<file name>` on all 827 rows (4
    components, at most 140 UTF-16 units). rekordbox replaces `/ : ? " |` in artist and
    album names with `_`, and a trailing `.` too; it cuts every component to 48 UTF-16
    units (file names keep their extension, and a cut stem may end in a space; a cut
    directory loses a trailing space). No component ends in a dot or a space. A track
    with no artist goes under `UnknownArtist`, one with no album under `UnknownAlbum`.
    `layout::ContentPath` gives back all 827 paths from the artist and album names and
    the file name (ADR-010).
  - `PPTH` in each `.DAT` and `.EXT` equals the track's slot 20, on all 826 tracks whose
    analysis files exist.
  - Non-zero gap bytes occur only before slots 17, 19 and 20 (30 rows).
- `exportExt.pdb` uses the same page framing, with its own table numbering (types 0–8).
