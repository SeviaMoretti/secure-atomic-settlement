#!/bin/sh
# OS-LEGACY-PROBE runner: start M1-updated G3 node, deploy OS-Legacy contract, run probe.
cd ~/code/exp2 || exit 1
NODE=/home/gz785/code/assetxchain-g3-v2/target/release/assetxchain-node
WASM=/home/gz785/code/exp2/contracts/os_legacy/target/ink/os_legacy_market.wasm
LOG=/tmp/os_legacy_node.log
OUT=/tmp/os_legacy_probe.out
rm -f "$LOG" "$OUT"
"$NODE" --dev --tmp --rpc-cors all --rpc-methods unsafe > "$LOG" 2>&1 &
NODE_PID=$!
echo "node pid $NODE_PID"
sleep 8
POLKADOT_DRIVER_ROOT=/home/gz785/code/exp2/scripts/phase4/node \
OS_LEGACY_WASM="$WASM" \
/home/gz785/.local/node-v24.15.0-linux-x64/bin/node scripts/os_probe/os_legacy_probe.js > "$OUT" 2>&1
RC=$?
kill "$NODE_PID" 2>/dev/null || true
echo "probe exit code: $RC"
echo "----- probe output -----"
cat "$OUT"
