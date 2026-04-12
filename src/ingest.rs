//! Run the coding agent (`aikit` or `opencode`) with the expanded ingest prompt on stdin.

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Which CLI runs the prompt (working directory is always `wiki_root`).
#[derive(Clone, Copy, Debug, Default)]
pub enum Runner {
    /// `aikit run -a opencode` (prompt on stdin).
    #[default]
    Aikit,
    /// `opencode run` (prompt on stdin).
    Opencode,
}

/// Spawn the agent, write `prompt` to its stdin, wait for exit, propagate failure.
pub fn run_agent(wiki_root: &Path, prompt: &str, runner: Runner) -> Result<()> {
    match runner {
        Runner::Aikit => {
            let mut cmd = Command::new("aikit");
            cmd.args(["run", "-a", "opencode"])
                .current_dir(wiki_root)
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
        }
        Runner::Opencode => {
            let mut cmd = Command::new("opencode");
            cmd.arg("run")
                .current_dir(wiki_root)
                .stdin(Stdio::piped())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            let mut child = cmd
                .spawn()
                .context("failed to spawn `opencode` (is it on PATH?)")?;
            let mut stdin = child.stdin.take().context("stdin")?;
            stdin.write_all(prompt.as_bytes())?;
            drop(stdin);
            let st = child.wait()?;
            if !st.success() {
                bail!("opencode exited with status {:?}", st.code());
            }
        }
    }
    Ok(())
}
