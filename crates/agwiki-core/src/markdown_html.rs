//! Shared markdown-to-HTML conversion used by `serve` and `export-html`.

use pulldown_cmark::{html, CowStr, Event, Options, Parser};

/// Convert markdown to HTML using `pulldown-cmark`.
///
/// Raw HTML is stripped to reduce script injection risk when browsing untrusted content.
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
