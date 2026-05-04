//! In-process CLI harness tests using CliTestHarness.

mod harness_tests {
    use cli_framework::app::builder::AppBuilder;
    use cli_framework::app::context::AppContext;
    use cli_framework::command::{Command, CommandArgs};
    use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
    use cli_framework::spec::command_tree::{
        CommandPath, CommandSpec, ExitCodeEntry, GroupMetadata,
    };
    use cli_framework::spec::value::ArgValue;
    use cli_framework::testkit::CliTestHarness;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::tempdir;

    struct TestCtx;
    impl AppContext for TestCtx {}

    #[allow(dead_code)]
    fn flag(args: &CommandArgs, key: &str) -> bool {
        args.named.get(key).map(|v| v == "true").unwrap_or(false)
    }

    fn opt<'a>(args: &'a CommandArgs, key: &str) -> Option<&'a str> {
        args.named
            .get(key)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }

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
            help: "Root of the wiki",
        }
    }

    fn make_init_cmd() -> Command {
        Command {
            id: "init",
            summary: "Create a new wiki root",
            syntax: Some("init [dir]"),
            category: Some("scaffold"),
            spec: Some(Arc::new(CommandSpec {
                summary: "Create a new wiki root",
                long_about: None,
                examples: vec![],
                aliases: vec![],
                hidden: false,
                deprecated: None,
                env_vars: vec![],
                exit_codes: vec![ExitCodeEntry {
                    code: 0,
                    description: "Success",
                }],
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
                    help: "Directory to initialise",
                }],
                notes: None,
            })),
            validator: None,
            execute: Arc::new(|_ctx, args| {
                Box::pin(async move {
                    let dir = args
                        .positional
                        .first()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("."));
                    agwiki::init::run_init(&dir)?;
                    Ok(())
                })
            }),
        }
    }

    fn make_check_wiki_cmd() -> Command {
        Command {
            id: "wiki",
            summary: "Check broken wikilinks and orphan pages",
            syntax: Some("check wiki [--format text|json]"),
            category: Some("check"),
            spec: Some(Arc::new(CommandSpec {
                summary: "Check broken wikilinks and orphan pages",
                long_about: None,
                examples: vec![],
                aliases: vec![],
                hidden: false,
                deprecated: None,
                env_vars: vec![],
                exit_codes: vec![
                    ExitCodeEntry {
                        code: 0,
                        description: "Clean",
                    },
                    ExitCodeEntry {
                        code: 1,
                        description: "Issues found",
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
                        help: "Output format",
                    },
                ],
                notes: None,
            })),
            validator: None,
            execute: Arc::new(|_ctx, args| {
                Box::pin(async move {
                    let root = opt(&args, "wiki-root")
                        .map(PathBuf::from)
                        .map(Ok)
                        .unwrap_or_else(|| std::env::current_dir().map_err(anyhow::Error::from))?;
                    agwiki::upkeep::validate_wiki_root(&root)?;
                    let report = agwiki::validate::validate_wiki(&root)?;
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

    fn build_test_app() -> cli_framework::app::App<TestCtx> {
        AppBuilder::new()
            .with_version("agwiki", env!("CARGO_PKG_VERSION"))
            .register_command(make_init_cmd())
            .unwrap()
            .register_group(
                &CommandPath::new(&["check"]).unwrap(),
                GroupMetadata {
                    summary: "Quality checks",
                    hidden: false,
                },
            )
            .unwrap()
            .register_command_at(
                &CommandPath::new(&["check", "wiki"]).unwrap(),
                make_check_wiki_cmd(),
            )
            .unwrap()
            .build(TestCtx)
            .unwrap()
    }

    #[tokio::test]
    async fn harness_init_creates_wiki_structure() {
        let tmp = tempdir().unwrap();
        let tmp_path = tmp.path().to_string_lossy().to_string();

        let app = build_test_app();
        let mut harness = CliTestHarness::new(app);
        let output = harness.run(&["agwiki", "init", &tmp_path]).await;

        assert_eq!(
            output.exit_code(),
            0,
            "expected exit 0, stderr: {}",
            output.stderr()
        );
        assert!(
            tmp.path().join("agwiki.toml").exists(),
            "expected agwiki.toml to exist"
        );
        assert!(tmp.path().join("wiki").is_dir(), "expected wiki/ directory");
        assert!(tmp.path().join("ingest.md").exists(), "expected ingest.md");
    }

    #[tokio::test]
    async fn harness_check_wiki_clean() {
        let tmp = tempdir().unwrap();
        agwiki::init::run_init(tmp.path()).unwrap();
        let tmp_path = tmp.path().to_string_lossy().to_string();

        let app = build_test_app();
        let mut harness = CliTestHarness::new(app);
        let output = harness
            .run(&[
                "agwiki",
                "check",
                "wiki",
                "--wiki-root",
                &tmp_path,
                "--format",
                "json",
            ])
            .await;

        assert_eq!(
            output.exit_code(),
            0,
            "expected exit 0, stderr: {}",
            output.stderr()
        );
    }
}
