#!/bin/bash
set -e  # Exit immediately if a command fails

# Define the root directory of the workspace
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="$ROOT_DIR/target/wasm32-wasip2/release"

# Define directories for your two groups of WASM components
CORE_WASM_DIR="$ROOT_DIR/paxos-wasm/core"
CORE_COMPONENTS=("proposer" "acceptor" "learner" "kv-store")

COMPOSED_GRPC_WASM_DIR="$ROOT_DIR/paxos-wasm/composed-grpc"
COMPOSED_GRPC_COMPONENTS=("paxos-coordinator")

echo "Building CORE WASM components..."
for component in "${CORE_COMPONENTS[@]}"; do
    COMPONENT_DIR="$CORE_WASM_DIR/$component"
    echo "Building $component in CORE..."
    (cd "$COMPONENT_DIR" && cargo build --release --target wasm32-wasip2)
    echo "Finished building $component"
done

echo "Building COMPOSED GRPC WASM components..."
for component in "${COMPOSED_GRPC_COMPONENTS[@]}"; do
    COMPONENT_DIR="$COMPOSED_GRPC_WASM_DIR/$component"
    echo "Building $component in COMPOSED GRPC..."
    (cd "$COMPONENT_DIR" && cargo build --release --target wasm32-wasip2)
    echo "Finished building $component"
done

echo "All WASM components built successfully."
