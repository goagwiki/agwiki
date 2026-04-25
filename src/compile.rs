//! Compile: validate content/ sources, emit wiki/, regenerate catalog.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use ulid::Ulid;

use crate::init::AgwikiConfig;

pub const COMPILE_INDEX_START: &str = "<!-- agwiki:compile-index -->";
pub const COMPILE_INDEX_END: &str = "<!-- /agwiki:compile-index -->";

/// Front matter schema for entity source files under content/<kind>/.
#[derive(Debug, Deserialize)]
pub struct EntityFrontMatter {
    /// Stable globally unique entity id.
    pub id: Option<String>,
    /// Human-readable entity title.
    pub title: Option<String>,
    /// Source schema version. Only version 1 is currently supported.
    pub schema_version: Option<u32>,
    /// Optional lifecycle status.
    #[serde(default)]
    pub status: Option<EntityStatus>,
    /// Optional graph relations to other entity ids.
    #[serde(default)]
    pub relations: Vec<Relation>,
    /// Optional alternate names for the entity.
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Lifecycle status for an entity source file.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EntityStatus {
    /// Entity is current and should be included in generated output.
    Active,
    /// Entity is retained for history but no longer current.
    Archived,
}

/// Relation from one entity source file to another entity id.
#[derive(Debug, Deserialize)]
pub struct Relation {
    /// Target entity id.
    pub target: String,
    /// Relation type label.
    pub rel: String,
}

/// In-memory registry mapping id → (content_file_path, kind_name).
pub type IdRegistry = HashMap<String, (PathBuf, String)>;

pub struct CompileOptions {
    /// Root directory containing `agwiki.toml` and `content/`.
    pub wiki_root: PathBuf,
    /// Validate and report planned writes without modifying files.
    pub dry_run: bool,
}

/// Result summary from a compile or source validation run.
pub struct CompileReport {
    /// Number of entity source files successfully compiled.
    pub entities_compiled: usize,
    /// Number of pass-through content pages copied to `wiki/`.
    pub pages_copied: usize,
    /// Validation or configuration errors collected during the run.
    pub errors: Vec<CompileError>,
    /// Non-fatal warnings collected during the run.
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum CompileError {
    /// agwiki.toml version is not 2 (E001).
    LegacyConfig { version: u32 },
    /// agwiki.toml missing or unreadable (E002).
    ConfigMissing { path: PathBuf },
    /// agwiki.toml TOML parse error (E003).
    ConfigParse { message: String },
    /// YAML front matter parse error (E004).
    YamlParse { path: PathBuf, message: String },
    /// Required field missing (E005/E006/E007).
    MissingField { path: PathBuf, field: &'static str },
    /// schema_version value is not 1 (E008).
    UnsupportedSchemaVersion { path: PathBuf, version: u32 },
    /// File's parent dir is not in ontology.kinds (E009).
    UnknownKind {
        path: PathBuf,
        kind: String,
        allowed: Vec<String>,
    },
    /// agwiki new: kind argument not in ontology.kinds (E010).
    InvalidKindArg { kind: String, allowed: Vec<String> },
    /// Duplicate id found in two files (E011).
    DuplicateId {
        id: String,
        first: PathBuf,
        second: PathBuf,
    },
    /// relations[].target references unknown id (E012).
    BrokenRelation {
        source_path: PathBuf,
        source_id: String,
        target_id: String,
    },
    /// relations[].rel not in ontology.relation_types (E013).
    DisallowedRelationType {
        source_path: PathBuf,
        rel: String,
        allowed: Vec<String>,
    },
    /// Two entities in the same kind produce the same output slug (E014, warning only).
    SlugCollision {
        kind: String,
        slug: String,
        paths: Vec<PathBuf>,
    },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::LegacyConfig { version } => write!(
                f,
                "E001: agwiki.toml has version={version}; expected version=2 (legacy v1 config)"
            ),
            CompileError::ConfigMissing { path } => {
                write!(f, "E002: agwiki.toml missing: {}", path.display())
            }
            CompileError::ConfigParse { message } => {
                write!(f, "E003: agwiki.toml parse error: {message}")
            }
            CompileError::YamlParse { path, message } => {
                write!(f, "E004: YAML parse error in {}: {message}", path.display())
            }
            CompileError::MissingField { path, field } => {
                write!(
                    f,
                    "E00{}: missing required field `{field}` in {}",
                    match *field {
                        "id" => "5",
                        "title" => "6",
                        "schema_version" => "7",
                        _ => "?",
                    },
                    path.display()
                )
            }
            CompileError::UnsupportedSchemaVersion { path, version } => write!(
                f,
                "E008: schema_version={version} (expected 1) in {}",
                path.display()
            ),
            CompileError::UnknownKind {
                path,
                kind,
                allowed,
            } => write!(
                f,
                "E009: unknown kind `{kind}` in {}; allowed: [{}]",
                path.display(),
                allowed.join(", ")
            ),
            CompileError::InvalidKindArg { kind, allowed } => write!(
                f,
                "E010: unknown kind `{kind}`; valid kinds: [{}]",
                allowed.join(", ")
            ),
            CompileError::DuplicateId { id, first, second } => write!(
                f,
                "E011: duplicate id `{id}` in {} and {}",
                first.display(),
                second.display()
            ),
            CompileError::BrokenRelation {
                source_path,
                source_id,
                target_id,
            } => write!(
                f,
                "E012: broken relation in {} (id={source_id}): target `{target_id}` not found",
                source_path.display()
            ),
            CompileError::DisallowedRelationType {
                source_path,
                rel,
                allowed,
            } => write!(
                f,
                "E013: disallowed relation type `{rel}` in {}; allowed: [{}]",
                source_path.display(),
                allowed.join(", ")
            ),
            CompileError::SlugCollision { kind, slug, paths } => write!(
                f,
                "E014 (warning): slug collision `{slug}` in kind `{kind}` for {} files",
                paths.len()
            ),
        }
    }
}

/// Load and validate agwiki.toml from wiki_root. Returns Err on E001/E002/E003.
pub fn load_config(wiki_root: &Path) -> Result<AgwikiConfig> {
    let config_path = wiki_root.join("agwiki.toml");
    if !config_path.is_file() {
        bail!(
            "{}",
            CompileError::ConfigMissing {
                path: config_path.clone()
            }
        );
    }
    let toml_str = fs::read_to_string(&config_path)
        .with_context(|| format!("read {}", config_path.display()))?;
    let config: AgwikiConfig = toml::from_str(&toml_str).map_err(|e| {
        anyhow::anyhow!(
            "{}",
            CompileError::ConfigParse {
                message: e.to_string()
            }
        )
    })?;
    if config.version != 2 {
        bail!(
            "{}",
            CompileError::LegacyConfig {
                version: config.version
            }
        );
    }
    Ok(config)
}

/// Compute URL-safe slug from title.
pub fn title_to_slug(title: &str) -> String {
    let lower = title.to_lowercase();
    let with_hyphens = lower.replace(' ', "-");
    let cleaned: String = with_hyphens
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    cleaned.chars().take(80).collect()
}

fn walk_md(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk_md(&p));
            } else if p.extension().and_then(|s| s.to_str()) == Some("md") {
                out.push(p);
            }
        }
    }
    out
}

fn parse_front_matter(content: &str) -> Result<(EntityFrontMatter, &str), String> {
    if !content.starts_with("---") {
        return Err("file does not start with YAML front matter delimiter `---`".into());
    }
    let rest = &content[3..];
    let end = rest
        .find("\n---")
        .ok_or("no closing `---` for front matter")?;
    let yaml_str = &rest[..end];
    let body = &rest[end + 4..];
    let fm: EntityFrontMatter = serde_yaml::from_str(yaml_str).map_err(|e| e.to_string())?;
    Ok((fm, body))
}

struct EntityInfo {
    path: PathBuf,
    kind: String,
    id: String,
    title: String,
    status: Option<EntityStatus>,
    aliases: Vec<String>,
    relations: Vec<Relation>,
    body: String,
}

fn inject_compile_index(existing: &str, generated_body: &str) -> String {
    let gen_trim = generated_body.trim_end();
    let replacement = format!(
        "{}\n{}\n{}\n",
        COMPILE_INDEX_START, gen_trim, COMPILE_INDEX_END
    );

    if existing.contains(COMPILE_INDEX_START) {
        let si = existing.find(COMPILE_INDEX_START).unwrap();
        let after_start = si + COMPILE_INDEX_START.len();
        let tail = &existing[after_start..];
        if let Some(rel_ei) = tail.find(COMPILE_INDEX_END) {
            let abs_ei = after_start + rel_ei;
            let abs_after_end = abs_ei + COMPILE_INDEX_END.len();
            let before = &existing[..si];
            let after = &existing[abs_after_end..];
            return format!("{}{}{}", before, replacement, after);
        }
    }

    let sep = if existing.is_empty() || existing.ends_with("\n\n") {
        ""
    } else if existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{existing}{sep}{replacement}")
}

fn render_catalog(entities: &[EntityInfo], kinds: &[String], slugs: &[String]) -> String {
    let mut lines = vec!["## Entity catalog".to_string(), String::new()];
    for kind in kinds {
        let mut kind_entities: Vec<(usize, &EntityInfo)> = entities
            .iter()
            .enumerate()
            .filter(|(_, e)| &e.kind == kind)
            .collect();
        if kind_entities.is_empty() {
            continue;
        }
        kind_entities.sort_by_key(|(_, e)| e.title.to_lowercase());
        lines.push(format!("### {}", kind));
        lines.push(String::new());
        for (idx, e) in kind_entities {
            let slug = &slugs[idx];
            lines.push(format!("- [{}](wiki/{}/{}.md)", e.title, kind, slug));
        }
        lines.push(String::new());
    }
    let s = lines.join("\n");
    format!("{}\n", s.trim_end())
}

/// Main entry point for compile.
pub fn run_compile(opts: CompileOptions) -> Result<CompileReport> {
    let wiki_root = &opts.wiki_root;

    // Step 1: load config
    let config = load_config(wiki_root)?;

    let content_root = wiki_root.join(&config.content_root);
    let wiki_dir = wiki_root.join(&config.generated_wiki);

    let mut errors: Vec<CompileError> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Step 2-5: scan content/**/*.md, skip content/pages/
    let mut raw_entities: Vec<EntityInfo> = Vec::new();
    let pages_dir = content_root.join("pages");

    if content_root.is_dir() {
        let mut all_files = walk_md(&content_root);
        all_files.sort();

        for file in &all_files {
            // Skip pages directory
            if file.starts_with(&pages_dir) {
                continue;
            }

            let kind = match file
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
            {
                Some(k) => k.to_string(),
                None => continue,
            };

            let content = match fs::read_to_string(file) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(CompileError::YamlParse {
                        path: file.clone(),
                        message: e.to_string(),
                    });
                    continue;
                }
            };

            // Step 3: parse YAML front matter
            let (fm, body) = match parse_front_matter(&content) {
                Ok(r) => r,
                Err(msg) => {
                    errors.push(CompileError::YamlParse {
                        path: file.clone(),
                        message: msg,
                    });
                    continue;
                }
            };

            // Step 4: validate required fields
            let id = match &fm.id {
                Some(v) => v.clone(),
                None => {
                    errors.push(CompileError::MissingField {
                        path: file.clone(),
                        field: "id",
                    });
                    continue;
                }
            };
            let title = match &fm.title {
                Some(v) => v.clone(),
                None => {
                    errors.push(CompileError::MissingField {
                        path: file.clone(),
                        field: "title",
                    });
                    continue;
                }
            };
            let schema_version = match fm.schema_version {
                Some(v) => v,
                None => {
                    errors.push(CompileError::MissingField {
                        path: file.clone(),
                        field: "schema_version",
                    });
                    continue;
                }
            };
            if schema_version != 1 {
                errors.push(CompileError::UnsupportedSchemaVersion {
                    path: file.clone(),
                    version: schema_version,
                });
                continue;
            }

            // Step 5: validate kind
            if !config.ontology.kinds.contains(&kind) {
                errors.push(CompileError::UnknownKind {
                    path: file.clone(),
                    kind: kind.clone(),
                    allowed: config.ontology.kinds.clone(),
                });
                continue;
            }

            raw_entities.push(EntityInfo {
                path: file.clone(),
                kind,
                id,
                title,
                status: fm.status,
                aliases: fm.aliases,
                relations: fm.relations,
                body: body.trim_start_matches('\n').to_string(),
            });
        }
    }

    // Step 6: build IdRegistry, collect duplicate id errors
    let mut registry: IdRegistry = HashMap::new();
    for entity in &raw_entities {
        if let Some((existing_path, _)) = registry.get(&entity.id) {
            errors.push(CompileError::DuplicateId {
                id: entity.id.clone(),
                first: existing_path.clone(),
                second: entity.path.clone(),
            });
        } else {
            registry.insert(
                entity.id.clone(),
                (entity.path.clone(), entity.kind.clone()),
            );
        }
    }

    // Step 7: validate relations[].target
    for entity in &raw_entities {
        for rel in &entity.relations {
            if !registry.contains_key(&rel.target) {
                errors.push(CompileError::BrokenRelation {
                    source_path: entity.path.clone(),
                    source_id: entity.id.clone(),
                    target_id: rel.target.clone(),
                });
            }
            // Step 8: validate relations[].rel
            if !config.ontology.relation_types.is_empty()
                && !config.ontology.relation_types.contains(&rel.rel)
            {
                errors.push(CompileError::DisallowedRelationType {
                    source_path: entity.path.clone(),
                    rel: rel.rel.clone(),
                    allowed: config.ontology.relation_types.clone(),
                });
            }
        }
    }

    // Step 9: if errors, print and return
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("agwiki compile: {}", e);
        }
        return Ok(CompileReport {
            entities_compiled: 0,
            pages_copied: 0,
            errors,
            warnings,
        });
    }

    // Step 10: compute slugs, handle collisions
    // Group by kind, sort by path for deterministic ordering
    let mut kind_slug_counts: HashMap<String, HashMap<String, usize>> = HashMap::new();
    for entity in &raw_entities {
        let base_slug = title_to_slug(&entity.title);
        let counts = kind_slug_counts.entry(entity.kind.clone()).or_default();
        let count = counts.entry(base_slug.clone()).or_insert(0);
        *count += 1;
    }

    // Detect collisions (base slug used more than once)
    let mut collision_warned: HashSet<(String, String)> = HashSet::new();
    for (i, entity) in raw_entities.iter().enumerate() {
        let base_slug = title_to_slug(&entity.title);
        let count = kind_slug_counts[&entity.kind][&base_slug];
        if count > 1 {
            let key = (entity.kind.clone(), base_slug.clone());
            if collision_warned.insert(key) {
                let colliding_paths: Vec<PathBuf> = raw_entities
                    .iter()
                    .filter(|e| e.kind == entity.kind && title_to_slug(&e.title) == base_slug)
                    .map(|e| e.path.clone())
                    .collect();
                let warn = CompileError::SlugCollision {
                    kind: entity.kind.clone(),
                    slug: base_slug.clone(),
                    paths: colliding_paths,
                };
                warnings.push(warn.to_string());
                eprintln!("agwiki compile: {}", warn);
            }
        }
        let _ = i; // suppress unused warning
    }

    // Rebuild entity_slugs correctly (first occurrence gets plain slug, subsequent get -2, -3...)
    let mut slug_final: Vec<String> = vec![String::new(); raw_entities.len()];
    let mut kind_slug_seen: HashMap<String, HashMap<String, usize>> = HashMap::new();
    // Sort entities by path for deterministic first-wins
    let mut idx_sorted: Vec<usize> = (0..raw_entities.len()).collect();
    idx_sorted.sort_by_key(|&i| raw_entities[i].path.to_string_lossy().to_string());

    for i in idx_sorted {
        let entity = &raw_entities[i];
        let base_slug = title_to_slug(&entity.title);
        let seen = kind_slug_seen.entry(entity.kind.clone()).or_default();
        let count = seen.entry(base_slug.clone()).or_insert(0);
        *count += 1;
        slug_final[i] = if *count == 1 {
            base_slug
        } else {
            format!("{}-{}", base_slug, count)
        };
    }

    // Step 11: emit wiki/<kind>/<slug>.md (or print if dry-run)
    let entities_compiled = raw_entities.len();
    if !opts.dry_run {
        for kind in &config.ontology.kinds {
            let kind_dir = wiki_dir.join(kind);
            fs::create_dir_all(&kind_dir).with_context(|| format!("create wiki/{}", kind))?;
        }
    }

    for (i, entity) in raw_entities.iter().enumerate() {
        let slug = &slug_final[i];
        let out_path = wiki_dir.join(&entity.kind).join(format!("{}.md", slug));

        if opts.dry_run {
            println!("would write: {}", out_path.display());
            continue;
        }

        // Build reduced front matter
        let mut fm_parts = vec![format!("title: {:?}", entity.title)];
        if let Some(status) = &entity.status {
            let s = match status {
                EntityStatus::Active => "active",
                EntityStatus::Archived => "archived",
            };
            fm_parts.push(format!("status: {}", s));
        }
        if !entity.aliases.is_empty() {
            let aliases_str = entity
                .aliases
                .iter()
                .map(|a| format!("  - {:?}", a))
                .collect::<Vec<_>>()
                .join("\n");
            fm_parts.push(format!("aliases:\n{}", aliases_str));
        }

        let output = format!("---\n{}\n---\n\n{}", fm_parts.join("\n"), entity.body);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out_path, output).with_context(|| format!("write {}", out_path.display()))?;
    }

    // Step 12: pass-through content/pages/ files
    let page_names = ["index", "inbox", "log"];
    let mut pages_copied = 0;
    for page in &page_names {
        let src = pages_dir.join(format!("{}.md", page));
        let dst = wiki_dir.join(format!("{}.md", page));
        if src.is_file() {
            if opts.dry_run {
                println!("would copy: {} -> {}", src.display(), dst.display());
            } else {
                fs::copy(&src, &dst).with_context(|| format!("copy pages/{}.md", page))?;
                pages_copied += 1;
            }
        }
    }

    // Step 13: regenerate catalog in wiki/index.md
    let catalog = render_catalog(&raw_entities, &config.ontology.kinds, &slug_final);
    if opts.dry_run {
        println!("--- catalog (inside markers) ---");
        print!("{}", catalog);
    } else {
        let index_path = wiki_dir.join("index.md");
        let existing = fs::read_to_string(&index_path).unwrap_or_default();
        let updated = inject_compile_index(&existing, &catalog);
        fs::write(&index_path, updated).context("write wiki/index.md")?;
    }

    Ok(CompileReport {
        entities_compiled,
        pages_copied,
        errors,
        warnings,
    })
}

/// Create a new entity file at content/<kind>/<ulid>.md.
pub fn run_new(wiki_root: &Path, kind: &str, title: Option<&str>) -> Result<PathBuf> {
    let config = load_config(wiki_root)?;
    if !config.ontology.kinds.contains(&kind.to_string()) {
        bail!(
            "{}",
            CompileError::InvalidKindArg {
                kind: kind.to_string(),
                allowed: config.ontology.kinds.clone(),
            }
        );
    }

    let id = Ulid::new().to_string();
    let title_str = title.unwrap_or("Untitled");
    let content_root = wiki_root.join(&config.content_root);
    let kind_dir = content_root.join(kind);
    fs::create_dir_all(&kind_dir).with_context(|| format!("create content/{}", kind))?;

    let file_path = kind_dir.join(format!("{}.md", id));
    let content = format!(
        "---\nid: \"{}\"\ntitle: \"{}\"\nschema_version: 1\n---\n\n# {}\n\n",
        id, title_str, title_str
    );
    fs::write(&file_path, content).with_context(|| format!("write {}", file_path.display()))?;
    Ok(file_path)
}

/// Export wiki/ as static HTML to out_dir.
pub fn run_export_html(wiki_root: &Path, out_dir: &Path) -> Result<()> {
    let wiki_dir = wiki_root.join("wiki");
    if !wiki_dir.is_dir() {
        bail!(
            "E015: wiki/ directory does not exist at {}; run `agwiki compile` first",
            wiki_root.display()
        );
    }

    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let mut md_files = walk_md(&wiki_dir);
    md_files.sort();

    let mut html_links: Vec<String> = Vec::new();

    for md_path in &md_files {
        let rel = md_path
            .strip_prefix(&wiki_dir)
            .context("strip wiki prefix")?;
        let html_rel = rel.with_extension("html");
        let out_path = out_dir.join(&html_rel);

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let markdown =
            fs::read_to_string(md_path).with_context(|| format!("read {}", md_path.display()))?;
        let html_body = crate::markdown_html::markdown_to_html(&markdown);
        let title = rel
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("page")
            .to_string();
        let full_html = format!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{title}</title></head><body>{html_body}</body></html>"
        );
        fs::write(&out_path, full_html).with_context(|| format!("write {}", out_path.display()))?;

        let html_rel_str = html_rel.to_string_lossy().replace('\\', "/");
        html_links.push(format!(
            "<li><a href=\"{}\">{}</a></li>",
            html_rel_str, title
        ));
    }

    // Generate index.html
    let index_html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Wiki</title></head><body><h1>Wiki</h1><ul>{}</ul></body></html>",
        html_links.join("\n")
    );
    fs::write(out_dir.join("index.html"), index_html).context("write index.html")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::run_init;
    use tempfile::tempdir;

    #[test]
    fn title_to_slug_basic() {
        assert_eq!(title_to_slug("Hello World"), "hello-world");
        assert_eq!(title_to_slug("My Concept!"), "my-concept");
        assert_eq!(title_to_slug("  spaces  "), "--spaces--");
    }

    #[test]
    fn title_to_slug_truncates() {
        let long = "a".repeat(100);
        assert_eq!(title_to_slug(&long).len(), 80);
    }

    #[test]
    fn new_valid_kind_creates_file() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("wiki");
        run_init(&root).unwrap();
        let path = run_new(&root, "concepts", Some("Test Concept")).unwrap();
        assert!(path.is_file());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("title: \"Test Concept\""));
        assert!(content.contains("schema_version: 1"));
    }

    #[test]
    fn new_invalid_kind_returns_error() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("wiki");
        run_init(&root).unwrap();
        let result = run_new(&root, "unknown-kind", None);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("E010"));
    }
}
