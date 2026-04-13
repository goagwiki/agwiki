//! Run ingest via `aikit_sdk::run_agent_events` with the expanded ingest prompt.
//!
//! The prompt is built from the wiki's `ingest.md` with `{{INGEST_PATH}}` and `{{WIKI_ROOT}}` filled in (`toolkit::expand_ingest_prompt`).
//! Always emits an NDJSON event stream on stdout via the SDK callback (one JSON line per event).

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::toolkit::expand_ingest_prompt;
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

/// Discover all Markdown files (`*.md` / `*.MD`, case-insensitive) under `dir` recursively.
///
/// Does **not** follow symlinks. Returns paths sorted lexicographically by full path.
/// `dir` must exist and be a directory.
pub fn discover_md_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let canon_dir = dir
        .canonicalize()
        .with_context(|| format!("cannot access directory: {}", dir.display()))?;

    if !canon_dir.is_dir() {
        bail!("not a directory: {}", dir.display());
    }

    let mut results = Vec::new();
    let mut stack = vec![canon_dir];

    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("cannot read directory: {}", current.display()))?;

        for entry in entries {
            let entry =
                entry.with_context(|| format!("error reading entry in {}", current.display()))?;
            let file_type = entry
                .file_type()
                .with_context(|| format!("cannot get file type for {}", entry.path().display()))?;

            // Skip symlinks
            if file_type.is_symlink() {
                continue;
            }

            let path = entry.path();

            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file()
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
            {
                results.push(path);
            }
        }
    }

    results.sort();
    Ok(results)
}

/// Run the full ingest pipeline for a single file path.
fn run_ingest_for_path(
    wiki_root: &Path,
    file: &Path,
    prompt_path: &Path,
    agent: &str,
    model: Option<&str>,
    stream: bool,
) -> Result<()> {
    let ingest_path = resolve_ingest_source(file)?;
    let prompt = expand_ingest_prompt(wiki_root, &ingest_path, prompt_path)?;
    run_aikit(wiki_root, &prompt, agent, model, stream)
}

/// Summary returned by [`run_folder_ingest`].
#[derive(Debug)]
pub struct FolderIngestResult {
    /// Total files discovered.
    pub total: usize,
    /// Files that completed without error.
    pub succeeded: usize,
    /// Files that failed, paired with their error message.
    pub failures: Vec<(PathBuf, String)>,
}

/// Ingest all `*.md` files discovered under `folder` (recursive, no symlinks).
///
/// Returns an error immediately (before ingesting any file) if the discovered
/// file count exceeds `max_files` and `max_files > 0`.
/// Pass `max_files = 0` for no cap (unlimited).
pub fn run_folder_ingest(
    wiki_root: &Path,
    folder: &Path,
    prompt_path: &Path,
    agent: &str,
    model: Option<&str>,
    stream: bool,
    max_files: usize,
) -> Result<FolderIngestResult> {
    let files = discover_md_files(folder)?;
    let total = files.len();

    if max_files > 0 && total > max_files {
        bail!(
            "found {} markdown file(s) under {}; exceeds --max-files cap of {}. \
             Pass --max-files {} (or higher) to proceed.",
            total,
            folder.display(),
            max_files,
            total
        );
    }

    let mut failures: Vec<(PathBuf, String)> = Vec::new();

    for file in &files {
        if let Err(e) = run_ingest_for_path(wiki_root, file, prompt_path, agent, model, stream) {
            failures.push((file.clone(), e.to_string()));
        }
    }

    let succeeded = total - failures.len();
    Ok(FolderIngestResult {
        total,
        succeeded,
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // --- discover_md_files tests ---

    #[test]
    fn discover_md_files_empty_dir() {
        let tmp = tempdir().unwrap();
        let files = discover_md_files(tmp.path()).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn discover_md_files_finds_md_only() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("doc.md"), "# doc").unwrap();
        fs::write(tmp.path().join("file.txt"), "text").unwrap();
        fs::write(tmp.path().join("data.json"), "{}").unwrap();
        fs::write(tmp.path().join("noext"), "noext").unwrap();
        let files = discover_md_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("doc.md"));
    }

    #[test]
    fn discover_md_files_case_insensitive() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("upper.MD"), "# Up").unwrap();
        let files = discover_md_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn discover_md_files_nested_dirs() {
        let tmp = tempdir().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(tmp.path().join("root.md"), "root").unwrap();
        fs::write(sub.join("nested.md"), "nested").unwrap();
        let files = discover_md_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn discover_md_files_sorted_lexicographic() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("z.md"), "z").unwrap();
        fs::write(tmp.path().join("a.md"), "a").unwrap();
        fs::write(tmp.path().join("m.md"), "m").unwrap();
        let files = discover_md_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 3);
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, vec!["a.md", "m.md", "z.md"]);
    }

    #[test]
    fn discover_md_files_rejects_nonexistent() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("missing");
        assert!(discover_md_files(&missing).is_err());
    }

    #[test]
    fn run_folder_ingest_cap_exceeded_returns_error() {
        let tmp = tempdir().unwrap();
        let batch = tmp.path().join("batch");
        fs::create_dir(&batch).unwrap();
        for i in 0..5u32 {
            fs::write(batch.join(format!("f{i}.md")), "# note").unwrap();
        }
        // cap of 3 with 5 files → error
        let prompt_path = tmp.path().join("ingest.md");
        fs::write(&prompt_path, "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n").unwrap();
        let err = run_folder_ingest(tmp.path(), &batch, &prompt_path, "codex", None, false, 3)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("5"), "expected file count in error: {msg}");
        assert!(msg.contains("--max-files"), "expected hint in error: {msg}");
    }

    #[test]
    fn run_folder_ingest_zero_cap_means_unlimited() {
        let tmp = tempdir().unwrap();
        let batch = tmp.path().join("batch");
        fs::create_dir(&batch).unwrap();
        // 5 files, cap = 0 → no cap applied; will fail at agent step (not runnable) not at cap
        for i in 0..5u32 {
            fs::write(batch.join(format!("f{i}.md")), "# note").unwrap();
        }
        let prompt_path = tmp.path().join("ingest.md");
        fs::write(&prompt_path, "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n").unwrap();
        let result = run_folder_ingest(
            tmp.path(),
            &batch,
            &prompt_path,
            "nonexistent-agent-xyz",
            None,
            false,
            0,
        )
        .unwrap();
        // all 5 files should have failed at the agent step, not at cap
        assert_eq!(result.total, 5);
        assert_eq!(result.failures.len(), 5);
    }

    // --- resolve_ingest_source tests ---

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
