#!/usr/bin/env bash

REMOTE_DIR=~/master
LOG_DIR=logs/remote

# Format: server:cluster_id
servers=(
    "bbchain1:1"
    "bbchain2:2"
    "bbchain3:3"
    # "bbchain5:5"
    # "bbchain6:7"
    # "bbchain7:8"
    # "bbchain8:9"
)

echo "Starting cluster processes on each server"
for entry in "${servers[@]}"; do
    server="${entry%%:*}"
    cluster_id="${entry##*:}"
    echo "[$server] starting cluster $cluster_id…"
    ssh $server -T "cd $REMOTE_DIR && \
        LOG_DIR=\"\$(dirname \$PWD)/$LOG_DIR\" && \
        mkdir -p \"\$LOG_DIR\" && \
        ulimit -n 10000 && \
        nohup ./runners-ws --cluster-id $cluster_id --config ./config.remote.yaml \
            >> \"\$LOG_DIR/cluster_${cluster_id}.log\" 2>&1 &" &
done
wait

echo "All clusters started in parallel."

# ─── Determine highest cluster ID and corresponding server ───
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

ssh $max_server -T "tail -f ~/logs/remote/cluster_${max_cluster_id}.log"