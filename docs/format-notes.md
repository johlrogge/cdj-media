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
- `exportExt.pdb` uses the same page framing, with its own table numbering (types 0–8).
