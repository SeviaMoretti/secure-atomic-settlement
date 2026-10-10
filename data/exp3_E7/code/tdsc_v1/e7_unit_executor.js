'use strict';

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const base = require('./e7_base_executor');

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!argv[i].startsWith('--') || argv[i + 1] === undefined) throw new Error(`bad argument ${argv[i]}`);
    out[argv[i].slice(2).replaceAll('-', '_')] = argv[i + 1];
  }
  for (const key of ['baseline', 'operation', 'variant', 'repetition', 'output_root', 'rpc', 'run_id']) {
    if (!out[key]) throw new Error(`--${key.replaceAll('_', '-')} is required`);
  }
  out.repetition = Number(out.repetition);
  return out;
}

function sha256(value) {
  return crypto.createHash('sha256').update(value).digest('hex');
}

function loadJson(file) { return JSON.parse(fs.readFileSync(file, 'utf8')); }
function writeExclusive(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, { encoding: 'utf8', flag: 'wx' });
}

const OP_LEGAL = {
  TransferMeta: 'LEG-META-01', IssueRight: 'LEG-ISSUE-01', TransferRight: 'LEG-RESALE-01',
};
const OP_STALE = {
  TransferMeta: 'NEG-VERSION-01', IssueRight: 'NEG-AUTH-REVOKED-01', TransferRight: 'NEG-RIGHT-REVOKED-01',
};

function sourceScenario(catalog, operation, variant) {
  let id;
  if (variant === 'V0') id = OP_LEGAL[operation];
  else if (variant === 'V1') id = operation === 'TransferRight' ? 'NEG-RIGHT-EXPIRED-01' : 'NEG-AUTH-REVOKED-01';
  else if (variant === 'V2') id = 'NEG-SIGN-01';
  else if (variant === 'V3' || variant === 'V4') id = 'RACE-RELAYER-01';
  else if (variant === 'Q1') id = 'RACE-QUOTA-01';
  else throw new Error(`${variant} requires the concurrent E7 executor`);
  const found = catalog.scenarios.find((item) => item.scenario_id === id);
  if (!found) throw new Error(`source scenario missing ${id}`);
  const scenario = {
    ...found,
    attempts: found.attempts.map((attempt) => ({ ...attempt })),
    operation, e7_variant: variant, source_scenario_id: id,
  };
  if (variant === 'V2') {
    scenario.attempts = scenario.attempts.map((attempt) => ({
      ...attempt, expected_error_code: 'MalformedCommitment',
    }));
  }
  if (variant === 'V1' && operation === 'TransferRight') {
    scenario.attempts = scenario.attempts.map((attempt) => ({
      ...attempt, phase: 'fund_order', expected_error_code: 'RightInvalid',
    }));
  }
  return scenario;
}

function family(_variant) { return 'E7'; }

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!['B1', 'B2', 'G3'].includes(args.baseline)) throw new Error(`bad baseline ${args.baseline}`);
  if (!Object.keys(OP_LEGAL).includes(args.operation)) throw new Error(`bad operation ${args.operation}`);
  if (args.variant === 'Q1' && args.operation !== 'IssueRight') throw new Error('Q1 applies only to IssueRight');
  const root = '/home/gz785/code/exp2';
  const baseline = loadJson(path.join(root, 'configs/baselines', `${args.baseline.toLowerCase()}.json`));
  const catalog = loadJson(path.join(root, 'configs/phase4/formal-scenarios-v1.json'));
  const scenario = sourceScenario(catalog, args.operation, args.variant);
  const outputRoot = path.resolve(args.output_root);
  fs.mkdirSync(outputRoot, { recursive: true });
  const preflight = {
    run_id: args.run_id,
    created_at: new Date().toISOString(),
    experiment_family: family(args.variant),
    baseline: {
      baseline_id: args.baseline,
      baseline_name: baseline.baseline_name,
      head: baseline.git.head,
      worktree: baseline.git.worktree,
      dirty: false,
    },
    inputs: {
      workload: { path: 'configs/tdsc_v1/e7-v1.json' },
      workload_id: 'tdsc-e7-composite-attacks-v1',
      seed: 2026091907,
      e7_variant: args.variant,
      attack_entry: 'registered-market-relay-without-client-precheck',
    },
  };
  const context = base.contextFor({
    rpc: args.rpc,
    repetition: args.repetition,
    output_root: outputRoot,
    artifact_root: args.artifact_root || null,
  }, preflight, scenario);
  let run;
  if (args.baseline === 'B1') run = await base.runB1(context);
  else if (args.baseline === 'B2') run = await base.runB2(context);
  else run = await base.runG3(context);
  try {
    const unit = {
      schema_id: 'assetxchain-tdsc-e7-unit', schema_version: 1,
      run_id: args.run_id, baseline_id: args.baseline, operation: args.operation,
      variant: args.variant, source_scenario_id: scenario.source_scenario_id,
      repetition: args.repetition, status: 'PASS',
      genesis_hash: run.fixture.genesis_hash,
      fixture_anchor_block: run.fixture.fixture_anchor_block,
      start_block: run.startBlock, end_block: run.endBlock,
      attempt_count: context.results.length,
      results_sha256: sha256(Buffer.from(JSON.stringify(context.results))),
      result_summary: context.results.map((item) => ({
        attempt_id: item.request.attempt_id,
        expected_outcome: item.request.expected_outcome,
        protocol_outcome: item.outcome.protocol_outcome,
        common_error_code: item.outcome.common_error_code,
        included_block: item.outcome.included_block,
        extrinsic_index: item.outcome.extrinsic_index,
        settlement_record_count: item.settlement.settlement_record_count,
      })),
    };
    const cachePath = path.join(outputRoot, 'malicious-market-cache.json');
    writeExclusive(cachePath, {
      schema_id: 'assetxchain-tdsc-e7-malicious-market-cache', schema_version: 1,
      market_variant: 'registered-market-relay-without-client-precheck',
      local_precheck: false,
      cached_intent: {
        order_id: run.fixture.order_id,
        cached_status: 'Tradable',
        cached_at_block: run.fixture.fixture_anchor_block,
      },
      cache_retained_after_rpc_attempt: true,
      authority_source: args.baseline === 'G3' ? 'runtime' : 'baseline-authority',
      claim_boundary: 'local optimistic cache is observed separately; it is not settlement authority',
    });
    const concurrencyPath = path.join(outputRoot, 'e7-concurrency.json');
    unit.evidence_files = {
      malicious_market_cache: { path: 'malicious-market-cache.json', sha256: sha256(fs.readFileSync(cachePath)) },
      concurrency: fs.existsSync(concurrencyPath)
        ? { path: 'e7-concurrency.json', sha256: sha256(fs.readFileSync(concurrencyPath)) }
        : null,
      composite_probe: fs.existsSync(path.join(outputRoot, 'composite-probe.json'))
        ? { path: 'composite-probe.json', sha256: sha256(fs.readFileSync(path.join(outputRoot, 'composite-probe.json'))) }
        : null,
    };
    writeExclusive(path.join(outputRoot, 'e7-unit.json'), unit);
    process.stdout.write(`${JSON.stringify(unit)}\n`);
  } finally {
    await run.api.disconnect();
  }
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message || String(error)}\n`);
  process.exit(1);
});
