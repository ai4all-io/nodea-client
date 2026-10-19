//! The node's half of the OpenAI compatibility contract, end to end.
//!
//! The backend's integration suite drives the real `openai` SDK against the
//! HTTP surface, but stops at the gRPC boundary with a Python mock node - it
//! never exercises this code. These tests pick up exactly where that leaves
//! off: they build a real protobuf `ChatCompletionRequest` (the same message
//! type the backend sends) with every OpenAI field populated, and assert on the
//! *mistralrs request* that comes out the other side.
//!
//! What this catches that the per-field unit tests do not: a field added to
//! `protos/service.proto` and wired through the backend but silently never read
//! on the node. That request would still generate, still stream and still pass
//! every other test - it would just quietly ignore what the caller asked for,
//! which is the exact failure this whole surface exists to prevent.
//!
//! What it deliberately does not claim: that a constrained generation really
//! returns valid JSON, or that a tool call really parses. That needs a loaded
//! model, and macOS cannot create a Metal device from `cargo test`.

use ai4all_tauri::model_manager::{build_request, GenerationOptions};
use ai4all_tauri::openai_request::{chat_turns, generation_options};
use ai4all_tauri::proto::{ChatCompletionRequest, ChatMessage};
use mistralrs::{Constraint, RequestLike, ToolChoice};
use std::collections::HashMap;

const TOOLS: &str = r#"[{"type":"function","function":{
    "name":"get_weather","description":"Weather for a city",
    "parameters":{"type":"object","properties":{"city":{"type":"string"}}}}}]"#;

const SCHEMA: &str = r#"{"type":"json_schema","json_schema":{"name":"animal",
    "schema":{"type":"object","properties":{"animal":{"type":"string"}},
    "required":["animal"]}}}"#;

const WEB_SEARCH: &str = r#"{"search_context_size":"high",
    "user_location":{"type":"approximate","approximate":{"city":"Oslo",
    "country":"NO","region":"Oslo","timezone":"Europe/Oslo"}}}"#;

fn assert_close(actual: Option<f64>, expected: f64, field: &str) {
    let actual = actual.unwrap_or_else(|| panic!("{field} never reached the sampler"));
    assert!(
        (actual - expected).abs() < 1e-6,
        "{field}: expected ~{expected}, got {actual}",
    );
}

fn message(role: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: role.to_string(),
        content: content.to_string(),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    }
}

/// A request with **every** OpenAI field the backend can send populated.
///
/// Deliberately one fixture rather than one per field: the point is to catch
/// fields that go missing when they travel together, which a per-field test
/// cannot see.
fn everything() -> ChatCompletionRequest {
    ChatCompletionRequest {
        messages: vec![
            message("system", "Be terse."),
            message("user", "Weather in Oslo?"),
        ],
        temperature: Some(0.7),
        top_p: Some(0.9),
        max_tokens: Some(64),
        seed: Some(42),
        stop: vec!["END".to_string(), "STOP".to_string()],
        frequency_penalty: Some(0.5),
        presence_penalty: Some(-0.5),
        response_format: Some(SCHEMA.to_string()),
        tools: Some(TOOLS.to_string()),
        tool_choice: Some(r#""auto""#.to_string()),
        logprobs: Some(true),
        top_logprobs: Some(5),
        n: Some(3),
        logit_bias: HashMap::from([(1234u32, 5.0f32), (99u32, -100.0f32)]),
        web_search_options: Some(WEB_SEARCH.to_string()),
        reasoning_effort: Some("high".to_string()),
    }
}

#[test]
fn every_openai_field_survives_the_trip_from_protobuf_to_mistralrs() {
    let request = everything();
    let options = generation_options(&request);
    let mut built = build_request(&chat_turns(&request), &options).unwrap();

    // --- sampling ---------------------------------------------------------
    let sampling = built.take_sampling_params();
    // `temperature` and `top_p` are `float` (f32) on the wire and f64 here, so
    // 0.7 arrives as 0.699999988079071. That precision loss is real and
    // inherent to the proto's choice of `float`; it is far below anything that
    // changes sampling behaviour, so it is asserted with a tolerance rather
    // than papered over by widening the wire type on an already-shipped field.
    assert_close(sampling.temperature, 0.7, "temperature");
    assert_close(sampling.top_p, 0.9, "top_p");
    assert_eq!(sampling.max_len, Some(64), "max_tokens");
    assert_eq!(sampling.frequency_penalty, Some(0.5), "frequency_penalty");
    assert_eq!(sampling.presence_penalty, Some(-0.5), "presence_penalty");
    assert_eq!(sampling.top_n_logprobs, 5, "top_logprobs");
    assert_eq!(sampling.n_choices, 3, "n");
    assert_eq!(
        sampling.logits_bias,
        Some(HashMap::from([(1234u32, 5.0f32), (99u32, -100.0f32)])),
        "logit_bias",
    );
    match sampling.stop_toks {
        Some(mistralrs::StopTokens::Seqs(seqs)) => assert_eq!(seqs, vec!["END", "STOP"]),
        _ => panic!("stop sequences did not reach the sampler"),
    }

    // --- constraint, tools, search, logprobs ------------------------------
    assert!(
        matches!(built.take_constraint(), Constraint::JsonSchema(_)),
        "response_format",
    );
    let (tools, choice) = built.take_tools().expect("tools");
    assert_eq!(tools[0].function.name, "get_weather");
    assert!(matches!(choice, ToolChoice::Auto), "tool_choice");
    assert!(
        built.take_web_search_options().is_some(),
        "web_search_options"
    );
    assert!(RequestLike::return_logprobs(&built), "logprobs");

    // --- messages and reasoning effort ------------------------------------
    // take_messages last: it is where reasoning_effort is injected.
    match built.take_messages() {
        mistralrs::core::RequestMessage::Chat {
            messages,
            reasoning_effort,
            ..
        } => {
            assert_eq!(
                reasoning_effort,
                Some(mistralrs::core::ReasoningEffort::High)
            );
            // The caller's system prompt, not ours on top of it.
            assert_eq!(messages.len(), 2, "both turns reached the chat template");
        }
        _ => panic!("expected a chat request"),
    }
}

#[test]
fn an_empty_request_asks_the_engine_for_nothing_special() {
    // The other half of the contract: a backend that sends only messages must
    // not have defaults invented for it beyond the documented ones.
    let request = ChatCompletionRequest {
        messages: vec![message("user", "Hi")],
        ..Default::default()
    };
    let options = generation_options(&request);
    let mut built = build_request(&chat_turns(&request), &options).unwrap();

    assert_eq!(options, GenerationOptions::default());

    let sampling = built.take_sampling_params();
    assert_eq!(sampling.n_choices, 1, "one completion");
    assert_eq!(sampling.top_n_logprobs, 0, "no logprobs work");
    assert_eq!(sampling.logits_bias, None, "no per-token bias work");
    assert!(sampling.stop_toks.is_none());
    // The one default that is always applied, so a model that never emits EOS
    // cannot generate forever.
    assert_eq!(
        sampling.max_len,
        Some(ai4all_tauri::model_manager::DEFAULT_MAX_TOKENS),
    );

    assert!(matches!(built.take_constraint(), Constraint::None));
    assert!(built.take_tools().is_none());
    assert!(built.take_web_search_options().is_none());
    assert!(!RequestLike::return_logprobs(&built));

    match built.take_messages() {
        mistralrs::core::RequestMessage::Chat {
            messages,
            reasoning_effort,
            ..
        } => {
            assert_eq!(reasoning_effort, None);
            // Our default system prompt, added only because the caller sent none.
            assert_eq!(messages.len(), 2);
        }
        _ => panic!("expected a chat request"),
    }
}

#[test]
fn a_flattened_request_from_an_old_backend_still_works() {
    // A backend predating structured messages sends one user message holding
    // the whole conversation. The node must not need a version check.
    let request = ChatCompletionRequest {
        messages: vec![message("user", "System: Be terse.\nUser: Hi")],
        ..Default::default()
    };

    let turns = chat_turns(&request);

    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].role, "user");
    assert!(build_request(&turns, &generation_options(&request)).is_ok());
}

#[test]
fn a_tool_calling_conversation_round_trips() {
    // The model has to see that it asked for the tool before it sees the reply;
    // these messages used to be dropped silently.
    let calls = r#"[{"id":"call_1","type":"function",
        "function":{"name":"get_weather","arguments":"{\"city\":\"Oslo\"}"}}]"#;

    let request = ChatCompletionRequest {
        messages: vec![
            message("user", "Weather in Oslo?"),
            ChatMessage {
                tool_calls: Some(calls.to_string()),
                ..message("assistant", "")
            },
            ChatMessage {
                tool_call_id: Some("call_1".to_string()),
                ..message("tool", "sunny")
            },
        ],
        tools: Some(TOOLS.to_string()),
        ..Default::default()
    };

    let turns = chat_turns(&request);
    let built = build_request(&turns, &generation_options(&request));

    assert!(
        built.is_ok(),
        "a tool conversation must build: {:?}",
        built.err()
    );
    assert_eq!(turns[1].tool_calls.as_deref(), Some(calls));
    assert_eq!(turns[2].tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn a_request_the_node_cannot_honour_fails_loudly() {
    // Generating anyway would return prose where the caller asked for JSON, and
    // they could not tell that apart from a model ignoring them. The backend
    // validates these first, so reaching here at all is a bug - which is
    // exactly why it must not be silent.
    for (label, request) in [
        (
            "response_format",
            ChatCompletionRequest {
                messages: vec![message("user", "Hi")],
                response_format: Some("not json".to_string()),
                ..Default::default()
            },
        ),
        (
            "tools",
            ChatCompletionRequest {
                messages: vec![message("user", "Hi")],
                tools: Some("not json".to_string()),
                ..Default::default()
            },
        ),
        (
            "web_search_options",
            ChatCompletionRequest {
                messages: vec![message("user", "Hi")],
                web_search_options: Some("not json".to_string()),
                ..Default::default()
            },
        ),
        (
            "reasoning_effort",
            ChatCompletionRequest {
                messages: vec![message("user", "Hi")],
                // Rejected by the backend, so the node should never see it.
                reasoning_effort: Some("minimal".to_string()),
                ..Default::default()
            },
        ),
    ] {
        let options = generation_options(&request);
        assert!(
            build_request(&chat_turns(&request), &options).is_err(),
            "{label} should have failed rather than generating regardless",
        );
    }
}

#[test]
fn seed_is_the_one_field_read_and_deliberately_dropped() {
    // mistralrs 0.8.1 seeds one RNG per engine and has no per-request seed.
    // OpenAI documents seed as best-effort, so accepting it is spec-faithful -
    // but nothing downstream may act as though it were honoured.
    let with_seed = ChatCompletionRequest {
        messages: vec![message("user", "Hi")],
        seed: Some(42),
        ..Default::default()
    };
    let without = ChatCompletionRequest {
        seed: None,
        ..with_seed.clone()
    };

    assert_eq!(generation_options(&with_seed), generation_options(&without));
}
