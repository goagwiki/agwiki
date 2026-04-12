//! agwiki — agent-based wiki CLI.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use agwiki::export_skill::{run_export, ExportOptions};
use agwiki::ingest::{run_agent, Runner};
use agwiki::prep::prep;
use agwiki::toolkit::{default_prompt_path, expand_ingest_prompt, resolve_toolkit_root};
use agwiki::upkeep::{check_links, list_orphans, validate_wiki_root};

#[derive(Parser)]
#[command(
    name = "agwiki",
    version,
    about = "Agent-based wiki: prep, ingest, link checks, skill export"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Resolve and print absolute path to a .md ingest source
    #[command(
        after_help = "Example:\n  agwiki prep -C /path/to/wiki ./raw/note.md\n  agwiki prep -C /path/to/wiki --raw-only ./raw/note.md"
    )]
    Prep(PrepArgs),
    /// Expand ingest prompt and run aikit or opencode from the wiki root
    #[command(
        after_help = "Requires AGWIKI_ROOT (or FASTWIKI_ROOT / WIKIFY_ROOT) pointing at the agwiki toolkit.\n\nExample:\n  export AGWIKI_ROOT=/path/to/agwiki\n  agwiki ingest -C /path/to/content-wiki ./raw/note.md\n  agwiki ingest -C /path/to/wiki --runner opencode ./raw/note.md"
    )]
    Ingest(IngestArgs),
    /// Report broken wikilinks and relative markdown links under wiki/
    #[command(
        after_help = "Example:\n  agwiki check-links -C /path/to/wiki\n  Exits with status 1 if any broken link is found."
    )]
    CheckLinks(WikiRootArgs),
    /// List wiki pages with no incoming wikilink (excluding entry pages)
    #[command(after_help = "Example:\n  agwiki orphans -C /path/to/wiki")]
    Orphans(WikiRootArgs),
    /// Build skill/references and SKILL.md from template + wiki index
    #[command(
        after_help = "Example:\n  agwiki export-skill -C /path/to/wiki --prune\n  agwiki export-skill -C /path/to/wiki --dry-run"
    )]
    ExportSkill(ExportArgs),
}

#[derive(clap::Args)]
struct WikiRootArgs {
    #[arg(long = "wiki-root", short = 'C', value_name = "DIR", env = "WIKI_ROOT")]
    wiki_root: Option<PathBuf>,
}

#[derive(clap::Args)]
struct PrepArgs {
    #[command(flatten)]
    wiki: WikiRootArgs,
    #[arg(long)]
    raw_only: bool,
    file: PathBuf,
}

#[derive(clap::Args)]
struct IngestArgs {
    #[command(flatten)]
    wiki: WikiRootArgs,
    #[arg(long, default_value = "aikit", value_parser = parse_runner)]
    runner: Runner,
    file: PathBuf,
}

fn parse_runner(s: &str) -> Result<Runner, String> {
    match s.to_ascii_lowercase().as_str() {
        "aikit" => Ok(Runner::Aikit),
        "opencode" => Ok(Runner::Opencode),
        _ => Err("expected 'aikit' or 'opencode'".into()),
    }
}

#[derive(clap::Args)]
struct ExportArgs {
    #[command(flatten)]
    wiki: WikiRootArgs,
    #[arg(long, value_name = "PATH")]
    skill_dir: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    template: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    index: Option<PathBuf>,
    #[arg(long, default_value = "concepts,topics,projects")]
    sections: String,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    prune: bool,
    #[arg(long)]
    rewrite_wikilinks: bool,
}

fn resolve_wiki_root(opt: Option<PathBuf>) -> Result<PathBuf> {
    let p = opt
        .or_else(|| std::env::var("WIKI_ROOT").ok().map(PathBuf::from))
        .ok_or_else(|| anyhow::anyhow!("set --wiki-root/-C or WIKI_ROOT"))?;
    validate_wiki_root(&p)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Prep(a) => {
            let root = resolve_wiki_root(a.wiki.wiki_root)?;
            let path = prep(&root, &a.file, a.raw_only)?;
            println!("{}", path.display());
        }
        Commands::Ingest(a) => {
            let root = resolve_wiki_root(a.wiki.wiki_root)?;
            let ingest_path = prep(&root, &a.file, false)?;
            let toolkit = resolve_toolkit_root()?;
            let prompt_path = std::env::var("WIKI_INGEST_PROMPT")
                .ok()
                .map(PathBuf::from)
                .unwrap_or_else(|| default_prompt_path(&toolkit));
            let prompt = expand_ingest_prompt(&toolkit, &root, &ingest_path, &prompt_path)?;
            run_agent(&root, &prompt, a.runner)?;
        }
        Commands::CheckLinks(a) => {
            let root = resolve_wiki_root(a.wiki_root)?;
            let errs = check_links(&root)?;
            for e in &errs {
                println!("{}", e);
            }
            if !errs.is_empty() {
                std::process::exit(1);
            }
        }
        Commands::Orphans(a) => {
            let root = resolve_wiki_root(a.wiki_root)?;
            for p in list_orphans(&root)? {
                println!("{}", p.strip_prefix(&root).unwrap_or(&p).display());
            }
        }
        Commands::ExportSkill(a) => {
            let root = resolve_wiki_root(a.wiki.wiki_root)?;
            run_export(ExportOptions {
                wiki_root: &root,
                skill_dir: a.skill_dir.as_deref(),
                template: a.template.as_deref(),
                output: a.output.as_deref(),
                index: a.index.as_deref(),
                sections: &a.sections,
                dry_run: a.dry_run,
                prune: a.prune,
                rewrite_wikilinks: a.rewrite_wikilinks,
            })?;
        }
    }
    Ok(())
}
