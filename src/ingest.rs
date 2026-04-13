//! Run ingest via `aikit_sdk::run_agent_events` with the expanded ingest prompt.
//!
//! The prompt is built from the wiki's `ingest.md` with `{{INGEST_PATH}}` and `{{WIKI_ROOT}}` filled in (`toolkit::expand_ingest_prompt`).
//! Always emits an NDJSON event stream on stdout via the SDK callback (one JSON line per event).

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

use aikit_sdk::{is_runnable, run_agent_events, runnable_agents, RunOptions};

/// Canonicalize `file`, ensure it exists and contains valid text content.
pub fn resolve_ingest_source(file: &Path) -> Result<PathBuf> {
    let file = file
        .canonicalize()
        .with_context(|| format!("not found: {}", file.display()))?;

    validate_text_file(&file)?;

    Ok(file)
}

/// Validate that `path` contains text content (UTF-8 encoded, no null bytes).
fn validate_text_file(path: &Path) -> Result<()> {
    use std::fs::File;
    use std::io::Read;

    let mut file =
        File::open(path).with_context(|| format!("cannot read file: {}", path.display()))?;

    let mut buffer = [0u8; 8192];
    let bytes_read = file
        .read(&mut buffer)
        .with_context(|| format!("failed to read from file: {}", path.display()))?;

    let sample = &buffer[..bytes_read];

    // Check for null bytes (binary indicator)
    if sample.contains(&0) {
        bail!("file appears to be binary: {}", path.display());
    }

    // Validate UTF-8 encoding
    std::str::from_utf8(sample)
        .with_context(|| format!("file does not contain valid UTF-8 text: {}", path.display()))?;

    Ok(())
}

/// Run ingest via `aikit_sdk::run_agent_events`; emits NDJSON events on stdout.
pub fn run_aikit(
    wiki_root: &Path,
    prompt: &str,
    agent: &str,
    model: Option<&str>,
    stream: bool,
) -> Result<()> {
    if !is_runnable(agent) {
        bail!(
            "agent '{}' is not runnable; available agents: {}",
            agent,
            runnable_agents().join(", ")
        );
    }

    let mut opts = RunOptions::new()
        .with_current_dir(wiki_root.to_path_buf())
        .with_stream(stream);
    if let Some(m) = model {
        opts = opts.with_model(m.to_string());
    }

    let result = run_agent_events(agent, prompt, opts, |event| {
        if let Ok(s) = serde_json::to_string(&event) {
            println!("{}", s);
        }
    })
    .map_err(|e| anyhow::anyhow!("aikit-sdk agent execution failed: {}", e))?;

    let _ = std::io::stderr().write_all(&result.stderr);

    if !result.success() {
        bail!("agent exited with status {:?}", result.exit_code());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolve_ingest_source_accepts_md() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("note.md");
        fs::write(&f, "x").unwrap();
        let out = resolve_ingest_source(&f).unwrap();
        assert!(out.is_absolute());
        assert!(out.ends_with("note.md"));
    }

    #[test]
    fn resolve_ingest_source_accepts_txt() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("x.txt");
        fs::write(&f, "hello text").unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn resolve_ingest_source_accepts_json() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("data.json");
        fs::write(&f, r#"{"key": "value"}"#).unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn resolve_ingest_source_accepts_yaml() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("config.yaml");
        fs::write(&f, "key: value\n").unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn resolve_ingest_source_accepts_log() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("app.log");
        fs::write(&f, "INFO: started\n").unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn resolve_ingest_source_rejects_binary() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("binary.bin");
        fs::write(&f, b"binary\x00content").unwrap();
        let err = resolve_ingest_source(&f).unwrap_err();
        assert!(
            err.to_string().contains("file appears to be binary"),
            "error: {err}"
        );
    }

    #[test]
    fn resolve_ingest_source_rejects_invalid_utf8() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("bad.txt");
        fs::write(&f, b"\xff\xfe invalid utf8 bytes").unwrap();
        let err = resolve_ingest_source(&f).unwrap_err();
        assert!(err.to_string().contains("valid UTF-8"), "error: {err}");
    }

    #[test]
    fn resolve_ingest_source_accepts_empty_file() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("empty.txt");
        fs::write(&f, b"").unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn resolve_ingest_source_handles_missing_file() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("missing.md");
        assert!(resolve_ingest_source(&missing).is_err());
    }

    #[test]
    fn resolve_ingest_source_accepts_uppercase_md() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("note.MD");
        fs::write(&f, "content").unwrap();
        let result = resolve_ingest_source(&f);
        assert!(result.is_ok());
    }

    #[test]
    fn resolve_ingest_source_accepts_no_extension() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("note");
        fs::write(&f, "content").unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn agent_not_runnable_returns_error() {
        let tmp = tempdir().unwrap();
        let err =
            run_aikit(tmp.path(), "prompt", "nonexistent-agent-xyz", None, false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nonexistent-agent-xyz"), "error: {msg}");
        assert!(msg.contains("available agents"), "error: {msg}");
    }

    #[cfg(unix)]
    mod unix_tests {
        use super::*;
        use std::sync::Mutex;

        static PATH_MUTEX: Mutex<()> = Mutex::new(());

        #[test]
        fn run_aikit_with_stub_agent_succeeds() {
            let _guard = PATH_MUTEX.lock().unwrap();

            let stub_dir = tempdir().unwrap();
            // Write a stub script that exits 0 and prints nothing to stderr
            let stub_path = stub_dir.path().join("codex");
            fs::write(
                &stub_path,
                "#!/bin/sh\nwhile IFS= read -r line; do :; done\nexit 0\n",
            )
            .unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&stub_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&stub_path, perms).unwrap();

            let original_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var(
                "PATH",
                format!("{}:{}", stub_dir.path().display(), original_path),
            );

            let wiki_tmp = tempdir().unwrap();
            let result = run_aikit(wiki_tmp.path(), "hello", "codex", None, false);

            std::env::set_var("PATH", original_path);

            // The stub exits 0, so this should succeed (or fail with a spawn/io error, not a "not runnable" error)
            match result {
                Ok(()) => {}
                Err(e) => {
                    let msg = e.to_string();
                    // Must NOT be a "not runnable" failure
                    assert!(
                        !msg.contains("not runnable"),
                        "unexpected not-runnable error: {msg}"
                    );
                }
            }
        }
    }
}
