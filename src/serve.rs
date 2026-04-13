//! Local web server for browsing a markdown wiki.
//!
//! Security notes:
//! - Only files under `<wiki-root>/wiki/` are served, and URL paths are resolved with traversal protection.
//! - Raw HTML in markdown is stripped during rendering to reduce script injection risks when viewing untrusted
//!   content locally.

use anyhow::{Context, Result};
use axum::extract::ConnectInfo;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderValue, Request, StatusCode, Uri, Version};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{middleware, Json, Router};
use mime_guess::MimeGuess;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use time::{format_description, OffsetDateTime};
use tokio::net::TcpListener;
use tokio::signal;
use tower::ServiceBuilder;

use crate::upkeep::validate_wiki_root;

mod search;
mod templates;

pub use search::{SearchEntry, SearchIndex, SearchResult};
pub use templates::Templates;

/// Runtime configuration for `agwiki serve`.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// TCP port to bind to (use `0` to ask the OS to choose a free port).
    pub port: u16,
    /// Host/IP to bind to (default `127.0.0.1`).
    pub host: String,
    /// If true, attempts to open the server URL in the default browser.
    pub open_browser: bool,
    /// Canonical wiki root path (must contain a `wiki/` directory).
    pub wiki_root: PathBuf,
}

/// A running wiki server with a startup-built search index and embedded templates.
#[derive(Debug)]
pub struct WikiServer {
    config: ServerConfig,
    wiki_dir: PathBuf,
    templates: Templates,
    search_index: SearchIndex,
}

impl WikiServer {
    /// Create a server, validating the wiki root and building the search index.
    pub fn new(mut config: ServerConfig) -> Result<Self> {
        config.wiki_root = validate_wiki_root(&config.wiki_root)
            .map_err(|e| anyhow::anyhow!("SERVE_INVALID_WIKI: {e}"))?;

        let wiki_dir = config.wiki_root.join("wiki").canonicalize()?;
        let templates = Templates::new();
        let search_index = SearchIndex::build(&wiki_dir)
            .map_err(|e| anyhow::anyhow!("SERVE_SEARCH_BUILD_ERROR: {e}"))?;

        Ok(Self {
            config,
            wiki_dir,
            templates,
            search_index,
        })
    }

    /// Start the HTTP server and block until Ctrl+C.
    pub async fn start(self) -> Result<()> {
        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .with_context(|| {
                format!(
                    "invalid host/port {}:{}",
                    self.config.host, self.config.port
                )
            })
            .map_err(|e| anyhow::anyhow!("SERVE_INVALID_WIKI: {e}"))?;

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| anyhow::anyhow!("SERVE_PORT_UNAVAILABLE: {e}"))?;
        let local_addr = listener.local_addr()?;
        let url_host = match local_addr.ip() {
            ip if ip.is_unspecified() => "127.0.0.1".to_string(),
            ip => ip.to_string(),
        };
        let base_url = format!("http://{}:{}/", url_host, local_addr.port());
        if self.config.open_browser {
            if let Err(e) = webbrowser::open(&base_url) {
                eprintln!("warning: failed to open browser: {e}");
            }
        }

        let state = Arc::new(self);
        let app = state.router();

        eprintln!("Serving wiki at {base_url} (Ctrl+C to stop)");
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await?;
        Ok(())
    }

    /// Build the axum router for this server.
    pub fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/", get(handlers::root))
            .route("/wiki/*path", get(handlers::wiki_page))
            .route("/search", get(handlers::search))
            .route("/assets/:file", get(handlers::assets))
            .with_state(Arc::clone(self))
            .layer(
                ServiceBuilder::new()
                    .layer(middleware::from_fn(clf_log))
                    .into_inner(),
            )
    }

    fn url_for_md_path(&self, md_path: &Path) -> Option<String> {
        let rel = md_path.strip_prefix(&self.wiki_dir).ok()?;
        let mut rel_no_ext = rel.to_path_buf();
        if rel_no_ext.extension().and_then(|s| s.to_str()) == Some("md") {
            rel_no_ext.set_extension("");
        }
        let rel_s = rel_no_ext
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        Some(format!("/wiki/{}", rel_s.trim_start_matches('/')))
    }

    fn render_markdown_file(&self, md_path: &Path) -> Result<(String, String)> {
        let raw = std::fs::read_to_string(md_path)
            .map_err(|e| anyhow::anyhow!("SERVE_FILE_READ_ERROR: {e}"))?;
        let title = templates::extract_title(&raw, md_path);
        let rewritten =
            templates::rewrite_links(&raw, md_path, &self.wiki_dir, |p| self.url_for_md_path(p));
        let content_html = templates::markdown_to_html(&rewritten);
        Ok((title, content_html))
    }

    fn asset_bytes(&self, file: &str) -> Option<(&'static [u8], &'static str)> {
        match file {
            "style.css" => Some((
                self.templates.style_css.as_bytes(),
                "text/css; charset=utf-8",
            )),
            "search.js" => Some((
                self.templates.search_js.as_bytes(),
                "text/javascript; charset=utf-8",
            )),
            _ => None,
        }
    }

    fn search(&self, query: &str) -> Vec<SearchResult> {
        self.search_index
            .search(query, &self.wiki_dir, |p| self.url_for_md_path(p))
    }
}

async fn shutdown_signal() {
    let _ = signal::ctrl_c().await;
}

async fn clf_log(req: Request<axum::body::Body>, next: middleware::Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let version = req.version();
    let remote = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "-".to_string());
    let resp = next.run(req).await;
    let status = resp.status();

    // Common Log Format:
    // host ident authuser [date] "request" status bytes
    let ts = OffsetDateTime::now_utc();
    let ts_s = ts
        .format(clf_timestamp_format())
        .unwrap_or_else(|_| "-".to_string());
    let bytes = resp
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");
    println!(
        "{} - - [{}] \"{} {} {}\" {} {}",
        remote,
        ts_s,
        method,
        uri_to_path_query(&uri),
        version_to_str(version),
        status.as_u16(),
        bytes
    );

    resp
}

fn clf_timestamp_format() -> &'static [format_description::FormatItem<'static>] {
    static FMT: OnceLock<Vec<format_description::FormatItem<'static>>> = OnceLock::new();
    FMT.get_or_init(|| {
        format_description::parse("[day]/[month repr:short]/[year]:[hour]:[minute]:[second] +0000")
            .unwrap_or_default()
    })
}

fn uri_to_path_query(uri: &Uri) -> String {
    match uri.path_and_query() {
        Some(pq) => pq.as_str().to_string(),
        None => uri.path().to_string(),
    }
}

fn version_to_str(v: Version) -> &'static str {
    match v {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2.0",
        Version::HTTP_3 => "HTTP/3.0",
        _ => "HTTP/?",
    }
}

mod handlers {
    use super::*;
    use crate::upkeep::resolve_under_root;

    #[derive(Debug, serde::Deserialize)]
    pub struct SearchQuery {
        pub q: Option<String>,
    }

    pub async fn root(State(state): State<Arc<WikiServer>>) -> Response {
        wiki_page(State(state), AxumPath(String::new())).await
    }

    pub async fn wiki_page(
        State(state): State<Arc<WikiServer>>,
        AxumPath(path): AxumPath<String>,
    ) -> Response {
        let rel = if path.trim().is_empty() {
            PathBuf::from("index.md")
        } else {
            let trimmed = path.trim_start_matches('/');
            let p = PathBuf::from(trimmed);
            match p.extension().and_then(|s| s.to_str()) {
                Some("md") => p,
                Some(_) => p,
                None => PathBuf::from(format!("{trimmed}.md")),
            }
        };

        let file_path = match resolve_under_root(&state.wiki_dir, &rel) {
            Some(p) => p,
            None => {
                eprintln!("404 /wiki/{path} (path traversal blocked)");
                return (StatusCode::NOT_FOUND, "Not Found").into_response();
            }
        };

        if !file_path.is_file() {
            eprintln!("404 /wiki/{path} (missing: {})", file_path.display());
            return (StatusCode::NOT_FOUND, "Not Found").into_response();
        }

        if rel.extension().and_then(|s| s.to_str()) != Some("md") {
            return serve_static_file(&file_path, &path);
        }

        match state.render_markdown_file(&file_path) {
            Ok((title, content_html)) => {
                let url = state
                    .url_for_md_path(&file_path)
                    .unwrap_or_else(|| "/".to_string());
                let body = state
                    .templates
                    .render_page(&title, &content_html, &url)
                    .map_err(|e| anyhow::anyhow!("SERVE_TEMPLATE_ERROR: {e}"));
                match body {
                    Ok(html) => Html(html).into_response(),
                    Err(e) => {
                        eprintln!("500 /wiki/{path} ({e})");
                        (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
                    }
                }
            }
            Err(e) => {
                eprintln!("500 /wiki/{path} ({e})");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
            }
        }
    }

    fn serve_static_file(file_path: &Path, req_path: &str) -> Response {
        let bytes = match std::fs::read(file_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("500 /wiki/{req_path} (SERVE_FILE_READ_ERROR: {e})");
                return (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
                    .into_response();
            }
        };
        let mime = MimeGuess::from_path(file_path).first_or_octet_stream();
        let mut resp = bytes.into_response();
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(mime.as_ref())
                .unwrap_or(HeaderValue::from_static("application/octet-stream")),
        );
        resp
    }

    pub async fn search(
        State(state): State<Arc<WikiServer>>,
        Query(q): Query<SearchQuery>,
    ) -> Response {
        let query = q.q.unwrap_or_default();
        let results = state.search(query.trim());
        Json(results).into_response()
    }

    pub async fn assets(
        State(state): State<Arc<WikiServer>>,
        AxumPath(file): AxumPath<String>,
    ) -> Response {
        let Some((bytes, content_type)) = state.asset_bytes(&file) else {
            eprintln!("404 /assets/{file} (missing asset)");
            return (StatusCode::NOT_FOUND, "Not Found").into_response();
        };

        let mut resp = bytes.into_response();
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(content_type)
                .unwrap_or(HeaderValue::from_static("application/octet-stream")),
        );
        resp
    }
}

/// Blocking wrapper around [`WikiServer::start`].
pub fn run_serve_blocking(config: ServerConfig) -> Result<()> {
    let server = WikiServer::new(config)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create tokio runtime")?;
    rt.block_on(server.start())
}

// Keep this private for now; it is used by `run_serve_blocking` and tests.
async fn _run_serve(config: ServerConfig) -> Result<()> {
    let server = WikiServer::new(config)?;
    server.start().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use http::Request;
    use std::fs;
    use tempfile::tempdir;
    use tower::ServiceExt;

    #[tokio::test]
    async fn router_serves_index() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("wiki")).unwrap();
        fs::write(root.join("wiki/index.md"), "# Index\n").unwrap();

        let server = WikiServer::new(ServerConfig {
            port: 0,
            host: "127.0.0.1".to_string(),
            open_browser: false,
            wiki_root: root.to_path_buf(),
        })
        .unwrap();
        let app = Arc::new(server).router();
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("<h1>"));
    }

    #[test]
    fn new_rejects_missing_wiki_dir() {
        let tmp = tempdir().unwrap();
        let err = WikiServer::new(ServerConfig {
            port: 0,
            host: "127.0.0.1".to_string(),
            open_browser: false,
            wiki_root: tmp.path().to_path_buf(),
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("SERVE_INVALID_WIKI"));
    }
}
