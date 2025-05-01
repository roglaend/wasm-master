#!/usr/bin/env bash
set -euo pipefail

# —————————————————————————————————————————————————————————————
# **Edit this one line** on each server to your assigned node ID
NODE_ID=3
# —————————————————————————————————————————————————————————————

CONFIG=config.prod.yaml
TARGET=release
LOG_LEVEL=info

RUST_LOG=$LOG_LEVEL \
  ./target/$TARGET/modular-ws \
    --node-id $NODE_ID \
    --config $CONFIG
