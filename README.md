# Mina Network Debugger

(readme for developers)

## Build

See dockerfile.

Most likely developer machine already have all deps.

Need rust `nightly-2022-10-10`. And bpf linker.

```
rustup update nightly-2022-10-10
rustup component add rust-src --toolchain nightly-2022-10-10-x86_64-unknown-linux-gnu
cargo install bpf-linker --git https://github.com/vlad9486/bpf-linker --branch keep-btf
```

## Run

```
cargo build --bin bpf-recorder --release
```

Run using sudo:

```
sudo RUST_LOG=info ./target/release/bpf-recorder
```

Use environment variables for configuration:

* `SERVER_PORT`. Default value is `8000`. Set the port where debugger will listen http requests.
* `DB_PATH`. Default value is `target/db`.
* `DRY`. By default the variable is not set. Set any value `DRY=1` to disable BPF. Useful for inspecting database.
* `HTTPS_KEY_PATH` and `HTTPS_CERT_PATH`. By default the variables are not set. Set the path to crypto stuff in order to enable tls (https).
* `CHAIN_ID`. Default value is mainnet chain id: `coda/0.0.1/5f704cc0c82e0ed70e873f0893d7e06f148524e3f0bdae2afb02e7819a0c24d1`.

Line in log `libbpf: BTF loading error: -22` may be ignored.

In separate terminal run the application with env variable `BPF_ALIAS=` set.
The value of the variable doesn't matter. That is for bpf-recorder
to recognize the target application.

Maybe, we will pass some useful info to the debugger using this env variables.

## Protocol stack

Mina p2p traffic conform this protocol stack (incomplete):

* [Private Networks](https://github.com/libp2p/specs/blob/0c40ec885645c13f1ed43f763926973835178c6e/pnet/Private-Networks-PSK-V1.md). Uses XSalsa20 stream with pre-shared key. The key is derived from mina configuration, so it is not really secret key, but know for every peer that has the same config. 
* [Connection establishment](https://github.com/libp2p/specs/tree/0c40ec885645c13f1ed43f763926973835178c6e/connections). 
* [Multistream Select](https://github.com/multiformats/multistream-select/tree/c05dd722fc3d53e0de4576161e46eea72286eef3) Negotiate all further protocols. Mina usually using `/noise` and may use `/libp2p/simultaneous-connect`.
* [Noise handshake](https://github.com/libp2p/specs/tree/0c40ec885645c13f1ed43f763926973835178c6e/noise).
