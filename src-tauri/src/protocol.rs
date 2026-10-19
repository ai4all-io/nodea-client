//! What the backend just asked this node to do.
//!
//! `grpc_client::handle_server_message` used to decide this inline, tangled up
//! with spawning tasks, taking the model-manager lock and emitting Tauri
//! events, so the decision itself could not be tested without a running desktop
//! app. Pulling it out leaves a pure function over the wire types, which
//! `tests/grpc_roundtrip_tests.rs` drives across a real tonic connection.
//!
//! Keeping this honest matters more than it looks: every field the backend
//! adds arrives here first, and a `StreamEventsResponse` variant this does not
//! recognise is silently ignored by the node.

use crate::model_manager::{ChatTurn, GenerationOptions};
use crate::openai_request::{chat_turns, generation_options};
use crate::proto::{stream_events_response, StreamEventsResponse};

/// A backend instruction, with the request id it applies to.
#[derive(Clone, Debug, PartialEq)]
pub enum ServerAction {
    /// Generate a completion. The OpenAI fields are already mapped.
    Generate {
        request_id: String,
        turns: Vec<ChatTurn>,
        options: Box<GenerationOptions>,
    },
    /// Load (or confirm already loaded) a model.
    LoadModel {
        model_name: String,
        model_ref: String,
        /// The registry key to report back. Sent by the backend because it
        /// cannot be derived from the filename; see LoadModel in the proto.
        model_key: String,
    },
    /// Stop generating for this request; its client went away.
    Cancel { request_id: String },
    /// Free the loaded model and stay connected, unassigned.
    Unload,
    /// A message with no `msg` set. Logged, never fatal: a node that dropped
    /// its connection over an unrecognised message would be trivially
    /// bricked by a backend deploy.
    Malformed { request_id: String },
}

/// Classify one message from the backend.
pub fn classify(message: StreamEventsResponse) -> ServerAction {
    let request_id = message.request_id.clone();

    match message.msg {
        Some(stream_events_response::Msg::ChatCompletionRequest(chat_request)) => {
            ServerAction::Generate {
                request_id,
                turns: chat_turns(&chat_request),
                options: Box::new(generation_options(&chat_request)),
            }
        }
        Some(stream_events_response::Msg::LoadModel(load_model)) => ServerAction::LoadModel {
            model_name: load_model.model_name,
            model_ref: load_model.model_ref,
            model_key: load_model.model_key,
        },
        Some(stream_events_response::Msg::CancelGeneration(_)) => {
            ServerAction::Cancel { request_id }
        }
        Some(stream_events_response::Msg::UnloadModel(_)) => ServerAction::Unload,
        None => ServerAction::Malformed { request_id },
    }
}
