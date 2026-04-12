use agwiki::toolkit::{expand_ingest_prompt, require_wiki_ingest_prompt, wiki_ingest_prompt_path};
use std::fs;
use tempfile::tempdir;

#[test]
fn require_ingest_prompt_errors_when_missing() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    assert!(require_wiki_ingest_prompt(root).is_err());
}

#[test]
fn require_ingest_prompt_ok_when_present() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::write(root.join("ingest.md"), "x").unwrap();
    let p = require_wiki_ingest_prompt(root).unwrap();
    assert_eq!(p, wiki_ingest_prompt_path(root));
}

#[test]
fn expand_replaces_ingest_path_and_wiki_root() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::write(
        root.join("ingest.md"),
        "src={{INGEST_PATH}} root={{WIKI_ROOT}}",
    )
    .unwrap();
    let prompt_path = require_wiki_ingest_prompt(root).unwrap();
    let ingest = root.join("raw/x.md");
    fs::create_dir_all(root.join("raw")).unwrap();
    fs::write(&ingest, "x").unwrap();
    let root_abs = root.canonicalize().unwrap();
    let ingest_abs = ingest.canonicalize().unwrap();
    let out = expand_ingest_prompt(&root_abs, &ingest_abs, &prompt_path).unwrap();
    assert!(out.contains(ingest_abs.to_str().unwrap()));
    assert!(out.contains(root_abs.to_str().unwrap()));
}
