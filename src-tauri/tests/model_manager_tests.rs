#[cfg(test)]
mod tests {
    use ai4all_tauri::model_manager::{ChatTurn, GenerationOptions, ModelManager};
    use tempfile::TempDir;

    #[test]
    fn test_model_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::new(temp_dir.path().to_path_buf());

        assert!(!manager.is_loaded());
    }

    #[test]
    fn test_cache_path_generation() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::new(temp_dir.path().to_path_buf());

        let cached_path = manager.get_cached_model_path("test-model.gguf");
        assert!(cached_path.to_string_lossy().contains("test-model.gguf"));
    }

    #[test]
    fn test_is_model_cached() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::new(temp_dir.path().to_path_buf());

        // Model doesn't exist yet
        assert!(!manager.is_model_cached("nonexistent.gguf"));

        // Create a dummy file
        let model_path = manager.get_cached_model_path("exists.gguf");
        std::fs::write(&model_path, b"dummy").unwrap();

        // Now it should exist
        assert!(manager.is_model_cached("exists.gguf"));
    }

    #[tokio::test]
    async fn test_load_model_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = ModelManager::new(temp_dir.path().to_path_buf());

        let result = manager.load_model("/nonexistent/path/model.gguf").await;
        assert!(result.is_err());
        assert!(!manager.is_loaded());
    }

    #[tokio::test]
    async fn test_generate_without_model() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::new(temp_dir.path().to_path_buf());

        let result = manager.generate("test prompt", 10).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not loaded"));
    }

    #[tokio::test]
    async fn test_generate_streaming_without_model() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::new(temp_dir.path().to_path_buf());

        let result = manager
            .generate_streaming(
                &[ChatTurn::user("test")],
                &GenerationOptions::greedy(10),
                &tokio_util::sync::CancellationToken::new(),
                |_| Ok(()),
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not loaded"));
    }

    // Integration test (requires actual GGUF model file)
    #[tokio::test]
    #[ignore] // Run with: cargo test -- --ignored
    async fn test_load_and_generate_with_real_model() {
        // This test requires a real GGUF model file
        // Set GGUF_MODEL_PATH environment variable to test with real model
        if let Ok(model_path) = std::env::var("GGUF_MODEL_PATH") {
            let temp_dir = TempDir::new().unwrap();
            let mut manager = ModelManager::new(temp_dir.path().to_path_buf());

            // Load model
            manager
                .load_model(&model_path)
                .await
                .expect("Failed to load model");
            assert!(manager.is_loaded());

            // Generate text
            let result = manager.generate("Hello", 10).await;
            assert!(result.is_ok());

            let text = result.unwrap();
            assert!(!text.is_empty());
            println!("Generated: {}", text);
        } else {
            println!("Skipping real model test - set GGUF_MODEL_PATH to enable");
        }
    }

    // Integration test: downloads a real GGUF model (Llama-3.2-1B-Instruct,
    // ~0.7GB, cached across runs - the same model this app uses in production,
    // per grpc_client.rs's model download URL pattern) and drives
    // generate_streaming - the actual token-yielding component - across
    // several sequential prompts on the same loaded model. Catches
    // regressions where a generation call silently produces zero tokens/no
    // terminal signal, or - the actual root cause found in production -
    // cross-request KV-cache corruption from mistralrs's prefix cache, which
    // manifests as unrelated prompts starting with identical garbled tokens.
    //
    // IMPORTANT - macOS-only limitation: `MTLCreateSystemDefaultDevice` (how
    // candle-core picks a Metal device) is refused by macOS for any
    // non-interactive process - confirmed via `/usr/bin/log show`, which
    // reports "(Metal) Use of MTLCreateSystemDefaultDevice is not supported
    // for non-interactive (commandline or daemon) apps" - so a bare
    // `cargo test`/`cargo run` binary cannot create a Metal device at all: it
    // panics inside candle-core (`swap_remove index (is 0) should be < len
    // (is 0)`, since `Device::all()` comes back empty) on the very first
    // `Device::new_metal(0)` call, before this test's model even starts
    // loading. This previously misdiagnosed a *different* small model
    // (SmolLM2-135M) as having a GGUF-parsing bug - it was this same
    // restriction; SmolLM2 loads and runs fine once you're in a real
    // interactive process. To actually run this test on macOS, run it from
    // inside a genuine windowed process instead of bare `cargo test`:
    // `examples/real_repro_tauri.rs` does exactly that (a minimal Tauri
    // window + this same load/generate loop) and is the tool to reach for
    // when debugging this class of bug:
    //   GGUF_MODEL_PATH=/path/to/model.gguf cargo run --example real_repro_tauri
    // On Linux/CUDA this restriction doesn't apply and `cargo test --ignored`
    // should work directly.
    //
    // Run with: cargo test -- --ignored test_generate_streaming_survives_multiple_sequential_prompts
    #[tokio::test]
    #[ignore]
    async fn test_generate_streaming_survives_multiple_sequential_prompts() {
        const MODEL_URL: &str = "https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf";
        const MODEL_NAME: &str = "Llama-3.2-1B-Instruct-Q4_K_M.gguf";

        // Persistent (not TempDir) so repeat runs skip the download.
        let cache_dir = std::env::temp_dir().join("nodea-test-model-cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let mut manager = ModelManager::new(cache_dir);

        let model_path = manager
            .download_model(MODEL_URL, MODEL_NAME, |_| {})
            .await
            .expect("Failed to download model");

        manager
            .load_model(&model_path)
            .await
            .expect("Failed to load model");
        assert!(manager.is_loaded());

        let prompts = [
            "hi",
            "ok",
            "hello",
            "what is 2+2?",
            "tell me a short fact about the sea",
        ];
        let mut responses: Vec<(&str, String)> = Vec::new();

        for prompt in prompts {
            let mut chunks: Vec<(String, bool)> = Vec::new();

            manager
                .generate_streaming(
                    &[ChatTurn::user(prompt)],
                    &GenerationOptions::greedy(50),
                    &tokio_util::sync::CancellationToken::new(),
                    |update| {
                        chunks.push((update.text.clone(), update.is_last));
                        Ok(())
                    },
                )
                .await
                .unwrap_or_else(|e| {
                    panic!("generate_streaming failed for prompt {:?}: {}", prompt, e)
                });

            assert!(
                !chunks.is_empty(),
                "prompt {:?} produced no callback invocations at all",
                prompt
            );

            let last_count = chunks.iter().filter(|(_, is_last)| *is_last).count();
            assert_eq!(
                last_count, 1,
                "prompt {:?} should signal exactly one terminal chunk, got {}",
                prompt, last_count
            );
            assert!(
                chunks.last().unwrap().1,
                "prompt {:?}: the terminal chunk must be the last callback invocation",
                prompt
            );

            let full_text: String = chunks.iter().map(|(text, _)| text.as_str()).collect();
            assert!(
                !full_text.trim().is_empty(),
                "prompt {:?} produced only empty text",
                prompt
            );

            println!("prompt {:?} -> {:?}", prompt, full_text);
            responses.push((prompt, full_text));
        }

        // Regression guard for the actual root cause found in production:
        // mistralrs's sequence-level prefix cache matched unrelated requests
        // against a *different* completed sequence's cached KV state (all
        // requests here share the same static system prompt, which is enough
        // to trigger a match) and resumed generation from it. The observed
        // signature was unmistakable: two different prompts ("it worked!"
        // and "test something") produced the exact same garbled opening
        // tokens ("okud", "ia", ".") despite having nothing in common. Two
        // distinct prompts producing byte-identical output is never a
        // legitimate greedy-decoding outcome for a real model - it is the
        // fingerprint of cross-request state bleed.
        for i in 0..responses.len() {
            for j in (i + 1)..responses.len() {
                let (prompt_a, text_a) = &responses[i];
                let (prompt_b, text_b) = &responses[j];
                assert_ne!(
                    text_a, text_b,
                    "prompts {:?} and {:?} produced identical output ({:?}) - \
                     this is the signature of cross-request KV-cache contamination",
                    prompt_a, prompt_b, text_a
                );
            }
        }
    }
}
