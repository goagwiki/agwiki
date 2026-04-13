use pulldown_cmark::{html, CowStr, Event, Options, Parser};
use regex::Captures;
use std::borrow::Cow;
use std::path::Path;

use crate::upkeep::{mdlink_re, norm_md_link, norm_wikilink_target, wikilink_re};

#[derive(Debug, Clone)]
pub struct Templates {
    pub page_template: &'static str,
    pub index_template: &'static str,
    pub search_template: &'static str,
    pub style_css: &'static str,
    pub search_js: &'static str,
}

impl Templates {
    pub fn new() -> Self {
        Self {
            page_template: include_str!("assets/page.html"),
            index_template: include_str!("assets/index.html"),
            search_template: include_str!("assets/search.html"),
            style_css: include_str!("assets/style.css"),
            search_js: include_str!("assets/search.js"),
        }
    }

    pub fn render_page(
        &self,
        title: &str,
        content_html: &str,
        current_url: &str,
    ) -> anyhow::Result<String> {
        let mut out = self.page_template.to_string();
        out = out.replace("{{TITLE}}", &html_escape(title));
        out = out.replace("{{CONTENT}}", content_html);
        out = out.replace("{{CURRENT_URL}}", &html_escape(current_url));
        Ok(out)
    }
}

impl Default for Templates {
    fn default() -> Self {
        Self::new()
    }
}

pub fn extract_title(markdown: &str, md_path: &Path) -> String {
    for line in markdown.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("# ") {
            let title = rest.trim();
            if !title.is_empty() {
                return title.to_string();
            }
        }
    }
    md_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Wiki")
        .to_string()
}

pub fn rewrite_links<F>(markdown: &str, src_md: &Path, wiki_dir: &Path, url_for: F) -> String
where
    F: Fn(&Path) -> Option<String> + Copy,
{
    let wl = wikilink_re();
    let md = mdlink_re();

    let out = wl
        .replace_all(markdown, |caps: &Captures| {
            let full = caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            let inner = full.trim_start_matches("[[").trim_end_matches("]]");
            let (target, label) = inner
                .split_once('|')
                .map(|(a, b)| (a.trim(), b.trim()))
                .unwrap_or((inner.trim(), inner.trim()));
            if let Some(dest) = norm_wikilink_target(wiki_dir, target) {
                if let Some(url) = url_for(&dest) {
                    return Cow::Owned(format!("[{}]({})", label, url));
                }
            }
            Cow::Owned(full)
        })
        .into_owned();

    md.replace_all(&out, |caps: &Captures| {
        let full = caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
        let Some(target_m) = caps.get(1) else {
            return Cow::Owned(full);
        };
        let target = target_m.as_str();
        let Some(dest) = norm_md_link(wiki_dir, src_md, target) else {
            return Cow::Owned(full);
        };
        let Some(url) = url_for(&dest) else {
            return Cow::Owned(full);
        };

        let full_start = caps.get(0).unwrap().start();
        let t_start = target_m.start().saturating_sub(full_start);
        let t_end = target_m.end().saturating_sub(full_start);
        if t_end > full.len() || t_start > t_end {
            return Cow::Owned(full);
        }

        let mut replaced = String::with_capacity(full.len() - (t_end - t_start) + url.len());
        replaced.push_str(&full[..t_start]);
        replaced.push_str(&url);
        replaced.push_str(&full[t_end..]);
        Cow::Owned(replaced)
    })
    .into_owned()
}

pub fn markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let parser = Parser::new_ext(markdown, options).map(|e| match e {
        Event::Html(_) | Event::InlineHtml(_) => Event::Text(CowStr::Borrowed("")),
        other => other,
    });

    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn rewrite_links_converts_wikilinks_to_urls() {
        let tmp = tempdir().unwrap();
        let wiki_dir = tmp.path().join("wiki");
        fs::create_dir_all(wiki_dir.join("concepts")).unwrap();
        let src = wiki_dir.join("index.md");
        fs::write(&src, "See [[concepts/hello|Hello]]\n").unwrap();
        fs::write(wiki_dir.join("concepts/hello.md"), "# Hello\n").unwrap();

        let md = fs::read_to_string(&src).unwrap();
        let out = rewrite_links(&md, &src, &wiki_dir, |p| {
            let rel = p.strip_prefix(&wiki_dir).ok()?;
            let mut rel_no_ext = rel.to_path_buf();
            rel_no_ext.set_extension("");
            Some(format!("/wiki/{}", rel_no_ext.to_string_lossy()))
        });
        assert!(out.contains("[Hello](/wiki/concepts/hello)"));
    }

    #[test]
    fn markdown_to_html_strips_raw_html() {
        let html = markdown_to_html("# T\n\n<script>alert(1)</script>\n");
        assert!(!html.contains("<script>"));
        assert!(html.contains("<h1>"));
    }
}
