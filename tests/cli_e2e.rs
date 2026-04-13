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

#[test]
fn test_ingest_rejects_missing_file() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki"))?;
    fs::write(root.join("wiki/index.md"), "# Index\n")?;
    fs::write(
        root.join("ingest.md"),
        "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n",
    )?;

    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("ingest")
        .arg("--wiki-root")
        .arg(root)
        .arg("-a")
        .arg("codex")
        .arg(root.join("nonexistent.txt"));
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
    Ok(())
}

#[test]
fn test_ingest_rejects_binary_file() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki"))?;
    fs::write(root.join("wiki/index.md"), "# Index\n")?;
    fs::write(
        root.join("ingest.md"),
        "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n",
    )?;

    let binary_file = root.join("data.bin");
    fs::write(&binary_file, b"binary\x00content")?;

    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("ingest")
        .arg("--wiki-root")
        .arg(root)
        .arg("-a")
        .arg("codex")
        .arg(&binary_file);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("file appears to be binary"));
    Ok(())
}

fn setup_wiki(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(root.join("wiki"))?;
    fs::write(root.join("wiki/index.md"), "# Index\n")?;
    fs::write(
        root.join("ingest.md"),
        "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n",
    )?;
    Ok(())
}

#[test]
fn test_ingest_folder_cap_exceeded() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path();
    setup_wiki(root)?;

    let batch_dir = root.join("batch");
    fs::create_dir(&batch_dir)?;
    for i in 0..31u32 {
        fs::write(batch_dir.join(format!("file{i:02}.md")), "# Note\n")?;
    }

    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("ingest")
        .arg("--wiki-root")
        .arg(root)
        .arg("-a")
        .arg("codex")
        .arg("--folder")
        .arg(&batch_dir);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("31"))
        .stderr(predicate::str::contains("--max-files"));
    Ok(())
}

#[test]
fn test_ingest_folder_empty_dir_succeeds() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path();
    setup_wiki(root)?;

    let batch_dir = root.join("batch");
    fs::create_dir(&batch_dir)?;

    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("ingest")
        .arg("--wiki-root")
        .arg(root)
        .arg("-a")
        .arg("codex")
        .arg("--folder")
        .arg(&batch_dir);
    cmd.assert().success();
    Ok(())
}

#[test]
fn test_ingest_folder_missing_dir_fails() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path();
    setup_wiki(root)?;

    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("ingest")
        .arg("--wiki-root")
        .arg(root)
        .arg("-a")
        .arg("codex")
        .arg("--folder")
        .arg(root.join("nonexistent"));
    cmd.assert().failure();
    Ok(())
}

#[test]
fn test_ingest_file_and_folder_conflict_fails() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let root = tmp.path();
    setup_wiki(root)?;

    let batch_dir = root.join("batch");
    fs::create_dir(&batch_dir)?;
    let file = root.join("note.md");
    fs::write(&file, "# Note\n")?;

    let mut cmd = Command::cargo_bin("agwiki")?;
    cmd.arg("ingest")
        .arg("--wiki-root")
        .arg(root)
        .arg("-a")
        .arg("codex")
        .arg("--folder")
        .arg(&batch_dir)
        .arg(&file);
    cmd.assert().failure();
    Ok(())
}

#[cfg(unix)]
mod unix_tests {
    use super::*;
    use std::sync::Mutex;

    static PATH_MUTEX: Mutex<()> = Mutex::new(());

    fn make_stub_agent(stub_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;
        let stub_path = stub_dir.join("codex");
        fs::write(
            &stub_path,
            "#!/bin/sh\nwhile IFS= read -r line; do :; done\nexit 0\n",
        )?;
        let mut perms = fs::metadata(&stub_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub_path, perms)?;
        Ok(())
    }

    fn run_ingest_with_file(
        root: &std::path::Path,
        stub_dir: &std::path::Path,
        source_file: &std::path::Path,
        original_path: &str,
    ) -> std::process::Output {
        std::env::set_var("PATH", format!("{}:{}", stub_dir.display(), original_path));

        let output = Command::cargo_bin("agwiki")
            .unwrap()
            .arg("ingest")
            .arg("--wiki-root")
            .arg(root)
            .arg("-a")
            .arg("codex")
            .arg(source_file)
            .output()
            .unwrap();

        std::env::set_var("PATH", original_path);
        output
    }

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

    #[test]
    fn test_ingest_txt_file_with_agent_stub() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        fs::create_dir_all(root.join("wiki"))?;
        fs::write(root.join("wiki/index.md"), "# Index\n")?;
        fs::write(
            root.join("ingest.md"),
            "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n",
        )?;

        let source_file = root.join("notes.txt");
        fs::write(&source_file, "Plain text content\n")?;

        let original_path = std::env::var("PATH").unwrap_or_default();
        let output = run_ingest_with_file(root, stub_dir.path(), &source_file, &original_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !stderr.contains("not runnable"),
                "unexpected not-runnable error: {stderr}"
            );
            assert!(
                !stderr.contains("binary"),
                "unexpected binary error: {stderr}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_ingest_json_file_with_agent_stub() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        fs::create_dir_all(root.join("wiki"))?;
        fs::write(root.join("wiki/index.md"), "# Index\n")?;
        fs::write(
            root.join("ingest.md"),
            "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n",
        )?;

        let source_file = root.join("config.json");
        fs::write(&source_file, r#"{"name": "test", "value": 42}"#)?;

        let original_path = std::env::var("PATH").unwrap_or_default();
        let output = run_ingest_with_file(root, stub_dir.path(), &source_file, &original_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !stderr.contains("not runnable"),
                "unexpected not-runnable error: {stderr}"
            );
            assert!(
                !stderr.contains("binary"),
                "unexpected binary error: {stderr}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_ingest_yaml_file_with_agent_stub() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        fs::create_dir_all(root.join("wiki"))?;
        fs::write(root.join("wiki/index.md"), "# Index\n")?;
        fs::write(
            root.join("ingest.md"),
            "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n",
        )?;

        let source_file = root.join("deploy.yaml");
        fs::write(&source_file, "version: 1\nsteps:\n  - run: echo hello\n")?;

        let original_path = std::env::var("PATH").unwrap_or_default();
        let output = run_ingest_with_file(root, stub_dir.path(), &source_file, &original_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !stderr.contains("not runnable"),
                "unexpected not-runnable error: {stderr}"
            );
            assert!(
                !stderr.contains("binary"),
                "unexpected binary error: {stderr}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_ingest_extensionless_text_file_with_agent_stub(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        fs::create_dir_all(root.join("wiki"))?;
        fs::write(root.join("wiki/index.md"), "# Index\n")?;
        fs::write(
            root.join("ingest.md"),
            "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n",
        )?;

        let source_file = root.join("Makefile");
        fs::write(&source_file, "all:\n\techo done\n")?;

        let original_path = std::env::var("PATH").unwrap_or_default();
        let output = run_ingest_with_file(root, stub_dir.path(), &source_file, &original_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !stderr.contains("not runnable"),
                "unexpected not-runnable error: {stderr}"
            );
            assert!(
                !stderr.contains("binary"),
                "unexpected binary error: {stderr}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_ingest_folder_with_stub_agent() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        fs::create_dir_all(root.join("wiki"))?;
        fs::write(root.join("wiki/index.md"), "# Index\n")?;
        fs::write(
            root.join("ingest.md"),
            "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n",
        )?;

        let batch_dir = root.join("batch");
        fs::create_dir(&batch_dir)?;
        fs::write(batch_dir.join("a.md"), "# A\n")?;
        fs::write(batch_dir.join("b.md"), "# B\n")?;

        let original_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{}", stub_dir.path().display(), original_path),
        );

        let output = Command::cargo_bin("agwiki")
            .unwrap()
            .arg("ingest")
            .arg("--wiki-root")
            .arg(root)
            .arg("-a")
            .arg("codex")
            .arg("--folder")
            .arg(&batch_dir)
            .output()
            .unwrap();

        std::env::set_var("PATH", original_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !stderr.contains("not runnable"),
                "unexpected not-runnable error: {stderr}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_ingest_folder_max_files_override() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        fs::create_dir_all(root.join("wiki"))?;
        fs::write(root.join("wiki/index.md"), "# Index\n")?;
        fs::write(
            root.join("ingest.md"),
            "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n",
        )?;

        let batch_dir = root.join("batch");
        fs::create_dir(&batch_dir)?;
        // 5 files; default cap is 30 so this passes; test that --max-files 3 blocks it
        for i in 0..5u32 {
            fs::write(batch_dir.join(format!("f{i}.md")), "# note")?;
        }

        let original_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{}", stub_dir.path().display(), original_path),
        );

        // With --max-files 3, 5 files should be rejected
        let blocked = Command::cargo_bin("agwiki")
            .unwrap()
            .arg("ingest")
            .arg("--wiki-root")
            .arg(root)
            .arg("-a")
            .arg("codex")
            .arg("--folder")
            .arg(&batch_dir)
            .arg("--max-files")
            .arg("3")
            .output()
            .unwrap();

        // With --max-files 10, 5 files should proceed
        let allowed = Command::cargo_bin("agwiki")
            .unwrap()
            .arg("ingest")
            .arg("--wiki-root")
            .arg(root)
            .arg("-a")
            .arg("codex")
            .arg("--folder")
            .arg(&batch_dir)
            .arg("--max-files")
            .arg("10")
            .output()
            .unwrap();

        std::env::set_var("PATH", original_path);

        assert!(
            !blocked.status.success(),
            "expected failure with --max-files 3 for 5 files"
        );
        let blocked_stderr = String::from_utf8_lossy(&blocked.stderr);
        assert!(
            blocked_stderr.contains("--max-files"),
            "expected --max-files hint: {blocked_stderr}"
        );

        if !allowed.status.success() {
            let stderr = String::from_utf8_lossy(&allowed.stderr);
            assert!(
                !stderr.contains("--max-files"),
                "unexpected cap error with --max-files 10: {stderr}"
            );
        }
        Ok(())
    }
}
