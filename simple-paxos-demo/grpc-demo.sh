#!/bin/bash

# Default server address (adjust if needed)
SERVER_ADDR="http://127.0.0.1:50051"

# Loop to send 10 values to the server
for i in {1..10}; do
    echo "Sending value $i"
    cargo run --bin grpc-client -- --server-addr "$SERVER_ADDR" --value "value $i"
    echo "------------------------"
    sleep 1
done
