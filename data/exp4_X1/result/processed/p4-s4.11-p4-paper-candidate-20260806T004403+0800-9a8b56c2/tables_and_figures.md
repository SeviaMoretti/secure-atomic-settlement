# S4.11 Tables and Figures

Candidate: p4-paper-candidate-20260806T004403+0800; manifest SHA-256: 40c349d225241d08509f5ff9bf688c65da257a853b01eb02aab374499309cca2.

Validity: paper-final correctness only plus corrected formal-cost-candidate; fixed-medium-load-only; corrected cost is not paper-final. Main experiment contains B1/B2/G3 only; L1/L2 are excluded.

## Correctness gate

Correctness rows are copied from the immutable S4.8 candidate manifest. S4.11 does not re-read or modify E1-E5 raw files.

| Completed units | Completed attempts | Expected mismatches | False accepts | False rejects | Invariant violations | Duplicates | Dirty state | Event mismatches | Timeouts | SettlementRecords | Fields/record |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 705 | 1110 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 75 | 24 |

## E6 cost summary

The primary cost table first sums create_order, fund_order, and settle within each repetition, then reports n, median, mean, sample standard deviation, and p10-p90 across the ten complete orders. The phase table preserves the three components separately. N/A denotes architecture inapplicability, not zero. Setup calls are absent from the processed E6 schema and remain N/A; every measured order contains three required phase calls.

The fee column is the sum of TransactionFeePaid.actual_fee for the three extrinsics. The weight column is the sum of their terminal post-dispatch ref_time values; neither column uses paymentInfo estimates or the G3 declared contract gas limit.

| Baseline | Operation | Complete-order latency median (ms) | Actual fee median (plancks) | Post-dispatch ref_time median | Storage writes median | Events median | n |
|---|---|---:|---:|---:|---:|---:|---:|
| B1 Contract-only | Meta sale | 17718 | 1.764e+10 | 1.764e+10 | 131 | 7 | 10 |
| B1 Contract-only | Right issue | 17736 | 1.982e+10 | 1.982e+10 | 137 | 8 | 10 |
| B1 Contract-only | Right resale | 17693 | 1.670e+10 | 1.670e+10 | 127 | 6 | 10 |
| B2 Runtime-only | Meta sale | 17786 | 5.138e+09 | 5.138e+09 | 101 | 6 | 10 |
| B2 Runtime-only | Right issue | 17778.500 | 5.138e+09 | 5.138e+09 | 108 | 7 | 10 |
| B2 Runtime-only | Right resale | 17776.500 | 5.138e+09 | 5.138e+09 | 96 | 5 | 10 |
| G3 AssetXChain-CRS | Meta sale | 17758.500 | 9.427e+09 | 9.427e+09 | 142 | 9 | 10 |
| G3 AssetXChain-CRS | Right issue | 17685.500 | 9.429e+09 | 9.429e+09 | 145 | 9 | 10 |
| G3 AssetXChain-CRS | Right resale | 17736 | 9.432e+09 | 9.432e+09 | 138 | 9 | 10 |

No inferential significance test or scalability curve is produced. These fixed-medium-load observations support descriptive comparison only; corrected cost evidence remains pending S4.12 independent review.

## Figures

- s4_11_order_latency_median_ms.svg: complete-order latency median with p10-p90 whiskers.
- s4_11_order_actual_fee_median_plancks.svg: complete-order actual transaction fee median with p10-p90 whiskers.
- s4_11_order_weight_ref_time_median.svg: complete-order post-dispatch ref_time median with p10-p90 whiskers.
