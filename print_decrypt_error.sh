#!/usr/bin/env bash
lines=$(cat ~/mina-debugger-log-$1 | grep 'cannot decrypt' | awk -F ' ' '{print $10}' | awk '!visited[$0]++' | sed 's/.$//')
for line in $lines
do du -h /tmp/mina-debugger-db-$1/streams/$line
done
