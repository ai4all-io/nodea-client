use ai4all_tauri::model_manager::{DownloadProgress, ModelManager};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tempfile::TempDir;

#[tokio::test]
async fn test_download_progress_callback() {
    // This test verifies the progress callback is called during download
    let temp_dir = TempDir::new().unwrap();
    let manager = ModelManager::new(temp_dir.path().to_path_buf());

    let progress_calls = Arc::new(AtomicU64::new(0));
    let progress_calls_clone = progress_calls.clone();

    let callback = move |progress: DownloadProgress| {
        progress_calls_clone.fetch_add(1, Ordering::SeqCst);
        println!(
            "Progress: {:.1}% - {}",
            progress.percentage, progress.status
        );
    };

    // Test with a small file (this will fail gracefully for invalid URLs)
    let _result = manager
        .download_model(
            "https://httpbin.org/bytes/1024",
            "test-download.gguf",
            callback,
        )
        .await;

    // We expect at least one progress callback
    // (Even on failure, we should get initial callback)
    assert!(progress_calls.load(Ordering::SeqCst) > 0);
}

#[tokio::test]
async fn test_cached_model_skips_download() {
    let temp_dir = TempDir::new().unwrap();
    let manager = ModelManager::new(temp_dir.path().to_path_buf());

    // Create a fake cached model
    let model_path = manager.get_cached_model_path("fake-model.gguf");
    std::fs::write(&model_path, b"fake model data").unwrap();

    let download_called = Arc::new(AtomicU64::new(0));
    let download_called_clone = download_called.clone();

    let callback = move |_: DownloadProgress| {
        download_called_clone.fetch_add(1, Ordering::SeqCst);
    };

    // Try to download - should skip since cached
    let result = manager
        .download_model(
            "https://example.com/model.gguf",
            "fake-model.gguf",
            callback,
        )
        .await;

    assert!(result.is_ok());
    // Should only get one callback (the "already cached" message)
    assert_eq!(download_called.load(Ordering::SeqCst), 1);
}
