//! agwiki — agent-based wiki CLI.

use anyhow::{Context, Result};
use cli_framework::app::builder::AppBuilder;
use cli_framework::app::context::AppContext;
use cli_framework::command::{Command, CommandArgs};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::{CommandPath, CommandSpec, ExitCodeEntry, GroupMetadata};
use cli_framework::spec::value::ArgValue;
use std::path::PathBuf;
use std::sync::Arc;

use agwiki::compile::{run_compile, run_export_html, run_new, CompileOptions};
use agwiki::export_skill::{run_export, ExportOptions};
use agwiki::ingest::{
    run_folder_ingest, run_folder_ingest_with_resume, run_ingest_file_with_resume,
    IngestResumeConfig,
};
use agwiki::init::run_init;
use agwiki::serve::{run_serve_blocking, ServerConfig};
use agwiki::toolkit::require_wiki_ingest_prompt;
use agwiki::upkeep::validate_wiki_root;
use agwiki::validate::validate_wiki;

// ── Application context ──────────────────────────────────────────────────────

pub struct AgwikiContext;
impl AppContext for AgwikiContext {}

// ── Argument extraction helpers ──────────────────────────────────────────────

pub fn flag(args: &CommandArgs, key: &str) -> bool {
    args.named.get(key).map(|v| v == "true").unwrap_or(false)
}

pub fn opt<'a>(args: &'a CommandArgs, key: &str) -> Option<&'a str> {
    args.named
        .get(key)
        .map(String::as_str)
        .filter(|s| !s.is_empty())
}

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

// ── CommandSpec constructors ─────────────────────────────────────────────────

fn wiki_root_arg() -> ArgSpec {
    ArgSpec {
        name: "wiki-root",
        kind: ArgKind::Option,
        short: Some('C'),
        long: None,
        value_type: ArgValueType::String,
        cardinality: Cardinality::Optional,
        default: None,
        conflicts_with: vec![],
        requires: vec![],
        help: "Root of the content repository; must contain a wiki/ directory (default: cwd)",
    }
}

fn init_spec() -> CommandSpec {
    CommandSpec {
        summary: "Create a new wiki root",
        long_about: Some(
            "Scaffolds agwiki.toml, directory tree, and default ingest.md. \
             Fails if the target directory exists and is not empty.",
        ),
        examples: vec!["agwiki init", "agwiki init ./my-wiki"],
        aliases: vec![],
        hidden: false,
        deprecated: None,
        env_vars: vec![],
        exit_codes: vec![
            ExitCodeEntry {
                code: 0,
                description: "Success",
            },
            ExitCodeEntry {
                code: 1,
                description: "Target directory not empty or I/O error",
            },
        ],
        args: vec![ArgSpec {
            name: "dir",
            kind: ArgKind::Positional,
            short: None,
            long: None,
            value_type: ArgValueType::String,
            cardinality: Cardinality::Optional,
            default: Some(ArgValue::Str(".".into())),
            conflicts_with: vec![],
            requires: vec![],
            help: "Directory to create or populate as wiki root (must be empty if it exists)",
        }],
        notes: None,
    }
}

fn ingest_spec() -> CommandSpec {
    CommandSpec {
        summary: "Run ingest via aikit-sdk",
        long_about: Some(
            "Expands {{INGEST_PATH}} and {{WIKI_ROOT}} in <wiki-root>/ingest.md. \
             -a / --agent is required.",
        ),
        examples: vec![
            "agwiki ingest -a opencode ./raw/note.md",
            "agwiki ingest -a codex --folder ./raw --max-files 0",
            "agwiki ingest --resume -a codex ./raw/note.md",
        ],
        aliases: vec![],
        hidden: false,
        deprecated: None,
        env_vars: vec![],
        exit_codes: vec![
            ExitCodeEntry { code: 0, description: "Success" },
            ExitCodeEntry { code: 1, description: "Ingest error or batch failures" },
        ],
        args: vec![
            wiki_root_arg(),
            ArgSpec {
                name: "agent",
                kind: ArgKind::Option,
                short: Some('a'),
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Required,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Agent key for aikit-sdk (required; e.g. opencode, claude, codex)",
            },
            ArgSpec {
                name: "model",
                kind: ArgKind::Option,
                short: Some('m'),
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Optional model override passed to aikit-sdk",
            },
            ArgSpec {
                name: "stream",
                kind: ArgKind::Flag,
                short: None,
                long: None,
                value_type: ArgValueType::Bool,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Enable agent-native streaming via aikit-sdk where supported",
            },
            ArgSpec {
                name: "file",
                kind: ArgKind::Positional,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec!["folder"],
                requires: vec![],
                help: "Text source file to ingest (conflicts with --folder)",
            },
            ArgSpec {
                name: "folder",
                kind: ArgKind::Option,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec!["file"],
                requires: vec![],
                help: "Ingest all *.md files under DIR recursively (batch mode)",
            },
            ArgSpec {
                name: "max-files",
                kind: ArgKind::Option,
                short: None,
                long: None,
                value_type: ArgValueType::Int,
                cardinality: Cardinality::Optional,
                default: Some(ArgValue::Int(30)),
                conflicts_with: vec![],
                requires: vec![],
                help: "Maximum number of files to ingest in --folder mode (0 = unlimited)",
            },
            ArgSpec {
                name: "compile",
                kind: ArgKind::Flag,
                short: None,
                long: None,
                value_type: ArgValueType::Bool,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Run agwiki compile after successful agent ingest",
            },
            ArgSpec {
                name: "resume",
                kind: ArgKind::Flag,
                short: None,
                long: None,
                value_type: ArgValueType::Bool,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Enable resume mode: skip sources already successfully ingested",
            },
            ArgSpec {
                name: "force",
                kind: ArgKind::Flag,
                short: None,
                long: None,
                value_type: ArgValueType::Bool,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec!["resume"],
                help: "Force re-ingest even when resume finds a matching success record",
            },
            ArgSpec {
                name: "ingest-state",
                kind: ArgKind::Option,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec!["resume"],
                help: "Path to the ingest-state JSONL ledger (default: <wiki-root>/.agwiki/ingest-state.jsonl)",
            },
        ],
        notes: None,
    }
}

fn new_spec() -> CommandSpec {
    CommandSpec {
        summary: "Create a new ontology entity source file under content/<kind>/",
        long_about: None,
        examples: vec![
            "agwiki new concepts --title \"Knowledge Graphs\"",
            "agwiki new people",
        ],
        aliases: vec![],
        hidden: false,
        deprecated: None,
        env_vars: vec![],
        exit_codes: vec![ExitCodeEntry {
            code: 0,
            description: "Success",
        }],
        args: vec![
            wiki_root_arg(),
            ArgSpec {
                name: "kind",
                kind: ArgKind::Positional,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Required,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Ontology kind to create, for example `concepts`",
            },
            ArgSpec {
                name: "title",
                kind: ArgKind::Option,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Initial entity title",
            },
        ],
        notes: None,
    }
}

fn compile_spec() -> CommandSpec {
    CommandSpec {
        summary: "Validate content sources and render generated markdown into wiki/",
        long_about: None,
        examples: vec!["agwiki compile", "agwiki compile --dry-run"],
        aliases: vec![],
        hidden: false,
        deprecated: None,
        env_vars: vec![],
        exit_codes: vec![
            ExitCodeEntry {
                code: 0,
                description: "Success",
            },
            ExitCodeEntry {
                code: 1,
                description: "Compile errors found",
            },
        ],
        args: vec![
            wiki_root_arg(),
            ArgSpec {
                name: "dry-run",
                kind: ArgKind::Flag,
                short: None,
                long: None,
                value_type: ArgValueType::Bool,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Validate and print planned writes without changing files",
            },
        ],
        notes: None,
    }
}

fn serve_spec() -> CommandSpec {
    CommandSpec {
        summary: "Start a local HTTP server to browse the wiki in a web UI",
        long_about: None,
        examples: vec![
            "agwiki serve",
            "agwiki serve --port 8081",
            "agwiki serve --open",
        ],
        aliases: vec![],
        hidden: false,
        deprecated: None,
        env_vars: vec![],
        exit_codes: vec![ExitCodeEntry {
            code: 0,
            description: "Server stopped",
        }],
        args: vec![
            wiki_root_arg(),
            ArgSpec {
                name: "port",
                kind: ArgKind::Option,
                short: None,
                long: None,
                value_type: ArgValueType::Int,
                cardinality: Cardinality::Optional,
                default: Some(ArgValue::Int(8080)),
                conflicts_with: vec![],
                requires: vec![],
                help: "Port to listen on (default: 8080)",
            },
            ArgSpec {
                name: "host",
                kind: ArgKind::Option,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: Some(ArgValue::Str("127.0.0.1".into())),
                conflicts_with: vec![],
                requires: vec![],
                help: "Host/IP address to bind to (default: 127.0.0.1)",
            },
            ArgSpec {
                name: "open",
                kind: ArgKind::Flag,
                short: None,
                long: None,
                value_type: ArgValueType::Bool,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Automatically open wiki in default browser",
            },
        ],
        notes: None,
    }
}

fn export_skill_spec() -> CommandSpec {
    CommandSpec {
        summary: "Mirror wiki/ into the skill bundle and refresh the wiki index inside SKILL.md",
        long_about: Some(
            "Copies markdown from each immediate subdirectory of wiki/ into \
             skill/references/<name>/. Updates SKILL.md by replacing the generated-index block.",
        ),
        examples: vec![
            "agwiki export skill",
            "agwiki export skill --prune",
            "agwiki export skill --dry-run",
        ],
        aliases: vec![],
        hidden: false,
        deprecated: None,
        env_vars: vec![],
        exit_codes: vec![ExitCodeEntry {
            code: 0,
            description: "Success",
        }],
        args: vec![
            wiki_root_arg(),
            ArgSpec {
                name: "skill-root",
                kind: ArgKind::Option,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Agent Skill directory (default: <wiki-root>/skill)",
            },
            ArgSpec {
                name: "skill-md",
                kind: ArgKind::Option,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "SKILL.md path to create or update (default: <skill-root>/SKILL.md)",
            },
            ArgSpec {
                name: "dry-run",
                kind: ArgKind::Flag,
                short: None,
                long: None,
                value_type: ArgValueType::Bool,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Print planned copies/prunes and generated index; do not write files",
            },
            ArgSpec {
                name: "prune",
                kind: ArgKind::Flag,
                short: None,
                long: None,
                value_type: ArgValueType::Bool,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Remove files under skill/references/ when the source .md no longer exists",
            },
        ],
        notes: None,
    }
}

fn export_html_spec() -> CommandSpec {
    CommandSpec {
        summary: "Export generated wiki markdown as a static HTML tree",
        long_about: None,
        examples: vec!["agwiki export html", "agwiki export html --out dist/html"],
        aliases: vec![],
        hidden: false,
        deprecated: None,
        env_vars: vec![],
        exit_codes: vec![ExitCodeEntry {
            code: 0,
            description: "Success",
        }],
        args: vec![
            wiki_root_arg(),
            ArgSpec {
                name: "out",
                kind: ArgKind::Option,
                short: None,
                long: None,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: Some(ArgValue::Str("dist/html".into())),
                conflicts_with: vec![],
                requires: vec![],
                help: "Output directory for static HTML (default: dist/html)",
            },
        ],
        notes: None,
    }
}

fn check_sources_spec() -> CommandSpec {
    CommandSpec {
        summary: "Validate ontology content sources without writing generated wiki files",
        long_about: None,
        examples: vec![
            "agwiki check sources",
            "agwiki check sources -C /path/to/wiki",
        ],
        aliases: vec![],
        hidden: false,
        deprecated: None,
        env_vars: vec![],
        exit_codes: vec![
            ExitCodeEntry {
                code: 0,
                description: "No errors",
            },
            ExitCodeEntry {
                code: 1,
                description: "Validation errors found",
            },
        ],
        args: vec![wiki_root_arg()],
        notes: None,
    }
}

fn check_wiki_spec() -> CommandSpec {
    CommandSpec {
        summary: "Check broken wikilinks, relative markdown links, and orphan wiki pages",
        long_about: Some("Exits with status 1 if any broken link or orphan page is found."),
        examples: vec!["agwiki check wiki", "agwiki check wiki --format json"],
        aliases: vec![],
        hidden: false,
        deprecated: None,
        env_vars: vec![],
        exit_codes: vec![
            ExitCodeEntry {
                code: 0,
                description: "Wiki is clean",
            },
            ExitCodeEntry {
                code: 1,
                description: "Broken links or orphans found",
            },
        ],
        args: vec![
            wiki_root_arg(),
            ArgSpec {
                name: "format",
                kind: ArgKind::Option,
                short: None,
                long: None,
                value_type: ArgValueType::Enum(vec!["text", "json"]),
                cardinality: Cardinality::Optional,
                default: Some(ArgValue::Enum("text".into())),
                conflicts_with: vec![],
                requires: vec![],
                help: "Output format: text (default) or json",
            },
        ],
        notes: None,
    }
}

// ── Command constructors ─────────────────────────────────────────────────────

fn make_init_command() -> Command {
    Command {
        id: "init",
        summary: "Create a new wiki root: agwiki.toml, directory tree, and default ingest.md",
        syntax: Some("init [dir]"),
        category: Some("scaffold"),
        spec: Some(Arc::new(init_spec())),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dir = args
                    .positional
                    .first()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                run_init(&dir)?;
                Ok(())
            })
        }),
    }
}

fn make_ingest_command() -> Command {
    Command {
        id: "ingest",
        summary: "Run ingest via aikit-sdk (NDJSON progress on stdout from SDK event callback)",
        syntax: Some("ingest -a <agent> [file | --folder <dir>] [options]"),
        category: Some("ingest"),
        spec: Some(Arc::new(ingest_spec())),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let root = resolve_wiki_root(opt(&args, "wiki-root").map(PathBuf::from))?;

                let agent_str = opt(&args, "agent")
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| anyhow::anyhow!("--agent must not be empty"))?;
                let agent = agent_str.trim();

                let model = opt(&args, "model")
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);

                let do_resume = flag(&args, "resume");
                let do_force = flag(&args, "force");
                let do_compile = flag(&args, "compile");
                let do_stream = flag(&args, "stream");

                // E-ARGS-005: --force and --ingest-state require --resume
                if !do_resume && (do_force || opt(&args, "ingest-state").is_some()) {
                    anyhow::bail!("--force and --ingest-state require --resume");
                }

                let resume_cfg = if do_resume {
                    let state_path = resolve_ingest_state_path(
                        &root,
                        opt(&args, "ingest-state").map(PathBuf::from),
                    )?;
                    Some(IngestResumeConfig {
                        resume: true,
                        force: do_force,
                        ingest_state_path: state_path,
                    })
                } else {
                    None
                };

                let file = args.positional.first().map(PathBuf::from);
                let folder = opt(&args, "folder").map(PathBuf::from);
                let max_files = opt(&args, "max-files")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(30);

                match (file, folder) {
                    (Some(file), None) => {
                        let prompt_path = require_wiki_ingest_prompt(&root)?;
                        run_ingest_file_with_resume(
                            &root,
                            &file,
                            &prompt_path,
                            agent,
                            model.as_deref(),
                            do_stream,
                            resume_cfg.as_ref(),
                        )?;
                    }
                    (None, Some(folder)) => {
                        let prompt_path = require_wiki_ingest_prompt(&root)?;
                        if let Some(cfg) = resume_cfg.as_ref() {
                            let result = run_folder_ingest_with_resume(
                                &root,
                                &folder,
                                &prompt_path,
                                agent,
                                model.as_deref(),
                                do_stream,
                                max_files,
                                Some(cfg),
                            )?;
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
                            let result = run_folder_ingest(
                                &root,
                                &folder,
                                &prompt_path,
                                agent,
                                model.as_deref(),
                                do_stream,
                                max_files,
                            )?;
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
            })
        }),
    }
}

fn make_new_command() -> Command {
    Command {
        id: "new",
        summary: "Create a new ontology entity source file under content/<kind>/",
        syntax: Some("new <kind> [--title <title>]"),
        category: Some("scaffold"),
        spec: Some(Arc::new(new_spec())),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let root = resolve_root(opt(&args, "wiki-root").map(PathBuf::from))?;
                let kind = args
                    .positional
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("kind is required"))?;
                let title = opt(&args, "title");
                let path = run_new(&root, kind, title)?;
                println!("{}", path.display());
                Ok(())
            })
        }),
    }
}

fn make_compile_command() -> Command {
    Command {
        id: "compile",
        summary: "Validate content sources and render generated markdown into wiki/",
        syntax: Some("compile [--dry-run]"),
        category: Some("build"),
        spec: Some(Arc::new(compile_spec())),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let root = resolve_root(opt(&args, "wiki-root").map(PathBuf::from))?;
                let report = run_compile(CompileOptions {
                    wiki_root: root,
                    dry_run: flag(&args, "dry-run"),
                })?;
                if !report.errors.is_empty() {
                    std::process::exit(1);
                }
                Ok(())
            })
        }),
    }
}

fn make_serve_command() -> Command {
    Command {
        id: "serve",
        summary: "Start a local HTTP server to browse the wiki in a web UI",
        syntax: Some("serve [--port <port>] [--host <host>] [--open]"),
        category: Some("serve"),
        spec: Some(Arc::new(serve_spec())),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let root = resolve_wiki_root(opt(&args, "wiki-root").map(PathBuf::from))?;
                let port = opt(&args, "port")
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(8080);
                let host = opt(&args, "host")
                    .map(str::to_owned)
                    .unwrap_or_else(|| "127.0.0.1".to_string());
                let open_browser = flag(&args, "open");
                run_serve_blocking(ServerConfig {
                    port,
                    host,
                    open_browser,
                    wiki_root: root,
                })?;
                Ok(())
            })
        }),
    }
}

fn make_export_skill_command() -> Command {
    Command {
        id: "skill",
        summary: "Mirror wiki/ into the skill bundle and refresh the wiki index inside SKILL.md",
        syntax: Some("export skill [--dry-run] [--prune]"),
        category: Some("export"),
        spec: Some(Arc::new(export_skill_spec())),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let root = resolve_wiki_root(opt(&args, "wiki-root").map(PathBuf::from))?;
                let skill_root_buf = opt(&args, "skill-root").map(PathBuf::from);
                let skill_md_buf = opt(&args, "skill-md").map(PathBuf::from);
                let dry_run = flag(&args, "dry-run");
                let prune = flag(&args, "prune");
                run_export(ExportOptions {
                    wiki_root: &root,
                    skill_root: skill_root_buf.as_deref(),
                    skill_md: skill_md_buf.as_deref(),
                    dry_run,
                    prune,
                })?;
                Ok(())
            })
        }),
    }
}

fn make_export_html_command() -> Command {
    Command {
        id: "html",
        summary: "Export generated wiki markdown as a static HTML tree",
        syntax: Some("export html [--out <dir>]"),
        category: Some("export"),
        spec: Some(Arc::new(export_html_spec())),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let root = resolve_root(opt(&args, "wiki-root").map(PathBuf::from))?;
                let out_str = opt(&args, "out").unwrap_or("dist/html");
                let out = PathBuf::from(out_str);
                let out_dir = if out.is_absolute() {
                    out
                } else {
                    root.join(out)
                };
                run_export_html(&root, &out_dir)?;
                Ok(())
            })
        }),
    }
}

fn make_check_sources_command() -> Command {
    Command {
        id: "sources",
        summary: "Validate ontology content sources without writing generated wiki files",
        syntax: Some("check sources"),
        category: Some("check"),
        spec: Some(Arc::new(check_sources_spec())),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let root = resolve_root(opt(&args, "wiki-root").map(PathBuf::from))?;
                let report = run_compile(CompileOptions {
                    wiki_root: root,
                    dry_run: true,
                })?;
                if !report.errors.is_empty() {
                    std::process::exit(1);
                }
                Ok(())
            })
        }),
    }
}

fn make_check_wiki_command() -> Command {
    Command {
        id: "wiki",
        summary: "Check broken wikilinks, relative markdown links, and orphan wiki pages",
        syntax: Some("check wiki [--format text|json]"),
        category: Some("check"),
        spec: Some(Arc::new(check_wiki_spec())),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let root = resolve_wiki_root(opt(&args, "wiki-root").map(PathBuf::from))?;
                let report = validate_wiki(&root)?;
                let fmt = opt(&args, "format").unwrap_or("text");
                match fmt {
                    "json" => println!("{}", report.to_json()?),
                    _ => println!("{}", report.to_text()),
                }
                if !report.is_clean() {
                    std::process::exit(1);
                }
                Ok(())
            })
        }),
    }
}

// ── Entry point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = AppBuilder::new()
        .with_version("agwiki", env!("CARGO_PKG_VERSION"))
        .register_command(make_init_command())?
        .register_command(make_ingest_command())?
        .register_command(make_new_command())?
        .register_command(make_compile_command())?
        .register_command(make_serve_command())?
        .register_group(
            &CommandPath::new(&["export"])?,
            GroupMetadata {
                summary: "Parent command for publish/export workflows",
                hidden: false,
            },
        )?
        .register_command_at(
            &CommandPath::new(&["export", "skill"])?,
            make_export_skill_command(),
        )?
        .register_command_at(
            &CommandPath::new(&["export", "html"])?,
            make_export_html_command(),
        )?
        .register_group(
            &CommandPath::new(&["check"])?,
            GroupMetadata {
                summary: "Parent command for quality checks",
                hidden: false,
            },
        )?
        .register_command_at(
            &CommandPath::new(&["check", "sources"])?,
            make_check_sources_command(),
        )?
        .register_command_at(
            &CommandPath::new(&["check", "wiki"])?,
            make_check_wiki_command(),
        )?
        .build(AgwikiContext)?;
    app.run().await
}
