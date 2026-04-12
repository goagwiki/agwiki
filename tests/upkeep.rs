use agwiki::upkeep::{check_links, list_orphans};
use std::fs;
use tempfile::tempdir;

#[test]
fn check_links_reports_broken_wikilink() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::write(root.join("wiki/a.md"), "see [[missing-page]]\n").unwrap();

    let errs = check_links(root).unwrap();
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("broken wikilink"));
}

#[test]
fn orphans_lists_unlinked_page() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::write(root.join("wiki/index.md"), "# Index\n").unwrap();
    fs::write(root.join("wiki/orphan.md"), "# Orphan\n").unwrap();

    let o = list_orphans(root).unwrap();
    let names: Vec<_> = o
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(names.contains(&"orphan.md".to_string()));
}
