#!/usr/bin/env bash
set -euo pipefail

# ─── Tunables ────────────────────────────────────────────
NUM_CLIENTS=10
NUM_REQUESTS=100
MODE="persistent" # "oneshot" or "persistent"
LEADER="127.0.0.1:60000"
TIMEOUT_SECS=20 # used only by client logic now
TARGET="release" # "release or "debug"
LOG_DIR="./paxos-wasm/logs"

BIN="./target/${TARGET}/modular-ws-client"

# ─── Launch clients ──────────────────────────────────────
pids=()
for (( i=0; i<NUM_CLIENTS; i++ )); do
  echo "Starting client $i (mode=${MODE})…"
  "${BIN}" \
    --client-id "$i" \
    --num-requests "$NUM_REQUESTS" \
    --mode "$MODE" \
    --leader "$LEADER" \
    --timeout-secs "$TIMEOUT_SECS" \
    > "${LOG_DIR}/client${i}.log" 2>&1 &
  pids+=( $! )
done

# ─── Wait for all clients to finish ───────────────────────
for pid in "${pids[@]}"; do
  wait "$pid"
done

echo "All clients finished."
