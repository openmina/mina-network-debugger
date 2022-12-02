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
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- --default-toolchain nightly-2022-10-10 -y
ENV PATH=/root/.cargo/bin:$PATH
RUN rustup component add rust-src \
        --toolchain nightly-2022-10-10-x86_64-unknown-linux-gnu
RUN cargo install bpf-linker --git ${linker_src} --branch ${linker_branch}

# RUN cargo install --git https://github.com/name-placeholder/mina-network-debugger bpf-recorder --bin bpf-recorder
COPY . .
RUN cargo install --path bpf-recorder bpf-recorder
RUN cargo install --path mina-aggregator mina-aggregator

FROM ubuntu:20.04

RUN apt-get update && apt-get install -y zlib1g libelf1 libgcc1 libssl-dev
COPY --from=builder /root/.cargo/bin/bpf-recorder /usr/bin/bpf-recorder
COPY --from=builder /root/.cargo/bin/mina-aggregator /usr/bin/mina-aggregator

ENTRYPOINT ["/usr/bin/bpf-recorder"]
