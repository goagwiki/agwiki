//! Locate the agwiki toolkit (`prompts/ingest.md`, `AGENTS.md`) from environment variables.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Resolve toolkit root: `AGWIKI_ROOT`, then `FASTWIKI_ROOT`, then `WIKIFY_ROOT`.
pub fn resolve_toolkit_root() -> Result<PathBuf> {
    let key = ["AGWIKI_ROOT", "FASTWIKI_ROOT", "WIKIFY_ROOT"]
        .into_iter()
        .find_map(|k| std::env::var(k).ok().filter(|s| !s.is_empty()));
    let Some(raw) = key else {
        bail!("set AGWIKI_ROOT (or FASTWIKI_ROOT / WIKIFY_ROOT) to the agwiki toolkit directory");
    };
    let p = PathBuf::from(raw);
    let p = p
        .canonicalize()
        .with_context(|| format!("toolkit path not found: {}", p.display()))?;
    if !p.is_dir() {
        bail!("toolkit path is not a directory: {}", p.display());
    }
    Ok(p)
}

/// Default ingest template path inside the toolkit.
pub fn default_prompt_path(toolkit: &Path) -> PathBuf {
    toolkit.join("prompts").join("ingest.md")
}

/// Read `<toolkit>/AGENTS.md` for `{{WIKIFY_AGENTS_MD}}` substitution.
pub fn read_agents_md(toolkit: &Path) -> Result<String> {
    let p = toolkit.join("AGENTS.md");
    std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))
}

/// Load `prompt_path`, substitute ingest placeholders, return the full prompt text.
pub fn expand_ingest_prompt(
    toolkit: &Path,
    wiki_root: &Path,
    ingest_path: &Path,
    prompt_path: &Path,
) -> Result<String> {
    let mut text = std::fs::read_to_string(prompt_path)
        .with_context(|| format!("read prompt {}", prompt_path.display()))?;
    if text.contains("{{WIKIFY_AGENTS_MD}}") {
        let agents = read_agents_md(toolkit)?;
        text = text.replace("{{WIKIFY_AGENTS_MD}}", &agents);
    }
    let ingest = ingest_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("ingest path is not valid UTF-8"))?;
    let wiki = wiki_root
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("wiki root is not valid UTF-8"))?;
    let wikify = toolkit
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("toolkit path is not valid UTF-8"))?;
    text = text.replace("{{INGEST_PATH}}", ingest);
    text = text.replace("{{WIKI_ROOT}}", wiki);
    text = text.replace("{{WIKIFY_ROOT}}", wikify);
    Ok(text)
}
