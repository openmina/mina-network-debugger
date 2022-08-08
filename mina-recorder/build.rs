fn main() {
    prost_build::compile_protos(
        &[
            "src/connection/mina_protocol/meshsub.proto",
            "src/connection/mina_protocol/kademlia.proto",
        ],
        &["src/connection/mina_protocol"],
    )
    .unwrap();
}
