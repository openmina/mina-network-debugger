fn main() {
    #[cfg(feature = "user")]
    build_bpf()
}

#[cfg(feature = "user")]
fn build_bpf() {
    use std::{env, process::Command};

    let target_dir = env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "../target".to_string());
    let target_dir = format!("{}/bpf", target_dir);

    let args = &[
        "+nightly-2022-10-10",
        "rustc",
        "--package=bpf-recorder",
        "--bin=bpf-recorder-kern",
        "--features=kern",
        "--no-default-features",
        "--target=bpfel-unknown-none",
        "-Z",
        "build-std=core",
        "--release",
        "--",
        "-Cdebuginfo=2",
        "-Clink-arg=--disable-memory-builtins",
        "-Clink-arg=--keep-btf",
    ];
    let output = Command::new("cargo")
        .env("CARGO_TARGET_DIR", &target_dir)
        .args(args)
        .output()
        .expect("failed to build bpf code");
    if !output.status.success() {
        let error = String::from_utf8(output.stderr).expect("malformed error message");
        panic!("{}", error);
    }
    Command::new("sed")
        .current_dir(&target_dir)
        .arg("-i")
        .arg("s/ty__/type/g")
        .arg("bpfel-unknown-none/release/bpf-recorder-kern")
        .output()
        .expect("failed to patch bpf object");

    println!(
        "cargo:rustc-env=BPF_CODE_RECORDER={}/bpfel-unknown-none/release/bpf-recorder-kern",
        target_dir
    );
    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/context.rs");
    println!("cargo:rerun-if-changed=src/send.rs");
}
