# Roadmap

## Current milestone

**Phase 1: CDJ-2000 / CDJ-2000NXS support**

Read and write a USB export that these players accept, without rekordbox:

- [ ] `export.pdb` (DeviceSQL pages)
- [ ] `.DAT` ANLZ tags: PQTZ (beatgrid), PCOB (cues), PWAV / PWV2 (waveform previews), PPTH (path)
- [ ] PWV3, if the NXS reads it
- [ ] Pioneer encodings: waveform bytes, key table, string forms
- [ ] `PIONEER/` and `/Contents/Artist/Album` path layout

Fixtures are metadata and analysis output exported by rekordbox from real tracks,
never audio. Cross-check against Deep Symmetry's djl-analysis (and rekordcrate as a
dev-dependency, MIT only).

## Backlog

1. `.EXT` colour waveforms
2. OneLibrary

## Done

<!-- Completed milestones -->
