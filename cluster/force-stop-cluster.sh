#!/usr/bin/env bash

servers=(
    "bbchain1"
    "bbchain2"
    "bbchain3"
    "bbchain4"
    "bbchain5"
    "bbchain6"
    "bbchain7"
    "bbchain8"
    "bbchain9"
)

echo "Stopping runners-ws processes on all servers"
for server in "${servers[@]}"; do
  echo "[$server] stopping processes…"
  ssh $server -T "pkill -f runners-ws || true" &
done
wait

echo "All runners-ws processes stopped."
