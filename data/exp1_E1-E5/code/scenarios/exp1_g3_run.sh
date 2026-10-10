#!/usr/bin/env bash
set -euo pipefail

EXP_ROOT="${EXP_ROOT:-/home/gz785/code/exp2}"
PROJECT_ROOT="${PROJECT_ROOT:-/home/gz785/code/assetxchain}"
BENCH_NODE="${BENCH_NODE:-$PROJECT_ROOT/target-bench/release/assetxchain-node}"
STEPS="${STEPS:-5}"
REPEAT="${REPEAT:-10}"
RUN_ID="${RUN_ID:-$(date +%Y%m%d_%H%M%S)}"

RAW_DIR="$EXP_ROOT/results/raw/exp1_g3_$RUN_ID"
PROCESSED_DIR="$EXP_ROOT/results/processed"
LOG_DIR="$EXP_ROOT/logs/exp1_g3_$RUN_ID"
REPORT_DIR="$EXP_ROOT/reports"
BENCH_FAILURES="$RAW_DIR/benchmark_failures.csv"

mkdir -p "$RAW_DIR" "$PROCESSED_DIR" "$LOG_DIR" "$REPORT_DIR"

cd "$PROJECT_ROOT"

if [[ "${FORCE_BUILD:-0}" == "1" || ! -x "$BENCH_NODE" ]]; then
  CARGO_TARGET_DIR="$PROJECT_ROOT/target-bench" cargo build --release --features runtime-benchmarks -p assetxchain-node \
    >"$LOG_DIR/build_runtime_benchmarks.log" 2>&1
fi

cat > "$RAW_DIR/run_metadata.json" <<JSON
{
  "run_id": "$RUN_ID",
  "group": "G3",
  "experiment": "experiment_1_end_to_end_overhead",
  "project_root": "$PROJECT_ROOT",
  "exp_root": "$EXP_ROOT",
  "bench_node": "$BENCH_NODE",
  "steps": $STEPS,
  "repeat": $REPEAT,
  "git_commit": "$(git rev-parse --short HEAD)",
  "git_status_short": $(git status --short | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))'),
  "rustc": "$(rustc --version)",
  "cargo": "$(cargo --version)",
  "uname": "$(uname -a)",
  "node_command": "assetxchain-node benchmark pallet --dev --json",
  "chain_mode": "benchmark harness with --dev genesis",
  "rpc_endpoint": "not used by benchmark harness",
  "random_seed": "deterministic FRAME benchmark accounts",
  "notes": "Contract deployment is not executed because cargo-contract is not installed; market_orderbook tests validate the G3 contract-to-chain-extension order path."
}
JSON

printf 'pallet,extrinsic,status,stderr_file\n' > "$BENCH_FAILURES"

run_bench() {
  local pallet="$1"
  local extrinsic="$2"
  local out="$RAW_DIR/${pallet}__${extrinsic}.json"
  local err="$LOG_DIR/${pallet}__${extrinsic}.stderr.log"
  if "$BENCH_NODE" benchmark pallet \
    --dev \
    --pallet "$pallet" \
    --extrinsic "$extrinsic" \
    --steps "$STEPS" \
    --repeat "$REPEAT" \
    --json >"$out" 2>"$err"; then
    return 0
  fi

  printf '%s,%s,failed,%s\n' "$pallet" "$extrinsic" "$err" >> "$BENCH_FAILURES"
  return 0
}

run_bench pallet_markets register_market
run_bench pallet_dataassets register_asset
run_bench pallet_dataassets authorize_operator
run_bench pallet_dataassets transfer_asset
run_bench pallet_dataassets issue_certificate
run_bench pallet_dataassets transfer_certificate
run_bench pallet_dataassets revoke_authorization

cargo test -p market_orderbook -- --nocapture \
  > "$RAW_DIR/market_orderbook_tests.log" 2>&1

cargo test -p pallet-dataassets market -- --nocapture \
  > "$RAW_DIR/dataassets_market_tests.log" 2>&1

python3 "$EXP_ROOT/scripts/common/process_exp1_g3.py" \
  --raw-dir "$RAW_DIR" \
  --processed-dir "$PROCESSED_DIR" \
  --report-dir "$REPORT_DIR" \
  --run-id "$RUN_ID"

echo "$RUN_ID"
