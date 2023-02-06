# Mina Network Debugger

(readme for developers)

## Prepare for build

Most likely developer machine already have all deps.

Need rust `nightly-2022-10-10`. And bpf linker.

```
rustup update nightly-2022-10-10
rustup component add rust-src --toolchain nightly-2022-10-10-x86_64-unknown-linux-gnu
cargo install bpf-linker --git https://github.com/vlad9486/bpf-linker --branch keep-btf
```

Need dependencies, on ubuntu:

```
sudo apt install libelf-dev protobuf-compiler clang libssl-dev pkg-config libbpf-dev make
```

and capnproto

```
curl -sSL https://capnproto.org/capnproto-c++-0.10.2.tar.gz | tar -zxf - \
  && cd capnproto-c++-0.10.2 \
  && ./configure \
  && make -j6 check \
  && sudo make install \
  && cd .. \
  && rm -rf capnproto-c++-0.10.2
```

## Build and Run

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
* `AGGREGATOR`. No default value. If the value is not set, debugger will not connect aggregator. The value must be http or https url, for example: "http://develop.dev.openmina.com:8000".
* `DEBUGGER_NAME`. The name of this debugger for aggregator to distinguish. Any string is valid.

Line in log `libbpf: BTF loading error: -22` may be ignored.

In separate terminal run the application with env variable `BPF_ALIAS=` set.
The value of the variable must start with `mainnet-` or `devnet-` or `berkeley-`.
For example: `BPF_ALIAS=berkeley-node`.

Maybe, we will pass some useful info to the debugger using this env variables.

## Build and run aggregator

```
cargo build --bin mina-aggregator --release
```

Use environment variables:

* `SERVER_PORT`. Aggregator will listen here.
* `HTTPS_KEY_PATH` and `HTTPS_CERT_PATH`. Enables https.

## Docker

Build the image:

```
docker build -t mina-debugger:local .
```

The image containing both debugger and aggregator. Default entrypoint is debugger. 

Simple config for docker-compose:

```
services:
  aggregator:
    image: mina-debugger:local
    environment:
      - RUST_LOG=info
      - SERVER_PORT=8000
    ports:
      - "8000:8000"
    entrypoint: /usr/bin/mina-aggregator

  debugger:
    privileged: true
    image: mina-debugger:local
    environment:
      - RUST_LOG=info
      - SERVER_PORT=80
      - DB_PATH=/tmp/mina-debugger-db
      - AGGREGATOR=http://aggregator:8000
      - DEBUGGER_NAME=develop.dev.openmina.com
      # - HTTPS_KEY_PATH=".../privkey.pem"
      # - HTTPS_CERT_PATH=".../fullchain.pem"
    volumes:
      - "/sys/kernel/debug:/sys/kernel/debug:rw"
    ports:
      - "80:80"
```

## Protocol stack

Mina p2p traffic conform this protocol stack (incomplete):

* [Private Networks](https://github.com/libp2p/specs/blob/0c40ec885645c13f1ed43f763926973835178c6e/pnet/Private-Networks-PSK-V1.md). Uses XSalsa20 stream with pre-shared key. The key is derived from mina configuration, so it is not really secret key, but know for every peer that has the same config. 
* [Connection establishment](https://github.com/libp2p/specs/tree/0c40ec885645c13f1ed43f763926973835178c6e/connections). 
* [Multistream Select](https://github.com/multiformats/multistream-select/tree/c05dd722fc3d53e0de4576161e46eea72286eef3) Negotiate all further protocols. Mina usually using `/noise` and may use `/libp2p/simultaneous-connect`.
* [Noise handshake](https://github.com/libp2p/specs/tree/0c40ec885645c13f1ed43f763926973835178c6e/noise).
