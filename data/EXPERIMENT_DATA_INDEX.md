# AssetXChain Paper 2 Experiment Data Index

Last updated: 2026-08-07

This file is the top-level index for experiment data under `/home/gz785/code/exp2`.
Use it to locate raw outputs, processed summaries, reports, logs, and figures for each run.

## Directory Rules

Canonical experiment root:

```text
/home/gz785/code/exp2
```

Data layout:

```text
/home/gz785/code/exp2/
  results/raw/          raw machine outputs, per-run JSON/CSV/log files
  results/processed/    cleaned CSV/JSON files for analysis and tables
  results/figures/      generated figures, when a run produces figures
  reports/              human-readable run reports
  logs/                 node logs, benchmark stderr logs, probe logs
  scripts/              experiment scripts
  configs/              experiment configs and workloads
```

For paper writing, prefer `results/processed/` and `reports/` first, then use `results/raw/` for audit trails.

Evidence levels are explicit. The July 2026 Experiment 1 G1/G3 artifacts below
remain `legacy-pilot`; they are not part of the Phase 4 main-experiment candidate.
Only a manifest under `results/final/` may confer `paper-final` validity.

## Current Experiment Runs

### Phase 4 S4.8 Formal Correctness Candidate

Candidate ID:

```text
p4-paper-candidate-20260806T004403+0800
```

Status: frozen `paper-candidate` for E1-E5 correctness only. It authorizes the
S4.9/E6 run but contains no E6 data, performance result, ranking, processed
dataset, figure, paper conclusion, or `paper-final` promotion.

Candidate manifest:

```text
/home/gz785/code/exp2/results/candidates/p4-paper-candidate-20260806T004403+0800/candidate.manifest.json
SHA-256: 40c349d225241d08509f5ff9bf688c65da257a853b01eb02aab374499309cca2
```

Source provisional candidate:

```text
p4-e0-provisional-20260804T231736+0800
SHA-256: 4c8130bf2bbf93526628580d4cacf23d2ee25dce42a215b3c92daf722f30a1ac
```

Frozen correctness run IDs:

```text
E1:
  p4-e1-b1-20260804T231910+0800-8514bf07-79eb587a
  p4-e1-b2-20260804T231912+0800-dc10ea68-79eb587a
  p4-e1-g3-20260804T231913+0800-47ebd794-79eb587a
E2:
  p4-e2-b1-20260805T031717+0800-8514bf07-79eb587a
  p4-e2-b2-20260805T031719+0800-dc10ea68-79eb587a
  p4-e2-g3-20260805T031720+0800-47ebd794-79eb587a
E3:
  p4-e3-b1-20260805T145128+0800-8514bf07-79eb587a
  p4-e3-b2-20260805T145130+0800-dc10ea68-79eb587a
  p4-e3-g3-20260805T145131+0800-47ebd794-79eb587a
E4:
  p4-e4-b1-20260805T160453+0800-8514bf07-79eb587a
  p4-e4-b2-20260805T160455+0800-dc10ea68-79eb587a
  p4-e4-g3-20260805T160456+0800-47ebd794-79eb587a
E5:
  p4-e5-b1-20260805T202141+0800-8514bf07-79eb587a
  p4-e5-b2-20260805T202141+0800-dc10ea68-79eb587a
  p4-e5-g3-20260805T202141+0800-47ebd794-79eb587a
```

Gate facts:

```text
42 frozen scenarios: 39 live-chain + 3 test-gate
E1-E5 completion: 705/705 units, 1110/1110 attempts
expected mismatch / I1-I10 violation / false accept / false reject: 0
duplicate / dirty state / event mismatch / timeout: 0
sealed runs independently revalidated: 18 (E0-E5)
E5 SettlementRecords reconciled: 75, exact 24-field schema
G3 authority: Runtime; contract projection non-authoritative and unavailable
```

Raw and logs remain at their immutable per-run paths under
`results/raw/<run_id>` and `logs/<run_id>`. The candidate manifest references
their preflight, build-artifact, checker, seal, and independent recheck hashes;
it does not rewrite the source manifests.

### Phase 4 S4.9 E6 Formal Cost Runs

Status: completed as a three-baseline `formal-cost-candidate` batch. The
canonical completed runs are listed in the completion block below. The earlier
B1/B2/G3 entries in this section are preserved as historical attempts and are
not part of the completed batch, processed results, numerical paper claims, or
`paper-final` promotion.

```text
B1 run_id: p4-e6-b1-20260806T095057+0800-8514bf07-b140542f
status: completed, independently validated formal-cost-candidate
raw: /home/gz785/code/exp2/results/raw/p4-e6-b1-20260806T095057+0800-8514bf07-b140542f/
logs: /home/gz785/code/exp2/logs/p4-e6-b1-20260806T095057+0800-8514bf07-b140542f/
coverage: 30/30 units, 90/90 attempts, three legal operations, R=10
validation: generic checker PASS; E6 batch validator PASS; anomaly_count=0
claim boundary: formal-cost-candidate; fixed-medium-load-only; not-paper-final

B2 run_id: p4-e6-b2-20260806T095057+0800-dc10ea68-b140542f
status: invalid, retained for trace/debug evidence only
raw: /home/gz785/code/exp2/results/raw/p4-e6-b2-20260806T095057+0800-dc10ea68-b140542f/
logs: /home/gz785/code/exp2/logs/p4-e6-b2-20260806T095057+0800-dc10ea68-b140542f/
coverage: 0/30 units, 0/90 attempts; no cost sample created
failure: Linux Node rejected a Windows-form POLKADOT_DRIVER_ROOT while loading
         the common RPC driver for LEG-META-01/r-008

G3 reserved run_id: p4-e6-g3-20260806T095057+0800-47ebd794-b140542f
status: immutable preflight only; not started after B2 failure
raw preflight: /home/gz785/code/exp2/results/raw/p4-e6-g3-20260806T095057+0800-47ebd794-b140542f/manifest.preflight.json
```

All three preflights bind candidate
`p4-paper-candidate-20260806T004403+0800` at SHA-256
`40c349d225241d08509f5ff9bf688c65da257a853b01eb02aab374499309cca2`,
the frozen `formal_cost.json` seed `20260726`, and baseline commits B1
`8514bf0720ec9b5ed3c8d3ec1501264ae7bf1642`, B2
`dc10ea68ffddacf93e6d57d91c9acfa35a4b1bfa`, and G3
`47ebd794c4ef1264bb8265c49d8e92c5b44e6323`. The B2 invalid run and B1
completed run remain immutable; no raw file was edited or deleted, and no
processed/figure/report artifact was generated.

### S4.9 canonical completed E6 batch

Historical status: the raw/log batch remains immutable, but its numerical
weight/fee interpretation was invalidated by the post-dispatch measurement
correction. It is not the current cost source and must not be used for paper
cost claims.

Candidate binding:

```text
candidate: p4-paper-candidate-20260806T004403+0800
candidate manifest SHA-256: 40c349d225241d08509f5ff9bf688c65da257a853b01eb02aab374499309cca2
formal_cost.json SHA-256: b140542f79f3cf999d5669fdd0b13c8fad25ba313308d9bb633acdb41bf2583a
seed: 20260726
scenarios: LEG-META-01, LEG-ISSUE-01, LEG-RESALE-01
repetitions: R=10; warm-up policy: none; fresh-chain cold-start observations retained
```

Canonical run IDs and evidence:

```text
B1 Contract-only:
  run_id: p4-e6-b1-20260806T095057+0800-8514bf07-b140542f
  raw: /home/gz785/code/exp2/results/raw/p4-e6-b1-20260806T095057+0800-8514bf07-b140542f/
  logs: /home/gz785/code/exp2/logs/p4-e6-b1-20260806T095057+0800-8514bf07-b140542f/
  result: completed; 30/30 units; 90/90 attempts; checker PASS; independent validator PASS; anomaly_count=0

B2 Runtime-only:
  run_id: p4-e6-b2-20260806T111142+0800-dc10ea68-b140542f
  raw: /home/gz785/code/exp2/results/raw/p4-e6-b2-20260806T111142+0800-dc10ea68-b140542f/
  logs: /home/gz785/code/exp2/logs/p4-e6-b2-20260806T111142+0800-dc10ea68-b140542f/
  result: completed; 30/30 units; 90/90 attempts; checker PASS; independent validator PASS; anomaly_count=0

G3 AssetXChain-CRS:
  run_id: p4-e6-g3-20260806T111142+0800-47ebd794-b140542f
  raw: /home/gz785/code/exp2/results/raw/p4-e6-g3-20260806T111142+0800-47ebd794-b140542f/
  logs: /home/gz785/code/exp2/logs/p4-e6-g3-20260806T111142+0800-47ebd794-b140542f/
  result: completed; 30/30 units; 90/90 attempts; checker PASS; independent validator PASS; anomaly_count=0
```

All three canonical seals are immutable mode `0444`. Teardown found no owned
runner, executor, or `assetxchain-node --dev --tmp` process and no listener on
9944/30333/9615. The batch is `formal-cost-candidate; fixed-medium-load-only;
not-paper-final`. No processed, figure, or report artifact is created by S4.9;
S4.10 is the next allowed step.

### Phase 4 S4.10 Processed E6 Derivatives

Historical status: completed deterministic derivatives of the superseded E6
measurement contract. These artifacts remain immutable but are invalid for
numerical cost claims; the corrected S4.10 roots are indexed below with the
corrected S4.11 outputs.

```text
B1 processed:
  /home/gz785/code/exp2/results/processed/p4-e6-b1-20260806T095057+0800-8514bf07-b140542f/
  attempts: 90
  attempts.jsonl SHA-256: 7d6fa38cdb3548b9db3d21445e2a07968301861120a33a5a5b5d780a996ec7b1
  derivation.json SHA-256: 88363879f7dbd4321b51ae9decb4f14473b5f56fef5168649136b50d6aa56715

B2 processed:
  /home/gz785/code/exp2/results/processed/p4-e6-b2-20260806T111142+0800-dc10ea68-b140542f/
  attempts: 90
  attempts.jsonl SHA-256: eaed22bc9cc9ac01ddce3c64171f5dceb86dfa328a0fceb62e2fdcfa09686170
  derivation.json SHA-256: cb6db8bbcbc13ce629a8c579f0b7ee678e3acaf05cafe156849ca0f50c45563b

G3 processed:
  /home/gz785/code/exp2/results/processed/p4-e6-g3-20260806T111142+0800-47ebd794-b140542f/
  attempts: 90
  attempts.jsonl SHA-256: 44281aa285c63b25a0b0520262eed22893038ed0393411c91d84ecd3f54e53d7
  derivation.json SHA-256: 5db9ee0b8fbb348cc0b15ff197c13dbf90d26bc65e705a330abf3eb95f8453e0

Main B1/B2/G3-only aggregate:
  /home/gz785/code/exp2/results/processed/p4-e6-main-p4-paper-candidate-20260806T004403+0800/
  comparison.json SHA-256: c794b531a4c868ee41e502e1891685f68045d4500dd27f9e5a3c511ba118cd23
  comparison.csv SHA-256: 80911a86faf62905645153d999fc0ad8036c2ba813e7d7bcc8daaf691d8adbb7
```

Each per-run root contains immutable mode-`0444` `attempts.jsonl`,
`correctness.csv`, `summary.json`, `derivation.json`, and the empty E6
`test-gates.jsonl`. The versioned processor validated 874 sealed raw/log
artifacts per run before derivation and rebuilt all outputs byte-identically on
a second invocation. The aggregate scope is exactly
`main-experiment-B1-B2-G3-only`; L1/L2 are excluded. S4.11 is the next allowed
step.

### Experiment 1, G1 Full Pure Contract RPC Run

Run ID:

```text
20260710_1703_g1_full
```

Status: complete. This is the current preferred G1 baseline data run.

Purpose:

- Deploy the pure contract market baseline.
- Keep order state, asset ownership, certificate ownership, and settlement evidence in contract storage.
- Exercise MetaToken ownership purchase, RightToken issue-type purchase, RightToken transfer-type purchase, and asset-order cancellation.
- Record transaction fees, dispatch weights, contract dry-run gas, blocks, latency, and events.
- Check post-trade contract state invariants for asset owner and certificate holder consistency.

Data locations:

```text
Raw:
/home/gz785/code/exp2/results/raw/exp1_g1_full_20260710_1703_g1_full/
/home/gz785/code/exp2/results/raw/exp1_g1_full_20260710_1703_g1_full/exp1_g1_full_20260710_1703_g1_full_raw.json

Processed:
/home/gz785/code/exp2/results/processed/exp1_g1_full_20260710_1703_g1_full_transactions.csv
/home/gz785/code/exp2/results/processed/exp1_g1_full_20260710_1703_g1_full_summary.json

Report:
/home/gz785/code/exp2/reports/exp1_g1_full_20260710_1703_g1_full_report.md

Scenario script:
/home/gz785/code/exp2/scripts/scenarios/exp1_g1_full_rpc.js

Contract source and deployable wasm:
/home/gz785/code/exp2/contracts/g1_pure_market/
/home/gz785/code/exp2/contracts/g1_pure_market/target/wasm32v1-none/release/g1_pure_market.import_memory.no_bulk.mem16_stack64k.deploy_call_only.wasm
```

Quick facts:

```text
transactions: 15
successful transactions: 15
failed transactions: 0
blocks covered: 801-815
invariant checks: 3
invariant violations: 0
```

Key event evidence is in the raw JSON under `events`, including:

```text
contracts.Instantiated
contracts.Called
contracts.ContractEmitted
contracts.StorageDepositTransferredAndHeld
system.ExtrinsicSuccess
```

State consistency evidence is in:

```text
/home/gz785/code/exp2/results/processed/exp1_g1_full_20260710_1703_g1_full_summary.json
metadata.correctness.invariant_checks
```

### Experiment 1, G3 Full Cooperative RPC Run

Run ID:

```text
20260710_1544_g3_full
```

Status: complete. This is the current preferred G3 data run.

Purpose:

- Deploy the market contract.
- Register and authorize the market.
- Exercise MetaToken ownership settlement through contract-runtime cooperation.
- Exercise RightToken issuance and RightToken transfer through runtime-controlled settlement paths.
- Record transaction fees, dispatch weights, contract dry-run gas, blocks, latency, and events.

Data locations:

```text
Raw:
/home/gz785/code/exp2/results/raw/exp1_g3_full_20260710_1544_g3_full/
/home/gz785/code/exp2/results/raw/exp1_g3_full_20260710_1544_g3_full/exp1_g3_full_20260710_1544_g3_full_raw.json

Processed:
/home/gz785/code/exp2/results/processed/exp1_g3_full_20260710_1544_g3_full_transactions.csv
/home/gz785/code/exp2/results/processed/exp1_g3_full_20260710_1544_g3_full_summary.json

Report:
/home/gz785/code/exp2/reports/exp1_g3_full_20260710_1544_g3_full_report.md

Node logs:
/home/gz785/code/exp2/logs/g3_full_node_current.log
/home/gz785/code/exp2/logs/g3_full_node_debug.log
```

Quick facts:

```text
transactions: 19
successful transactions: 19
failed transactions: 0
blocks covered: 5-23
```

Key event evidence is in the raw JSON under `events`, including:

```text
dataAssets.AssetTransferred
dataAssets.TradeSettled
dataAssets.CertificateIssued
dataAssets.CertificateTransferred
markets.MarketRegistered
contracts.Called
contracts.ContractEmitted
```

### Experiment 1, G3 Runtime Benchmark/Probe Batch

Run ID:

```text
20260709_234807
```

Status: complete for the earlier runtime/probe-style G3 batch. This run is useful for pallet-level transaction data, benchmark outputs, and early experiment reporting. It is not the full contract-runtime RPC run.

Data locations:

```text
Raw:
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_234807/

Processed:
/home/gz785/code/exp2/results/processed/exp1_g3_20260709_234807_summary.csv
/home/gz785/code/exp2/results/processed/exp1_g3_20260709_234807_summary.json

Figures:
/home/gz785/code/exp2/results/figures/exp1_g3_20260709_234807_latency_db.svg
/home/gz785/code/exp2/results/figures/exp1_g3_20260709_234807_weight_ref_time.svg

Reports:
/home/gz785/code/exp2/reports/exp1_g3_20260709_234807_report.md
/home/gz785/code/exp2/reports/exp1_g3_20260709_234807_report_zh.md

Logs:
/home/gz785/code/exp2/logs/exp1_g3_20260709_234807/
```

Important raw files:

```text
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_234807/run_metadata.json
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_234807/pallet_markets__register_market.json
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_234807/pallet_dataassets__register_asset.json
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_234807/pallet_dataassets__authorize_operator.json
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_234807/pallet_dataassets__transfer_asset.json
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_234807/pallet_dataassets__issue_certificate.json
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_234807/pallet_dataassets__transfer_certificate.json
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_234807/pallet_dataassets__revoke_authorization.json
```

### Intermediate Experiment 1 G3 Runs

These runs are earlier or partial batches. Keep them for traceability, but do not use them as the main paper data unless explicitly comparing setup attempts.

```text
Run:
exp1_g3_20260709_233719

Raw:
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_233719/

Logs:
/home/gz785/code/exp2/logs/exp1_g3_20260709_233719/
```

```text
Run:
exp1_g3_20260709_234339

Raw:
/home/gz785/code/exp2/results/raw/exp1_g3_20260709_234339/

Logs:
/home/gz785/code/exp2/logs/exp1_g3_20260709_234339/
```

### Incomplete Full-G3 Attempts

These directories were created during failed or interrupted full G3 attempts. They are retained only as trace/debug evidence.

```text
/home/gz785/code/exp2/results/raw/exp1_g3_full_20260710_1335_full/
/home/gz785/code/exp2/results/raw/exp1_g3_full_20260710_1539_g3_full/
```

Use `20260710_1544_g3_full` as the complete full-G3 dataset.

### G1 Diagnostic Attempts

These directories were created while preparing the G1 contract wasm and correcting a getter-return decoding issue. They are retained only as trace/debug evidence.

```text
/home/gz785/code/exp2/results/raw/exp1_g1_full_20260710_1650_g1_full/
/home/gz785/code/exp2/results/raw/exp1_g1_full_20260710_1659_g1_full/
```

Use `20260710_1703_g1_full` as the complete full-G1 dataset.

## Probe Files

Probe files are small checks used to validate environment, runtime metadata, test availability, and individual benchmark calls. They are useful for debugging but are not final experiment tables.

```text
/home/gz785/code/exp2/results/raw/probe_register_asset.json
/home/gz785/code/exp2/results/raw/probe_register_market.json
/home/gz785/code/exp2/results/raw/probe_authorize_operator.json
/home/gz785/code/exp2/results/raw/probe_bench_register_asset.json
/home/gz785/code/exp2/results/raw/probe_dataassets_settlement_tests.log
/home/gz785/code/exp2/results/raw/probe_market_orderbook_g3_tests.log
/home/gz785/code/exp2/logs/probe_bench_register_asset.err
```

## How To Add Future Runs

For each new run, add a short entry with:

```text
run_id:
status:
purpose:
raw data path:
processed data path:
report path:
log path:
key correctness evidence:
notes on whether the run is complete or exploratory:
```

Use these conventions:

```text
Raw directory:
/home/gz785/code/exp2/results/raw/<run_id>/

Processed files:
/home/gz785/code/exp2/results/processed/<run_id>_summary.json
/home/gz785/code/exp2/results/processed/<run_id>_summary.csv

Report:
/home/gz785/code/exp2/reports/<run_id>_report.md

Logs:
/home/gz785/code/exp2/logs/<run_id>/
```

For full G3 RPC runs, keep the raw event dump, transaction CSV, summary JSON, and report together. The raw JSON should preserve transaction receipts and event evidence for authorization, settlement consistency, RightToken support, and auditability checks.

### Phase 4 S4.11 Tables, Summaries, and Figures

Historical status: completed at 2026-08-06T15:30:26+08:00, then invalidated for
numerical cost claims by the E6 post-dispatch measurement correction. Its
correctness component remains consistent with the unchanged E1-E5 evidence.
The versioned generator
scripts/phase4/generate_s411_artifacts.py (SHA-256
9b98aba1f64ab0fc689a6168ada1b6c14bcead89779e5ab8fa83e0980e271d7e) produced
descriptive tables and SVG figures without editing raw or S4.10 inputs.

Table root:
/home/gz785/code/exp2/results/processed/p4-s4.11-p4-paper-candidate-20260806T004403+0800/

Correctness outputs comprise 15 rows for E1-E5 x B1/B2/G3 and the candidate
gate totals: 705 completed units, 1110 completed attempts, zero expected
outcome mismatches, false accepts, false rejects, invariant violations,
duplicates, dirty state, event mismatches, and timeouts; E5 records 75
successful SettlementRecords with 24 fields each.

The E6 cost table contains only the canonical B1, B2, and G3 runs:
p4-e6-b1-20260806T095057+0800-8514bf07-b140542f,
p4-e6-b2-20260806T111142+0800-dc10ea68-b140542f, and
p4-e6-g3-20260806T111142+0800-47ebd794-b140542f. It has 126 rows and reports
n, median, mean, sample standard deviation, and p10-p90 for 14 metrics across
the nine baseline/operation cells. Per-order phase calls are reported
separately. That historical derivative encoded setup calls as N/A because the
processed schema had no setup field; treat those cells as uncollected rather
than architecture-inapplicable. The corrected `-v2` derivative below uses an
explicit `not-collected` label. N/A is not converted to zero. L1/L2 and the historical invalid B2 run
p4-e6-b2-20260806T095057+0800-dc10ea68-b140542f are excluded.

Figure root:
/home/gz785/code/exp2/results/figures/p4-s4.11-p4-paper-candidate-20260806T004403+0800/

The figure root contains latency, partial-fee, and weight-ref-time SVGs, each
with nine baseline/operation bars and p10-p90 whiskers. All SVG files parse as
XML. An isolated rebuild matched the core tables and figures byte-for-byte;
only output-location metadata differs in derivation.json. No inferential
significance test, scalability curve, paper text, or paper-final claim was
generated. Validity remains paper-candidate plus formal-cost-candidate,
fixed-medium-load-only, and not-paper-final.

### Phase 4 Corrected S4.11 Tables, Summaries, and Figures

Status: completed at 2026-08-07T11:44:53+08:00. Correctness remains
`paper-final correctness only`; corrected cost remains
`formal-cost-candidate; fixed-medium-load-only; not-paper-final` pending S4.12.

Corrected source contract:

```text
workload: configs/workloads/formal_cost_v2.json
workload SHA-256: 9a8b56c26c449b97560857c29fb7adde208f4ebd4878eabc29d7de2973283530
B1: p4-e6-b1-20260807T032215+0800-8514bf07-9a8b56c2
B2: p4-e6-b2-20260807T032218+0800-dc10ea68-9a8b56c2
G3: p4-e6-g3-20260807T032220+0800-47ebd794-9a8b56c2
```

Corrected processed attempts SHA-256 values are B1
`fba9c202ece8afd94e630d24e3a1da90719275ca4e2621b16d0af97f3987b5de`,
B2 `f489278a042627950f4dd5ae4063bbc46dc473b8c340b637b6b6812b8ec5a475`,
and G3 `dde02af420b9b96e3106277f3d64d2e58a59b6e9e551fb80539075103ce32449`.

Generator:

```text
/home/gz785/code/exp2/scripts/phase4/generate_s411_artifacts.py
SHA-256: dd2ddd8ecc06537b42294572f372173d6aff7aa62c13620440b01fe60f8e9667
```

Table root:

```text
/home/gz785/code/exp2/results/processed/p4-s4.11-p4-paper-candidate-20260806T004403+0800-9a8b56c2-v2/
```

The root contains `correctness_runs.csv` (15 rows), `correctness_gate.csv`,
`e6_cost_table.csv` (126 complete-order rows),
`e6_phase_cost_table.csv` (378 phase-detail rows),
`e6_order_call_counts.csv` (nine baseline/operation cells),
`tables_and_figures.md`, and `derivation.json`. Each measured primary cell has
`n=10`: create_order, fund_order, and settle are summed within a repetition
before descriptive statistics are computed. Each order has exactly three
required phase calls. Setup calls are labeled `not-collected` because they are
outside the processed E6 schema, not because they are architecture-inapplicable;
architecture-inapplicable metrics remain N/A with null source values and are
never encoded as zero.

Figure root:

```text
/home/gz785/code/exp2/results/figures/p4-s4.11-p4-paper-candidate-20260806T004403+0800-9a8b56c2-v2/
```

The root contains complete-order latency, actual transaction fee, and terminal
post-dispatch ref_time SVGs. Each shows the nine B1/B2/G3 x legal-operation
cells with median bars and p10-p90 whiskers. All three SVGs parse as XML and
passed 1180x620 raster visual inspection without clipping or overlap.

Independent validation recomputed all 126 primary rows and 378 phase rows from
the corrected processed attempts, confirmed the 705-unit/1110-attempt
correctness gate and 75 complete 24-field SettlementRecords, verified B1/B2/G3
scope with L1/L2 excluded, and reproduced six core table/report files and three
SVGs byte-identically in an isolated rebuild. No sample was deleted, and no
inferential significance, ranking, causal, or scalability conclusion was
generated. S4.12 is the next allowed step.

The preserved non-`v2` corrected S4.11 roots ending in `-9a8b56c2/` are a
superseded derivative attempt: they mislabeled uncollected setup calls as
architecture N/A. They are not raw runs, no run ID was invalidated, and they
must not be selected instead of the `-v2` roots above.
