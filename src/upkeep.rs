//! Read-only checks: broken links and orphan wiki pages.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

pub(crate) fn wikilink_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").unwrap())
}

pub(crate) fn mdlink_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\[[^\]]*\]\(([^)]+)\)").unwrap())
}

const ENTRYPOINT_NAMES: &[&str] = &[
    "wiki/index.md",
    "wiki/inbox.md",
    "wiki/sources/index.md",
    "wiki/log.md",
];

/// Ensure `wiki_home` exists, canonicalizes, and contains a `wiki/` directory.
pub fn validate_wiki_root(wiki_home: &Path) -> Result<PathBuf> {
    let w = wiki_home
        .canonicalize()
        .with_context(|| format!("wiki root {}", wiki_home.display()))?;
    if !w.join("wiki").is_dir() {
        bail!("not a wiki root (missing wiki/): {}", w.display());
    }
    Ok(w)
}

/// Resolve `rel` under `root` (root must be canonical). Works when the leaf path does not exist.
pub(crate) fn resolve_under_root(root: &Path, rel: &Path) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    for c in rel.components() {
        match c {
            Component::Normal(x) => out.push(x),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
                if !out.starts_with(root) {
                    return None;
                }
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    out.starts_with(root).then_some(out)
}

pub(crate) fn norm_wikilink_target(wiki: &Path, target: &str) -> Option<PathBuf> {
    let mut t = target.trim();
    if t.is_empty() || t.starts_with("http://") || t.starts_with("https://") {
        return None;
    }
    if let Some((a, _)) = t.split_once('#') {
        t = a;
    }
    if t.is_empty() {
        return None;
    }
    let rel = if t.ends_with(".md") {
        t.to_string()
    } else {
        format!("{}.md", t)
    };
    resolve_under_root(wiki, Path::new(&rel))
}

pub(crate) fn norm_md_link(wiki: &Path, src_file: &Path, target: &str) -> Option<PathBuf> {
    let t = target.trim();
    if t.is_empty() || t.contains("://") || t.starts_with('#') {
        return None;
    }
    let t = t.split_once('#').map(|(a, _)| a).unwrap_or(t).trim();
    if t.is_empty() {
        return None;
    }
    let base = src_file.parent()?.canonicalize().ok()?;
    let out = resolve_under_root(&base, Path::new(t))?;
    out.starts_with(wiki).then_some(out)
}

pub(crate) fn walk_md(dir: &Path) -> Vec<PathBuf> {
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

/// Return human-readable lines for each broken wikilink or relative markdown link.
pub fn check_links(wiki_home: &Path) -> Result<Vec<String>> {
    let wiki = wiki_home.join("wiki");
    let wiki_canon = wiki.canonicalize()?;
    let mut errors = Vec::new();
    let mut paths = walk_md(&wiki);
    paths.sort_by_key(|p| p.clone());
    let wl = wikilink_re();
    let ml = mdlink_re();
    for md in paths {
        let text = std::fs::read_to_string(&md).unwrap_or_default();
        for cap in wl.captures_iter(&text) {
            let target = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            if let Some(dest) = norm_wikilink_target(&wiki_canon, target) {
                if !dest.is_file() {
                    errors.push(format!(
                        "{}: broken wikilink [[{}]] -> {}",
                        md.strip_prefix(wiki_home).unwrap_or(&md).display(),
                        target,
                        dest.strip_prefix(wiki_home).unwrap_or(&dest).display()
                    ));
                }
            }
        }
        for cap in ml.captures_iter(&text) {
            let target = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            if let Some(dest) = norm_md_link(&wiki_canon, &md, target) {
                if !dest.is_file() {
                    errors.push(format!(
                        "{}: broken link ({}) -> {}",
                        md.strip_prefix(wiki_home).unwrap_or(&md).display(),
                        target,
                        dest.strip_prefix(wiki_home).unwrap_or(&dest).display()
                    ));
                }
            }
        }
    }
    Ok(errors)
}

fn wikilink_targets(wiki: &Path, text: &str) -> HashSet<PathBuf> {
    let wiki_canon = wiki.canonicalize().unwrap_or_else(|_| wiki.to_path_buf());
    let mut out = HashSet::new();
    let wl = wikilink_re();
    for cap in wl.captures_iter(text) {
        let target = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some(dest) = norm_wikilink_target(&wiki_canon, target) {
            if dest.is_file() {
                out.insert(dest);
            }
        }
    }
    out
}

/// Pages under `wiki/` never targeted by an incoming `[[wikilink]]`, excluding known entry files.
pub fn list_orphans(wiki_home: &Path) -> Result<Vec<PathBuf>> {
    let wiki = wiki_home.join("wiki");
    let wiki_canon = wiki.canonicalize()?;
    let all_md: HashSet<PathBuf> = walk_md(&wiki)
        .into_iter()
        .map(|p| p.canonicalize().unwrap_or(p))
        .collect();
    let mut linked: HashSet<PathBuf> = HashSet::new();
    for md in &all_md {
        let text = std::fs::read_to_string(md).unwrap_or_default();
        linked.extend(wikilink_targets(&wiki_canon, &text));
    }
    let mut skip: HashSet<PathBuf> = HashSet::new();
    for name in ENTRYPOINT_NAMES {
        let ep = wiki_home.join(name);
        if let Ok(c) = ep.canonicalize() {
            if c.is_file() {
                skip.insert(c);
            }
        }
    }
    let mut orphans: Vec<PathBuf> = all_md
        .into_iter()
        .filter(|p| !skip.contains(p) && !linked.contains(p))
        .collect();
    orphans.sort_by_key(|p| p.to_string_lossy().to_string());
    Ok(orphans)
}
