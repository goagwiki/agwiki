use agwiki::prep::prep;
use std::fs;
use tempfile::tempdir;

#[test]
fn prep_resolves_md_path() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    let f = root.join("note.md");
    fs::write(&f, "x").unwrap();
    let out = prep(root, &f, false).unwrap();
    assert!(out.is_absolute());
    assert!(out.ends_with("note.md"));
}

#[test]
fn prep_raw_only_rejects_outside_raw() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::create_dir_all(root.join("raw")).unwrap();
    let f = root.join("note.md");
    fs::write(&f, "x").unwrap();
    assert!(prep(root, &f, true).is_err());
}

#[test]
fn prep_raw_only_accepts_under_raw() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::create_dir_all(root.join("raw")).unwrap();
    let f = root.join("raw/note.md");
    fs::write(&f, "x").unwrap();
    assert!(prep(root, &f, true).is_ok());
}
