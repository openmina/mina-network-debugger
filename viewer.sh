#!/usr/bin/env bash
mina-viewer /tmp/mina-debugger-db-$1/streams/$2 &> target/$2.log
