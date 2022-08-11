use std::process::Command;

fn main() {
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    prost_build::compile_protos(
        &[
            "src/connection/mina_protocol/meshsub.proto",
            "src/connection/mina_protocol/kademlia.proto",
        ],
        &["src/connection/mina_protocol"],
    )
    .unwrap();
}
