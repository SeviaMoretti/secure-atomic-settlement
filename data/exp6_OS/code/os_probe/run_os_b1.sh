#!/bin/sh
# OS-B1-PROBE runner: start b1-control node, run probe, stop node.
cd ~/code/exp2 || exit 1
NODE=~/code/exp2/build/r3-x1-formal-artifacts-20260808-v1/b1-control/node/assetxchain-node
META=~/code/exp2/build/r3-x1-formal-artifacts-20260808-v1/b1-control/ink/market_pure_contract.json
WASM=~/code/exp2/build/r3-x1-formal-artifacts-20260808-v1/b1-control/ink/market_pure_contract.wasm
LOG=/tmp/os_b1_node.log
OUT=/tmp/os_b1_probe.out
rm -f "$LOG" "$OUT"
"$NODE" --dev --tmp --rpc-cors all --rpc-methods unsafe > "$LOG" 2>&1 &
NODE_PID=$!
echo "node pid $NODE_PID"
sleep 8
POLKADOT_DRIVER_ROOT=/home/gz785/code/exp2/scripts/phase4/node \
B1_CONTRACT_METADATA="$META" \
B1_CONTRACT_WASM="$WASM" \
/home/gz785/.local/node-v24.15.0-linux-x64/bin/node scripts/os_probe/os_b1_probe.js > "$OUT" 2>&1
RC=$?
kill "$NODE_PID" 2>/dev/null || true
echo "probe exit code: $RC"
echo "----- probe output -----"
cat "$OUT"
