//! End-to-end CLI tests exercising the `agwiki` binary.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_version_command() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("agwiki"));
    Ok(())
}

#[test]
fn test_init_creates_wiki_structure() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("init").arg(tmp.path());
    cmd.assert().success();

    assert!(tmp.path().join("agwiki.toml").exists());
    assert!(tmp.path().join("wiki").is_dir());
    assert!(tmp.path().join("ingest.md").exists());
    Ok(())
}

#[test]
fn test_validate_clean_wiki() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki"))?;
    fs::write(root.join("wiki/index.md"), "# Index\n")?;

    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("validate").arg("--wiki-root").arg(root);
    cmd.assert().success();
    Ok(())
}

#[test]
fn test_validate_broken_links() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki"))?;
    fs::write(root.join("wiki/index.md"), "# Index\n")?;
    fs::write(root.join("wiki/broken.md"), "see [[nowhere]]\n")?;

    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("validate").arg("--wiki-root").arg(root);
    cmd.assert()
        .failure()
        .stdout(predicate::str::contains("broken"));
    Ok(())
}

#[test]
fn test_export_skill_dry_run() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path();

    // Set up minimal wiki structure required by export-skill
    fs::create_dir_all(root.join("wiki"))?;
    fs::write(root.join("wiki/index.md"), "# Index\n")?;
    fs::create_dir_all(root.join("skill/references"))?;
    fs::write(root.join("skill/SKILL.md"), "# Skill\n")?;

    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("export-skill")
        .arg("--wiki-root")
        .arg(root)
        .arg("--dry-run");
    cmd.assert().success();
    Ok(())
}

#[cfg(unix)]
mod unix_tests {
    use super::*;
    use std::sync::Mutex;

    static PATH_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_ingest_with_agent_stub() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        // Create stub agent executable that exits 0
        let stub_dir = tempdir()?;
        let stub_path = stub_dir.path().join("codex");
        fs::write(
            &stub_path,
            "#!/bin/sh\nwhile IFS= read -r line; do :; done\nexit 0\n",
        )?;
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&stub_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub_path, perms)?;

        // Create a wiki with required structure
        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        fs::create_dir_all(root.join("wiki"))?;
        fs::write(root.join("wiki/index.md"), "# Index\n")?;
        fs::write(
            root.join("ingest.md"),
            "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n",
        )?;

        // Create a source file to ingest
        let source_file = root.join("note.md");
        fs::write(&source_file, "# Test Note\n")?;

        let original_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{}", stub_dir.path().display(), original_path),
        );

        let result = Command::cargo_bin("agwiki")
            .unwrap()
            .arg("ingest")
            .arg("--wiki-root")
            .arg(root)
            .arg("-a")
            .arg("codex")
            .arg(&source_file)
            .output();

        std::env::set_var("PATH", original_path);

        let output = result?;
        // The stub exits 0, so the command should either succeed or fail with a non-"not runnable" error
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !stderr.contains("not runnable"),
                "unexpected not-runnable error: {stderr}"
            );
        }
        Ok(())
    }
}
