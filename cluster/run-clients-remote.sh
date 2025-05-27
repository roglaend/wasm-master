#!/usr/bin/env bash
set -euo pipefail

# Tunables
TARGET="release"
BIN="./modular-ws-client"
TOTAL_REQUESTS=100000
NUM_PROCESSES=10
LOGICAL_CLIENTS_PER_PROCESS=50
TIMEOUT_SECS=120
MODE="persistent"
LEADER="152.94.162.14:60000"
BENCHMARK_NAME="crash-test"
BENCHMARK_COUNT=$((NUM_PROCESSES * LOGICAL_CLIENTS_PER_PROCESS))
BENCHMARK="${BENCHMARK_NAME}_${BENCHMARK_COUNT}"
WASM_FILENAME="composed_paxos_ws_client.wasm"

# List of servers to run the clients on
servers=(
  "bbchain1"
  # "bbchain2"
  # "bbchain3"
)

NUM_REQUESTS=$((TOTAL_REQUESTS / (NUM_PROCESSES * LOGICAL_CLIENTS_PER_PROCESS)))

echo "Creating log directory on target servers…"
for server in "${servers[@]}"; do
  echo "[$server] creating log directory $BENCHMARK"
  ssh "$server" -T "
    REMOTE_DIR=\"\$HOME/master_files\"
    REMOTE_LOG_DIR=\"\$REMOTE_DIR/logs/$BENCHMARK\"
    mkdir -p \"\$REMOTE_LOG_DIR\"
  "
done

echo "Starting client processes on each target server…"
for server in "${servers[@]}"; do
  echo "[$server] starting clients…"
  ssh "$server" -T "
    REMOTE_DIR=\"\$HOME/master_files\"
    REMOTE_LOG_DIR=\"\$REMOTE_DIR/logs/$BENCHMARK\"
    WASM_PATH=\"\$REMOTE_DIR/$WASM_FILENAME\"

    cd \"\$REMOTE_DIR\" || { echo \"Failed to cd into \$REMOTE_DIR\"; exit 1; }

    pids=()
    for (( p=0; p<$NUM_PROCESSES; p++ )); do
      OFFSET=\$((p * $LOGICAL_CLIENTS_PER_PROCESS))
      echo \"Starting client group \$p with offset=\$OFFSET\"
      ( nohup $BIN \
            --client-id \"\$OFFSET\" \
            --client-id-offset \"\$OFFSET\" \
            --num-logical-clients $LOGICAL_CLIENTS_PER_PROCESS \
            --num-requests $NUM_REQUESTS \
            --mode $MODE \
            --leader $LEADER \
            --timeout-secs $TIMEOUT_SECS \
            --wasm \"\$WASM_PATH\" \
          >> \"\$REMOTE_LOG_DIR/client\${p}.log\" 2>&1 & )
      pids+=( \$! )
    done
    for pid in \"\${pids[@]}\"; do
      wait \"\$pid\"
    done
    echo \"All client groups completed on \$server!\"
  " &
done

wait
echo "All client processes completed on server(s)!"


