#!/usr/bin/env bash
lines=$(cat ~/mina-debugger-log-$1 | grep 'cannot decrypt' | awk -F ' ' '{print $9}' | awk '!visited[$0]++' | sed 's/.$//')
for line in $lines
do du -h /tmp/mina-debugger-db-5/streams/$line
done
