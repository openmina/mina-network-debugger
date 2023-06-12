# docker build -t mina-tester-k:local -f tester-k.dockerfile --build-arg mina_daemon_image=minaprotocol/mina-daemon:2.0.0rampup4-14047c5-buster-berkeley .

ARG mina_daemon_image

FROM rust:1.67-buster as builder

RUN curl -sSL https://capnproto.org/capnproto-c++-0.10.2.tar.gz | tar -zxf - \
  && cd capnproto-c++-0.10.2 \
  && ./configure \
  && make -j6 check \
  && make install \
  && cd .. \
  && rm -rf capnproto-c++-0.10.2

WORKDIR /root

COPY . .

RUN cargo build --bin mina-simulator --release

FROM ${mina_daemon_image} as builder-tcpflow

RUN DEBIAN_FRONTEND=noninteractive apt-get update && \
    apt-get -y install git automake make gcc g++ zlib1g-dev libssl-dev libboost-dev libpcap-dev libcairo2 libcairo2-dev libpython2.7-dev && \
    git clone --recursive --branch tcpflow-1.6.1 https://github.com/simsong/tcpflow.git && \
    cd tcpflow && \
    ./bootstrap.sh && \
    ./configure && make

FROM ${mina_daemon_image}

RUN DEBIAN_FRONTEND=noninteractive apt-get update && apt-get -y install libpcap-dev libcairo2-dev

COPY --from=builder-tcpflow /root/tcpflow/src/tcpflow /usr/local/bin/tcpflow

COPY --from=builder /root/target/release/mina-simulator /usr/local/bin/mina-simulator

ENTRYPOINT ["mina-simulator", "registry"]
