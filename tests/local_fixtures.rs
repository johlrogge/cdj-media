//! Smoke tests over real sticks in `local-fixtures/`. Each test skips itself when its
//! fixture is missing.

mod common;

#[test]
fn real_stick_export_pdb_is_whole_pages() {
    let Some(path) = common::local_fixture("real-stick/PIONEER/rekordbox/export.pdb") else {
        return;
    };
    let bytes = std::fs::read(&path).expect("read export.pdb");
    let name = path.display();
    let len = bytes.len();
    assert!(len > 0, "{name} is empty");

    // `len_page` is the little-endian u32 at offset 4 of the file header.
    let field = bytes.get(4..8).expect("header has a len_page field");
    let len_page = u32::from_le_bytes(field.try_into().expect("four bytes"));
    let len_page = usize::try_from(len_page).expect("len_page fits usize");
    assert!(len_page > 0, "{name}: len_page is zero");
    assert_eq!(
        len % len_page,
        0,
        "{name}: {len} is not whole {len_page}-byte pages"
    );
}
