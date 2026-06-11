//! Operator lifecycle hooks — shell commands run at ingest/materialize points.
//!
//! This is a **CLI-only** concern (see ADR 0002): `agwiki-core` never runs or
//! reads hooks. Hooks are configured under `[hooks]` in `.agwiki/config.toml` and
//! run via `sh -c` from the wiki root with `AGWIKI_*` env vars describing the
//! lifecycle event.

use std::path::Path;
use std::process::Command;

use anyhow::Result;

/// Run a hook command via `sh -c` from `wiki_root` with the given env vars.
///
/// stdout/stderr are inherited so hook output reaches the operator's terminal. A
/// non-zero exit is an error naming the hook command and exit code, unless
/// `continue_on_error` is set — in which case a warning is printed to stderr and
/// `Ok(())` is returned.
pub fn run_hook(
    cmd: &str,
    wiki_root: &Path,
    env: &[(&str, String)],
    continue_on_error: bool,
) -> Result<()> {
    let mut command = Command::new("sh");
    command.arg("-c").arg(cmd).current_dir(wiki_root);
    for (k, v) in env {
        command.env(k, v);
    }

    let status = command
        .status()
        .map_err(|e| anyhow::anyhow!("failed to spawn hook `{cmd}`: {e}"))?;

    if status.success() {
        return Ok(());
    }

    let code = status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "signal".to_string());

    if continue_on_error {
        eprintln!(
            "warning: hook `{cmd}` exited with status {code} (continue_on_error set; ignoring)"
        );
        Ok(())
    } else {
        Err(anyhow::anyhow!("hook `{cmd}` failed with exit code {code}"))
    }
}

/// Run a hook only when configured. A `None` command is a no-op returning `Ok`.
pub fn run_hook_if_set(
    cmd: &Option<String>,
    wiki_root: &Path,
    env: &[(&str, String)],
    continue_on_error: bool,
) -> Result<()> {
    match cmd {
        Some(c) => run_hook(c, wiki_root, env, continue_on_error),
        None => Ok(()),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn run_hook_true_succeeds() {
        let dir = tempdir().unwrap();
        assert!(run_hook("true", dir.path(), &[], false).is_ok());
    }

    #[test]
    fn run_hook_nonzero_errors() {
        let dir = tempdir().unwrap();
        let err = run_hook("exit 3", dir.path(), &[], false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("exit code 3"), "error: {msg}");
    }

    #[test]
    fn run_hook_nonzero_tolerated_under_continue_on_error() {
        let dir = tempdir().unwrap();
        assert!(run_hook("exit 3", dir.path(), &[], true).is_ok());
    }

    #[test]
    fn run_hook_if_set_none_is_noop() {
        let dir = tempdir().unwrap();
        assert!(run_hook_if_set(&None, dir.path(), &[], false).is_ok());
    }

    #[test]
    fn run_hook_runs_in_wiki_root_with_env() {
        let dir = tempdir().unwrap();
        let env = [("AGWIKI_TEST_VAR", "marker.txt".to_string())];
        run_hook("touch \"$AGWIKI_TEST_VAR\"", dir.path(), &env, false).unwrap();
        assert!(dir.path().join("marker.txt").exists());
    }
}
