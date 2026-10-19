# Nodea Tauri Desktop App

A cross-platform desktop application for the Nodea distributed AI inference platform.

> **Provenance.** This app was developed inside Nodea's private monorepo and
> is published here as a single squashed commit; its earlier history is not
> included.

## What this app does on your machine

Nodea Desktop turns your computer into an inference node for your
organization. Concretely:

- **Signs you in** with Supabase (email/password or Google). The access and
  refresh tokens are stored in `<app data>/.auth_tokens.json`.
- **Creates a random device ID** (`<app data>/.device_id`) and opens one gRPC
  stream to the Nodea backend. On connect it reports: device ID, hostname,
  OS and CPU architecture, app version, total/available/GPU memory, and which
  models it has cached. It then sends an empty heartbeat every 5 seconds.
- **Downloads models** the backend assigns from Hugging Face into
  `<app data>/models/`. Each model is under its own licence (e.g. Llama,
  Gemma, Qwen); using it is subject to that licence and its acceptable-use
  policy.
- **Runs inference locally** with mistral.rs. Requests from your
  organization are relayed to this machine **through Nodea's hosted
  backend**, and the generated tokens are streamed back the same way. The app
  does not log prompt or completion text.
- **Talks only to:** the Nodea API and gRPC endpoints, your Supabase project,
  and huggingface.co. No telemetry or analytics.
- **Does not auto-update.** New versions are manual downloads.

To stop sharing, toggle "Share compute" off or sign out.

The backend this app connects to is closed source and not self-hostable, so
without a Nodea account the app can be built and its UI explored (see
[tests/README.md](tests/README.md)), but the node will not serve anything.

## Features

- **Supabase Authentication**: Secure login with email/password and Google OAuth
- **Interactive Playground** (dev builds only): Test models with real-time markdown rendering
- **gRPC Streaming**: Bidirectional streaming connection to Nodea backend
- **Node**: Share this machine's compute with your organization and see its status
- **Fleet**: See the other devices connected to your organization
- **Invitations**: Join an organization from an invitation link
- **Local AI Inference**: Run models locally using mistralrs, with Metal acceleration on macOS

## Architecture

The app is built with:
- **Backend**: Rust (Tauri) with tonic for gRPC and mistralrs for local inference
- **Frontend**: React with TypeScript, TailwindCSS, and shadcn/ui components
- **Authentication**: Supabase with JWT (RS256)
- **Protocol**: Protocol Buffers (see `protos/service.proto`)

### Key Components

1. **src-tauri/src/auth.rs**: Supabase authentication with email/password and OAuth
2. **src-tauri/src/grpc_client.rs**: gRPC client with bidirectional streaming
3. **src-tauri/src/main.rs**: Tauri commands and state management
4. **src/**: React frontend with routing, API client, and UI components

## Prerequisites

- Rust 1.88+ (the vendored `mistralrs-quant` crate's minimum)
- Node.js 20+
- On Linux, the Tauri v1 system packages: `libgtk-3-dev libwebkit2gtk-4.0-dev
  libappindicator3-dev librsvg2-dev patchelf libssl-dev`. Ubuntu 24.04 dropped
  `libwebkit2gtk-4.0-dev`, so build on 22.04 (CI does, in a container).

Supported platforms: macOS on Apple Silicon is the primary target, and its
releases are signed and notarised. Linux (AppImage) and Windows (MSI) builds are
produced but unsigned and less tested.

`protoc` is **not** a prerequisite: `src-tauri/build.rs` compiles the proto with
a vendored binary (`protoc-bin-vendored`). See [docs/PROTO_SYNC.md](docs/PROTO_SYNC.md).

## Quick Start

```bash
npm install

# src-tauri/src/auth.rs reads SUPABASE_URL, SUPABASE_ANON_KEY and
# VITE_API_BASE_URL through the env!() macro, so the Rust crate does not
# compile at all until one of the .env files is in place.
cp .env.example .env.local

npm run tauri:local
```

`npm run dev` alone starts only the Vite dev server (the UI in a browser, in
demo mode), not the desktop app.

### Environment Configuration

Create `.env.local` for local development against a local backend:

```bash
cp .env.example .env.local
```

The defaults target a local stack:
- `GRPC_SERVER=http://localhost:50051`
- `SUPABASE_URL` / `VITE_SUPABASE_URL` and the matching anon keys point at a
  local Supabase CLI stack (`http://127.0.0.1:54321`, with the CLI's public demo
  key)

To point the app at a shared remote backend instead, edit `.env.development` with its host.


## Running the App

### Development Mode

```bash
npm run tauri:local  # local backend (default: `npm run tauri dev` also uses this)
npm run tauri:dev    # shared remote backend
```

This starts the Vite dev server and launches the Tauri app with hot-reload.

### Production Build

```bash
npm run build
npm run tauri build
```

The built app will be in `src-tauri/target/release/bundle/`.

### Releases

Releases are versioned `YY.M.X`: the year, the month, and a counter for that
month (`26.10.1`, `26.10.2`, then `26.11.1`). Running the **Release Desktop
App** workflow on `main` tags the next version, builds macOS, Linux and Windows
from that tag, and creates a draft GitHub Release with the bundles, a
`SHA256SUMS` file and build-provenance attestations. Publishing the draft makes
it the download at `releases/latest`. The version in `package.json`,
`tauri.conf.json` and `Cargo.toml` is a placeholder; the release build stamps
the real one in.

To check a download came from this repository:

```bash
gh attestation verify Nodea-mac.dmg --repo ai4all-io/nodea-client
```

## Testing

### Unit Tests (Rust)

```bash
cd src-tauri
cargo test
```

### Integration Tests (Playwright)

```bash
# Run tests headless
npm test

# Run with browser visible
npm run test:headed

# Open Playwright UI
npm run test:ui
```

## Development Notes

### Authentication Flow
- Supabase handles email/password and OAuth
- The Rust side keeps the access and refresh tokens in
  `<app data>/.auth_tokens.json`. When the UI runs in a plain browser instead
  (`npm run dev`), the Supabase JS client keeps its session in `localStorage`
- Tokens sent in HTTP headers and gRPC metadata
- Backend verifies using Supabase JWKS endpoint

### gRPC Connection
- Bidirectional streaming for device communication
- Automatic reconnection on disconnect
- Heartbeat messages maintain connection
- Status changes reflected in UI

### Local AI Inference
- Uses mistralrs for local model execution
- Metal acceleration on macOS, CPU elsewhere
- Model download and caching
- Progress tracking during download

## Project Structure

```
.
├── src/                    # React frontend
│   ├── components/        # UI components
│   ├── lib/               # API client and utilities
│   ├── pages/             # Route pages
│   └── App.tsx            # Main app component
├── src-tauri/             # Rust backend
│   ├── src/
│   │   ├── main.rs        # Entry point, Tauri commands and state
│   │   ├── lib.rs         # Library crate shared with tests
│   │   ├── auth.rs        # Supabase authentication
│   │   ├── grpc_client.rs # gRPC streaming client
│   │   └── proto/         # Generated protobuf files
│   ├── build.rs           # Build script (proto compilation)
│   ├── Cargo.toml         # Rust dependencies
│   └── tauri.conf.json    # Tauri configuration
├── tests/                 # Playwright tests - no backend required
├── protos/                # Vendored gRPC contract, see docs/PROTO_SYNC.md
│   └── service.proto
├── scripts/               # release job helpers (versioning, bundle selection) and their tests
├── .github/workflows/     # CI and the release matrix
├── package.json           # Node dependencies
└── vite.config.ts         # Vite configuration
```

## Related Documentation

- [Protobuf sync](docs/PROTO_SYNC.md) - How the gRPC contract stays in step
- [Test setup](tests/README.md) - Running the Playwright suite

## Third-party licences

The desktop bundle statically embeds its whole dependency tree - around 900 Rust
crates and 200 npm packages. It is almost entirely MIT and Apache-2.0, plus
BSD, ISC, Zlib, Unicode, CC0, 0BSD, OpenSSL, CDLA-Permissive-2.0 and a handful
of MPL-2.0 crates (`cssparser`, `selectors`, the `symphonia` codecs and a few
others). There is no GPL, AGPL, SSPL or BUSL. MPL-2.0 is file-level copyleft
and those files are unmodified, so it imposes nothing on this code.

Every licence text is in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md),
which is bundled into every build next to LICENSE and NOTICE. It is generated by
`make licenses` (`scripts/generate-third-party-licenses.sh`: `cargo about` for
the crates, `license-checker` for the npm tree). The `licenses` CI job fails if
it is stale or if a dependency brings a licence outside the allowlist in
`src-tauri/about.toml`.

`src-tauri/vendor/mistralrs-quant-0.8.1` is a **modified** copy of the MIT
`mistralrs-quant` crate, patched to fix its Metal build. Its licence and a
description of the changes are in that directory (`LICENSE`, `PATCHES.md`).

Models the app downloads are not part of this software and carry their own
licences.

## License

Apache License 2.0 - see [LICENSE](LICENSE) and [NOTICE](NOTICE).
