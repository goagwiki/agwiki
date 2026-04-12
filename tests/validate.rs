use agwiki::validate::ValidationReport;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

#[test]
fn validate_json_includes_broken_link_and_orphan_kinds() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::write(root.join("wiki/index.md"), "# Index\n").unwrap();
    fs::write(root.join("wiki/broken.md"), "see [[nowhere]]\n").unwrap();
    fs::write(root.join("wiki/orphan.md"), "# Orphan\n").unwrap();

    let report = ValidationReport::run(root).unwrap();
    assert!(!report.is_clean());

    let v: Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    assert!(v.get("wiki_root").is_some());
    let problems = v["problems"].as_array().unwrap();
    let kinds: Vec<_> = problems.iter().filter_map(|p| p["kind"].as_str()).collect();
    assert!(kinds.contains(&"broken_link"));
    assert!(kinds.contains(&"orphan"));
}

#[test]
fn validate_clean_wiki_json_empty_problems() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::write(root.join("wiki/index.md"), "# Index\n").unwrap();

    let report = ValidationReport::run(root).unwrap();
    assert!(report.is_clean());
    let v: Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    assert_eq!(v["problems"].as_array().unwrap().len(), 0);
}
