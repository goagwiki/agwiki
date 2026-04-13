use agwiki::serve::{ServerConfig, WikiServer};
use axum::body::{to_bytes, Body};
use http::Request;
use tower::ServiceExt;

use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn serve_root_renders_index_and_converts_links() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki/concepts")).unwrap();
    fs::write(
        root.join("wiki/index.md"),
        "# Index\n\n- [[concepts/hello|Hello]]\n- [Hello md](concepts/hello.md)\n- [[concepts/hello#sec|Hello sec]]\n- [Hello sec md](concepts/hello.md#sec)\n",
    )
    .unwrap();
    fs::write(root.join("wiki/concepts/hello.md"), "# Hello\n").unwrap();

    let server = WikiServer::new(ServerConfig {
        port: 0,
        host: "127.0.0.1".to_string(),
        open_browser: false,
        wiki_root: root.to_path_buf(),
    })
    .unwrap();

    let app = std::sync::Arc::new(server).router();
    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("href=\"/wiki/concepts/hello\""));
    assert!(html.contains("href=\"/wiki/concepts/hello#sec\""));
}

#[tokio::test]
async fn serve_search_returns_json_results() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::write(root.join("wiki/index.md"), "# Index\nhello world\n").unwrap();
    fs::write(root.join("wiki/other.md"), "# Other\nworld peace\n").unwrap();

    let server = WikiServer::new(ServerConfig {
        port: 0,
        host: "127.0.0.1".to_string(),
        open_browser: false,
        wiki_root: root.to_path_buf(),
    })
    .unwrap();

    let app = std::sync::Arc::new(server).router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/search?q=world")
                .header("accept", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().any(|r| r["url"] == "/wiki/index"));
}

#[tokio::test]
async fn serve_assets_are_embedded() {
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

    let app = std::sync::Arc::new(server).router();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/assets/style.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/css"));
}
