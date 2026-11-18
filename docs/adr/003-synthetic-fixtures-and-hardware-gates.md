# ADR-003: Synthetic committed fixtures, local real-stick fixtures, hardware gates
## Status
Proposed
## Decision
- **Committed fixtures** are synthetic: made-up metadata cases (artists, titles, cues,
  beatgrids, including non-ASCII), built from what we learn reading real exports. Only
  `PIONEER/`-style metadata and analysis output, never audio.
- **Local fixtures** under `/local-fixtures/` (gitignored) hold real rekordbox- and
  CDJ-written sticks. They contain the user's real library and are never committed. Tests
  that use them are skipped when the folder is missing, so CI passes without them.
- A test rejects audio in `tests/fixtures` by extension and by magic bytes (ID3,
  RIFF/WAVE, fLaC, ftyp, OggS, MPEG frame sync).
- Slices that write output have a manual hardware test on a CDJ-2000 and a CDJ-2000NXS,
  recorded in the PR.
- Clean-room sources: djl-analysis prose and diagrams only; no GPL or EPL code (rbox,
  Mixxx, Crate Digger `.ksy`); rekordcrate (MIT) as a dev-dependency only.
## Why
"The hardware is the judge", the no-audio rule, the user's library staying private, and the
crate's MIT clean-room status.
## Consequences
- Real-world coverage depends on local fixtures that CI doesn't have.
- Hardware tests can't be automated.
