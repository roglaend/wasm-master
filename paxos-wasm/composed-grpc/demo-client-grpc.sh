#!/bin/bash
set -euo pipefail

# Default server addresses (adjust if needed)
LEADER_NODE_ADDR="http://127.0.0.1:50051"
NON_LEADER_NODE_ADDR="http://127.0.0.1:50052"

# Build the client once in release mode.
cargo build --release -p composed-grpc-client

NUM_SLOTS=10  # Change to desired number of slots
LOG_LEVEL=INFO

# Loop to send NUM_SLOTS values to the server, starting from 1.
for (( i=1; i<=NUM_SLOTS; i++ )); do
    echo "Sending value $i"
    RUST_LOG=$LOG_LEVEL ./target/release/composed-grpc-client --server-addr "$LEADER_NODE_ADDR" --value "value $i"
    echo "------------------------"
    sleep 0.5
done

# Request the current Paxos state from a non-leader node.
echo "Requesting Paxos state from non-leader node..."
RUST_LOG=$LOG_LEVEL ./target/release/composed-grpc-client --server-addr "$NON_LEADER_NODE_ADDR"

# Fetch log entries from the leader node starting with last_offset = 0.
echo "Fetching logs from leader node (last_offset = 0)..."
RUST_LOG=$LOG_LEVEL ./target/release/composed-grpc-client --server-addr "$LEADER_NODE_ADDR" --fetch-logs --last-offset 0
