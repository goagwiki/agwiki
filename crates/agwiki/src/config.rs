//! Operator settings loaded from `<wiki-root>/.agwiki/config.toml` (git-ignored).
//!
//! This is a **CLI-only** concern: `agwiki-core` never reads it. It holds operator
//! defaults — currently the default `agent` and `model` for `ingest` — that fill in
//! flags the operator does not want to type on every run.
//!
//! It is deliberately distinct from the two other config surfaces:
//! - `agwiki.toml` — the committed wiki content **schema** (`agwiki-core`'s concern).
//! - `.agwiki/ingest-state.jsonl` — the ingest **ledger** (runtime state, not settings).
//!
//! Precedence for any resolved value (highest to lowest), matching cli-framework's
//! `project_config` convention: **CLI flag > `AGWIKI_*` env var > this file > none**.

use std::path::Path;

use cli_framework::project_config::{load_toml_file, ProjectConfigError};
use serde::Deserialize;

/// Operator-facing settings under `.agwiki/config.toml`. Every field is optional;
/// an absent file is equivalent to an empty config.
#[derive(Debug, Default, Deserialize)]
pub struct OperatorConfig {
    #[serde(default)]
    pub defaults: Defaults,
}

/// Default flag values for commands that accept an agent/model.
#[derive(Debug, Default, Deserialize)]
pub struct Defaults {
    /// Default agent key for `ingest` when `-a` and `AGWIKI_AGENT` are both absent.
    pub agent: Option<String>,
    /// Default model for `ingest` when `-m` and `AGWIKI_MODEL` are both absent.
    pub model: Option<String>,
}

impl OperatorConfig {
    /// Load `<wiki_root>/.agwiki/config.toml`.
    ///
    /// A missing file yields the default (empty) config — operator settings are
    /// optional. A present-but-malformed file is a hard error so typos are not
    /// silently ignored.
    pub fn load(wiki_root: &Path) -> anyhow::Result<Self> {
        let path = wiki_root.join(".agwiki/config.toml");
        match load_toml_file::<OperatorConfig>(&path) {
            Ok(cfg) => Ok(cfg),
            Err(ProjectConfigError::IoError { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(OperatorConfig::default())
            }
            Err(e) => Err(anyhow::Error::new(e)),
        }
    }
}

/// Resolve a value by precedence: first non-empty of `flag`, then `env`, then
/// `config`. Whitespace-only values are treated as absent so an empty flag falls
/// through to the next source.
pub fn pick(flag: Option<String>, env: Option<String>, config: Option<String>) -> Option<String> {
    [flag, env, config]
        .into_iter()
        .flatten()
        .map(|s| s.trim().to_owned())
        .find(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn pick_prefers_flag_then_env_then_config() {
        assert_eq!(
            pick(Some("flag".into()), Some("env".into()), Some("cfg".into())).as_deref(),
            Some("flag")
        );
        assert_eq!(
            pick(None, Some("env".into()), Some("cfg".into())).as_deref(),
            Some("env")
        );
        assert_eq!(pick(None, None, Some("cfg".into())).as_deref(), Some("cfg"));
        assert_eq!(pick(None, None, None), None);
    }

    #[test]
    fn pick_treats_blank_as_absent_and_trims() {
        // Blank flag falls through to env.
        assert_eq!(
            pick(Some("   ".into()), Some("env".into()), None).as_deref(),
            Some("env")
        );
        // Surrounding whitespace is trimmed off the winner.
        assert_eq!(
            pick(Some("  codex  ".into()), None, None).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn load_missing_file_is_empty_config() {
        let dir = tempdir().unwrap();
        let cfg = OperatorConfig::load(dir.path()).unwrap();
        assert!(cfg.defaults.agent.is_none());
        assert!(cfg.defaults.model.is_none());
    }

    #[test]
    fn load_reads_defaults() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agwiki")).unwrap();
        fs::write(
            dir.path().join(".agwiki/config.toml"),
            "[defaults]\nagent = \"codex\"\nmodel = \"gpt-5\"\n",
        )
        .unwrap();
        let cfg = OperatorConfig::load(dir.path()).unwrap();
        assert_eq!(cfg.defaults.agent.as_deref(), Some("codex"));
        assert_eq!(cfg.defaults.model.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn load_malformed_file_errors() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agwiki")).unwrap();
        fs::write(dir.path().join(".agwiki/config.toml"), "not = valid = toml").unwrap();
        assert!(OperatorConfig::load(dir.path()).is_err());
    }
}
