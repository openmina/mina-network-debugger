#!/usr/bin/env bash

set -e

kubectl -n test-debugger delete job mock || true
helm upgrade mina-debugger-test helm/tester --values=helm/tester/values.yaml --namespace=test-debugger --set=build_number=$1 --set=blocks=${BLOCKS} --set=delay=${DELAY} --set=parallelism=${PARALLElISM} --set=image.tag="${DRONE_COMMIT_SHA:0:8}"
sleep $(( ${BLOCKS} * ${DELAY} ))
kubectl -n test-debugger wait --for=condition=complete --timeout=120s job/mock || true
