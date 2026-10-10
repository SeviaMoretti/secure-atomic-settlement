#!/usr/bin/env bash
set -uo pipefail

plan_root="${1:-/home/gz785/code/exp2/reports/r3/x1-diagnostic-plans-v2-20260808}"
result_root="/home/gz785/code/exp2/results/diagnostics"
executor="/home/gz785/code/exp2/scripts/r2/x1_live_unit_executor.js"
runner="/home/gz785/code/exp2/scripts/r2/x1_live_runner.py"
export PATH="/home/gz785/.local/node-v24.15.0-linux-x64/bin:$PATH"

mapfile -t plans < <(find "$plan_root" -maxdepth 1 -type f -name "*.json" | sort)
total=${#plans[@]}
if [[ $total -ne 36 ]]; then
    echo "BATCH_BLOCKED expected=36 actual=$total"
    exit 2
fi

for index in "${!plans[@]}"; do
    plan=${plans[$index]}
    run_id=$(jq -r .run_id "$plan")
    node=$(jq -r .artifacts.node.path "$plan")
    ordinal=$((index + 1))
    echo "START $ordinal/$total $run_id"
    python3 "$runner" \
        --plan "$plan" \
        --node "$node" \
        --executor "$executor" \
        --output-root "$result_root/$run_id"
    rc=$?
    if [[ $rc -ne 0 ]]; then
        echo "BATCH_BLOCKED $ordinal/$total $run_id rc=$rc"
        exit "$rc"
    fi
done

echo "BATCH_PASS $total/$total"
