//! Mirror wiki sections into `skill/references/` and update `SKILL.md` inside agwiki marker comments.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::validate::validate_wiki;

/// Opening HTML comment: generated wiki index is inserted or replaced between this and [`GENERATED_INDEX_END`].
pub const GENERATED_INDEX_START: &str = "<!-- agwiki:generated-index -->";
/// Closing HTML comment for the generated block.
pub const GENERATED_INDEX_END: &str = "<!-- /agwiki:generated-index -->";

fn wikilink_full_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\[\[([^\]|]+)(?:\|([^\]]+))?\]\]").unwrap())
}

fn heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(#{2,3})\s+(.+?)\s*$").unwrap())
}

/// Inputs for [`run_export`].
pub struct ExportOptions<'a> {
    /// Content wiki root (contains `wiki/`).
    pub wiki_root: &'a Path,
    /// Agent Skill bundle root; default `<wiki-root>/skill`.
    pub skill_root: Option<&'a Path>,
    /// Path to `SKILL.md` to create or update; default `<skill-root>/SKILL.md`.
    pub skill_md: Option<&'a Path>,
    pub dry_run: bool,
    pub prune: bool,
}

/// Top-level directory names under `wiki/` (sorted), each mirrored to `skill/references/<name>/`.
pub fn wiki_mirror_sections(wiki_root: &Path) -> Result<Vec<String>> {
    let wiki = wiki_root.join("wiki");
    if !wiki.is_dir() {
        bail!("missing wiki directory: {}", wiki.display());
    }
    let mut names: Vec<String> = fs::read_dir(&wiki)
        .with_context(|| wiki.display().to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

fn copy_section(
    wiki_root: &Path,
    skill_dir: &Path,
    section: &str,
    dry_run: bool,
) -> Result<Vec<PathBuf>> {
    let src_root = wiki_root.join("wiki").join(section);
    let dst_root = skill_dir.join("references").join(section);
    let mut copied = Vec::new();
    if !src_root.is_dir() {
        return Ok(copied);
    }
    let mut paths: Vec<_> = walk_md(&src_root);
    paths.sort();
    for src in paths {
        let rel = src
            .strip_prefix(&src_root)
            .context("strip_prefix src_root")?;
        let dst = dst_root.join(rel);
        copied.push(dst.clone());
        if dry_run {
            println!("copy {} -> {}", src.display(), dst.display());
            continue;
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&src, &dst)
            .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    }
    Ok(copied)
}

fn walk_md(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
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

fn prune_section(wiki_root: &Path, skill_dir: &Path, section: &str, dry_run: bool) -> Result<()> {
    let ref_sec = skill_dir.join("references").join(section);
    let wiki_sec = wiki_root.join("wiki").join(section);
    if !ref_sec.is_dir() {
        return Ok(());
    }
    let mut paths: Vec<_> = walk_md(&ref_sec);
    paths.sort();
    for dst in paths {
        let rel = dst.strip_prefix(&ref_sec).context("strip ref_sec")?;
        let src = wiki_sec.join(rel);
        if !src.is_file() {
            if dry_run {
                println!("prune {}", dst.display());
            } else {
                fs::remove_file(&dst)?;
            }
        }
    }
    Ok(())
}

fn titleish_stem(s: &str) -> String {
    Path::new(s)
        .file_stem()
        .map(|x| x.to_string_lossy())
        .unwrap_or_default()
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

type IndexGroups = Vec<(String, Vec<(String, String)>)>;

fn parse_index_for_groups(
    wiki_root: &Path,
    index_text: &str,
    sections: &[String],
) -> (IndexGroups, HashSet<String>) {
    let section_set: HashSet<_> = sections.iter().cloned().collect();
    let re = wikilink_full_re();
    let hre = heading_re();
    let mut current_heading = "Index".to_string();
    let mut heading_order: Vec<String> = Vec::new();
    let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut included_refs: HashSet<String> = HashSet::new();

    let mut add_item = |heading: &str, label: String, ref_rel: String| {
        if !map.contains_key(heading) {
            heading_order.push(heading.to_string());
        }
        map.entry(heading.to_string())
            .or_default()
            .push((label, ref_rel.clone()));
        included_refs.insert(ref_rel);
    };

    for line in index_text.lines() {
        if let Some(cap) = hre.captures(line) {
            current_heading = cap.get(2).unwrap().as_str().trim().to_string();
            continue;
        }
        for cap in re.captures_iter(line) {
            let target = cap.get(1).map(|x| x.as_str()).unwrap_or("").trim();
            let alias = cap.get(2).map(|m| m.as_str().trim());
            let key = target.strip_suffix(".md").unwrap_or(target);
            if key.is_empty() {
                continue;
            }
            let (section, rel_under) = if let Some((a, b)) = key.split_once('/') {
                (Some(a.to_string()), b.to_string())
            } else {
                let stem = key.to_string();
                let mut found: Option<(String, String)> = None;
                for sec in sections {
                    let flat = wiki_root
                        .join("wiki")
                        .join(sec)
                        .join(format!("{}.md", stem));
                    if flat.is_file() {
                        found = Some((sec.clone(), stem.clone()));
                        break;
                    }
                }
                if found.is_none() {
                    for sec in sections {
                        let base = wiki_root.join("wiki").join(sec);
                        let matches: Vec<_> = walk_md(&base)
                            .into_iter()
                            .filter(|p| {
                                p.file_stem()
                                    .map(|s| s.to_string_lossy() == stem)
                                    .unwrap_or(false)
                            })
                            .collect();
                        if matches.len() == 1 {
                            let rel = matches[0]
                                .strip_prefix(&base)
                                .unwrap()
                                .to_string_lossy()
                                .replace('\\', "/");
                            let rel = rel.strip_suffix(".md").unwrap_or(&rel).to_string();
                            found = Some((sec.clone(), rel));
                            break;
                        }
                    }
                }
                match found {
                    Some(x) => (Some(x.0), x.1),
                    None => continue,
                }
            };
            let Some(section) = section else { continue };
            if !section_set.contains(&section) {
                continue;
            }
            let ref_rel = format!("references/{}/{}.md", section, rel_under);
            let label = alias
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| titleish_stem(&rel_under));
            add_item(&current_heading, label, ref_rel);
        }
    }

    let mut ordered = Vec::new();
    for h in heading_order {
        if let Some(items) = map.get(&h) {
            if !items.is_empty() {
                ordered.push((h, items.clone()));
            }
        }
    }
    (ordered, included_refs)
}

fn list_mirrored_files(skill_dir: &Path, sections: &[String]) -> HashSet<String> {
    let mut out = HashSet::new();
    let ref_root = skill_dir.join("references");
    for section in sections {
        let d = ref_root.join(section);
        if !d.is_dir() {
            continue;
        }
        for p in walk_md(&d) {
            if let Ok(rel) = p.strip_prefix(skill_dir) {
                out.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out
}

fn render_generated_block(groups: &IndexGroups, orphan_refs: &[String]) -> String {
    let mut lines = vec!["## Wiki index".to_string(), String::new()];
    for (heading, items) in groups {
        lines.push(format!("### {}", heading));
        lines.push(String::new());
        for (label, ref_rel) in items {
            lines.push(format!("- [{}]({})", label, ref_rel));
        }
        lines.push(String::new());
    }
    if !orphan_refs.is_empty() {
        lines.push("### On disk but not linked from index".to_string());
        lines.push(String::new());
        for ref_rel in orphan_refs {
            let stem = Path::new(ref_rel)
                .file_stem()
                .map(|s| s.to_string_lossy())
                .unwrap_or_default();
            let title = titleish_stem(&stem);
            lines.push(format!("- [{}]({})", title, ref_rel));
        }
        lines.push(String::new());
    }
    let s = lines.join("\n");
    format!("{}\n", s.trim_end())
}

/// Insert or replace the block between [`GENERATED_INDEX_START`] and [`GENERATED_INDEX_END`].
pub fn merge_skill_generated_index(existing: &str, generated_body: &str) -> Result<String> {
    let gen_trim = generated_body.trim_end();
    let replacement = format!(
        "{}\n{}\n{}\n",
        GENERATED_INDEX_START, gen_trim, GENERATED_INDEX_END
    );

    if existing.contains(GENERATED_INDEX_START) {
        let si = existing
            .find(GENERATED_INDEX_START)
            .expect("contains start");
        let after_start = si + GENERATED_INDEX_START.len();
        let tail = &existing[after_start..];
        let Some(rel_ei) = tail.find(GENERATED_INDEX_END) else {
            bail!(
                "SKILL.md: found {} but no closing {}",
                GENERATED_INDEX_START,
                GENERATED_INDEX_END
            );
        };
        let abs_ei = after_start + rel_ei;
        let abs_after_end = abs_ei + GENERATED_INDEX_END.len();
        let before = &existing[..si];
        let after = &existing[abs_after_end..];
        return Ok(format!("{}{}{}", before, replacement, after));
    }

    if existing.contains(GENERATED_INDEX_END) {
        bail!(
            "SKILL.md: found {} without {}",
            GENERATED_INDEX_END,
            GENERATED_INDEX_START
        );
    }

    let sep = if existing.is_empty() || existing.ends_with("\n\n") {
        ""
    } else if existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    Ok(format!("{existing}{sep}{replacement}"))
}

fn warn_if_wiki_not_clean(wiki_root: &Path) {
    match validate_wiki(wiki_root) {
        Ok(report) => {
            if !report.is_clean() {
                eprintln!(
                    "agwiki export-skill: wiki validation reported {} problem(s); run `agwiki validate` for strict checks.",
                    report.problems.len()
                );
                eprintln!("{}", report.to_text());
            }
        }
        Err(e) => {
            eprintln!(
                "agwiki export-skill: wiki validation failed to run: {:#}",
                e
            );
        }
    }
}

/// Mirror each top-level `wiki/<dir>/` into `skill/references/<dir>/`, then update `SKILL.md` inside agwiki markers.
pub fn run_export(opts: ExportOptions<'_>) -> Result<()> {
    let wiki_root = opts.wiki_root.canonicalize()?;
    let skill_root_base = opts
        .skill_root
        .map(Path::to_path_buf)
        .unwrap_or_else(|| wiki_root.join("skill"));
    let skill_root = skill_root_base.canonicalize().unwrap_or(skill_root_base);
    let skill_md = opts
        .skill_md
        .map(Path::to_path_buf)
        .unwrap_or_else(|| skill_root.join("SKILL.md"));
    let index_path = wiki_root.join("wiki").join("index.md");

    let sections = wiki_mirror_sections(&wiki_root)?;

    if !index_path.is_file() {
        bail!("index not found: {}", index_path.display());
    }

    if opts.dry_run {
        println!("wiki_root={}", wiki_root.display());
        println!("skill_root={}", skill_root.display());
        println!("skill_md={}", skill_md.display());
    }

    for sec in &sections {
        copy_section(&wiki_root, &skill_root, sec, opts.dry_run)?;
        if opts.prune {
            prune_section(&wiki_root, &skill_root, sec, opts.dry_run)?;
        }
    }

    let index_text = fs::read_to_string(&index_path)?;
    let (groups, included) = parse_index_for_groups(&wiki_root, &index_text, &sections);

    if !opts.dry_run {
        fs::create_dir_all(&skill_root)?;
        fs::create_dir_all(skill_root.join("references"))?;
    }

    let all_on_disk = list_mirrored_files(&skill_root, &sections);
    let mut orphan_refs: Vec<String> = all_on_disk
        .into_iter()
        .filter(|r| !included.contains(r))
        .collect();
    orphan_refs.sort();

    let gen = render_generated_block(&groups, &orphan_refs);

    if opts.dry_run {
        warn_if_wiki_not_clean(&wiki_root);
        println!("--- generated index (inside markers) ---");
        print!("{}", gen);
        let _ = std::io::stdout().flush();
        return Ok(());
    }

    let existing = fs::read_to_string(&skill_md).unwrap_or_default();
    let merged = merge_skill_generated_index(&existing, &gen)?;
    fs::write(&skill_md, merged)?;

    warn_if_wiki_not_clean(&wiki_root);

    Ok(())
}
