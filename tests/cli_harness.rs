//! In-process CLI harness tests using CliTestHarness.

mod harness_tests {
    use cli_framework::app::builder::AppBuilder;
    use cli_framework::app::context::AppContext;
    use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
    use cli_framework::path;
    use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
    use cli_framework::spec::command_tree::{
        CommandPath, CommandSpec, ExitCodeEntry, GroupMetadata,
    };
    use cli_framework::spec::value::ArgValue;
    use cli_framework::testkit::CliTestHarness;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::tempdir;

    struct TestCtx;
    impl AppContext for TestCtx {}

    // ── init typed args ──────────────────────────────────────────────────────

    struct InitArgs {
        dir: Option<PathBuf>,
    }

    impl IntoCommandSpec for InitArgs {
        fn command_spec() -> CommandSpec {
            CommandSpec {
                summary: "Create a new wiki root",
                syntax: Some("init [dir]"),
                category: Some("scaffold"),
                exit_codes: vec![ExitCodeEntry {
                    code: 0,
                    description: "Success",
                }],
                args: vec![ArgSpec {
                    name: "dir",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str(".".into())),
                    help: "Directory to initialise",
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

    // ── check wiki typed args ────────────────────────────────────────────────

    struct CheckWikiArgs {
        wiki_root: Option<PathBuf>,
        format: Option<String>,
    }

    impl IntoCommandSpec for CheckWikiArgs {
        fn command_spec() -> CommandSpec {
            CommandSpec {
                summary: "Check broken wikilinks and orphan pages",
                syntax: Some("check wiki [--format text|json]"),
                category: Some("check"),
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
                    ArgSpec {
                        name: "wiki-root",
                        kind: ArgKind::Option,
                        short: Some('C'),
                        value_type: ArgValueType::String,
                        cardinality: Cardinality::Optional,
                        help: "Root of the wiki",
                        ..Default::default()
                    },
                    ArgSpec {
                        name: "format",
                        kind: ArgKind::Option,
                        value_type: ArgValueType::Enum(vec!["text", "json"]),
                        cardinality: Cardinality::Optional,
                        default: Some(ArgValue::Enum("text".into())),
                        help: "Output format",
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }
        }
    }

    impl FromArgValueMap for CheckWikiArgs {
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
                format: map.get("format").and_then(|v| match v {
                    ArgValue::Str(s) => Some(s.clone()),
                    ArgValue::Enum(s) => Some(s.clone()),
                    _ => None,
                }),
            }
        }
    }

    // ── app builder ──────────────────────────────────────────────────────────

    fn build_test_app() -> cli_framework::app::App<TestCtx> {
        AppBuilder::new()
            .with_version("agwiki", env!("CARGO_PKG_VERSION"))
            .register::<InitArgs, _, _>(path!["init"], |_ctx, args| async move {
                let dir = args.dir.unwrap_or_else(|| PathBuf::from("."));
                agwiki::init::run_init(&dir)?;
                Ok(())
            })
            .unwrap()
            .register_group(
                &CommandPath::new(&["check"]).unwrap(),
                GroupMetadata {
                    summary: "Quality checks",
                    hidden: false,
                },
            )
            .unwrap()
            .register::<CheckWikiArgs, _, _>(path!["check", "wiki"], |_ctx, args| async move {
                let root = args
                    .wiki_root
                    .map(Ok)
                    .unwrap_or_else(|| std::env::current_dir().map_err(anyhow::Error::from))?;
                agwiki::upkeep::validate_wiki_root(&root)?;
                let report = agwiki::validate::validate_wiki(&root)?;
                let fmt = args.format.as_deref().unwrap_or("text");
                match fmt {
                    "json" => println!("{}", report.to_json()?),
                    _ => println!("{}", report.to_text()),
                }
                if !report.is_clean() {
                    std::process::exit(1);
                }
                Ok(())
            })
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
