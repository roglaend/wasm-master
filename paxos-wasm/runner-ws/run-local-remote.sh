#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG="$SCRIPT_DIR/src/config.yaml"
TARGET=release
MODEL=runners-ws

clust_line=$(grep -n '^clusters:' "$CONFIG" | cut -d: -f1)
if [[ -z "$clust_line" ]]; then
  echo "No clusters found in $CONFIG" >&2
  exit 1
fi

mapfile -t CLUSTER_IDS < <(
  sed -n "$((clust_line+1)),\$p" "$CONFIG" \
    | grep '^[[:space:]]*[0-9]\+:' \
    | sed -E 's/^[[:space:]]*([0-9]+):.*/\1/'
)

if [[ ${#CLUSTER_IDS[@]} -eq 0 ]]; then
  echo "No clusters defined under 'clusters:' in $CONFIG" >&2
  exit 1
fi

# ─── prepare log dir and PID file ─────────────────────────────
LOG_DIR="$(dirname "$SCRIPT_DIR")/logs/remote"
mkdir -p "$LOG_DIR"
PID_FILE="$LOG_DIR/cluster-pids"
> "$PID_FILE"

echo "Starting clusters in background…"
for cluster in "${CLUSTER_IDS[@]}"; do
  logfile="$LOG_DIR/cluster_${cluster}.log"
  echo "  Starting cluster $cluster (log: $logfile)…"
  (
    ulimit -n 10000
    exec ./target/$TARGET/$MODEL --cluster-id "$cluster" --config "$CONFIG"
  ) >> "$logfile" 2>&1 &
  echo $! >> "$PID_FILE"
done

# ─── attach to last cluster’s log output ───────────────────────
last_cluster="${CLUSTER_IDS[-1]}"
echo
echo "Attached to last cluster’s log: $last_cluster"
echo "Logs for other clusters are in: $LOG_DIR/"
echo "To stop everything, run:"
echo "  xargs kill < \"$PID_FILE\""
echo

tail -f "$LOG_DIR/cluster_${last_cluster}.log"
