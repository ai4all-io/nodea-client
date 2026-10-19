use ai4all_tauri::status::{ConnectionStatus, ModelStatus, StatusSnapshot};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_status_serialization() {
        // Test Disconnected
        let status = ConnectionStatus::Disconnected;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#"{"type":"Disconnected"}"#);

        // Test Connecting
        let status = ConnectionStatus::Connecting;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#"{"type":"Connecting"}"#);

        // Test Connected
        let status = ConnectionStatus::Connected;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#"{"type":"Connected"}"#);

        // Test Error
        let status = ConnectionStatus::Error {
            message: "Connection failed".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""type":"Error""#));
        assert!(json.contains(r#""message":"Connection failed""#));
    }

    #[test]
    fn test_connection_status_deserialization() {
        // Test Disconnected
        let json = r#"{"type":"Disconnected"}"#;
        let status: ConnectionStatus = serde_json::from_str(json).unwrap();
        assert!(matches!(status, ConnectionStatus::Disconnected));

        // Test Connecting
        let json = r#"{"type":"Connecting"}"#;
        let status: ConnectionStatus = serde_json::from_str(json).unwrap();
        assert!(matches!(status, ConnectionStatus::Connecting));

        // Test Connected
        let json = r#"{"type":"Connected"}"#;
        let status: ConnectionStatus = serde_json::from_str(json).unwrap();
        assert!(matches!(status, ConnectionStatus::Connected));

        // Test Error
        let json = r#"{"type":"Error","message":"Connection failed"}"#;
        let status: ConnectionStatus = serde_json::from_str(json).unwrap();
        match status {
            ConnectionStatus::Error { message } => {
                assert_eq!(message, "Connection failed");
            }
            _ => panic!("Expected Error variant"),
        }
    }

    #[test]
    fn test_model_status_serialization() {
        // Test NoModel
        let status = ModelStatus::NoModel;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#"{"type":"NoModel"}"#);

        // Test Downloading
        let status = ModelStatus::Downloading {
            model_name: "test-model.gguf".to_string(),
            progress: 50,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""type":"Downloading""#));
        assert!(json.contains(r#""model_name":"test-model.gguf""#));
        assert!(json.contains(r#""progress":50"#));

        // Test Loading
        let status = ModelStatus::Loading {
            model_name: "test-model.gguf".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""type":"Loading""#));
        assert!(json.contains(r#""model_name":"test-model.gguf""#));

        // Test Ready
        let status = ModelStatus::Ready {
            model_name: "test-model.gguf".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""type":"Ready""#));
        assert!(json.contains(r#""model_name":"test-model.gguf""#));

        // Test Error
        let status = ModelStatus::Error {
            message: "Failed to load".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""type":"Error""#));
        assert!(json.contains(r#""message":"Failed to load""#));
    }

    #[test]
    fn test_model_status_deserialization() {
        // Test NoModel
        let json = r#"{"type":"NoModel"}"#;
        let status: ModelStatus = serde_json::from_str(json).unwrap();
        assert!(matches!(status, ModelStatus::NoModel));

        // Test Downloading
        let json = r#"{"type":"Downloading","model_name":"test.gguf","progress":75}"#;
        let status: ModelStatus = serde_json::from_str(json).unwrap();
        match status {
            ModelStatus::Downloading {
                model_name,
                progress,
            } => {
                assert_eq!(model_name, "test.gguf");
                assert_eq!(progress, 75);
            }
            _ => panic!("Expected Downloading variant"),
        }

        // Test Loading
        let json = r#"{"type":"Loading","model_name":"test.gguf"}"#;
        let status: ModelStatus = serde_json::from_str(json).unwrap();
        match status {
            ModelStatus::Loading { model_name } => {
                assert_eq!(model_name, "test.gguf");
            }
            _ => panic!("Expected Loading variant"),
        }

        // Test Ready
        let json = r#"{"type":"Ready","model_name":"test.gguf"}"#;
        let status: ModelStatus = serde_json::from_str(json).unwrap();
        match status {
            ModelStatus::Ready { model_name } => {
                assert_eq!(model_name, "test.gguf");
            }
            _ => panic!("Expected Ready variant"),
        }

        // Test Error
        let json = r#"{"type":"Error","message":"Load failed"}"#;
        let status: ModelStatus = serde_json::from_str(json).unwrap();
        match status {
            ModelStatus::Error { message } => {
                assert_eq!(message, "Load failed");
            }
            _ => panic!("Expected Error variant"),
        }
    }

    #[test]
    fn test_status_snapshot_serialization() {
        let snapshot = StatusSnapshot {
            connection: ConnectionStatus::Connected,
            model: ModelStatus::Ready {
                model_name: "llama-3.2.gguf".to_string(),
            },
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains(r#""connection""#));
        assert!(json.contains(r#""model""#));
        assert!(json.contains(r#""type":"Connected""#));
        assert!(json.contains(r#""type":"Ready""#));
        assert!(json.contains(r#""model_name":"llama-3.2.gguf""#));
    }

    #[test]
    fn test_status_snapshot_deserialization() {
        let json = r#"{
            "connection": {"type":"Connected"},
            "model": {"type":"Downloading","model_name":"test.gguf","progress":42}
        }"#;

        let snapshot: StatusSnapshot = serde_json::from_str(json).unwrap();
        assert!(matches!(snapshot.connection, ConnectionStatus::Connected));
        match snapshot.model {
            ModelStatus::Downloading {
                model_name,
                progress,
            } => {
                assert_eq!(model_name, "test.gguf");
                assert_eq!(progress, 42);
            }
            _ => panic!("Expected Downloading variant"),
        }
    }

    #[test]
    fn test_model_status_progress_bounds() {
        // Progress should be u8, so max is 255, but we expect 0-100 in practice
        let status = ModelStatus::Downloading {
            model_name: "model.gguf".to_string(),
            progress: 0,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""progress":0"#));

        let status = ModelStatus::Downloading {
            model_name: "model.gguf".to_string(),
            progress: 100,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""progress":100"#));
    }
}
