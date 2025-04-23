#!/bin/bash

# NUM_CLIENTS=1
# NUM_REQUESTS=1

# for (( i=0; i<NUM_CLIENTS; i++ ))
# do
#   echo "Starting client $i"
#   ./target/debug/modular-ws-test --client-id "$i" --num-requests "$NUM_REQUESTS" > "client_$i.log" 2>&1 &
# done

# wait
# echo "All clients finished."

NUM_CLIENTS=1
NUM_REQUESTS=10
DEADLINE=20

pids=()

# 1) launch them in background and collect PIDs
for (( i=0; i<NUM_CLIENTS; i++ )); do
  echo "Starting client $i…"
  ./target/release/modular-ws-test \
    --client-id "$i" \
    --num-requests "$NUM_REQUESTS" \
  > "client_${i}.log" 2>&1 &
  pids+=( $! )
done

# 2) wait up to $DEADLINE seconds for all of them to exit
start=$SECONDS
while (( ${#pids[@]} > 0 && SECONDS - start < DEADLINE )); do
  for idx in "${!pids[@]}"; do
    pid=${pids[idx]}
    if ! kill -0 "$pid" 2>/dev/null; then
      # process is gone → remove from list
      unset 'pids[idx]'
    fi
  done
  sleep 1
done

# 3) if any are still alive, kill them
if (( ${#pids[@]} > 0 )); then
  echo "⏰ Time’s up, killing ${#pids[@]} stuck clients…"
  kill "${pids[@]}"
fi

echo "All clients finished or were timed out."