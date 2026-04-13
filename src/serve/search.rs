use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::upkeep::walk_md;

/// In-memory full-text index for markdown files under `wiki/`.
///
/// Built once at server startup by tokenizing each line of each `*.md` file.
#[derive(Debug, Clone)]
pub struct SearchIndex {
    /// Token -> occurrences across files.
    pub word_map: HashMap<String, Vec<SearchEntry>>,
    /// Canonical markdown path -> full file contents.
    pub file_content: HashMap<PathBuf, String>,
}

/// One word occurrence in a specific file/line, used for snippets and ranking.
#[derive(Debug, Clone)]
pub struct SearchEntry {
    /// Canonical path to the markdown file that contained the word.
    pub file_path: PathBuf,
    /// 1-based line number within the file.
    pub line_number: usize,
    /// Trimmed line text (used for snippets).
    pub context: String,
}

/// JSON-serializable search result returned by `GET /search?q=...`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SearchResult {
    /// Display path (e.g. `wiki/concepts/example.md`).
    pub file_path: String,
    /// Best-effort title (first `# ` heading or file stem).
    pub title: String,
    /// Short snippet for quick scanning.
    pub snippet: String,
    /// Server-relative URL to open the page (e.g. `/wiki/concepts/example`).
    pub url: String,
}

impl SearchIndex {
    /// Build a `SearchIndex` from all markdown files under `wiki_dir`.
    ///
    /// Returns an error if any markdown file is not valid UTF-8.
    pub fn build(wiki_dir: &Path) -> Result<Self> {
        let mut word_map: HashMap<String, Vec<SearchEntry>> = HashMap::new();
        let mut file_content: HashMap<PathBuf, String> = HashMap::new();

        let mut files = walk_md(wiki_dir);
        files.sort_by_key(|p| p.to_string_lossy().to_string());

        for path in files {
            let canon = path.canonicalize().unwrap_or(path);
            let text = std::fs::read_to_string(&canon)
                .with_context(|| format!("read markdown {}", canon.display()))?;
            file_content.insert(canon.clone(), text.clone());

            for (idx, line) in text.lines().enumerate() {
                let line_no = idx + 1;
                let ctx = line.trim().to_string();
                for word in tokenize(line) {
                    word_map.entry(word).or_default().push(SearchEntry {
                        file_path: canon.clone(),
                        line_number: line_no,
                        context: ctx.clone(),
                    });
                }
            }
        }

        Ok(Self {
            word_map,
            file_content,
        })
    }

    /// Search `query` and return ranked results.
    ///
    /// Tokenization is ASCII-lowercased alphanumeric runs. Results are ranked by:
    /// 1) number of distinct query words matched, then 2) total hits, then 3) path.
    pub fn search<F>(&self, query: &str, wiki_dir: &Path, url_for: F) -> Vec<SearchResult>
    where
        F: Fn(&Path) -> Option<String> + Copy,
    {
        let words: Vec<String> = tokenize(query).into_iter().collect();
        if words.is_empty() {
            return Vec::new();
        }
        let unique: Vec<String> = {
            let mut seen = HashSet::new();
            let mut out = Vec::new();
            for w in words {
                if seen.insert(w.clone()) {
                    out.push(w);
                }
            }
            out
        };

        #[derive(Default)]
        struct Score {
            matched_words: usize,
            hits: usize,
            first: Option<SearchEntry>,
        }

        let mut scores: HashMap<PathBuf, Score> = HashMap::new();

        for w in &unique {
            let Some(entries) = self.word_map.get(w) else {
                continue;
            };
            let mut touched: HashSet<&Path> = HashSet::new();
            for e in entries {
                let score = scores.entry(e.file_path.clone()).or_default();
                score.hits += 1;
                if score.first.is_none() {
                    score.first = Some(e.clone());
                }
                touched.insert(&e.file_path);
            }
            for p in touched {
                if let Some(s) = scores.get_mut(p) {
                    s.matched_words += 1;
                }
            }
        }

        let mut ranked: Vec<(PathBuf, usize, usize, SearchEntry)> = scores
            .into_iter()
            .filter_map(|(p, s)| Some((p, s.matched_words, s.hits, s.first?)))
            .collect();
        ranked.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| a.0.to_string_lossy().cmp(&b.0.to_string_lossy()))
        });

        ranked
            .into_iter()
            .take(50)
            .filter_map(|(path, _, _, first)| {
                let url = url_for(&path)?;
                let rel = path.strip_prefix(wiki_dir).ok()?;
                let file_path = format!(
                    "wiki/{}",
                    rel.to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/")
                );
                let content = self
                    .file_content
                    .get(&path)
                    .map(String::as_str)
                    .unwrap_or("");
                let title = super::templates::extract_title(content, &path);
                Some(SearchResult {
                    file_path,
                    title,
                    snippet: snippet(&first.context),
                    url,
                })
            })
            .collect()
    }
}

fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn snippet(line: &str) -> String {
    const MAX: usize = 180;
    let t = line.trim();
    if t.len() <= MAX {
        return t.to_string();
    }
    let mut s = t[..MAX].to_string();
    s.push('…');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn search_index_finds_words_across_files() {
        let tmp = tempdir().unwrap();
        let wiki_dir = tmp.path().join("wiki");
        fs::create_dir_all(&wiki_dir).unwrap();
        fs::write(wiki_dir.join("index.md"), "# Index\nhello world\n").unwrap();
        fs::write(wiki_dir.join("a.md"), "# A\nworld peace\n").unwrap();

        let idx = SearchIndex::build(&wiki_dir).unwrap();
        let results = idx.search("world", &wiki_dir, |p| {
            let rel = p.strip_prefix(&wiki_dir).ok()?;
            let mut rel_no_ext = rel.to_path_buf();
            rel_no_ext.set_extension("");
            Some(format!("/wiki/{}", rel_no_ext.to_string_lossy()))
        });

        assert!(results.iter().any(|r| r.url == "/wiki/index"));
        assert!(results.iter().any(|r| r.url == "/wiki/a"));
    }

    #[test]
    fn search_index_requires_utf8_markdown() {
        let tmp = tempdir().unwrap();
        let wiki_dir = tmp.path().join("wiki");
        fs::create_dir_all(&wiki_dir).unwrap();
        fs::write(wiki_dir.join("index.md"), b"# Index\n").unwrap();
        fs::write(wiki_dir.join("bad.md"), vec![0xff, 0xfe, 0xfd]).unwrap();

        let err = SearchIndex::build(&wiki_dir).unwrap_err().to_string();
        assert!(err.contains("read markdown"));
    }
}
