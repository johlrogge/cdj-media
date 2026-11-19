# ADR-012: Fresh export.pdb page layout
## Status
Accepted (data pages). Page chaining, index pages and the file header follow in slice 7d.
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
