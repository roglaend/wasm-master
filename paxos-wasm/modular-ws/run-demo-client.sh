#!/usr/bin/env bash
set -euo pipefail

# ─── Tunables ────────────────────────────────────────────
NUM_CLIENTS=10
NUM_REQUESTS=100
MODE="oneshot" # "oneshot" or "persistent"
LEADER="127.0.0.1:50057"
TIMEOUT_SECS=20 
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

# ─── Wait up to TIMEOUT_SECS ─────────────────────────────
start=$SECONDS
while (( ${#pids[@]} > 0 && SECONDS - start < TIMEOUT_SECS )); do
  for idx in "${!pids[@]}"; do
    if ! kill -0 "${pids[idx]}" 2>/dev/null; then
      unset 'pids[idx]'
    fi
  done
  sleep 1
done

# ─── Kill any stragglers ─────────────────────────────────
if (( ${#pids[@]} > 0 )); then
  echo "Reached $TIMEOUT_SECS seconds; killing ${#pids[@]} remaining client(s)…"
  kill "${pids[@]}"
fi

echo "All clients done or timed out."
