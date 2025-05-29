#!/usr/bin/env bash
set -euo pipefail

# Deployment script to run from SSH proxy server

echo "Creating minimal archive to deploy to servers…"

cd ~/master_files
tar czf deployment.tar.gz \
    runners-ws \
    modular-ws-client \
    config.remote.yaml \
    final_composed_runner.wasm \
    composed_paxos_ws_client.wasm

REMOTE_DIR="\$HOME/master_files"
DEPLOY_TAR="./deployment.tar.gz"

servers=(
    "bbchain1"
    "bbchain2"
    "bbchain3"
    "bbchain4"
    "bbchain5"
    "bbchain6"
    "bbchain7"
    "bbchain8"
    "bbchain9"
)

echo "Copying archive to each server in parallel…"
for server in "${servers[@]}"; do
  echo "[$server] transferring archive…"
  scp "$DEPLOY_TAR" "$server:~/" &
done

wait

echo "Extracting archive on each server and cleaning up in parallel…"
for server in "${servers[@]}"; do
  echo "[$server] extracting and setting up…"
  ssh "$server" -T "
    mkdir -p ~/master_files &&
    tar xzf ~/deployment.tar.gz -C ~/master_files &&
    rm ~/deployment.tar.gz
  " &
done

wait

echo "Deployment to servers complete. Files are in each server's ~/master_files."
