#!/usr/bin/env bash
sudo kill $(ps aux | grep bpf | grep root | tail -1 | awk -F ' ' '{print $2}') &> /dev/null
sleep 1
sudo rm -Rf /tmp/mina-debugger-db-$1 &> /dev/null
nohup sudo RUST_LOG=info SERVER_PORT=443 HTTPS_KEY_PATH=/etc/letsencrypt/live/debug.dev.tezedge.com/privkey.pem HTTPS_CERT_PATH=/etc/letsencrypt/live/debug.dev.tezedge.com/fullchain.pem DB_PATH="/tmp/mina-debugger-db-$1" ~/.cargo/bin/bpf-recorder &> ~/mina-debugger-log-$1 &
