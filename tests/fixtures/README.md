# Test fixtures

This directory holds the fixtures that are committed to the repository. The policy is set
by [ADR-003](../../docs/adr/003-synthetic-fixtures-and-hardware-gates.md).

## Committed fixtures (this directory)

- **Synthetic metadata only.** Fixtures here are made-up cases (artists, titles, cues,
  beatgrids, including non-ASCII text), built from what we learn reading real exports.
- **Only `PIONEER/`-style metadata and analysis output**, such as `export.pdb` and ANLZ
  `.DAT`/`.EXT` files.
- **Never audio.** No audio files, not even short or silent ones.
  `tests/no_audio.rs` fails the build if any file here has an audio extension or starts
  with audio magic bytes.
- **Never real library data.** Nothing exported from anyone's real collection is
  committed, even if it contains no audio.

## Local fixtures (`local-fixtures/`, never committed)

Real sticks written by rekordbox or a CDJ go in `local-fixtures/` at the repository root,
which is gitignored. For example:

```text
local-fixtures/real-stick/PIONEER/rekordbox/export.pdb
local-fixtures/real-stick/PIONEER/USBANLZ/...
```

They contain a real library and must never be committed. Tests that use them resolve
paths with `local_fixture` in `tests/common/mod.rs` and skip themselves when the file is
missing, so CI passes without them. Set `CDJ_MEDIA_REQUIRE_LOCAL_FIXTURES=1` to make a
missing local fixture fail the test instead.
