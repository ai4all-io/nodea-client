# CLAUDE.md

Guidance for working in `nodea-client` — the standalone Nodea desktop app
(Tauri + React frontend in `src/`, Rust backend in `src-tauri/`). Merge with
any user-level instructions.

## Incremental flow and testing

- Write unit tests for new code where applicable.
- Cover happy paths with integration tests where applicable.
- Ask clarifying questions before jumping into implementation.
- Work in incremental, independently shippable steps.

## Authentication

The frontend always passes authentication tokens via headers (see
`src/lib/api.ts`).

## Testing before commit

Run the tests for the code you touched before committing, and distinguish what
you ran locally from what CI runs:

- **Rust (`src-tauri/`)** — `make test-rust` (or `cargo test`). CI runs this in
  the `rust` job of `.github/workflows/ci.yml`.
- **Frontend (`src/`)** — the frontend end-to-end suite is Playwright
  (`npm test`, specs in `tests/`, `testDir: ./tests`). CI runs this in the
  `e2e` job. The `frontend` job type-checks and builds (`tsc && vite build`).
- When a change spans both surfaces (e.g. a Rust command the frontend calls),
  run both — a type-check pass does not prove the runtime wiring works.

Other CI jobs in `ci.yml`: `scripts` (pytest over `scripts/tests/`),
`licenses`, `fmt`, and `clippy`. A suite CI does not run is decorative — if you
add one, wire it into `ci.yml` in the same PR.

## mistralrs / mistralrs-quant (Metal release build)

`src-tauri/vendor/mistralrs-quant-0.8.1/` is a vendored, patched copy of the
`mistralrs-quant` crate, wired in via `[patch.crates-io]` in
`src-tauri/Cargo.toml`. This is deliberate — do not remove it or "clean up" the
vendor directory without reading this section.

**Why it's vendored:** `mistralrs-quant`'s own `build.rs` compiles the Metal
(GPU) kernels this app needs, and on this toolchain that build script has two
real bugs that only surface in a **production release build**
(`npm run tauri build` / `cargo build --release`), never in `cargo test` or
`tauri dev`:

1. The `.air -> .metallib` link step passed no `--sdk`/`-std` flags, so it
   silently targeted the wrong Metal version and dropped every kernel from the
   output — while still exiting 0. Deterministic, not a timing issue.
2. Independently: under a release build's heavier concurrent load, the same
   link step's subprocess can get resource-starved and produce a near-empty
   `.metallib` — also while exiting 0.

Either one leaves the model with no working GPU code for functions it needs
(symptom: every chat response comes back empty, with
`Error while loading function: fused_glu_float` or similar in the logs). The
vendored copy fixes (1) directly and adds a verify-and-retry safety net for (2)
(checks the compiled `.metallib` has a reasonable symbol count via `metal-nm`,
retries the link, and fails the build loudly instead of shipping a broken one).
It also skips building iOS/tvOS metallibs — this is a macOS-only app. Full
investigation and evidence: PR #161 in the upstream `ai4all-io/nodea` repo.

**If you bump the `mistralrs`/`mistralrs-quant` version:**

1. Update the version in `src-tauri/Cargo.toml` (`mistralrs = "0.X"`).
2. Pull the new version's source (temporarily remove the `[patch.crates-io]`
   entry, `cargo build` once to fetch it into
   `~/.cargo/registry/src/index.crates.io-*/mistralrs-quant-<version>/`, then
   copy that directory into `src-tauri/vendor/`).
3. Re-apply the patches to the new `build.rs` — search the old vendored
   `build.rs` for `NODEA PATCH` comments (on the `.metal -> .air` step, the
   `.air -> .metallib` step's `--sdk`/`-std` flags, and the verify-and-retry
   wrapper) and port them over. Drop the iOS/tvOS `compile(...)` calls the same
   way.
4. Update the path in `[patch.crates-io]` to the new vendored directory.
5. **Verify on a real production build before merging** — `npm run tauri build`,
   then run an actual chat message through the built `.app`. `cargo test` and
   `tauri dev` cannot catch either bug.
6. If a future version fixes `build.rs` upstream, confirm with a real
   release-build test first, then the vendor directory and `[patch.crates-io]`
   entry can be deleted.

## No extra documentation by default

Generate only what was asked. Do not create README, docs, or explanation files
unless explicitly requested.
