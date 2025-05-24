#!/usr/bin/env bash
set -euo pipefail

# ─── Tunables ────────────────────────────────────────────
MODE="persistent" # "oneshot" or "persistent"
LEADER="127.0.0.1:60000"
TIMEOUT_SECS=60 # used only by client logic now
TARGET="release" # "release or "debug"


BIN="./target/${TARGET}/modular-ws-client"

# echo "Removing existing client log files..."'
# rm -f "${LOG_DIR}/client"*.log

TOTAL_REQUESTS=10000
NUM_PROCESSES=10
LOGICAL_CLIENTS_PER_PROCESS=1

BENCHMARK_NAME="crash-test"
BENCHMARK_COUNT=$((NUM_PROCESSES * LOGICAL_CLIENTS_PER_PROCESS))
BENCHMARK="${BENCHMARK_NAME}_${BENCHMARK_COUNT}"

LOG_DIR="./paxos-wasm/logs/$BENCHMARK"

mkdir -p "$LOG_DIR"


NUM_REQUESTS=$((TOTAL_REQUESTS / (NUM_PROCESSES * LOGICAL_CLIENTS_PER_PROCESS)))


pids=()
for (( p=0; p<NUM_PROCESSES; p++ )); do
  OFFSET=$((p * LOGICAL_CLIENTS_PER_PROCESS))
  echo "Starting client group $p with offset=$OFFSET"
  "${BIN}" \
    --client-id "$OFFSET" \
    --client-id-offset "$OFFSET" \
    --num-logical-clients "$LOGICAL_CLIENTS_PER_PROCESS" \
    --num-requests "$NUM_REQUESTS" \
    --mode "$MODE"  \
    --leader "$LEADER"  \
    --timeout-secs "$TIMEOUT_SECS" \
    >> "${LOG_DIR}/client${p}.log" 2>&1 &

  pids+=( $! )
done

for pid in "${pids[@]}"; do
  wait "$pid"
done


echo "All clients completed. Calculating latency statistics..."

all_latencies=$(mktemp)
for log in "$LOG_DIR"/client*.log; do
    grep -E '^[0-9]+$' "$log" >> "$all_latencies"
done

if [ ! -s "$all_latencies" ]; then
    echo "No latency data found in logs."
    exit 1
fi

sorted_latencies=$(mktemp)
sort -n "$all_latencies" > "$sorted_latencies"

count=$(wc -l < "$sorted_latencies")
sum=$(awk '{s+=$1} END {print s}' "$sorted_latencies")
mean=$(awk -v c="$count" -v s="$sum" 'BEGIN { printf "%.2f", s / c }')

min=$(head -n 1 "$sorted_latencies")
max=$(tail -n 1 "$sorted_latencies")

if (( count % 2 == 1 )); then
    median=$(awk "NR == $((count / 2 + 1))" "$sorted_latencies")
else
    m1=$(awk "NR == $((count / 2))" "$sorted_latencies")
    m2=$(awk "NR == $((count / 2 + 1))" "$sorted_latencies")
    median=$(awk -v a="$m1" -v b="$m2" 'BEGIN { printf "%.2f", (a + b) / 2 }')
fi

p95_index=$(( (95 * count + 99) / 100 ))  # ceil(0.95 * count)
p95=$(awk "NR == $p95_index" "$sorted_latencies")
p99_index=$(( (99 * count + 99) / 100 ))
p99=$(awk "NR == $p99_index" "$sorted_latencies")

echo ""
echo "==== Latency Statistics ===="
echo "Count:  $count"
echo "Min:    $min ms"
echo "Max:    $max ms"
echo "Mean:   $mean ms"
echo "Median: $median ms"
echo "P95:    $p95 ms"
echo "P99:    $p99 ms"
echo "============================"

rm "$all_latencies" "$sorted_latencies"