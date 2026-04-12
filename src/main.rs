//! agwiki — agent-based wiki CLI.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use agwiki::export_skill::{run_export, ExportOptions};
use agwiki::ingest::{resolve_ingest_source, run_aikit};
use agwiki::init::run_init;
use agwiki::toolkit::{expand_ingest_prompt, require_wiki_ingest_prompt};
use agwiki::upkeep::validate_wiki_root;
use agwiki::validate::validate_wiki;

#[derive(Parser)]
#[command(
    name = "agwiki",
    version,
    about = "CLI for agent-driven markdown wikis (init, ingest, validate, skill export)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new wiki root: `agwiki.toml`, directory tree, and default `ingest.md`
    #[command(
        after_help = "Example:\n  agwiki init\n  agwiki init ./my-wiki\n  Fails if the target directory exists and is not empty."
    )]
    Init(InitArgs),
    /// Run ingest via `aikit run` with `--events` (NDJSON progress on stdout; see `aikit run --help`).
    ///
    /// Expands `{{INGEST_PATH}}` and `{{WIKI_ROOT}}` in `<wiki-root>/ingest.md`. **`-a` / `--agent` is required** (no default). Optional `-m`, `--stream` as for `aikit run`.
    #[command(
        after_help = "Example:\n  agwiki ingest -a opencode ./raw/note.md\n  agwiki ingest -C /path/to/wiki -a claude ./raw/note.md\n  agwiki ingest --stream -a opencode ./raw/note.md\n  agwiki ingest -a opencode -m MODEL ./raw/note.md\n  `-C` / `--wiki-root` defaults to the current working directory when omitted."
    )]
    Ingest(IngestArgs),
    /// Check broken wikilinks, relative markdown links, and orphan wiki pages
    #[command(
        after_help = "Example:\n  agwiki validate\n  agwiki validate -C /path/to/wiki\n  agwiki validate --format json\n  Exits with status 1 if any broken link or orphan page is found.\n  `-C` / `--wiki-root` defaults to the current working directory when omitted."
    )]
    Validate(ValidateArgs),
    /// Mirror `wiki/<top-level-dir>/` into the skill bundle and refresh the wiki index inside `SKILL.md` (agwiki HTML comment markers)
    #[command(
        long_about = "Copies markdown from each immediate subdirectory of wiki/ into skill/references/<name>/. \
Reads wiki/index.md to build a link index. Updates SKILL.md by replacing the block between \
<!-- agwiki:generated-index --> and <!-- /agwiki:generated-index -->, or appends that block if missing. \
Runs wiki validation and prints warnings on stderr if there are broken links or orphans (export still succeeds).",
        after_help = "Example:\n  agwiki export-skill\n  agwiki export-skill --prune\n  agwiki export-skill -C /path/to/wiki --dry-run\n  `-C` / `--wiki-root` defaults to the current working directory when omitted.\n  Use `agwiki validate` in CI for a non-zero exit on issues."
    )]
    ExportSkill(ExportArgs),
}

#[derive(clap::Args)]
struct WikiRootArgs {
    #[arg(
        long = "wiki-root",
        short = 'C',
        value_name = "DIR",
        help = "Root of the content repository; must contain a wiki/ directory (default: current working directory)"
    )]
    wiki_root: Option<PathBuf>,
}

#[derive(clap::Args)]
struct InitArgs {
    #[arg(
        default_value = ".",
        help = "Directory to create or populate as wiki root (must be empty if it already exists)"
    )]
    dir: PathBuf,
}

#[derive(ValueEnum, Clone, Copy, Default, Debug, PartialEq, Eq)]
enum ValidateFormat {
    #[default]
    Text,
    Json,
}

#[derive(clap::Args)]
struct ValidateArgs {
    #[command(flatten)]
    wiki: WikiRootArgs,
    #[arg(long, value_enum, default_value_t = ValidateFormat::Text)]
    format: ValidateFormat,
}

#[derive(clap::Args)]
struct IngestArgs {
    #[command(flatten)]
    wiki: WikiRootArgs,
    #[arg(
        short = 'a',
        long = "agent",
        value_name = "NAME",
        help = "Agent for `aikit run -a` (required; e.g. opencode, claude, codex, gemini)"
    )]
    agent: String,
    #[arg(
        short = 'm',
        long = "model",
        value_name = "MODEL",
        help = "Optional model for `aikit run -m`"
    )]
    model: Option<String>,
    #[arg(
        long,
        help = "Pass `--stream` to `aikit run` (with `--events`) for agent-native streaming where supported"
    )]
    stream: bool,
    #[arg(help = "Markdown source file (resolved from cwd, must exist, .md extension)")]
    file: PathBuf,
}

#[derive(clap::Args)]
struct ExportArgs {
    #[command(flatten)]
    wiki: WikiRootArgs,
    #[arg(
        long = "skill-root",
        value_name = "DIR",
        help_heading = "Skill bundle",
        help = "Agent Skill directory (default: <wiki-root>/skill)"
    )]
    skill_root: Option<PathBuf>,
    #[arg(
        long = "skill-md",
        value_name = "FILE",
        help = "SKILL.md path to create or update (default: <skill-root>/SKILL.md)"
    )]
    skill_md: Option<PathBuf>,
    #[arg(
        long,
        help_heading = "Behavior",
        help = "Print planned copies/prunes and generated index; do not write files"
    )]
    dry_run: bool,
    #[arg(
        long,
        help = "Remove files under skill/references/ when the source .md no longer exists in the wiki"
    )]
    prune: bool,
}

fn resolve_wiki_root(opt: Option<PathBuf>) -> Result<PathBuf> {
    let p = opt
        .map(Ok)
        .unwrap_or_else(|| std::env::current_dir().context("current directory"))?;
    validate_wiki_root(&p)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init(a) => {
            run_init(&a.dir)?;
        }
        Commands::Ingest(a) => {
            let root = resolve_wiki_root(a.wiki.wiki_root)?;
            let ingest_path = resolve_ingest_source(&a.file)?;
            let prompt_path = require_wiki_ingest_prompt(&root)?;
            let prompt = expand_ingest_prompt(&root, &ingest_path, &prompt_path)?;
            let agent = a.agent.trim();
            if agent.is_empty() {
                return Err(anyhow::anyhow!("--agent must not be empty"));
            }
            let model = a.model.as_deref().map(str::trim).filter(|s| !s.is_empty());
            run_aikit(&root, &prompt, agent, model, a.stream)?;
        }
        Commands::Validate(a) => {
            let root = resolve_wiki_root(a.wiki.wiki_root)?;
            let report = validate_wiki(&root)?;
            match a.format {
                ValidateFormat::Text => println!("{}", report.to_text()),
                ValidateFormat::Json => println!("{}", report.to_json()?),
            }
            if !report.is_clean() {
                std::process::exit(1);
            }
        }
        Commands::ExportSkill(a) => {
            let root = resolve_wiki_root(a.wiki.wiki_root)?;
            run_export(ExportOptions {
                wiki_root: &root,
                skill_root: a.skill_root.as_deref(),
                skill_md: a.skill_md.as_deref(),
                dry_run: a.dry_run,
                prune: a.prune,
            })?;
        }
    }
    Ok(())
}
