//! Build glue for Anchors: builds the LSP/CLI binary and the Zed extension
//! WASM so both distribution paths work from one command.
//!
//! Usage: cargo run -p xtask -- build
//! Copies `target/release/canc` and `extension/extension.wasm` into
//! `target/dist/` (next to the repo's `Cargo.toml`).

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "build".into());
    if arg != "build" {
        eprintln!("usage: cargo run -p xtask -- build [--release]");
        std::process::exit(2);
    }

    let release = std::env::args().nth(2).map(|s| s == "--release").unwrap_or(true);
    let profile = if release { "release" } else { "debug" };

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let dist = root.join("target").join("dist");
    std::fs::create_dir_all(&dist).expect("creating dist dir");

    // 1. Build the server binary.
    run(
        root.join("Cargo.toml").as_path(),
        &[
            "build".to_string(),
            "--release".to_string(),
            "--bin".to_string(),
            "canc".to_string(),
        ],
        &root,
    );
    let binary_src = root
        .join("target")
        .join("release")
        .join("canc");
    let binary_dst = dist.join("canc");
    if binary_src.exists() {
        std::fs::copy(&binary_src, &binary_dst)
            .expect("copying canc binary into dist/");
        eprintln!("built canc -> {}", binary_dst.display());
    } else {
        eprintln!("warning: {} not found (build failed?)", binary_src.display());
    }

    // 2. Build the extension WASM (needs wasm32-wasip2 target;
    //    extension/ is its own workspace).
    run(
        root.join("extension").join("Cargo.toml").as_path(),
        &[
            "build".to_string(),
            "--target".to_string(),
            "wasm32-wasip2".to_string(),
            if release { "--release".to_string() } else { String::new() },
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>(),
        &root,
    );
    let wasm_src = root
        .join("extension")
        .join("target")
        .join("wasm32-wasip2")
        .join(profile)
        .join("canc_zed.wasm");
    let wasm_dst = dist.join("extension.wasm");
    if wasm_src.exists() {
        std::fs::copy(&wasm_src, &wasm_dst)
            .expect("copying extension.wasm into dist/");
        eprintln!("built extension.wasm -> {}", wasm_dst.display());
    } else {
        eprintln!("warning: {} not found (wasm build failed?)", wasm_src.display());
    }
    eprintln!("done: {}/{{canc, extension.wasm}}", dist.display());
}

fn run(manifest: &std::path::Path, args: &[String], cwd: &std::path::Path) {
    let status = std::process::Command::new("cargo")
        .args(args)
        .arg("--manifest-path")
        .arg(manifest)
        .current_dir(cwd)
        .status()
        .expect("spawning cargo");
    if !status.success() {
        eprintln!("warning: cargo exited with {status}");
    }
}
