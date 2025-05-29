#!/usr/bin/env bash
set -euo pipefail

# Run from root of project
PROXY_HOST=uis  # SSH config alias

echo "Creating archive of relevant files"

# Create temporary directory
tmp_dir=$(mktemp -d)

# Copy binaries, config, and WASM
cp ./cluster/config.remote.yaml "$tmp_dir/"
cp ./target/release/runners-ws "$tmp_dir/"
cp ./target/release/modular-ws-client "$tmp_dir/"
cp ./target/wasm32-wasip2/release/final_composed_runner.wasm "$tmp_dir/"
cp ./target/wasm32-wasip2/release/composed_paxos_ws_client.wasm "$tmp_dir/"

# Copy deployment and run scripts
cp ./cluster/deploy-remote.sh "$tmp_dir/"
cp ./cluster/force-stop-cluster.sh "$tmp_dir/"
cp ./cluster/run-cluster-remote.sh "$tmp_dir/"
cp ./cluster/run-clients-remote.sh "$tmp_dir/"

# Create archive
tar czf master_files.tar.gz -C "$tmp_dir" .

# Clean up temp dir
rm -rf "$tmp_dir"

echo "Uploading archive to SSH proxy server…"
ssh "$PROXY_HOST" "mkdir -p ~/master_files"
scp master_files.tar.gz "${PROXY_HOST}:~/master_files/"

# Unpack, clean up, and make scripts executable automatically on the proxy
ssh "$PROXY_HOST" -T "
  cd ~/master_files &&
  tar xzf master_files.tar.gz &&
  rm master_files.tar.gz &&
  chmod +x deploy-remote.sh force-stop-cluster.sh run-cluster-remote.sh run-clients-remote.sh
"

echo "Archive uploaded and unpacked on proxy at ~/master_files/"
