use agwiki::export_skill::{run_export, ExportOptions};
use std::fs;
use tempfile::tempdir;

#[test]
fn export_skill_mirror_and_skill_md() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki/concepts")).unwrap();
    fs::create_dir_all(root.join("skill")).unwrap();
    fs::write(
        root.join("skill/SKILL.md.template"),
        "---\nname: skill\ndescription: Test wiki skill export fixture.\n---\n\n# Test skill\n\nStatic body.\n",
    )
    .unwrap();
    fs::write(
        root.join("wiki/index.md"),
        "## Concepts\n\n- [[concepts/hello|Hello page]]\n\n",
    )
    .unwrap();
    fs::write(root.join("wiki/concepts/hello.md"), "# Hello\n").unwrap();

    run_export(ExportOptions {
        wiki_root: root,
        skill_dir: None,
        template: None,
        output: None,
        index: None,
        sections: "concepts,topics,projects",
        dry_run: false,
        prune: false,
        rewrite_wikilinks: false,
    })
    .unwrap();

    let skill_md = fs::read_to_string(root.join("skill/SKILL.md")).unwrap();
    assert!(skill_md.contains("references/concepts/hello.md"));
    assert!(root.join("skill/references/concepts/hello.md").is_file());
}
