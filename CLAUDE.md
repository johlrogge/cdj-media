# CLAUDE.md

## Project Overview

cdj-media is a Rust library for reading and writing Pioneer CDJ/rekordbox USB exports
(`export.pdb`, ANLZ) without rekordbox. Early work.

**Scope:** Pioneer's on-stick formats: DeviceSQL `export.pdb` pages, ANLZ tags
(`.DAT`/`.EXT`), Pioneer's encodings (waveform bytes, key table, string forms), and the
`PIONEER/` and `/Contents/Artist/Album` path layout.

**Boundary:** no audio decoding, no analysis, no image processing, no networking (Pro DJ
Link and NFS belong to consumers such as `../prodj`). Callers supply beatgrids, cues,
waveform envelopes and JPEG artwork. See `VISION.md`.

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

## Workflow

Delegation is a hard constraint, not a preference:

- **architect** designs and reviews; **code-minion** implements and writes tests.
- The orchestrating session does not write source code under `src/` directly.
- **commit** stages and commits; **release-manager** owns git flow branches and releases.
  Pushing stays with the human unless they ask for it.
- Work happens on `feature/*` branches off `develop`.

## Environment

All tooling comes from `devenv.nix`. Never use `cargo install`, `rustup`, `pip`, `npm -g`,
system package managers or any other imperative installer. If a tool is missing, add it
to `devenv.nix` and say so.

## Tool Usage Policy

- Prefer MCP tools (`rust-codebase`, `git-read`/`git-write`, `git-flow`, `just`) over Bash.
- If you fall back to Bash, say why.
- If an MCP tool is broken, report it. Never silently work around it.

## Commands

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt` (rustfmt and clippy also run as devenv git hooks)

There is no `justfile` yet. Add recipes here when one exists.
