#!/usr/bin/env bash

kill $(ps aux | grep [m]ina-aggregator | awk -F ' ' '{print $2}') &> /dev/null
sleep 1

SERVER_PORT='8000' \
HTTPS_KEY_PATH=$HOME/privkey.pem \
HTTPS_CERT_PATH=$HOME/fullchain.pem \
nohup ~/.cargo/bin/mina-aggregator &
