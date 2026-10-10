'use strict';

const crypto = require('crypto');
const fs = require('fs');
const Module = require('module');
const path = require('path');

const e6 = require('../formal_e6_executor.js');

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

const frozen = path.resolve(__dirname, '../formal_e0_executor.js');
const hash = crypto.createHash('sha256').update(fs.readFileSync(frozen)).digest('hex');
const source = e6.e6Source(frozen, hash);
assert(source.includes("E6: ['LEG-META-01', 'LEG-ISSUE-01', 'LEG-RESALE-01']"),
  'E6 family was not injected');
assert(source.includes("family === 'E6' ? 10"), 'E6 R=10 was not injected');
assert(source.includes('storage_writes: tx.storageWrites'), 'storage metric was not injected');
assert(source.includes('rpc_response_ms: tx.rpcResponseMs'), 'RPC metric was not injected');
assert(source.includes('partial_fee: tx.actualFee'), 'actual fee metric was not injected');
assert(source.includes('parseWeight(tx.postDispatchWeight)'), 'post-dispatch weight was not injected');

function codec(value) {
  return {
    toJSON: () => value,
    toString: () => String(value),
  };
}

const liveMetrics = require('../../scenarios/s314_live_common.js').dispatchMetrics([
  { event: {
    section: 'transactionPayment', method: 'TransactionFeePaid',
    data: [codec('payer'), codec('2921622787'), codec('0')],
  } },
  { event: {
    section: 'system', method: 'ExtrinsicSuccess',
    data: [codec({
      weight: { refTime: '2921622145', proofSize: '85932' },
      class: 'Normal', paysFee: 'Yes',
    })],
  } },
]);
assert(liveMetrics.actualFee === '2921622787', 'actual fee event decoding failed');
assert(liveMetrics.postDispatchWeight.refTime === '2921622145',
  'post-dispatch ref_time decoding failed');
assert(liveMetrics.postDispatchWeight.proofSize === '85932',
  'post-dispatch proof_size decoding failed');
assert(liveMetrics.postDispatchWeight.refTime !== e6.G3_FIXED_OUTER_CONTRACT_GAS_LIMIT.refTime,
  'actual weight must not be replaced by the fixed G3 gas limit');

const child = new Module(frozen, module);
child.filename = frozen;
child.paths = Module._nodeModulePaths(path.dirname(frozen));
child._compile(source, frozen);
assert(typeof child.exports.__assetxE6Main === 'function', 'E6 main export is unavailable');

assert(e6.decodeExtrinsicIndex('Some(02000000)') === 2, 'extrinsic index decoding failed');
const prefixes = {
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: 'Contracts',
  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb: 'DataAssets',
};
const trace = {
  blockTrace: {
    events: [
      { target: 'state', data: { stringValues: {
        key: Buffer.from(':extrinsic_index').toString('hex'), method: 'Put', value: 'Some(01000000)',
      } } },
      { target: 'state', data: { stringValues: {
        key: `${'a'.repeat(32)}01`, method: 'Put', value: 'Some(01)',
      } } },
      { target: 'state', data: { stringValues: {
        key: `${'b'.repeat(32)}02`, method: 'Delete', value: 'None',
      } } },
      { target: 'state', data: { stringValues: {
        key: Buffer.from(':extrinsic_index').toString('hex'), method: 'Put', value: 'Some(02000000)',
      } } },
      { target: 'state', data: { stringValues: {
        key: `${'a'.repeat(32)}03`, method: 'Put', value: 'Some(01)',
      } } },
    ],
  },
};
const summary = e6.traceSummary(trace, 1, 'G3', prefixes);
assert(summary.write_call_count === 2, 'trace total write count differs');
assert(summary.contract_write_call_count === 1, 'trace contract write count differs');
assert(summary.runtime_write_call_count === 1, 'trace Runtime write count differs');
assert(summary.projection_write_call_count === 1, 'trace projection write count differs');

console.log('E6_TOOLING_SELF_TEST_PASS');
