#!/bin/bash

# Build the acceptor component
cd acceptor && cargo component build --release
cd ..

# Build the proposer component
cd proposer && cargo component build --release
cd ..

# Build the command component
cd command && cargo component build --release
cd ..

# Plug the proposer and acceptor components
wac plug proposer/target/wasm32-wasip1/release/proposer.wasm --plug acceptor/target/wasm32-wasip1/release/acceptor.wasm -o composed.wasm

# Plug the command component with the composed result
wac plug command/target/wasm32-wasip1/release/command.wasm --plug composed.wasm -o final.wasm

echo "Build and composition completed successfully!"