use futures::StreamExt;
use indexmap::IndexMap;
use mistralrs::core::{ReasoningEffort, RequestMessage};
use mistralrs::{
    CalledFunction, ChatCompletionChunkResponse, ChunkChoice, Constraint, GgufModelBuilder, Model,
    RequestBuilder, Response, SamplingParams, StopTokens, TextMessageRole, Tool, ToolCallResponse,
    ToolCallType, ToolChoice,
};
use mistralrs::{CustomLogitsProcessor, MessageContent, RequestLike, WebSearchOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub struct ModelManager {
    model: Option<Model>,
    model_path: Option<PathBuf>,
    cache_dir: PathBuf,
}

#[derive(Clone, serde::Serialize)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percentage: f32,
    pub status: String,
}

/// Generation cap when the caller does not send `max_tokens`.
pub const DEFAULT_MAX_TOKENS: usize = 500;
/// Matches the OpenAI API default when the caller does not send `temperature`.
pub const DEFAULT_TEMPERATURE: f64 = 1.0;

/// OpenAI-style sampling options for one generation. `None` means "use the
/// default", which is what an unset field in the gRPC request maps to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GenerationOptions {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<usize>,
    pub stop: Vec<String>,
    pub frequency_penalty: Option<f32>,
    pub presence_penalty: Option<f32>,
    /// OpenAI's response_format object as JSON, exactly as the caller sent it.
    /// The backend has already validated the envelope.
    pub response_format: Option<String>,
    /// OpenAI's tools array as JSON. mistralrs's Tool deserializes from
    /// OpenAI's own wire shape, so this needs no translation.
    pub tools: Option<String>,
    /// OpenAI's tool_choice as JSON (a string or an object).
    pub tool_choice: Option<String>,
    /// Whether the caller asked for log probabilities.
    pub logprobs: bool,
    /// How many alternatives per token, from OpenAI's top_logprobs.
    pub top_logprobs: Option<usize>,
    /// How many completions to generate, from OpenAI's n. None means one.
    pub n: Option<usize>,
    /// Per-token logit adjustments, keyed by *this* model's token ids.
    pub logit_bias: std::collections::HashMap<u32, f32>,
    /// OpenAI's web_search_options object as JSON. Only honourable by a node
    /// loaded with search enabled.
    pub web_search_options: Option<String>,
    /// OpenAI's reasoning_effort, already narrowed by the backend to what the
    /// engine can express (low, medium, high).
    pub reasoning_effort: Option<String>,
}

impl GenerationOptions {
    /// Greedy decoding (the previous hard-coded behavior), used by the local
    /// `generate_text` command.
    pub fn greedy(max_tokens: usize) -> Self {
        Self {
            temperature: Some(0.0),
            max_tokens: Some(max_tokens),
            ..Self::default()
        }
    }

    /// `max_len` is always set: a runaway generation (e.g. a small quantized
    /// model failing to produce an EOS token) otherwise has no hard stop.
    fn to_sampling_params(&self) -> SamplingParams {
        let temperature = self.temperature.unwrap_or(DEFAULT_TEMPERATURE);
        // Temperature 0 means greedy in the OpenAI API; mistralrs expresses
        // that as top_k = 1 with no temperature.
        let (temperature, top_k) = if temperature <= 0.0 {
            (None, Some(1))
        } else {
            (Some(temperature), None)
        };

        SamplingParams {
            temperature,
            top_k,
            top_p: self.top_p,
            frequency_penalty: self.frequency_penalty,
            presence_penalty: self.presence_penalty,
            stop_toks: (!self.stop.is_empty()).then(|| StopTokens::Seqs(self.stop.clone())),
            top_n_logprobs: self.top_logprobs.unwrap_or(0),
            n_choices: self.n.unwrap_or(1),
            logits_bias: (!self.logit_bias.is_empty()).then(|| self.logit_bias.clone()),
            max_len: Some(self.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS)),
            ..SamplingParams::neutral()
        }
    }
}

/// The system prompt used only when the caller supplied none of its own.
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful AI assistant.";

/// One turn of a conversation, as the backend sends it.
///
/// Roles arrive already normalised by the backend ("developer" is OpenAI's
/// rename of "system"), so this only has to map names onto mistralrs roles.
/// A log line for a single-prompt generation. Metadata only: the prompt is
/// someone's inference traffic and must never reach stdout.
pub fn describe_prompt(prompt: &str, max_tokens: usize) -> String {
    format!(
        "Generating response: prompt {} chars, max_tokens {}",
        prompt.chars().count(),
        max_tokens
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
    /// An assistant turn's tool calls, as OpenAI's tool_calls array in JSON.
    pub tool_calls: Option<String>,
    /// A tool turn's tool_call_id.
    pub tool_call_id: Option<String>,
}

impl ChatTurn {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn message_role(&self) -> TextMessageRole {
        match self.role.as_str() {
            "system" | "developer" => TextMessageRole::System,
            "assistant" => TextMessageRole::Assistant,
            "tool" => TextMessageRole::Tool,
            // An unknown role is treated as a user turn rather than dropped:
            // dropping it would silently remove content the model was meant to
            // see, which is the failure mode this whole change exists to fix.
            _ => TextMessageRole::User,
        }
    }
}

/// OpenAI's tool_calls JSON, parsed into what add_message_with_tool_call wants.
fn parse_tool_calls(raw: &str) -> Result<Vec<ToolCallResponse>, String> {
    #[derive(serde::Deserialize)]
    struct RawFunction {
        name: String,
        #[serde(default)]
        arguments: String,
    }

    #[derive(serde::Deserialize)]
    struct RawToolCall {
        #[serde(default)]
        id: String,
        function: RawFunction,
    }

    let raw_calls: Vec<RawToolCall> =
        serde_json::from_str(raw).map_err(|e| format!("Could not parse tool_calls: {}", e))?;

    Ok(raw_calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| ToolCallResponse {
            index,
            id: call.id,
            tp: ToolCallType::Function,
            function: CalledFunction {
                name: call.function.name,
                arguments: call.function.arguments,
            },
        })
        .collect())
}

/// OpenAI's tools array and tool_choice, parsed into mistralrs's types.
///
/// Both derive Deserialize in OpenAI's own wire shape, so this is a parse
/// rather than a translation. tool_choice defaults to Auto, matching OpenAI.
fn parse_tools(
    tools: Option<&str>,
    tool_choice: Option<&str>,
) -> Result<Option<(Vec<Tool>, ToolChoice)>, String> {
    let Some(raw_tools) = tools else {
        return Ok(None);
    };

    let parsed: Vec<Tool> =
        serde_json::from_str(raw_tools).map_err(|e| format!("Could not parse tools: {}", e))?;

    let choice = match tool_choice {
        Some(raw) => {
            serde_json::from_str(raw).map_err(|e| format!("Could not parse tool_choice: {}", e))?
        }
        None => ToolChoice::Auto,
    };

    Ok(Some((parsed, choice)))
}

/// How a generation ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GenerationOutcome {
    /// The model finished, or hit a stop condition.
    Completed,
    /// The backend asked us to stop, because its client went away.
    Cancelled,
}

/// A `RequestBuilder` that can also set `reasoning_effort`.
///
/// `RequestBuilder` in mistralrs 0.8.1 hardcodes `reasoning_effort: None` when
/// it builds its `RequestMessage::Chat`, and exposes no setter - the field only
/// exists on the message enum. Rather than abandon `RequestBuilder` and hand-
/// assemble a `NormalRequest` (which would mean reaching for the engine's
/// low-level sender and re-implementing everything `Model::stream_chat_request`
/// does), this wraps it and rewrites just that one field on the way past.
///
/// mistralrs 0.9.x adds `RequestBuilder::with_reasoning_effort`, at which point
/// this whole type can be deleted.
pub struct ReasoningRequest {
    inner: RequestBuilder,
    reasoning_effort: Option<ReasoningEffort>,
}

impl RequestLike for ReasoningRequest {
    fn messages_ref(&self) -> &[IndexMap<String, MessageContent>] {
        self.inner.messages_ref()
    }

    fn images_ref(&self) -> &[image::DynamicImage] {
        self.inner.images_ref()
    }

    fn resolve_pending_prefixes(&mut self, category: &mistralrs::ModelCategory) {
        // Delegated rather than left as the trait's no-op default: Model calls
        // this before sending, and skipping it would silently change behaviour
        // for anything the inner builder had deferred.
        self.inner.resolve_pending_prefixes(category)
    }

    fn take_messages(&mut self) -> RequestMessage {
        let messages = self.inner.take_messages();
        match (messages, self.reasoning_effort) {
            (
                RequestMessage::Chat {
                    messages,
                    enable_thinking,
                    ..
                },
                Some(effort),
            ) => RequestMessage::Chat {
                messages,
                enable_thinking,
                reasoning_effort: Some(effort),
            },
            (other, _) => other,
        }
    }

    fn take_logits_processors(&mut self) -> Option<Vec<Arc<dyn CustomLogitsProcessor>>> {
        self.inner.take_logits_processors()
    }

    fn take_adapters(&mut self) -> Option<Vec<String>> {
        self.inner.take_adapters()
    }

    fn return_logprobs(&self) -> bool {
        // Qualified: RequestBuilder has an inherent return_logprobs(self, bool)
        // builder setter that shadows the trait method of the same name.
        RequestLike::return_logprobs(&self.inner)
    }

    fn enable_search(&self) -> Option<bool> {
        self.inner.enable_search()
    }

    fn take_constraint(&mut self) -> Constraint {
        self.inner.take_constraint()
    }

    fn take_tools(&mut self) -> Option<(Vec<Tool>, ToolChoice)> {
        self.inner.take_tools()
    }

    fn take_sampling_params(&mut self) -> SamplingParams {
        self.inner.take_sampling_params()
    }

    fn take_web_search_options(&mut self) -> Option<WebSearchOptions> {
        self.inner.take_web_search_options()
    }

    fn truncate_sequence(&self) -> bool {
        self.inner.truncate_sequence()
    }
}

/// Map OpenAI's reasoning_effort onto the engine's.
///
/// The backend has already rejected the values 0.8.1 cannot express, so an
/// unrecognised one here is a bug rather than bad input - but rounding it
/// silently would hide that, so it is an error.
fn map_reasoning_effort(effort: &str) -> Result<ReasoningEffort, String> {
    match effort {
        "low" => Ok(ReasoningEffort::Low),
        "medium" => Ok(ReasoningEffort::Medium),
        "high" => Ok(ReasoningEffort::High),
        other => Err(format!("Unsupported reasoning_effort: {}", other)),
    }
}

/// Whether this node loads its model with mistralrs web search enabled.
///
/// Search needs an embedding model downloaded and network egress at load time,
/// so it is a deployment decision rather than something every node should do.
/// A node with it off does not advertise the capability, and the backend then
/// rejects web_search_options rather than quietly ignoring it.
pub fn web_search_enabled() -> bool {
    std::env::var("NODEA_ENABLE_WEB_SEARCH")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Turn OpenAI's response_format into a mistralrs generation constraint.
///
/// `json_object` maps to the JSON Schema `{"type": "object"}` rather than to a
/// hand-written Lark grammar for arbitrary JSON: that *is* what json_object
/// means, and it reuses the same well-tested constrained-decoding path as
/// json_schema instead of introducing a second grammar dialect to get wrong.
fn constraint_for(response_format: Option<&str>) -> Result<Constraint, String> {
    let Some(raw) = response_format else {
        return Ok(Constraint::None);
    };

    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("Could not parse response_format: {}", e))?;

    match value.get("type").and_then(|t| t.as_str()) {
        Some("json_object") => Ok(Constraint::JsonSchema(
            serde_json::json!({"type": "object"}),
        )),
        Some("json_schema") => {
            let schema = value
                .get("json_schema")
                .and_then(|w| w.get("schema"))
                .ok_or("response_format.json_schema.schema is missing")?;
            Ok(Constraint::JsonSchema(schema.clone()))
        }
        // "text" is OpenAI's default and the backend does not send it; anything
        // else the backend already rejected.
        _ => Ok(Constraint::None),
    }
}

/// Build a chat request from the real conversation.
///
/// Previously the node took `messages.last()` and prepended its own system
/// prompt unconditionally, so a caller's system prompt ended up *inside* a user
/// turn underneath the node's, and all history was discarded. The model now
/// sees its own chat template applied to the actual turns.
pub fn build_request(
    messages: &[ChatTurn],
    options: &GenerationOptions,
) -> Result<ReasoningRequest, String> {
    let mut builder = RequestBuilder::new();

    // Only supply a default system prompt when the caller gave none: adding
    // ours on top of theirs is how the caller's instructions got buried.
    if !messages
        .iter()
        .any(|m| matches!(m.message_role(), TextMessageRole::System))
    {
        builder = builder.add_message(TextMessageRole::System, DEFAULT_SYSTEM_PROMPT);
    }

    for message in messages {
        builder = match (&message.tool_calls, &message.tool_call_id) {
            // An assistant turn that called a tool. Replaying it matters: the
            // model must see that it asked before it sees the answer.
            (Some(raw), _) => builder.add_message_with_tool_call(
                message.message_role(),
                &message.content,
                parse_tool_calls(raw)?,
            ),
            // A tool's reply, which needs the id of the call it answers.
            (None, Some(id)) => builder.add_tool_message(&message.content, id),
            (None, None) => builder.add_message(message.message_role(), &message.content),
        };
    }

    if options.logprobs {
        builder = builder.return_logprobs(true);
    }

    if let Some(raw) = options.web_search_options.as_deref() {
        let options: mistralrs::WebSearchOptions = serde_json::from_str(raw)
            .map_err(|e| format!("Could not parse web_search_options: {}", e))?;
        builder = builder.with_web_search_options(options);
    }

    if let Some((tools, choice)) =
        parse_tools(options.tools.as_deref(), options.tool_choice.as_deref())?
    {
        builder = builder.set_tools(tools).set_tool_choice(choice);
    }

    builder = builder.set_sampling(options.to_sampling_params());

    // An unusable constraint is an error, not something to shrug at: generating
    // unconstrained would return prose where the caller asked for JSON, and
    // they would have no way to tell it apart from a model that ignored them.
    // B3's error path carries this back to the caller as a 502.
    builder = match constraint_for(options.response_format.as_deref())? {
        Constraint::None => builder,
        constraint => builder.set_constraint(constraint),
    };

    let reasoning_effort = match options.reasoning_effort.as_deref() {
        Some(effort) => Some(map_reasoning_effort(effort)?),
        None => None,
    };

    Ok(ReasoningRequest {
        inner: builder,
        reasoning_effort,
    })
}

/// Real token counts, as the engine reports them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GenerationUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// One callback invocation: a piece of text, and on the terminal one, the
/// metadata that only exists at the end.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TokenUpdate {
    /// Which of the n requested completions this belongs to.
    pub choice_index: usize,
    pub text: String,
    /// This choice has finished; its finish_reason is set.
    pub is_last: bool,
    /// Every choice has finished, so there are no more tokens for this request
    /// at all. Equal to `is_last` whenever n == 1, which is the default.
    pub is_final: bool,
    /// Set only on the terminal update. Already mapped to OpenAI's vocabulary.
    pub finish_reason: Option<String>,
    /// Set only on the terminal update, and only when the engine reported it.
    pub usage: Option<GenerationUsage>,
    /// Tool calls the model emitted, as OpenAI's tool_calls array in JSON.
    ///
    /// The engine parses these out of the generated text and emits each one
    /// complete in a single chunk, alongside finish_reason "tool_calls" - it
    /// does not stream partial argument deltas.
    pub tool_calls: Option<String>,
    /// This token's log probabilities, as one OpenAI ChatCompletionTokenLogprob
    /// object in JSON.
    pub logprobs: Option<String>,
}

/// mistralrs finish reasons that are not in OpenAI's enum and need a decision.
///
/// `canceled` is a failure, not a finish reason: the sequence did not complete,
/// so reporting it as one would claim the model stopped on its own.
/// `generated_image` and `generated_speech` are unreachable for a text GGUF, so
/// they are logged and fall back to "stop" rather than being invented into the
/// OpenAI enum.
pub const FINISH_REASON_CANCELED: &str = "canceled";

/// Map a mistralrs finish reason onto OpenAI's, or refuse.
pub fn map_finish_reason(reason: &str) -> Result<String, String> {
    match reason {
        "stop" | "length" | "tool_calls" => Ok(reason.to_string()),
        FINISH_REASON_CANCELED => Err("Generation was canceled".to_string()),
        other => {
            eprintln!(
                "Unexpected finish_reason {:?} from a text model; reporting \"stop\"",
                other
            );
            Ok("stop".to_string())
        }
    }
}

/// One logprob, in the units and the range OpenAI's schema promises.
///
/// Two corrections, both forced by the engine:
///
/// mistralrs computes `prob.log(10.0)` - base 10 - for the sampled token
/// (`mistralrs-core-0.8.1/src/sampler.rs:445`) and for every alternative
/// (`:426`, `:434`, `:619`, `:627`). OpenAI's `logprob` is a *natural* log, so
/// passing these through unchanged understates every value by a factor of
/// ln(10) ≈ 2.303. Consistent within a response, and consistently wrong against
/// the field's definition: a client converting to a probability with `exp()`
/// gets `p^0.434`.
///
/// And `log10(0)` is `-inf`, which `serde_json` writes as `null` - for a field
/// the schema types as a required number. Measured live: every alternative in a
/// `top_logprobs: 3` response came back `"logprob": null`. OpenAI's own answer
/// for a token too unlikely to represent is `-9999`, so that is what a
/// non-finite value becomes here.
fn openai_logprob_value(base_ten: f32) -> f32 {
    /// OpenAI uses this for "so unlikely the real value is not representable".
    const UNREPRESENTABLE: f32 = -9999.0;

    let natural = base_ten * std::f32::consts::LN_10;
    if natural.is_finite() {
        natural
    } else {
        UNREPRESENTABLE
    }
}

/// One token's log probabilities, in OpenAI's ChatCompletionTokenLogprob shape.
///
/// Built here rather than on the backend because only the node has the
/// tokenizer. Two mismatches with mistralrs's own shape have to be bridged:
///
/// - mistralrs reports each alternative by token *id* (`TopLogprob.token: u32`)
///   with the decoded text in `bytes: Option<String>`, while OpenAI's `token` is
///   the text. The decoded text is used when present; otherwise the id is
///   rendered, because emitting a number where a client expects a token string
///   is less misleading than emitting an empty one.
/// - OpenAI's `bytes` is an array of integers, so the token text is re-encoded
///   as UTF-8 bytes here.
/// - The values themselves need converting; see `openai_logprob_value`.
fn openai_logprob(logprob: &mistralrs::core::ResponseLogprob) -> serde_json::Value {
    fn entry(token: &str, logprob: f32) -> serde_json::Value {
        serde_json::json!({
            "token": token,
            "logprob": openai_logprob_value(logprob),
            "bytes": token.as_bytes(),
        })
    }

    let mut value = entry(&logprob.token, logprob.logprob);
    value["top_logprobs"] = serde_json::Value::Array(
        logprob
            .top_logprobs
            .iter()
            .map(|top| {
                let token = top
                    .bytes
                    .clone()
                    .unwrap_or_else(|| format!("<token {}>", top.token));
                entry(&token, top.logprob)
            })
            .collect(),
    );
    value
}

/// One choice's contribution to a streaming chunk.
struct ChunkPiece {
    choice_index: usize,
    text: String,
    finish_reason: Option<String>,
    tool_calls: Option<String>,
    logprobs: Option<String>,
}

/// Every choice present in a streaming chunk.
///
/// With n > 1 mistralrs groups the choices of a generation step into one chunk,
/// each carrying its own `index`, so taking `choices.first()` - which is what
/// this did - would silently drop every completion but the first.
///
/// A chunk's terminal choice can carry `finish_reason` with no `content` at
/// all - a normal shape for the last chunk of many OpenAI-style streaming
/// APIs, including mistralrs. `content` must not be required to match here:
/// doing so previously meant a stream whose final (or only) chunk had no
/// content produced zero callback invocations and no `last` signal, so
/// `generate_streaming` returned `Ok(())` having generated nothing, with no
/// way for a caller to distinguish "nothing generated" from "still running".
///
/// The finish reason string was previously reduced to `.is_some()` and thrown
/// away, which is why every response claimed finish_reason "stop" even when the
/// token cap had been hit.
///
/// `tool_calls` and `logprobs` are serialised here, into OpenAI's own shapes,
/// because only the node has the tokenizer and mistralrs's `ToolCallResponse`
/// already serialises to exactly OpenAI's tool-call shape.
fn extract_chunk_pieces(response: &Response) -> Vec<ChunkPiece> {
    let Response::Chunk(ChatCompletionChunkResponse { choices, .. }) = response else {
        return Vec::new();
    };

    choices
        .iter()
        .map(
            |ChunkChoice {
                 delta,
                 finish_reason,
                 index,
                 logprobs,
             }| ChunkPiece {
                choice_index: *index,
                text: delta.content.clone().unwrap_or_default(),
                finish_reason: finish_reason.clone(),
                tool_calls: delta
                    .tool_calls
                    .as_ref()
                    .filter(|calls| !calls.is_empty())
                    .and_then(|calls| match serde_json::to_string(calls) {
                        Ok(json) => Some(json),
                        Err(e) => {
                            eprintln!("Could not serialise tool calls: {}", e);
                            None
                        }
                    }),
                logprobs: logprobs.as_ref().and_then(|logprobs| {
                    match serde_json::to_string(&openai_logprob(logprobs)) {
                        Ok(json) => Some(json),
                        Err(e) => {
                            eprintln!("Could not serialise logprobs: {}", e);
                            None
                        }
                    }
                }),
            },
        )
        .collect()
}

/// What a response that is not a streaming chunk means for the generation.
///
/// `extract_chunk_pieces` returns nothing for these, which used to be the whole
/// of our handling: the loop ignored them, the stream ended, and every failure
/// came back as "Generation stream ended without a finish reason (N tokens
/// received)" no matter what the engine had said. Two live failures - a named
/// `tool_choice` and `n: 2` - were indistinguishable from the outside for
/// exactly this reason.
#[derive(Debug, PartialEq)]
enum StreamEnd {
    /// A chunk, or something with no bearing on the chat path.
    NotTerminal,
    /// The engine finished and sent its aggregate response. It carries a
    /// finish reason per choice, so a generation that ends this way is
    /// complete - not a generation that failed to report one.
    Completed {
        finish_reasons: Vec<(usize, String)>,
    },
    /// The engine gave up. Its own message is the useful part.
    Failed(String),
}

/// Classify a non-chunk response.
///
/// `Response::ModelError` carries a partial `ChatCompletionResponse` alongside
/// the message; the message is what a caller needs, so the partial is only
/// used to say how far it got.
fn classify_stream_end(response: &Response) -> StreamEnd {
    match response {
        Response::Chunk(_) => StreamEnd::NotTerminal,
        Response::Done(done) => StreamEnd::Completed {
            finish_reasons: done
                .choices
                .iter()
                .map(|choice| (choice.index, choice.finish_reason.clone()))
                .collect(),
        },
        Response::ModelError(message, partial) => StreamEnd::Failed(format!(
            "The model failed after {} choice(s): {}",
            partial.choices.len(),
            message
        )),
        Response::InternalError(error) => {
            StreamEnd::Failed(format!("The inference engine failed: {}", error))
        }
        Response::ValidationError(error) => StreamEnd::Failed(format!(
            "The inference engine rejected the request: {}",
            error
        )),
        // Completion, image, speech, raw and embedding responses cannot arrive
        // on a chat stream. Treating them as terminal would turn an unexpected
        // message into a failed generation.
        _ => StreamEnd::NotTerminal,
    }
}

/// Usage off a streaming chunk, when the engine attached it.
///
/// mistralrs puts it on the final chunk, but reading it from any chunk that
/// carries it means the order never matters.
fn extract_chunk_usage(response: &Response) -> Option<GenerationUsage> {
    if let Response::Chunk(ChatCompletionChunkResponse {
        usage: Some(usage), ..
    }) = response
    {
        return Some(GenerationUsage {
            prompt_tokens: usage.prompt_tokens as u32,
            completion_tokens: usage.completion_tokens as u32,
        });
    }
    None
}

/// Timeouts for a model download.
///
/// A single whole-request timeout used to cap the entire multi-GB download at
/// 5 minutes, so any connection slower than ~15 MB/s failed partway through.
/// `idle` catches a stalled connection without penalising a slow but steady
/// one; `total` is only a backstop.
#[derive(Clone, Copy, Debug)]
pub struct DownloadTimeouts {
    pub connect: Duration,
    /// Max wait for the response headers or for the next body chunk.
    pub idle: Duration,
    pub total: Duration,
}

impl Default for DownloadTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(30),
            idle: Duration::from_secs(60),
            total: Duration::from_secs(900),
        }
    }
}

/// Download `url` to `dest_path` via a sibling `.tmp` file, renamed into place
/// only once the whole body has arrived.
async fn download_to_path<F>(
    url: &str,
    dest_path: &Path,
    timeouts: DownloadTimeouts,
    progress_callback: F,
) -> Result<(), String>
where
    F: Fn(DownloadProgress),
{
    tokio::time::timeout(
        timeouts.total,
        fetch_to_path(url, dest_path, timeouts, progress_callback),
    )
    .await
    .map_err(|_| format!("Download timed out after {}s", timeouts.total.as_secs()))?
}

async fn fetch_to_path<F>(
    url: &str,
    dest_path: &Path,
    timeouts: DownloadTimeouts,
    progress_callback: F,
) -> Result<(), String>
where
    F: Fn(DownloadProgress),
{
    progress_callback(DownloadProgress {
        downloaded_bytes: 0,
        total_bytes: 0,
        percentage: 0.0,
        status: "Starting download...".to_string(),
    });

    // reqwest 0.11 has no read timeout, so the idle timeout wraps each await below.
    let client = reqwest::Client::builder()
        .connect_timeout(timeouts.connect)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let stalled = || format!("Download stalled: no data for {}s", timeouts.idle.as_secs());

    let response = tokio::time::timeout(timeouts.idle, client.get(url).send())
        .await
        .map_err(|_| stalled())?
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let total_bytes = response.content_length().unwrap_or(0);
    println!(
        "Total download size: {} bytes ({:.2} GB)",
        total_bytes,
        total_bytes as f64 / 1_073_741_824.0
    );

    let temp_path = dest_path.with_extension("tmp");
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| format!("Failed to create file: {}", e))?;

    let mut downloaded_bytes = 0u64;
    let mut stream = response.bytes_stream();
    let mut last_update = Instant::now();

    // Download chunks and report progress (throttled to once per second)
    while let Some(chunk_result) = tokio::time::timeout(timeouts.idle, stream.next())
        .await
        .map_err(|_| stalled())?
    {
        let chunk = chunk_result.map_err(|e| format!("Download error: {}", e))?;

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write to file: {}", e))?;

        downloaded_bytes += chunk.len() as u64;

        // Only send progress updates once per second
        let now = Instant::now();
        if now.duration_since(last_update) >= Duration::from_secs(1) {
            let percentage = if total_bytes > 0 {
                (downloaded_bytes as f64 / total_bytes as f64 * 100.0) as f32
            } else {
                0.0
            };

            progress_callback(DownloadProgress {
                downloaded_bytes,
                total_bytes,
                percentage,
                status: format!(
                    "Downloading: {:.2} GB / {:.2} GB",
                    downloaded_bytes as f64 / 1_073_741_824.0,
                    total_bytes as f64 / 1_073_741_824.0
                ),
            });

            last_update = now;
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Failed to flush file: {}", e))?;
    drop(file);

    tokio::fs::rename(&temp_path, dest_path)
        .await
        .map_err(|e| format!("Failed to finalize file: {}", e))?;

    progress_callback(DownloadProgress {
        downloaded_bytes: total_bytes,
        total_bytes,
        percentage: 100.0,
        status: "Download complete".to_string(),
    });
    Ok(())
}

/// Download a model into `cache_dir`, unless it is already there.
///
/// A free function so it needs no ModelManager - and therefore no
/// model_manager lock. A node serves generations under that lock, so a
/// download that held it would take the node out of service for minutes.
pub async fn download_model_to<F>(
    cache_dir: &Path,
    url: &str,
    model_name: &str,
    timeouts: DownloadTimeouts,
    progress_callback: F,
) -> Result<PathBuf, String>
where
    F: Fn(DownloadProgress) + Send + 'static,
{
    crate::catalogue::validate_model_file_name(model_name)?;
    let dest_path = cache_dir.join(model_name);

    // Check if already exists
    if dest_path.exists() {
        progress_callback(DownloadProgress {
            downloaded_bytes: 0,
            total_bytes: 0,
            percentage: 100.0,
            status: "Model already cached".to_string(),
        });
        return Ok(dest_path);
    }

    println!("Downloading model from: {}", url);
    download_to_path(url, &dest_path, timeouts, progress_callback).await?;
    println!("Download complete: {:?}", dest_path);
    Ok(dest_path)
}

impl ModelManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        // Create cache directory if it doesn't exist
        std::fs::create_dir_all(&cache_dir).ok();

        Self {
            model: None,
            model_path: None,
            cache_dir,
        }
    }

    /// Load a model from a local path
    pub async fn load_model(&mut self, model_path: impl AsRef<Path>) -> Result<(), String> {
        println!("Loading GGUF model from: {:?}", model_path.as_ref());

        let path = model_path.as_ref();

        // GgufModelBuilder doesn't fail cleanly on a missing file - it panics
        // deep inside its GGUF parsing (an out-of-bounds Vec::swap_remove)
        // instead of returning an Err. Fail fast here so callers get a
        // Result instead of a crash.
        if !path.exists() {
            return Err(format!("Model file not found: {:?}", path));
        }

        // Split path into directory and filename for GgufModelBuilder
        let dir = path
            .parent()
            .ok_or_else(|| "Invalid model path: no parent directory".to_string())?
            .to_str()
            .ok_or_else(|| "Invalid model path: non-UTF8 characters".to_string())?;

        let filename = path
            .file_name()
            .ok_or_else(|| "Invalid model path: no filename".to_string())?
            .to_str()
            .ok_or_else(|| "Invalid model filename: non-UTF8 characters".to_string())?;

        // EXPERIMENT (mistralrs 0.8 upgrade, no vendoring): mistralrs 0.8.1's
        // GgufModelBuilder::build() -> build_gguf_pipeline() hardcodes
        // resolve_device(builder.force_cpu, None) - it silently ignores
        // whatever .with_device() was given and always picks its own device
        // via best_device() internally (confirmed by reading
        // model_builder_trait.rs:752 in the 0.8.1 source). Creating our own
        // Device::new_metal(0) here first and also letting mistralrs create
        // a second one internally is a double Metal device creation in the
        // same process, which crashed with a candle-core panic
        // (Device::all().swap_remove on an emptied list) in earlier testing.
        // Not calling .with_device() at all avoids that entirely and lets
        // mistralrs's own device selection run exactly once.

        // PagedAttention is CUDA-only in mistralrs-paged-attn 0.7.0 (see
        // mistralrs-paged-attn/src/lib.rs: cfg(feature = "cuda")). This build
        // never enables the "cuda" feature (only "metal"/"accelerate" on
        // macOS), so requesting paged attention leaves the pipeline's
        // cache_config unset and mistralrs panics on its own unwrap() when
        // building the scheduler. Don't request it here.
        //
        // Each generate_streaming()/generate() call builds a brand-new,
        // independent single-turn request (system + one user message) - there
        // is never a real multi-turn conversation sharing a growing prompt.
        // mistralrs's sequence-level prefix cache keys purely on raw
        // token-prefix overlap across *all* past sequences, so unrelated
        // requests that happen to share the same static system prompt (every
        // request here) get matched against a completed previous sequence's
        // cached KV state and resumed from it - producing corrupted/garbled
        // output, and sometimes a completion that never reaches a finish
        // reason. Verified directly with a real loaded model: with the cache
        // enabled, the 2nd and 3rd of a sequence of unrelated single-turn
        // prompts started generating the exact same garbled tokens regardless
        // of prompt content, and one ran away past 4000 tokens without ever
        // emitting a finish reason - this is the root cause of "playground
        // works once, then goes silent/hangs." Disabling the cache removes
        // the cross-request corruption; we get no benefit from it anyway
        // since there's no real shared prefix to reuse.
        // Free the resident model before building the next: building while
        // it is still loaded peaks both at once, which is what "This model
        // does not fit on the devices" was.
        self.unload();

        let mut builder = GgufModelBuilder::new(dir, vec![filename.to_string()])
            .with_logging()
            .with_prefix_cache_n(None);

        if web_search_enabled() {
            // Downloads an embedding model on first use and needs network
            // egress, which is why this is opt-in per node rather than default.
            println!("Web search enabled; loading the search embedding model");
            builder = builder.with_search(mistralrs::core::SearchEmbeddingModel::default());
        }

        let model = builder
            .build()
            .await
            .map_err(|e| format!("Failed to load model: {}", e))?;

        println!("Model loaded successfully!");

        self.model = Some(model);
        self.model_path = Some(path.to_path_buf());

        Ok(())
    }

    /// Generate text from a prompt
    pub async fn generate(&self, prompt: &str, max_tokens: usize) -> Result<String, String> {
        let model = self.model.as_ref().ok_or("Model not loaded")?;

        let messages = build_request(
            &[ChatTurn::user(prompt)],
            &GenerationOptions::greedy(max_tokens),
        )?;

        println!("{}", describe_prompt(prompt, max_tokens));

        // Send chat request (non-streaming)
        let response = model
            .send_chat_request(messages)
            .await
            .map_err(|e| format!("Failed to generate: {}", e))?;

        // Extract text from response
        let text = response
            .choices
            .first()
            .ok_or("No response choices")?
            .message
            .content
            .as_ref()
            .ok_or("No content in response")?
            .clone();

        println!("Generation complete!");
        Ok(text)
    }

    /// Generate a response to a conversation with a streaming callback.
    /// The callback receives (token_text, is_last) and should return Ok(()) to continue or Err to stop
    pub async fn generate_streaming<F>(
        &self,
        turns: &[ChatTurn],
        options: &GenerationOptions,
        cancel: &CancellationToken,
        mut callback: F,
    ) -> Result<GenerationOutcome, String>
    where
        F: FnMut(TokenUpdate) -> Result<(), String>,
    {
        let model = self.model.as_ref().ok_or("Model not loaded")?;

        if turns.is_empty() {
            return Err("No messages in chat request".to_string());
        }

        let messages = build_request(turns, options)?;

        println!(
            "Generating streaming response for {} message(s)",
            turns.len()
        );

        // Start timing
        let start = Instant::now();
        let mut first_token_time: Option<Duration> = None;
        let mut token_count = 0;

        // Stream chat request
        let mut stream = model
            .stream_chat_request(messages)
            .await
            .map_err(|e| format!("Failed to start streaming: {}", e))?;

        // Process stream chunks
        let expected_choices = options.n.unwrap_or(1);
        let mut finished_choices: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        let mut saw_final = false;
        // mistralrs attaches Usage to the final chunk, but keeping the latest
        // seen means the ordering never matters. It is per-request rather than
        // per-choice, so the same value accompanies each choice's terminal
        // update and the backend can take any of them.
        let mut latest_usage: Option<GenerationUsage> = None;
        'chunks: while let Some(chunk) = stream.next().await {
            if cancel.is_cancelled() {
                // Dropping `stream` drops the engine's response receiver, which
                // is exactly how mistralrs cancels a sequence: a failed
                // maybe_send_streaming_response ends it (see
                // mistralrs-core/src/pipeline/sampling.rs). No upstream API is
                // needed, and the model_manager lock is released as soon as the
                // caller returns.
                println!("Generation cancelled by the backend");
                return Ok(GenerationOutcome::Cancelled);
            }

            if let Some(usage) = extract_chunk_usage(&chunk) {
                latest_usage = Some(usage);
            }

            match classify_stream_end(&chunk) {
                StreamEnd::NotTerminal => {}
                // The engine's own words, rather than our guess that a stream
                // which stopped early must have had no finish reason.
                StreamEnd::Failed(message) => {
                    eprintln!("{}", message);
                    return Err(message);
                }
                // The aggregate response ends the generation. Any choice that
                // never reported a finish reason on a chunk gets its terminal
                // update from here - which is what stops a generation whose
                // choices finish in different chunks from being reported as a
                // failure.
                StreamEnd::Completed { finish_reasons } => {
                    for (choice_index, reason) in finish_reasons {
                        if !finished_choices.insert(choice_index) {
                            continue;
                        }
                        callback(TokenUpdate {
                            choice_index,
                            text: String::new(),
                            is_last: true,
                            is_final: finished_choices.len() >= expected_choices,
                            finish_reason: Some(map_finish_reason(&reason)?),
                            usage: latest_usage,
                            tool_calls: None,
                            logprobs: None,
                        })?;
                    }
                    // An aggregate response naming no choice at all has told us
                    // nothing, and breaking here would leave the backend
                    // waiting for a terminal update that never comes. Fall
                    // through to the error below instead.
                    if finished_choices.is_empty() {
                        break 'chunks;
                    }
                    saw_final = true;
                    println!("\nGeneration complete!");
                    break 'chunks;
                }
            }

            for piece in extract_chunk_pieces(&chunk) {
                let is_last = piece.finish_reason.is_some();

                if !piece.text.is_empty() {
                    // Record first token time
                    if first_token_time.is_none() {
                        first_token_time = Some(start.elapsed());
                    }
                    token_count += 1;

                    print!("{}", piece.text);
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                }

                if is_last {
                    finished_choices.insert(piece.choice_index);
                }
                let is_final = finished_choices.len() >= expected_choices;

                if !piece.text.is_empty() || is_last || piece.tool_calls.is_some() {
                    let mapped = match piece.finish_reason.as_deref() {
                        // A cancellation is a failure, not a finish reason:
                        // reporting it as one would claim the model stopped on
                        // its own.
                        Some(reason) => Some(map_finish_reason(reason)?),
                        None => None,
                    };
                    callback(TokenUpdate {
                        choice_index: piece.choice_index,
                        text: piece.text,
                        is_last,
                        is_final,
                        finish_reason: mapped,
                        usage: if is_last { latest_usage } else { None },
                        tool_calls: piece.tool_calls,
                        logprobs: piece.logprobs,
                    })?;
                }

                if is_final {
                    saw_final = true;
                    println!("\nGeneration complete!");
                    break 'chunks;
                }
            }
        }

        if !saw_final {
            let message = format!(
                "Generation stream ended without a finish reason ({} tokens received)",
                token_count
            );
            eprintln!("{}", message);
            return Err(message);
        }

        // Log performance metrics
        let total_time = start.elapsed();
        if let Some(ttft) = first_token_time {
            println!("Time to first token (TTFT): {:?}", ttft);
        }
        println!("Total tokens: {}", token_count);
        println!("Total time: {:?}", total_time);
        println!(
            "Throughput: {:.2} tokens/sec",
            token_count as f64 / total_time.as_secs_f64()
        );

        Ok(GenerationOutcome::Completed)
    }

    pub fn is_loaded(&self) -> bool {
        self.model.is_some()
    }

    /// Drop the resident model, freeing its memory.
    pub fn unload(&mut self) {
        self.model = None;
        self.model_path = None;
    }

    /// The file the resident model was loaded from; None when nothing is loaded.
    pub fn loaded_model_path(&self) -> Option<&Path> {
        self.model.as_ref().and(self.model_path.as_deref())
    }

    /// Whether `model_name` is already loaded in memory - guards against
    /// reloading (and briefly double-allocating GPU/Metal memory for) a
    /// model the server asks for again on every fresh connection, even
    /// though pausing/resuming sharing never unloads it (by design: the
    /// model stays warm so resuming is instant).
    pub fn is_model_loaded_and_matches(&self, model_name: &str) -> bool {
        self.model.is_some()
            && self
                .model_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|f| f == std::ffi::OsStr::new(model_name))
                .unwrap_or(false)
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Get the local path where a model would be cached
    pub fn get_cached_model_path(&self, model_name: &str) -> PathBuf {
        self.cache_dir.join(model_name)
    }

    /// Check if a model exists in cache
    pub fn is_model_cached(&self, model_name: &str) -> bool {
        let path = self.get_cached_model_path(model_name);
        path.exists()
    }

    /// Download a model from a URL to the cache directory
    pub async fn download_model<F>(
        &self,
        url: &str,
        model_name: &str,
        progress_callback: F,
    ) -> Result<PathBuf, String>
    where
        F: Fn(DownloadProgress) + Send + 'static,
    {
        self.download_model_with_timeouts(
            url,
            model_name,
            DownloadTimeouts::default(),
            progress_callback,
        )
        .await
    }

    pub async fn download_model_with_timeouts<F>(
        &self,
        url: &str,
        model_name: &str,
        timeouts: DownloadTimeouts,
        progress_callback: F,
    ) -> Result<PathBuf, String>
    where
        F: Fn(DownloadProgress) + Send + 'static,
    {
        download_model_to(
            &self.cache_dir,
            url,
            model_name,
            timeouts,
            progress_callback,
        )
        .await
    }
}

// Thread-safe global model manager
pub type SharedModelManager = Arc<Mutex<ModelManager>>;

pub fn create_model_manager(cache_dir: PathBuf) -> SharedModelManager {
    Arc::new(Mutex::new(ModelManager::new(cache_dir)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mistralrs::Delta;
    use mistralrs::{
        ChatCompletionResponse, Choice, RequestLike, ResponseMessage, TopLogprob, Usage,
    };

    #[test]
    fn describe_prompt_carries_lengths_not_content() {
        let line = describe_prompt("my secret prompt", 64);
        assert!(!line.contains("secret"));
        assert!(line.contains("16 chars"));
        assert!(line.contains("max_tokens 64"));
    }

    #[test]
    fn unload_leaves_nothing_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = ModelManager::new(dir.path().to_path_buf());
        manager.model_path = Some(dir.path().join("model.gguf"));

        manager.unload();

        assert!(manager.model.is_none());
        assert!(!manager.is_loaded());
        assert!(manager.loaded_model_path().is_none());
        assert!(!manager.is_model_loaded_and_matches("model.gguf"));
    }

    #[tokio::test]
    async fn download_model_to_skips_a_file_already_cached() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("model.gguf"), b"x").unwrap();

        let path = download_model_to(
            dir.path(),
            "http://127.0.0.1:9/unreachable",
            "model.gguf",
            DownloadTimeouts::default(),
            |_| {},
        )
        .await
        .unwrap();

        assert_eq!(path, dir.path().join("model.gguf"));
    }

    #[test]
    fn build_request_caps_generation_at_max_tokens() {
        // Regression guard: a model that never emits an EOS token would
        // generate forever without a hard cap.
        let options = GenerationOptions {
            max_tokens: Some(42),
            ..Default::default()
        };
        let mut request = build_request(&[ChatTurn::user("hello")], &options).unwrap();
        assert_eq!(request.take_sampling_params().max_len, Some(42));
    }

    fn turn(role: &str, content: &str) -> ChatTurn {
        ChatTurn {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// The rendered turns of a built request, as (role, text) pairs.
    fn rendered(messages: &[ChatTurn]) -> Vec<(String, String)> {
        use mistralrs::RequestLike;

        let request = build_request(messages, &GenerationOptions::default()).unwrap();
        request
            .messages_ref()
            .iter()
            .map(|entry| {
                let role = match entry.get("role") {
                    Some(mistralrs::MessageContent::Left(text)) => text.clone(),
                    other => panic!("unexpected role content: {:?}", other),
                };
                let content = match entry.get("content") {
                    Some(mistralrs::MessageContent::Left(text)) => text.clone(),
                    _ => String::new(),
                };
                (role, content)
            })
            .collect()
    }

    #[test]
    fn the_whole_conversation_reaches_the_chat_template() {
        // Regression guard: the node used to take messages.last() only, so a
        // multi-turn request lost its system prompt and all history.
        let turns = vec![
            turn("system", "You are terse."),
            turn("user", "Hi"),
            turn("assistant", "Hello."),
            turn("user", "Again"),
        ];

        assert_eq!(
            rendered(&turns),
            vec![
                ("system".to_string(), "You are terse.".to_string()),
                ("user".to_string(), "Hi".to_string()),
                ("assistant".to_string(), "Hello.".to_string()),
                ("user".to_string(), "Again".to_string()),
            ]
        );
    }

    #[test]
    fn the_default_system_prompt_is_added_only_when_the_caller_sent_none() {
        let rendered = rendered(&[ChatTurn::user("Hi")]);

        assert_eq!(
            rendered[0],
            ("system".to_string(), DEFAULT_SYSTEM_PROMPT.to_string())
        );
        assert_eq!(rendered.len(), 2);
    }

    #[test]
    fn a_callers_system_prompt_is_not_buried_under_ours() {
        // The old behaviour put the node's own system prompt first and the
        // caller's *inside* a user turn beneath it.
        let rendered = rendered(&[turn("system", "You are terse."), ChatTurn::user("Hi")]);

        assert_eq!(rendered.len(), 2);
        assert!(!rendered
            .iter()
            .any(|(_, text)| text == DEFAULT_SYSTEM_PROMPT));
    }

    #[test]
    fn developer_is_treated_as_system() {
        // OpenAI's own rename. The backend normalises it, but a node handed the
        // raw role must not fall through to "user" and re-introduce the bug.
        let rendered = rendered(&[turn("developer", "You are terse."), ChatTurn::user("Hi")]);

        assert_eq!(rendered[0].0, "system");
        assert!(!rendered
            .iter()
            .any(|(_, text)| text == DEFAULT_SYSTEM_PROMPT));
    }

    #[test]
    fn an_unknown_role_is_kept_as_a_user_turn_rather_than_dropped() {
        // Dropping it would silently remove content the model was meant to see,
        // which is the failure mode this change exists to fix.
        let rendered = rendered(&[turn("moderator", "Be careful")]);

        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[1], ("user".to_string(), "Be careful".to_string()));
    }

    #[test]
    fn a_flattened_single_message_from_an_old_backend_still_works() {
        let rendered = rendered(&[ChatTurn::user("System: Be terse.\nUser: Hi")]);

        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[0].0, "system");
        assert_eq!(rendered[1].1, "System: Be terse.\nUser: Hi");
    }

    #[test]
    fn defaults_sample_at_openai_temperature_with_default_cap() {
        let params = GenerationOptions::default().to_sampling_params();
        assert_eq!(params.temperature, Some(DEFAULT_TEMPERATURE));
        assert_eq!(params.top_k, None);
        assert_eq!(params.max_len, Some(DEFAULT_MAX_TOKENS));
        assert!(params.stop_toks.is_none());
    }

    #[test]
    fn zero_temperature_is_greedy() {
        let params = GenerationOptions {
            temperature: Some(0.0),
            ..Default::default()
        }
        .to_sampling_params();
        assert_eq!(params.temperature, None);
        assert_eq!(params.top_k, Some(1));
    }

    #[test]
    fn greedy_keeps_the_local_generate_behavior() {
        let params = GenerationOptions::greedy(100).to_sampling_params();
        assert_eq!(params.top_k, Some(1));
        assert_eq!(params.max_len, Some(100));
    }

    #[test]
    fn forwards_every_sampling_option() {
        let params = GenerationOptions {
            temperature: Some(0.7),
            top_p: Some(0.9),
            max_tokens: Some(64),
            stop: vec!["\n\n".to_string(), "END".to_string()],
            frequency_penalty: Some(0.5),
            presence_penalty: Some(-0.5),
            ..Default::default()
        }
        .to_sampling_params();

        assert_eq!(params.temperature, Some(0.7));
        assert_eq!(params.top_k, None);
        assert_eq!(params.top_p, Some(0.9));
        assert_eq!(params.max_len, Some(64));
        assert_eq!(params.frequency_penalty, Some(0.5));
        assert_eq!(params.presence_penalty, Some(-0.5));
        match params.stop_toks {
            Some(StopTokens::Seqs(seqs)) => assert_eq!(seqs, vec!["\n\n", "END"]),
            other => panic!("expected stop sequences, got {:?}", other),
        }
    }

    fn schema_of(constraint: Constraint) -> serde_json::Value {
        match constraint {
            Constraint::JsonSchema(schema) => schema,
            _ => panic!("expected a JSON schema constraint"),
        }
    }

    const TOOLS: &str = r#"[{"type":"function","function":{"name":"get_weather",
        "description":"Weather","parameters":{"type":"object",
        "properties":{"city":{"type":"string"}}}}}]"#;

    #[test]
    fn no_tools_means_the_request_carries_none() {
        use mistralrs::RequestLike;

        let mut request =
            build_request(&[ChatTurn::user("hi")], &GenerationOptions::default()).unwrap();

        assert!(request.take_tools().is_none());
    }

    #[test]
    fn openai_tool_definitions_parse_straight_into_mistralrs_tools() {
        let (tools, choice) = parse_tools(Some(TOOLS), None).unwrap().unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name, "get_weather");
        assert!(tools[0].function.parameters.is_some());
        // OpenAI's default when tools are present.
        assert!(matches!(choice, ToolChoice::Auto));
    }

    #[test]
    fn tool_choice_none_and_auto_are_understood() {
        let (_, none) = parse_tools(Some(TOOLS), Some(r#""none""#))
            .unwrap()
            .unwrap();
        let (_, auto) = parse_tools(Some(TOOLS), Some(r#""auto""#))
            .unwrap()
            .unwrap();

        assert!(matches!(none, ToolChoice::None));
        assert!(matches!(auto, ToolChoice::Auto));
    }

    #[test]
    fn a_named_tool_choice_forces_that_tool() {
        let raw = r#"{"type":"function","function":{"name":"get_weather"}}"#;

        let (_, choice) = parse_tools(Some(TOOLS), Some(raw)).unwrap().unwrap();

        match choice {
            ToolChoice::Tool(tool) => assert_eq!(tool.function.name, "get_weather"),
            _ => panic!("expected a forced tool choice"),
        }
    }

    #[test]
    fn unparseable_tools_are_an_error_not_a_silent_prose_answer() {
        // A half-working tools implementation is worse than an honest failure,
        // because clients retry-loop instead of failing.
        assert!(parse_tools(Some("not json"), None).is_err());
        assert!(parse_tools(Some(TOOLS), Some("not json")).is_err());
    }

    #[test]
    fn a_request_with_tools_carries_them_and_the_choice() {
        use mistralrs::RequestLike;

        let options = GenerationOptions {
            tools: Some(TOOLS.to_string()),
            tool_choice: Some(r#""none""#.to_string()),
            ..Default::default()
        };
        let mut request = build_request(&[ChatTurn::user("weather?")], &options).unwrap();

        let (tools, choice) = request.take_tools().unwrap();
        assert_eq!(tools.len(), 1);
        assert!(matches!(choice, ToolChoice::None));
    }

    #[test]
    fn an_assistant_tool_call_is_replayed_to_the_model() {
        // The model has to see that it asked before it sees the answer.
        let calls = r#"[{"id":"call_1","type":"function",
            "function":{"name":"get_weather","arguments":"{\"city\":\"Oslo\"}"}}]"#;

        let parsed = parse_tool_calls(calls).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "call_1");
        assert_eq!(parsed[0].function.name, "get_weather");
        assert_eq!(parsed[0].function.arguments, r#"{"city":"Oslo"}"#);
    }

    #[test]
    fn a_tool_reply_is_rendered_as_a_tool_turn_with_its_call_id() {
        let turns = vec![
            ChatTurn::user("weather?"),
            ChatTurn {
                role: "assistant".to_string(),
                content: String::new(),
                tool_calls: Some(
                    r#"[{"id":"call_1","type":"function",
                      "function":{"name":"get_weather","arguments":"{}"}}]"#
                        .to_string(),
                ),
                tool_call_id: None,
            },
            ChatTurn {
                role: "tool".to_string(),
                content: "sunny".to_string(),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
            },
        ];

        let rendered = rendered(&turns);

        // system (default) + user + assistant + tool
        assert_eq!(rendered.len(), 4);
        assert_eq!(rendered[3], ("tool".to_string(), "sunny".to_string()));
    }

    #[test]
    fn unparseable_tool_calls_on_a_message_are_an_error() {
        let turns = vec![ChatTurn {
            role: "assistant".to_string(),
            content: String::new(),
            tool_calls: Some("not json".to_string()),
            tool_call_id: None,
        }];

        assert!(build_request(&turns, &GenerationOptions::default()).is_err());
    }

    #[test]
    fn no_response_format_means_no_constraint() {
        assert!(matches!(constraint_for(None).unwrap(), Constraint::None));
    }

    #[test]
    fn json_object_constrains_to_any_object() {
        // That is exactly what json_object means, and it reuses the same
        // constrained-decoding path as json_schema rather than introducing a
        // second grammar dialect to get wrong.
        let constraint = constraint_for(Some(r#"{"type":"json_object"}"#)).unwrap();

        assert_eq!(schema_of(constraint), serde_json::json!({"type": "object"}));
    }

    #[test]
    fn json_schema_passes_the_callers_schema_through_verbatim() {
        let raw = r#"{"type":"json_schema","json_schema":{"name":"p",
            "schema":{"type":"object","properties":{"a":{"type":"integer"}},
            "required":["a"]}}}"#;

        assert_eq!(
            schema_of(constraint_for(Some(raw)).unwrap()),
            serde_json::json!({
                "type": "object",
                "properties": {"a": {"type": "integer"}},
                "required": ["a"],
            })
        );
    }

    #[test]
    fn an_unusable_response_format_is_an_error_not_silent_prose() {
        // Generating unconstrained would return prose where the caller asked
        // for JSON, and they could not tell that apart from a model ignoring
        // them. B3's error path carries this back as a 502.
        assert!(constraint_for(Some("not json")).is_err());
        assert!(constraint_for(Some(r#"{"type":"json_schema"}"#)).is_err());
    }

    #[test]
    fn a_constrained_request_carries_the_constraint() {
        use mistralrs::RequestLike;

        let options = GenerationOptions {
            response_format: Some(r#"{"type":"json_object"}"#.to_string()),
            ..Default::default()
        };
        let mut request = build_request(&[ChatTurn::user("hi")], &options).unwrap();

        assert!(matches!(
            request.take_constraint(),
            Constraint::JsonSchema(_)
        ));
    }

    #[test]
    fn an_unconstrained_request_carries_no_constraint() {
        use mistralrs::RequestLike;

        let mut request =
            build_request(&[ChatTurn::user("hi")], &GenerationOptions::default()).unwrap();

        assert!(matches!(request.take_constraint(), Constraint::None));
    }

    #[test]
    fn stop_and_length_pass_through_unchanged() {
        assert_eq!(map_finish_reason("stop").unwrap(), "stop");
        // The node's DEFAULT_MAX_TOKENS cap becomes visible here: a request
        // that omits max_tokens and runs out reports "length", which is the
        // truth and used to be reported as "stop".
        assert_eq!(map_finish_reason("length").unwrap(), "length");
        assert_eq!(map_finish_reason("tool_calls").unwrap(), "tool_calls");
    }

    #[test]
    fn a_cancellation_is_an_error_not_a_finish_reason() {
        // The sequence did not complete, so reporting it as a finish reason
        // would claim the model stopped on its own.
        assert!(map_finish_reason(FINISH_REASON_CANCELED).is_err());
    }

    #[test]
    fn an_unreachable_finish_reason_falls_back_to_stop() {
        // generated_image / generated_speech are not in OpenAI's enum and are
        // unreachable for a text GGUF; inventing an enum value would be worse.
        assert_eq!(map_finish_reason("generated_image").unwrap(), "stop");
        assert_eq!(map_finish_reason("generated_speech").unwrap(), "stop");
    }

    #[test]
    fn the_finish_reason_string_is_kept_not_reduced_to_a_bool() {
        // Regression guard: the extractor used to return finish_reason.is_some()
        // and throw the string away, which is why every response claimed "stop"
        // even when the token cap had been hit.
        let pieces = extract_chunk_pieces(&chunk(Some("hi"), Some("length")));
        assert_eq!(pieces[0].finish_reason.as_deref(), Some("length"));
    }

    #[test]
    fn usage_is_read_off_a_chunk_that_carries_it() {
        let response = chunk_with_usage(Some("hi"), Some("stop"), Some((11, 7)));

        assert_eq!(
            extract_chunk_usage(&response),
            Some(GenerationUsage {
                prompt_tokens: 11,
                completion_tokens: 7
            })
        );
    }

    #[test]
    fn logprobs_are_mapped_to_openais_shape() {
        // OpenAI's `bytes` is an array of integers, so the token text is
        // re-encoded here.
        let response = chunk_with_logprobs("Hi", -0.25, Some("Hey"));

        let json: serde_json::Value = serde_json::from_str(
            extract_chunk_pieces(&response)[0]
                .logprobs
                .as_ref()
                .unwrap(),
        )
        .unwrap();

        assert_eq!(json["token"], "Hi");
        // The engine's -0.25 is a base-10 log; OpenAI's field is a natural log,
        // so what reaches the caller is -0.25 × ln(10).
        let logprob = json["logprob"].as_f64().unwrap();
        assert!(
            (logprob - (-0.25 * std::f64::consts::LN_10)).abs() < 1e-6,
            "expected the natural-log conversion, got {logprob}"
        );
        assert_eq!(json["bytes"], serde_json::json!([72, 105]));
        assert_eq!(json["top_logprobs"][0]["token"], "Hey");
        assert_eq!(
            json["top_logprobs"][0]["bytes"],
            serde_json::json!([72, 101, 121])
        );
    }

    #[test]
    fn an_alternative_with_no_decoded_text_falls_back_to_its_token_id() {
        // mistralrs reports alternatives by token *id* with the decoded text in
        // `bytes`; emitting a number where a client expects a token string is
        // less misleading than emitting an empty one.
        let response = chunk_with_logprobs("Hi", -0.25, None);

        let json: serde_json::Value = serde_json::from_str(
            extract_chunk_pieces(&response)[0]
                .logprobs
                .as_ref()
                .unwrap(),
        )
        .unwrap();

        assert_eq!(json["top_logprobs"][0]["token"], "<token 4242>");
    }

    #[test]
    fn a_chunk_without_logprobs_reports_none() {
        assert_eq!(
            extract_chunk_pieces(&chunk(Some("hi"), None))[0].logprobs,
            None
        );
    }

    #[test]
    fn every_choice_in_a_chunk_is_extracted() {
        // Regression guard: this took choices.first(), so with n > 1 every
        // completion but the first was silently dropped.
        let response = chunk_with_choices(&[(0, "Hello"), (1, "Goodbye"), (2, "Hi")]);

        let pieces = extract_chunk_pieces(&response);

        assert_eq!(pieces.len(), 3);
        assert_eq!(
            pieces
                .iter()
                .map(|p| (p.choice_index, p.text.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "Hello"), (1, "Goodbye"), (2, "Hi")]
        );
    }

    #[test]
    fn a_choices_own_index_is_used_not_its_position() {
        // The engine sets ChunkChoice.index; trusting position would mis-attribute
        // a chunk that carries only some of the choices.
        let response = chunk_with_choices(&[(2, "third"), (0, "first")]);

        let pieces = extract_chunk_pieces(&response);

        assert_eq!(pieces[0].choice_index, 2);
        assert_eq!(pieces[1].choice_index, 0);
    }

    const WEB_SEARCH: &str = r#"{"search_context_size":"low",
        "user_location":{"type":"approximate","approximate":{"city":"Oslo",
        "country":"NO","region":"Oslo","timezone":"Europe/Oslo"}}}"#;

    #[tokio::test]
    async fn a_pre_cancelled_generation_does_not_even_load() {
        // Cheap guard that the cancel argument is threaded through at all: with
        // no model loaded this fails before the loop, but the signature and the
        // call site are what regress silently.
        let manager = ModelManager::new(std::env::temp_dir());
        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = manager
            .generate_streaming(
                &[ChatTurn::user("hi")],
                &GenerationOptions::default(),
                &cancel,
                |_| Ok(()),
            )
            .await;

        assert!(
            result.is_err(),
            "no model is loaded, so this cannot succeed"
        );
    }

    #[test]
    fn a_cancelled_outcome_is_distinct_from_a_completed_one() {
        // handle_chat_completion branches on this: a cancellation is not a
        // failure, because the backend already gave up on the request, so
        // nothing should be reported back for it.
        assert_ne!(GenerationOutcome::Cancelled, GenerationOutcome::Completed);
    }

    #[test]
    fn reasoning_effort_reaches_the_chat_message() {
        // RequestBuilder hardcodes reasoning_effort: None and exposes no setter,
        // so this is the whole reason ReasoningRequest exists.
        let options = GenerationOptions {
            reasoning_effort: Some("high".to_string()),
            ..Default::default()
        };
        let mut request = build_request(&[ChatTurn::user("think")], &options).unwrap();

        match request.take_messages() {
            RequestMessage::Chat {
                reasoning_effort, ..
            } => {
                assert_eq!(reasoning_effort, Some(ReasoningEffort::High));
            }
            _ => panic!("expected a chat message"),
        }
    }

    #[test]
    fn no_reasoning_effort_leaves_the_message_alone() {
        let mut request =
            build_request(&[ChatTurn::user("hi")], &GenerationOptions::default()).unwrap();

        match request.take_messages() {
            RequestMessage::Chat {
                reasoning_effort, ..
            } => {
                assert_eq!(reasoning_effort, None);
            }
            _ => panic!("expected a chat message"),
        }
    }

    #[test]
    fn the_three_expressible_efforts_map() {
        assert_eq!(map_reasoning_effort("low").unwrap(), ReasoningEffort::Low);
        assert_eq!(
            map_reasoning_effort("medium").unwrap(),
            ReasoningEffort::Medium
        );
        assert_eq!(map_reasoning_effort("high").unwrap(), ReasoningEffort::High);
    }

    #[test]
    fn an_effort_the_engine_cannot_express_is_an_error_not_a_rounding() {
        // The backend rejects these before they reach the node; rounding here
        // would hide a bug and charge the caller reasoning tokens they tried to
        // avoid.
        for effort in ["minimal", "none", "xhigh", "max"] {
            assert!(map_reasoning_effort(effort).is_err(), "{}", effort);
        }
    }

    #[test]
    fn wrapping_the_builder_preserves_everything_else() {
        // ReasoningRequest delegates the whole RequestLike surface; a missed
        // method would silently drop tools, a constraint or the sampling params.
        let options = GenerationOptions {
            reasoning_effort: Some("low".to_string()),
            response_format: Some(r#"{"type":"json_object"}"#.to_string()),
            tools: Some(TOOLS.to_string()),
            web_search_options: Some(WEB_SEARCH.to_string()),
            logprobs: true,
            max_tokens: Some(42),
            ..Default::default()
        };
        let mut request = build_request(&[ChatTurn::user("hi")], &options).unwrap();

        assert_eq!(request.messages_ref().len(), 2);
        assert!(RequestLike::return_logprobs(&request));
        assert!(matches!(
            request.take_constraint(),
            Constraint::JsonSchema(_)
        ));
        assert!(request.take_tools().is_some());
        assert!(request.take_web_search_options().is_some());
        assert_eq!(request.take_sampling_params().max_len, Some(42));
    }

    #[test]
    fn web_search_options_parse_straight_into_mistralrs_types() {
        use mistralrs::RequestLike;

        let options = GenerationOptions {
            web_search_options: Some(WEB_SEARCH.to_string()),
            ..Default::default()
        };
        let mut request = build_request(&[ChatTurn::user("news?")], &options).unwrap();

        let parsed = request
            .take_web_search_options()
            .expect("options should be set");
        assert!(matches!(
            parsed.search_context_size,
            Some(mistralrs::core::SearchContextSize::Low)
        ));
        match parsed.user_location {
            Some(mistralrs::core::WebSearchUserLocation::Approximate { approximate }) => {
                assert_eq!(approximate.city, "Oslo");
                assert_eq!(approximate.timezone, "Europe/Oslo");
            }
            None => panic!("expected a user location"),
        }
    }

    #[test]
    fn no_web_search_options_leaves_the_request_without_any() {
        use mistralrs::RequestLike;

        let mut request =
            build_request(&[ChatTurn::user("hi")], &GenerationOptions::default()).unwrap();

        assert!(request.take_web_search_options().is_none());
    }

    #[test]
    fn unparseable_web_search_options_are_an_error() {
        let options = GenerationOptions {
            web_search_options: Some("not json".to_string()),
            ..Default::default()
        };

        assert!(build_request(&[ChatTurn::user("hi")], &options).is_err());
    }

    #[test]
    fn logit_bias_reaches_the_sampler() {
        let bias = std::collections::HashMap::from([(1234u32, 5.0f32), (99u32, -100.0f32)]);

        let params = GenerationOptions {
            logit_bias: bias.clone(),
            ..Default::default()
        }
        .to_sampling_params();

        assert_eq!(params.logits_bias, Some(bias));
    }

    #[test]
    fn no_logit_bias_leaves_the_sampler_alone() {
        // An empty map is not the same as "bias nothing by zero": the engine
        // does extra work per token when logits_bias is Some.
        assert_eq!(
            GenerationOptions::default()
                .to_sampling_params()
                .logits_bias,
            None
        );
    }

    #[test]
    fn n_reaches_the_sampler() {
        let params = GenerationOptions {
            n: Some(3),
            ..Default::default()
        }
        .to_sampling_params();

        assert_eq!(params.n_choices, 3);
    }

    #[test]
    fn no_n_asks_the_sampler_for_one_choice() {
        assert_eq!(
            GenerationOptions::default().to_sampling_params().n_choices,
            1
        );
    }

    #[test]
    fn top_logprobs_reaches_the_sampler() {
        let params = GenerationOptions {
            top_logprobs: Some(5),
            ..Default::default()
        }
        .to_sampling_params();

        assert_eq!(params.top_n_logprobs, 5);
    }

    #[test]
    fn no_top_logprobs_asks_the_sampler_for_none() {
        assert_eq!(
            GenerationOptions::default()
                .to_sampling_params()
                .top_n_logprobs,
            0
        );
    }

    #[test]
    fn tool_calls_serialise_to_openais_wire_shape() {
        // ToolCallResponse already serialises to exactly that shape (its `tp`
        // field is renamed to "type"), so no translation layer is needed.
        let response = chunk_with_tool_calls("get_weather", r#"{"city":"Oslo"}"#);

        let json: serde_json::Value = serde_json::from_str(
            extract_chunk_pieces(&response)[0]
                .tool_calls
                .as_ref()
                .unwrap(),
        )
        .unwrap();

        assert_eq!(json[0]["type"], "function");
        assert_eq!(json[0]["id"], "call_1");
        assert_eq!(json[0]["function"]["name"], "get_weather");
        assert_eq!(json[0]["function"]["arguments"], r#"{"city":"Oslo"}"#);
    }

    #[test]
    fn a_chunk_with_no_tool_calls_reports_none() {
        assert_eq!(
            extract_chunk_pieces(&chunk(Some("hi"), None))[0].tool_calls,
            None
        );
    }

    #[test]
    fn tool_calls_arrive_with_the_tool_calls_finish_reason() {
        // The engine sets finish_reason "tool_calls" on the same chunk it emits
        // the calls on, so the two never have to be correlated.
        assert_eq!(map_finish_reason("tool_calls").unwrap(), "tool_calls");
    }

    #[test]
    fn a_chunk_without_usage_reports_none_rather_than_zeroes() {
        // Zeroes would be indistinguishable from a real measurement of nothing,
        // and the backend would pass them on as fact.
        assert_eq!(extract_chunk_usage(&chunk(Some("hi"), Some("stop"))), None);
    }

    fn chunk_with_choices(choices: &[(usize, &str)]) -> Response {
        let mut response = chunk(Some("ignored"), None);
        if let Response::Chunk(ref mut chunk) = response {
            chunk.choices = choices
                .iter()
                .map(|(index, text)| ChunkChoice {
                    finish_reason: None,
                    index: *index,
                    delta: Delta {
                        content: Some(text.to_string()),
                        role: "assistant".to_string(),
                        tool_calls: None,
                        reasoning_content: None,
                    },
                    logprobs: None,
                })
                .collect();
        }
        response
    }

    fn chunk_with_logprobs(token: &str, logprob: f32, alternative: Option<&str>) -> Response {
        let mut response = chunk(Some(token), None);
        if let Response::Chunk(ref mut chunk) = response {
            chunk.choices[0].logprobs = Some(mistralrs::core::ResponseLogprob {
                token: token.to_string(),
                logprob,
                bytes: Some(token.as_bytes().to_vec()),
                top_logprobs: vec![mistralrs::core::TopLogprob {
                    token: 4242,
                    logprob,
                    bytes: alternative.map(|a| a.to_string()),
                }],
            });
        }
        response
    }

    fn chunk_with_tool_calls(name: &str, arguments: &str) -> Response {
        let mut response = chunk(None, Some("tool_calls"));
        if let Response::Chunk(ref mut chunk) = response {
            chunk.choices[0].delta.tool_calls = Some(vec![ToolCallResponse {
                index: 0,
                id: "call_1".to_string(),
                tp: ToolCallType::Function,
                function: CalledFunction {
                    name: name.to_string(),
                    arguments: arguments.to_string(),
                },
            }]);
        }
        response
    }

    fn chunk_with_usage(
        content: Option<&str>,
        finish_reason: Option<&str>,
        usage: Option<(usize, usize)>,
    ) -> Response {
        let mut response = chunk(content, finish_reason);
        if let (Response::Chunk(ref mut chunk), Some((prompt, completion))) = (&mut response, usage)
        {
            chunk.usage = Some(mistralrs::Usage {
                prompt_tokens: prompt,
                completion_tokens: completion,
                total_tokens: prompt + completion,
                avg_tok_per_sec: 0.0,
                avg_prompt_tok_per_sec: 0.0,
                avg_compl_tok_per_sec: 0.0,
                total_time_sec: 0.0,
                total_prompt_time_sec: 0.0,
                total_completion_time_sec: 0.0,
            });
        }
        response
    }

    fn chunk(content: Option<&str>, finish_reason: Option<&str>) -> Response {
        Response::Chunk(ChatCompletionChunkResponse {
            id: "test".to_string(),
            choices: vec![ChunkChoice {
                finish_reason: finish_reason.map(|s| s.to_string()),
                index: 0,
                delta: Delta {
                    content: content.map(|s| s.to_string()),
                    role: "assistant".to_string(),
                    tool_calls: None,
                    reasoning_content: None,
                },
                logprobs: None,
            }],
            created: 0,
            model: "test-model".to_string(),
            system_fingerprint: "local".to_string(),
            object: "chat.completion.chunk".to_string(),
            usage: None,
        })
    }

    #[test]
    fn extracts_text_from_a_content_chunk() {
        let [piece] = &extract_chunk_pieces(&chunk(Some("hello"), None))[..] else {
            panic!("expected exactly one piece")
        };
        assert_eq!(piece.text, "hello");
        assert_eq!(piece.finish_reason, None);
        assert_eq!(piece.choice_index, 0);
    }

    #[test]
    fn terminal_chunk_with_no_content_is_still_marked_last() {
        // Regression test: a chunk carrying `finish_reason` with `content:
        // None` used to be silently dropped, so `generate_streaming` never
        // learned generation had finished.
        let [piece] = &extract_chunk_pieces(&chunk(None, Some("stop")))[..] else {
            panic!("expected exactly one piece")
        };
        assert_eq!(piece.text, "");
        assert_eq!(piece.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn terminal_chunk_can_carry_both_content_and_finish_reason() {
        let [piece] = &extract_chunk_pieces(&chunk(Some("!"), Some("stop")))[..] else {
            panic!("expected exactly one piece")
        };
        assert_eq!(piece.text, "!");
        assert_eq!(piece.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn chunk_with_no_choices_yields_nothing() {
        let response = Response::Chunk(ChatCompletionChunkResponse {
            id: "test".to_string(),
            choices: vec![],
            created: 0,
            model: "test-model".to_string(),
            system_fingerprint: "local".to_string(),
            object: "chat.completion.chunk".to_string(),
            usage: None,
        });
        assert!(extract_chunk_pieces(&response).is_empty());
    }

    #[test]
    fn non_chunk_response_yields_nothing() {
        let response = Response::Done(ChatCompletionResponse {
            id: "test".to_string(),
            choices: vec![],
            created: 0,
            model: "test-model".to_string(),
            system_fingerprint: "local".to_string(),
            object: "chat.completion".to_string(),
            usage: Usage {
                completion_tokens: 0,
                prompt_tokens: 0,
                total_tokens: 0,
                avg_tok_per_sec: 0.0,
                avg_prompt_tok_per_sec: 0.0,
                avg_compl_tok_per_sec: 0.0,
                total_time_sec: 0.0,
                total_prompt_time_sec: 0.0,
                total_completion_time_sec: 0.0,
            },
        });
        assert!(extract_chunk_pieces(&response).is_empty());
    }

    // --- what a stream that is not a chunk means -------------------------
    //
    // These cover the defect behind both failures a live probe found against
    // api.getnodea.com after the parity work shipped: a named `tool_choice`
    // failing 4/4 and `n: 2` failing 8/10, *both* reported as "Generation
    // stream ended without a finish reason". Every non-chunk response was
    // discarded, so the engine's own diagnosis never reached anyone.

    fn done(finish_reasons: &[(usize, &str)]) -> Response {
        Response::Done(ChatCompletionResponse {
            id: "test".to_string(),
            choices: finish_reasons
                .iter()
                .map(|(index, reason)| Choice {
                    finish_reason: reason.to_string(),
                    index: *index,
                    message: ResponseMessage {
                        content: Some("hi".to_string()),
                        role: "assistant".to_string(),
                        tool_calls: None,
                        reasoning_content: None,
                    },
                    logprobs: None,
                })
                .collect(),
            created: 0,
            model: "test-model".to_string(),
            system_fingerprint: "local".to_string(),
            object: "chat.completion".to_string(),
            usage: Usage {
                completion_tokens: 0,
                prompt_tokens: 0,
                total_tokens: 0,
                avg_tok_per_sec: 0.0,
                avg_prompt_tok_per_sec: 0.0,
                avg_compl_tok_per_sec: 0.0,
                total_time_sec: 0.0,
                total_prompt_time_sec: 0.0,
                total_completion_time_sec: 0.0,
            },
        })
    }

    #[test]
    fn a_chunk_is_not_the_end_of_anything() {
        assert_eq!(
            classify_stream_end(&chunk(Some("hi"), None)),
            StreamEnd::NotTerminal
        );
        // Not even the one carrying a finish reason: the chunk path handles
        // that, and treating it as terminal here would end the generation
        // before the other choices of an n > 1 request had reported.
        assert_eq!(
            classify_stream_end(&chunk(Some("hi"), Some("stop"))),
            StreamEnd::NotTerminal
        );
    }

    #[test]
    fn the_aggregate_response_ends_the_generation() {
        // `Done` is how a generation legitimately finishes. Discarding it was
        // why a request whose choices finished in different chunks was
        // reported as a failure.
        assert_eq!(
            classify_stream_end(&done(&[(0, "stop")])),
            StreamEnd::Completed {
                finish_reasons: vec![(0, "stop".to_string())]
            }
        );
    }

    #[test]
    fn the_aggregate_response_carries_every_choice() {
        assert_eq!(
            classify_stream_end(&done(&[(0, "stop"), (1, "length")])),
            StreamEnd::Completed {
                finish_reasons: vec![(0, "stop".to_string()), (1, "length".to_string())],
            }
        );
    }

    #[test]
    fn a_model_error_reports_what_the_engine_said() {
        let Response::Done(partial) = done(&[(0, "stop")]) else {
            unreachable!()
        };
        let response = Response::ModelError("no tokens were sampled".to_string(), partial);

        match classify_stream_end(&response) {
            StreamEnd::Failed(message) => {
                // The engine's words, not ours: a generic message is what made
                // two unrelated production failures look identical.
                assert!(message.contains("no tokens were sampled"), "{message}");
                assert!(message.contains("1 choice"), "{message}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn an_internal_error_reports_what_the_engine_said() {
        let response = Response::InternalError(Box::new(std::io::Error::other("kv cache full")));

        match classify_stream_end(&response) {
            StreamEnd::Failed(message) => assert!(message.contains("kv cache full"), "{message}"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn a_validation_error_reports_what_the_engine_said() {
        let response =
            Response::ValidationError(Box::new(std::io::Error::other("tool schema rejected")));

        match classify_stream_end(&response) {
            StreamEnd::Failed(message) => {
                assert!(message.contains("tool schema rejected"), "{message}")
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn an_unrelated_response_kind_is_not_a_failure() {
        // Speech, image and embedding responses cannot arrive on a chat
        // stream. Classifying one as Failed would turn a surprise into a
        // broken generation.
        let response = Response::Speech {
            pcm: std::sync::Arc::new(vec![0.0]),
            rate: 16_000,
            channels: 1,
        };
        assert_eq!(classify_stream_end(&response), StreamEnd::NotTerminal);
    }

    // --- logprob units ---------------------------------------------------

    #[test]
    fn logprobs_are_converted_from_base_ten_to_natural() {
        // mistralrs computes prob.log(10.0); OpenAI's logprob is ln. A
        // probability of 0.5 is log10 = -0.30103 and ln = -0.693147.
        let converted = openai_logprob_value(-std::f32::consts::LOG10_2);
        assert!(
            (converted - (-std::f32::consts::LN_2)).abs() < 1e-5,
            "expected ln(0.5), got {converted}"
        );
    }

    #[test]
    fn a_certain_token_stays_at_zero() {
        // log(1) is 0 in every base, so the conversion must not move it.
        assert_eq!(openai_logprob_value(0.0), 0.0);
    }

    #[test]
    fn an_impossible_token_gets_openais_sentinel() {
        // log10(0) is -inf, and serde_json writes that as `null` - for a field
        // the schema types as a required number. Measured live: every
        // alternative of a top_logprobs request came back null.
        assert_eq!(openai_logprob_value(f32::NEG_INFINITY), -9999.0);
        assert_eq!(openai_logprob_value(f32::NAN), -9999.0);
    }

    #[test]
    fn no_logprob_serialises_as_null() {
        let logprob = mistralrs::core::ResponseLogprob {
            token: "hi".to_string(),
            logprob: f32::NEG_INFINITY,
            bytes: None,
            top_logprobs: vec![
                TopLogprob {
                    token: 1,
                    logprob: f32::NEG_INFINITY,
                    bytes: Some("a".to_string()),
                },
                TopLogprob {
                    token: 2,
                    logprob: -std::f32::consts::LOG10_2,
                    bytes: Some("b".to_string()),
                },
            ],
        };

        let json = serde_json::to_string(&openai_logprob(&logprob)).unwrap();

        assert!(
            !json.contains("null"),
            "a required number came back null: {json}"
        );
    }
}
