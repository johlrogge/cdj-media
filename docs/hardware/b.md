# Hardware runs B and B2: a stick built from MDMA, no rekordbox

**Date:** 2026-11-20. **Player:** CDJ-2000NXS. **Stick:** `CDJ_RB_HW1`, wiped of `PIONEER/` and
`Contents/` first.

## Image B

`cargo run --example mdma_export` (develop `d42cc5d`) built image B from MDMA's export of
playlist `cdj-media-hw1`: 7 AIFF tracks, with grids and cues from MDMA's timeline JSON.
Everything on the stick is ours:
- `export.pdb`;
- one `ANLZ0000.DAT` per track, with a flat waveform placeholder, at ADR-010's id-derived
  folders `P00N/0000000N`;
- the audio files.

It has no `.EXT`, `exportExt.pdb`, `USBMNG.DAT` or setting files. `export.pdb` is 40 pages,
and its sha256 begins with `3f7cccb8c3ed8e89`.

| What | Result |
|---|---|
| Browse menus, playlist | Shown |
| 6 real tracks | Load and play |
| 1 s "M4A Test Track" | Doesn't load. MDMA's own test file is +21.7 ms (an MDMA bug, being fixed); not pursued. |
| Waveforms, BPM, cues | Missing: the NXS treated every track as unanalysed and analysed it itself |

**Write-back** (local fixture `hw-runs/b-nxs`):
- **Its own analysis:** for each track played, the NXS wrote its own `ANLZ0000.DAT` at a
  *hashed* folder (e.g. `P01B/000243B5`). Each one has PPTH, an empty PQTZ, its own
  PWAV/PWV2 and empty PCOBs.
- **New files:** `USBMNG.DAT`, `rekordbox/RBFLTR.DAT` and a `LOG/` json.
- **`export.pdb`:** only page 0 (the header) changed.
- **The same hash as rekordbox:** one track has the same path on both sticks, and the NXS
  chose the same folder rekordbox did.

## Image B2

Image B plus copies of our `.DAT`s at the hashed folders the NXS had chosen, for tracks 1,
3, 4, 5 (and 7). Tracks 2 and 6, which hadn't been played, kept only their id-derived
folders and served as the control.

| Tracks | Result |
|---|---|
| 1, 3, 4, 5 | Our BPM and grid; hot and memory cues where they should be; flat waveform (our placeholder) |
| 2, 6 | No BPM; the NXS started analysing |

## Image B3

Built the same way as image B, but with ADR-019's hashed analysis paths (feature branch
`anlz-path-hash`, commit `ff35bd3`). All 7 `.DAT`s are at their hashed folders. For the 5
tracks the NXS played in run B, those are exactly the folders it chose. Nothing is copied
from rekordbox or from the player.

**Result (NXS):** works as expected.
- All 6 real tracks show our BPM, grid and cues, with the flat waveform.
- No track was analysed on the player.

In run B2, the NXS had written its own analysis only for the two control tracks (local
fixture `hw-runs/b2-nxs`).

## What this settles

- **The NXS ignores slot 14.** It finds the analysis by hashing the audio path, which leads
  to ADR-019.
- **The NXS accepts all of our `.DAT`:** PPTH, the zero PVBR, PQTZ, PCOB and the flat
  PWAV/PWV2 placeholder.
- **Phase 1 works on the NXS:** a stick built only by cdj-media (image B3) works end to end.
- **None of the missing files are needed:** the NXS works without `.EXT`, `exportExt.pdb`,
  `USBMNG.DAT` or setting files.

## Still open

- **Grid bars:** MDMA's `first_beat_ms` isn't always the downbeat, so the bar counter
  should be off on 4 of 6 tracks. This hasn't been checked on the player.
- **A collision** (`ANLZ0001`) on hardware.
- **The CDJ-2000.**
