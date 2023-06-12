# `docker build -f dev-dockerfile -t local/network-debugger:latest .`
# `docker run --rm --privileged -v /sys/kernel/debug:/sys/kernel/debug local/network-debugger:latest`

FROM ubuntu:20.04 as builder

ARG linker_src="https://github.com/vlad9486/bpf-linker"
ARG linker_branch="keep-btf"

ENV TZ=UTC
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone
RUN DEBIAN_FRONTEND=noninteractive apt-get update \
    && apt-get install -y git curl clang make pkg-config libelf-dev \
    protobuf-compiler libbz2-dev libssl-dev


RUN curl -sSL https://capnproto.org/capnproto-c++-0.10.2.tar.gz | tar -zxf - \
  && cd capnproto-c++-0.10.2 \
  && ./configure \
  && make -j6 check \
  && make install \
  && cd .. \
  && rm -rf capnproto-c++-0.10.2

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- --default-toolchain nightly-2023-06-01 -y
ENV PATH=/root/.cargo/bin:$PATH
RUN rustup update nightly-2022-10-10

RUN rustup component add rust-src \
        --toolchain nightly-2023-06-01-x86_64-unknown-linux-gnu
RUN rustup component add rust-src \
        --toolchain nightly-2022-10-10-x86_64-unknown-linux-gnu

RUN cargo install bpf-linker --git ${linker_src} --branch ${linker_branch}

# RUN cargo install --git https://github.com/openmina/mina-network-debugger bpf-recorder --bin bpf-recorder
COPY . .
RUN CARGO_TARGET_DIR=target/bpf cargo +nightly-2022-10-10 rustc --package=bpf-recorder --bin=bpf-recorder-kern --features=kern --no-default-features --target=bpfel-unknown-none -Z build-std=core --release -- -Cdebuginfo=2 -Clink-arg=--disable-memory-builtins -Clink-arg=--keep-btf
RUN cargo install --path bpf-recorder bpf-recorder
RUN cargo install --path mina-aggregator mina-aggregator
RUN cargo install --path topology-tool topology-tool

FROM ubuntu:20.04

RUN apt-get update && apt-get install -y zlib1g libelf1 libgcc1 libssl-dev
COPY --from=builder /root/.cargo/bin/bpf-recorder /usr/bin/bpf-recorder
COPY --from=builder /root/.cargo/bin/mina-aggregator /usr/bin/mina-aggregator
COPY --from=builder /root/.cargo/bin/topology-tool /usr/bin/topology-tool

ENTRYPOINT ["/usr/bin/bpf-recorder"]
