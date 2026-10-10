'use strict';

const crypto = require('crypto');
const fs = require('fs');
const Module = require('module');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const EXTRINSIC_INDEX_KEY = Buffer.from(':extrinsic_index', 'utf8').toString('hex');
const COMMON_METRICS = [
  'latency_ms', 'completion_blocks', 'partial_fee', 'weight_ref_time',
  'weight_proof_size', 'storage_writes', 'events_count', 'rpc_response_ms',
  'phase_call_count',
];
const ARCH_METRICS = [
  'contract_gas_or_ref_time', 'contract_storage_writes',
  'runtime_dispatch_count', 'runtime_storage_writes', 'typed_ce_call_count',
  'projection_storage_writes',
];
const ARCH_APPLICABLE = {
  B1: new Set(['contract_gas_or_ref_time', 'contract_storage_writes']),
  B2: new Set(['runtime_dispatch_count', 'runtime_storage_writes']),
  G3: new Set(ARCH_METRICS),
};
const CLAIM_BOUNDARY = 'formal-cost-candidate; fixed-medium-load-only; not-paper-final';
const G3_FIXED_OUTER_CONTRACT_GAS_LIMIT = {
  refTime: '1000000000000',
  proofSize: '104857600',
};

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function sha256Bytes(value) {
  return crypto.createHash('sha256').update(value).digest('hex');
}

function sha256File(filePath) {
  return sha256Bytes(fs.readFileSync(filePath));
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  }
  return value;
}

function jsonBytes(value) {
  return Buffer.from(`${JSON.stringify(stable(value))}\n`, 'utf8');
}

function writeJsonExclusive(filePath, value) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, jsonBytes(value), { flag: 'wx' });
}

function elapsedMs(started) {
  const value = Number(process.hrtime.bigint() - started) / 1_000_000;
  return Math.max(0.001, Number(value.toFixed(3)));
}

function weightField(weight, camel, snake) {
  const value = weight && (weight[camel] ?? weight[snake]);
  assert(value !== undefined && value !== null, `transaction weight omitted ${camel}`);
  return String(value);
}

function parseOwnArgs(argv) {
  const ownNames = new Set([
    'frozen-executor', 'frozen-executor-sha256', 'candidate-manifest',
    'candidate-sha256', 'validator', 'plan-builder',
  ]);
  const own = {};
  const forwarded = [];
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith('--') || !ownNames.has(token.slice(2))) {
      forwarded.push(token);
      continue;
    }
    const value = argv[index + 1];
    assert(value !== undefined && !value.startsWith('--'), `missing value for ${token}`);
    assert(own[token.slice(2)] === undefined, `duplicate ${token}`);
    own[token.slice(2)] = value;
    index += 1;
  }
  for (const name of ownNames) assert(own[name], `--${name} is required`);
  return { own, forwarded };
}

function argValue(argv, name) {
  const index = argv.indexOf(name);
  assert(index >= 0 && argv[index + 1], `${name} is required`);
  return argv[index + 1];
}

function replaceArg(argv, name, value) {
  const output = [...argv];
  const index = output.indexOf(name);
  assert(index >= 0 && output[index + 1], `${name} is required`);
  output[index + 1] = value;
  return output;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function verifyCandidate(preflight, own) {
  assert(sha256File(own['candidate-manifest']) === own['candidate-sha256'],
    'approved candidate manifest hash differs from the E6 command');
  const candidate = readJson(own['candidate-manifest']);
  const mode = fs.statSync(own['candidate-manifest']).mode & 0o777;
  assert(mode === 0o444, `approved candidate mode must be 0444, got ${mode.toString(8)}`);
  assert(candidate.status === 'frozen', 'approved candidate is not frozen');
  assert(candidate.validity_level === 'paper-candidate', 'approved candidate is not paper-candidate');
  assert(candidate.authorization && candidate.authorization.e6_authorized === true,
    'approved candidate does not authorize E6');
  assert(candidate.authorization.performance_claims_authorized === false,
    'S4.8 candidate must not authorize performance claims');
  assert(preflight.prerequisites.approved_s4_8_candidate_id === candidate.candidate_id,
    'E6 preflight candidate ID differs from the hashed candidate manifest');
  return candidate;
}

function replaceOnce(source, before, after, label) {
  const index = source.indexOf(before);
  assert(index >= 0, `frozen executor patch point is absent: ${label}`);
  assert(source.indexOf(before, index + before.length) < 0,
    `frozen executor patch point is ambiguous: ${label}`);
  return `${source.slice(0, index)}${after}${source.slice(index + before.length)}`;
}

function e6Source(frozenPath, expectedHash) {
  const bytes = fs.readFileSync(frozenPath);
  assert(sha256Bytes(bytes) === expectedHash, 'frozen correctness executor hash drift');
  let source = bytes.toString('utf8');
  const legal = "  E0: ['LEG-META-01', 'LEG-ISSUE-01', 'LEG-RESALE-01'],\n";
  source = replaceOnce(
    source,
    legal,
    `${legal}  E6: ['LEG-META-01', 'LEG-ISSUE-01', 'LEG-RESALE-01'],\n`,
    'E6 family',
  );
  source = replaceOnce(
    source,
    "  const expectedRepetitions = family === 'E0' ? 2 : 5;",
    "  const expectedRepetitions = family === 'E0' ? 2 : (family === 'E6' ? 10 : 5);",
    'E6 repetitions',
  );
  source = replaceOnce(
    source,
    "    weight_proof_size: weight.proofSize,\n    events_count: eventNames.length,",
    "    weight_proof_size: weight.proofSize,\n    storage_writes: tx.storageWrites,\n    events_count: eventNames.length,\n    rpc_response_ms: tx.rpcResponseMs,",
    'common E6 metrics',
  );
  source = replaceOnce(
    source,
    "function costPackage(tx, eventNames) {\n  const weight = parseWeight(tx.paymentWeight);\n  const measured = {\n    latency_ms: tx.latencyMs === undefined ? null : tx.latencyMs,\n    completion_blocks: 0,\n    partial_fee: tx.partialFee === undefined ? null : tx.partialFee,\n    weight_ref_time: weight.refTime,\n    weight_proof_size: weight.proofSize,",
    "function costPackage(tx, eventNames) {\n  const weight = parseWeight(tx.postDispatchWeight);\n  const measured = {\n    latency_ms: tx.latencyMs === undefined ? null : tx.latencyMs,\n    completion_blocks: 0,\n    partial_fee: tx.actualFee === undefined ? null : tx.actualFee,\n    weight_ref_time: weight.refTime,\n    weight_proof_size: weight.proofSize,",
    'actual E6 cost metrics',
  );
  source = replaceOnce(
    source,
    "function architecturePackage(baseline, tx, contractGas = null, typedCeCall = baseline === 'G3') {\n  const measured = {};",
    "function architecturePackage(baseline, tx, contractGas = null, typedCeCall = baseline === 'G3') {\n  const measured = {};\n  if (baseline === 'B1') measured.contract_storage_writes = tx.contractStorageWrites;\n  if (baseline === 'B2') measured.runtime_storage_writes = tx.runtimeStorageWrites;\n  if (baseline === 'G3') {\n    measured.contract_storage_writes = tx.contractStorageWrites;\n    measured.runtime_storage_writes = tx.runtimeStorageWrites;\n    measured.projection_storage_writes = tx.projectionStorageWrites;\n  }",
    'architecture E6 metrics',
  );
  source = replaceOnce(
    source,
    "    claim_boundary: context.preflight.experiment_family === 'E0'\n      ? 'E0-reproducibility-only; not-performance-evidence'\n      : 'formal-correctness-only; not-performance-evidence',",
    `    claim_boundary: context.preflight.experiment_family === 'E6'\n      ? '${CLAIM_BOUNDARY}'\n      : (context.preflight.experiment_family === 'E0'\n        ? 'E0-reproducibility-only; not-performance-evidence'\n        : 'formal-correctness-only; not-performance-evidence'),`,
    'E6 claim boundary',
  );
  source += '\nmodule.exports.__assetxE6Main = main;\n';
  return source;
}

function decodeExtrinsicIndex(value) {
  if (typeof value !== 'string') return null;
  const match = /^Some\(([0-9a-fA-F]+)\)$/.exec(value);
  if (!match || match[1].length < 8) return null;
  return Buffer.from(match[1].slice(0, 8), 'hex').readUInt32LE(0);
}

function palletPrefixes(xxhashAsHex) {
  const names = [
    'System', 'Balances', 'TransactionPayment', 'Contracts', 'MarketRuntime',
    'DataAssets', 'Markets',
  ];
  return Object.fromEntries(names.map((name) => [xxhashAsHex(name, 128).slice(2), name]));
}

function traceSummary(traceJson, extrinsicIndex, baseline, prefixes) {
  const blockTrace = traceJson.blockTrace || traceJson.block_trace || traceJson;
  const events = blockTrace.events || [];
  let activeExtrinsic = null;
  const writes = [];
  for (const event of events) {
    if (event.target !== 'state') continue;
    const data = event.data || {};
    const values = data.stringValues || data.string_values || {};
    const key = String(values.key || '').replace(/^0x/, '').toLowerCase();
    const method = values.method;
    if (key === EXTRINSIC_INDEX_KEY && method === 'Put') {
      activeExtrinsic = decodeExtrinsicIndex(values.value);
      continue;
    }
    if (activeExtrinsic !== extrinsicIndex || !new Set(['Put', 'Delete', 'Append', 'ClearPrefix']).has(method)) {
      continue;
    }
    const childId = values.child_id || values.childId || null;
    const pallet = prefixes[key.slice(0, 32)] || (childId ? 'ContractsChild' : 'Unknown');
    writes.push({
      method,
      key,
      child_id: childId,
      pallet,
      ext_id: values.ext_id || null,
    });
  }
  const contractWrites = writes.filter((item) => (
    item.pallet === 'Contracts' || item.pallet === 'ContractsChild'
    || item.key.startsWith(Buffer.from(':child_storage:', 'utf8').toString('hex'))
  )).length;
  let runtimeWrites = null;
  if (baseline === 'B2') runtimeWrites = writes.length;
  if (baseline === 'G3') runtimeWrites = writes.length - contractWrites;
  return {
    write_call_count: writes.length,
    unique_key_count: new Set(writes.map((item) => `${item.child_id || ''}:${item.key}`)).size,
    contract_write_call_count: contractWrites,
    runtime_write_call_count: runtimeWrites,
    projection_write_call_count: baseline === 'G3' ? contractWrites : null,
    writes,
  };
}

function patchSendTx(preflight, measurements) {
  const commonPath = path.resolve(__dirname, '../scenarios/s314_live_common.js');
  const common = require(commonPath);
  const { xxhashAsHex } = common.dep('@polkadot/util-crypto');
  const prefixes = palletPrefixes(xxhashAsHex);
  const original = common.sendTx;
  common.sendTx = async function e6SendTx(api, label, tx, signer, expectedFailure = null) {
    const submitted = await original(api, label, tx, signer, expectedFailure);
    const rpcStarted = process.hrtime.bigint();
    await api.rpc.chain.getHeader(submitted.blockHash);
    const rpcResponseMs = elapsedMs(rpcStarted);
    const traceStarted = process.hrtime.bigint();
    const traceCodec = await api.rpc.state.traceBlock(submitted.blockHash, '', '', '');
    const traceRpcResponseMs = elapsedMs(traceStarted);
    const traceJson = typeof traceCodec.toJSON === 'function' ? traceCodec.toJSON() : traceCodec;
    const apply = (submitted.events || []).find((record) => (
      record.phase && record.phase.isApplyExtrinsic
    ));
    assert(apply, `transaction ${label} has no apply-extrinsic phase`);
    const extrinsicIndex = apply.phase.asApplyExtrinsic.toNumber();
    const summary = traceSummary(
      traceJson, extrinsicIndex, preflight.baseline.baseline_id, prefixes,
    );
    assert(summary.write_call_count > 0, `storage trace for ${label} has no writes`);
    submitted.rpcResponseMs = rpcResponseMs;
    submitted.storageWrites = summary.write_call_count;
    submitted.contractStorageWrites = summary.contract_write_call_count;
    submitted.runtimeStorageWrites = summary.runtime_write_call_count;
    submitted.projectionStorageWrites = summary.projection_write_call_count;
    measurements.push({
      label,
      transaction_hash: submitted.txHash,
      included_block_hash: submitted.blockHash,
      included_block: submitted.blockNumber,
      extrinsic_index: extrinsicIndex,
      latency_ms: submitted.latencyMs,
      partial_fee: submitted.actualFee,
      actual_fee: submitted.actualFee,
      estimated_partial_fee: submitted.estimatedPartialFee,
      weight_ref_time: weightField(submitted.postDispatchWeight, 'refTime', 'ref_time'),
      weight_proof_size: weightField(submitted.postDispatchWeight, 'proofSize', 'proof_size'),
      post_dispatch_weight: submitted.postDispatchWeight,
      declared_payment_weight: submitted.declaredPaymentWeight,
      declared_contract_gas_limit: submitted.declaredContractGasLimit,
      metric_source: {
        fee: submitted.feeEvent,
        weight: submitted.dispatchEvent,
      },
      transaction_event_count: submitted.eventRows.length,
      rpc_response_ms: rpcResponseMs,
      rpc_probe_method: 'chain_getHeader(included_block)',
      storage_trace_rpc_response_ms: traceRpcResponseMs,
      storage_trace_response_sha256: sha256Bytes(jsonBytes(traceJson)),
      storage: summary,
    });
    return submitted;
  };
  return () => {
    common.sendTx = original;
  };
}

function walkFiles(root) {
  const rows = [];
  function walk(current) {
    for (const name of fs.readdirSync(current).sort()) {
      const item = path.join(current, name);
      const stat = fs.statSync(item);
      if (stat.isDirectory()) walk(item);
      else rows.push(item);
    }
  }
  walk(root);
  return rows;
}

function addMeasurementEvidence(tempRoot, preflight, measurements, own) {
  const measuredHashes = new Set();
  const phases = {};
  for (const attemptDir of fs.readdirSync(tempRoot)
    .filter((name) => name.startsWith('attempt-')).sort()
    .map((name) => path.join(tempRoot, name))) {
    const block = readJson(path.join(attemptDir, 'block-events.json'));
    const result = readJson(path.join(attemptDir, 'result.json'));
    const invocation = readJson(path.join(attemptDir, 'invocation.json'));
    const measurement = measurements.find((item) => item.transaction_hash === block.transaction_hash);
    assert(measurement, `missing E6 trace measurement for ${block.transaction_hash}`);
    measuredHashes.add(measurement.transaction_hash);
    phases[invocation.phase] = (phases[invocation.phase] || 0) + 1;
    writeJsonExclusive(path.join(attemptDir, 'e6-measurement.json'), {
      schema_id: 'assetxchain-exp2-phase4-e6-attempt-measurement',
      schema_version: 2,
      run_id: preflight.run_id,
      baseline_id: preflight.baseline.baseline_id,
      scenario_id: invocation.scenario_id,
      repetition: invocation.repetition,
      attempt_id: invocation.attempt_id,
      phase: invocation.phase,
      operation: invocation.operation,
      transaction_hash: measurement.transaction_hash,
      included_block_hash: measurement.included_block_hash,
      extrinsic_index: measurement.extrinsic_index,
      metric_definition: {
        partial_fee: 'transactionPayment.TransactionFeePaid.actual_fee for this extrinsic',
        weight_ref_time: 'system.ExtrinsicSuccess/Failed dispatch_info.weight.refTime for this extrinsic',
        weight_proof_size: 'system.ExtrinsicSuccess/Failed dispatch_info.weight.proofSize for this extrinsic',
        estimated_partial_fee: 'paymentInfo.partialFee; declared estimate only',
        declared_payment_weight: 'paymentInfo.weight; declared estimate only',
        declared_contract_gas_limit: 'contracts.call WeightV2 gas limit; declared bound only',
        storage_writes: 'state_traceBlock write calls for the measured extrinsic, excluding :extrinsic_index',
        rpc_response_ms: measurement.rpc_probe_method,
        contract_storage_writes: 'Contracts pallet and contract child-storage write calls',
        runtime_storage_writes: 'B2 total writes; G3 non-Contracts writes in the measured extrinsic',
        projection_storage_writes: 'G3 contract storage writes; N/A outside G3',
      },
      raw_measurement: measurement,
      result_cost: result.cost,
      architecture_cost: result.architecture_cost,
      warm_up: false,
      retained: true,
      anomaly_flags: [],
    });
  }
  assert(JSON.stringify(phases) === JSON.stringify({ create_order: 1, fund_order: 1, settle: 1 }),
    `E6 measured phase count differs: ${JSON.stringify(phases)}`);
  const setup = measurements.filter((item) => !measuredHashes.has(item.transaction_hash));
  const sumNumber = (name) => Number(setup.reduce((total, item) => total + Number(item[name] || 0), 0).toFixed(3));
  const sumInteger = (name) => setup.reduce((total, item) => total + BigInt(item[name] || 0), 0n).toString();
  const setupStorageWrites = setup.reduce((total, item) => total + item.storage.write_call_count, 0);
  writeJsonExclusive(path.join(tempRoot, 'e6-unit-cost.json'), {
    schema_id: 'assetxchain-exp2-phase4-e6-unit-cost',
    schema_version: 2,
    run_id: preflight.run_id,
    baseline_id: preflight.baseline.baseline_id,
    scenario_id: argValue(process.argv.slice(2), '--scenario-id'),
    repetition: Number(argValue(process.argv.slice(2), '--repetition')),
    workload_id: preflight.inputs.workload_id,
    seed: preflight.inputs.seed,
    candidate: {
      candidate_id: preflight.prerequisites.approved_s4_8_candidate_id,
      manifest_path: path.resolve(own['candidate-manifest']),
      manifest_sha256: own['candidate-sha256'],
    },
    order_policy: {
      source: 'preflight.planned_execution',
      seed: preflight.inputs.seed,
      warm_up_policy: 'none; every R=10 sample is a retained fresh-chain cold-start observation',
      anomaly_policy: 'record all flags and retain every raw sample; no manual deletion',
    },
    cost_boundary: {
      build_cost: 'excluded from transaction cost and retained in build/runner logs',
      one_time_setup: 'reported separately below and never added to per-order metrics',
      per_order: 'three measured phases in attempt result files',
      amortized_setup_denominator_orders: 1,
    },
    setup: {
      transaction_count: setup.length,
      labels: setup.map((item) => item.label),
      latency_ms_sum: sumNumber('latency_ms'),
      partial_fee_sum: sumInteger('partial_fee'),
      weight_ref_time_sum: sumInteger('weight_ref_time'),
      weight_proof_size_sum: sumInteger('weight_proof_size'),
      storage_write_call_count_sum: setupStorageWrites,
      rpc_response_ms_sum: sumNumber('rpc_response_ms'),
      transactions: setup,
    },
    per_order: {
      phase_call_counts: phases,
      transaction_count: measuredHashes.size,
      attempt_measurement_files: fs.readdirSync(tempRoot)
        .filter((name) => name.startsWith('attempt-')).sort()
        .map((name) => `${name}/e6-measurement.json`),
    },
    anomalies: [],
    retained: true,
    claim_boundary: CLAIM_BOUNDARY,
  });
}

function validateTemp(tempRoot, preflightPath, own) {
  const completed = spawnSync('python3', [
    own.validator, 'unit', tempRoot,
    '--preflight', preflightPath,
    '--candidate-manifest', own['candidate-manifest'],
    '--candidate-sha256', own['candidate-sha256'],
  ], { encoding: 'utf8' });
  assert(completed.status === 0,
    `E6 unit validator failed: ${completed.stderr || completed.stdout}`);
  const report = JSON.parse(completed.stdout);
  writeJsonExclusive(path.join(tempRoot, 'e6-unit-validation.json'), report);
}

function rebuildUnitManifest(tempRoot) {
  const manifestPath = path.join(tempRoot, 'unit-manifest.json');
  const manifest = readJson(manifestPath);
  fs.rmSync(manifestPath);
  manifest.artifact_inventory = walkFiles(tempRoot).map((filePath) => ({
    path: path.relative(tempRoot, filePath).split(path.sep).join('/'),
    bytes: fs.statSync(filePath).size,
    sha256: sha256File(filePath),
  }));
  return manifest;
}

function publishExclusive(tempRoot, finalRoot, unitManifest) {
  assert(fs.existsSync(finalRoot) && fs.readdirSync(finalRoot).length === 0,
    'formal E6 output root must exist and be empty');
  for (const source of walkFiles(tempRoot)) {
    const relative = path.relative(tempRoot, source);
    const target = path.join(finalRoot, relative);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.copyFileSync(source, target, fs.constants.COPYFILE_EXCL);
  }
  writeJsonExclusive(path.join(finalRoot, 'unit-manifest.json'), unitManifest);
}

async function main() {
  const parsed = parseOwnArgs(process.argv.slice(2));
  const preflightPath = path.resolve(argValue(parsed.forwarded, '--preflight'));
  const finalRoot = path.resolve(argValue(parsed.forwarded, '--output-root'));
  const preflight = readJson(preflightPath);
  assert(preflight.experiment_family === 'E6', 'E6 executor rejects non-E6 preflight');
  assert(preflight.inputs.repetitions === 10, 'E6 requires R=10');
  assert(preflight.inputs.seed === 20260726, 'E6 seed drift');
  verifyCandidate(preflight, parsed.own);
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'assetxchain-e6-'));
  const measurements = [];
  const restoreSendTx = patchSendTx(preflight, measurements);
  const originalArgv = process.argv;
  try {
    const source = e6Source(
      path.resolve(parsed.own['frozen-executor']),
      parsed.own['frozen-executor-sha256'],
    );
    const child = new Module(path.resolve(parsed.own['frozen-executor']), module);
    child.filename = path.resolve(parsed.own['frozen-executor']);
    child.paths = Module._nodeModulePaths(path.dirname(child.filename));
    child._compile(source, child.filename);
    assert(typeof child.exports.__assetxE6Main === 'function', 'patched E6 main is unavailable');
    process.argv = [originalArgv[0], path.resolve(__filename),
      ...replaceArg(parsed.forwarded, '--output-root', tempRoot)];
    await child.exports.__assetxE6Main();
    process.argv = originalArgv;
    addMeasurementEvidence(tempRoot, preflight, measurements, parsed.own);
    validateTemp(tempRoot, preflightPath, parsed.own);
    const unitManifest = rebuildUnitManifest(tempRoot);
    publishExclusive(tempRoot, finalRoot, unitManifest);
    console.log(JSON.stringify({
      status: 'PASS', family: 'E6', baseline: preflight.baseline.baseline_id,
      scenario: argValue(parsed.forwarded, '--scenario-id'),
      repetition: Number(argValue(parsed.forwarded, '--repetition')),
      measured_transactions: 3,
      setup_transactions: measurements.length - 3,
    }));
  } finally {
    process.argv = originalArgv;
    restoreSendTx();
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
}

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stack || error.message || String(error));
    process.exit(1);
  });
}

module.exports = {
  CLAIM_BOUNDARY,
  G3_FIXED_OUTER_CONTRACT_GAS_LIMIT,
  decodeExtrinsicIndex,
  e6Source,
  traceSummary,
};
