#!/bin/bash

set -e

# Directories to build
COMPONENTS=("proposer" "acceptor" "learner")

# Build and convert each component
for COMPONENT in "${COMPONENTS[@]}"; do
    echo "Building $COMPONENT..."
    cargo build --target wasm32-unknown-unknown --release -p $COMPONENT
    wasm-tools component new \
        target/wasm32-unknown-unknown/release/${COMPONENT}.wasm \
        -o ${COMPONENT}.component.wasm
    echo "$COMPONENT.component.wasm built successfully."
done

echo "All WASM components built successfully!"
