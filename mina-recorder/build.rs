use std::process::Command;

fn main() {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    prost_build::compile_protos(
        &[
            "src/decode/meshsub.proto",
            "src/decode/kademlia.proto",
            "src/decode/structs.proto",
            "src/decode/envelope.proto",
            "src/decode/identify.proto",
        ],
        &["src/decode"],
    )
    .unwrap();

    capnpc::CompilerCommand::new()
        .file("libp2p_ipc.capnp")
        .run()
        .expect("compiling schema");
}
