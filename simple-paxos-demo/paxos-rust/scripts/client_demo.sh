#!/bin/bash
export RUST_LOG=INFO

echo "Sending proposal: SET key1 value1"
cargo run --features distributed -- client 127.0.0.1:8001 "SET key1 value1"

echo "Sending proposal: SET key2 value2"
cargo run --features distributed -- client 127.0.0.1:8001 "SET key2 value2"

echo "Sending proposal: GET key1"
cargo run --features distributed -- client 127.0.0.1:8001 "GET key1"
