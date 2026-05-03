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
        for name in ["codex", "claude"] {
            let stub_path = stub_dir.join(name);
            fs::write(
                &stub_path,
                "#!/bin/sh\n# Test stub agent for agwiki CLI e2e tests.\n# If AGWIKI_STUB_HITS is set, append one line per invocation.\nif [ -n \"${AGWIKI_STUB_HITS:-}\" ]; then\n  echo hit >> \"$AGWIKI_STUB_HITS\"\nfi\nwhile IFS= read -r line; do :; done\nexit 0\n",
            )?;
            let mut perms = fs::metadata(&stub_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&stub_path, perms)?;
        }
        Ok(())
    }

    fn run_ingest_with_file(
        root: &std::path::Path,
        stub_dir: &std::path::Path,
        source_file: &std::path::Path,
        agent: &str,
        extra_args: &[&str],
        envs: &[(&str, &std::path::Path)],
        original_path: &str,
    ) -> std::process::Output {
        std::env::set_var("PATH", format!("{}:{}", stub_dir.display(), original_path));

        let mut cmd = Command::cargo_bin("agwiki").unwrap();
        cmd.arg("ingest")
            .arg("--wiki-root")
            .arg(root)
            .arg("-a")
            .arg(agent);
        for a in extra_args {
            cmd.arg(a);
        }
        cmd.arg(source_file);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let output = cmd.output().unwrap();

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
        let output = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &[],
            &[],
            &original_path,
        );

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
        let output = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &[],
            &[],
            &original_path,
        );

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
        let output = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &[],
            &[],
            &original_path,
        );

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
        let output = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &[],
            &[],
            &original_path,
        );

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

    fn read_hits(path: &std::path::Path) -> usize {
        fs::read_to_string(path)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    #[test]
    fn test_ingest_resume_second_run_skips_single_file() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        setup_wiki(root)?;

        let source_file = root.join("raw/note.md");
        fs::create_dir_all(source_file.parent().unwrap())?;
        fs::write(&source_file, "# Note\n")?;

        let hits = root.join("hits.txt");
        let original_path = std::env::var("PATH").unwrap_or_default();

        let first = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(first.status.success(), "first run failed");
        assert_eq!(read_hits(&hits), 1, "expected one agent invocation");

        let second = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(second.status.success(), "second run failed");
        assert_eq!(
            read_hits(&hits),
            1,
            "expected second run to skip agent invocation"
        );
        let stderr = String::from_utf8_lossy(&second.stderr);
        assert!(
            stderr.contains("SKIP:"),
            "expected skip notice on stderr: {stderr}"
        );

        let ledger = root.join(".agwiki/ingest-state.jsonl");
        assert!(ledger.is_file(), "expected default ledger at {ledger:?}");
        Ok(())
    }

    #[test]
    fn test_ingest_resume_content_change_reingests() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        setup_wiki(root)?;

        let source_file = root.join("raw/note.md");
        fs::create_dir_all(source_file.parent().unwrap())?;
        fs::write(&source_file, "# Note v1\n")?;

        let hits = root.join("hits.txt");
        let original_path = std::env::var("PATH").unwrap_or_default();

        let first = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(first.status.success());
        assert_eq!(read_hits(&hits), 1);

        fs::write(&source_file, "# Note v2\n")?;
        let second = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(second.status.success());
        assert_eq!(read_hits(&hits), 2, "expected re-ingest on content change");
        Ok(())
    }

    #[test]
    fn test_ingest_resume_policy_change_reingests() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        setup_wiki(root)?;

        let source_file = root.join("raw/note.md");
        fs::create_dir_all(source_file.parent().unwrap())?;
        fs::write(&source_file, "# Note\n")?;

        let hits = root.join("hits.txt");
        let original_path = std::env::var("PATH").unwrap_or_default();

        let first = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(first.status.success());
        assert_eq!(read_hits(&hits), 1);

        fs::write(
            root.join("ingest.md"),
            "Ingest policy changed {{INGEST_PATH}} {{WIKI_ROOT}}\n",
        )?;
        let second = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(second.status.success());
        assert_eq!(read_hits(&hits), 2, "expected re-ingest on policy change");
        Ok(())
    }

    #[test]
    fn test_ingest_resume_force_reingests() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        setup_wiki(root)?;

        let source_file = root.join("raw/note.md");
        fs::create_dir_all(source_file.parent().unwrap())?;
        fs::write(&source_file, "# Note\n")?;

        let hits = root.join("hits.txt");
        let original_path = std::env::var("PATH").unwrap_or_default();

        let first = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(first.status.success());
        assert_eq!(read_hits(&hits), 1);

        let second = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume", "--force"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(second.status.success());
        assert_eq!(read_hits(&hits), 2, "expected --force to re-ingest");
        Ok(())
    }

    #[test]
    fn test_ingest_resume_agent_change_reingests() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        setup_wiki(root)?;

        let source_file = root.join("raw/note.md");
        fs::create_dir_all(source_file.parent().unwrap())?;
        fs::write(&source_file, "# Note\n")?;

        let hits = root.join("hits.txt");
        let original_path = std::env::var("PATH").unwrap_or_default();

        let first = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(first.status.success());
        assert_eq!(read_hits(&hits), 1);

        let second = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "claude",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(second.status.success());
        assert_eq!(read_hits(&hits), 2, "expected re-ingest on agent change");
        Ok(())
    }

    #[test]
    fn test_ingest_resume_model_change_reingests() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        setup_wiki(root)?;

        let source_file = root.join("raw/note.md");
        fs::create_dir_all(source_file.parent().unwrap())?;
        fs::write(&source_file, "# Note\n")?;

        let hits = root.join("hits.txt");
        let original_path = std::env::var("PATH").unwrap_or_default();

        let first = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume", "-m", "MODEL_A"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(first.status.success());
        assert_eq!(read_hits(&hits), 1);

        let second = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume", "-m", "MODEL_B"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(second.status.success());
        assert_eq!(read_hits(&hits), 2, "expected re-ingest on model change");
        Ok(())
    }

    #[test]
    fn test_ingest_resume_explicit_ingest_state_relative_path(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        setup_wiki(root)?;

        let source_file = root.join("raw/note.md");
        fs::create_dir_all(source_file.parent().unwrap())?;
        fs::write(&source_file, "# Note\n")?;

        let hits = root.join("hits.txt");
        let original_path = std::env::var("PATH").unwrap_or_default();

        let state_rel = ".agwiki/custom-ingest-state.jsonl";
        let out = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume", "--ingest-state", state_rel],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(out.status.success());
        assert_eq!(read_hits(&hits), 1);

        let expected = root.join(state_rel);
        assert!(
            expected.is_file(),
            "expected --ingest-state relative path to resolve under wiki root: {expected:?}"
        );
        Ok(())
    }

    #[test]
    fn test_ingest_resume_folder_reports_skipped() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        setup_wiki(root)?;

        let batch_dir = root.join("raw");
        fs::create_dir(&batch_dir)?;
        fs::write(batch_dir.join("a.md"), "# A\n")?;
        fs::write(batch_dir.join("b.md"), "# B\n")?;

        let hits = root.join("hits.txt");
        let original_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{}", stub_dir.path().display(), original_path),
        );

        let first = Command::cargo_bin("agwiki")
            .unwrap()
            .arg("ingest")
            .arg("--wiki-root")
            .arg(root)
            .arg("--resume")
            .arg("-a")
            .arg("codex")
            .arg("--folder")
            .arg(&batch_dir)
            .arg("--max-files")
            .arg("0")
            .env("AGWIKI_STUB_HITS", &hits)
            .output()
            .unwrap();
        assert!(first.status.success());
        assert_eq!(read_hits(&hits), 2);

        let second = Command::cargo_bin("agwiki")
            .unwrap()
            .arg("ingest")
            .arg("--wiki-root")
            .arg(root)
            .arg("--resume")
            .arg("-a")
            .arg("codex")
            .arg("--folder")
            .arg(&batch_dir)
            .arg("--max-files")
            .arg("0")
            .env("AGWIKI_STUB_HITS", &hits)
            .output()
            .unwrap();
        assert!(second.status.success());
        assert_eq!(
            read_hits(&hits),
            2,
            "expected both files skipped on second run"
        );
        let stderr = String::from_utf8_lossy(&second.stderr);
        assert!(
            stderr.contains("skipped"),
            "expected summary to include skipped count: {stderr}"
        );

        std::env::set_var("PATH", original_path);
        Ok(())
    }

    #[test]
    fn test_ingest_resume_lock_contention_fails_fast() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        setup_wiki(root)?;

        let source_file = root.join("raw/note.md");
        fs::create_dir_all(source_file.parent().unwrap())?;
        fs::write(&source_file, "# Note\n")?;

        let ledger = root.join(".agwiki/ingest-state.jsonl");
        fs::create_dir_all(ledger.parent().unwrap())?;
        let lock_path = std::path::PathBuf::from(format!("{}.lock", ledger.display()));
        fs::write(&lock_path, "locked")?;

        let hits = root.join("hits.txt");
        let original_path = std::env::var("PATH").unwrap_or_default();
        let output = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(
            !output.status.success(),
            "expected resume run to fail when lock exists"
        );
        assert_eq!(
            read_hits(&hits),
            0,
            "expected no agent invocations when lock exists"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("AGWIKI_INGEST_STATE_LOCKED"),
            "expected lock error code: {stderr}"
        );

        Ok(())
    }

    #[test]
    fn test_ingest_resume_invalid_ledger_line_fails() -> Result<(), Box<dyn std::error::Error>> {
        let _guard = PATH_MUTEX.lock().unwrap();

        let stub_dir = tempdir()?;
        make_stub_agent(stub_dir.path())?;

        let wiki_tmp = tempdir()?;
        let root = wiki_tmp.path();
        setup_wiki(root)?;

        let source_file = root.join("raw/note.md");
        fs::create_dir_all(source_file.parent().unwrap())?;
        fs::write(&source_file, "# Note\n")?;

        let ledger = root.join(".agwiki/ingest-state.jsonl");
        fs::create_dir_all(ledger.parent().unwrap())?;
        fs::write(&ledger, "{not valid json}\n")?;

        let hits = root.join("hits.txt");
        let original_path = std::env::var("PATH").unwrap_or_default();
        let output = run_ingest_with_file(
            root,
            stub_dir.path(),
            &source_file,
            "codex",
            &["--resume"],
            &[("AGWIKI_STUB_HITS", &hits)],
            &original_path,
        );
        assert!(
            !output.status.success(),
            "expected resume run to fail on invalid ledger"
        );
        assert_eq!(
            read_hits(&hits),
            0,
            "expected no agent invocations on invalid ledger"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("AGWIKI_INGEST_STATE_READ_FAILED"),
            "expected read-failed error code: {stderr}"
        );
        Ok(())
    }
}
