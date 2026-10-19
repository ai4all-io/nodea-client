# Local modifications

This is `mistralrs-quant` 0.8.1 from crates.io (MIT, Copyright (c) 2024 Eric
Buehler; see `LICENSE`), wired in through `[patch.crates-io]` in
`src-tauri/Cargo.toml`. Only `build.rs` differs from the published crate. Each
change is marked with a `NODEA PATCH` comment there.

1. **Metal compile step runs once and its status is checked.** Upstream ran the
   `.metal -> .air` compile twice and discarded the first run's exit status.
2. **The `.air -> .metallib` link step passes `--sdk` and `-std=metal3.1`**, the
   same as the compile step. Without them it links against a default AIR
   version and drops every kernel, while still exiting 0.
3. **The linked `.metallib` is verified and the link retried.** A metallib with
   fewer than 100 symbols (as produced by a resource-starved link during a
   release build) is rejected, the link is retried, and the build fails loudly
   if it never succeeds.
4. **Only the macOS metallib is built.** The app ships on macOS only for Metal,
   and the iOS/tvOS links were broken on this toolchain.

Upstream: https://github.com/EricLBuehler/mistral.rs
