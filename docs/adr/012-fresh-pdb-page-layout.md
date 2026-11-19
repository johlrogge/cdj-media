# ADR-012: Fresh export.pdb page layout
## Status
Accepted. Data pages are from slice 7c; the whole file (below) is from slice 7d.
## Decision
Writers lay out data pages from typed rows. The layout is computed, never copied from a
fixture. Everything below is measured on the real-stick fixture (`docs/format-notes.md`,
"Data page layout").

- **Sizes:** `used_size` is the sum of the row footprints. `free_size` is what the page
  header (0x28), the heap and the row index leave. Both match on 292 of 292 real data pages.
- **`index_shift`** = 0x20 × slot for track, artist and album rows. The builder sets it,
  overwriting whatever the row held. It matches all 1,343 present rows.
- **Row footprints:**
  - Rows with one inline string (genre, label, key, colour, artwork): their length rounded
    up to 4.
  - Artists and albums: their length in the far layout, rounded up to 4, plus 4.
  - Tracks: 0x88 plus each string rounded up to 4, rounded up, plus 4.
  - A row longer than its rule is sized by its own length, so it never overlaps the next row.
- **Fresh page:**
  - Every row is present, in slot order from heap offset 0.
  - Zero padding, and a zero heap after `used_size`.
  - Groups of 16 row offsets; only the last group is partial.
  - Flags 0x24, and `unknown1` to `unknown4` are 0.
  - Whole-page transaction values: the count equals the number of rows, the index is 0, and
    `tranrf` equals `rowpf`.
- **Page breaks:** fill greedily until the next row's footprint plus its index entry no
  longer fits. There is no row-count limit, and a page may end with 0 bytes free.
- `Page::encode` rejects `num_row_offsets` above 13 bits and `num_rows` above 11 bits.
- The page writer is crate-private (`pdb::writer`). Its local fixture tests live in the
  crate (`src/pdb/writer/local.rs`), with the same skip and require-env behaviour as
  `tests/common`.
## Why
Rebuilding every typed real page without deleted rows from its rows gives identical bytes
for 79 of 91. The other 12 hold rows rekordbox rewrote in place. The stick was updated over
many sessions: 183 pages have deleted rows (flags 0x34), and many page breaks leave room
for the next row. Those breaks come from later updates, not from a layout rule.
## Alternatives considered
- **Copy rekordbox's page splits:** not possible for new content, and the splits on the
  fixture reflect its update history.
- **A public page builder:** nothing outside the crate needs one. The `Database` writer
  (7e) is the public entry point.
## Consequences
- 7d decides `sequence`, the `next_page` of each table's last page, `empty_candidate`, the
  index pages, the fixed tables and the file header.
- It is not yet verified whether a fresh rekordbox export also packs its pages full. A fresh
  export of the hardware playlist (slice 7h) answers that.
- 5 track rows on pages with deleted rows end with 4 more zero bytes than the rule
  predicts. They are unexplained and pinned in the test.
- Untyped tables (playlists, history, columns, tables 17 and 18) cannot be laid out until
  their rows are typed.

## Whole file (slice 7d)
The crate-private `write_pdb(len_page, rows)` builds a complete file. Everything below is
measured on the fixture (`docs/format-notes.md`).

- **Tables:** all 20, types 0 to 19 in type order, including the empty unnamed tables.
- **Layout:**
  - table t's index page is page 2t+1;
  - page 2t+2 is its first data page or, for an empty table, its zero-filled
    `empty_candidate`;
  - further data pages follow from page 41, table by table, each table's pages contiguous;
  - then each table with data pages gets an `empty_candidate` past the end of the file, in
    type order.
- **Chaining:** each page's `next_page` is the next page of its table. A table's last page
  points at its `empty_candidate`. A table pointer's first page is the index page, and its
  last page is the last data page (or the index page).
- **Index pages:** always empty, because a fresh file has no deleted rows:
  - flags 0x64;
  - a slot count derived from `len_page`;
  - every slot 0x1ffffff8.

  On the fixture, the generated index page equals the real one for all 8 tables without
  deleted rows.
- **Sequence:** index pages 1; data pages 2, 3, … in page order; the header's seqdb is the
  highest page sequence plus 1, as on the fixture.
- **Header:**
  - `unknown1` and `unknown3` are 0, and `unknown2` is 1;
  - the tail is zero;
  - `next_unused_page` is one past the file and every `empty_candidate`.

  Rebuilding the real file from its pages with these rules gives back every byte.
- **Fixed tables:**
  - Colours are rekordbox's 8 defaults unless the caller passes colour rows.
  - Columns are always rekordbox's 27 built-in UI labels.
  - Both are format constants, not library data.
- **Untyped tables** (playlists, history, 9, 10, 14, 15, 17, 18) are written empty, as an
  index page only.

### Checked on the fixture
The stick's 2,221 typed rows, written fresh, read back in order in all 8 typed tables.
`Database::from_pdb` loads the result equal to the stick's database, with 0 dangling
references. The fresh file has 144 pages; the stick has 331.

### For the hardware test (slice 7h)
- Does the player accept `empty_candidate`s and a `next_unused_page` past the end of the
  file? The fixture has 2 such candidates; ours has one per non-empty table.
- Does it need table 17 or 18? Their rows are not understood, and table 17 may hold user
  settings.
- Do empty playlist and history tables work?
- Does a fresh rekordbox export also pack pages full and number its sequences in order?
