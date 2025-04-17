#!/bin/bash

NUM_CLIENTS=1
NUM_REQUESTS=10

for (( i=0; i<NUM_CLIENTS; i++ ))
do
  echo "Starting client $i"
  ./target/debug/modular-ws-test --client-id "$i" --num-requests "$NUM_REQUESTS" > "client_$i.log" 2>&1 &
done

wait
echo "All clients finished."