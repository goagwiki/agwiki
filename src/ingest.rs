//! Run ingest via `aikit_sdk::run_agent_events` with the expanded ingest prompt.
//!
//! The prompt is built from the wiki's `ingest.md` with `{{INGEST_PATH}}` and `{{WIKI_ROOT}}` filled in (`toolkit::expand_ingest_prompt`).
//! Always emits an NDJSON event stream on stdout via the SDK callback (one JSON line per event).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::toolkit::expand_ingest_prompt;
use aikit_sdk::{is_runnable, run_agent_events, runnable_agents, RunOptions};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const CODE_INGEST_STATE_LOCKED: &str = "AGWIKI_INGEST_STATE_LOCKED";
const CODE_INGEST_STATE_READ_FAILED: &str = "AGWIKI_INGEST_STATE_READ_FAILED";
const CODE_INGEST_STATE_WRITE_FAILED: &str = "AGWIKI_INGEST_STATE_WRITE_FAILED";
const CODE_INGEST_STATE_UTF8_PATH: &str = "AGWIKI_INGEST_STATE_UTF8_PATH";

/// Ledger record schema for a single successful ingest (JSON Lines / `.jsonl`).
///
/// Records are appended only after an agent run succeeds.
///
/// Example (one JSON line):
/// ```json
/// {"schema_version":1,"status":"success","wiki_root":"/abs/wiki","source_key":"raw/note.md","content_sha256":"<64-hex>","ingest_policy_sha256":"<64-hex>","agent":"codex","model":null,"completed_at":"2026-04-25T23:10:00Z","agwiki_version":"0.2.0"}
/// ```
///
/// Example (parse a JSONL line):
/// ```no_run
/// # use agwiki::ingest::IngestStateRecordV1;
/// let line = format!(
///   r#"{{"schema_version":1,"status":"success","wiki_root":"/abs/wiki","source_key":"raw/note.md","content_sha256":"{}","ingest_policy_sha256":"{}","agent":"codex","model":null,"completed_at":"2026-04-25T23:10:00Z","agwiki_version":"0.2.0"}}"#,
///   "0".repeat(64),
///   "1".repeat(64),
/// );
/// let rec: IngestStateRecordV1 = serde_json::from_str(&line)?;
/// assert_eq!(rec.schema_version, 1);
/// # Ok::<(), anyhow::Error>(())
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IngestStateRecordV1 {
    /// Schema version (MUST be `1`).
    pub schema_version: u32,
    /// Record status.
    pub status: IngestStatus,
    /// Canonical wiki root path as UTF-8.
    pub wiki_root: String,
    /// Stable source identity key.
    pub source_key: String,
    /// SHA-256 of source file bytes (lowercase hex).
    pub content_sha256: String,
    /// SHA-256 of `<wiki-root>/ingest.md` bytes (lowercase hex).
    pub ingest_policy_sha256: String,
    /// Agent key used for the ingest.
    pub agent: String,
    /// Optional model override.
    pub model: Option<String>,
    /// Completion time (RFC3339 UTC).
    pub completed_at: String,
    /// `agwiki` version (from `CARGO_PKG_VERSION`).
    pub agwiki_version: String,
}

/// Ledger record status.
///
/// Example:
/// ```no_run
/// # use agwiki::ingest::IngestStatus;
/// let s = serde_json::to_string(&IngestStatus::Success)?;
/// assert_eq!(s, "\"success\"");
/// # Ok::<(), anyhow::Error>(())
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IngestStatus {
    /// A completed ingest where the agent run succeeded.
    Success,
}

/// Identity key used to decide whether a prior success record can be reused.
///
/// A prior record is reusable only when all identity fields match and the record
/// status is `success`.
///
/// Example:
/// ```no_run
/// # use agwiki::ingest::IngestIdentity;
/// let id = IngestIdentity {
///   wiki_root: "/abs/wiki".to_string(),
///   source_key: "raw/note.md".to_string(),
///   content_sha256: "0".repeat(64),
///   ingest_policy_sha256: "1".repeat(64),
///   agent: "codex".to_string(),
///   model: None,
/// };
/// assert_eq!(id.source_key, "raw/note.md");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IngestIdentity {
    pub wiki_root: String,
    pub source_key: String,
    pub content_sha256: String,
    pub ingest_policy_sha256: String,
    pub agent: String,
    pub model: Option<String>,
}

/// Configuration for resume mode.
///
/// When `resume` is enabled, ingests consult an append-only JSONL ledger and may
/// skip sources with a matching prior `success` identity. When `force` is set,
/// sources are never skipped (but successes are still appended).
///
/// Example:
/// ```no_run
/// # use std::path::Path;
/// # use agwiki::ingest::IngestResumeConfig;
/// let cfg = IngestResumeConfig {
///   resume: true,
///   force: false,
///   ingest_state_path: Path::new(".agwiki/ingest-state.jsonl").to_path_buf(),
/// };
/// assert!(cfg.resume);
/// ```
#[derive(Debug, Clone)]
pub struct IngestResumeConfig {
    /// Enable resume ledger behavior.
    pub resume: bool,
    /// Force ingest even when a matching success record exists.
    pub force: bool,
    /// Path to the append-only JSON Lines ledger file.
    pub ingest_state_path: PathBuf,
}

#[derive(Debug)]
struct IngestStateLock {
    path: PathBuf,
    _file: std::fs::File,
}

impl IngestStateLock {
    fn acquire(ingest_state_path: &Path) -> Result<Self> {
        let lock_path = lock_path_for(ingest_state_path);
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                anyhow::anyhow!(
                    "{CODE_INGEST_STATE_LOCKED}: failed to create ingest-state lock parent dir {}: {e}",
                    parent.display()
                )
            })?;
        }
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => Ok(Self {
                path: lock_path,
                _file: file,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => bail!(
                "{CODE_INGEST_STATE_LOCKED}: ingest-state lock already held at {} (another ingest may be in progress)",
                lock_path.display()
            ),
            Err(e) => bail!(
                "{CODE_INGEST_STATE_LOCKED}: failed to acquire ingest-state lock at {}: {e}",
                lock_path.display()
            ),
        }
    }
}

impl Drop for IngestStateLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_path_for(ingest_state_path: &Path) -> PathBuf {
    let mut s = ingest_state_path.as_os_str().to_os_string();
    s.push(".lock");
    PathBuf::from(s)
}

fn path_to_utf8_slash(path: &Path) -> Result<String> {
    let mut out = String::new();
    let mut first = true;
    for c in path.components() {
        let part = match c {
            std::path::Component::Normal(s) => s.to_str().ok_or_else(|| {
                anyhow::anyhow!(
                    "{CODE_INGEST_STATE_UTF8_PATH}: path component is not valid UTF-8"
                )
            })?,
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => "..",
            std::path::Component::RootDir => continue,
            std::path::Component::Prefix(_) => {
                return Err(anyhow::anyhow!(
                    "{CODE_INGEST_STATE_UTF8_PATH}: path contains a platform prefix and cannot be represented as a UTF-8 relative source key"
                ))
            }
        };
        if first {
            first = false;
        } else {
            out.push('/');
        }
        out.push_str(part);
    }
    Ok(out)
}

/// Load the ingest-state ledger file into a lookup map keyed by [`IngestIdentity`].
///
/// MUST error on invalid JSON lines when `resume == true`.
///
/// Example:
/// ```no_run
/// # use std::path::Path;
/// # use agwiki::ingest::load_ingest_state;
/// let _state = load_ingest_state(Path::new(".agwiki/ingest-state.jsonl"), true)?;
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn load_ingest_state(
    path: &Path,
    resume: bool,
) -> Result<HashMap<IngestIdentity, IngestStateRecordV1>> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => bail!(
            "{CODE_INGEST_STATE_READ_FAILED}: failed to open ingest-state ledger {}: {e}",
            path.display()
        ),
    };

    let reader = std::io::BufReader::new(file);
    let mut out: HashMap<IngestIdentity, IngestStateRecordV1> = HashMap::new();

    for (line_no, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| {
            anyhow::anyhow!(
                "{CODE_INGEST_STATE_READ_FAILED}: failed to read ingest-state ledger {}: {e}",
                path.display()
            )
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let rec: IngestStateRecordV1 = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) if resume => {
                return Err(anyhow::anyhow!(
                    "{CODE_INGEST_STATE_READ_FAILED}: invalid JSON at {}:{}: {e}",
                    path.display(),
                    line_no + 1
                ))
            }
            Err(_) => continue,
        };
        if rec.schema_version != 1 || rec.status != IngestStatus::Success {
            continue;
        }
        let key = IngestIdentity {
            wiki_root: rec.wiki_root.clone(),
            source_key: rec.source_key.clone(),
            content_sha256: rec.content_sha256.clone(),
            ingest_policy_sha256: rec.ingest_policy_sha256.clone(),
            agent: rec.agent.clone(),
            model: rec.model.clone(),
        };
        out.insert(key, rec);
    }

    Ok(out)
}

/// Compute SHA-256 as lowercase hex (raw file bytes).
///
/// Example:
/// ```no_run
/// # use std::path::Path;
/// # use agwiki::ingest::sha256_hex_file;
/// let sha = sha256_hex_file(Path::new("ingest.md"))?;
/// assert_eq!(sha.len(), 64);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn sha256_hex_file(path: &Path) -> Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("read file for sha256: {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{:02x}", b)).collect())
}

/// Compute SHA-256 of `<wiki-root>/ingest.md` (raw file bytes, not expanded).
///
/// Example:
/// ```no_run
/// # use std::path::Path;
/// # use agwiki::ingest::ingest_policy_sha256;
/// let sha = ingest_policy_sha256(Path::new("."))?;
/// assert_eq!(sha.len(), 64);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn ingest_policy_sha256(wiki_root: &Path) -> Result<String> {
    sha256_hex_file(&wiki_root.join("ingest.md"))
}

/// Compute a stable `source_key` for a canonical source path.
///
/// If the source is under canonical `wiki_root`, the key is the UTF-8 relative
/// path from `wiki_root` using `/` separators. Otherwise, the key is the UTF-8
/// absolute canonical path.
///
/// Example:
/// ```no_run
/// # use std::path::Path;
/// # use agwiki::ingest::source_key_for;
/// let key = source_key_for(Path::new("/abs/wiki"), Path::new("/abs/wiki/raw/note.md"))?;
/// assert_eq!(key, "raw/note.md");
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn source_key_for(wiki_root: &Path, canonical_source: &Path) -> Result<String> {
    let canonical_source = canonical_source.canonicalize().with_context(|| {
        format!(
            "canonicalize ingest source for source_key: {}",
            canonical_source.display()
        )
    })?;

    if canonical_source.starts_with(wiki_root) {
        let rel = canonical_source
            .strip_prefix(wiki_root)
            .expect("prefix checked");
        return path_to_utf8_slash(rel);
    }

    if !canonical_source.is_absolute() {
        return Err(anyhow::anyhow!(
            "{CODE_INGEST_STATE_UTF8_PATH}: source path is not absolute after canonicalization: {}",
            canonical_source.display()
        ));
    }

    canonical_source
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{CODE_INGEST_STATE_UTF8_PATH}: source path is not valid UTF-8: {}",
                canonical_source.display()
            )
        })
}

/// Append a success record as a single JSON line.
///
/// MUST be called only after the agent succeeds.
///
/// Example:
/// ```no_run
/// # use std::path::Path;
/// # use agwiki::ingest::{append_ingest_success, IngestStateRecordV1, IngestStatus};
/// let rec = IngestStateRecordV1{
///   schema_version: 1,
///   status: IngestStatus::Success,
///   wiki_root: "/abs/wiki".to_string(),
///   source_key: "raw/note.md".to_string(),
///   content_sha256: "0".repeat(64),
///   ingest_policy_sha256: "1".repeat(64),
///   agent: "codex".to_string(),
///   model: None,
///   completed_at: "2026-01-01T00:00:00Z".to_string(),
///   agwiki_version: env!("CARGO_PKG_VERSION").to_string(),
/// };
/// append_ingest_success(Path::new(".agwiki/ingest-state.jsonl"), &rec)?;
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn append_ingest_success(path: &Path, record: &IngestStateRecordV1) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            anyhow::anyhow!(
                "{CODE_INGEST_STATE_WRITE_FAILED}: failed to create ingest-state parent dir {}: {e}",
                parent.display()
            )
        })?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| {
            anyhow::anyhow!(
                "{CODE_INGEST_STATE_WRITE_FAILED}: failed to open ingest-state ledger {} for append: {e}",
                path.display()
            )
        })?;

    let line = serde_json::to_string(record).map_err(|e| {
        anyhow::anyhow!(
            "{CODE_INGEST_STATE_WRITE_FAILED}: failed to serialize ingest-state record: {e}"
        )
    })?;
    writeln!(&mut file, "{}", line).map_err(|e| {
        anyhow::anyhow!(
            "{CODE_INGEST_STATE_WRITE_FAILED}: failed to append ingest-state record to {}: {e}",
            path.display()
        )
    })?;
    Ok(())
}

/// Outcome of a resume-aware single-file ingest.
///
/// Example:
/// ```no_run
/// # use agwiki::ingest::IngestFileOutcome;
/// let outcome = IngestFileOutcome::Skipped;
/// match outcome {
///   IngestFileOutcome::Ingested => {}
///   IngestFileOutcome::Skipped => {}
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestFileOutcome {
    /// The file was ingested (agent executed).
    Ingested,
    /// The file was skipped due to a matching prior success record.
    Skipped,
}

/// Run a single-file ingest with optional resume semantics.
///
/// MUST preserve existing stdout NDJSON behavior for agent events when ingesting.
/// When skipping, prints a single-line skip notice to stderr.
///
/// Example:
/// ```no_run
/// # use std::path::Path;
/// # use agwiki::ingest::{run_ingest_file_with_resume, IngestResumeConfig};
/// let cfg = IngestResumeConfig{ resume: true, force: false, ingest_state_path: Path::new(".agwiki/ingest-state.jsonl").to_path_buf() };
/// let _ = run_ingest_file_with_resume(Path::new("."), Path::new("raw/note.md"), Path::new("ingest.md"), "codex", None, false, Some(&cfg))?;
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn run_ingest_file_with_resume(
    wiki_root: &Path,
    source_file: &Path,
    prompt_path: &Path,
    agent: &str,
    model: Option<&str>,
    stream: bool,
    resume: Option<&IngestResumeConfig>,
) -> Result<IngestFileOutcome> {
    let Some(cfg) = resume.filter(|c| c.resume) else {
        run_ingest_for_path(wiki_root, source_file, prompt_path, agent, model, stream)?;
        return Ok(IngestFileOutcome::Ingested);
    };

    if let Some(parent) = cfg.ingest_state_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create ingest-state parent dir {}", parent.display()))?;
    }

    let _lock = IngestStateLock::acquire(&cfg.ingest_state_path)?;
    let state = load_ingest_state(&cfg.ingest_state_path, true)?;
    let policy_sha = ingest_policy_sha256(wiki_root)?;

    let ingest_path = resolve_ingest_source(source_file)?;
    let wiki_root_str = wiki_root.to_str().ok_or_else(|| {
        anyhow::anyhow!(
            "{CODE_INGEST_STATE_UTF8_PATH}: wiki root is not valid UTF-8: {}",
            wiki_root.display()
        )
    })?;
    let source_key = source_key_for(wiki_root, &ingest_path)?;
    let content_sha = sha256_hex_file(&ingest_path)?;

    let identity = IngestIdentity {
        wiki_root: wiki_root_str.to_string(),
        source_key: source_key.clone(),
        content_sha256: content_sha.clone(),
        ingest_policy_sha256: policy_sha.clone(),
        agent: agent.to_string(),
        model: model.map(|s| s.to_string()),
    };

    if !cfg.force && state.contains_key(&identity) {
        eprintln!(
            "SKIP: {} (already ingested under same policy/content/agent/model)",
            source_key
        );
        return Ok(IngestFileOutcome::Skipped);
    }

    let prompt = expand_ingest_prompt(wiki_root, &ingest_path, prompt_path)?;
    run_aikit(wiki_root, &prompt, agent, model, stream)?;

    let completed_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
    let record = IngestStateRecordV1 {
        schema_version: 1,
        status: IngestStatus::Success,
        wiki_root: wiki_root_str.to_string(),
        source_key,
        content_sha256: content_sha,
        ingest_policy_sha256: policy_sha,
        agent: agent.to_string(),
        model: model.map(|s| s.to_string()),
        completed_at,
        agwiki_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    append_ingest_success(&cfg.ingest_state_path, &record)?;
    Ok(IngestFileOutcome::Ingested)
}

/// Folder ingest summary with resume support.
///
/// Example:
/// ```no_run
/// # use agwiki::ingest::FolderIngestResultV2;
/// let r = FolderIngestResultV2 { total: 2, succeeded: 1, skipped: 1, failures: vec![] };
/// assert_eq!(r.skipped, 1);
/// ```
#[derive(Debug)]
pub struct FolderIngestResultV2 {
    pub total: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub failures: Vec<(PathBuf, String)>,
}

/// Run folder ingest with resume support and return summary including skipped count.
///
/// Existing [`run_folder_ingest`] remains available and preserves current behavior.
///
/// Example:
/// ```no_run
/// # use std::path::Path;
/// # use agwiki::ingest::{run_folder_ingest_with_resume, IngestResumeConfig};
/// let cfg = IngestResumeConfig{ resume: true, force: false, ingest_state_path: Path::new(".agwiki/ingest-state.jsonl").to_path_buf() };
/// let _ = run_folder_ingest_with_resume(Path::new("."), Path::new("raw"), Path::new("ingest.md"), "codex", None, false, 0, Some(&cfg))?;
/// # Ok::<(), anyhow::Error>(())
/// ```
#[allow(clippy::too_many_arguments)]
pub fn run_folder_ingest_with_resume(
    wiki_root: &Path,
    folder: &Path,
    prompt_path: &Path,
    agent: &str,
    model: Option<&str>,
    stream: bool,
    max_files: usize,
    resume: Option<&IngestResumeConfig>,
) -> Result<FolderIngestResultV2> {
    let Some(cfg) = resume.filter(|c| c.resume) else {
        let r = run_folder_ingest(
            wiki_root,
            folder,
            prompt_path,
            agent,
            model,
            stream,
            max_files,
        )?;
        return Ok(FolderIngestResultV2 {
            total: r.total,
            succeeded: r.succeeded,
            skipped: 0,
            failures: r.failures,
        });
    };

    if let Some(parent) = cfg.ingest_state_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create ingest-state parent dir {}", parent.display()))?;
    }

    let _lock = IngestStateLock::acquire(&cfg.ingest_state_path)?;
    let mut state = load_ingest_state(&cfg.ingest_state_path, true)?;
    let policy_sha = ingest_policy_sha256(wiki_root)?;

    let files = discover_md_files(folder)?;
    let total = files.len();

    if max_files > 0 && total > max_files {
        bail!(
            "found {} markdown file(s) under {}; exceeds --max-files cap of {}. \
             Pass --max-files {} (or higher) to proceed.",
            total,
            folder.display(),
            max_files,
            total
        );
    }

    let wiki_root_str = wiki_root.to_str().ok_or_else(|| {
        anyhow::anyhow!(
            "{CODE_INGEST_STATE_UTF8_PATH}: wiki root is not valid UTF-8: {}",
            wiki_root.display()
        )
    })?;

    let mut failures: Vec<(PathBuf, String)> = Vec::new();
    let mut skipped = 0usize;
    let mut succeeded = 0usize;

    for file in &files {
        let ingest_path = match resolve_ingest_source(file) {
            Ok(p) => p,
            Err(e) => {
                failures.push((file.clone(), e.to_string()));
                continue;
            }
        };

        let source_key = match source_key_for(wiki_root, &ingest_path) {
            Ok(k) => k,
            Err(e) => {
                failures.push((file.clone(), e.to_string()));
                continue;
            }
        };

        let content_sha = match sha256_hex_file(&ingest_path) {
            Ok(s) => s,
            Err(e) => {
                failures.push((file.clone(), e.to_string()));
                continue;
            }
        };

        let identity = IngestIdentity {
            wiki_root: wiki_root_str.to_string(),
            source_key: source_key.clone(),
            content_sha256: content_sha.clone(),
            ingest_policy_sha256: policy_sha.clone(),
            agent: agent.to_string(),
            model: model.map(|s| s.to_string()),
        };

        if !cfg.force && state.contains_key(&identity) {
            skipped += 1;
            eprintln!(
                "SKIP: {} (already ingested under same policy/content/agent/model)",
                source_key
            );
            continue;
        }

        let prompt = match expand_ingest_prompt(wiki_root, &ingest_path, prompt_path) {
            Ok(p) => p,
            Err(e) => {
                failures.push((file.clone(), e.to_string()));
                continue;
            }
        };

        if let Err(e) = run_aikit(wiki_root, &prompt, agent, model, stream) {
            failures.push((file.clone(), e.to_string()));
            continue;
        }

        let completed_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        let record = IngestStateRecordV1 {
            schema_version: 1,
            status: IngestStatus::Success,
            wiki_root: wiki_root_str.to_string(),
            source_key,
            content_sha256: content_sha,
            ingest_policy_sha256: policy_sha.clone(),
            agent: agent.to_string(),
            model: model.map(|s| s.to_string()),
            completed_at,
            agwiki_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        if let Err(e) = append_ingest_success(&cfg.ingest_state_path, &record) {
            failures.push((file.clone(), e.to_string()));
            continue;
        }
        state.insert(identity, record);
        succeeded += 1;
    }

    Ok(FolderIngestResultV2 {
        total,
        succeeded,
        skipped,
        failures,
    })
}

/// Canonicalize `file`, ensure it exists and contains valid text content.
pub fn resolve_ingest_source(file: &Path) -> Result<PathBuf> {
    let file = file
        .canonicalize()
        .with_context(|| format!("not found: {}", file.display()))?;

    validate_text_file(&file)?;

    Ok(file)
}

/// Validate that `path` contains text content (UTF-8 encoded, no null bytes).
fn validate_text_file(path: &Path) -> Result<()> {
    use std::fs::File;
    use std::io::Read;

    let mut file =
        File::open(path).with_context(|| format!("cannot read file: {}", path.display()))?;

    let mut buffer = [0u8; 8192];
    let bytes_read = file
        .read(&mut buffer)
        .with_context(|| format!("failed to read from file: {}", path.display()))?;

    let sample = &buffer[..bytes_read];

    // Check for null bytes (binary indicator)
    if sample.contains(&0) {
        bail!("file appears to be binary: {}", path.display());
    }

    // Validate UTF-8 encoding
    std::str::from_utf8(sample)
        .with_context(|| format!("file does not contain valid UTF-8 text: {}", path.display()))?;

    Ok(())
}

/// Run ingest via `aikit_sdk::run_agent_events`; emits NDJSON events on stdout.
pub fn run_aikit(
    wiki_root: &Path,
    prompt: &str,
    agent: &str,
    model: Option<&str>,
    stream: bool,
) -> Result<()> {
    if !is_runnable(agent) {
        bail!(
            "agent '{}' is not runnable; available agents: {}",
            agent,
            runnable_agents().join(", ")
        );
    }

    let mut opts = RunOptions::new()
        .with_current_dir(wiki_root.to_path_buf())
        .with_stream(stream);
    if let Some(m) = model {
        opts = opts.with_model(m.to_string());
    }

    let result = run_agent_events(agent, prompt, opts, |event| {
        if let Ok(s) = serde_json::to_string(&event) {
            println!("{}", s);
        }
    })
    .map_err(|e| anyhow::anyhow!("aikit-sdk agent execution failed: {}", e))?;

    let _ = std::io::stderr().write_all(&result.stderr);

    if !result.success() {
        bail!("agent exited with status {:?}", result.exit_code());
    }
    Ok(())
}

/// Discover all Markdown files (`*.md` / `*.MD`, case-insensitive) under `dir` recursively.
///
/// Does **not** follow symlinks. Returns paths sorted lexicographically by full path.
/// `dir` must exist and be a directory.
pub fn discover_md_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let canon_dir = dir
        .canonicalize()
        .with_context(|| format!("cannot access directory: {}", dir.display()))?;

    if !canon_dir.is_dir() {
        bail!("not a directory: {}", dir.display());
    }

    let mut results = Vec::new();
    let mut stack = vec![canon_dir];

    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("cannot read directory: {}", current.display()))?;

        for entry in entries {
            let entry =
                entry.with_context(|| format!("error reading entry in {}", current.display()))?;
            let file_type = entry
                .file_type()
                .with_context(|| format!("cannot get file type for {}", entry.path().display()))?;

            // Skip symlinks
            if file_type.is_symlink() {
                continue;
            }

            let path = entry.path();

            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file()
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
            {
                results.push(path);
            }
        }
    }

    results.sort();
    Ok(results)
}

/// Run the full ingest pipeline for a single file path.
fn run_ingest_for_path(
    wiki_root: &Path,
    file: &Path,
    prompt_path: &Path,
    agent: &str,
    model: Option<&str>,
    stream: bool,
) -> Result<()> {
    let ingest_path = resolve_ingest_source(file)?;
    let prompt = expand_ingest_prompt(wiki_root, &ingest_path, prompt_path)?;
    run_aikit(wiki_root, &prompt, agent, model, stream)
}

/// Summary returned by [`run_folder_ingest`].
#[derive(Debug)]
pub struct FolderIngestResult {
    /// Total files discovered.
    pub total: usize,
    /// Files that completed without error.
    pub succeeded: usize,
    /// Files that failed, paired with their error message.
    pub failures: Vec<(PathBuf, String)>,
}

/// Ingest all `*.md` files discovered under `folder` (recursive, no symlinks).
///
/// Returns an error immediately (before ingesting any file) if the discovered
/// file count exceeds `max_files` and `max_files > 0`.
/// Pass `max_files = 0` for no cap (unlimited).
pub fn run_folder_ingest(
    wiki_root: &Path,
    folder: &Path,
    prompt_path: &Path,
    agent: &str,
    model: Option<&str>,
    stream: bool,
    max_files: usize,
) -> Result<FolderIngestResult> {
    let files = discover_md_files(folder)?;
    let total = files.len();

    if max_files > 0 && total > max_files {
        bail!(
            "found {} markdown file(s) under {}; exceeds --max-files cap of {}. \
             Pass --max-files {} (or higher) to proceed.",
            total,
            folder.display(),
            max_files,
            total
        );
    }

    let mut failures: Vec<(PathBuf, String)> = Vec::new();

    for file in &files {
        if let Err(e) = run_ingest_for_path(wiki_root, file, prompt_path, agent, model, stream) {
            failures.push((file.clone(), e.to_string()));
        }
    }

    let succeeded = total - failures.len();
    Ok(FolderIngestResult {
        total,
        succeeded,
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // --- discover_md_files tests ---

    #[test]
    fn discover_md_files_empty_dir() {
        let tmp = tempdir().unwrap();
        let files = discover_md_files(tmp.path()).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn discover_md_files_finds_md_only() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("doc.md"), "# doc").unwrap();
        fs::write(tmp.path().join("file.txt"), "text").unwrap();
        fs::write(tmp.path().join("data.json"), "{}").unwrap();
        fs::write(tmp.path().join("noext"), "noext").unwrap();
        let files = discover_md_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("doc.md"));
    }

    #[test]
    fn discover_md_files_case_insensitive() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("upper.MD"), "# Up").unwrap();
        let files = discover_md_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn discover_md_files_nested_dirs() {
        let tmp = tempdir().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(tmp.path().join("root.md"), "root").unwrap();
        fs::write(sub.join("nested.md"), "nested").unwrap();
        let files = discover_md_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn discover_md_files_sorted_lexicographic() {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("z.md"), "z").unwrap();
        fs::write(tmp.path().join("a.md"), "a").unwrap();
        fs::write(tmp.path().join("m.md"), "m").unwrap();
        let files = discover_md_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 3);
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, vec!["a.md", "m.md", "z.md"]);
    }

    #[test]
    fn discover_md_files_rejects_nonexistent() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("missing");
        assert!(discover_md_files(&missing).is_err());
    }

    #[test]
    fn run_folder_ingest_cap_exceeded_returns_error() {
        let tmp = tempdir().unwrap();
        let batch = tmp.path().join("batch");
        fs::create_dir(&batch).unwrap();
        for i in 0..5u32 {
            fs::write(batch.join(format!("f{i}.md")), "# note").unwrap();
        }
        // cap of 3 with 5 files → error
        let prompt_path = tmp.path().join("ingest.md");
        fs::write(&prompt_path, "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n").unwrap();
        let err = run_folder_ingest(tmp.path(), &batch, &prompt_path, "codex", None, false, 3)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("5"), "expected file count in error: {msg}");
        assert!(msg.contains("--max-files"), "expected hint in error: {msg}");
    }

    #[test]
    fn run_folder_ingest_zero_cap_means_unlimited() {
        let tmp = tempdir().unwrap();
        let batch = tmp.path().join("batch");
        fs::create_dir(&batch).unwrap();
        // 5 files, cap = 0 → no cap applied; will fail at agent step (not runnable) not at cap
        for i in 0..5u32 {
            fs::write(batch.join(format!("f{i}.md")), "# note").unwrap();
        }
        let prompt_path = tmp.path().join("ingest.md");
        fs::write(&prompt_path, "Ingest {{INGEST_PATH}} into {{WIKI_ROOT}}\n").unwrap();
        let result = run_folder_ingest(
            tmp.path(),
            &batch,
            &prompt_path,
            "nonexistent-agent-xyz",
            None,
            false,
            0,
        )
        .unwrap();
        // all 5 files should have failed at the agent step, not at cap
        assert_eq!(result.total, 5);
        assert_eq!(result.failures.len(), 5);
    }

    // --- resolve_ingest_source tests ---

    #[test]
    fn resolve_ingest_source_accepts_md() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("note.md");
        fs::write(&f, "x").unwrap();
        let out = resolve_ingest_source(&f).unwrap();
        assert!(out.is_absolute());
        assert!(out.ends_with("note.md"));
    }

    #[test]
    fn resolve_ingest_source_accepts_txt() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("x.txt");
        fs::write(&f, "hello text").unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn resolve_ingest_source_accepts_json() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("data.json");
        fs::write(&f, r#"{"key": "value"}"#).unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn resolve_ingest_source_accepts_yaml() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("config.yaml");
        fs::write(&f, "key: value\n").unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn resolve_ingest_source_accepts_log() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("app.log");
        fs::write(&f, "INFO: started\n").unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn resolve_ingest_source_rejects_binary() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("binary.bin");
        fs::write(&f, b"binary\x00content").unwrap();
        let err = resolve_ingest_source(&f).unwrap_err();
        assert!(
            err.to_string().contains("file appears to be binary"),
            "error: {err}"
        );
    }

    #[test]
    fn resolve_ingest_source_rejects_invalid_utf8() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("bad.txt");
        fs::write(&f, b"\xff\xfe invalid utf8 bytes").unwrap();
        let err = resolve_ingest_source(&f).unwrap_err();
        assert!(err.to_string().contains("valid UTF-8"), "error: {err}");
    }

    #[test]
    fn resolve_ingest_source_accepts_empty_file() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("empty.txt");
        fs::write(&f, b"").unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn resolve_ingest_source_handles_missing_file() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("missing.md");
        assert!(resolve_ingest_source(&missing).is_err());
    }

    #[test]
    fn resolve_ingest_source_accepts_uppercase_md() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("note.MD");
        fs::write(&f, "content").unwrap();
        let result = resolve_ingest_source(&f);
        assert!(result.is_ok());
    }

    #[test]
    fn resolve_ingest_source_accepts_no_extension() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("note");
        fs::write(&f, "content").unwrap();
        assert!(resolve_ingest_source(&f).is_ok());
    }

    #[test]
    fn agent_not_runnable_returns_error() {
        let tmp = tempdir().unwrap();
        let err =
            run_aikit(tmp.path(), "prompt", "nonexistent-agent-xyz", None, false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nonexistent-agent-xyz"), "error: {msg}");
        assert!(msg.contains("available agents"), "error: {msg}");
    }

    #[cfg(unix)]
    mod unix_tests {
        use super::*;
        use std::sync::Mutex;

        static PATH_MUTEX: Mutex<()> = Mutex::new(());

        #[test]
        fn run_aikit_with_stub_agent_succeeds() {
            let _guard = PATH_MUTEX.lock().unwrap();

            let stub_dir = tempdir().unwrap();
            // Write a stub script that exits 0 and prints nothing to stderr
            let stub_path = stub_dir.path().join("codex");
            fs::write(
                &stub_path,
                "#!/bin/sh\nwhile IFS= read -r line; do :; done\nexit 0\n",
            )
            .unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&stub_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&stub_path, perms).unwrap();

            let original_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var(
                "PATH",
                format!("{}:{}", stub_dir.path().display(), original_path),
            );

            let wiki_tmp = tempdir().unwrap();
            let result = run_aikit(wiki_tmp.path(), "hello", "codex", None, false);

            std::env::set_var("PATH", original_path);

            // The stub exits 0, so this should succeed (or fail with a spawn/io error, not a "not runnable" error)
            match result {
                Ok(()) => {}
                Err(e) => {
                    let msg = e.to_string();
                    // Must NOT be a "not runnable" failure
                    assert!(
                        !msg.contains("not runnable"),
                        "unexpected not-runnable error: {msg}"
                    );
                }
            }
        }
    }
}
