#!/bin/bash

# Dynamic configuration values.
NUM_NODES=4
BASE_PORT=50051
BASE_IP="127.0.0.1"
LEADER_ID=$NUM_NODES
IS_EVENT_DRIVEN=true
LOG_LEVEL="INFO"

# Build the server binary once.
cargo build -p composed-grpc

# Build the Windows Terminal command string with environment variables included.
CMD="wt.exe new-tab --title \"Node 4\" bash -c \"NUM_NODES=${NUM_NODES} BASE_PORT=${BASE_PORT} BASE_IP=${BASE_IP} LEADER_ID=${LEADER_ID} IS_EVENT_DRIVEN=${IS_EVENT_DRIVEN} RUST_LOG=${LOG_LEVEL} ./target/debug/composed-grpc --node-id 4\""

echo "Starting node"
# Execute the command to open a single Windows Terminal window with multiple tabs.
eval "$CMD"
echo "All nodes started."
