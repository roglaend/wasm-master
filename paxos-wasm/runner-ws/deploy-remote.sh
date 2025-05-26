#!/usr/bin/env bash

# Called from the root of your git repo
REMOTE_DIR=~/master

# Servers to deploy to
servers=(
    "bbchain1"
    "bbchain2"
    "bbchain3"
    # "bbchain5"
    # "bbchain6"
    # "bbchain7"
    # "bbchain8"
)

# Stop old processes
echo "Stopping old runners-ws processes on remote servers…"
for server in "${servers[@]}"; do
    echo "[$server] stopping processes…"
    ssh $server -T "pkill -f runners-ws || true; mkdir -p $REMOTE_DIR" &
done
wait

echo "Uploading binary and config to root of remote project…"
for server in "${servers[@]}"; do
    echo "[$server] copying binary and config.yaml…"
    scp ./target/release/runners-ws $server:$REMOTE_DIR/runners-ws &
    scp ./src/config.yaml $server:$REMOTE_DIR/config.yaml &
done
wait

echo "Uploading WASM file with correct folder structure…"
for server in "${servers[@]}"; do
    echo "[$server] copying final_composed_runner.wasm…"
    rsync -az -R ./target/wasm32-wasip2/release/final_composed_runner.wasm $server:$REMOTE_DIR/ &
done
wait

echo "Deployment done. Correct structure ensured."
