//! Translating the backend's gRPC `ChatCompletionRequest` into what the
//! inference engine wants.
//!
//! Deliberately a separate module with no Tauri or transport dependencies: it
//! is the whole OpenAI-compatibility surface on the node side, and keeping it
//! pure is what lets `tests/openai_conformance_tests.rs` drive every field of a
//! real protobuf request and assert on the resulting mistralrs request.

use crate::model_manager::{ChatTurn, GenerationOptions};
use crate::proto::ChatCompletionRequest;

/// Map the request's OpenAI sampling fields onto generation options. Unset
/// fields (older backends never set them) stay `None` and get the defaults.
pub fn generation_options(chat_request: &ChatCompletionRequest) -> GenerationOptions {
    if chat_request.seed.is_some() {
        // mistralrs 0.8 seeds one RNG per engine and exposes no per-request seed.
        eprintln!("Ignoring unsupported 'seed' sampling option");
    }
    GenerationOptions {
        temperature: chat_request.temperature.map(f64::from),
        top_p: chat_request.top_p.map(f64::from),
        max_tokens: chat_request.max_tokens.map(|n| n as usize),
        stop: chat_request.stop.clone(),
        frequency_penalty: chat_request.frequency_penalty,
        presence_penalty: chat_request.presence_penalty,
        response_format: chat_request.response_format.clone(),
        tools: chat_request.tools.clone(),
        tool_choice: chat_request.tool_choice.clone(),
        logprobs: chat_request.logprobs.unwrap_or(false),
        top_logprobs: chat_request.top_logprobs.map(|n| n as usize),
        n: chat_request.n.map(|n| n as usize),
        logit_bias: chat_request.logit_bias.clone(),
        web_search_options: chat_request.web_search_options.clone(),
        reasoning_effort: chat_request.reasoning_effort.clone(),
    }
}

/// The conversation, as the engine's chat template will see it.
///
/// A backend predating structured messages sends exactly one flattened user
/// message, so this handles both shapes without a version check.
pub fn chat_turns(chat_request: &ChatCompletionRequest) -> Vec<ChatTurn> {
    chat_request
        .messages
        .iter()
        .map(|message| ChatTurn {
            role: message.role.clone(),
            content: message.content.clone(),
            tool_calls: message.tool_calls.clone(),
            tool_call_id: message.tool_call_id.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::ChatMessage;

    #[test]
    fn generation_options_default_when_the_backend_sends_no_sampling_fields() {
        let request = ChatCompletionRequest::default();
        assert_eq!(generation_options(&request), GenerationOptions::default());
    }

    #[test]
    fn generation_options_carry_the_request_sampling_fields() {
        let request = ChatCompletionRequest {
            temperature: Some(0.5),
            top_p: Some(0.25),
            max_tokens: Some(128),
            seed: Some(7),
            stop: vec!["END".to_string()],
            frequency_penalty: Some(1.5),
            presence_penalty: Some(-1.0),
            ..Default::default()
        };

        assert_eq!(
            generation_options(&request),
            GenerationOptions {
                temperature: Some(0.5),
                top_p: Some(0.25),
                max_tokens: Some(128),
                stop: vec!["END".to_string()],
                frequency_penalty: Some(1.5),
                presence_penalty: Some(-1.0),
                ..Default::default()
            }
        );
    }

    #[test]
    fn chat_turns_carry_the_tool_fields() {
        let request = ChatCompletionRequest {
            messages: vec![ChatMessage {
                role: "tool".to_string(),
                content: "sunny".to_string(),
                name: Some("weather".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
            }],
            ..Default::default()
        };

        let [turn] = &chat_turns(&request)[..] else {
            panic!("expected one turn")
        };

        assert_eq!(turn.role, "tool");
        assert_eq!(turn.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn no_messages_yields_no_turns() {
        assert!(chat_turns(&ChatCompletionRequest::default()).is_empty());
    }
}
