fn main() {
    prost_build::compile_protos(
        &["src/connection/mina_protocol/rpc.proto", "src/connection/mina_protocol/kad.proto"],
        &["src/connection/mina_protocol"],
    )
    .unwrap();
}
