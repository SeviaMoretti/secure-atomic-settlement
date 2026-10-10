# Phase 4 formal tooling contract

These scripts implement S4.1 readiness only. They do not authorize or contain a
formal result. The first live use is S4.2/E0.

## Entry points

- `phase4_manifest.py build`: validate and print a preflight without creating a run.
- `phase4_manifest.py create`: exclusively create `manifest.preflight.json` before build or node launch.
- `phase4_runner.py plan`: validate exact scenario/repetition coverage without running anything.
- `phase4_runner.py run`: build, execute one fresh `--dev --tmp` node per live unit, check, tear down, and seal.
- `phase4_checker.py`: recompute expected outcomes, Q_ALL cleanliness, I1-I10, and duplicate/race gates.
- `process_formal.py`: derive deterministic processed rows and reports only from completed sealed raw.
- `validate_phase4_readiness.py`: read-only S4.1 contract, Git, environment, and test audit.

## Executor argv contract

The preflight records and hashes the exact baseline executor argv. For every
planned unit, the runner appends:

```text
--preflight <manifest.preflight.json>
--scenario-id <frozen scenario ID>
--repetition <one-based integer>
--execution-mode <live-chain|test-gate>
--output-root <fresh repetition directory>
--rpc ws://127.0.0.1:9944
```

For `live-chain`, the executor uses the selected Phase 3 baseline adapter and a
real RPC/contract transport. It must create `unit-manifest.json` and one ordered
`attempt-NNN` directory per catalog attempt containing `invocation.json`,
`dispatch.json`, `pre-state.json`, `post-state.json`, `events.json`, and
`result.json`. The result uses `schemas/phase4/formal-result.schema.json` and the
snapshots use the frozen Phase 3 normalized snapshot schema.

For `test-gate`, no node is running. The executor runs the frozen baseline unit
test for the named `CLEAN-ABORT-*` scenario and writes `test-gate.json`; it must
state that Q_ALL rolled back and that no production failpoint was exposed.

Missing common capability, missing transport, timeout, malformed output, or an
unmapped error is a hard run failure. The executor must not fabricate a reject,
timeout, N/A value, success event, snapshot, or settlement record.

## Immutability

Preflight and seal use exclusive creation. The seal inventories exact bytes under
raw and logs, excluding only the seal itself. Any retry or code/config/input
change requires a new run ID. Processed outputs accept an identical rebuild but
refuse a differing overwrite.
