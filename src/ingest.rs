//! Run `aikit run` with the expanded ingest prompt on stdin.
//!
//! The prompt is built from the wiki’s `ingest.md` with `{{INGEST_PATH}}` and `{{WIKI_ROOT}}` filled in (`toolkit::expand_ingest_prompt`).
//! Always passes `--events` so aikit emits an NDJSON event stream on stdout (inherited).

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Canonicalize `file`, ensure it exists and has a `.md` extension.
pub fn resolve_ingest_source(file: &Path) -> Result<PathBuf> {
    let file = file
        .canonicalize()
        .with_context(|| format!("not found: {}", file.display()))?;
    let ext = file
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "md" {
        bail!("expected .md file, got: {}", file.display());
    }
    Ok(file)
}

/// Spawn `aikit run --events` (and optional `--stream`), `-a` / `-m`, prompt on stdin; stdout/stderr inherited.
pub fn run_aikit(
    wiki_root: &Path,
    prompt: &str,
    agent: &str,
    model: Option<&str>,
    stream: bool,
) -> Result<()> {
    let mut cmd = Command::new("aikit");
    cmd.arg("run").arg("--events");
    if stream {
        cmd.arg("--stream");
    }
    cmd.arg("-a").arg(agent);
    if let Some(m) = model {
        cmd.arg("-m").arg(m);
    }
    cmd.current_dir(wiki_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child = cmd
        .spawn()
        .context("failed to spawn `aikit` (is it on PATH?)")?;
    let mut stdin = child.stdin.take().context("stdin")?;
    stdin.write_all(prompt.as_bytes())?;
    drop(stdin);
    let st = child.wait()?;
    if !st.success() {
        bail!("aikit exited with status {:?}", st.code());
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
    fn resolve_ingest_source_rejects_non_md() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("x.txt");
        fs::write(&f, "x").unwrap();
        assert!(resolve_ingest_source(&f).is_err());
    }
}

