#!/usr/bin/env bash

sudo kill $(ps aux | grep bpf | grep root | tail -1 | awk -F ' ' '{print $2}') &> /dev/null
sleep 1
sudo rm -Rf /tmp/mina-debugger-db-$1 &> /dev/null
nohup sudo \
    RUST_LOG='info' \
    AGGREGATOR='https://debug.dev.openmina.com:8000' \
    DEBUGGER_NAME='debug.dev.openmina.com' \
    SERVER_PORT='443' \
    HTTPS_KEY_PATH='/etc/letsencrypt/live/debug.dev.openmina.com/privkey.pem' \
    HTTPS_CERT_PATH='/etc/letsencrypt/live/debug.dev.openmina.com/fullchain.pem' \
    DB_PATH="/tmp/mina-debugger-db-$1" \
    ~/.cargo/bin/bpf-recorder &> ~/mina-log/$1.log &
