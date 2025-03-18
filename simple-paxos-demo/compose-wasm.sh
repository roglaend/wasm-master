#!/bin/bash
set -euo pipefail

# Define the directory where your WASM components are located.
TARGET_DIR="$(pwd)/target/wasm32-wasip2/release"

# Define the file paths for each component:
# - The plugs: proposer, acceptor, learner, and kv-store.
# - The socket: paxos_coordinator which expects those imports.
PROPOSER="$TARGET_DIR/proposer_grpc.wasm"
ACCEPTOR="$TARGET_DIR/acceptor_grpc.wasm"
LEARNER="$TARGET_DIR/learner_grpc.wasm"
KV_STORE="$TARGET_DIR/kv_store_grpc.wasm"
PAXOS="$TARGET_DIR/paxos_grpc.wasm"

# Define the output composite component file.
OUTPUT="$TARGET_DIR/composed_paxos_grpc.wasm"

# Use wac plug to plug the exports of the four components into the paxos coordinator.
# The syntax is: wac plug --plug <plug1> --plug <plug2> ... <socket> -o <output>
wac plug --plug "$PROPOSER" --plug "$ACCEPTOR" --plug "$LEARNER" --plug "$KV_STORE" "$PAXOS" -o "$OUTPUT"

echo "Composite Paxos component created: $OUTPUT"
