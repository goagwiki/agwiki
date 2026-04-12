//! Scaffold a new wiki root: `agwiki.toml`, directory tree, default `ingest.md`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const DEFAULT_INGEST_TEMPLATE: &str = include_str!("../prompts/ingest.md");

#[derive(Debug, Serialize, Deserialize)]
pub struct AgwikiConfig {
    pub version: u32,
    /// Directory paths relative to the wiki root (created with `create_dir_all`).
    pub directories: Vec<String>,
}

impl AgwikiConfig {
    pub fn default_layout() -> Self {
        Self {
            version: 1,
            directories: vec![
                "wiki".into(),
                "wiki/concepts".into(),
                "wiki/topics".into(),
                "wiki/sources".into(),
                "wiki/projects".into(),
                "wiki/people".into(),
                "wiki/syntheses".into(),
                "raw".into(),
                "templates".into(),
                "skill".into(),
            ],
        }
    }
}

/// Create `DIR` if missing; require `DIR` empty if it already exists. Write `agwiki.toml`, create dirs, write `ingest.md`.
pub fn run_init(target: &Path) -> Result<()> {
    if target.exists() {
        if !target.is_dir() {
            bail!("target exists and is not a directory: {}", target.display());
        }
        let mut it = fs::read_dir(target).with_context(|| target.display().to_string())?;
        if it.next().is_some() {
            bail!("target exists and is not empty: {}", target.display());
        }
    } else {
        fs::create_dir_all(target).with_context(|| target.display().to_string())?;
    }

    let config = AgwikiConfig::default_layout();
    let toml_str = toml::to_string_pretty(&config).context("serialize agwiki.toml")?;
    fs::write(target.join("agwiki.toml"), toml_str).context("write agwiki.toml")?;

    for d in &config.directories {
        fs::create_dir_all(target.join(d)).with_context(|| format!("create directory {}", d))?;
    }

    fs::write(target.join("ingest.md"), DEFAULT_INGEST_TEMPLATE).context("write ingest.md")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_creates_layout() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("newwiki");
        run_init(&root).unwrap();
        assert!(root.join("agwiki.toml").is_file());
        assert!(root.join("ingest.md").is_file());
        assert!(root.join("wiki").is_dir());
        assert!(root.join("raw").is_dir());
        assert!(root.join("ingest.md").metadata().unwrap().len() > 0);
    }

    #[test]
    fn init_empty_dir_ok() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("w");
        fs::create_dir_all(&root).unwrap();
        run_init(&root).unwrap();
        assert!(root.join("wiki").is_dir());
    }

    #[test]
    fn init_rejects_nonempty() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("w");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("x.txt"), "a").unwrap();
        assert!(run_init(&root).is_err());
    }
}
