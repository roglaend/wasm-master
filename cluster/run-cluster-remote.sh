#!/usr/bin/env bash
set -euo pipefail

# List of servers with their cluster-id
servers=(
  # "bbchain1:1"
  "bbchain2:1"
  "bbchain3:2"
  "bbchain4:3"
  # "bbchain5:4"
  # "bbchain6:5"
  # "bbchain7:6"
  # "bbchain8:7"
  # "bbchain9:8"
  # "bbchain10:9"
)

echo "Starting cluster processes on each server…"
for entry in "${servers[@]}"; do
  server="${entry%%:*}"
  cluster_id="${entry##*:}"

  echo "[$server] starting cluster $cluster_id…"
  ssh "$server" -T "
    REMOTE_DIR=\"\$HOME/master_files\"
    REMOTE_LOG_DIR=\"\$REMOTE_DIR/logs\"

    mkdir -p \"\$REMOTE_LOG_DIR\"
    cd \"\$REMOTE_DIR\" || { echo \"Failed to cd into \$REMOTE_DIR\"; exit 1; }

    ulimit -n 10000

    ( nohup ./runners-ws \
      --cluster-id \"$cluster_id\" \
      --config ./config.remote.yaml \
      --wasm ./final_composed_runner.wasm \
      --logs-dir ./logs \
      >> \"\$REMOTE_LOG_DIR/cluster_${cluster_id}.log\" 2>&1 & )
  " &
done

# Wait for all SSH sessions to finish
wait
echo "All clusters started in parallel."

# Determine highest cluster-id and the corresponding server
max_cluster_id=0
max_server=""
for entry in "${servers[@]}"; do
  server="${entry%%:*}"
  cluster_id="${entry##*:}"
  if (( cluster_id > max_cluster_id )); then
    max_cluster_id=$cluster_id
    max_server=$server
  fi
done

echo
echo "Tailing logs for the node with the highest cluster_id: $max_cluster_id on $max_server"
echo "Use Ctrl-C to stop tailing. Logs for other clusters are stored on their servers."

ssh "$max_server" -T "
  REMOTE_DIR=\"\$HOME/master_files\"
  REMOTE_LOG_DIR=\"\$REMOTE_DIR/logs\"
  tail -f \"\$REMOTE_LOG_DIR/cluster_${max_cluster_id}.log\"
"
