use crate::model_manager;
use crate::proto::ai4_all_service_client::Ai4AllServiceClient;
use crate::proto::{
    stream_events_request, DeviceIdentity, Heartbeat, ModelLoadStatus, StreamEventsRequest,
};
use crate::protocol::{self, ServerAction};
use crate::reconnect::{
    attempt_after_failure, graceful_close_outcome, is_auth_error, is_device_limit_error,
    retry_delay_secs, ConnectError, KEEPALIVE_INTERVAL_SECS, KEEPALIVE_TIMEOUT_SECS,
};
use crate::status;
use crate::{set_connection_status, set_model_status, AppState, MessageLogEntry};
use ai4all_tauri::catalogue;
use ai4all_tauri::reload::{self, ReloadEvent, ReloadTimeouts};
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Manager;
use tokio::sync::Mutex;
use tokio::time::{interval, sleep};
use tokio_util::sync::CancellationToken;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, ClientTlsConfig};
use tonic::{Request, Status};

/// Whether connect_with_retry should stop instead of continuing its backoff loop.
fn should_skip_retry(current_status: &status::ConnectionStatus) -> bool {
    matches!(
        current_status,
        status::ConnectionStatus::DeviceLimitReached { .. }
            | status::ConnectionStatus::Refused { .. }
    )
}

/// Check if debug logging is enabled via GRPC_DEBUG environment variable
fn is_debug_enabled() -> bool {
    std::env::var("GRPC_DEBUG")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

/// Debug log helper - only logs if GRPC_DEBUG=1
fn debug_log(message: &str) {
    if is_debug_enabled() {
        println!("[GRPC_DEBUG] {}", message);
    }
}

fn get_grpc_server() -> String {
    let compiled = option_env!("GRPC_SERVER")
        .unwrap_or("http://localhost:50051")
        .to_string();
    // Debug builds may point at another server at runtime. A release build
    // never does: the access token goes to whatever host this returns, so an
    // environment variable must not be able to redirect it.
    #[cfg(debug_assertions)]
    if let Ok(server) = std::env::var("GRPC_SERVER") {
        return server;
    }
    compiled
}

/// Log connection error to Dev Logs and console
async fn log_connection_error(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    error_type: &str,
    error_message: &str,
) {
    let full_message = format!("{}: {}", error_type, error_message);
    eprintln!("Connection Error - {}", full_message);

    log_message(
        &state.message_log,
        app_handle,
        "ConnectionError",
        &full_message,
    )
    .await;
}

/// Validate connection requirements before attempting to connect
fn validate_connection_requirements(access_token: &str, device_id: &str) -> Result<(), String> {
    // Check access token is present and not empty
    if access_token.is_empty() {
        return Err("Access token is empty".to_string());
    }

    // Basic JWT format validation (should have 3 parts separated by dots)
    let parts: Vec<&str> = access_token.split('.').collect();
    if parts.len() != 3 {
        return Err("Access token format is invalid (not a valid JWT)".to_string());
    }

    // Check device ID is present and not empty
    if device_id.is_empty() {
        return Err("Device ID is empty".to_string());
    }

    Ok(())
}

/// Connect, and keep reconnecting until told to stop or told it is pointless.
pub async fn connect_with_retry(
    device_id: String,
    state: AppState,
    app_handle: tauri::AppHandle,
    server_url: Option<String>,
    cancel_token: CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut attempt = 0;
    // One refresh per run of consecutive refusals, reset by any other outcome.
    // Without the reset a node reconnecting hours later gets no refresh at all;
    // without the flag a backend that refuses every token loops on refreshing.
    let mut refreshed = false;

    loop {
        if cancel_token.is_cancelled() {
            println!(
                "Connection cancelled before attempt {} started",
                attempt + 1
            );
            return Ok(());
        }

        attempt += 1;

        // Read the token per attempt, not once at the top. It used to be taken
        // by value and cloned into every attempt, so a refresh - by this loop
        // or anywhere else - could never reach it, and a node whose stream
        // dropped after its token expired retried the same dead credential
        // until the 60s backoff cap, forever.
        let access_token = match state.access_token.lock().await.clone() {
            Some(token) => token,
            None => {
                println!("No access token; nothing to connect with");
                return Err("Not authenticated".into());
            }
        };

        println!("Connection attempt {}", attempt);
        debug_log(&format!("Starting connection attempt {}", attempt));

        // Attempt connection and immediately reduce the error to owned data
        // for Send safety.
        let mut established = false;
        let (error_msg, terminal, unauthenticated) = {
            let result = connect_and_stream_with_server(
                access_token,
                device_id.clone(),
                state.clone(),
                app_handle.clone(),
                server_url.clone(),
                cancel_token.clone(),
                &mut established,
            )
            .await;

            match result {
                Ok(_) => {
                    println!("Connection closed cleanly on attempt {}", attempt);
                    return Ok(());
                }
                Err(e) => {
                    let msg = e.message().to_string();
                    println!("Connection failed on attempt {}: {}", attempt, msg);
                    debug_log(&format!("Connection attempt {} failed: {}", attempt, msg));
                    (msg, e.is_terminal(), e.is_unauthenticated())
                }
            }
        };

        // Refused credentials: fetch a new token and try again at once. Only a
        // refresh that *fails* is the user's problem.
        if unauthenticated {
            if !refreshed {
                refreshed = true;
                if crate::refresh_access_token_into_state(&state, &app_handle)
                    .await
                    .is_some()
                {
                    log_message(
                        &state.message_log,
                        &app_handle,
                        "Authentication",
                        "Access token refreshed; reconnecting",
                    )
                    .await;
                    continue;
                }
            }
            println!("Not retrying - {} (token refresh did not help)", error_msg);
            return Err(error_msg.into());
        }
        refreshed = false;

        // A device-limit rejection is terminal - another attempt presents the
        // same plan.
        {
            let current_status = state.connection_status.lock().await;
            let at_device_limit = should_skip_retry(&current_status);
            drop(current_status);

            if terminal || at_device_limit {
                println!("Not retrying - {}", error_msg);
                return Err(error_msg.into());
            }
        }

        attempt = attempt_after_failure(attempt, established);
        let delay_secs = retry_delay_secs(attempt);

        log_message(
            &state.message_log,
            &app_handle,
            "ConnectionRetry",
            &format!(
                "Retrying in {} seconds (attempt {})",
                delay_secs,
                attempt + 1
            ),
        )
        .await;

        set_connection_status(&state, &app_handle, status::ConnectionStatus::Connecting).await;

        println!("Waiting {} seconds before retry...", delay_secs);

        // Wait before retrying, bailing out immediately (not just on the
        // next 1s tick) if the user disconnects during the backoff.
        for i in 0..delay_secs {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    println!("Retry cancelled - connection was manually disconnected");
                    return Ok(());
                }
                _ = sleep(Duration::from_secs(1)) => {}
            }

            // Log countdown for visibility
            if i < delay_secs - 1 {
                let remaining = delay_secs - i - 1;
                if remaining > 0 && (remaining <= 5 || remaining.is_multiple_of(5)) {
                    println!("Retrying in {} seconds...", remaining);
                }
            }
        }
    }
}

pub async fn connect_and_stream(
    device_id: String,
    state: AppState,
    app_handle: tauri::AppHandle,
    cancel_token: CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    connect_with_retry(
        device_id,
        state,
        app_handle,
        None, // Use default server
        cancel_token,
    )
    .await
}

/// Internal function that accepts optional server URL for testing
pub async fn connect_and_stream_with_server(
    access_token: String,
    device_id: String,
    state: AppState,
    app_handle: tauri::AppHandle,
    server_url: Option<String>, // Allow injecting server URL for tests
    cancel_token: CancellationToken,
    // Set once the stream is up, so the retry loop can tell a connection that
    // was established and then closed from one that never got there.
    established: &mut bool,
) -> Result<(), ConnectError> {
    let grpc_server = server_url.unwrap_or_else(get_grpc_server);
    println!("Connecting to gRPC server: {}", grpc_server);

    // Validate connection requirements before attempting to connect
    if let Err(e) = validate_connection_requirements(&access_token, &device_id) {
        log_connection_error(&state, &app_handle, "Validation Failed", &e).await;
        set_connection_status(
            &state,
            &app_handle,
            status::ConnectionStatus::Error {
                message: format!("Validation failed: {}", e),
            },
        )
        .await;
        return Err(ConnectError::Terminal(e));
    }

    // Set connecting status
    set_connection_status(&state, &app_handle, status::ConnectionStatus::Connecting).await;

    // Attempt to connect with error handling
    let channel = match Channel::from_shared(grpc_server.clone()) {
        Ok(endpoint) => {
            // https:// endpoints (e.g. a remote deployment) need an explicit TLS
            // config - tonic doesn't infer TLS from the URI scheme on its own.
            let endpoint = if grpc_server.starts_with("https://") {
                match endpoint.tls_config(ClientTlsConfig::new()) {
                    Ok(endpoint) => endpoint,
                    Err(e) => {
                        let error_msg = format!("Invalid TLS config for {}: {}", grpc_server, e);
                        log_connection_error(&state, &app_handle, "Invalid TLS Config", &error_msg)
                            .await;
                        set_connection_status(
                            &state,
                            &app_handle,
                            status::ConnectionStatus::Error {
                                message: error_msg.clone(),
                            },
                        )
                        .await;
                        return Err(ConnectError::Terminal(error_msg));
                    }
                }
            } else {
                endpoint
            };

            // Keepalive pings keep a half-open connection from parking
            // forever in a read: without them a laptop that loses its network
            // (or a backend machine replaced mid-deploy) leaves the stream
            // looking alive to the client while no bytes will ever arrive.
            let endpoint = endpoint
                .http2_keep_alive_interval(Duration::from_secs(KEEPALIVE_INTERVAL_SECS))
                .keep_alive_timeout(Duration::from_secs(KEEPALIVE_TIMEOUT_SECS))
                .keep_alive_while_idle(true);

            match endpoint.connect().await {
                Ok(channel) => channel,
                Err(e) => {
                    let error_msg = format!("Failed to connect to {}: {}", grpc_server, e);
                    log_connection_error(&state, &app_handle, "Connection Failed", &error_msg)
                        .await;
                    set_connection_status(
                        &state,
                        &app_handle,
                        status::ConnectionStatus::Error {
                            message: error_msg.clone(),
                        },
                    )
                    .await;
                    return Err(ConnectError::Retryable(error_msg));
                }
            }
        }
        Err(e) => {
            let error_msg = format!("Invalid gRPC server URL {}: {}", grpc_server, e);
            log_connection_error(&state, &app_handle, "Invalid URL", &error_msg).await;
            set_connection_status(
                &state,
                &app_handle,
                status::ConnectionStatus::Error {
                    message: error_msg.clone(),
                },
            )
            .await;
            return Err(ConnectError::Terminal(error_msg));
        }
    };

    let mut client = Ai4AllServiceClient::new(channel);

    // Get bidirectional stream
    let (tx, rx) = tokio::sync::mpsc::channel::<StreamEventsRequest>(100);

    // Prepare device identity message
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "Unknown".to_string());

    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let memory = ai4all_tauri::memory::probe();
    println!("Reporting memory: {:?}", memory);

    // What is already resident and on disk, as registry keys, so the backend
    // adopts a warm node instead of reloading it.
    let loaded_model_ref = state
        .model_manager
        .lock()
        .await
        .loaded_model_path()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .and_then(|name| catalogue::registry_key(&state.model_cache_dir, name));
    let cached_model_refs = catalogue::cached_model_refs(&state.model_cache_dir);
    println!(
        "Reporting loaded model {:?}, cached {:?}",
        loaded_model_ref, cached_model_refs
    );

    let device_identity = DeviceIdentity {
        device_id: device_id.clone(),
        device_type: format!("{}-{}", os, arch),
        device_name: hostname.clone(),
        device_model: format!("{} Desktop", os),
        // Deprecated field, still read by backends that predate
        // total_memory_mb. Total RAM is what macOS always sent here.
        free_memory_mb: memory.total_memory_mb,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: ai4all_tauri::node_capabilities(),
        total_memory_mb: memory.total_memory_mb,
        available_memory_mb: memory.available_memory_mb,
        accelerator_memory_mb: memory.accelerator_memory_mb,
        loaded_model_ref,
        cached_model_refs,
    };

    let identity_request = StreamEventsRequest {
        msg: Some(stream_events_request::Msg::DeviceIdentity(device_identity)),
        user_id: String::new(),
        device_id: device_id.to_string(),
    };

    println!("Sending DeviceIdentity request for device: {}", device_id);

    // Create a stream that starts with the device identity, then chains the receiver
    let receiver_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let outbound = stream::once(async move { identity_request }).chain(receiver_stream);

    let mut request = Request::new(outbound);
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", access_token))
            .map_err(|e| ConnectError::Terminal(format!("Invalid access token: {}", e)))?,
    );
    request.metadata_mut().insert(
        "device-id",
        MetadataValue::try_from(device_id.clone())
            .map_err(|e| ConnectError::Terminal(format!("Invalid device id: {}", e)))?,
    );

    println!("Initiating stream_events connection...");
    let response = match client.stream_events(request).await {
        Ok(response) => response,
        Err(e) if is_device_limit_error(&e) => {
            // The org has hit its plan's device limit - retrying won't help,
            // so this is surfaced as a distinct, non-retryable status.
            let error_msg = "You've reached the device limit for your plan. Contact hello@getnodea.com to increase it.".to_string();
            log_connection_error(&state, &app_handle, "Device Limit Reached", &error_msg).await;
            set_connection_status(
                &state,
                &app_handle,
                status::ConnectionStatus::DeviceLimitReached {
                    message: error_msg.clone(),
                },
            )
            .await;
            return Err(ConnectError::Terminal(error_msg));
        }
        Err(e) if is_auth_error(&e) => {
            // Refusing the *token we hold* is not the same as refusing the
            // user: access tokens are short-lived, so this is what a node that
            // was offline longer than one lifetime sees on every reconnect.
            // The retry loop refreshes and tries once more before giving up.
            let error_msg = format!("Authentication rejected by the server: {}", e.message());
            log_connection_error(&state, &app_handle, "Authentication Failed", &error_msg).await;
            set_connection_status(
                &state,
                &app_handle,
                status::ConnectionStatus::Refused {
                    message: e.message().to_string(),
                },
            )
            .await;
            return Err(ConnectError::Unauthenticated(error_msg));
        }
        Err(e) => {
            let error_msg = format!("Failed to establish stream: {}", e);
            log_connection_error(&state, &app_handle, "Stream Failed", &error_msg).await;
            set_connection_status(
                &state,
                &app_handle,
                status::ConnectionStatus::Error {
                    message: error_msg.clone(),
                },
            )
            .await;
            return Err(ConnectError::Retryable(error_msg));
        }
    };
    let mut inbound = response.into_inner();
    println!("Stream connection established successfully");
    *established = true;

    // Set connected status
    set_connection_status(&state, &app_handle, status::ConnectionStatus::Connected).await;

    // Fired when the heartbeat task gives up. The receive loop watches it, so
    // a dead heartbeat unwinds the connection into the retry loop instead of
    // parking the app in Error with nothing left to reconnect it.
    let heartbeat_failed = CancellationToken::new();

    // This connection's lifetime, cancelled on every path out of this function
    // by the drop guard. The heartbeat used to watch only the global
    // `cancel_token` - which only a user disconnect or shutdown cancels - so
    // after a server-side close (a deploy, a revoked session) it outlived its
    // stream. Two failures followed from that one cause: it wrote Error over
    // the *next* connection's Connected about 15s later (#241), and each
    // reconnect left another orphan writing to a closed channel whose "3
    // consecutive failures" tore down the new connection as fast as it was
    // made, so a backend restart could park the node in a permanent reconnect
    // loop in which it never heartbeated and served nothing. A child token
    // keeps user-requested cancellation propagating.
    let connection_scope = cancel_token.child_token();
    let _connection_guard = connection_scope.clone().drop_guard();

    let heartbeat_failed_signal = heartbeat_failed.clone();
    let heartbeat_scope = connection_scope.clone();
    let tx_heartbeat = tx.clone();
    let device_id_heartbeat = device_id.clone();
    let state_heartbeat = state.clone();
    let app_handle_heartbeat = app_handle.clone();
    tokio::spawn(async move {
        let end = run_heartbeat(
            heartbeat_scope.clone(),
            Duration::from_secs(HEARTBEAT_INTERVAL_SECS),
            MAX_HEARTBEAT_FAILURES,
            || {
                let tx = tx_heartbeat.clone();
                let device_id = device_id_heartbeat.clone();
                async move {
                    send_heartbeat(&tx, &device_id)
                        .await
                        .map_err(|e| e.message().to_string())
                }
            },
            |failures, error| {
                let state = state_heartbeat.clone();
                let app_handle = app_handle_heartbeat.clone();
                async move {
                    let error_msg = format!(
                        "Heartbeat failed ({}/{}): {}",
                        failures, MAX_HEARTBEAT_FAILURES, error
                    );
                    log_connection_error(&state, &app_handle, "Heartbeat Failed", &error_msg).await;
                }
            },
        )
        .await;

        match end {
            HeartbeatEnd::Stopped => println!("Heartbeat task stopped with its connection"),
            HeartbeatEnd::GaveUp => {
                println!("Max consecutive heartbeat failures reached, tearing the connection down");
                set_connection_status(
                    &state_heartbeat,
                    &app_handle_heartbeat,
                    status::ConnectionStatus::Error {
                        message: format!(
                            "Connection lost after {} failed heartbeats",
                            MAX_HEARTBEAT_FAILURES
                        ),
                    },
                )
                .await;
                heartbeat_failed_signal.cancel();
            }
        }
    });

    // Log initial connection
    log_message(
        &state.message_log,
        &app_handle,
        "Connection",
        "Connected to gRPC server",
    )
    .await;

    // Track connection health metrics
    let connection_start = Instant::now();

    // Receive messages from server
    println!("Listening for messages from server...");
    debug_log("Starting message receive loop");
    let mut heartbeat_gave_up = false;
    let stream_ended_gracefully = loop {
        let next_message = tokio::select! {
            _ = cancel_token.cancelled() => {
                println!("Receive loop stopped - connection was disconnected");
                break true;
            }
            _ = heartbeat_failed.cancelled() => {
                println!("Receive loop stopped - the heartbeat task gave up");
                heartbeat_gave_up = true;
                break false;
            }
            msg = inbound.message() => msg,
        };
        match next_message {
            Ok(Some(message)) => {
                debug_log(&format!(
                    "Received message type: {:?}",
                    message.msg.as_ref().map(std::mem::discriminant)
                ));

                // Process message - errors in message handling don't crash the loop
                handle_server_message(message, &state, &app_handle, &tx, &device_id).await;
            }
            Ok(None) => {
                // Stream ended gracefully by server
                println!("Server stream ended gracefully");
                log_message(
                    &state.message_log,
                    &app_handle,
                    "Disconnection",
                    "Server closed connection gracefully",
                )
                .await;
                break true;
            }
            Err(e) => {
                // Stream ended with error - likely network issue or server crash
                let error_message = e.message().to_string();
                let error_kind = if error_message.contains("Connection reset")
                    || error_message.contains("Broken pipe")
                    || error_message.contains("Connection refused")
                {
                    "Network Error"
                } else if error_message.contains("deadline") || error_message.contains("timeout") {
                    "Timeout"
                } else {
                    "Stream Error"
                };

                let error_msg = format!("{}: {}", error_kind, error_message);
                log_connection_error(&state, &app_handle, error_kind, &error_message).await;

                set_connection_status(
                    &state,
                    &app_handle,
                    status::ConnectionStatus::Error {
                        message: error_msg.clone(),
                    },
                )
                .await;

                return Err(ConnectError::Retryable(error_msg));
            }
        }
    };

    // Log connection statistics
    let uptime_secs = connection_start.elapsed().as_secs();
    let uptime_msg = format!(
        "Connection closed after {} seconds ({} minutes)",
        uptime_secs,
        uptime_secs / 60
    );
    debug_log(&uptime_msg);
    log_message(
        &state.message_log,
        &app_handle,
        "ConnectionStats",
        &uptime_msg,
    )
    .await;

    if heartbeat_gave_up {
        return Err(ConnectError::Retryable(
            "Heartbeats stopped reaching the server".to_string(),
        ));
    }

    let cancelled = cancel_token.is_cancelled();

    // Only a user-requested disconnect is really "disconnected". A server-side
    // close is the start of a reconnect, and labelling it Disconnected would
    // tell the user the app is idle while it is about to come back.
    if stream_ended_gracefully && cancelled {
        set_connection_status(&state, &app_handle, status::ConnectionStatus::Disconnected).await;
    }

    graceful_close_outcome(cancelled)
}

const HEARTBEAT_INTERVAL_SECS: u64 = 5;
const MAX_HEARTBEAT_FAILURES: u32 = 3;

/// How a connection's heartbeat loop ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HeartbeatEnd {
    /// Its connection ended (or the user disconnected). Nothing to report.
    Stopped,
    /// `max_failures` consecutive sends failed while the connection was still
    /// current: the caller reports the connection lost and tears it down.
    GaveUp,
}

/// Send a heartbeat every `period` until `scope` (the connection's lifetime)
/// is cancelled or `max_failures` sends in a row fail.
///
/// A failure once the scope is cancelled is the connection ending, not the
/// heartbeat failing, so it is neither reported nor counted: only a
/// connection that is still current may give up.
pub(crate) async fn run_heartbeat<S, SF, L, LF>(
    scope: CancellationToken,
    period: Duration,
    max_failures: u32,
    mut send: S,
    mut on_failure: L,
) -> HeartbeatEnd
where
    S: FnMut() -> SF,
    SF: std::future::Future<Output = Result<(), String>>,
    L: FnMut(u32, String) -> LF,
    LF: std::future::Future<Output = ()>,
{
    let mut ticker = interval(period);
    let mut consecutive_failures = 0;

    loop {
        tokio::select! {
            _ = scope.cancelled() => return HeartbeatEnd::Stopped,
            _ = ticker.tick() => {}
        }

        match send().await {
            Ok(()) => consecutive_failures = 0,
            Err(error) => {
                if scope.is_cancelled() {
                    return HeartbeatEnd::Stopped;
                }
                consecutive_failures += 1;
                on_failure(consecutive_failures, error).await;
                if consecutive_failures >= max_failures {
                    return if scope.is_cancelled() {
                        HeartbeatEnd::Stopped
                    } else {
                        HeartbeatEnd::GaveUp
                    };
                }
            }
        }
    }
}

// tonic::Status is large; boxing it would only diverge from every other tonic API.
#[allow(clippy::result_large_err)]
async fn send_heartbeat(
    tx: &tokio::sync::mpsc::Sender<StreamEventsRequest>,
    device_id: &str,
) -> Result<(), Status> {
    debug_log(&format!("Sending heartbeat for device: {}", device_id));

    let request = StreamEventsRequest {
        msg: Some(stream_events_request::Msg::Heartbeat(Heartbeat {})),
        user_id: String::new(),
        device_id: device_id.to_string(),
    };

    // Don't log heartbeats to avoid spam (unless debug mode)
    tx.send(request).await.map_err(|e| {
        debug_log(&format!("Heartbeat send failed: {}", e));
        Status::internal(e.to_string())
    })?;

    debug_log("Heartbeat sent successfully");
    Ok(())
}

async fn handle_server_message(
    message: crate::proto::StreamEventsResponse,
    state: &AppState,
    app_handle: &tauri::AppHandle,
    tx: &tokio::sync::mpsc::Sender<StreamEventsRequest>,
    device_id: &str,
) {
    // The decision itself lives in `protocol::classify`, which is pure and
    // therefore testable over a real gRPC connection without a desktop app.
    match protocol::classify(message) {
        ServerAction::Generate {
            request_id,
            turns,
            options,
        } => {
            let content = format!("ChatCompletionRequest with {} messages", turns.len());
            log_message(
                &state.message_log,
                app_handle,
                "ChatCompletionRequest",
                &content,
            )
            .await;

            // Handle chat completion in a separate task
            let model_manager_clone = state.model_manager.clone();
            let app_handle_clone = app_handle.clone();
            let message_log_clone = state.message_log.clone();
            let tx_clone = tx.clone();
            let device_id_clone = device_id.to_string();
            let request_id_for_error = request_id.clone();

            // Registered before the task starts, so a cancel that arrives
            // while the model manager lock is still being acquired is not lost.
            let Some(cancel) = state.generations.begin(&request_id).await else {
                // Draining for a model switch: the model this was routed for
                // is about to be unloaded. Fail it now rather than run it on
                // whatever replaces it.
                let error = "Node is switching models; retry on another node";
                log_message(&state.message_log, app_handle, "ChatError", error).await;
                send_generation_error(tx, device_id, &request_id, error).await;
                return;
            };
            let generations = state.generations.clone();
            let request_id_for_cleanup = request_id.clone();

            tokio::spawn(async move {
                let result = handle_chat_completion(
                    &model_manager_clone,
                    &app_handle_clone,
                    &message_log_clone,
                    &tx_clone,
                    &device_id_clone,
                    &request_id,
                    turns,
                    *options,
                    &cancel,
                )
                .await;

                generations.finish(&request_id_for_cleanup).await;

                if let Err(e) = result {
                    eprintln!("Failed to handle chat completion: {}", e);
                    log_message(
                        &message_log_clone,
                        &app_handle_clone,
                        "ChatError",
                        &format!("Failed to generate response: {}", e),
                    )
                    .await;

                    // Tell the backend. Previously this was logged here and
                    // dropped, so the backend waited out its timeout and the
                    // caller got an empty HTTP 200 - the failure was invisible
                    // to everyone but whoever had the node's console open.
                    send_generation_error(&tx_clone, &device_id_clone, &request_id_for_error, &e)
                        .await;
                }
            });
        }
        ServerAction::Cancel { request_id } => {
            // The backend's client went away. Cancelling matters because
            // handle_chat_completion holds the model_manager lock for the whole
            // generation, so an abandoned request otherwise pins the node until
            // it finishes on its own.
            if state.generations.cancel(&request_id).await {
                log_message(
                    &state.message_log,
                    app_handle,
                    "CancelGeneration",
                    &format!("Cancelling generation for {}", request_id),
                )
                .await;
            } else {
                // Normal: the generation already finished, and the cancel
                // lost the race.
                debug_log(&format!(
                    "No in-flight generation to cancel for {}",
                    request_id
                ));
            }
        }
        ServerAction::LoadModel {
            model_name,
            model_ref,
            model_key,
        } => {
            let content = format!(
                "LoadModel: {} (ref: {}, key: {})",
                model_name, model_ref, model_key
            );
            log_message(&state.message_log, app_handle, "LoadModel", &content).await;

            // Handle model loading in a separate task
            let state_clone = state.clone();
            let app_handle_clone = app_handle.clone();
            let reporter = LoadStatusReporter {
                tx: tx.clone(),
                device_id: device_id.to_string(),
            };

            tokio::spawn(async move {
                if let Err(e) = handle_load_model(
                    &state_clone,
                    &app_handle_clone,
                    &reporter,
                    &model_name,
                    &model_ref,
                    &model_key,
                )
                .await
                {
                    eprintln!("Failed to load model: {}", e);
                    log_message(
                        &state_clone.message_log,
                        &app_handle_clone,
                        "ModelError",
                        &format!("Failed to load model: {}", e),
                    )
                    .await;
                }
            });
        }
        ServerAction::Unload => {
            // The backend does not send this yet; freeing the model lands with
            // the scheduler's apply step. Logged so it is visible if it does.
            log_message(
                &state.message_log,
                app_handle,
                "UnloadModel",
                "UnloadModel received; not handled by this build",
            )
            .await;
        }
        ServerAction::Malformed { request_id } => {
            // Log empty/malformed message but don't crash the connection
            let warning_msg = format!(
                "Received empty or malformed message (request_id: {})",
                request_id
            );
            log_message(
                &state.message_log,
                app_handle,
                "MalformedMessage",
                &warning_msg,
            )
            .await;
            eprintln!("Warning: {}", warning_msg);
        }
    }
}

/// Report a terminal generation failure to the backend.
///
/// Shaped as a GeneratedText with `last` set and `error` populated - the same
/// shape the backend's own timeout sentinel already produces - so no new
/// message type or correlation logic is needed.
async fn send_generation_error(
    tx: &tokio::sync::mpsc::Sender<StreamEventsRequest>,
    device_id: &str,
    request_id: &str,
    error: &str,
) {
    let request = StreamEventsRequest {
        msg: Some(stream_events_request::Msg::GeneratedText(
            crate::proto::GeneratedText {
                request_id: request_id.to_string(),
                text: String::new(),
                last: true,
                usage: None,
                finish_reason: None,
                error: Some(error.to_string()),
                tool_calls: None,
                logprobs: None,
                choice_index: None,
            },
        )),
        user_id: String::new(),
        device_id: device_id.to_string(),
    };

    if let Err(e) = tx.send(request).await {
        eprintln!("Could not report generation error to the backend: {}", e);
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_chat_completion(
    model_manager: &model_manager::SharedModelManager,
    app_handle: &tauri::AppHandle,
    message_log: &Arc<Mutex<Vec<MessageLogEntry>>>,
    tx: &tokio::sync::mpsc::Sender<StreamEventsRequest>,
    device_id: &str,
    request_id: &str,
    turns: Vec<model_manager::ChatTurn>,
    options: model_manager::GenerationOptions,
    cancel: &CancellationToken,
) -> Result<(), String> {
    if turns.is_empty() {
        return Err("No messages in chat request".to_string());
    }

    log_message(
        message_log,
        app_handle,
        "ChatGenerate",
        &describe_chat_request(&turns),
    )
    .await;

    // Get a lock on the model manager
    let model_mgr = model_manager.lock().await;

    // Check if model is loaded
    if !model_mgr.is_loaded() {
        return Err("Model not loaded".to_string());
    }

    // Prepare variables for the callback
    let tx_clone = tx.clone();
    let device_id_clone = device_id.to_string();
    let request_id_clone = request_id.to_string();

    // Generate response with streaming - NOTE: This is now async
    let outcome = model_mgr
        .generate_streaming(&turns, &options, cancel, move |update| {
            // Create the GeneratedText message. Terminal metadata rides the
            // terminal token, so the backend never has to correlate two messages.
            let generated_text = crate::proto::GeneratedText {
                request_id: request_id_clone.clone(),
                text: update.text.clone(),
                // `last` means "no more tokens for this request at all", not "this
                // choice finished" - with n > 1 the backend would otherwise stop
                // reading after the first completion to end.
                last: update.is_final,
                choice_index: Some(update.choice_index as u32),
                usage: update.usage.map(|usage| crate::proto::Usage {
                    prompt_tokens: usage.prompt_tokens,
                    completion_tokens: usage.completion_tokens,
                }),
                finish_reason: update.finish_reason.clone(),
                error: None,
                tool_calls: update.tool_calls.clone(),
                logprobs: update.logprobs.clone(),
            };

            // Create the StreamEventsRequest
            let request = StreamEventsRequest {
                msg: Some(stream_events_request::Msg::GeneratedText(generated_text)),
                user_id: String::new(),
                device_id: device_id_clone.clone(),
            };

            // Debug: Log token being sent
            println!("{}", describe_token_update(&request_id_clone, &update));

            // Send the message through the gRPC channel
            // Use block_in_place to safely block from within the runtime
            // SendError hands the whole unsent request back, hence its size.
            #[allow(clippy::result_large_err)]
            let send_result = tokio::task::block_in_place(|| tx_clone.blocking_send(request));

            match send_result {
                Ok(_) => println!("✓ Token sent successfully"),
                Err(ref e) => println!("✗ Failed to send token: {}", e),
            }

            send_result.map_err(|e| format!("Failed to send generated text: {}", e))?;

            Ok(())
        })
        .await?;

    // A cancellation is not a failure: the backend already gave up on this
    // request, so there is nobody to report anything to.
    let content = match outcome {
        model_manager::GenerationOutcome::Completed => "Response generation complete",
        model_manager::GenerationOutcome::Cancelled => "Generation cancelled by the backend",
    };

    log_message(message_log, app_handle, "ChatComplete", content).await;

    Ok(())
}

/// Sends ModelLoadStatus for one connection.
#[derive(Clone)]
struct LoadStatusReporter {
    tx: tokio::sync::mpsc::Sender<StreamEventsRequest>,
    device_id: String,
}

impl LoadStatusReporter {
    fn message(
        model_ref: &str,
        status: &str,
        progress_pct: Option<u32>,
        load_ms: Option<u32>,
        error: Option<String>,
        device_id: &str,
    ) -> StreamEventsRequest {
        StreamEventsRequest {
            msg: Some(stream_events_request::Msg::ModelLoadStatus(
                ModelLoadStatus {
                    model_ref: model_ref.to_string(),
                    status: status.to_string(),
                    progress_pct,
                    load_ms,
                    error,
                },
            )),
            user_id: String::new(),
            device_id: device_id.to_string(),
        }
    }

    async fn send(
        &self,
        model_ref: Option<&str>,
        status: &str,
        progress_pct: Option<u32>,
        load_ms: Option<u32>,
        error: Option<String>,
    ) {
        // The backend knows models only by registry key; a file outside the
        // catalogue has nothing it could match.
        let Some(model_ref) = model_ref else { return };
        let message = Self::message(
            model_ref,
            status,
            progress_pct,
            load_ms,
            error,
            &self.device_id,
        );
        if let Err(e) = self.tx.send(message).await {
            eprintln!("Could not report {} for {}: {}", status, model_ref, e);
        }
    }

    /// For the download's progress callback, which cannot await. A dropped
    /// progress update is harmless; the next one supersedes it.
    fn try_send_progress(&self, model_ref: Option<&str>, progress_pct: u32) {
        let Some(model_ref) = model_ref else { return };
        let message = Self::message(
            model_ref,
            "downloading",
            Some(progress_pct),
            None,
            None,
            &self.device_id,
        );
        let _ = self.tx.try_send(message);
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default()
        .to_string()
}

async fn handle_load_model(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    reporter: &LoadStatusReporter,
    model_name: &str,
    model_ref: &str,
    model_key: &str,
) -> Result<(), String> {
    let _switch = state.model_switch.lock().await;

    // What every ModelLoadStatus names: the registry key, never the filename.
    // Stated by the backend, never derived - and remembered, so this file can
    // still be named in cached_model_refs after a restart.
    let registry_key: Option<String> = (!model_key.is_empty()).then(|| model_key.to_string());

    // Both strings become a path and a URL below; refuse anything that is not
    // a plain file name and an owner/repo id before touching either.
    if let Err(e) = catalogue::validate_model_file_name(model_name)
        .and_then(|_| catalogue::validate_model_ref(model_ref))
    {
        set_model_status(
            state,
            app_handle,
            status::ModelStatus::Error { message: e.clone() },
        )
        .await;
        reporter
            .send(
                registry_key.as_deref(),
                "failed",
                None,
                None,
                Some(e.clone()),
            )
            .await;
        return Err(e);
    }

    if registry_key.is_none() {
        eprintln!(
            "LoadModel for {} carried no model_key; its load status will not be reported",
            model_name
        );
    } else {
        catalogue::remember(&state.model_cache_dir, model_name, model_key);
    }

    // The server sends LoadModel on every fresh connection, including a
    // reconnect after the user just paused and resumed sharing - it has no
    // way to know this device already has the model warm in memory.
    if state
        .model_manager
        .lock()
        .await
        .is_model_loaded_and_matches(model_name)
    {
        println!("Model {} already loaded - skipping reload", model_name);
        set_model_status(
            state,
            app_handle,
            status::ModelStatus::Ready {
                model_name: model_name.to_string(),
            },
        )
        .await;
        reporter
            .send(registry_key.as_deref(), "ready", Some(100), None, None)
            .await;
        return Ok(());
    }

    log_message(
        &state.message_log,
        app_handle,
        "ModelInit",
        &format!("Starting model load: {} from {}", model_name, model_ref),
    )
    .await;

    // Build HuggingFace URL from ref and model name
    // The ref should contain the repository path: {owner}/{repo}
    // Example: bartowski/Llama-3.2-1B-Instruct-GGUF
    // The model_name contains the filename: Llama-3.2-1B-Instruct-Q4_K_M.gguf
    let model_url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        model_ref, model_name
    );

    // No model_manager lock anywhere in here: the current model keeps serving
    // for the whole download (see reload.rs).
    let cache_dir = state.model_cache_dir.clone();
    let download = async {
        let cached_path = cache_dir.join(model_name);
        if cached_path.exists() {
            println!("Model found in cache: {}", model_name);
            return Ok(cached_path);
        }

        println!("Model not in cache, downloading...");
        log_message(
            &state.message_log,
            app_handle,
            "ModelDownload",
            &format!("Downloading from: {}", model_url),
        )
        .await;
        set_model_status(
            state,
            app_handle,
            status::ModelStatus::Downloading {
                model_name: model_name.to_string(),
                progress: 0,
            },
        )
        .await;
        reporter
            .send(registry_key.as_deref(), "downloading", Some(0), None, None)
            .await;

        let app_handle_clone = app_handle.clone();
        let model_name_clone = model_name.to_string();
        let state_clone = state.clone();
        let progress_reporter = reporter.clone();
        // Owned: the callback outlives this frame.
        let progress_key = registry_key.clone();

        let progress_callback = move |progress: model_manager::DownloadProgress| {
            println!(
                "Download progress: {:.1}% - {}",
                progress.percentage, progress.status
            );

            // Emit legacy event for compatibility
            app_handle_clone
                .emit_all("model-download-progress", progress.clone())
                .ok();

            // At 100% the switch moves on to draining/loading, which report themselves.
            if progress.percentage < 100.0 {
                progress_reporter
                    .try_send_progress(progress_key.as_deref(), progress.percentage as u32);

                let app_handle_clone2 = app_handle_clone.clone();
                let model_name_clone2 = model_name_clone.clone();
                let state_clone2 = state_clone.clone();

                tokio::spawn(async move {
                    set_model_status(
                        &state_clone2,
                        &app_handle_clone2,
                        status::ModelStatus::Downloading {
                            model_name: model_name_clone2,
                            progress: progress.percentage as u8,
                        },
                    )
                    .await;
                });
            }
        };

        model_manager::download_model_to(
            &cache_dir,
            &model_url,
            model_name,
            model_manager::DownloadTimeouts::default(),
            progress_callback,
        )
        .await
    };

    // reload() reports synchronously; the reports need awaiting, in order.
    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel::<ReloadEvent>();
    let switch = reload::reload(
        &*state.model_manager,
        &state.generations,
        download,
        ReloadTimeouts::default(),
        move |event| {
            let _ = events_tx.send(event);
        },
    );
    let report_events = async {
        while let Some(event) = events_rx.recv().await {
            report_reload_event(
                state,
                app_handle,
                reporter,
                model_name,
                registry_key.as_deref(),
                event,
            )
            .await;
        }
    };
    let (result, ()) = tokio::join!(switch, report_events);
    result
}

async fn report_reload_event(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    reporter: &LoadStatusReporter,
    model_name: &str,
    registry_key: Option<&str>,
    event: ReloadEvent,
) {
    match event {
        ReloadEvent::Draining => {
            log_message(
                &state.message_log,
                app_handle,
                "ModelDrain",
                &format!(
                    "Draining in-flight generations before loading {}",
                    model_name
                ),
            )
            .await;
            reporter
                .send(registry_key, "draining", None, None, None)
                .await;
        }
        ReloadEvent::Loading { load_ms } => {
            if load_ms == 0 {
                set_model_status(
                    state,
                    app_handle,
                    status::ModelStatus::Loading {
                        model_name: model_name.to_string(),
                    },
                )
                .await;
            }
            reporter
                .send(registry_key, "loading", None, Some(load_ms), None)
                .await;
        }
        ReloadEvent::Ready { load_ms } => {
            log_message(
                &state.message_log,
                app_handle,
                "ModelLoaded",
                &format!(
                    "Model loaded successfully: {} in {} ms",
                    model_name, load_ms
                ),
            )
            .await;
            set_model_status(
                state,
                app_handle,
                status::ModelStatus::Ready {
                    model_name: model_name.to_string(),
                },
            )
            .await;
            reporter
                .send(registry_key, "ready", Some(100), Some(load_ms), None)
                .await;
        }
        ReloadEvent::Failed { error } => {
            log_message(
                &state.message_log,
                app_handle,
                "ModelError",
                &format!("Model load failed: {}", error),
            )
            .await;
            set_model_status(
                state,
                app_handle,
                status::ModelStatus::Error {
                    message: error.clone(),
                },
            )
            .await;
            reporter
                .send(registry_key, "failed", None, None, Some(error))
                .await;
        }
        ReloadEvent::Restored { path, load_ms } => {
            let previous = file_name(&path);
            log_message(
                &state.message_log,
                app_handle,
                "ModelLoaded",
                &format!("Restored previous model {} in {} ms", previous, load_ms),
            )
            .await;
            set_model_status(
                state,
                app_handle,
                status::ModelStatus::Ready {
                    model_name: previous.clone(),
                },
            )
            .await;
            let previous_key = catalogue::registry_key(&state.model_cache_dir, &previous);
            reporter
                .send(
                    previous_key.as_deref(),
                    "ready",
                    Some(100),
                    Some(load_ms),
                    None,
                )
                .await;
        }
    }
}

/// Log lines for inference traffic carry metadata only - counts and lengths,
/// never the text. The message log is shown in the node's UI, and a node
/// serves other people's prompts.
fn describe_chat_request(turns: &[model_manager::ChatTurn]) -> String {
    format!(
        "Generating response to {} message(s), last: {} chars",
        turns.len(),
        turns.last().map(|t| t.content.chars().count()).unwrap_or(0),
    )
}

fn describe_token_update(request_id: &str, update: &model_manager::TokenUpdate) -> String {
    format!(
        "Sending token to backend - chars: {}, choice: {}, is_last: {}, is_final: {}, request_id: {}",
        update.text.chars().count(),
        update.choice_index,
        update.is_last,
        update.is_final,
        request_id,
    )
}

async fn log_message(
    message_log: &Arc<Mutex<Vec<MessageLogEntry>>>,
    app_handle: &tauri::AppHandle,
    message_type: &str,
    content: &str,
) {
    let entry = MessageLogEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        log_type: "grpc".to_string(),
        message_type: message_type.to_string(),
        content: content.to_string(),
        status_code: None,
        request_body: None,
        response_body: None,
        duration_ms: None,
    };

    let mut log = message_log.lock().await;
    ai4all_tauri::devlog::push_capped(&mut log, entry.clone());

    // Emit event to frontend
    app_handle.emit_all("new-message", entry).ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_chat_request_carries_counts_not_content() {
        let turns = vec![
            model_manager::ChatTurn::user("first secret"),
            model_manager::ChatTurn::user("second secret!"),
        ];
        let line = describe_chat_request(&turns);
        assert!(!line.contains("secret"));
        assert!(line.contains("2 message(s)"));
        assert!(line.contains("14 chars"));
    }

    #[test]
    fn describe_token_update_carries_metadata_not_text() {
        let update = model_manager::TokenUpdate {
            text: "private reply".to_string(),
            choice_index: 1,
            is_last: true,
            is_final: true,
            ..Default::default()
        };
        let line = describe_token_update("req-1", &update);
        assert!(!line.contains("private"));
        assert!(line.contains("chars: 13"));
        assert!(line.contains("request_id: req-1"));
    }

    #[test]
    fn resource_exhausted_status_is_a_device_limit_error() {
        let status = Status::resource_exhausted("Organization has reached its plan device limit");
        assert!(is_device_limit_error(&status));
    }

    #[test]
    fn other_statuses_are_not_device_limit_errors() {
        assert!(!is_device_limit_error(&Status::unavailable("server down")));
        assert!(!is_device_limit_error(&Status::internal(
            "unexpected error"
        )));
        assert!(!is_device_limit_error(&Status::unauthenticated(
            "bad token"
        )));
    }

    #[test]
    fn device_limit_status_skips_retry() {
        assert!(should_skip_retry(
            &status::ConnectionStatus::DeviceLimitReached {
                message: "at limit".to_string(),
            }
        ));
    }

    #[test]
    fn a_refused_account_skips_retry() {
        // Deleted or deactivated: the same token would be refused again.
        assert!(should_skip_retry(&status::ConnectionStatus::Refused {
            message: "Account deleted".to_string(),
        }));
    }

    #[test]
    fn other_statuses_do_not_skip_retry() {
        assert!(!should_skip_retry(&status::ConnectionStatus::Error {
            message: "transient".to_string(),
        }));
        assert!(!should_skip_retry(&status::ConnectionStatus::Connecting));
        assert!(!should_skip_retry(&status::ConnectionStatus::Disconnected));
        assert!(!should_skip_retry(&status::ConnectionStatus::Connected));
    }

    // ---- run_heartbeat: a heartbeat lives and dies with its connection (#241) ----

    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc as StdArc;

    const TICK: Duration = Duration::from_millis(10);

    fn counting_failures() -> (
        StdArc<AtomicU32>,
        impl FnMut(u32, String) -> std::future::Ready<()>,
    ) {
        let count = StdArc::new(AtomicU32::new(0));
        let seen = count.clone();
        (count, move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        })
    }

    #[tokio::test]
    async fn heartbeat_stops_without_reporting_once_its_connection_ended() {
        // The #241 case: the server closed the stream, so the receiver is gone
        // and every send fails. The connection's scope is cancelled as the
        // connection function returns, as its drop guard does.
        let (tx, rx) = tokio::sync::mpsc::channel::<StreamEventsRequest>(4);
        drop(rx);
        let scope = CancellationToken::new();
        let guard = scope.clone().drop_guard();
        let (failures, on_failure) = counting_failures();

        let heartbeat = tokio::spawn(run_heartbeat(
            scope,
            TICK,
            3,
            move || {
                let tx = tx.clone();
                async move {
                    send_heartbeat(&tx, "device-1")
                        .await
                        .map_err(|e| e.message().to_string())
                }
            },
            on_failure,
        ));
        drop(guard);

        let end = tokio::time::timeout(Duration::from_secs(1), heartbeat)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(end, HeartbeatEnd::Stopped);
        assert_eq!(failures.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn heartbeat_gives_up_after_max_failures_while_its_connection_is_current() {
        let (failures, on_failure) = counting_failures();

        let end = tokio::time::timeout(
            Duration::from_secs(1),
            run_heartbeat(
                CancellationToken::new(),
                TICK,
                3,
                || std::future::ready(Err("channel closed".to_string())),
                on_failure,
            ),
        )
        .await
        .unwrap();

        assert_eq!(end, HeartbeatEnd::GaveUp);
        assert_eq!(failures.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_send_that_fails_because_the_connection_just_ended_is_not_a_failure() {
        let scope = CancellationToken::new();
        let ending = scope.clone();
        let (failures, on_failure) = counting_failures();

        let end = tokio::time::timeout(
            Duration::from_secs(1),
            run_heartbeat(
                scope,
                TICK,
                1,
                move || {
                    ending.cancel();
                    std::future::ready(Err("channel closed".to_string()))
                },
                on_failure,
            ),
        )
        .await
        .unwrap();

        assert_eq!(end, HeartbeatEnd::Stopped);
        assert_eq!(failures.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_successful_heartbeat_resets_the_failure_count() {
        // Fail, fail, succeed, then fail, fail: never 3 in a row, until the
        // scope ends it.
        let scope = CancellationToken::new();
        let ending = scope.clone();
        let calls = StdArc::new(AtomicU32::new(0));
        let seen = calls.clone();
        let (failures, on_failure) = counting_failures();

        let end = tokio::time::timeout(
            Duration::from_secs(1),
            run_heartbeat(
                scope,
                TICK,
                3,
                move || {
                    let n = seen.fetch_add(1, Ordering::SeqCst);
                    if n == 5 {
                        ending.cancel();
                    }
                    std::future::ready(if n == 2 {
                        Ok(())
                    } else {
                        Err("flaky".to_string())
                    })
                },
                on_failure,
            ),
        )
        .await
        .unwrap();

        assert_eq!(end, HeartbeatEnd::Stopped);
        assert_eq!(failures.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn the_user_disconnecting_stops_the_heartbeat_through_the_connection_scope() {
        // The connection scope is a child of the global cancel token, so a
        // user disconnect still stops every heartbeat.
        let global = CancellationToken::new();
        let scope = global.child_token();
        let (failures, on_failure) = counting_failures();

        let heartbeat = tokio::spawn(run_heartbeat(
            scope,
            TICK,
            3,
            || std::future::ready(Ok(())),
            on_failure,
        ));
        global.cancel();

        let end = tokio::time::timeout(Duration::from_secs(1), heartbeat)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(end, HeartbeatEnd::Stopped);
        assert_eq!(failures.load(Ordering::SeqCst), 0);
    }
}
