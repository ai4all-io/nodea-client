#[cfg(feature = "cuda")]
#[allow(unused)]
fn cuda_version_from_build_system() -> (usize, usize) {
    let output = std::process::Command::new("nvcc")
        .arg("--version")
        .output()
        .expect("Failed to execute `nvcc`");

    if !output.status.success() {
        panic!(
            "`nvcc --version` failed.\nstdout:\n{}\n\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let version_line = stdout.lines().nth(3).unwrap();
    let release_section = version_line.split(", ").nth(1).unwrap();
    let version_number = release_section.split(' ').nth(1).unwrap();

    match version_number {
        "13.2" => (13, 2),
        "13.1" => (13, 1),
        "13.0" => (13, 0),
        "12.9" => (12, 9),
        "12.8" => (12, 8),
        "12.6" => (12, 6),
        "12.5" => (12, 5),
        "12.4" => (12, 4),
        "12.3" => (12, 3),
        "12.2" => (12, 2),
        "12.1" => (12, 1),
        "12.0" => (12, 0),
        "11.8" => (11, 8),
        "11.7" => (11, 7),
        "11.6" => (11, 6),
        "11.5" => (11, 5),
        "11.4" => (11, 4),
        v => panic!("Unsupported cuda toolkit version: `{v}`. Please raise a github issue."),
    }
}

fn main() -> Result<(), String> {
    // Declare expected cfg values for check-cfg lint
    println!("cargo::rustc-check-cfg=cfg(has_marlin_kernels)");
    println!("cargo::rustc-check-cfg=cfg(has_blockwise_fp8_kernels)");
    println!("cargo::rustc-check-cfg=cfg(has_scalar_fp8_kernels)");
    println!("cargo::rustc-check-cfg=cfg(has_vector_fp8_kernels)");
    println!("cargo::rustc-check-cfg=cfg(has_mxfp4_kernels)");
    println!("cargo::rustc-check-cfg=cfg(has_mxfp4_wmma_kernels)");

    #[cfg(feature = "cuda")]
    {
        use std::{path::PathBuf, vec};
        const CUDA_NVCC_FLAGS: Option<&'static str> = option_env!("CUDA_NVCC_FLAGS");

        println!("cargo:rerun-if-changed=build.rs");
        let build_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

        let mut builder = cudaforge::KernelBuilder::new()
            .source_glob("kernels/*/*.cu")
            .out_dir(build_dir.clone())
            .arg("-std=c++17")
            .arg("-O3")
            .arg("-U__CUDA_NO_HALF_OPERATORS__")
            .arg("-U__CUDA_NO_HALF_CONVERSIONS__")
            .arg("-U__CUDA_NO_HALF2_OPERATORS__")
            .arg("-U__CUDA_NO_BFLOAT16_CONVERSIONS__")
            .arg("--expt-relaxed-constexpr")
            .arg("--expt-extended-lambda")
            .arg("--use_fast_math")
            .arg("--verbose")
            .arg("--compiler-options")
            .arg("-fPIC");

        let compute_cap = builder.get_compute_cap().unwrap_or(80);
        // ======== Handle optional kernel compilation via rustc-cfg flags
        let cc_over_80 = compute_cap >= 80;

        if cc_over_80 {
            println!("cargo:rustc-cfg=has_marlin_kernels");
            println!("cargo:rustc-cfg=has_blockwise_fp8_kernels");
            println!("cargo:rustc-cfg=has_scalar_fp8_kernels");
            println!("cargo:rustc-cfg=has_vector_fp8_kernels");
            // WMMA tensor core MXFP4 kernel (FP16/BF16 WMMA requires SM >= 80)
            println!("cargo:rustc-cfg=has_mxfp4_wmma_kernels");
        }
        // MXFP4 is always enabled with CUDA (uses LUT-based dequantization)
        println!("cargo:rustc-cfg=has_mxfp4_kernels");

        let excluded_files = if cc_over_80 {
            vec!["dummy_*.cu", "*_dummy.cu"]
        } else {
            vec!["marlin_*.cu", "*_fp8.cu", "*_fp8_gemm.cu", "*_wmma.cu"]
        };
        builder = builder.exclude(&excluded_files);

        // https://github.com/EricLBuehler/mistral.rs/issues/286
        if let Some(cuda_nvcc_flags_env) = CUDA_NVCC_FLAGS {
            builder = builder.arg("--compiler-options");
            builder = builder.arg(cuda_nvcc_flags_env);
        }

        let target = std::env::var("TARGET").unwrap();
        let build_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        // https://github.com/EricLBuehler/mistral.rs/issues/588
        let out_file = if target.contains("msvc") {
            // Windows case
            build_dir.join("mistralrsquant.lib")
        } else {
            build_dir.join("libmistralrsquant.a")
        };
        builder
            .build_lib(out_file)
            .expect("Build mistral quant lib failed!");
        println!("cargo:rustc-link-search={}", build_dir.display());
        println!("cargo:rustc-link-lib=mistralrsquant");
        println!("cargo:rustc-link-lib=dylib=cudart");

        if target.contains("msvc") {
            // nothing to link to
        } else if target.contains("apple")
            || target.contains("freebsd")
            || target.contains("openbsd")
        {
            println!("cargo:rustc-link-lib=dylib=c++");
        } else if target.contains("android") {
            println!("cargo:rustc-link-lib=dylib=c++_shared");
        } else {
            println!("cargo:rustc-link-lib=dylib=stdc++");
        }

        let (major, minor) = cuda_version_from_build_system();
        println!("cargo:rustc-cfg=feature=\"cuda-{major}0{minor}0\"");

        Ok(())
    }

    #[cfg(feature = "metal")]
    {
        use std::path::PathBuf;
        use std::process::Command;
        use std::{env, str};

        const METAL_SOURCES: [&str; 15] = [
            "bitwise",
            "blockwise_fp8",
            "bnb_dequantize",
            "f8q8",
            "fused_glu",
            "hqq_dequantize",
            "hqq_bitpack",
            "mxfp4",
            "quantized",
            "scalar_fp8",
            "scan",
            "sdpa_with_sinks",
            "softmax_with_sinks",
            "sort",
            "copy",
        ];
        const HEADER_SOURCES: [&str; 5] = ["utils", "bf16", "scan_impl", "sort_impl", "copy_impl"];
        // Include-only headers (not compiled directly, just tracked for changes)
        const INCLUDE_ONLY: [&str; 2] = ["float8", "float4"];
        for src in METAL_SOURCES {
            println!("cargo::rerun-if-changed=src/metal_kernels/{src}.metal");
        }
        for src in HEADER_SOURCES {
            println!("cargo::rerun-if-changed=src/metal_kernels/{src}.metal");
        }
        for src in INCLUDE_ONLY {
            println!("cargo::rerun-if-changed=src/metal_kernels/{src}.metal");
        }
        println!("cargo::rerun-if-changed=build.rs");

        // Check if precompilation should be skipped
        // https://github.com/EricLBuehler/mistral.rs/pull/1311#issuecomment-3001309885
        println!("cargo:rerun-if-env-changed=MISTRALRS_METAL_PRECOMPILE");
        let skip_precompile = env::var("MISTRALRS_METAL_PRECOMPILE")
            .map(|v| v == "0" || v.to_lowercase() == "false")
            .unwrap_or(false);

        if skip_precompile {
            println!(
                "cargo:warning=Skipping Metal kernel precompilation (MISTRALRS_METAL_PRECOMPILE=0)"
            );
            // Write a dummy metallib file to satisfy the include_bytes! macro
            let out_dir = PathBuf::from(std::env::var("OUT_DIR").map_err(|_| "OUT_DIR not set")?);
            std::fs::write(out_dir.join("mistralrs_quant.metallib"), []).unwrap();
            std::fs::write(out_dir.join("mistralrs_quant_ios.metallib"), []).unwrap();
            std::fs::write(out_dir.join("mistralrs_quant_tvos.metallib"), []).unwrap();
            return Ok(());
        }

        #[derive(Clone, Copy)]
        enum Platform {
            MacOS,
            Ios,
            TvOS,
        }

        impl Platform {
            fn sdk(&self) -> &str {
                match self {
                    Platform::MacOS => "macosx",
                    Platform::Ios => "iphoneos",
                    Platform::TvOS => "appletvos",
                }
            }

            fn metal_std(&self) -> &str {
                // Use Metal 3.1 unified standard for all platforms.
                // This fixes Xcode 26+ where the default Metal standard may be too low.
                // https://github.com/EricLBuehler/mistral.rs/issues/1844
                //
                // Note: tvOS devices with A15+ (Apple TV 4K 3rd gen) support Metal 3.1.
                match self {
                    Platform::MacOS | Platform::Ios | Platform::TvOS => "metal3.1",
                }
            }
        }

        // NODEA PATCH: verified on real hardware that xcrun metal can exit 0
        // (success) for the air->metallib link step while still producing an
        // essentially empty .metallib (92 bytes vs. a real ~30MB/~4000-symbol
        // one) - reproduced by manually re-running the exact same link
        // command against the exact same (valid, non-empty) .air inputs
        // outside of `cargo build`, which then linked correctly every time.
        // Points at the linker subprocess getting starved of resources by
        // everything else a release build is compiling in parallel at that
        // moment, without xcrun propagating that as a failing exit status.
        // Checking status alone (the earlier double-invocation fix) does not
        // catch this - it requires actually inspecting the output.
        fn verify_metallib_has_symbols(metallib: &std::path::Path) -> bool {
            // A real metallib for this crate has ~4000 symbols; a starved
            // link can produce a "successful" file with none at all. No
            // need to enumerate every expected kernel name - just check the
            // output isn't suspiciously close to empty.
            const MIN_EXPECTED_SYMBOLS: usize = 100;
            let output = match Command::new("xcrun")
                .arg("metal-nm")
                .arg(metallib)
                .output()
            {
                Ok(o) => o,
                Err(_) => return false,
            };
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).lines().count() >= MIN_EXPECTED_SYMBOLS
        }

        fn compile(platform: Platform) -> Result<(), String> {
            const MAX_ATTEMPTS: u32 = 3;
            let lib_name = match platform {
                Platform::MacOS => "mistralrs_quant.metallib",
                Platform::Ios => "mistralrs_quant_ios.metallib",
                Platform::TvOS => "mistralrs_quant_tvos.metallib",
            };

            for attempt in 1..=MAX_ATTEMPTS {
                let metallib = compile_once(platform)?;

                if verify_metallib_has_symbols(&metallib) {
                    return Ok(());
                }

                println!(
                    "cargo:warning=Compiled {lib_name} but it looks corrupted (too few Metal \
                     kernel symbols found) - the Metal compiler can silently produce an \
                     incomplete .metallib while still reporting success when starved of \
                     resources by everything else this build is compiling at the same time. \
                     Retrying ({attempt}/{MAX_ATTEMPTS})..."
                );
                std::thread::sleep(std::time::Duration::from_secs(3));
            }

            Err(format!(
                "Metal kernel compilation for {lib_name} kept producing a corrupted .metallib \
                 (missing symbols) after {MAX_ATTEMPTS} attempts - this isn't a transient \
                 resource-contention issue, something else is wrong"
            ))
        }

        fn compile_once(platform: Platform) -> Result<PathBuf, String> {
            let current_dir = env::current_dir().expect("Failed to get current directory");
            let out_dir = PathBuf::from(std::env::var("OUT_DIR").map_err(|_| "OUT_DIR not set")?);
            let working_directory = out_dir.to_string_lossy().to_string();
            let sources = current_dir.join("src").join("metal_kernels");

            // Compile metal to air
            let mut compile_air_cmd = Command::new("xcrun");
            compile_air_cmd
                .arg("--sdk")
                .arg(platform.sdk())
                .arg("metal")
                .arg(format!("-std={}", platform.metal_std()))
                .arg(format!("-working-directory={working_directory}"))
                .arg("-Wall")
                .arg("-Wextra")
                .arg("-O3")
                .arg("-c")
                .arg("-w");
            for metal_file in METAL_SOURCES {
                compile_air_cmd.arg(sources.join(format!("{metal_file}.metal")));
            }
            for metal_file in HEADER_SOURCES {
                compile_air_cmd.arg(sources.join(format!("{metal_file}.metal")));
            }
            // NODEA PATCH: this used to spawn+wait compile_air_cmd here, throw
            // away that first run's exit status entirely (only I/O errors from
            // spawn/wait were checked, never status.success()), then spawn the
            // exact same command a second time into the same output directory
            // and check *that* run's status instead. Confirmed on real
            // hardware: a resource-starved first invocation (more likely
            // under a release build's much heavier concurrent codegen load)
            // can leave a function (fused_glu_float, seen on this machine)
            // missing from the final metallib despite the build reporting
            // success, because nothing checks whether the first run actually
            // completed cleanly before silently proceeding to the second.
            // Single invocation, status actually checked.
            let mut child = compile_air_cmd.spawn().expect("Failed to compile air");

            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        panic!("Compiling metal -> air failed. Exit with status: {status}")
                    }
                }
                Ok(None) => {
                    let status = child
                        .wait()
                        .expect("Compiling metal -> air failed while waiting for result");
                    if !status.success() {
                        panic!("Compiling metal -> air failed. Exit with status: {status}")
                    }
                }
                Err(e) => panic!("Compiling metal -> air failed: {e:?}"),
            }

            // Compile air to metallib
            let lib_name = match platform {
                Platform::MacOS => "mistralrs_quant.metallib",
                Platform::Ios => "mistralrs_quant_ios.metallib",
                Platform::TvOS => "mistralrs_quant_tvos.metallib",
            };
            let metallib = out_dir.join(lib_name);
            let mut compile_metallib_cmd = Command::new("xcrun");
            // NODEA PATCH: this used to pass no --sdk/-std flags at all, so
            // this link step's target AIR version came from whatever
            // xcrun/clang falls back to by default (observed: AIR 2.0,
            // apparently derived from an ambient env var like
            // MACOSX_DEPLOYMENT_TARGET that `cargo build` sets for build
            // scripts) - a real, deterministic mismatch against the AIR
            // version the compile step above actually produced (2.6/2.7,
            // from its explicit -std=metal3.1), silently dropping every
            // single input file ("AIR version is bigger than the one of the
            // target being linked") and producing a valid-looking but
            // symbol-less .metallib despite exiting 0. Passing the same
            // --sdk/-std here that the compile step used keeps both stages
            // consistent regardless of the ambient environment.
            compile_metallib_cmd
                .arg("--sdk")
                .arg(platform.sdk())
                .arg("metal")
                .arg(format!("-std={}", platform.metal_std()))
                .arg("-o")
                .arg(&metallib);

            for metal_file in METAL_SOURCES {
                compile_metallib_cmd.arg(out_dir.join(format!("{metal_file}.air")));
            }
            for metal_file in HEADER_SOURCES {
                compile_metallib_cmd.arg(out_dir.join(format!("{metal_file}.air")));
            }

            let mut child = compile_metallib_cmd
                .spawn()
                .expect("Failed to compile air -> metallib");

            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        panic!("Compiling air -> metallib failed. Exit with status: {status}")
                    }
                }
                Ok(None) => {
                    let status = child
                        .wait()
                        .expect("Compiling air -> metallib failed while waiting for result");
                    if !status.success() {
                        panic!("Compiling air -> metallib failed. Exit with status: {status}")
                    }
                }
                Err(e) => panic!("Compiling air -> metallib failed: {e:?}"),
            }

            Ok(metallib)
        }

        // NODEA PATCH: this app is macOS-only (Cargo.toml gates the "metal"
        // feature to `cfg(target_os = "macos")`) - the iOS/tvOS metallibs
        // built below were never used. Also found to be genuinely broken on
        // this toolchain independent of anything above: the link step
        // produces "AIR version (2.7) is bigger than the one of the target
        // being linked (2.6)" for every input file, because it passes no
        // --sdk/-std flags of its own and so links against a stale default
        // target version mismatched with what the compile step (which does
        // pass -std=metal3.1) actually produced - a real, deterministic bug,
        // not the resource-starvation one verify_metallib_has_symbols exists
        // for. Not worth fixing for platforms we don't ship; skip them.
        compile(Platform::MacOS)?;

        Ok(())
    }

    #[cfg(not(any(feature = "metal", feature = "cuda")))]
    Ok(())
}
