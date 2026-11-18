//! Helpers shared by integration tests.

use std::path::{Path, PathBuf};

/// Set to `1` to make a missing local fixture fail the test instead of skipping it.
const REQUIRE_ENV: &str = "CDJ_MEDIA_REQUIRE_LOCAL_FIXTURES";

/// Resolves `rel` under `local-fixtures/` at the crate root.
///
/// Returns `None`, with a note on stderr, when the path does not exist. Local fixtures are
/// real sticks that are never committed (see ADR-003), so tests that need them call this
/// and return early on `None`.
///
/// # Panics
///
/// Panics instead of returning `None` when `CDJ_MEDIA_REQUIRE_LOCAL_FIXTURES=1` is set, so a
/// machine that is expected to have the fixtures cannot silently skip them.
pub fn local_fixture(rel: &str) -> Option<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("local-fixtures").join(rel);
    if path.exists() {
        return Some(path);
    }
    if std::env::var(REQUIRE_ENV).is_ok_and(|v| v == "1") {
        panic!(
            "local fixture {} not found and {REQUIRE_ENV}=1",
            path.display()
        );
    }
    eprintln!("skipping: local fixture {} not found", path.display());
    None
}
