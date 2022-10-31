#!/usr/bin/env bash

sudo kill $(ps aux | grep bpf | grep root | tail -1 | awk -F ' ' '{print $2}') &> /dev/null
sleep 1
sudo rm -Rf /tmp/mina-debugger-db-$1 &> /dev/null
nohup sudo \
    RUST_LOG='info' \
    SERVER_PORT='80' \
    DB_PATH="/tmp/mina-debugger-db-$1" \
    ~/.cargo/bin/bpf-recorder &> ~/mina-log/$1.log &
