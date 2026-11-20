# ADR-020: .EXT file with PWV3

## Status

Accepted. This amends ADR-018's Phase 1 file list.

## Context

On image B4 (`docs/hardware/b.md`), the CDJ-2000NXS loads everything from our `.DAT` but
shows no large waveform. The NXS draws that waveform from PWV3 in the `.EXT` file, and we
don't write `.EXT` yet. The CDJ-2000 has no large waveform, so this only affects the NXS.

Survey of the fixtures (832 `.EXT` files):

- **Tag order:** `PPTH PWV3 PCOB PCOB PCO2 PCO2 [PQT2] PWV5 PWV4 [PSSI]`.
  - PCO2, PQT2, PWV4, PWV5 and PSSI are nexus2-era tags: colour waveforms, cue labels and
    colours, and the phrase analysis.
  - PQT2 is missing in 52 files, and PSSI is present in 157.
- **PPTH** is byte-identical to the `.DAT`'s in 832 of 832 files.
- **The two PCOBs** are always empty (`hot([])` and `memory([])`), in all 832 files. They
  equal the `.DAT`'s only when the track has no cues.
- **PWV3** has a 24-byte header. After the tag header come:
  - the entry size (1);
  - the entry count;
  - a u16 of 0x0096, which is 150 entries per second;
  - a u16 of 0.
- **The PMAI header** has one modal variant (823 of 826 on the old stick, 6 of 6 on hw1).

## Decision

### The file

The builder writes one `.EXT` per track, next to its `.DAT`: the same hashed folder and the
same number, with extension `.EXT` (ADR-019).

Its tags, in rekordbox's order, are `PPTH PWV3 PCOB(hot) PCOB(memory)`:
- PPTH is the same bytes as in the `.DAT`;
- PWV3 is the detail waveform at 150 entries per second;
- both PCOBs are empty, as rekordbox writes them. The cues live in the `.DAT`, and run V0
  showed the NXS reading them from there while rekordbox's `.EXT` had empty PCOBs;
- the header is the modal PMAI header, the same bytes as the `.DAT`'s.

### PWV3

- **Entries:** one byte per entry, with height and whiteness packed as in djl-analysis.
- **Count:** `ceil(ms · 3 / 20)`, which is 150 per second rounded up. It is exact against
  the AIFF frame counts of all 6 hw1 tracks.
- **Content:** a new `WaveformDetail` input. `Waveform::Flat` gives a flat detail too, with
  rekordbox's silence value `0xe0` (height 0, whiteness 7). As with the overview, the
  placeholder is what rekordbox writes for silence.
  `Waveform::Previews` gains a detail field, so the fixture rebuild can pass rekordbox's.
- **Envelope:** MDMA's 150-per-second envelope is still deferred, but it maps one entry to
  one entry onto PWV3.

### Left out of Phase 1

PCO2, PQT2, PWV4, PWV5 and PSSI. They are needed only for nexus2 colour features, and
leaving them out matches the deferral of cue labels and colours.

## Why

- **The NXS's large waveform:** writing PWV3 restores it, and with it the red downbeat lines
  DJs use to check the grid.
- **Empty PCOBs:** writing them empty is exactly what rekordbox does, and the hardware has
  shown it works.

## Alternatives considered

- **Every rekordbox tag:** most need data we don't have (colour waveforms, phrase
  analysis). Writing them as placeholders invites players to show wrong colours or
  phrases.
- **Copying the `.DAT`'s cues into the `.EXT` PCOBs:** rekordbox never does this, and no
  player is known to need it.

## Consequences

- **Acceptance test:** the fixture rebuild also compares the `.EXT`. Expect it to equal
  rekordbox's up to the tags we leave out: compare the prefix
  `PPTH PWV3 PCOB PCOB`, and the PMAI header apart from the total length, on 6 of 6.
- **Open finding** (format notes): rekordbox's row duration may not be `floor(ms/1000)` for
  whole-second lengths. This is not addressed here.
- **Hardware:** an image B5 should show the large waveform (flat) on the NXS, and nothing
  should change on the CDJ-2000.
