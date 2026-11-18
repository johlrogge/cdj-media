# ADR-002: Lossless round-trip with separate raw and model layers
## Status
Accepted
## Decision
Two layers:
- **Raw layer** (`Pdb`, `AnlzFile` with `Tag`, `RawTag`, `RawRow`) mirrors the format and
  keeps every unknown field and tag verbatim. Re-encoding an unchanged value gives back
  identical bytes.
- **Model layer** (`Database`, typed tags) holds the domain records with resolved IDs. It
  is built from the raw layer and lowered to it. `Database::to_pdb()` lays out pages fresh.
  Rekordbox's default values for unknown fields live in one module, `pdb::defaults`, and
  each cites the fixture it came from.
## Why
- Identical-bytes round-trips are the strongest automated proxy for "the player accepts it".
- Files the CDJ has modified contain fields we may not understand, and we must not lose them.
- Debugging the model is easier once the raw layer is known to be correct.
## Consequences
- Raw structs are `#[non_exhaustive]` with `unknown_*` fields, so fields can be renamed as
  they become understood without breaking callers.
- An identical-bytes test runs for every fixture.
