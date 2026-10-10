# S4.11 Tables and Figures

Candidate: p4-paper-candidate-20260806T004403+0800; manifest SHA-256: 40c349d225241d08509f5ff9bf688c65da257a853b01eb02aab374499309cca2.

Validity: paper-candidate correctness plus formal-cost-candidate; fixed-medium-load-only; not-paper-final. Main experiment contains B1/B2/G3 only; L1/L2 are excluded.

## Correctness gate

Correctness rows are copied from the immutable S4.8 candidate manifest. S4.11 does not re-read or modify E1-E5 raw files.

| Completed units | Completed attempts | Expected mismatches | False accepts | False rejects | Invariant violations | Duplicates | Dirty state | Event mismatches | Timeouts | SettlementRecords | Fields/record |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 705 | 1110 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 75 | 24 |

## E6 cost summary

The complete CSV reports n, median, mean, standard deviation, and p10-p90 for every required metric. N/A denotes architecture inapplicability, not zero. Setup calls are absent from the processed E6 schema and remain N/A; the measured per-order call total is three phases.

| Baseline | Operation | Latency median (ms) | Partial fee median (plancks) | Weight ref_time median | Storage writes median | Events median | n |
|---|---|---:|---:|---:|---:|---:|---:|
| B1 Contract-only | Meta sale | 5893.500 | 4.983e+09 | 4.874e+09 | 44 | 2 | 30 |
| B1 Contract-only | Right issue | 5895 | 6.579e+09 | 6.470e+09 | 49 | 3 | 30 |
| B1 Contract-only | Right resale | 5898.500 | 5.080e+09 | 4.972e+09 | 44 | 2 | 30 |
| B2 Runtime-only | Meta sale | 5920.500 | 1.646e+09 | 1.538e+09 | 32 | 2 | 30 |
| B2 Runtime-only | Right issue | 5916 | 1.646e+09 | 1.538e+09 | 37 | 3 | 30 |
| B2 Runtime-only | Right resale | 5922.500 | 1.646e+09 | 1.538e+09 | 32 | 2 | 30 |
| G3 AssetXChain-CRS | Meta sale | 5911 | 1.001e+12 | 1.001e+12 | 48 | 3 | 30 |
| G3 AssetXChain-CRS | Right issue | 5906.500 | 1.001e+12 | 1.001e+12 | 47 | 3 | 30 |
| G3 AssetXChain-CRS | Right resale | 5908.500 | 1.001e+12 | 1.001e+12 | 46 | 3 | 30 |

No inferential significance test or scalability curve is produced. These fixed-medium-load observations support descriptive comparison only; paper-final promotion remains S4.12.

## Figures

- s4_11_latency_median_ms.svg: median latency with p10-p90 whiskers.
- s4_11_partial_fee_median_plancks.svg: median partial fee with p10-p90 whiskers.
- s4_11_weight_ref_time_median.svg: median weight ref_time with p10-p90 whiskers.
