#!/usr/bin/env bash
cat $1 | grep 'cannot decrypt' | awk -F ' ' '{print $9}' | awk '!visited[$0]++'
