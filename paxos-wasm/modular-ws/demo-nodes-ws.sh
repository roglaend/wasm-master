#!/bin/bash

# Dynamic configuration values.
NUM_NODES=7
BASE_PORT=50051
BASE_IP="127.0.0.1"
LEADER_ID=$NUM_NODES
IS_EVENT_DRIVEN=true
LOG_LEVEL="INFO"

# Build the Windows Terminal command string with environment variables included.


LEARNERS="wt.exe new-tab --title \"Learner 1\" bash -c \"NUM_NODES=${NUM_NODES} BASE_PORT=${BASE_PORT} BASE_IP=${BASE_IP} LEADER_ID=${LEADER_ID} IS_EVENT_DRIVEN=${IS_EVENT_DRIVEN} RUST_LOG=${LOG_LEVEL} ./target/release/modular-ws --node-id 1\""
LEARNERS+=" \; new-tab --title \"Learner 2\" bash -c \"NUM_NODES=${NUM_NODES} BASE_PORT=${BASE_PORT} BASE_IP=${BASE_IP} LEADER_ID=${LEADER_ID} IS_EVENT_DRIVEN=${IS_EVENT_DRIVEN} RUST_LOG=${LOG_LEVEL} ./target/release/modular-ws --node-id 2\""

ACCEPTORS="wt.exe new-tab --title \"Acceptor 3\" bash -c \"NUM_NODES=${NUM_NODES} BASE_PORT=${BASE_PORT} BASE_IP=${BASE_IP} LEADER_ID=${LEADER_ID} IS_EVENT_DRIVEN=${IS_EVENT_DRIVEN} RUST_LOG=${LOG_LEVEL} ./target/release/modular-ws --node-id 3\""
for (( node_id=4; node_id<=5; node_id++ )); do
    ACCEPTORS+=" \; new-tab --title \"Acceptor ${node_id}\" bash -c \"NUM_NODES=${NUM_NODES} BASE_PORT=${BASE_PORT} BASE_IP=${BASE_IP} LEADER_ID=${LEADER_ID} IS_EVENT_DRIVEN=${IS_EVENT_DRIVEN} RUST_LOG=${LOG_LEVEL} ./target/release/modular-ws --node-id ${node_id}\""
done


PROPOSERS="wt.exe new-tab --title \"Proposer 6\" bash -c \"NUM_NODES=${NUM_NODES} BASE_PORT=${BASE_PORT} BASE_IP=${BASE_IP} LEADER_ID=${LEADER_ID} IS_EVENT_DRIVEN=${IS_EVENT_DRIVEN} RUST_LOG=${LOG_LEVEL} ./target/release/modular-ws --node-id 6\""
PROPOSERS+=" \; new-tab --title \"Proposer 7\" bash -c \"NUM_NODES=${NUM_NODES} BASE_PORT=${BASE_PORT} BASE_IP=${BASE_IP} LEADER_ID=${LEADER_ID} IS_EVENT_DRIVEN=${IS_EVENT_DRIVEN} RUST_LOG=${LOG_LEVEL} ./target/release/modular-ws --node-id 7\""


echo "Starting nodes..."
# Execute the command to open a single Windows Terminal window with multiple tabs.
eval "$PROPOSERS"
eval "$ACCEPTORS"
eval "$LEARNERS"
echo "All nodes started."
