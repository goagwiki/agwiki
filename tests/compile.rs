//! Integration coverage for ontology sources, compile, validation, and HTML export.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn init_wiki(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Command::cargo_bin("agwiki")?
        .arg("init")
        .arg(root)
        .assert()
        .success();
    Ok(())
}

fn write_entity(
    root: &Path,
    kind: &str,
    file: &str,
    id: &str,
    title: &str,
    extra_front_matter: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = root.join("content").join(kind);
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join(file),
        format!(
            "---\nid: \"{id}\"\ntitle: \"{title}\"\nschema_version: 1\n{extra_front_matter}---\n\n{body}\n"
        ),
    )?;
    Ok(())
}

#[test]
fn new_command_creates_entity_source() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path().join("wiki");
    init_wiki(&root)?;

    Command::cargo_bin("agwiki")?
        .args(["new", "concepts", "--title", "Knowledge Graphs"])
        .arg("-C")
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("content/concepts"));

    let entries = fs::read_dir(root.join("content/concepts"))?.count();
    assert_eq!(entries, 1);
    Ok(())
}

#[test]
fn compile_happy_path_writes_wiki_and_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path().join("wiki");
    init_wiki(&root)?;
    write_entity(
        &root,
        "concepts",
        "a.md",
        "01HZABC123DEF456GHI789JKL0",
        "First Concept",
        "",
        "# First Concept\n\nBody.",
    )?;

    Command::cargo_bin("agwiki")?
        .arg("compile")
        .arg("-C")
        .arg(&root)
        .assert()
        .success();

    assert!(root.join("wiki/concepts/first-concept.md").is_file());
    let index = fs::read_to_string(root.join("wiki/index.md"))?;
    assert!(index.contains("<!-- agwiki:compile-index -->"));
    assert!(index.contains("wiki/concepts/first-concept.md"));
    Ok(())
}

#[test]
fn compile_dry_run_does_not_write() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path().join("wiki");
    init_wiki(&root)?;
    write_entity(
        &root,
        "concepts",
        "a.md",
        "01HZABC123DEF456GHI789JKL0",
        "Dry Run Concept",
        "",
        "Body.",
    )?;

    Command::cargo_bin("agwiki")?
        .args(["compile", "--dry-run"])
        .arg("-C")
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("would write"));

    assert!(!root.join("wiki/concepts/dry-run-concept.md").exists());
    Ok(())
}

#[test]
fn compile_duplicate_id_fails() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path().join("wiki");
    init_wiki(&root)?;
    write_entity(
        &root,
        "concepts",
        "a.md",
        "01HZABC123DEF456GHI789JKL0",
        "A",
        "",
        "Body.",
    )?;
    write_entity(
        &root,
        "topics",
        "b.md",
        "01HZABC123DEF456GHI789JKL0",
        "B",
        "",
        "Body.",
    )?;

    Command::cargo_bin("agwiki")?
        .arg("compile")
        .arg("-C")
        .arg(&root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("E011"));
    Ok(())
}

#[test]
fn compile_broken_relation_fails() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path().join("wiki");
    init_wiki(&root)?;
    write_entity(
        &root,
        "concepts",
        "a.md",
        "01HZABC123DEF456GHI789JKL0",
        "A",
        "relations:\n  - target: \"01HZMISSING000000000000000\"\n    rel: \"related-to\"\n",
        "Body.",
    )?;

    Command::cargo_bin("agwiki")?
        .arg("compile")
        .arg("-C")
        .arg(&root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("E012"));
    Ok(())
}

#[test]
fn compile_unknown_kind_fails() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path().join("wiki");
    init_wiki(&root)?;
    write_entity(
        &root,
        "experiments",
        "a.md",
        "01HZABC123DEF456GHI789JKL0",
        "Experiment",
        "",
        "Body.",
    )?;

    Command::cargo_bin("agwiki")?
        .arg("compile")
        .arg("-C")
        .arg(&root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("E009"));
    Ok(())
}

#[test]
fn compile_slug_collision_disambiguates_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path().join("wiki");
    init_wiki(&root)?;
    write_entity(
        &root,
        "concepts",
        "a.md",
        "01HZABC123DEF456GHI789JKL0",
        "Same Title",
        "",
        "Body A.",
    )?;
    write_entity(
        &root,
        "concepts",
        "b.md",
        "01HZABC123DEF456GHI789JKL1",
        "Same Title",
        "",
        "Body B.",
    )?;

    Command::cargo_bin("agwiki")?
        .arg("compile")
        .arg("-C")
        .arg(&root)
        .assert()
        .success()
        .stderr(predicate::str::contains("E014"));

    assert!(root.join("wiki/concepts/same-title.md").is_file());
    assert!(root.join("wiki/concepts/same-title-2.md").is_file());
    let index = fs::read_to_string(root.join("wiki/index.md"))?;
    assert!(index.contains("wiki/concepts/same-title.md"));
    assert!(index.contains("wiki/concepts/same-title-2.md"));
    Ok(())
}

#[test]
fn validate_sources_does_not_write() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path().join("wiki");
    init_wiki(&root)?;
    write_entity(
        &root,
        "concepts",
        "a.md",
        "01HZABC123DEF456GHI789JKL0",
        "Source Check",
        "",
        "Body.",
    )?;

    Command::cargo_bin("agwiki")?
        .arg("validate-sources")
        .arg("-C")
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("would write"));

    assert!(!root.join("wiki/concepts/source-check.md").exists());
    Ok(())
}

#[test]
fn export_html_smoke() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path().join("wiki");
    init_wiki(&root)?;
    write_entity(
        &root,
        "concepts",
        "a.md",
        "01HZABC123DEF456GHI789JKL0",
        "HTML Concept",
        "",
        "# HTML Concept\n\nBody.",
    )?;
    Command::cargo_bin("agwiki")?
        .arg("compile")
        .arg("-C")
        .arg(&root)
        .assert()
        .success();

    Command::cargo_bin("agwiki")?
        .arg("export-html")
        .arg("-C")
        .arg(&root)
        .assert()
        .success();

    assert!(root.join("dist/html/concepts/html-concept.html").is_file());
    assert!(root.join("dist/html/index.html").is_file());
    Ok(())
}
