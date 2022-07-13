FROM ubuntu:22.04 as builder

ARG linker_src="https://github.com/vlad9486/bpf-linker"
ARG linker_branch="keep-btf"
ARG src="https://github.com/vlad9486/mina-network-debugger"
ARG branch="main"

RUN apt-get update \
    && apt-get install -y git curl clang make pkg-config libelf-dev \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- --default-toolchain nightly-2022-07-01 -y
ENV PATH=/root/.cargo/bin:$PATH
RUN rustup component add rust-src \
        --toolchain nightly-2022-07-01-x86_64-unknown-linux-gnu
RUN cargo install bpf-linker --git ${linker_src} --branch ${linker_branch}
RUN cargo install bpf-recorder --git ${src} --branch ${branch}

FROM ubuntu:22.04

RUN apt-get update && apt-get install -y zlib1g libelf1 libgcc1
COPY --from=builder /root/.cargo/bin/bpf-recorder /usr/bin/bpf-recorder

ENTRYPOINT ["/usr/bin/bpf-recorder"]
