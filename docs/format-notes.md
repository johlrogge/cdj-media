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
- **PMAI header** (bytes 12..28; `len_header` is 28 in all 2,500 files): four u32 words.
  The fourth is always 0. In `.DAT` files:
  - `1, 0x00010000, 0x00010000, 0` in 793 of 848;
  - the middle two words are 0 in the 22 `.DAT` files that have no `.EXT` or `.2EX`
    next to them (17 with first word 1, 5 with 0). They look like stale analyses: 21 of
    the 22 have no beats, and there are 826 tracks with analysis files but 848 `.DAT`s;
  - the first word is 0 in 38: all 25 MP3 tracks, 8 AIFF tracks with nothing else in
    common, and 5 of the stale files. No other field explains it.
  - `.EXT` files: the modal header in 823 of 826, first word 0 in 3 (AIFF). `.2EX`: the
    modal header in all 826.
- **Tag order** is fixed in `.DAT`: `PPTH PVBR PQTZ PWAV PWV2 PCOB PCOB` in all 848, hot
  cues first. `.EXT` is `PPTH PWV3 PCOB PCOB PCO2 PCO2 [PQT2] PWV5 PWV4 [PSSI]` (826
  files): 625 with PQT2 only, 149 with both, 44 with neither, 8 with PSSI only. `.2EX` is
  `PPTH PWV7 PWV6 PWVC [PVDI]`, PVDI in 99 of 826.
- **PVBR**: `len_header` 16 with a zero u32, and a 1,604-byte body in all 848 `.DAT`
  files. The body is all zero for every AIFF and WAV track (823) and non-zero for all 25
  MP3 tracks.
- **PWAV and PWV2**: `len_header` 0x14, as djl-analysis documents. The u32 at 0x0c is the
  column count (400 and 100 in every file) and the u32 at 0x10 is `0x00010000` in all
  848 files each; djl-analysis gives `00100000`, the same nibble shift as PQTZ
  `unknown2`.
  - PWAV whiteness (high three bits) takes 0 to 5, never 6 or 7; height (low five bits)
    takes 0 and 2 to 25, never 1. Low columns are mostly whiteness 5 (heights 2 to 9),
    loud ones mostly 0 to 3.
  - PWV2 never sets a bit above the low four; heights 0 to 15 occur.
  - The one silent track (no beats) has 400 PWAV columns of `0xa2` (height 2, whiteness
    5) and 100 PWV2 columns of 1.
  - PWV2 is not a simple reduction of PWAV: max/2, rounded max/2, max · 15/31, and the
    mean or rounded mean over each 4 PWAV columns match all 100 columns in only one file
    (the silent one).
- **PWV3**: `len_header` 0x18; `len_entry_bytes` is 1, `len_entries` equals the body
  length and the u32 at 0x14 is `0x00960000` in all 826 `.EXT` files, as djl-analysis
  documents. Columns use all three whiteness bits (up to 7) and heights up to 31.
- **Rebuilding `.DAT` files** (`tests/anlz_local.rs::real_stick_dat_files_rebuild_identically`):
  `AnlzFile::track_dat` from each file's typed tags gives 793 of 848 back byte for byte.
  The other 55 differ only in the PMAI header (30: the 22 stale files and the 8 AIFF
  tracks with first word 0) or in the PMAI header and PVBR (the 25 MP3 tracks). With the
  cue lists rebuilt from slots and spans by `CueList::hot` and `CueList::memory`, the
  same 793 come back.
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
- **Data page layout** (292 data pages, checked by `src/pdb/writer/local.rs`):
  - `free_size = len_page − 0x28 − row_index_len(num_row_offsets) − used_size` on all 292,
    with the partial last row group above.
  - `index_shift` is 0x20 × the slot on all 1,343 present track, artist and album rows,
    and the same two bytes hold it in all 608 absent (deleted) rows, whose bytes stay in
    the heap. Genre, label, key, colour and artwork rows have no such field.
  - Rows lie in the heap in slot order, the first at offset 0, on every page.
  - What a row takes in the heap before the next row: genre, label, key, colour and
    artwork rows their length rounded up to a multiple of 4; artist and album rows their
    far length (fixed part 12 or 24 plus the name) rounded up, plus 4; **track rows the
    length they would have if every string started at a multiple of 4 (0x88 plus each
    string rounded up to 4), rounded up, plus 4**. The last row's padding is inside
    `used_size`. On pages without deleted rows this is exact for every row whose padding
    is zero; the others hold leftover bytes of a longer, earlier version of the row,
    rewritten in place (49 track, 8 artist and 11 album rows on such pages; on pages with
    deleted rows also 508 track and 2 artist rows). 5 track rows, all on pages with
    deleted rows, are followed by 4 more zero bytes than this; not explained.
  - Header fields: `unknown1` (0x00), `unknown2` (0x14) and the two u16s at 0x24 and 0x26
    are 0 on every data page. `page_flags` is 0x24 on all 109 pages without deleted rows
    and 0x34 on all 183 with them, so bit 4 means "holds deleted rows". `num_rows` is the
    number of set `rowpf` bits.
  - The transaction fields (`transaction_row_count`, `transaction_row_index` at 0x20 and
    each group's `tranrf`) look like a record of the page's last write: the count is the
    number of set `tranrf` bits on all but 15 pages, which hold 0x1fff in both u16s and no
    bits. 229 pages flag one row, 12 two. 33 pages were written in one go: the count is
    every row, the index 0 and each `tranrf` equals `rowpf` (the colour page, for one).
    `sequence` differs on every page and is not derived from the rows.
  - **Rebuilding pages:** from its typed rows (with `index_shift` zeroed), its
    `page_index`, `next_page` and `sequence`, and its transaction fields, the page writer
    gives back 79 of the 91 typed pages without deleted rows byte for byte, 24 of them
    with whole-page transaction values. The other 12 hold rows rewritten in place. Not
    rebuilt: 174 pages with deleted rows (168 of the 178 track pages) and 27 pages of
    untyped tables. Per table, identical of typed pages without deleted rows: tracks 1 of
    10, genres 6 of 6, artists 8 of 9, albums 17 of 19, labels 5 of 5, keys 11 of 11,
    colours 1 of 1, artwork 30 of 30.
  - **Page breaks:** a page ends when the next row, its footprint and its row index entry
    (2 bytes, or 6 for a new group) no longer fit; a page may end with 0 bytes free (5
    artwork pages of 106 rows). No row-count limit shows: pages hold up to 116 artist,
    106 artwork and 284 playlist-entry rows. Of the breaks after which the next page's
    first row is present, 99 are full pages (90 of them track pages) and 75 left room for
    that row; on 72 of those 75 the next page has a higher `sequence`, which fits rows
    added in a later update rather than a layout rule. Packing a whole table therefore gives fewer pages than the
    stick has (artwork 8 against 31, tracks 92 against 178).
- **File layout** (331 pages; checked by `src/pdb/writer/local_file.rs`):
  - Header: `unknown1` 0, `len_page` 4,096, 20 tables, `unknown2` 1, `sequence` 15,493,
    `unknown3` 0, and zero after the table pointers. The tables are types 0 to 19 in type
    order, including the unnamed 9, 10, 14, 15, 17 and 18.
  - Table `t` (its type) starts with its index page at page 2t + 1. Page 2t + 2 is its
    first data page, or, for the 4 tables without data pages (9, 10, 14, 15), its
    `empty_candidate`. So pages 1–40 look like the database's creation; every later page
    came with later updates. No table with two or more data pages has them contiguous:
    they are interleaved with other tables' (tracks run 1, 2, 49, 52, 70, …).
  - Every `empty_candidate` is a page of its own, in no chain. The 18 pages no chain
    reaches are exactly the `empty_candidate`s inside the file, all zero; 2 (tracks 332,
    playlist entries 331) are past the end. `next_unused_page` (333) is one past the
    highest of the file's pages and the `empty_candidate`s.
  - The header's `sequence` is one more than the highest page `sequence` (15,492). Index
    pages of the 8 tables without deleted rows have `sequence` 1. The three one-page
    tables never rewritten have low data-page `sequence`s: colours 2, columns 3, table 18
    5, which looks like creation order. The other pages' `sequence`s record later writes
    and follow no page order.
  - Rebuilding the file from its decoded pages, its table pointers and those two header
    rules gives it back byte for byte.
- **Index pages** (the first page of every table): header flags 0x64, `unknown1`,
  `unknown2`, the row counts, `free_size` and `used_size` 0; `next_page` is the first data
  page, or the `empty_candidate` for a table without one. After the 0x20-byte header:
  - 0x00: two `u16` 0x1fff (the value a data page's transaction fields take without a
    transaction); 0x04: the number of entry slots, 0x3ec (1,004), which is what the page
    leaves for them: (4,096 − 0x20 − 0x1c − 0x14) / 4; 0x06: a `u16`, 0 without entries,
    1 to 169 with them, at least the entry count (not derived); 0x08: the page's own index;
    0x0c: the first data page, or 0x03ffffff without one; 0x10: 0x03ffffff; 0x14: 0;
    0x18: the number of entries; 0x1a: 0x1fff, or 1 (artists, albums, artwork) or 16
    (tracks) on tables with entries (not derived); 0x1c: 1,004 `u32` entry slots; then
    0x14 zero bytes.
  - An entry is a page index shifted left 3, with 3 flag bits (0 in every entry); an empty
    slot is 0x1ffffff8 (0x03ffffff shifted). The entries name exactly the table's data
    pages with deleted rows (12 tables, 1 to 168 entries), which fits djl-analysis' "find
    free space". The 8 tables without deleted rows have empty index pages, and building an
    empty index page from the page's links gives all 8 back byte for byte; the other 12
    equal the built page except for the entries, their count and the two `u16`s above.
- **Untyped tables:** playlist tree 2 data pages (229 row offsets, 32 present), playlist
  entries 6 (903 present rows of 12 bytes), history playlists 7 (10 present), history
  entries 7 (83 present), history 1 (1 present of 96), table 17 2 pages (22 present rows of
  8 bytes, rewritten: `sequence` 2,326 to 2,332), table 18 1 page (17 rows of 8 bytes,
  `sequence` 5, never rewritten). Tables 9, 10, 14 and 15 are empty.
- **Fixed tables:** the colour and column tables are one data page each, written once in
  one transaction. The 8 colour rows (ids 1–8) and the 27 column rows equal rekordbox's
  defaults: building each page from the defaults (`writer::fixed`) with its links gives it
  back byte for byte. A column row is an id (`u16`, 1–27), a second `u16` (0x80 to 0xaa,
  not in id order) and the label as UTF-16LE between U+FFFA and U+FFFB (`GENRE`,
  `ARTIST`, …, `MATCHING`); it takes its length rounded up to 4 in the heap.
- **A fresh file from the stick's rows** (`write_pdb`): 144 pages against 331 (tracks 92
  data pages against 178, artwork 8 against 31, the small tables 1 to 4 against 5 to 20),
  `next_unused_page` 153, `sequence` 114. All 2,221 typed rows come back in order, and it
  loads into the same `Database` as the stick, with no dangling reference.
  `Database::to_pdb` writes the same layout from the loaded database (144 pages), its
  own colour table included, and the result loads back equal to it.
- **Names and ids** (checked by `tests/database_local.rs`):
  - Names are unique within every small table, compared exactly: artists 277, albums 239,
    genres 19, labels 21, keys 43, artwork paths 787. Case counts: 3 pairs of artists
    differ only in case (274 names case-folded). No name is empty or has leading or
    trailing whitespace, so the stick doesn't show whether rekordbox trims. Tracks
    without a value reference 0 instead: 24 have no artist, 51 no album, 713 no genre,
    800 no label, 137 no key and 40 no artwork.
  - Album names are unique on their own, so the stick can't tell "one album per name"
    from "one album per name and album artist". The album artist is a tag of its own:
    76 albums have none, 17 albums hold tracks of more than one track artist (16 of them
    with an album artist), and 125 tracks on albums with an album artist have a
    different track artist.
  - Every record is referenced: no artist, album, genre, label, key or artwork row is
    unused. Each artwork row belongs to exactly one track (787 rows, 787 tracks with
    artwork), even within an album.
  - Ids have gaps: tracks 1 to 881 for 827 rows, artists 1 to 292 for 277, albums 1 to
    251 for 239, genres 1 to 22 for 19, keys 1 to 44 for 43, artwork 1 to 835 for 787;
    labels 1 to 21 without gaps. Rebuilding the database through find-or-insert in
    track id order (each track's artists, album artist, album, genre, label, key and
    artwork, then the track) allocates ids in the same order as rekordbox for all 827
    tracks, 787 artworks, 19 genres and 21 labels, 262 of 277 artists and 226 of 239
    albums: the rebuilt id equals the rank of rekordbox's id. So rekordbox allocates in
    the order records first appear and doesn't reuse the ids of deleted records. Key ids
    follow another order (0 of 43). Rebuilding in page order matches almost nothing.
- `exportExt.pdb` uses the same page framing, with its own table numbering (types 0–8).
