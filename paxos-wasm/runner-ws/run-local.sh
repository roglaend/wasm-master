#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG="$SCRIPT_DIR/src/config.yaml"

TARGET=release
WT=wt.exe
MODEL=runners-ws

# ─── find the line number that says "clusters:" ──────────────────
clust_line=$(grep -n '^clusters:' "$CONFIG" | cut -d: -f1)
if [[ -z "$clust_line" ]]; then
  echo "No clusters found in $CONFIG" >&2
  exit 1
fi

# ─── grab everything from just *after* that line to EOF, then
#     look for lines that start with optional space + digits + ":"  
mapfile -t CLUSTER_IDS < <(
  sed -n "$((clust_line+1)),\$p" "$CONFIG" \
    | grep '^[[:space:]]*[0-9]\+:' \
    | sed -E 's/^[[:space:]]*([0-9]+):.*/\1/'
)

if [[ ${#CLUSTER_IDS[@]} -eq 0 ]]; then
  echo "No clusters defined under 'clusters:' in $CONFIG" >&2
  exit 1
fi

# ─── build the WT command: one tab per cluster ─────────────────
CMD="$WT"
for cluster in "${CLUSTER_IDS[@]}"; do
  title="cluster $cluster"
  CMD+=" new-tab --title \"$title\" bash -c 'ulimit -n 10000 && \
    ./target/$TARGET/$MODEL \
      --cluster-id $cluster \
      --config \"$CONFIG\"' \\;"
done
# strip the final ' \;'
CMD=${CMD% \\;}
echo "Launching local Paxos clusters in one WT window…"
eval "$CMD"