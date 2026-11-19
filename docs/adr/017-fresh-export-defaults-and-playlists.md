# ADR-017: Fresh-export defaults and playlists
## Status
Accepted. Amends ADR-012 and ADR-016.
## Context
A fresh rekordbox export of one 6-track playlist (local fixture `rekordbox-hw1`, verified
on CDJs) differs from the long-lived stick that ADR-012 was measured on. See
`docs/format-notes.md`, "Fresh export (rekordbox-hw1)".
## Decision
### export.pdb (amends ADR-012)
- **Header `unknown2`:** 5, as in the fresh export and its `exportExt.pdb`. Decoding keeps
  any value; the old stick has 1.
- **Tables 17 and 18:** fixed tables like colours and columns, always written from the fresh
  export's rows: 22 and 17 rows of 8 bytes, one page each.
  - The rows hold no strings. Each one is led by a distinct column id (1–27), so they look
    like browse-menu settings rather than library data.
  - The built pages equal the fresh export's apart from links and sequence. Table 18 also
    equals the old stick's.
- **Table 19 and the history tables** stay empty until the hardware test decides.
- **Playlist tree and playlist entries** are typed tables:
  - **Tree row:** parent, unknown, sort order, id, folder flag, then the name as a
    DeviceSqlString at 0x14.
  - **Entry row:** entry index, track id, playlist id (12 bytes).
  - **Layout:** both footprints are the row's length rounded up to 4. Neither has
    `index_shift`.
  - **Round trip:** all 935 rows on the old stick and 7 on the fresh export decode and
    encode back byte for byte.
### Database (amends ADR-016)
- **Contents:** playlists and folders (id, parent, sort order, folder flag, name), plus each
  playlist's tracks in entry order. Entries are kept apart from the tree, so entries of a
  missing playlist still load and get reported.
- **Building:** `add_playlist(name, parent)`, `add_folder(name, parent)` and
  `add_to_playlist(id, track)`.
  - Playlists and folders share one id sequence, last + 1.
  - A new node's sort order is its parent's last + 1, and 0 for the first child.
  - The tree `unknown` is 0 and the folder flag is 1. The folder flag is unverified, since
    neither fixture has a folder.
  - `to_pdb` writes entry indices 1..n per playlist, so gaps aren't kept.
- **Strictness:**
  - **Reading is lenient.** An entry naming track or playlist 0 or a missing record loads,
    and `dangling_references()` reports it. A 0 reference has no `RecordId`, so
    `Dangling.id` is `Option<RecordId>`; `playlist_tracks` skips 0 tracks.
  - **Writing is strict.** `to_pdb` refuses dangling references. A parent that exists but
    isn't a folder is rejected by `add_playlist`/`add_folder`, and by `to_pdb` for loaded
    databases.
  - **Duplicates.** A duplicate playlist id or (playlist, entry index) is `DuplicateId`.
## Why
Rebuilding the fresh export's playlist through the public API gives rekordbox's tree and
entry rows byte for byte. Both fixtures' databases survive `to_pdb` unchanged, playlists
included.
## Consequences
- Table 17 is always the fresh export's, even for a database loaded from a stick whose table
  17 differs (9 rows on the old stick). Lossless editing of a foreign stick goes through the
  raw `Pdb` (ADR-009).
- The hardware test still has to decide:
  - whether the history row matters;
  - whether whole-page transaction values are accepted;
  - track `unknown2` and string slots 2–4;
  - the folder flag.
