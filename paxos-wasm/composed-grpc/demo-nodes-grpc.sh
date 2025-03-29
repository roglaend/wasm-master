#!/bin/bash

# Dynamic configuration values.
NUM_NODES=3
BASE_PORT=50051
BASE_IP="127.0.0.1"
LEADER_ID=$NUM_NODES
LOG_LEVEL="INFO"

# Build the Windows Terminal command string with environment variables included.
CMD="wt.exe new-tab --title \"Node 1\" bash -c \"NUM_NODES=${NUM_NODES} BASE_PORT=${BASE_PORT} BASE_IP=${BASE_IP} LEADER_ID=${LEADER_ID} RUST_LOG=${LOG_LEVEL} ./target/debug/composed-grpc --node-id 1\""

for (( node_id=2; node_id<=NUM_NODES; node_id++ )); do
    CMD+=" \; new-tab --title \"Node ${node_id}\" bash -c \"NUM_NODES=${NUM_NODES} BASE_PORT=${BASE_PORT} BASE_IP=${BASE_IP} LEADER_ID=${LEADER_ID} RUST_LOG=${LOG_LEVEL} ./target/debug/composed-grpc --node-id ${node_id}\""
done

echo "Starting nodes..."
# Execute the command to open a single Windows Terminal window with multiple tabs.
eval "$CMD"
echo "All nodes started."
