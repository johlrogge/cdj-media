# ADR-005: DeviceSQL strings keep their form in the raw layer
## Status
Accepted
## Decision
Refines ADR-002 for `export.pdb` strings, as ADR-004 does for ANLZ tags.
- **Raw layer:** rows hold `DeviceSqlString`, which keeps the on-stick form (short ASCII,
  long ASCII, UTF-16LE, ISRC) and the long-form pad byte, so a re-encode gives identical
  bytes whoever wrote the file. Equality includes the form.
- **Model layer:** `Database` holds `String` (and `Option<String>` for ISRC) and lowers
  through `DeviceSqlString::new`, which picks rekordbox's form: ASCII of at most 126 bytes
  → short, longer ASCII → long ASCII, anything non-ASCII → UTF-16LE. This matches every
  one of 17,966 strings in the real-stick fixture.
- **ISRC is decided by slot, never by content.** The track row's first string slot is
  decoded as ISRC; elsewhere `0x90` is always UTF-16LE. Content guessing is ambiguous
  (`"\u{103}a"` in UTF-16LE has the ISRC shape).
- **Strictness:** store bytes that are not understood (the pad byte); reject only what the
  type cannot represent (non-ASCII in an ASCII form, invalid UTF-16, unknown flags).
## Why
CDJ- or third-party-written files may use non-rekordbox forms or pad values, and loading
them must not fail or lose bytes. Building new exports must look like rekordbox's.
## Alternatives considered
- **Normalise strings to `String` in the raw layer:** loses the form, so files not written
  by rekordbox would not round-trip.
- **Guess ISRC from content:** ambiguous, see above.
- **Reject a non-zero pad byte:** contradicts ADR-002; one odd string would make the whole
  `export.pdb` unreadable.
## Consequences
- The byte-level entry points on `DeviceSqlString` become crate-private once the pdb codec
  lands (slice 4).
- Typed rows must also reproduce the string heap layout (order and gaps) to round-trip;
  slice 4 checks that against the fixture.
