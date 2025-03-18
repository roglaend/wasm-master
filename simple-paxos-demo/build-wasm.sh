#!/bin/bash

set -e  # Exit immediately if a command fails

# Define the root directory of the workspace
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# WASM_DIR="$ROOT_DIR/paxos-wasm"
WASM_DIR="$ROOT_DIR/grpc"
TARGET_DIR="target/wasm32-wasip2/release"

# Define the WASM component directories inside paxos-wasm
# COMPONENTS=("proposer" "acceptor" "learner")
# COMPONENTS=("proposer-wrpc")
# COMPONENTS=("proposer-grpc" "acceptor-grpc" "learner-grpc" "kv-store-grpc")
# COMPONENTS=("paxos-grpc")
COMPONENTS=("proposer-grpc" "acceptor-grpc" "learner-grpc" "kv-store-grpc" "paxos-grpc")


echo "Building WASM components..."

# Loop through each component and build it
for component in "${COMPONENTS[@]}"; do
    COMPONENT_DIR="$WASM_DIR/$component"

    echo "Building $component..."
    (cd "$COMPONENT_DIR" && cargo build --release --target wasm32-wasip2)

    WASM_FILE="$TARGET_DIR/$component.wasm"
    # if [ ! -f "$WASM_FILE" ]; then
    #     echo "Error: $component.wasm not found in $TARGET_DIR"
    #     exit 1
    # fi

    echo "Finished building $component"
done

echo "All WASM components built successfully."
