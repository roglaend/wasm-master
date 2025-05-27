#!/usr/bin/env bash
set -euo pipefail

# Deployment script to copy only the config.remote.yaml to each server.
echo "Creating minimal archive for config.remote.yaml…"

cd ~/master_files
tar czf config_only.tar.gz config.remote.yaml

REMOTE_DIR="\$HOME/master_files"
DEPLOY_TAR="./config_only.tar.gz"

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

echo "Copying config archive to each server in parallel…"
for server in "${servers[@]}"; do
  echo "[$server] transferring config.remote.yaml…"
  scp "$DEPLOY_TAR" "$server:~/" &
done

wait

echo "Extracting config.remote.yaml on each server and cleaning up in parallel…"
for server in "${servers[@]}"; do
  echo "[$server] extracting config.remote.yaml…"
  ssh "$server" -T "
    mkdir -p ~/master_files &&
    tar xzf ~/config_only.tar.gz -C ~/master_files &&
    rm ~/config_only.tar.gz
  " &
done

wait

echo "Config file deployed to all servers in ~/master_files."
