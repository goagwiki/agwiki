//! agwiki — agent-based wiki CLI.

use anyhow::{Context, Result};
use cli_framework::app::builder::AppBuilder;
use cli_framework::app::context::AppContext;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::path;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use std::collections::HashMap;
use std::path::PathBuf;

use agwiki::ingest_render::IngestRenderer;
use agwiki::serve::{run_serve_blocking, ServerConfig};
use agwiki_core::compile::{run_compile, run_export_html, run_new, CompileOptions};
use agwiki_core::export_skill::{run_export, ExportOptions};
use agwiki_core::ingest::{
    run_folder_ingest, run_folder_ingest_with_resume, run_ingest_file_with_resume,
    IngestResumeConfig,
};
use agwiki_core::init::run_init;
use agwiki_core::toolkit::require_wiki_ingest_prompt;
use agwiki_core::upkeep::validate_wiki_root;
use agwiki_core::validate::validate_wiki;

// ── Application context ──────────────────────────────────────────────────────

pub struct AgwikiContext;
impl AppContext for AgwikiContext {}

// ── Path resolution helpers (verbatim from pre-migration) ────────────────────

fn resolve_wiki_root(o: Option<PathBuf>) -> Result<PathBuf> {
    let p = o
        .map(Ok)
        .unwrap_or_else(|| std::env::current_dir().context("current directory"))?;
    validate_wiki_root(&p)
}

fn resolve_root(o: Option<PathBuf>) -> Result<PathBuf> {
    o.map(Ok)
        .unwrap_or_else(|| std::env::current_dir().context("current directory"))
}

fn resolve_ingest_state_path(wiki_root: &PathBuf, user: Option<PathBuf>) -> Result<PathBuf> {
    let Some(p) = user else {
        return Ok(wiki_root.join(".agwiki/ingest-state.jsonl"));
    };
    if p.is_absolute() {
        return Ok(p);
    }

    let mut out = wiki_root.clone();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::ParentDir => {
                if out == *wiki_root {
                    anyhow::bail!(
                        "--ingest-state path escapes <wiki-root>; pass an absolute path to write outside the wiki root"
                    );
                }
                out.pop();
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                anyhow::bail!("--ingest-state must be an absolute path or a relative path")
            }
        }
    }
    Ok(out)
}

// ── ArgSpec helpers ──────────────────────────────────────────────────────────

fn wiki_root_arg() -> ArgSpec {
    ArgSpec {
        name: "wiki-root",
        kind: ArgKind::Option,
        short: Some('C'),
        value_type: ArgValueType::String,
        cardinality: Cardinality::Optional,
        help: "Root of the content repository; must contain a wiki/ directory (default: cwd)",
        ..Default::default()
    }
}

// ── Typed args + specs ───────────────────────────────────────────────────────

// ── init ──────────────────────────────────────────────────────────────────────

pub struct InitArgs {
    pub dir: Option<PathBuf>,
}

impl IntoCommandSpec for InitArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Create a new wiki root",
            syntax: Some("init [dir]"),
            category: Some("scaffold"),
            long_about: Some(
                "Scaffolds agwiki.toml, directory tree, and default ingest.md. \
                 Fails if the target directory exists and is not empty.",
            ),
            examples: vec!["agwiki init", "agwiki init ./my-wiki"],
            exit_codes: vec![
                cli_framework::spec::command_tree::ExitCodeEntry {
                    code: 0,
                    description: "Success",
                },
                cli_framework::spec::command_tree::ExitCodeEntry {
                    code: 1,
                    description: "Target directory not empty or I/O error",
                },
            ],
            args: vec![ArgSpec {
                name: "dir",
                kind: ArgKind::Positional,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: Some(ArgValue::Str(".".into())),
                help: "Directory to create or populate as wiki root (must be empty if it exists)",
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for InitArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            dir: map
                .get("dir")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
        }
    }
}

async fn execute_init(args: InitArgs) -> Result<()> {
    let dir = args.dir.unwrap_or_else(|| PathBuf::from("."));
    run_init(&dir)?;
    Ok(())
}

// ── ingest ────────────────────────────────────────────────────────────────────

pub struct IngestArgs {
    pub wiki_root: Option<PathBuf>,
    pub agent: String,
    pub model: Option<String>,
    pub stream: bool,
    pub file: Option<PathBuf>,
    pub folder: Option<PathBuf>,
    pub max_files: usize,
    pub compile: bool,
    pub resume: bool,
    pub progress: bool,
    pub force: bool,
    pub ingest_state: Option<PathBuf>,
}

impl IntoCommandSpec for IngestArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Run ingest via aikit-sdk",
            syntax: Some("ingest -a <agent> [file | --folder <dir>] [options]"),
            category: Some("ingest"),
            long_about: Some(
                "Expands {{INGEST_PATH}} and {{WIKI_ROOT}} in <wiki-root>/ingest.md. \
                 -a / --agent is required.",
            ),
            examples: vec![
                "agwiki ingest -a opencode ./raw/note.md",
                "agwiki ingest -a codex --folder ./raw --max-files 0",
                "agwiki ingest --resume -a codex ./raw/note.md",
            ],
            exit_codes: vec![
                cli_framework::spec::command_tree::ExitCodeEntry {
                    code: 0,
                    description: "Success",
                },
                cli_framework::spec::command_tree::ExitCodeEntry {
                    code: 1,
                    description: "Ingest error or batch failures",
                },
            ],
            args: vec![
                wiki_root_arg(),
                ArgSpec {
                    name: "agent",
                    kind: ArgKind::Option,
                    short: Some('a'),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Agent key for aikit-sdk (required; e.g. opencode, claude, codex)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "model",
                    kind: ArgKind::Option,
                    short: Some('m'),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Optional model override passed to aikit-sdk",
                    ..Default::default()
                },
                ArgSpec {
                    name: "stream",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Enable agent-native streaming via aikit-sdk where supported",
                    ..Default::default()
                },
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    conflicts_with: vec!["folder"],
                    help: "Text source file to ingest (conflicts with --folder)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "folder",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    conflicts_with: vec!["file"],
                    help: "Ingest all *.md files under DIR recursively (batch mode)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "max-files",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Int(30)),
                    help: "Maximum number of files to ingest in --folder mode (0 = unlimited)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "compile",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Run agwiki compile after successful agent ingest",
                    ..Default::default()
                },
                ArgSpec {
                    name: "resume",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Enable resume mode: skip sources already successfully ingested",
                    ..Default::default()
                },
                ArgSpec {
                    name: "progress",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Display live human-readable progress on stderr; suppresses per-event NDJSON on stdout",
                    ..Default::default()
                },
                ArgSpec {
                    name: "force",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    requires: vec!["resume"],
                    help: "Force re-ingest even when resume finds a matching success record",
                    ..Default::default()
                },
                ArgSpec {
                    name: "ingest-state",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    requires: vec!["resume"],
                    help: "Path to the ingest-state JSONL ledger (default: <wiki-root>/.agwiki/ingest-state.jsonl)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for IngestArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            wiki_root: map
                .get("wiki-root")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
            agent: map
                .get("agent")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("fw bug: missing agent")),
            model: map
                .get("model")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .filter(|s| !s.trim().is_empty()),
            stream: matches!(map.get("stream"), Some(ArgValue::Bool(true))),
            file: map
                .get("file")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
            folder: map
                .get("folder")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
            max_files: map
                .get("max-files")
                .and_then(|v| {
                    if let ArgValue::Int(i) = v {
                        Some(*i as usize)
                    } else {
                        None
                    }
                })
                .unwrap_or(30),
            compile: matches!(map.get("compile"), Some(ArgValue::Bool(true))),
            resume: matches!(map.get("resume"), Some(ArgValue::Bool(true))),
            progress: matches!(map.get("progress"), Some(ArgValue::Bool(true))),
            force: matches!(map.get("force"), Some(ArgValue::Bool(true))),
            ingest_state: map
                .get("ingest-state")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
        }
    }
}

async fn execute_ingest(args: IngestArgs) -> Result<()> {
    let root = resolve_wiki_root(args.wiki_root)?;

    let agent_str = args.agent.trim().to_owned();
    if agent_str.is_empty() {
        anyhow::bail!("--agent must not be empty");
    }
    let agent = agent_str.as_str();

    let model = args
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let do_resume = args.resume;
    let do_force = args.force;
    let do_compile = args.compile;
    let do_stream = args.stream;
    let do_progress = args.progress;

    // E-ARGS-005: --force and --ingest-state require --resume
    if !do_resume && (do_force || args.ingest_state.is_some()) {
        anyhow::bail!("--force and --ingest-state require --resume");
    }

    let resume_cfg = if do_resume {
        let state_path = resolve_ingest_state_path(&root, args.ingest_state)?;
        Some(IngestResumeConfig {
            resume: true,
            force: do_force,
            ingest_state_path: state_path,
        })
    } else {
        None
    };

    let file = args.file;
    let folder = args.folder;
    let max_files = args.max_files;

    let mut renderer = IngestRenderer::new(do_progress);

    match (file, folder) {
        (Some(file), None) => {
            let prompt_path = require_wiki_ingest_prompt(&root)?;
            let mut sink = renderer.sink();
            run_ingest_file_with_resume(
                &root,
                &file,
                &prompt_path,
                agent,
                model.as_deref(),
                do_stream,
                do_progress,
                resume_cfg.as_ref(),
                &mut sink,
            )?;
        }
        (None, Some(folder)) => {
            let prompt_path = require_wiki_ingest_prompt(&root)?;
            if let Some(cfg) = resume_cfg.as_ref() {
                let mut sink = renderer.sink();
                let result = run_folder_ingest_with_resume(
                    &root,
                    &folder,
                    &prompt_path,
                    agent,
                    model.as_deref(),
                    do_stream,
                    do_progress,
                    max_files,
                    Some(cfg),
                    &mut sink,
                )?;
                drop(sink);
                eprintln!(
                    "Batch ingest: {} total, {} succeeded, {} skipped, {} failed.",
                    result.total,
                    result.succeeded,
                    result.skipped,
                    result.failures.len()
                );
                for (path, err) in &result.failures {
                    eprintln!("  FAILED: {} — {}", path.display(), err);
                }
                if !result.failures.is_empty() {
                    std::process::exit(1);
                }
            } else {
                let mut sink = renderer.sink();
                let result = run_folder_ingest(
                    &root,
                    &folder,
                    &prompt_path,
                    agent,
                    model.as_deref(),
                    do_stream,
                    do_progress,
                    max_files,
                    &mut sink,
                )?;
                drop(sink);
                eprintln!(
                    "Batch ingest: {} total, {} succeeded, {} failed.",
                    result.total,
                    result.succeeded,
                    result.failures.len()
                );
                for (path, err) in &result.failures {
                    eprintln!("  FAILED: {} — {}", path.display(), err);
                }
                if !result.failures.is_empty() {
                    std::process::exit(1);
                }
            }
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("cannot use both a file argument and --folder; choose one");
        }
        (None, None) => {
            anyhow::bail!("either a file argument or --folder is required");
        }
    }

    if do_compile {
        let report = run_compile(CompileOptions {
            wiki_root: root,
            dry_run: false,
        })?;
        if !report.errors.is_empty() {
            std::process::exit(1);
        }
    }

    Ok(())
}

// ── new ───────────────────────────────────────────────────────────────────────

pub struct NewArgs {
    pub wiki_root: Option<PathBuf>,
    pub kind: String,
    pub title: Option<String>,
}

impl IntoCommandSpec for NewArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Create a new ontology entity source file under content/<kind>/",
            syntax: Some("new <kind> [--title <title>]"),
            category: Some("scaffold"),
            examples: vec![
                "agwiki new concepts --title \"Knowledge Graphs\"",
                "agwiki new people",
            ],
            exit_codes: vec![cli_framework::spec::command_tree::ExitCodeEntry {
                code: 0,
                description: "Success",
            }],
            args: vec![
                wiki_root_arg(),
                ArgSpec {
                    name: "kind",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Ontology kind to create, for example `concepts`",
                    ..Default::default()
                },
                ArgSpec {
                    name: "title",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Initial entity title",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for NewArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            wiki_root: map
                .get("wiki-root")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
            kind: map
                .get("kind")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("fw bug: missing kind")),
            title: map.get("title").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

async fn execute_new(args: NewArgs) -> Result<()> {
    let root = resolve_root(args.wiki_root)?;
    let path = run_new(&root, &args.kind, args.title.as_deref())?;
    println!("{}", path.display());
    Ok(())
}

// ── compile ───────────────────────────────────────────────────────────────────

pub struct CompileArgs {
    pub wiki_root: Option<PathBuf>,
    pub dry_run: bool,
}

impl IntoCommandSpec for CompileArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Validate content sources and render generated markdown into wiki/",
            syntax: Some("compile [--dry-run]"),
            category: Some("build"),
            examples: vec!["agwiki compile", "agwiki compile --dry-run"],
            exit_codes: vec![
                cli_framework::spec::command_tree::ExitCodeEntry {
                    code: 0,
                    description: "Success",
                },
                cli_framework::spec::command_tree::ExitCodeEntry {
                    code: 1,
                    description: "Compile errors found",
                },
            ],
            args: vec![
                wiki_root_arg(),
                ArgSpec {
                    name: "dry-run",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Validate and print planned writes without changing files",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for CompileArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            wiki_root: map
                .get("wiki-root")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
            dry_run: matches!(map.get("dry-run"), Some(ArgValue::Bool(true))),
        }
    }
}

async fn execute_compile(args: CompileArgs) -> Result<()> {
    let root = resolve_root(args.wiki_root)?;
    let report = run_compile(CompileOptions {
        wiki_root: root,
        dry_run: args.dry_run,
    })?;
    if !report.errors.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

// ── serve ─────────────────────────────────────────────────────────────────────

pub struct ServeArgs {
    pub wiki_root: Option<PathBuf>,
    pub port: u16,
    pub host: String,
    pub open_browser: bool,
}

impl IntoCommandSpec for ServeArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Start a local HTTP server to browse the wiki in a web UI",
            syntax: Some("serve [--port <port>] [--host <host>] [--open]"),
            category: Some("serve"),
            examples: vec![
                "agwiki serve",
                "agwiki serve --port 8081",
                "agwiki serve --open",
            ],
            exit_codes: vec![cli_framework::spec::command_tree::ExitCodeEntry {
                code: 0,
                description: "Server stopped",
            }],
            args: vec![
                wiki_root_arg(),
                ArgSpec {
                    name: "port",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Int(8080)),
                    help: "Port to listen on (default: 8080)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "host",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str("127.0.0.1".into())),
                    help: "Host/IP address to bind to (default: 127.0.0.1)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "open",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Automatically open wiki in default browser",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ServeArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            wiki_root: map
                .get("wiki-root")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
            port: map
                .get("port")
                .and_then(|v| {
                    if let ArgValue::Int(i) = v {
                        Some(*i as u16)
                    } else {
                        None
                    }
                })
                .unwrap_or(8080),
            host: map
                .get("host")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            open_browser: matches!(map.get("open"), Some(ArgValue::Bool(true))),
        }
    }
}

async fn execute_serve(args: ServeArgs) -> Result<()> {
    let root = resolve_wiki_root(args.wiki_root)?;
    run_serve_blocking(ServerConfig {
        port: args.port,
        host: args.host,
        open_browser: args.open_browser,
        wiki_root: root,
    })?;
    Ok(())
}

// ── export ────────────────────────────────────────────────────────────────────

pub struct ExportArgs {
    pub subcommand: String,
    pub wiki_root: Option<PathBuf>,
    pub skill_root: Option<PathBuf>,
    pub skill_md: Option<PathBuf>,
    pub dry_run: bool,
    pub prune: bool,
    pub out: Option<String>,
}

impl IntoCommandSpec for ExportArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Publish/export workflows: export skill or export html",
            syntax: Some("export <skill|html> [options]"),
            category: Some("export"),
            long_about: Some(
                "Subcommands: skill — mirror wiki/ into the skill bundle; html — static HTML export.",
            ),
            examples: vec![
                "agwiki export skill",
                "agwiki export skill --prune",
                "agwiki export skill --dry-run",
            ],
            exit_codes: vec![cli_framework::spec::command_tree::ExitCodeEntry {
                code: 0,
                description: "Success",
            }],
            args: vec![
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::Enum(vec!["skill", "html"]),
                    cardinality: Cardinality::Required,
                    help: "Export subcommand: skill or html",
                    ..Default::default()
                },
                wiki_root_arg(),
                ArgSpec {
                    name: "skill-root",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Agent Skill directory (default: <wiki-root>/skill)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "skill-md",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "SKILL.md path to create or update (default: <skill-root>/SKILL.md)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "dry-run",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Print planned copies/prunes and generated index; do not write files",
                    ..Default::default()
                },
                ArgSpec {
                    name: "prune",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Remove files under skill/references/ when the source .md no longer exists",
                    ..Default::default()
                },
                ArgSpec {
                    name: "out",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output directory for static HTML export (default: dist/html)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ExportArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            subcommand: map
                .get("subcommand")
                .and_then(|v| match v {
                    ArgValue::Str(s) => Some(s.clone()),
                    ArgValue::Enum(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("fw bug: missing subcommand")),
            wiki_root: map
                .get("wiki-root")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
            skill_root: map
                .get("skill-root")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
            skill_md: map
                .get("skill-md")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
            dry_run: matches!(map.get("dry-run"), Some(ArgValue::Bool(true))),
            prune: matches!(map.get("prune"), Some(ArgValue::Bool(true))),
            out: map.get("out").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

async fn execute_export(args: ExportArgs) -> Result<()> {
    match args.subcommand.as_str() {
        "skill" => {
            let root = resolve_wiki_root(args.wiki_root)?;
            run_export(ExportOptions {
                wiki_root: &root,
                skill_root: args.skill_root.as_deref(),
                skill_md: args.skill_md.as_deref(),
                dry_run: args.dry_run,
                prune: args.prune,
            })?;
        }
        "html" => {
            let root = resolve_root(args.wiki_root)?;
            let out_str = args.out.as_deref().unwrap_or("dist/html");
            let out = PathBuf::from(out_str);
            let out_dir = if out.is_absolute() {
                out
            } else {
                root.join(out)
            };
            run_export_html(&root, &out_dir)?;
        }
        _ => anyhow::bail!(
            "unknown subcommand '{}': expected 'skill' or 'html'",
            args.subcommand
        ),
    }
    Ok(())
}

// ── check ─────────────────────────────────────────────────────────────────────

pub struct CheckArgs {
    pub subcommand: String,
    pub wiki_root: Option<PathBuf>,
    pub format: Option<String>,
}

impl IntoCommandSpec for CheckArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Quality checks: check wiki or check sources",
            syntax: Some("check <wiki|sources> [options]"),
            category: Some("check"),
            long_about: Some(
                "Subcommands: wiki — check broken links and orphans; sources — validate ontology sources.",
            ),
            examples: vec![
                "agwiki check wiki",
                "agwiki check wiki --format json",
                "agwiki check sources",
            ],
            exit_codes: vec![
                cli_framework::spec::command_tree::ExitCodeEntry {
                    code: 0,
                    description: "Clean",
                },
                cli_framework::spec::command_tree::ExitCodeEntry {
                    code: 1,
                    description: "Issues found",
                },
            ],
            args: vec![
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::Enum(vec!["wiki", "sources"]),
                    cardinality: Cardinality::Required,
                    help: "Check subcommand: wiki or sources",
                    ..Default::default()
                },
                wiki_root_arg(),
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::Enum(vec!["text", "json"]),
                    cardinality: Cardinality::Optional,
                    help: "Output format for wiki subcommand: text (default) or json",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for CheckArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            subcommand: map
                .get("subcommand")
                .and_then(|v| match v {
                    ArgValue::Str(s) => Some(s.clone()),
                    ArgValue::Enum(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("fw bug: missing subcommand")),
            wiki_root: map
                .get("wiki-root")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .map(PathBuf::from),
            format: map.get("format").and_then(|v| match v {
                ArgValue::Str(s) => Some(s.clone()),
                ArgValue::Enum(s) => Some(s.clone()),
                _ => None,
            }),
        }
    }
}

async fn execute_check(args: CheckArgs) -> Result<()> {
    match args.subcommand.as_str() {
        "wiki" => {
            let root = resolve_wiki_root(args.wiki_root)?;
            let report = validate_wiki(&root)?;
            let fmt = args.format.as_deref().unwrap_or("text");
            match fmt {
                "json" => println!("{}", report.to_json()?),
                _ => println!("{}", report.to_text()),
            }
            if !report.is_clean() {
                std::process::exit(1);
            }
        }
        "sources" => {
            let root = resolve_root(args.wiki_root)?;
            let report = run_compile(CompileOptions {
                wiki_root: root,
                dry_run: true,
            })?;
            if !report.errors.is_empty() {
                std::process::exit(1);
            }
        }
        _ => anyhow::bail!(
            "unknown subcommand '{}': expected 'wiki' or 'sources'",
            args.subcommand
        ),
    }
    Ok(())
}

// ── Entry point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = AppBuilder::new()
        .with_version("agwiki", env!("CARGO_PKG_VERSION"))
        .register::<InitArgs, _, _>(path!["init"], |_ctx, args| async move {
            execute_init(args).await
        })?
        .register::<IngestArgs, _, _>(path!["ingest"], |_ctx, args| async move {
            execute_ingest(args).await
        })?
        .register::<NewArgs, _, _>(
            path!["new"],
            |_ctx, args| async move { execute_new(args).await },
        )?
        .register::<CompileArgs, _, _>(path!["compile"], |_ctx, args| async move {
            execute_compile(args).await
        })?
        .register::<ServeArgs, _, _>(path!["serve"], |_ctx, args| async move {
            execute_serve(args).await
        })?
        .register::<ExportArgs, _, _>(path!["export"], |_ctx, args| async move {
            execute_export(args).await
        })?
        .register::<CheckArgs, _, _>(path!["check"], |_ctx, args| async move {
            execute_check(args).await
        })?
        .build(AgwikiContext)?;
    app.run().await
}
