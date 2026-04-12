//! Resolve ingest source paths for markdown wikis.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Canonicalize `file`, ensure it is `.md`, and optionally require it under `<wiki_root>/raw/`.
pub fn prep(wiki_root: &Path, file: &Path, raw_only: bool) -> Result<PathBuf> {
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
    if raw_only {
        let raw_base = wiki_root.join("raw");
        let raw_base = raw_base.canonicalize().with_context(|| {
            format!(
                "raw/ missing under wiki root (needed for --raw-only): {}",
                wiki_root.display()
            )
        })?;
        if !file.starts_with(&raw_base) {
            bail!("not under {}: {}", raw_base.display(), file.display());
        }
    }
    Ok(file)
}
