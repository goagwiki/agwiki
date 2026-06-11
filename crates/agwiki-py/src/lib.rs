//! Python bindings for `agwiki-core`.
//!
//! This crate is a thin PyO3 extension module (`agwiki`) that exposes the
//! agwiki pipeline to Python so a script can ingest raw notes, materialize the
//! content model into a target layout (wiki / skill / html), and check a wiki —
//! all in-process, without shelling out to the `agwiki` CLI binary.
//!
//! The three entry points mirror the CLI verbs:
//! - [`ingest_file`] → `agwiki_core::ingest::run_ingest_file`
//! - [`materialize`] → `run_compile` / `run_export` / `run_export_html`
//! - [`check_wiki`] → `agwiki_core::validate::validate_wiki`
//!
//! All `anyhow::Error` values are mapped to Python `RuntimeError`.

use std::path::{Path, PathBuf};

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use agwiki_core::compile::{run_compile, run_export_html, CompileOptions};
use agwiki_core::event::{IngestEvent, PlanAction};
use agwiki_core::export_skill::{run_export, ExportOptions};
use agwiki_core::ingest::{run_ingest_file, IngestConfig, IngestFileOutcome};
use agwiki_core::validate::validate_wiki;

/// Map an `anyhow::Error` to a Python `RuntimeError`.
fn to_py_err(e: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(format!("{e:#}"))
}

/// Convert a single [`IngestEvent`] into a Python dict.
///
/// The dict always carries a `"type"` discriminator. Lifecycle variants
/// (`skipped`, `planned`) expose their fields directly; the nested aikit agent
/// event is serialized to JSON under a `"json"` key (its `AgentEvent` is
/// `Serialize`), with the agent key under `"agent_key"`.
fn ingest_event_to_dict<'py>(py: Python<'py>, event: &IngestEvent) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    match event {
        IngestEvent::Agent { agent_key, event } => {
            d.set_item("type", "agent")?;
            d.set_item("agent_key", agent_key)?;
            match serde_json::to_string(event) {
                Ok(json) => d.set_item("json", json)?,
                Err(e) => d.set_item("json_error", e.to_string())?,
            }
        }
        IngestEvent::AgentStderr(bytes) => {
            d.set_item("type", "agent_stderr")?;
            d.set_item("text", String::from_utf8_lossy(bytes).into_owned())?;
        }
        IngestEvent::AgentRunFinished => {
            d.set_item("type", "agent_run_finished")?;
        }
        IngestEvent::ProgressReset { source_key } => {
            d.set_item("type", "progress_reset")?;
            d.set_item("source_key", source_key)?;
        }
        IngestEvent::ProgressFinalFooter => {
            d.set_item("type", "progress_final_footer")?;
        }
        IngestEvent::Skipped { source_key } => {
            d.set_item("type", "skipped")?;
            d.set_item("source_key", source_key)?;
        }
        IngestEvent::Planned {
            source_key,
            action,
            reason,
            external_id,
        } => {
            d.set_item("type", "planned")?;
            d.set_item("source_key", source_key)?;
            d.set_item(
                "action",
                match action {
                    PlanAction::Ingest => "ingest",
                    PlanAction::Skip => "skip",
                },
            )?;
            d.set_item("reason", reason)?;
            d.set_item("external_id", external_id.clone())?;
        }
    }
    Ok(d)
}

/// Ingest a single source file into the wiki at `wiki_root`.
///
/// The ingest ledger lives at `<wiki_root>/.agwiki/ingest-state.jsonl` and the
/// ingest prompt at `<wiki_root>/ingest.md`. Returns a dict like
/// `{"outcome": "ingested"|"skipped", "source_key": ..., "external_id": ...}`.
///
/// If `on_event` is a callable, each emitted [`IngestEvent`] is delivered to it
/// as a dict after the (GIL-released) core run completes.
#[pyfunction]
#[pyo3(signature = (
    wiki_root,
    source,
    agent,
    model=None,
    external_id=None,
    force=false,
    dry_run=false,
    on_event=None,
))]
#[allow(clippy::too_many_arguments)]
fn ingest_file(
    py: Python<'_>,
    wiki_root: PathBuf,
    source: PathBuf,
    agent: String,
    model: Option<String>,
    external_id: Option<String>,
    force: bool,
    dry_run: bool,
    on_event: Option<Py<PyAny>>,
) -> PyResult<Py<PyDict>> {
    let prompt_path = wiki_root.join("ingest.md");
    let cfg = IngestConfig {
        force,
        ingest_state_path: wiki_root.join(".agwiki").join("ingest-state.jsonl"),
        external_id,
        dry_run,
    };

    // Collect events during the run, then deliver to Python afterward. This is
    // the simplest correct approach: the blocking core call holds no GIL, and
    // the Python callback runs only once the run finishes.
    let mut events: Vec<IngestEvent> = Vec::new();

    let result = py.detach(|| {
        let mut sink = |ev: IngestEvent| events.push(ev);
        run_ingest_file(
            &wiki_root,
            &source,
            &prompt_path,
            &agent,
            model.as_deref(),
            false, // stream
            false, // progress
            &cfg,
            &mut sink,
        )
    });

    let result = result.map_err(to_py_err)?;

    if let Some(cb) = on_event {
        for ev in &events {
            let dict = ingest_event_to_dict(py, ev)?;
            cb.call1(py, (dict,))?;
        }
    }

    let outcome = match result.outcome {
        IngestFileOutcome::Ingested => "ingested",
        IngestFileOutcome::Skipped => "skipped",
    };

    let d = PyDict::new(py);
    d.set_item("outcome", outcome)?;
    d.set_item("source_key", result.source_key)?;
    d.set_item("external_id", result.external_id)?;
    Ok(d.into())
}

/// Materialize the content model into a concrete target layout.
///
/// `target` is one of `"wiki"`, `"skill"`, or `"html"`, routing to
/// `run_compile`, `run_export`, and `run_export_html` respectively — the same
/// dispatch the CLI's `materialize --target` performs. Returns `None` on
/// success; raises `RuntimeError` on error (including target/flag misuse and a
/// non-empty compile error set).
#[pyfunction]
#[pyo3(signature = (
    wiki_root,
    target,
    out=None,
    skill_root=None,
    skill_md=None,
    prune=false,
    dry_run=false,
))]
#[allow(clippy::too_many_arguments)]
fn materialize(
    py: Python<'_>,
    wiki_root: PathBuf,
    target: String,
    out: Option<String>,
    skill_root: Option<PathBuf>,
    skill_md: Option<PathBuf>,
    prune: bool,
    dry_run: bool,
) -> PyResult<()> {
    py.detach(|| {
        materialize_impl(
            &wiki_root, &target, out, skill_root, skill_md, prune, dry_run,
        )
    })
    .map_err(to_py_err)
}

#[allow(clippy::too_many_arguments)]
fn materialize_impl(
    wiki_root: &Path,
    target: &str,
    out: Option<String>,
    skill_root: Option<PathBuf>,
    skill_md: Option<PathBuf>,
    prune: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    match target {
        "wiki" => {
            if skill_root.is_some() || skill_md.is_some() || prune {
                anyhow::bail!("--skill-root/--skill-md/--prune are only valid with target 'skill'");
            }
            if out.is_some() {
                anyhow::bail!("--out is only valid with target 'html'");
            }
            let report = run_compile(CompileOptions {
                wiki_root: wiki_root.to_path_buf(),
                dry_run,
            })?;
            if !report.errors.is_empty() {
                anyhow::bail!("compile failed with {} error(s)", report.errors.len());
            }
        }
        "skill" => {
            if out.is_some() {
                anyhow::bail!("--out is only valid with target 'html'");
            }
            run_export(ExportOptions {
                wiki_root,
                skill_root: skill_root.as_deref(),
                skill_md: skill_md.as_deref(),
                dry_run,
                prune,
            })?;
        }
        "html" => {
            if skill_root.is_some() || skill_md.is_some() || prune {
                anyhow::bail!("--skill-root/--skill-md/--prune are only valid with target 'skill'");
            }
            let out_str = out.as_deref().unwrap_or("dist/html");
            let out_path = PathBuf::from(out_str);
            let out_dir = if out_path.is_absolute() {
                out_path
            } else {
                wiki_root.join(out_path)
            };
            run_export_html(wiki_root, &out_dir)?;
        }
        other => {
            anyhow::bail!("unknown target '{other}': expected one of wiki, skill, or html");
        }
    }
    Ok(())
}

/// Check a wiki for broken wikilinks and orphan pages.
///
/// Returns a list of `{"kind": ..., "message": ...}` dicts; an empty list means
/// the wiki is clean.
#[pyfunction]
fn check_wiki(py: Python<'_>, wiki_root: PathBuf) -> PyResult<Vec<Py<PyDict>>> {
    let report = py.detach(|| validate_wiki(&wiki_root)).map_err(to_py_err)?;

    let mut out = Vec::with_capacity(report.problems.len());
    for problem in &report.problems {
        let d = PyDict::new(py);
        // ProblemKind is Serialize (snake_case); render to its string form.
        let kind = serde_json::to_value(problem.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{:?}", problem.kind));
        d.set_item("kind", kind)?;
        d.set_item("message", &problem.message)?;
        out.push(d.into());
    }
    Ok(out)
}

/// The `agwiki` Python extension module.
#[pymodule]
fn agwiki(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(ingest_file, m)?)?;
    m.add_function(wrap_pyfunction!(materialize, m)?)?;
    m.add_function(wrap_pyfunction!(check_wiki, m)?)?;
    Ok(())
}
