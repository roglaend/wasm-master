#!/bin/bash

NUM_NODES=5
BASE_PORT=50053
BASE_IP="127.0.0.1"
LEADER_ID=${NUM_NODES}
IS_EVENT_DRIVEN=true
LOG_LEVEL="INFO"
TARGET="release"  # or "debug"

# Build up a single wt.exe command that chains 7 tabs
CMD=""
for (( id=1; id<=NUM_NODES; id++ )); do
  CMD+="wt.exe new-tab --title \"Coordinator ${id}\" bash -c \\\"\
NUM_NODES=${NUM_NODES} \
BASE_PORT=${BASE_PORT} \
BASE_IP=${BASE_IP} \
LEADER_ID=${LEADER_ID} \
IS_EVENT_DRIVEN=${IS_EVENT_DRIVEN} \
RUST_LOG=${LOG_LEVEL} \
./target/${TARGET}/modular-ws --node-id ${id}\\\""
  if (( id < NUM_NODES )); then
    CMD+=" \; "
  fi
done

echo "Starting ${NUM_NODES} coordinators…"
eval "$CMD"
echo "All coordinators started."
