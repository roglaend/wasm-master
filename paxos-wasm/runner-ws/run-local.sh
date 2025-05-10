#!/usr/bin/env bash
set -euo pipefail

# ─── locate this script’s own directory ─────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG="$SCRIPT_DIR/src/config.yaml"

TARGET=release
LOG_LEVEL=warning
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
  id="${IDS[$idx]}"
  role="${ROLES[$idx]}"
  title="$role $id"

  CMD+=" new-tab --title \"$title\" bash -c \\\"\
    RUST_LOG=$LOG_LEVEL \
    ./target/$TARGET/$MODEL \
      --node-id $id \
      --config $CONFIG\\\" \\;"
done

# strip the trailing escaped semicolon
CMD=${CMD%\\;}

echo "Launching local Paxos cluster with one WT window…"
eval "$CMD"
