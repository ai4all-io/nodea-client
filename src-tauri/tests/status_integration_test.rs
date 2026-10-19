use ai4all_tauri::status::{ConnectionStatus, ModelStatus, StatusSnapshot};
use std::sync::Arc;
use tokio::sync::Mutex;

#[cfg(test)]
mod tests {
    use super::*;

    // Mock AppState for testing
    #[derive(Clone)]
    struct TestAppState {
        connection_status: Arc<Mutex<ConnectionStatus>>,
        model_status: Arc<Mutex<ModelStatus>>,
    }

    impl TestAppState {
        fn new() -> Self {
            Self {
                connection_status: Arc::new(Mutex::new(ConnectionStatus::Disconnected)),
                model_status: Arc::new(Mutex::new(ModelStatus::NoModel)),
            }
        }

        async fn get_all_status(&self) -> StatusSnapshot {
            let connection = self.connection_status.lock().await.clone();
            let model = self.model_status.lock().await.clone();
            StatusSnapshot { connection, model }
        }

        async fn set_connection_status(&self, status: ConnectionStatus) {
            let mut current = self.connection_status.lock().await;
            *current = status;
        }

        async fn set_model_status(&self, status: ModelStatus) {
            let mut current = self.model_status.lock().await;
            *current = status;
        }
    }

    #[tokio::test]
    async fn test_get_all_status_initial_state() {
        let state = TestAppState::new();
        let snapshot = state.get_all_status().await;

        assert!(matches!(
            snapshot.connection,
            ConnectionStatus::Disconnected
        ));
        assert!(matches!(snapshot.model, ModelStatus::NoModel));
    }

    #[tokio::test]
    async fn test_connection_status_lifecycle() {
        let state = TestAppState::new();

        // Initial state
        let snapshot = state.get_all_status().await;
        assert!(matches!(
            snapshot.connection,
            ConnectionStatus::Disconnected
        ));

        // Set to connecting
        state
            .set_connection_status(ConnectionStatus::Connecting)
            .await;
        let snapshot = state.get_all_status().await;
        assert!(matches!(snapshot.connection, ConnectionStatus::Connecting));

        // Set to connected
        state
            .set_connection_status(ConnectionStatus::Connected)
            .await;
        let snapshot = state.get_all_status().await;
        assert!(matches!(snapshot.connection, ConnectionStatus::Connected));

        // Set to error
        state
            .set_connection_status(ConnectionStatus::Error {
                message: "Test connection error".to_string(),
            })
            .await;
        let snapshot = state.get_all_status().await;
        match snapshot.connection {
            ConnectionStatus::Error { message } => {
                assert_eq!(message, "Test connection error");
            }
            _ => panic!("Expected Error status"),
        }

        // Back to disconnected
        state
            .set_connection_status(ConnectionStatus::Disconnected)
            .await;
        let snapshot = state.get_all_status().await;
        assert!(matches!(
            snapshot.connection,
            ConnectionStatus::Disconnected
        ));
    }

    #[tokio::test]
    async fn test_model_status_download_lifecycle() {
        let state = TestAppState::new();

        // Initial state
        let snapshot = state.get_all_status().await;
        assert!(matches!(snapshot.model, ModelStatus::NoModel));

        // Start downloading
        state
            .set_model_status(ModelStatus::Downloading {
                model_name: "test-model.gguf".to_string(),
                progress: 0,
            })
            .await;

        let snapshot = state.get_all_status().await;
        match snapshot.model {
            ModelStatus::Downloading {
                model_name,
                progress,
            } => {
                assert_eq!(model_name, "test-model.gguf");
                assert_eq!(progress, 0);
            }
            _ => panic!("Expected Downloading status"),
        }

        // Update progress
        state
            .set_model_status(ModelStatus::Downloading {
                model_name: "test-model.gguf".to_string(),
                progress: 50,
            })
            .await;

        let snapshot = state.get_all_status().await;
        match snapshot.model {
            ModelStatus::Downloading { progress, .. } => {
                assert_eq!(progress, 50);
            }
            _ => panic!("Expected Downloading status"),
        }

        // Complete download, start loading
        state
            .set_model_status(ModelStatus::Loading {
                model_name: "test-model.gguf".to_string(),
            })
            .await;

        let snapshot = state.get_all_status().await;
        match snapshot.model {
            ModelStatus::Loading { model_name } => {
                assert_eq!(model_name, "test-model.gguf");
            }
            _ => panic!("Expected Loading status"),
        }

        // Model ready
        state
            .set_model_status(ModelStatus::Ready {
                model_name: "test-model.gguf".to_string(),
            })
            .await;

        let snapshot = state.get_all_status().await;
        match snapshot.model {
            ModelStatus::Ready { model_name } => {
                assert_eq!(model_name, "test-model.gguf");
            }
            _ => panic!("Expected Ready status"),
        }
    }

    #[tokio::test]
    async fn test_model_status_error_handling() {
        let state = TestAppState::new();

        // Set downloading
        state
            .set_model_status(ModelStatus::Downloading {
                model_name: "test-model.gguf".to_string(),
                progress: 25,
            })
            .await;

        // Encounter error during download
        state
            .set_model_status(ModelStatus::Error {
                message: "Download failed: network error".to_string(),
            })
            .await;

        let snapshot = state.get_all_status().await;
        match snapshot.model {
            ModelStatus::Error { message } => {
                assert_eq!(message, "Download failed: network error");
            }
            _ => panic!("Expected Error status"),
        }
    }

    #[tokio::test]
    async fn test_concurrent_status_updates() {
        let state = TestAppState::new();

        // Update both statuses concurrently
        let state_clone1 = state.clone();
        let state_clone2 = state.clone();

        let handle1 = tokio::spawn(async move {
            state_clone1
                .set_connection_status(ConnectionStatus::Connected)
                .await;
        });

        let handle2 = tokio::spawn(async move {
            state_clone2
                .set_model_status(ModelStatus::Ready {
                    model_name: "model.gguf".to_string(),
                })
                .await;
        });

        handle1.await.unwrap();
        handle2.await.unwrap();

        // Verify both updates succeeded
        let snapshot = state.get_all_status().await;
        assert!(matches!(snapshot.connection, ConnectionStatus::Connected));
        match snapshot.model {
            ModelStatus::Ready { model_name } => {
                assert_eq!(model_name, "model.gguf");
            }
            _ => panic!("Expected Ready status"),
        }
    }

    #[tokio::test]
    async fn test_model_status_progress_increments() {
        let state = TestAppState::new();

        // Simulate incremental progress updates
        let progress_values = vec![0, 10, 25, 50, 75, 90, 100];

        for progress in progress_values {
            state
                .set_model_status(ModelStatus::Downloading {
                    model_name: "model.gguf".to_string(),
                    progress,
                })
                .await;

            let snapshot = state.get_all_status().await;
            match snapshot.model {
                ModelStatus::Downloading { progress: p, .. } => {
                    assert_eq!(p, progress);
                }
                _ => panic!("Expected Downloading status"),
            }
        }
    }

    #[tokio::test]
    async fn test_status_independence() {
        let state = TestAppState::new();

        // Connection status should not affect model status
        state
            .set_connection_status(ConnectionStatus::Connected)
            .await;
        let snapshot = state.get_all_status().await;
        assert!(matches!(snapshot.connection, ConnectionStatus::Connected));
        assert!(matches!(snapshot.model, ModelStatus::NoModel)); // Should still be NoModel

        // Model status should not affect connection status
        state
            .set_model_status(ModelStatus::Ready {
                model_name: "model.gguf".to_string(),
            })
            .await;
        let snapshot = state.get_all_status().await;
        assert!(matches!(snapshot.connection, ConnectionStatus::Connected)); // Should still be Connected
        match snapshot.model {
            ModelStatus::Ready { .. } => {}
            _ => panic!("Expected Ready status"),
        }
    }
}
