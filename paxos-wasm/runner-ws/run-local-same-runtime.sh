#!/usr/bin/env bash
set -euo pipefail

# ─── locate this script’s own directory ─────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG="$SCRIPT_DIR/src/config.yaml"

TARGET=release
LOG_LEVEL=info
WT=wt.exe
MODEL=runners-ws

# ─── pull node_ids and roles into parallel arrays ───────────────
mapfile -t IDS < <(
  grep '^[[:space:]]*-\s*node_id:' "$CONFIG" \
    | sed -E 's/.*node_id:[[:space:]]*([0-9]+).*/\1/'
)
mapfile -t ROLES < <(
  grep '^[[:space:]]*role:' "$CONFIG" \
    | sed -E 's/.*role:[[:space:]]*"([^"]+)".*/\1/'
)

# sanity check
if [[ ${#IDS[@]} -ne ${#ROLES[@]} ]]; then
  echo "Error: mismatched node_id / role length" >&2
  exit 1
fi

# ─── build the Windows Terminal command ─────────────────────────
CMD="$WT"
for idx in "${!IDS[@]}"; do
  # Check if idx is one of the specified values: 1, 4, or 7
  if [[ "$idx" == 0 || "$idx" == 3 || "$idx" == 6 ]]; then
    id="${IDS[$idx]}"
    cluster_id=$(( (idx / 3) + 1 ))  # This will map idx 1, 4, 7 -> cluster 1, 2, 3
    title="cluster $cluster_id"

    CMD+=" new-tab --title \"$title\" bash -c \\\"\
      RUST_LOG=$LOG_LEVEL \
      ./target/$TARGET/$MODEL \
        --node-id $id \
        --config $CONFIG\\\" \\;"
  fi
done

# strip the trailing escaped semicolon
CMD=${CMD%\\;}

echo "Launching local Paxos cluster with one WT window…"
eval "$CMD"