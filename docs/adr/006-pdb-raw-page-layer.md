# ADR-006: export.pdb raw page layer is schema-agnostic
## Status
Accepted
## Decision
Refines ADR-002 for DeviceSQL databases (`export.pdb`, `exportExt.pdb`).
- **`Pdb` is schema-agnostic.** It decodes the file header, table pointers, page chains,
  data pages (heap, row groups, presence flags) and index pages (verbatim), and re-encodes
  an unchanged file to identical bytes. Pages that belong to no chain are kept as raw
  bytes. Table and page types are raw `u32`s at this layer.
- **The schema is chosen where rows are typed.** Each database has its own table enum
  (`ExportTable` for `export.pdb`; `exportExt.pdb` gets its own in slice 5), because the
  two files reuse type numbers for different tables. `Pdb::pages_of(&TablePointer)` is the
  unambiguous primitive.
- **Rows carry their absolute file position**, so string and typed-row decode errors keep
  ADR-001's exact absolute offsets even when rows are typed in a later pass.
- **`free_size` and `used_size` are stored, not derived.** On every real data page,
  `free_size = len_page − 0x28 − row_index_len(num_row_offsets) − used_size`, but that's
  one fixture from one writer, and a check on decode could make a CDJ-modified stick
  unreadable. The slice 7 writer computes them, with one crate-private function that a
  local test checks against every real page. `used_size` is the heap high-water mark,
  including deleted rows, so it can't be derived from rows alone.
- The row-index layout follows `docs/format-notes.md`: only the last row group is
  partial, which differs from djl-analysis.
## Why
One page codec serves every DeviceSQL file. Choosing the schema at `from_bytes` (for
example a `Pdb<Schema>` parameter) buys nothing at the raw layer, and names like `Tracks`
would be wrong for `exportExt.pdb`.
## Alternatives considered
- **Name the tables in the raw layer:** wrong for `exportExt.pdb`.
- **Derive and check `free_size`/`used_size` on decode:** not proven across writers.
## Consequences
- Slice 7 builds pages through a builder that enforces at most 16 offsets per group, only
  the last group short, `num_row_offsets` derived from the groups, and the index flag
  agreeing with the page body. Encoding returns `EncodeError` for packed-field overflow.
- Slice 5 types rows from `RawRow` and takes over the string heap layout checks.
