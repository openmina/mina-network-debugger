# Mina Network Debugger

(readme for developers)

## Build

See dockerfile.

Most likely developer machine already have all deps.

Need rust `nightly-2022-07-01`. And bpf linker.

```
rustup update nightly-2022-07-01
rustup component add rust-src --toolchain nightly-2022-07-01-x86_64-unknown-linux-gnu
cargo install bpf-linker --git https://github.com/vlad9486/bpf-linker --branch keep-btf
```

## Run

```
cargo run --bin bpf-recorder --release
```

Will ask your superuser password.

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
