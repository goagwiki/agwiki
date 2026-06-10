//! Scaffold a new wiki root: `agwiki.toml`, directory tree, default `ingest.md`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const WIKI_INDEX_TEMPLATE: &str = include_str!("../prompts/wiki-index.md");
const WIKI_INBOX_TEMPLATE: &str = include_str!("../prompts/wiki-inbox.md");
const WIKI_LOG_TEMPLATE: &str = include_str!("../prompts/wiki-log.md");

#[derive(Debug, Serialize, Deserialize)]
pub struct AgwikiConfig {
    pub version: u32,
    /// Directory path relative to wiki root where entity source files live.
    #[serde(default = "default_content_root")]
    pub content_root: String,
    /// Directory path relative to wiki root for generated wiki output.
    #[serde(default = "default_generated_wiki")]
    pub generated_wiki: String,
    pub ontology: OntologyConfig,
}

fn default_content_root() -> String {
    "content".into()
}
fn default_generated_wiki() -> String {
    "wiki".into()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OntologyConfig {
    /// Ordered list of entity kind identifiers.
    pub kinds: Vec<String>,
    /// Optional allow-list for relations[].rel values. When empty, any string is accepted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relation_types: Vec<String>,
}

impl AgwikiConfig {
    pub fn default_layout() -> Self {
        Self {
            version: 2,
            content_root: "content".into(),
            generated_wiki: "wiki".into(),
            ontology: OntologyConfig {
                kinds: vec![
                    "concepts".into(),
                    "topics".into(),
                    "projects".into(),
                    "people".into(),
                    "syntheses".into(),
                    "sources".into(),
                ],
                relation_types: vec![],
            },
        }
    }
}

fn generate_ingest_md(kinds: &[String]) -> String {
    let kinds_list = kinds
        .iter()
        .map(|k| format!("- `content/{k}/` — {k} entity files"))
        .collect::<Vec<_>>()
        .join("\n");
    let kinds_names = kinds.join(", ");

    format!(
        r#"Ingest {{{{INGEST_PATH}}}} into the wiki at {{{{WIKI_ROOT}}}}.

Follow the split-ingest workflow below. Work only under `{{{{WIKI_ROOT}}}}`; do not require reading paths outside that wiki root.

# Purpose

Rules for structured content wikis used with **agwiki** `ingest`. Paths below are relative to the wiki root; `{{{{WIKI_ROOT}}}}` is an absolute path for the agent.

# Source of truth

Agents MUST write entity files only to `content/<kind>/`, NOT to `wiki/`. The `wiki/` directory is generated output produced by `agwiki compile` — do not modify it directly.

# Front matter

Every entity file must start with a YAML front matter block:

```yaml
---
id: "<ulid>"
title: "Entity Title"
schema_version: 1
---
```

Required fields: `id` (string, stable ULID), `title` (string), `schema_version` (integer, must be 1).
Optional fields: `status` (active|archived), `relations` (list of {{target: id, rel: string}}), `aliases` (list of strings).

# Kinds

Declared ontology kinds for this wiki: {kinds_names}

{kinds_list}
- `content/pages/` — non-entity navigation pages (index.md, log.md, inbox.md)

# Rules

- Never modify files inside `raw/`
- Treat `raw/` as source material
- Write entity files to `content/<kind>/` with YAML front matter
- Use a stable ULID for the `id` field (run `agwiki new <kind> --title "..."` to get a pre-scaffolded file)
- Prefer updating existing entity files over creating duplicates
- Preserve uncertainty and disagreements between sources
- Use concise markdown
- Add relations between related entities using the `relations` front matter field

# Ingest workflow

When asked to ingest a file from `raw/`:

1. Read the source
2. Create or update entity files in the appropriate `content/<kind>/` directories
3. Each file needs valid YAML front matter with id, title, schema_version: 1
4. Add relations to link related entities
5. Update `content/pages/index.md` when navigation changes
6. Append a short note to `content/pages/log.md`

# Query workflow

When asked a question:

1. Start from `content/pages/index.md` or `wiki/index.md`
2. Read relevant entity files
3. Answer using the wiki content
"#,
        kinds_names = kinds_names,
        kinds_list = kinds_list,
    )
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

    // Create content/<kind>/ directories
    for kind in &config.ontology.kinds {
        let dir = target.join(&config.content_root).join(kind);
        fs::create_dir_all(&dir).with_context(|| format!("create directory content/{}", kind))?;
    }
    // Create content/pages/
    fs::create_dir_all(target.join(&config.content_root).join("pages"))
        .context("create content/pages")?;

    // Create other top-level dirs
    fs::create_dir_all(target.join("raw")).context("create raw")?;
    fs::create_dir_all(target.join("skill")).context("create skill")?;

    // Create wiki/ with stubs
    fs::create_dir_all(target.join(&config.generated_wiki)).context("create wiki")?;

    let ingest_content = generate_ingest_md(&config.ontology.kinds);
    fs::write(target.join("ingest.md"), ingest_content).context("write ingest.md")?;
    fs::write(target.join("wiki/index.md"), WIKI_INDEX_TEMPLATE).context("write wiki/index.md")?;
    fs::write(target.join("wiki/inbox.md"), WIKI_INBOX_TEMPLATE).context("write wiki/inbox.md")?;
    fs::write(target.join("wiki/log.md"), WIKI_LOG_TEMPLATE).context("write wiki/log.md")?;

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
        assert!(root.join("wiki/index.md").is_file());
        assert!(root.join("wiki/index.md").metadata().unwrap().len() > 0);
        assert!(root.join("wiki/inbox.md").is_file());
        assert!(root.join("wiki/inbox.md").metadata().unwrap().len() > 0);
        assert!(root.join("wiki/log.md").is_file());
        assert!(root.join("wiki/log.md").metadata().unwrap().len() > 0);
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

    #[test]
    fn init_creates_content_dirs() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("wiki");
        run_init(&root).unwrap();
        for kind in &[
            "concepts",
            "topics",
            "projects",
            "people",
            "syntheses",
            "sources",
        ] {
            assert!(
                root.join("content").join(kind).is_dir(),
                "content/{} should exist",
                kind
            );
        }
        assert!(root.join("content/pages").is_dir());
    }

    #[test]
    fn init_writes_v2_config() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("wiki");
        run_init(&root).unwrap();
        let toml_str = fs::read_to_string(root.join("agwiki.toml")).unwrap();
        let config: AgwikiConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.version, 2);
        assert_eq!(config.ontology.kinds.len(), 6);
    }

    #[test]
    fn init_ingest_md_lists_kinds() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("wiki");
        run_init(&root).unwrap();
        let content = fs::read_to_string(root.join("ingest.md")).unwrap();
        for kind in &[
            "concepts",
            "topics",
            "projects",
            "people",
            "syntheses",
            "sources",
        ] {
            assert!(content.contains(kind), "ingest.md should mention {}", kind);
        }
    }
}
