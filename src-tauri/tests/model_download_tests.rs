// Model download tests, run against a local raw-HTTP server so timeouts and
// failure modes are exercised without touching the network.

use ai4all_tauri::model_manager::{DownloadProgress, DownloadTimeouts, ModelManager};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BODY: &[u8] = b"0123456789";
const MODEL: &str = "model.gguf";

fn fast_timeouts() -> DownloadTimeouts {
    DownloadTimeouts {
        connect: Duration::from_secs(1),
        idle: Duration::from_millis(300),
        total: Duration::from_secs(5),
    }
}

/// Serve one request: send headers for BODY, then send BODY one byte at a
/// time with `delay_per_byte` between bytes, stopping after `send_bytes`
/// bytes and holding the connection open.
async fn serve(send_bytes: usize, delay_per_byte: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = socket.read(&mut buf).await;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\r\n",
            BODY.len()
        );
        socket.write_all(headers.as_bytes()).await.unwrap();
        for byte in &BODY[..send_bytes] {
            tokio::time::sleep(delay_per_byte).await;
            if socket.write_all(&[*byte]).await.is_err() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    format!("http://{}/{}", addr, MODEL)
}

/// Serve one request with an empty-bodied response of the given status.
async fn serve_status(status: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = socket.read(&mut buf).await;
        let response = format!("HTTP/1.1 {}\r\nContent-Length: 0\r\n\r\n", status);
        socket.write_all(response.as_bytes()).await.unwrap();
    });
    format!("http://{}/{}", addr, MODEL)
}

fn manager() -> (TempDir, ModelManager) {
    let dir = TempDir::new().unwrap();
    let manager = ModelManager::new(dir.path().to_path_buf());
    (dir, manager)
}

#[tokio::test]
async fn downloads_the_whole_body_and_removes_the_temp_file() {
    let (_dir, manager) = manager();
    let url = serve(BODY.len(), Duration::ZERO).await;

    let path = manager
        .download_model_with_timeouts(&url, MODEL, fast_timeouts(), |_| {})
        .await
        .unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), BODY);
    assert!(!path.with_extension("tmp").exists());
}

#[tokio::test]
async fn slow_but_steady_download_is_not_cut_off() {
    // Regression test: the whole download used to share one 5 minute
    // timeout, so a slow connection failed partway through. Here the
    // transfer takes ~4x the idle timeout, but never idles for long.
    let (_dir, manager) = manager();
    let url = serve(BODY.len(), Duration::from_millis(120)).await;

    let path = manager
        .download_model_with_timeouts(&url, MODEL, fast_timeouts(), |_| {})
        .await
        .unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), BODY);
}

#[tokio::test]
async fn stalled_download_fails_after_the_idle_timeout() {
    let (_dir, manager) = manager();
    let url = serve(3, Duration::ZERO).await;

    let err = manager
        .download_model_with_timeouts(&url, MODEL, fast_timeouts(), |_| {})
        .await
        .unwrap_err();

    assert!(err.contains("stalled"), "unexpected error: {err}");
    assert!(!manager.is_model_cached(MODEL));
}

#[tokio::test]
async fn download_fails_once_the_total_timeout_is_exceeded() {
    let (_dir, manager) = manager();
    let url = serve(BODY.len(), Duration::from_millis(100)).await;
    let timeouts = DownloadTimeouts {
        total: Duration::from_millis(400),
        ..fast_timeouts()
    };

    let err = manager
        .download_model_with_timeouts(&url, MODEL, timeouts, |_| {})
        .await
        .unwrap_err();

    assert!(err.contains("timed out after"), "unexpected error: {err}");
    assert!(!manager.is_model_cached(MODEL));
}

#[tokio::test]
async fn http_error_status_fails_without_creating_files() {
    let (_dir, manager) = manager();
    let url = serve_status("404 Not Found").await;

    let err = manager
        .download_model_with_timeouts(&url, MODEL, fast_timeouts(), |_| {})
        .await
        .unwrap_err();

    assert!(err.contains("404"), "unexpected error: {err}");
    let dest = manager.get_cached_model_path(MODEL);
    assert!(!dest.exists());
    assert!(!dest.with_extension("tmp").exists());
}

#[tokio::test]
async fn progress_starts_at_zero_and_ends_complete() {
    let (_dir, manager) = manager();
    let url = serve(BODY.len(), Duration::ZERO).await;
    let events: Arc<Mutex<Vec<DownloadProgress>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();

    manager
        .download_model_with_timeouts(&url, MODEL, fast_timeouts(), move |p| {
            sink.lock().unwrap().push(p)
        })
        .await
        .unwrap();

    let events = events.lock().unwrap();
    let first = events.first().unwrap();
    let last = events.last().unwrap();
    assert_eq!(first.percentage, 0.0);
    assert_eq!(last.percentage, 100.0);
    assert_eq!(last.total_bytes, BODY.len() as u64);
    assert_eq!(last.status, "Download complete");
}

#[tokio::test]
async fn refuses_a_model_name_that_would_escape_the_cache_dir() {
    let parent = TempDir::new().unwrap();
    let cache = parent.path().join("models");
    std::fs::create_dir_all(&cache).unwrap();
    let manager = ModelManager::new(cache.clone());
    // Served but never fetched: the name is refused before any request.
    let url = serve(BODY.len(), Duration::ZERO).await;

    for name in ["../escape.gguf", "/tmp/escape.gguf", "sub/escape.gguf"] {
        let result = manager
            .download_model_with_timeouts(&url, name, fast_timeouts(), |_| {})
            .await;
        assert!(result.is_err(), "{} was accepted", name);
    }

    assert!(!parent.path().join("escape.gguf").exists());
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
}
