#!/usr/bin/env bash
set -euo pipefail
ROOT=/home/gz785/code/assetxchain
EXP=$ROOT/experiments/runtime_noise_latency
DUAL_NODE=$ROOT/target/release/assetxchain-node
SINGLE_NODE=/home/gz785/code/assetxchain-noise-single/target-noise-single/release/assetxchain-node
DRIVER=/home/gz785/code/exp2/scripts/phase4/node
RUN_ID=${RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)}
OUT=$EXP/${RUN_ID}
mkdir -p "${OUT}"
NOISE_LEVELS=${NOISE_LEVELS:-"0 8 16 32"}
REPETITIONS=${REPETITIONS:-5}
PORT=${PORT:-9944}
run_one() {
  local variant="$1"
  local noise="$2"
  local node="$3"
  local log="${OUT}/${variant}_noise${noise}.node.log"
  local result="${OUT}/${variant}_noise${noise}.json"
  echo "starting variant=${variant} noise=${noise}"
  "${node}" --dev --tmp --rpc-cors all --rpc-methods unsafe --rpc-port "${PORT}" --port 30333 >"${log}" 2>&1 &
  local pid=$!
  trap 'kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true' RETURN
  for i in $(seq 1 90); do
    if grep -q "Running JSON-RPC server" "${log}"; then break; fi
    sleep 1
  done
  grep -q "Running JSON-RPC server" "${log}"
  POLKADOT_DRIVER_ROOT="${DRIVER}" ASSETX_RPC="ws://127.0.0.1:${PORT}" \
    STATE_VARIANT="${variant}" NOISE_COUNT="${noise}" REPETITIONS="${REPETITIONS}" \
    OUTPUT="${result}" node "${EXP}/condition.js"
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  trap - RETURN
}
for noise in ${NOISE_LEVELS}; do run_one dual_prefix "${noise}" "${DUAL_NODE}"; done
for noise in ${NOISE_LEVELS}; do run_one single_tree "${noise}" "${SINGLE_NODE}"; done
printf '%s\n' "${OUT}"

