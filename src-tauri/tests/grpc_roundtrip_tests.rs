//! The node and the backend, talking over a real gRPC connection.
//!
//! A real `tonic` server implementing `Ai4AllService` stands in for the
//! backend, a real `tonic` client for the node, and every assertion is made on
//! messages that actually crossed the wire. This replaces
//! `tests/grpc_integration_test.rs`, where every meaningful test was
//! `#[ignore]`d with a `println!("TODO: …")` body.
//!
//! Why this is worth having on top of the unit tests: protobuf encoding is
//! where `optional` scalars, `map` fields and `oneof` members are genuinely
//! easy to get wrong, and a field that fails to survive the trip looks
//! identical to one the node chose to ignore. Everything asserted here is
//! checked *after* a serialize/deserialize round trip, not on a struct built
//! in-process.
//!
//! What it deliberately cannot assert: that generation honours the fields it
//! receives. That needs a loaded model, and macOS cannot create a Metal device
//! from `cargo test` - see `examples/real_repro_tauri.rs`.

use ai4all_tauri::proto::ai4_all_service_client::Ai4AllServiceClient;
use ai4all_tauri::proto::ai4_all_service_server::{Ai4AllService, Ai4AllServiceServer};
use ai4all_tauri::proto::{
    stream_events_request, stream_events_response, CancelGeneration, ChatCompletionRequest,
    ChatMessage, DeviceIdentity, GeneratedText, Heartbeat, LoadModel, StreamEventsRequest,
    StreamEventsResponse, UnloadModel, Usage,
};
use ai4all_tauri::protocol::{classify, ServerAction};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};

/// A backend that records everything the node sent it, and replies with
/// whatever the test queued up.
#[derive(Clone, Default)]
struct MockBackend {
    received: Arc<Mutex<Vec<StreamEventsRequest>>>,
    to_send: Arc<Mutex<Vec<StreamEventsResponse>>>,
}

#[tonic::async_trait]
impl Ai4AllService for MockBackend {
    type StreamEventsStream = ReceiverStream<Result<StreamEventsResponse, Status>>;

    async fn stream_events(
        &self,
        request: Request<Streaming<StreamEventsRequest>>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let mut inbound = request.into_inner();
        let received = self.received.clone();
        let queued = self.to_send.lock().await.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(16);

        tokio::spawn(async move {
            for message in queued {
                if tx.send(Ok(message)).await.is_err() {
                    return;
                }
            }
            while let Some(Ok(message)) = inbound.next().await {
                received.lock().await.push(message);
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

/// Start the mock backend on an ephemeral port and connect a client to it.
async fn connect(
    backend: MockBackend,
) -> (
    Ai4AllServiceClient<tonic::transport::Channel>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        Server::builder()
            .add_service(Ai4AllServiceServer::new(backend))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });

    // The server needs a moment to start accepting.
    let client = loop {
        match Ai4AllServiceClient::connect(format!("http://{addr}")).await {
            Ok(client) => break client,
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    };

    (client, handle)
}

const TOOLS: &str = r#"[{"type":"function","function":{"name":"get_weather"}}]"#;
const SCHEMA: &str = r#"{"type":"json_schema","json_schema":{"name":"a",
    "schema":{"type":"object"}}}"#;

fn fully_populated_request() -> ChatCompletionRequest {
    ChatCompletionRequest {
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: "Be terse.".to_string(),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "tool".to_string(),
                content: "sunny".to_string(),
                name: Some("weather".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
            },
        ],
        temperature: Some(0.7),
        top_p: Some(0.9),
        max_tokens: Some(64),
        seed: Some(42),
        stop: vec!["END".to_string()],
        frequency_penalty: Some(0.5),
        presence_penalty: Some(-0.5),
        response_format: Some(SCHEMA.to_string()),
        tools: Some(TOOLS.to_string()),
        tool_choice: Some(r#""auto""#.to_string()),
        logprobs: Some(true),
        top_logprobs: Some(5),
        n: Some(3),
        logit_bias: HashMap::from([(1234u32, 5.0f32)]),
        web_search_options: Some(r#"{"search_context_size":"low"}"#.to_string()),
        reasoning_effort: Some("high".to_string()),
    }
}

#[tokio::test]
async fn a_chat_completion_request_survives_the_wire_with_every_field_intact() {
    let backend = MockBackend::default();
    backend.to_send.lock().await.push(StreamEventsResponse {
        request_id: "req-1".to_string(),
        msg: Some(stream_events_response::Msg::ChatCompletionRequest(
            fully_populated_request(),
        )),
    });

    let (mut client, _server) = connect(backend).await;
    let (_tx, rx) = tokio::sync::mpsc::channel::<StreamEventsRequest>(1);
    let mut inbound = client
        .stream_events(ReceiverStream::new(rx))
        .await
        .unwrap()
        .into_inner();

    let message = inbound.next().await.unwrap().unwrap();

    // Classified exactly as the node's own handle_server_message does.
    match classify(message) {
        ServerAction::Generate {
            request_id,
            turns,
            options,
        } => {
            assert_eq!(request_id, "req-1");

            // Messages, including the fields that only exist for tool calls.
            assert_eq!(turns.len(), 2);
            assert_eq!(turns[0].role, "system");
            assert_eq!(turns[1].tool_call_id.as_deref(), Some("call_1"));

            // Every optional scalar. proto3 `optional` is a presence wrapper,
            // so "unset" and "set to the zero value" are different on the wire -
            // this is where that goes wrong silently.
            assert!((options.temperature.unwrap() - 0.7).abs() < 1e-6);
            assert!((options.top_p.unwrap() - 0.9).abs() < 1e-6);
            assert_eq!(options.max_tokens, Some(64));
            assert_eq!(options.stop, vec!["END".to_string()]);
            assert_eq!(options.frequency_penalty, Some(0.5));
            assert_eq!(options.presence_penalty, Some(-0.5));
            assert_eq!(options.top_logprobs, Some(5));
            assert_eq!(options.n, Some(3));
            assert!(options.logprobs);
            assert_eq!(options.reasoning_effort.as_deref(), Some("high"));

            // The JSON-carrying fields.
            assert!(options.response_format.is_some());
            assert!(options.tools.is_some());
            assert!(options.tool_choice.is_some());
            assert!(options.web_search_options.is_some());

            // A protobuf `map`, which encodes quite differently from the rest.
            assert_eq!(options.logit_bias, HashMap::from([(1234u32, 5.0f32)]));
        }
        other => panic!("expected a Generate action, got {other:?}"),
    }
}

#[tokio::test]
async fn an_empty_request_arrives_with_everything_still_unset() {
    // The inverse, and the one proto3 `optional` actually protects: an unset
    // field must not arrive as its zero value, or every caller silently gets
    // temperature 0 (greedy) and max_tokens 0.
    let backend = MockBackend::default();
    backend.to_send.lock().await.push(StreamEventsResponse {
        request_id: "req-2".to_string(),
        msg: Some(stream_events_response::Msg::ChatCompletionRequest(
            ChatCompletionRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "Hi".to_string(),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                }],
                ..Default::default()
            },
        )),
    });

    let (mut client, _server) = connect(backend).await;
    let (_tx, rx) = tokio::sync::mpsc::channel::<StreamEventsRequest>(1);
    let mut inbound = client
        .stream_events(ReceiverStream::new(rx))
        .await
        .unwrap()
        .into_inner();

    match classify(inbound.next().await.unwrap().unwrap()) {
        ServerAction::Generate { options, .. } => {
            assert_eq!(options.temperature, None, "not 0.0, which would be greedy");
            assert_eq!(
                options.max_tokens, None,
                "not 0, which would generate nothing"
            );
            assert_eq!(options.n, None);
            assert_eq!(options.top_logprobs, None);
            assert!(!options.logprobs);
            assert!(options.logit_bias.is_empty());
            assert!(options.response_format.is_none());
        }
        other => panic!("expected a Generate action, got {other:?}"),
    }
}

#[tokio::test]
async fn load_model_unload_and_cancel_are_recognised_over_the_wire() {
    let backend = MockBackend::default();
    backend.to_send.lock().await.extend([
        StreamEventsResponse {
            request_id: String::new(),
            msg: Some(stream_events_response::Msg::LoadModel(LoadModel {
                model_name: "Qwen2.5-Coder-7B-Instruct-Q4_K_M.gguf".to_string(),
                model_ref: "bartowski/Qwen2.5-Coder-7B-Instruct-GGUF".to_string(),
                model_key: "qwen-2.5-coder-7b".to_string(),
            })),
        },
        StreamEventsResponse {
            request_id: "req-3".to_string(),
            msg: Some(stream_events_response::Msg::CancelGeneration(
                CancelGeneration {},
            )),
        },
        StreamEventsResponse {
            request_id: String::new(),
            msg: Some(stream_events_response::Msg::UnloadModel(UnloadModel {})),
        },
        // A message with no `msg` at all: a node that dropped its connection
        // over this would be trivially bricked by a backend deploy.
        StreamEventsResponse {
            request_id: "req-4".to_string(),
            msg: None,
        },
    ]);

    let (mut client, _server) = connect(backend).await;
    let (_tx, rx) = tokio::sync::mpsc::channel::<StreamEventsRequest>(1);
    let mut inbound = client
        .stream_events(ReceiverStream::new(rx))
        .await
        .unwrap()
        .into_inner();

    let mut actions = Vec::new();
    for _ in 0..4 {
        actions.push(classify(inbound.next().await.unwrap().unwrap()));
    }

    assert_eq!(
        actions,
        vec![
            // model_ref is the Hugging Face repo to download from; model_key
            // is what every report back names. They are different strings and
            // neither can be derived from the other.
            ServerAction::LoadModel {
                model_name: "Qwen2.5-Coder-7B-Instruct-Q4_K_M.gguf".to_string(),
                model_ref: "bartowski/Qwen2.5-Coder-7B-Instruct-GGUF".to_string(),
                model_key: "qwen-2.5-coder-7b".to_string(),
            },
            ServerAction::Cancel {
                request_id: "req-3".to_string()
            },
            ServerAction::Unload,
            ServerAction::Malformed {
                request_id: "req-4".to_string()
            },
        ]
    );
}

#[tokio::test]
async fn the_device_identity_handshake_carries_capabilities_version_and_memory() {
    // B1: capability strings are the feature-gating mechanism, because every
    // build reports app_version 0.1.0 and a version compare is a no-op. If
    // these do not reach the backend, every capability-gated field is rejected
    // for every request.
    let backend = MockBackend::default();
    let (mut client, _server) = connect(backend.clone()).await;

    let (tx, rx) = tokio::sync::mpsc::channel::<StreamEventsRequest>(4);
    tx.send(StreamEventsRequest {
        msg: Some(stream_events_request::Msg::DeviceIdentity(DeviceIdentity {
            device_id: "device-1".to_string(),
            device_type: "macos-aarch64".to_string(),
            device_name: "Test".to_string(),
            device_model: "macos Desktop".to_string(),
            free_memory_mb: 16384,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            capabilities: ai4all_tauri::node_capabilities(),
            total_memory_mb: 16384,
            available_memory_mb: 9000,
            accelerator_memory_mb: Some(10922),
            loaded_model_ref: None,
            cached_model_refs: Vec::new(),
        })),
        user_id: String::new(),
        device_id: "device-1".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEventsRequest {
        msg: Some(stream_events_request::Msg::Heartbeat(Heartbeat {})),
        user_id: String::new(),
        device_id: "device-1".to_string(),
    })
    .await
    .unwrap();

    let _inbound = client.stream_events(ReceiverStream::new(rx)).await.unwrap();
    wait_for(&backend, 2).await;

    let received = backend.received.lock().await;
    let identity = match &received[0].msg {
        Some(stream_events_request::Msg::DeviceIdentity(identity)) => identity,
        other => panic!("expected DeviceIdentity first, got {other:?}"),
    };

    assert_eq!(identity.app_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(identity.total_memory_mb, 16384);
    assert_eq!(identity.available_memory_mb, 9000);
    assert_eq!(identity.accelerator_memory_mb, Some(10922));
    for capability in ai4all_tauri::node_capabilities() {
        assert!(
            identity.capabilities.contains(&capability),
            "{capability} did not survive the wire",
        );
    }
    assert!(matches!(
        received[1].msg,
        Some(stream_events_request::Msg::Heartbeat(_))
    ));
}

#[tokio::test]
async fn a_terminal_generated_text_carries_all_its_metadata_back() {
    // B3/C2/C3/C4: usage, finish_reason, tool_calls, logprobs and choice_index
    // all ride the terminal token rather than a separate message, so the
    // backend never has to correlate two. If any of them fails to survive, the
    // response silently loses its token counts or its tool call.
    let backend = MockBackend::default();
    let (mut client, _server) = connect(backend.clone()).await;

    let (tx, rx) = tokio::sync::mpsc::channel::<StreamEventsRequest>(4);
    tx.send(StreamEventsRequest {
        msg: Some(stream_events_request::Msg::GeneratedText(GeneratedText {
            request_id: "req-5".to_string(),
            text: "Hello".to_string(),
            last: true,
            usage: Some(Usage {
                prompt_tokens: 11,
                completion_tokens: 7,
            }),
            finish_reason: Some("length".to_string()),
            error: None,
            tool_calls: Some(r#"[{"id":"call_1"}]"#.to_string()),
            logprobs: Some(r#"{"token":"Hello"}"#.to_string()),
            choice_index: Some(2),
        })),
        user_id: String::new(),
        device_id: "device-1".to_string(),
    })
    .await
    .unwrap();

    let _inbound = client.stream_events(ReceiverStream::new(rx)).await.unwrap();
    wait_for(&backend, 1).await;

    let received = backend.received.lock().await;
    let generated = match &received[0].msg {
        Some(stream_events_request::Msg::GeneratedText(generated)) => generated,
        other => panic!("expected GeneratedText, got {other:?}"),
    };

    assert_eq!(
        generated.usage,
        Some(Usage {
            prompt_tokens: 11,
            completion_tokens: 7
        })
    );
    assert_eq!(generated.finish_reason.as_deref(), Some("length"));
    assert_eq!(
        generated.tool_calls.as_deref(),
        Some(r#"[{"id":"call_1"}]"#)
    );
    assert_eq!(generated.logprobs.as_deref(), Some(r#"{"token":"Hello"}"#));
    assert_eq!(generated.choice_index, Some(2));
    assert!(generated.error.is_none());
}

#[tokio::test]
async fn a_generation_failure_reaches_the_backend_as_a_terminal_error() {
    // Before B3 this was logged on the node and dropped, so the backend waited
    // out its whole first-token budget and the caller got an empty HTTP 200.
    let backend = MockBackend::default();
    let (mut client, _server) = connect(backend.clone()).await;

    let (tx, rx) = tokio::sync::mpsc::channel::<StreamEventsRequest>(2);
    tx.send(StreamEventsRequest {
        msg: Some(stream_events_request::Msg::GeneratedText(GeneratedText {
            request_id: "req-6".to_string(),
            text: String::new(),
            last: true,
            error: Some("Model not loaded".to_string()),
            ..Default::default()
        })),
        user_id: String::new(),
        device_id: "device-1".to_string(),
    })
    .await
    .unwrap();

    let _inbound = client.stream_events(ReceiverStream::new(rx)).await.unwrap();
    wait_for(&backend, 1).await;

    let received = backend.received.lock().await;
    match &received[0].msg {
        Some(stream_events_request::Msg::GeneratedText(generated)) => {
            assert_eq!(generated.error.as_deref(), Some("Model not loaded"));
            assert!(generated.last, "a failure is terminal");
            assert!(generated.text.is_empty());
            assert!(generated.usage.is_none());
        }
        other => panic!("expected GeneratedText, got {other:?}"),
    }
}

/// Wait until the backend has recorded `count` messages.
async fn wait_for(backend: &MockBackend, count: usize) {
    for _ in 0..100 {
        if backend.received.lock().await.len() >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "backend only ever received {} of {count} messages",
        backend.received.lock().await.len()
    );
}
