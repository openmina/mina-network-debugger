`docker build -t local/network-debugger:latest .`

`docker run --rm --privileged -v /sys/kernel/debug:/sys/kernel/debug local/network-debugger:latest`
