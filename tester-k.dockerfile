# docker build -t mina-tester-k:local -f tester-k.dockerfile --build-arg mina_daemon_tag=1.3.2beta2-release-2.0.0-0b63498-bullseye-berkeley .

ARG mina_daemon_tag

FROM rust:1.65-bullseye as builder

RUN curl -sSL https://capnproto.org/capnproto-c++-0.10.2.tar.gz | tar -zxf - \
  && cd capnproto-c++-0.10.2 \
  && ./configure \
  && make -j6 check \
  && make install \
  && cd .. \
  && rm -rf capnproto-c++-0.10.2

WORKDIR /root

COPY . .

RUN cargo build --bin mina-tester-k --release

FROM minaprotocol/mina-daemon:${mina_daemon_tag}

RUN DEBIAN_FRONTEND=noninteractive apt-get update && apt-get -y install conntrack net-tools tcpflow

COPY --from=builder /root/target/release/mina-tester-k /usr/local/bin/mina-tester-k

ENTRYPOINT ["mina-tester-k", "registry"]
