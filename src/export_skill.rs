//! Mirror wiki sections into `skill/references/` and generate `SKILL.md` from template + index.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn wikilink_full_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\[\[([^\]|]+)(?:\|([^\]]+))?\]\]").unwrap())
}

fn heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(#{2,3})\s+(.+?)\s*$").unwrap())
}

/// Inputs for [`run_export`]: wiki tree, optional paths, section list, and post-copy options.
pub struct ExportOptions<'a> {
    /// Content wiki root (contains `wiki/`).
    pub wiki_root: &'a Path,
    pub skill_dir: Option<&'a Path>,
    pub template: Option<&'a Path>,
    pub output: Option<&'a Path>,
    pub index: Option<&'a Path>,
    /// Comma-separated top-level names under `wiki/` to mirror (e.g. `concepts,topics,projects`).
    pub sections: &'a str,
    pub dry_run: bool,
    pub prune: bool,
    pub rewrite_wikilinks: bool,
}

fn first_heading_title(md_text: &str) -> Option<String> {
    for line in md_text.lines() {
        if let Some(rest) = line.strip_prefix('#') {
            let t = rest.trim_start_matches('#').trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
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

fn build_slug_maps(
    skill_dir: &Path,
    sections: &[String],
) -> (HashMap<String, PathBuf>, HashMap<String, PathBuf>) {
    let mut stem_map: HashMap<String, PathBuf> = HashMap::new();
    let mut full_map: HashMap<String, PathBuf> = HashMap::new();
    let ref_root = skill_dir.join("references");
    for section in sections {
        let sec_dir = ref_root.join(section);
        if !sec_dir.is_dir() {
            continue;
        }
        let mut paths: Vec<_> = walk_md(&sec_dir);
        paths.sort();
        for p in paths {
            let Ok(rel_from_ref) = p.strip_prefix(&ref_root) else {
                continue;
            };
            let key = rel_from_ref.to_string_lossy().replace('\\', "/");
            let key = key.strip_suffix(".md").unwrap_or(&key).to_string();
            full_map.entry(key.clone()).or_insert_with(|| p.clone());
            let stem = p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            stem_map.entry(stem).or_insert_with(|| p.clone());
        }
    }
    (stem_map, full_map)
}

fn resolve_wikilink_target(
    raw: &str,
    stem_map: &HashMap<String, PathBuf>,
    full_map: &HashMap<String, PathBuf>,
) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if full_map.contains_key(raw) {
        return full_map.get(raw).cloned();
    }
    let key = raw.strip_suffix(".md").unwrap_or(raw);
    if full_map.contains_key(key) {
        return full_map.get(key).cloned();
    }
    if raw.contains('/') {
        return full_map.get(raw).cloned();
    }
    stem_map.get(raw).cloned()
}

fn markdown_link_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace(']', "\\]")
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

fn rewrite_wikilinks_in_file(
    path: &Path,
    stem_map: &HashMap<String, PathBuf>,
    full_map: &HashMap<String, PathBuf>,
    dry_run: bool,
) -> Result<()> {
    let text = fs::read_to_string(path)?;
    let re = wikilink_full_re();
    let mut new_text = String::with_capacity(text.len());
    let mut last = 0usize;
    for cap in re.captures_iter(&text) {
        let m = cap.get(0).unwrap();
        new_text.push_str(&text[last..m.start()]);
        let target = cap.get(1).map(|x| x.as_str()).unwrap_or("").trim();
        let alias = cap.get(2).map(|x| x.as_str());
        let repl = if let Some(dest) = resolve_wikilink_target(target, stem_map, full_map) {
            if dest.is_file() {
                let parent = path.parent().unwrap_or(Path::new("."));
                let rel = pathdiff::diff_paths(&dest, parent)
                    .unwrap_or_else(|| dest.strip_prefix(parent).unwrap_or(&dest).to_path_buf());
                let rel = rel.to_string_lossy().replace('\\', "/");
                let label = alias
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        let inner = fs::read_to_string(&dest).unwrap_or_default();
                        first_heading_title(&inner).unwrap_or_else(|| {
                            dest.file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default()
                        })
                    });
                format!("[{}]({})", markdown_link_escape(&label), rel)
            } else {
                m.as_str().to_string()
            }
        } else {
            m.as_str().to_string()
        };
        new_text.push_str(&repl);
        last = m.end();
    }
    new_text.push_str(&text[last..]);
    if new_text != text {
        if dry_run {
            println!("rewrite wikilinks in {}", path.display());
        } else {
            fs::write(path, new_text)?;
        }
    }
    Ok(())
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

/// Mirror `wiki/<sections>/` into `skill/references/`, append generated index to `SKILL.md`.
pub fn run_export(opts: ExportOptions<'_>) -> Result<()> {
    let wiki_root = opts.wiki_root.canonicalize()?;
    let skill_dir_base = opts
        .skill_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| wiki_root.join("skill"));
    let skill_dir = skill_dir_base.canonicalize().unwrap_or(skill_dir_base);
    let template = opts
        .template
        .map(Path::to_path_buf)
        .unwrap_or_else(|| skill_dir.join("SKILL.md.template"));
    let output_md = opts
        .output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| skill_dir.join("SKILL.md"));
    let index_path = opts
        .index
        .map(Path::to_path_buf)
        .unwrap_or_else(|| wiki_root.join("wiki").join("index.md"));

    let sections: Vec<String> = opts
        .sections
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if !template.is_file() {
        bail!("template not found: {}", template.display());
    }
    if !index_path.is_file() {
        bail!("index not found: {}", index_path.display());
    }

    if opts.dry_run {
        println!("wiki_root={}", wiki_root.display());
        println!("skill_dir={}", skill_dir.display());
    }

    for sec in &sections {
        copy_section(&wiki_root, &skill_dir, sec, opts.dry_run)?;
        if opts.prune {
            prune_section(&wiki_root, &skill_dir, sec, opts.dry_run)?;
        }
    }

    let index_text = fs::read_to_string(&index_path)?;
    let (groups, included) = parse_index_for_groups(&wiki_root, &index_text, &sections);

    if !opts.dry_run {
        fs::create_dir_all(&skill_dir)?;
        fs::create_dir_all(skill_dir.join("references"))?;
    }

    let all_on_disk = list_mirrored_files(&skill_dir, &sections);
    let mut orphan_refs: Vec<String> = all_on_disk
        .into_iter()
        .filter(|r| !included.contains(r))
        .collect();
    orphan_refs.sort();

    let gen = render_generated_block(&groups, &orphan_refs);
    let template_body = fs::read_to_string(&template)?;
    let body = format!("{}\n\n{}", template_body.trim_end(), gen);

    if opts.dry_run {
        println!("--- generated SKILL.md tail ---");
        print!("{}", gen);
        let _ = std::io::stdout().flush();
        return Ok(());
    }

    fs::write(&output_md, body)?;

    if opts.rewrite_wikilinks {
        let (stem_map, full_map) = build_slug_maps(&skill_dir, &sections);
        let ref_root = skill_dir.join("references");
        if ref_root.is_dir() {
            let mut paths: Vec<_> = walk_md(&ref_root);
            paths.sort();
            for md in paths {
                rewrite_wikilinks_in_file(&md, &stem_map, &full_map, false)?;
            }
        }
    }

    Ok(())
}
