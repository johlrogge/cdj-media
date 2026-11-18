# CLAUDE.md

## Project Overview

cdj-media is a Rust library for reading and writing Pioneer CDJ/rekordbox USB exports
(`export.pdb`, ANLZ) without rekordbox. Early work.

**Scope:** Pioneer's on-stick formats: DeviceSQL `export.pdb` pages, ANLZ tags
(`.DAT`/`.EXT`), Pioneer's encodings (waveform bytes, key table, string forms), and the
`PIONEER/` and `/Contents/Artist/Album` path layout.

**Boundary:** no audio decoding, no analysis, no image processing. Callers supply
beatgrids, cues, waveform envelopes and JPEG artwork.

**References:** Deep Symmetry's djl-analysis is the format reference. rekordcrate (MIT) may
be a dev-dependency for cross-checking. Never copy code from GPL projects such as rbox.

**Phase 1:** CDJ-2000 / CDJ-2000NXS: `export.pdb` plus `.DAT` (PQTZ, PCOB, PWAV/PWV2,
PPTH), plus PWV3 if the NXS reads it. Later: `.EXT` colour waveforms, then OneLibrary.

## Rules

- **Never commit audio files.** Test fixtures are metadata and analysis output exported by
  rekordbox from real tracks, never the audio itself.

## Conventions

metadev conventions apply: git flow, conventional commits, and devenv-only tooling
(declare tools in `devenv.nix`, never install imperatively).
