# Vision

## North star

cdj-media lets developers read and write Pioneer CDJ media over USB without involving
rekordbox.

## Who it's for

Developers who want to read and write Pioneer media. It is a library, nothing more, nothing
less.

It was developed as part of the
[modular-digital-music-array](https://github.com/johlrogge/modular-digital-music-array)
project, but it stands on its own.

Known and likely consumers:

- **modular-digital-music-array:** prepares USB sticks for CDJs and reads back what they
  wrote.
- **prodj** (a Pro DJ Link sniffer): may depend on cdj-media in the future to parse Pioneer
  media it reaches through the players.

## Why it exists

rekordbox is a locked-in system. It only runs on macOS and Windows, and it is a poor piece
of software that you are forced to use if you want to use Pioneer hardware. cdj-media
removes that dependency.

## What success looks like

- **Write:** we prepare a USB stick that a CDJ-2000 and a CDJ-2000NXS accept and play.
- **Read back:** we read everything a CDJ writes to the stick while playing from it:
  - songs played, as a play session (history)
  - anything marked on the CDJ
  - anything deleted on the CDJ

  This lets what happened at the player flow back into the developer's own library.

Later players and formats (`.EXT` colour waveforms, OneLibrary) are on the roadmap and are
not part of the first measure of success.

## Principles

- **Clean-room and MIT.** Deep Symmetry's djl-analysis is the format reference. Code from
  GPL projects such as rbox is never copied. rekordcrate (MIT) may be used only as a
  dev-dependency, to cross-check output.
- **Callers supply the analysis.** cdj-media encodes and decodes Pioneer's formats.
  Beatgrids, cues, waveform envelopes and artwork come from the caller.
- **The hardware is the judge.** Output counts as correct when a real player accepts it.
  Matching a spec or another tool's output is not enough.
- **Files, not transport.** cdj-media works on bytes and paths. It doesn't care whether
  they came from a USB stick or were fetched from a player over the network.
- **No audio in the repository.** Test fixtures are metadata and analysis output only.

## Non-goals

It is just a library. That means:

- no GUI, CLI or end-user application
- no audio decoding, no audio analysis, no image processing
- no library management (organising collections, playlists as a user feature, tagging)
- no dependency on rekordbox, at runtime or for producing exports
- no networking: no Pro DJ Link, and no fetching media from players over NFS. That belongs
  in consumers such as prodj.
