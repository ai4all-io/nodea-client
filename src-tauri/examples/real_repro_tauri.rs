// On-demand diagnostic for the "playground works once then goes silent"
// class of bug: reproduces the exact production code path -
// ModelManager::load_model followed by several sequential
// ModelManager::generate_streaming calls, exactly as grpc_client.rs's
// handle_chat_completion does per incoming request - inside a real Tauri
// window/event loop.
//
// This must run inside a real windowed process, not bare `cargo test`/`cargo
// run`: on macOS, `MTLCreateSystemDefaultDevice` (how candle-core picks a
// Metal device) is refused for any non-interactive process, so a bare
// binary panics on the very first `Device::new_metal(0)` call before the
// model even starts loading. See the comment on
// test_generate_streaming_survives_multiple_sequential_prompts in
// tests/model_manager_tests.rs for the full explanation.
//
// Usage:
//   GGUF_MODEL_PATH=/path/to/model.gguf cargo run --example real_repro_tauri
//
// A real window will briefly appear (Tauri needs one to run its event loop);
// output streams to stdout. The process exits itself once all prompts finish.
use ai4all_tauri::model_manager::{ChatTurn, GenerationOptions, ModelManager};
use std::path::PathBuf;

const MODEL_PATH_ENV: &str = "GGUF_MODEL_PATH";

fn main() {
    tauri::Builder::default()
        .setup(|_app| {
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    let model_path = std::env::var(MODEL_PATH_ENV)
                        .expect("set GGUF_MODEL_PATH to the .gguf file to load");
                    let mut manager = ModelManager::new(PathBuf::from("/tmp/nodea-repro-cache"));

                    println!("=== Loading model: {model_path} ===");
                    manager
                        .load_model(&model_path)
                        .await
                        .expect("model load failed");
                    println!("=== Model loaded ===");

                    let prompts = [
                        "testing this, tell me something",
                        "it worked!",
                        "test something",
                        "test something here",
                        "one more for good measure",
                    ];

                    for (i, prompt) in prompts.iter().enumerate() {
                        println!("\n=== Request {i}: {prompt:?} ===");
                        let mut token_count = 0usize;
                        let result = manager
                            .generate_streaming(
                                &[ChatTurn::user(*prompt)],
                                &GenerationOptions::default(),
                                &tokio_util::sync::CancellationToken::new(),
                                |update| {
                                    token_count += 1;
                                    println!(
                                        "  [req {i}] token#{token_count} text={:?} is_last={} \
                                         finish_reason={:?} usage={:?}",
                                        update.text,
                                        update.is_last,
                                        update.finish_reason,
                                        update.usage,
                                    );
                                    Ok(())
                                },
                            )
                            .await;

                        match result {
                            Ok(outcome) => println!(
                                "=== Request {i} finished {outcome:?}, {token_count} tokens ==="
                            ),
                            Err(e) => println!(
                                "=== Request {i} FAILED after {token_count} tokens: {e} ==="
                            ),
                        }
                    }

                    println!("\n=== ALL REQUESTS DONE ===");
                    std::process::exit(0);
                });
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
