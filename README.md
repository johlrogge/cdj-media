# cdj-media

Read and write Pioneer CDJ/rekordbox USB exports (`export.pdb`, ANLZ) without rekordbox.

**Status: early work.** Nothing is implemented yet and the API will change.

## Scope

This crate owns Pioneer's on-stick formats: DeviceSQL `export.pdb` pages, ANLZ tags
(`.DAT`/`.EXT`), Pioneer's encodings (waveform bytes, key table, string forms), and the
`PIONEER/` and `/Contents/Artist/Album` path layout.

It has no audio decoding, no analysis and no image processing. Callers supply beatgrids,
cues, waveform envelopes and JPEG artwork.

## References

- Format reference: Deep Symmetry's djl-analysis
  (<https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/>).
- rekordcrate (MIT) may be used as a dev-dependency to cross-check output.
- Code from GPL projects such as rbox is not copied.

## Roadmap

- Phase 1: CDJ-2000 and CDJ-2000NXS. `export.pdb` plus `.DAT` (PQTZ, PCOB, PWAV/PWV2,
  PPTH), plus PWV3 if the NXS reads it.
- Later: `.EXT` colour waveforms, then OneLibrary.

## Test fixtures

Fixtures are metadata and analysis output exported by rekordbox from real tracks. Audio is
never committed.

## License

MIT, see [LICENSE](LICENSE).
