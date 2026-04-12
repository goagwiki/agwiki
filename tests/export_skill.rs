use agwiki::export_skill::{
    merge_skill_generated_index, run_export, wiki_mirror_sections, ExportOptions,
    GENERATED_INDEX_END, GENERATED_INDEX_START,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn export_skill_mirror_and_skill_md() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki/concepts")).unwrap();
    fs::create_dir_all(root.join("skill")).unwrap();
    fs::write(
        root.join("wiki/index.md"),
        "## Concepts\n\n- [[concepts/hello|Hello page]]\n\n",
    )
    .unwrap();
    fs::write(root.join("wiki/concepts/hello.md"), "# Hello\n").unwrap();

    run_export(ExportOptions {
        wiki_root: root,
        skill_root: None,
        skill_md: None,
        dry_run: false,
        prune: false,
    })
    .unwrap();

    let skill_md = fs::read_to_string(root.join("skill/SKILL.md")).unwrap();
    assert!(skill_md.contains(GENERATED_INDEX_START));
    assert!(skill_md.contains(GENERATED_INDEX_END));
    assert!(skill_md.contains("references/concepts/hello.md"));
    assert!(root.join("skill/references/concepts/hello.md").is_file());
}

#[test]
fn wiki_mirror_sections_lists_sorted_dirs() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki/zebra")).unwrap();
    fs::create_dir_all(root.join("wiki/apple")).unwrap();
    let s = wiki_mirror_sections(root).unwrap();
    assert_eq!(s, vec!["apple", "zebra"]);
}

#[test]
fn merge_skill_replaces_marked_block() {
    let existing = format!(
        "front\n{}\n{}\n{}\ntail\n",
        GENERATED_INDEX_START, "## Wiki index\n\n- x", GENERATED_INDEX_END
    );
    let gen = "## Wiki index\n\n- new";
    let out = merge_skill_generated_index(&existing, gen).unwrap();
    assert!(out.contains("- new"));
    assert!(!out.contains("- x"));
    assert!(out.contains("front"));
    assert!(out.contains("tail"));
}

#[test]
fn merge_skill_errors_on_end_without_start() {
    let r = merge_skill_generated_index(
        &format!("x\n{}\n", GENERATED_INDEX_END),
        "## Wiki index",
    );
    assert!(r.is_err());
}

#[test]
fn merge_skill_appends_when_no_markers() {
    let out = merge_skill_generated_index("# Title\n", "## Wiki index\n\n- a").unwrap();
    assert!(out.starts_with("# Title"));
    assert!(out.contains(GENERATED_INDEX_START));
    assert!(out.ends_with('\n'));
}

#[test]
fn export_ok_when_wiki_has_broken_link() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki/concepts")).unwrap();
    fs::create_dir_all(root.join("skill")).unwrap();
    fs::write(root.join("wiki/index.md"), "# I\n").unwrap();
    fs::write(root.join("wiki/concepts/a.md"), "broken [[missing]]\n").unwrap();

    run_export(ExportOptions {
        wiki_root: root,
        skill_root: None,
        skill_md: None,
        dry_run: false,
        prune: false,
    })
    .unwrap();
}
