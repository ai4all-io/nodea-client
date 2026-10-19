// On-demand diagnostic for the "the reaper cancelled a node mid-switch" bug.
//
// The backend saw a node go silent for ~28s across a model switch and read
// that as a dead stream. The claim that followed - that the GGUF load blocks
// the async runtime and starves the 5s heartbeat task - was never measured,
// and the evidence is ambiguous: a *later*, larger load (qwen, 4.7GB,
// load_ms=11205) did NOT stop the heartbeats, while an earlier, smaller one
// (llama, 0.8GB) did. So before moving the Metal load path - which per
// CLAUDE.md and the spec's §16 cannot be validated by `cargo test` or
// `tauri dev` at all - find out whether it actually starves anything, and
// which phase does it.
//
// Ticks a 500ms timer on `tauri::async_runtime` (the runtime production
// spawns the heartbeat task on) while running the real switch sequence:
//
//     load(FIRST)  ->  load(SECOND)
//
// The second load is the interesting one: ModelManager::load_model calls
// self.unload() first, so it drops the resident model - a synchronous drop of
// several GB of Metal allocations - and then builds the next one. Point
// FIRST at the big model to reproduce the failing 16:12 sequence
// (unload qwen 7B, load llama 1B).
//
// A tick gap near 500ms means the runtime kept running and the heartbeat
// hypothesis is wrong. A gap of seconds names the phase that stalls.
//
// Must run inside a real windowed process, not bare `cargo run`: on macOS
// MTLCreateSystemDefaultDevice is refused for a non-interactive process, so a
// bare binary panics before the model loads. See real_repro_tauri.rs.
//
// Usage:
//   FIRST_GGUF=/path/to/qwen.gguf SECOND_GGUF=/path/to/llama.gguf \
//     cargo run --example heartbeat_during_load
use ai4all_tauri::model_manager::ModelManager;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(500);

fn main() {
    tauri::Builder::default()
        .setup(|_app| {
            // The ticker stands in for the heartbeat task: same runtime, same
            // "wake on a timer and do a trivial amount of work" shape.
            let worst_gap_ms = Arc::new(AtomicU64::new(0));
            let ticks = Arc::new(AtomicU64::new(0));
            let gap_for_ticker = worst_gap_ms.clone();
            let ticks_for_ticker = ticks.clone();

            tauri::async_runtime::spawn(async move {
                let mut last = Instant::now();
                loop {
                    tokio::time::sleep(TICK).await;
                    let now = Instant::now();
                    let gap = now.duration_since(last).as_millis() as u64;
                    last = now;
                    ticks_for_ticker.fetch_add(1, Ordering::Relaxed);
                    gap_for_ticker.fetch_max(gap, Ordering::Relaxed);
                    if gap > TICK.as_millis() as u64 * 2 {
                        println!(
                            "  [ticker] STALLED {gap}ms (expected ~{}ms)",
                            TICK.as_millis()
                        );
                    }
                }
            });

            tauri::async_runtime::spawn(async move {
                let first = std::env::var("FIRST_GGUF").expect("set FIRST_GGUF");
                let second = std::env::var("SECOND_GGUF").expect("set SECOND_GGUF");
                let mut manager = ModelManager::new(PathBuf::from("/tmp/nodea-hb-cache"));

                for (label, path) in [("FIRST", &first), ("SECOND", &second)] {
                    // Reset the watermark so each phase is attributed separately.
                    worst_gap_ms.store(0, Ordering::Relaxed);
                    let before = ticks.load(Ordering::Relaxed);
                    let started = Instant::now();
                    println!("\n=== {label}: load {path} ===");

                    manager.load_model(path).await.expect("model load failed");

                    let elapsed = started.elapsed();
                    let observed = ticks.load(Ordering::Relaxed) - before;
                    let expected = elapsed.as_millis() as u64 / TICK.as_millis() as u64;
                    println!(
                        "=== {label} done in {}ms | ticks {observed}/{expected} expected | \
                         worst gap {}ms ===",
                        elapsed.as_millis(),
                        worst_gap_ms.load(Ordering::Relaxed),
                    );
                    if observed * 2 < expected {
                        println!(
                            "=== {label}: the runtime WAS starved - a 5s heartbeat would have \
                             been missed ==="
                        );
                    } else {
                        println!("=== {label}: the ticker kept up; no starvation ===");
                    }
                }
                println!("\n=== DONE ===");
                std::process::exit(0);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
