"""Check raw attempts, including state, events and 24-field settlement records."""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from e7_preflight import ROOT, load, sha

sys.path.insert(0, str(ROOT / 'scripts/phase4'))
import phase4_checker as shared


def check(root):
    root = Path(root).resolve()
    manifest = load(root / 'manifest.completed.json')
    failures = shared.CheckFailure()
    errors = []
    rows = []
    seen = set()
    if manifest['status'] != 'COMPLETED':
        errors.append('batch did not complete')
    if not manifest['planned_units']:
        errors.append('empty batch cannot pass')
    catalog = load(ROOT / 'configs/phase4/formal-scenarios-v1.json')
    attempts = 0
    for entry in manifest['units']:
        before = len(failures.rows)
        try:
            path = Path(entry['result'])
            if not path.resolve().is_relative_to(root):
                raise ValueError('result escapes batch directory')
            if sha(path) != entry['result_sha256']:
                raise ValueError('unit hash mismatch')
            unit = load(path)
            key = tuple(unit[k] for k in ('baseline_id', 'operation', 'variant', 'repetition'))
            if key in seen:
                raise ValueError('duplicate experiment unit')
            seen.add(key)
            for field, unit_field in [('baseline', 'baseline_id'), ('operation', 'operation'),
                                      ('variant', 'variant'), ('repetition', 'repetition')]:
                if entry[field] != unit[unit_field]:
                    raise ValueError(f'unit identity differs: {field}')
            scenario = next(dict(s) for s in catalog['scenarios']
                            if s['scenario_id'] == unit['source_scenario_id'])
            scenario['operation'] = unit['operation']
            if unit['variant'] == 'V2':
                scenario['attempts'] = [
                    {**attempt, 'expected_error_code': 'MalformedCommitment'}
                    for attempt in scenario['attempts']
                ]
            if unit['variant'] == 'V1' and unit['operation'] == 'TransferRight':
                scenario['attempts'] = [
                    {**attempt, 'phase': 'fund_order', 'expected_error_code': 'RightInvalid'}
                    for attempt in scenario['attempts']
                ]
            baseline = load(ROOT / 'configs/baselines' / (unit['baseline_id'].lower() + '.json'))
            preflight = dict(run_id=manifest['batch_id'],
                             baseline=dict(baseline_id=unit['baseline_id'],
                                           baseline_name=baseline['baseline_name'], head=baseline['git']['head']),
                             inputs=dict(seed=2026091907,
                                         workload=dict(path='configs/tdsc_v1/e7-v1.json'),
                                         workload_id='tdsc-e7-composite-attacks-v1'))
            dirs = sorted(path.parent.glob('attempt-*'))
            if len(dirs) != len(scenario['attempts']) or len(dirs) != unit['attempt_count']:
                raise ValueError('attempt count differs from predeclared scenario')
            if len(unit['result_summary']) != len(dirs):
                raise ValueError('summary attempt count differs')
            results, evidence = [], []
            same_block_boundary = unit['variant'] in {'V3', 'V4', 'Q1'}
            boundary_exemptions = []
            for directory, expected, summary in zip(dirs, scenario['attempts'], unit['result_summary']):
                before_attempt_failures = len(failures.rows)
                result, block = shared._check_attempt(directory, preflight, scenario, expected,
                                                      unit['repetition'], failures, shared._settlement_fixture())
                if same_block_boundary:
                    # A loser in a genuinely overlapping submission set is
                    # executed after the winner inside the same block.  The
                    # generic phase-4 checker assumes every attempt has a
                    # block-1 pre-state and therefore reports boundary/Q_ALL
                    # noise for this valid intra-block observation.  Keep the
                    # raw dispatch and concurrency evidence, while removing
                    # only those two generic diagnostics for E7 V3/V4.
                    retained = failures.rows[:before_attempt_failures]
                    for failure in failures.rows[before_attempt_failures:]:
                        if failure.get('code') in {'block-evidence', 'dirty-state'}:
                            boundary_exemptions.append({
                                'attempt_id': expected['attempt_id'],
                                'code': failure['code'],
                                'message': failure.get('message'),
                            })
                        else:
                            retained.append(failure)
                    failures.rows[:] = retained
                for field in ('protocol_outcome', 'common_error_code', 'included_block', 'extrinsic_index'):
                    if summary[field] != result['outcome'][field]:
                        raise ValueError('summary differs from raw outcome: ' + field)
                if summary['attempt_id'] != expected['attempt_id']:
                    raise ValueError('summary attempt identity differs')
                if result['outcome']['included_block'] is None:
                    raise ValueError('request not included; cannot count as business rejection')
                results.append(result)
                evidence.append(block)
                attempts += 1
            shared._check_submission_sequences(results, scenario, unit['repetition'], failures)
            if unit['variant'] in {'V3', 'V4'}:
                if unit['variant'] == 'V4':
                    probe = load(path.parent / 'composite-probe.json')
                    if not probe.get('signature_retained'):
                        raise ValueError('V4 field replacement did not retain the original signature')
                    if probe.get('original_value') == probe.get('mutated_value'):
                        raise ValueError('V4 composite probe did not replace a signed field')
                    if not probe.get('dispatch_error'):
                        raise ValueError('V4 composite field probe was unexpectedly accepted')
                concurrency = load(path.parent / 'e7-concurrency.json')
                submissions = concurrency.get('submissions', [])
                if concurrency.get('concurrency') != 4 or len(submissions) != 4:
                    raise ValueError('concurrent submission count is not four')
                if not concurrency.get('overlap_observed'):
                    raise ValueError('RPC overlap window was not observed')
                positions = [(x.get('included_block'), x.get('extrinsic_index')) for x in submissions]
                if len(set(positions)) != 4:
                    raise ValueError('concurrent attempts do not have unique inclusion positions')
                accepted = [x for x in results if x['outcome']['protocol_outcome'] == 'accepted']
                rejected = [x for x in results if x['outcome']['protocol_outcome'] == 'rejected']
                if len(accepted) != 1 or len(rejected) != 3:
                    raise ValueError('concurrent race did not produce one success and three rejections')
            elif unit['variant'] == 'Q1':
                concurrency = load(path.parent / 'e7-concurrency.json')
                submissions = concurrency.get('submissions', [])
                if concurrency.get('concurrency') != 2 or len(submissions) != 2:
                    raise ValueError('Q1 concurrent candidate count is not two')
                if not concurrency.get('overlap_observed'):
                    raise ValueError('Q1 RPC overlap window was not observed')
                positions = [(x.get('included_block'), x.get('extrinsic_index')) for x in submissions]
                if len(set(positions)) != 2:
                    raise ValueError('Q1 candidates do not have unique inclusion positions')
                candidate_results = [r for r in results if r['request']['phase'] == 'fund_order']
                if len([r for r in candidate_results if r['outcome']['protocol_outcome'] == 'accepted']) != 1 \
                        or len([r for r in candidate_results if r['outcome']['protocol_outcome'] == 'rejected']) != 1:
                    raise ValueError('Q1 did not produce one accepted and one rejected candidate')
                settle_results = [r for r in results if r['request']['phase'] == 'settle']
                if len(settle_results) != 1 or settle_results[0]['outcome']['protocol_outcome'] != 'accepted':
                    raise ValueError('Q1 winner settlement was not accepted')
            else:
                shared._check_attempt_continuity(results, evidence, scenario, unit['repetition'], failures)
            rows.append(dict(baseline=unit['baseline_id'], operation=unit['operation'],
                             variant=unit['variant'], repetition=unit['repetition'], attempts=len(dirs),
                             same_block_boundary_exemptions=boundary_exemptions,
                             status='PASS' if len(failures.rows) == before else 'FAIL'))
        except Exception as exc:
            errors.append(f'{entry["unit_id"]}: {type(exc).__name__}: {exc}')
    if len(rows) != manifest['planned_units'] or len(manifest['units']) != manifest['planned_units']:
        errors.append('completed unit count differs from plan')
    # V0 regression cannot establish E7 coverage even when every raw assertion passes.
    return dict(schema_id='assetxchain-tdsc-e7-checker', schema_version=2,
                batch_id=manifest['batch_id'], checked_units=len(rows), checked_attempts=attempts,
                status='FAIL' if errors or failures.rows else 'PASS', errors=errors,
                failures=failures.rows, rows=rows, evidence_scope='S0 regression',
                e7_pilot_complete=False, e7_formal_ready=False)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('batch')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    report = check(args.batch)
    output = args.output or ROOT / 'results/processed' / report['batch_id'] / 'checker-report.json'
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open('x') as handle:
        json.dump(report, handle, indent=2)
        handle.write('\n')
    print(json.dumps({k: report[k] for k in ('batch_id', 'checked_units', 'checked_attempts', 'status', 'errors', 'failures')}))
    return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
