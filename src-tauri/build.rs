fn main() {
    // Load environment variables based on profile, plus APP_ENV to pick between
    // local and development in debug builds (both use Cargo's "debug" profile).
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "local".to_string());
    println!("cargo:rerun-if-env-changed=APP_ENV");

    let env_file = if profile == "release" {
        "../.env.production"
    } else if app_env == "development" {
        "../.env.development"
    } else {
        "../.env.local"
    };

    // Load the appropriate .env file
    if let Err(e) = dotenv::from_filename(env_file) {
        eprintln!("Warning: Could not load {}: {}", env_file, e);
        eprintln!("Falling back to default values");
    } else {
        println!("cargo:rerun-if-changed={}", env_file);
        eprintln!("Loaded environment from: {}", env_file);
    }

    // Pass environment variables to compile-time
    if let Ok(grpc_server) = std::env::var("GRPC_SERVER") {
        eprintln!("Setting GRPC_SERVER to: {}", grpc_server);
        println!("cargo:rustc-env=GRPC_SERVER={}", grpc_server);
    } else {
        eprintln!("Warning: GRPC_SERVER not found in environment");
    }

    // Pass Supabase configuration to compile-time
    if let Ok(supabase_url) = std::env::var("SUPABASE_URL") {
        eprintln!("Setting SUPABASE_URL to: {}", supabase_url);
        println!("cargo:rustc-env=SUPABASE_URL={}", supabase_url);
    } else {
        eprintln!("Warning: SUPABASE_URL not found in environment");
    }

    if let Ok(supabase_anon_key) = std::env::var("SUPABASE_ANON_KEY") {
        eprintln!(
            "Setting SUPABASE_ANON_KEY (length: {})",
            supabase_anon_key.len()
        );
        println!("cargo:rustc-env=SUPABASE_ANON_KEY={}", supabase_anon_key);
    } else {
        eprintln!("Warning: SUPABASE_ANON_KEY not found in environment");
    }

    // Same backend REST API base URL the frontend's ApiClient uses
    if let Ok(api_base_url) = std::env::var("VITE_API_BASE_URL") {
        eprintln!("Setting VITE_API_BASE_URL to: {}", api_base_url);
        println!("cargo:rustc-env=VITE_API_BASE_URL={}", api_base_url);
    } else {
        eprintln!("Warning: VITE_API_BASE_URL not found in environment");
    }

    // A vendored protoc, so `git clone && cargo build` works with no
    // system protobuf-compiler on any platform in the release matrix.
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());

    tonic_build::configure()
        // The server half is generated so integration tests can stand up a
        // mock backend and exercise the real wire (tests/grpc_roundtrip_tests.rs).
        // It is unused by the app itself, which is only ever the client.
        .build_server(true)
        // service.proto uses proto3 `optional` fields for sampling params.
        // Older protoc versions (e.g. on CI) gate these behind this flag.
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile(&["../protos/service.proto"], &["../protos"])
        .unwrap_or_else(|e| panic!("protobuf compile error: {}", e));

    tauri_build::build()
}
