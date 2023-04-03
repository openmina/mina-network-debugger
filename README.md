

# Mina Network Debugger

## Table of Contents

1. [Introduction](#Introduction)  

    1.1 [Preparing for build](#Preparing-for-build)
    
    1.2 [Build and Run](#Build-and-Run)
    
    1.3 [Build and run aggregator](#Build-and-run-aggregator)
    
    1.4 [Run tests](#Run-tests)

    1.5 [Docker](#Docker)

    1.6 [Kubernetes](#Kubernetes)
    
    1.7 [Protocol stack](#Protocol-stack)

2. [The Network Debugger Front End](#The-Network-Debugger-Front-End)

    2.1 [Messages](#Messages)
    
    2.2 [Connections](#Connections)
    
    2.3 [Blocks](#Blocks)
    
    2.4 [Blocks IPC](#Blocks-IPC)
  
## Introduction

The P2P network is the key component of the Mina blockchain. It is used for communication between nodes, which, among other things, also includes block propagation and the updating of the blockchain state. We want to have a close look at the messages sent by nodes to see if there are inefficiencies in communication so that we know where to optimize.

We achieve this by externally tracing the Mina application through eBPF. eBPF allows developers to run their code inside the Linux kernel. It is secure because the code is translated into bytecode, not machine code, and statically analyzed before execution. This bytecode allows limited read-only access to internal kernel structures and is triggered by a kernel event such as (but not limited to) `syscall`. 

The Mina Network Debugger consists of two parts: an eBPF module and a normal userspace application. The eBPF module collects the data from the kernel and the userspace application decrypts the data, parses it, stores it in a database and serves it over http.

The Mina application launches the libp2p_helper subprocess to communicate with peers over the network. It does this through an `exec` syscall. The eBPF module in the kernel listens for this syscall and thus detects the libp2p_helper subprocess. After that, the eBPF module is can focus on the libp2p_helper and listen to its syscalls.

The libp2p_helper communicated with the Mina application through its stdin (standatd input) and stdout (standard output) pipes. Every Linux process has such pipes. The Mina application writes commands to libp2p_helper's stdin and reads events from libp2p_helper's stdout. In addition, libp2p_helper communicates with peers around the world via TCP connections. The eBPF module intercepts all the data read and written by libp2p_helper and sends it to userspace via a shared memory.

The userspace part of the debugger receives all data from the eBPF module, decrypts it and parses it. The debugger doesn't need a secret key to decrypt the data, because for network interaction the fresh secret key is generated when the Mina application is started and then it is signed by the static (permanent) secret key. However, because the key is generated at startup, the debugger can intercept it, just like any other data. Please note that this is not the key that protects the user's tokens, which is much more difficult to intercept.


## Preparing for build

List of pre-requisites:

Install required dependencies, here is commands for Ubuntu:

```
sudo apt-get update
sudo apt-get install -y git curl libelf-dev protobuf-compiler clang libssl-dev pkg-config libbpf-dev make
```

Install Rust.

```
curl https://sh.rustup.rs -sSf | sh -s -- -y
source ~/.cargo/env
```

Rust `nightly-2022-10-10` and bpf linker.

```
rustup update nightly-2022-10-10
rustup component add rust-src --toolchain nightly-2022-10-10-x86_64-unknown-linux-gnu
cargo install bpf-linker --git https://github.com/vlad9486/bpf-linker --branch keep-btf
```

And finally, capnproto.

```
curl -sSL https://capnproto.org/capnproto-c++-0.10.2.tar.gz | tar -zxf - \
  && cd capnproto-c++-0.10.2 \
  && ./configure \
  && make -j6 check \
  && sudo make install \
  && cd .. \
  && rm -rf capnproto-c++-0.10.2
```

## Build and Run

```
cargo build --bin bpf-recorder --release
```

Cargo itself (not rustc) will display a warning about a file `main.rs` that was found to be present in multiple build targets. It is intentional that the file is present in two targets.

Run using sudo:

```
sudo -E RUST_LOG=info ./target/release/bpf-recorder
```

Before running, you can use environment variables for configuration:

* `SERVER_PORT`. Default value is `8000`. Set the port where debugger will listen http requests.
* `DB_PATH`. Default value is `target/db`.
* `DRY`. Set any value (for example `DRY=1`) to disable BPF. This is useful for inspecting the database.
* `HTTPS_KEY_PATH` and `HTTPS_CERT_PATH`. By default, the variables are not set. Set the path to crypto stuff in order to enable them (https).
* `DEBUGGER_INDEX_LEDGER_HASH`. By default it is disabled, set any value to enable indexing ledger hash, it may be cpu expensive.
* `FIREWALL_INTERFACE`. Set interface name where firewall will be attached. Default is `eth0`.

Line in log `libbpf: BTF loading error: -22` may be ignored. It is because we wrote BPF module in Rust, which generate incompatible debug information. 

In a separate terminal, run the application with env variable `BPF_ALIAS=` set.
Important note: set the variable for mina application, not for debugger.

The value of the variable must be the following format: `${CHAIN_ID}-${EXTERNAL_IP}`.
The `${CHAIN_ID}` is one of: `mainnet` or `devnet` or `berkeley` or literal chain id like `/coda/0.0.1/d8a8e53385b4629a1838156529ff2687e31f951873704ddcc490076052698a88`.

For example: 

```
BPF_ALIAS=/coda/0.0.1/d8a8e53385b4629a1838156529ff2687e31f951873704ddcc490076052698a88-0.0.0.0
```

or

```
BPF_ALIAS=berkeley-0.0.0.0
```

or

```
BPF_ALIAS=devnet-127.0.0.1
```

## Run tests

Run unit tests is very simple. There are few dozens of such tests.

```
cargo test
```

There is also an integration test which opens TCP connections with itself and
simultaneously performs disk IO. The test is checking the debugger sees only TCP data and the data is correct.

Build the test:

```
cargo build --bin coda-libp2p_helper-test
```

To run the test, run debugger first with environment variable `TEST=1`.

```
sudo -E RUST_LOG=info TEST=1 TERMINATE=1 ./target/release/bpf-recorder
```

In separate terminal run the test, note, `PATH` variable must be adjusted:

```
BPF_ALIAS=test PATH=$(pwd)/target/debug coda-libp2p_helper-test
```

In few seconds the test is done and both running application exited. Debugger must print "test is passed" in log.

## Docker

The debugger requires privileged access to the system, read-write access to `/sys/kernel/debug` directory and read-only access to `/proc` directory.

Build the image:

```
docker build -t mina-debugger:local .
```

Simple config for docker-compose:

```
services:
  debugger:
    privileged: true
    image: mina-debugger:local
    environment:
      - RUST_LOG=info
      - SERVER_PORT=80
      - DB_PATH=/tmp/mina-debugger-db
      # - HTTPS_KEY_PATH=".../privkey.pem"
      # - HTTPS_CERT_PATH=".../fullchain.pem"
    volumes:
      - "/sys/kernel/debug:/sys/kernel/debug:rw"
      - "/proc:/proc:ro"
    ports:
      - "80:80"
```

## Kubernetes

This repository contains `run.yaml` specification, which run latest debugger (at the moment) and Mina node.

```
kubectl apply -f run.yaml
```

## Protocol stack

Mina p2p traffic conform this protocol stack (incomplete):

* [Private Networks](https://github.com/libp2p/specs/blob/0c40ec885645c13f1ed43f763926973835178c6e/pnet/Private-Networks-PSK-V1.md). Uses XSalsa20 stream with pre-shared key. The key is derived from mina configuration, so it is not really secret key, but know for every peer that has the same config. 
* [Connection establishment](https://github.com/libp2p/specs/tree/0c40ec885645c13f1ed43f763926973835178c6e/connections). 
* [Multistream Select](https://github.com/multiformats/multistream-select/tree/c05dd722fc3d53e0de4576161e46eea72286eef3) Negotiate all further protocols. Mina usually using `/noise` and may use `/libp2p/simultaneous-connect`.
* [Noise handshake](https://github.com/libp2p/specs/tree/0c40ec885645c13f1ed43f763926973835178c6e/noise).

## The Network Debugger Front End

You can view the Network’s front end on the Metrics and Tracing interface.

Open up [the Metrics and Tracing website’s “Network” page](http://1.k8.openmina.com:31308/network/messages?node=node1)

The **Messages** tab shows a list of all messages sent across the P2P network.


Click on the Filters icon:

Various filters for P2P messages will be displayed:

![4-1-0-Network--Messages Filters](https://user-images.githubusercontent.com/1679939/220913940-3d463b63-8af2-40b1-bc8b-150d2a2daa99.png)


Hovering over each filter category or filter will display a tooltip with additional information.

Clicking on a filter will filter out all other messages, displaying only those that are related to that category:

You can also click on a filter category (in this case, `/noise`), which may contain multiple filters:

  ![4-1-0-0-Network--Messages Filters Noise](https://user-images.githubusercontent.com/1679939/220934310-f85860cb-902f-4568-ace8-de43bc58b262.png)

There is also the option of combining multiple filters from various categories:

<screenshot - network - messages - multiple filters selected>

Below the filters is a list of network **Messages**. They are sorted by their message **ID** and **datetime**. You can also see their **Remote Address**, **Direction** (whether they are incoming or outgoing), their **Size**, their **Stream Kind** and **Message Kind**.

The most recent messages continuously appear at the bottom of the list as long as the green **Live** button on the bottom of the screen is selected. Click on **Pause** to stop the continuous addition of new messages. There are buttons for going to the top of the list as well as for browsing back and forth between pages.

![4-1-2-Network--Messages Time](https://user-images.githubusercontent.com/1679939/220934442-e77a0634-212d-4931-9b27-cc4f08079cd5.png)

There is also a time filter. Click on the button to open up a window into which you can input your own time range, and the page will filter out network messages made within that time range.

Click on a network message to display further details:

![4-1-2-Network--Messages Detail](https://user-images.githubusercontent.com/1679939/220915125-8c6a90a3-710c-4d62-a093-62fbb524ff59.png)

  
**Message**

By default, the Info window will first display the contents of the **Message**. 

Click on **Expand all** to show full details of all values, and **Collapse all** to minimize them. You can **Copy** the information into your clipboard or **Save** it, either as a **JSON** file or as a **Bin**. You can also click on **Copy link** to copy the a web link for this message to your clipboard.



**Message hex**

This is the Hex value of the message. 

You can **Copy** the information into your clipboard or **Save** it, either as a JSON file or as a Bin. Click on **Copy link** to copy a link to the message to your clipboard.

![4-1-3-Network--Messages Detail](https://user-images.githubusercontent.com/1679939/220915327-ac963f48-0639-4701-9992-92f626ee3f90.png) 
  
**Connection**

Same as with the Message Hex, you can **Copy** the information into your clipboard or **Save** it, either as a JSON file or as a Bin. Click on **Copy link** to copy the link to the message to your clipbaord.

Now let’s move onto the next tab in the Network page - **Connections**


### Connections


Connections made across the P2P network have to be encrypted and decrypted. We want to see whether these processes have been completed, and if not, to see which connections failed to do so.

For this purpose, we’ve created a list of connections to other peers in the Mina P2P network.

![4-2-0-Connection](https://user-images.githubusercontent.com/1679939/220934557-1dfa42d2-9517-4d26-9e6b-5c0806d6635f.png)

**Datetime** - when the connection was made. Click on the datetime to open up a window with additional Connection details.

**Remote Address** - the address of the peer. Clicking on the address will take you back to the Messages tab and filter out all messages from that peer.

**PID** - the process id given to applications by the operating system. It will most likely remain the same for all messages while the node is running, but it will change if the node crashes and is rebooted. 

**FD** - the messages’s TCP socket ID (a file descriptor, but it is used for items other than files). The fd is valid inside the process, another process may also have the same fd, but it is different socket. Similarly to pid, it is subject to change when the connection is closed or fails.

**Incoming** - This value informs us of who initiated the communication, if it is the selected node in the top right corner, it will be marked as _outgoing_. If it is a different node, then it is marked as _incoming_.

**Decrypted In** - the percentage of messages coming into the node that the debugger was able to decrypt

**Decrypted Out** - the percentage of messages coming from the node that the debugger was able to decrypt

Click on a connection’s datetime to open up the **Connection details** window on the right side of your screen:

![4-2-1-Connection Sidebar](https://user-images.githubusercontent.com/1679939/220934682-4c260a1f-44d5-4781-af6f-d7fe9215dca9.png)

**connectionId** - the ID number of the connection

**date** - the time and date when the connection was made

**incoming** - whether it is an incoming or outgoing connection

**addr** - the remote address from where the connection is coming

**pid** - process ID, explained above.

**fd** - TCP socket ID, explained above.

**Stats_in** - accumulated statistics about incoming traffic

**Stats_out** - accumulated statistic about outgoing traffic

Click on **Expand all** to show full details of all values, and **Collapse all** to minimize them. You can **Copy** the information into your clipboard or **Save** it as a JSON file.


### Blocks

We want to view inter-node communication so that we can detect any inefficiencies that we can then optimize. We created a page that provides an overview of blocks propagated across the Mina P2P network. Note that everything is from the perspective of the node selected in the top right corner of the screen.

![4-3-0-Blocks](https://user-images.githubusercontent.com/1679939/220934745-9fb1d346-5b1d-442d-9369-573a40f1f689.png)

You can browse back and forth through lists of blocks depending on their block **Height**, or move to the top (meaning the highest or most recent block height). Below the height scroller, you can see the **Block candidates** at the selected height, as well as their hashes. Click on a Block candidate to filter out messages broadcasting that block.

Block candidates are known as such because for each global slot, multiple nodes are vying for the opportunity to produce a block. These nodes do not know about the rights of each other until a new block appears. Multiple nodes may assume that they can produce a valid block and end up doing so, creating these block candidates, but ultimately, there is a clear rule to select only one block as the canonical one.

Click on the icon on the right edge of the screen to open up a window titled **Distributions** that displays a histogram with the distribution of times from the sample we collected. 

![4-3-0-Blocks Histogram](https://user-images.githubusercontent.com/1679939/220934818-f34df5b9-b3f9-423d-b812-c76be51448a3.png)

This histogram lets you see how much variation there is between block send times, what is the range and what are the most common times.


### Blocks IPC

A Mina node communicates over the network with other peers as well as inter-process commands from Mina daemon on the same device. We want to track the block as the local node is creating it or as the local node first sees it so that we can detect any problems during this communication. 

For that purpose, we’ve created the Block IPC tab, which displays inter-process communication (IPC). 


![4-4-0-Blocks IPC](https://user-images.githubusercontent.com/1679939/220934862-bef4ad43-a9ec-428a-b3ef-ecc1fea7c760.png)

**Height** - the block height at which the candidate blocks are attempting to be published. 

**Block candidates** - blocks that have been created as candidate blocks for that block height, but have yet to be selected as canonical.

**Datetime** - The time and date when they were added

**Message Hash** - A hash calculated for each message that helps detect duplicates.

**Height** - the height of the blocks

**Node Address** - address of the node that the debugger is attached to.

**Peer Address** - if this value is `received_gossip`, it means the peer sent us the message.  If it is `publish_gossip`, then this field is absent, because it was published to all peers.

**Type** - the type of event, either `received_gossip` or `publish_gossip`.

**Message Type** - The three possible messages Mina can broadcast are `new_state`, `snark_pool_diff` or `transaction_pool_diff`.

**Block Latency** - the time between the creation of this block and when this block has been seen by the peer

Click on the icon on the right edge of the screen to open up a window titled **Distributions** that displays a graph with block count on the y-axis and Block Latency values on the x-axis.


