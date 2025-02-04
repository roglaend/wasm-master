#!/bin/bash
# Start three nodes with different ports and a list of peers.
# Adjust the paths as needed.

export RUST_LOG=INFO

# Node 1: ID=1, listening on port 8001; peers: 127.0.0.1:8002 and 127.0.0.1:8003.
cargo run --features distributed -- node 1 127.0.0.1:8001 127.0.0.1:8002 127.0.0.1:8003 &

# Node 2: ID=2, listening on port 8002; peers: 127.0.0.1:8001 and 127.0.0.1:8003.
cargo run --features distributed -- node 2 127.0.0.1:8002 127.0.0.1:8001 127.0.0.1:8003 &

# Node 3: ID=3, listening on port 8003; peers: 127.0.0.1:8001 and 127.0.0.1:8002.
cargo run --features distributed -- node 3 127.0.0.1:8003 127.0.0.1:8001 127.0.0.1:8002 &

# wait

echo "running, enter to stop"
read && killall paxos-rust