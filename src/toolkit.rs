//! Ingest prompt path under the wiki root and placeholder expansion.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// `<wiki-root>/ingest.md`
pub fn wiki_ingest_prompt_path(wiki_root: &Path) -> PathBuf {
    wiki_root.join("ingest.md")
}

/// Require `ingest.md` at the wiki root.
pub fn require_wiki_ingest_prompt(wiki_root: &Path) -> Result<PathBuf> {
    let p = wiki_ingest_prompt_path(wiki_root);
    if !p.is_file() {
        bail!("missing ingest.md at wiki root (expected {})", p.display());
    }
    Ok(p)
}

/// Load `prompt_path`, substitute placeholders, return the prompt sent to the agent.
///
/// Replacements: `{{INGEST_PATH}}`, `{{WIKI_ROOT}}` (absolute paths as UTF-8).
pub fn expand_ingest_prompt(
    wiki_root: &Path,
    ingest_path: &Path,
    prompt_path: &Path,
) -> Result<String> {
    let mut text = std::fs::read_to_string(prompt_path)
        .with_context(|| format!("read prompt {}", prompt_path.display()))?;
    let ingest = ingest_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("ingest path is not valid UTF-8"))?;
    let wiki = wiki_root
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("wiki root is not valid UTF-8"))?;
    text = text.replace("{{INGEST_PATH}}", ingest);
    text = text.replace("{{WIKI_ROOT}}", wiki);
    Ok(text)
}
