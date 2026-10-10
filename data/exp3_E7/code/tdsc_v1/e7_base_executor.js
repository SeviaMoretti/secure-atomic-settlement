'use strict';

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const NODE_DEP_ROOT = path.join(__dirname, 'node');
if (!process.env.POLKADOT_DRIVER_ROOT) process.env.POLKADOT_DRIVER_ROOT = NODE_DEP_ROOT;

const common = require('../scenarios/s314_live_common');
const b1 = require('../scenarios/s314_b1_live_smoke');
const b2 = require('../scenarios/s314_b2_live_smoke');
const g3 = require('../scenarios/s314_g3_live_smoke');

const { CodePromise, ContractPromise } = common.dep('@polkadot/api-contract');
const { blake2AsU8a } = common.dep('@polkadot/util-crypto');
const { stringToU8a, u8aToHex } = common.dep('@polkadot/util');

const E2_SCENARIOS = [
  'NEG-PROTO-01', 'NEG-SIGN-01', 'NEG-MARKET-01', 'NEG-CAPABILITY-01',
  'NEG-AUTH-01', 'NEG-OWNER-01', 'NEG-HOLDER-01', 'NEG-PARENT-01',
  'NEG-RIGHT-MISSING-01', 'NEG-RIGHT-EXPIRED-01',
  'NEG-RIGHT-EXHAUSTED-01', 'NEG-RIGHT-REVOKED-01', 'NEG-META-RIGHT-01',
  'NEG-VERSION-01', 'NEG-NONCE-01', 'NEG-PRICE-01', 'NEG-FEE-01',
  'NEG-BALANCE-01', 'NEG-PAYER-01', 'NEG-ORDER-PHASE-01',
  'NEG-AUTH-REVOKED-01', 'NEG-REFUND-EARLY-01',
  'NEG-MARKET-RESTRICTED-01', 'NEG-CAPACITY-01',
];
const FAMILY_SCENARIOS = {
  E0: ['LEG-META-01', 'LEG-ISSUE-01', 'LEG-RESALE-01'],
  E1: [
    'LEG-META-01', 'LEG-ISSUE-01', 'LEG-RESALE-01',
    'LEG-CANCEL-01', 'LEG-EXPIRE-01', 'LEG-REFUND-01',
  ],
  E2: E2_SCENARIOS,
  E3: [
    'CLEAN-LATE-META-01', 'CLEAN-LATE-RESALE-01',
    'CLEAN-ABORT-META-01', 'CLEAN-ABORT-ISSUE-01', 'CLEAN-ABORT-RESALE-01',
  ],
  E4: [
    'DUP-SETTLE-01', 'DUP-CREATE-01', 'DUP-REFUND-01',
    'RACE-BUYER-01', 'RACE-RELAYER-01', 'RACE-QUOTA-01', 'RACE-OBJECT-01',
  ],
  E5: [
    'LEG-META-01', 'LEG-ISSUE-01', 'LEG-RESALE-01',
    'RACE-RELAYER-01', 'RACE-QUOTA-01',
  ],
};
const E5_COMPETITION_SCENARIOS = new Set(['RACE-RELAYER-01', 'RACE-QUOTA-01']);
const SCENARIO_OPERATION = {
  'LEG-META-01': 'TransferMeta',
  'LEG-ISSUE-01': 'IssueRight',
  'LEG-RESALE-01': 'TransferRight',
  'LEG-CANCEL-01': 'TransferMeta',
  'LEG-EXPIRE-01': 'TransferRight',
  'LEG-REFUND-01': 'IssueRight',
  'NEG-PROTO-01': 'TransferMeta',
  'NEG-SIGN-01': 'TransferMeta',
  'NEG-MARKET-01': 'TransferMeta',
  'NEG-CAPABILITY-01': 'IssueRight',
  'NEG-AUTH-01': 'TransferRight',
  'NEG-OWNER-01': 'TransferMeta',
  'NEG-HOLDER-01': 'TransferRight',
  'NEG-PARENT-01': 'TransferRight',
  'NEG-RIGHT-MISSING-01': 'TransferRight',
  'NEG-RIGHT-EXPIRED-01': 'TransferRight',
  'NEG-RIGHT-EXHAUSTED-01': 'TransferRight',
  'NEG-RIGHT-REVOKED-01': 'TransferRight',
  'NEG-META-RIGHT-01': 'TransferMeta',
  'NEG-VERSION-01': 'TransferMeta',
  'NEG-NONCE-01': 'TransferMeta',
  'NEG-PRICE-01': 'IssueRight',
  'NEG-FEE-01': 'TransferRight',
  'NEG-BALANCE-01': 'TransferMeta',
  'NEG-PAYER-01': 'TransferRight',
  'NEG-ORDER-PHASE-01': 'TransferMeta',
  'NEG-AUTH-REVOKED-01': 'IssueRight',
  'NEG-REFUND-EARLY-01': 'IssueRight',
  'NEG-MARKET-RESTRICTED-01': 'TransferMeta',
  'NEG-CAPACITY-01': 'TransferRight',
  'CLEAN-LATE-META-01': 'TransferMeta',
  'CLEAN-LATE-RESALE-01': 'TransferRight',
  'CLEAN-ABORT-META-01': 'TransferMeta',
  'CLEAN-ABORT-ISSUE-01': 'IssueRight',
  'CLEAN-ABORT-RESALE-01': 'TransferRight',
  'DUP-SETTLE-01': 'TransferMeta',
  'DUP-CREATE-01': 'TransferMeta',
  'DUP-REFUND-01': 'IssueRight',
  'RACE-BUYER-01': 'TransferMeta',
  'RACE-RELAYER-01': 'TransferRight',
  'RACE-QUOTA-01': 'IssueRight',
  'RACE-OBJECT-01': 'TransferRight',
};
const SCENARIO_PHASES = {
  'LEG-META-01': ['create_order', 'fund_order', 'settle'],
  'LEG-ISSUE-01': ['create_order', 'fund_order', 'settle'],
  'LEG-RESALE-01': ['create_order', 'fund_order', 'settle'],
  'LEG-CANCEL-01': ['cancel_open'],
  'LEG-EXPIRE-01': ['expire_open'],
  'LEG-REFUND-01': ['refund_funded'],
  'NEG-PROTO-01': ['create_order'],
  'NEG-SIGN-01': ['create_order'],
  'NEG-MARKET-01': ['create_order'],
  'NEG-CAPABILITY-01': ['create_order'],
  'NEG-AUTH-01': ['create_order'],
  'NEG-OWNER-01': ['create_order'],
  'NEG-HOLDER-01': ['create_order'],
  'NEG-PARENT-01': ['create_order'],
  'NEG-RIGHT-MISSING-01': ['create_order'],
  'NEG-RIGHT-EXPIRED-01': ['create_order'],
  'NEG-RIGHT-EXHAUSTED-01': ['create_order'],
  'NEG-RIGHT-REVOKED-01': ['create_order'],
  'NEG-META-RIGHT-01': ['create_order'],
  'NEG-VERSION-01': ['create_order'],
  'NEG-NONCE-01': ['create_order'],
  'NEG-PRICE-01': ['create_order'],
  'NEG-FEE-01': ['create_order'],
  'NEG-BALANCE-01': ['fund_order'],
  'NEG-PAYER-01': ['fund_order'],
  'NEG-ORDER-PHASE-01': ['settle'],
  'NEG-AUTH-REVOKED-01': ['fund_order'],
  'NEG-REFUND-EARLY-01': ['refund_funded'],
  'NEG-MARKET-RESTRICTED-01': ['create_order'],
  'NEG-CAPACITY-01': ['fund_order'],
  'CLEAN-LATE-META-01': ['settle'],
  'CLEAN-LATE-RESALE-01': ['settle'],
  'CLEAN-ABORT-META-01': ['settle'],
  'CLEAN-ABORT-ISSUE-01': ['settle'],
  'CLEAN-ABORT-RESALE-01': ['settle'],
  'DUP-SETTLE-01': ['settle', 'settle'],
  'DUP-CREATE-01': ['create_order', 'create_order'],
  'DUP-REFUND-01': ['refund_funded', 'refund_funded'],
  'RACE-BUYER-01': ['fund_order', 'fund_order'],
  'RACE-RELAYER-01': ['settle', 'settle', 'settle', 'settle'],
  'RACE-QUOTA-01': ['fund_order', 'fund_order', 'settle'],
  'RACE-OBJECT-01': ['create_order', 'create_order'],
};
const OPERATION_PRICE = common.PRICES;
const E0_FIXTURE_ANCHOR_BLOCK = 16;
const ZERO_HASH = `0x${'00'.repeat(32)}`;
const ATTEMPT_BLOCK_EVIDENCE = Symbol('attempt-block-evidence');
const BASELINE_NAMES = { B1: 'Contract-only', B2: 'Runtime-only', G3: 'AssetXChain-CRS' };
const COMMON_ERROR_ORDER = [
  'UnsupportedProtocolVersion', 'MalformedCommitment', 'DomainMismatch',
  'MarketNotRegistered', 'MarketRestricted', 'CapabilityDenied',
  'CapacityExceeded', 'AuthorizationMissing', 'AuthorizationExpired',
  'AuthorizationRevoked', 'QuotaExceeded', 'InvalidSignature', 'NonceUsed',
  'StateVersionMismatch', 'OrderNotFound', 'OrderNotOpen', 'OrderNotFunded',
  'OrderExpired', 'AlreadySettled', 'TerminalOrder', 'AssetNotTradable',
  'RightInvalid', 'OwnershipMismatch', 'ParentMismatch',
  'MutualExclusionConflict', 'BelowPriceFloor', 'FeeOutOfBounds',
  'InsufficientBalance', 'EscrowMismatch', 'PayerBuyerMismatch',
  'RefundNotAllowed', 'GracePeriodActive', 'RefundBeneficiaryMismatch',
];
const G3_COMMON_STATUS = Object.fromEntries(COMMON_ERROR_ORDER.map((name, index) => [name, index + 1]));
const NATIVE_COMMON = {
  B1: {
    UnsupportedProtocolVersion: 'UnsupportedProtocolVersion',
    MalformedCommitment: 'MalformedCommitment',
    InvalidSignature: 'InvalidSignature', MarketNotRegistered: 'MarketNotRegistered',
    CapabilityDenied: 'CapabilityDenied', AuthorizationMissing: 'AuthorizationMissing',
    OwnershipMismatch: 'OwnershipMismatch', ParentMismatch: 'ParentMismatch',
    RightNotFound: 'RightInvalid', RightInvalid: 'RightInvalid',
    MutualExclusionConflict: 'MutualExclusionConflict',
    ActiveOrderConflict: 'MutualExclusionConflict',
    StateVersionMismatch: 'StateVersionMismatch', NonceUsed: 'NonceUsed',
    BelowPriceFloor: 'BelowPriceFloor', FeeOutOfBounds: 'FeeOutOfBounds',
    InsufficientBalance: 'InsufficientBalance', PayerBuyerMismatch: 'PayerBuyerMismatch',
    OrderNotFunded: 'OrderNotFunded', AuthorizationRevoked: 'AuthorizationRevoked',
    GracePeriodActive: 'GracePeriodActive', MarketRestricted: 'MarketRestricted',
    CapacityExceeded: 'CapacityExceeded', AssetNotTradable: 'AssetNotTradable',
    AlreadySettled: 'AlreadySettled', TerminalOrder: 'TerminalOrder',
    OrderNotOpen: 'OrderNotOpen', QuotaExceeded: 'QuotaExceeded',
  },
  B2: {
    UnsupportedProtocolVersion: 'UnsupportedProtocolVersion',
    MalformedCommitment: 'MalformedCommitment',
    InvalidSignature: 'InvalidSignature', MarketNotRegistered: 'MarketNotRegistered',
    CapabilityDenied: 'CapabilityDenied', AuthorizationMissing: 'AuthorizationMissing',
    OwnershipMismatch: 'OwnershipMismatch', ParentMismatch: 'ParentMismatch',
    RightNotFound: 'RightInvalid', RightInvalid: 'RightInvalid',
    MutualExclusionConflict: 'MutualExclusionConflict',
    StateVersionMismatch: 'StateVersionMismatch', NonceUsed: 'NonceUsed',
    BelowPriceFloor: 'BelowPriceFloor', FeeOutOfBounds: 'FeeOutOfBounds',
    InsufficientBalance: 'InsufficientBalance', PayerBuyerMismatch: 'PayerBuyerMismatch',
    OrderNotFunded: 'OrderNotFunded', AuthorizationRevoked: 'AuthorizationRevoked',
    GracePeriodActive: 'GracePeriodActive', MarketRestricted: 'MarketRestricted',
    CapacityExceeded: 'CapacityExceeded', AssetNotTradable: 'AssetNotTradable',
    AlreadySettled: 'AlreadySettled', TerminalOrder: 'TerminalOrder',
    OrderNotOpen: 'OrderNotOpen', QuotaExceeded: 'QuotaExceeded',
    ActiveOrderConflict: 'MutualExclusionConflict',
  },
};
const E2_NATIVE_ERROR = {
  B1: {
    'NEG-PROTO-01': 'UnsupportedProtocolVersion', 'NEG-SIGN-01': 'InvalidSignature',
    'NEG-MARKET-01': 'MarketNotRegistered', 'NEG-CAPABILITY-01': 'CapabilityDenied',
    'NEG-AUTH-01': 'AuthorizationMissing', 'NEG-OWNER-01': 'OwnershipMismatch',
    'NEG-HOLDER-01': 'OwnershipMismatch', 'NEG-PARENT-01': 'ParentMismatch',
    'NEG-RIGHT-MISSING-01': 'RightNotFound', 'NEG-RIGHT-EXPIRED-01': 'RightInvalid',
    'NEG-RIGHT-EXHAUSTED-01': 'RightInvalid', 'NEG-RIGHT-REVOKED-01': 'RightInvalid',
    'NEG-META-RIGHT-01': 'MutualExclusionConflict',
    'NEG-VERSION-01': 'StateVersionMismatch', 'NEG-NONCE-01': 'NonceUsed',
    'NEG-PRICE-01': 'BelowPriceFloor', 'NEG-FEE-01': 'FeeOutOfBounds',
    'NEG-BALANCE-01': 'InsufficientBalance', 'NEG-PAYER-01': 'PayerBuyerMismatch',
    'NEG-ORDER-PHASE-01': 'OrderNotFunded',
    'NEG-AUTH-REVOKED-01': 'AuthorizationRevoked',
    'NEG-REFUND-EARLY-01': 'GracePeriodActive',
    'NEG-MARKET-RESTRICTED-01': 'MarketRestricted',
    'NEG-CAPACITY-01': 'CapacityExceeded',
  },
  B2: {},
};
E2_NATIVE_ERROR.B2 = { ...E2_NATIVE_ERROR.B1 };
const E3_NATIVE_ERROR = {
  B1: {
    'CLEAN-LATE-META-01': 'AssetNotTradable',
    'CLEAN-LATE-RESALE-01': 'RightInvalid',
  },
  B2: {
    'CLEAN-LATE-META-01': 'AssetNotTradable',
    'CLEAN-LATE-RESALE-01': 'RightInvalid',
  },
};
const E4_NATIVE_ERROR = {
  B1: {
    'DUP-SETTLE-01': 'AlreadySettled',
    'DUP-CREATE-01': 'NonceUsed',
    'DUP-REFUND-01': 'TerminalOrder',
    'RACE-BUYER-01': 'OrderNotOpen',
    'RACE-RELAYER-01': 'AlreadySettled',
    'RACE-QUOTA-01': 'QuotaExceeded',
    'RACE-OBJECT-01': 'ActiveOrderConflict',
  },
  B2: {
    'DUP-SETTLE-01': 'AlreadySettled',
    'DUP-CREATE-01': 'NonceUsed',
    'DUP-REFUND-01': 'TerminalOrder',
    'RACE-BUYER-01': 'OrderNotOpen',
    'RACE-RELAYER-01': 'AlreadySettled',
    'RACE-QUOTA-01': 'QuotaExceeded',
    'RACE-OBJECT-01': 'ActiveOrderConflict',
  },
};
const TEST_GATE_FILTER = {
  B1: {
    'CLEAN-ABORT-META-01': 'clean_abort_meta_01',
    'CLEAN-ABORT-ISSUE-01': 'clean_abort_issue_01',
    'CLEAN-ABORT-RESALE-01': 'clean_abort_resale_01',
  },
  B2: {
    'CLEAN-ABORT-META-01': 's38_clean_abort_meta_01',
    'CLEAN-ABORT-ISSUE-01': 's38_clean_abort_issue_01',
    'CLEAN-ABORT-RESALE-01': 's38_clean_abort_resale_01',
  },
  G3: {
    'CLEAN-ABORT-META-01': 'clean_abort_meta_01',
    'CLEAN-ABORT-ISSUE-01': 'clean_abort_issue_01',
    'CLEAN-ABORT-RESALE-01': 'clean_abort_resale_01',
  },
};
const COMMON_METRIC_UNITS = {
  latency_ms: 'ms', completion_blocks: 'blocks', partial_fee: 'plancks',
  weight_ref_time: 'ref_time', weight_proof_size: 'bytes', storage_writes: 'count',
  events_count: 'count', rpc_response_ms: 'ms', phase_call_count: 'count',
};
const ARCH_METRIC_UNITS = {
  contract_gas_or_ref_time: 'ref_time', contract_storage_writes: 'count',
  runtime_dispatch_count: 'count', runtime_storage_writes: 'count',
  typed_ce_call_count: 'count', projection_storage_writes: 'count',
};
const ARCH_APPLICABLE = {
  B1: new Set(['contract_gas_or_ref_time', 'contract_storage_writes']),
  B2: new Set(['runtime_dispatch_count', 'runtime_storage_writes']),
  G3: new Set(Object.keys(ARCH_METRIC_UNITS)),
};
const RECORD_FIELDS = [
  'settlement_id', 'order_id', 'operation', 'asset_id', 'right_id',
  'authorization_id', 'market_id', 'policy_hash', 'order_commitment_hash',
  'seller', 'buyer', 'currency', 'gross_price', 'seller_amount',
  'royalty_recipient', 'royalty_amount', 'market_fee_recipient',
  'market_fee_amount', 'safety_fee_amount', 'pre_state_version',
  'post_state_version', 'pre_state_commitment', 'block_number',
  'settlement_protocol_version',
];
const REQUIRED_DEPENDENCIES = [
  __filename,
  path.join(__dirname, '../scenarios/s314_live_common.js'),
  path.join(__dirname, '../scenarios/s314_b1_live_smoke.js'),
  path.join(__dirname, '../scenarios/s314_b2_live_smoke.js'),
  path.join(__dirname, '../scenarios/s314_g3_live_smoke.js'),
  path.join(NODE_DEP_ROOT, 'package.json'),
  path.join(NODE_DEP_ROOT, 'package-lock.json'),
  path.join(__dirname, '../../configs/phase4/formal-toolchain-v1.json'),
].map((item) => path.resolve(item));
const ARGUMENTS = new Set([
  'preflight', 'scenario_id', 'repetition', 'execution_mode', 'output_root', 'rpc', 'dependency',
]);

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function field(value, camel, snake = camel) {
  if (value && Object.prototype.hasOwnProperty.call(value, camel)) return value[camel];
  if (value && Object.prototype.hasOwnProperty.call(value, snake)) return value[snake];
  return undefined;
}

function parseArgs(argv) {
  const values = { dependency: [] };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    assert(token.startsWith('--'), `unexpected positional argument ${token}`);
    const key = token.slice(2).replace(/-/g, '_');
    assert(ARGUMENTS.has(key), `unknown argument ${token}`);
    const next = argv[index + 1];
    assert(next !== undefined && !next.startsWith('--'), `missing value for ${token}`);
    if (key === 'dependency') values.dependency.push(next);
    else {
      assert(values[key] === undefined, `duplicate argument ${token}`);
      values[key] = next;
    }
    index += 1;
  }
  for (const key of ['preflight', 'scenario_id', 'repetition', 'execution_mode', 'output_root', 'rpc']) {
    assert(values[key] !== undefined, `--${key.replace(/_/g, '-')} is required`);
  }
  values.repetition = Number(values.repetition);
  assert(Number.isInteger(values.repetition) && values.repetition > 0, 'repetition must be a positive integer');
  assert(new Set(values.dependency).size === values.dependency.length, 'duplicate --dependency is forbidden');
  return values;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  }
  return typeof value === 'bigint' ? value.toString() : value;
}

function jsonBytes(value) {
  return Buffer.from(`${JSON.stringify(stable(common.safeJson(value)))}\n`, 'utf8');
}

function writeJsonExclusive(filePath, value) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, jsonBytes(value), { flag: 'wx' });
}

function writeBytesExclusive(filePath, value) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, value, { flag: 'wx' });
}

function blake(value) {
  const bytes = Buffer.isBuffer(value) || value instanceof Uint8Array ? value : jsonBytes(value);
  return u8aToHex(blake2AsU8a(bytes, 256));
}

function sha256File(filePath) {
  const crypto = require('crypto');
  return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

function sha256Bytes(value) {
  const crypto = require('crypto');
  return crypto.createHash('sha256').update(value).digest('hex');
}

function optionJson(codec) {
  if (!codec || codec.isNone) return null;
  return codec.unwrap().toJSON();
}

function enumName(value) {
  if (value === null || value === undefined) return null;
  if (typeof value === 'string') return value.charAt(0).toUpperCase() + value.slice(1);
  if (typeof value === 'object') {
    const keys = Object.keys(value);
    if (keys.length === 1) return keys[0].charAt(0).toUpperCase() + keys[0].slice(1);
  }
  return String(value);
}

function hex32(value) {
  if (value === null || value === undefined) return null;
  const hex = typeof value === 'string' ? value : (value.toHex ? value.toHex() : String(value));
  assert(/^0x[0-9a-fA-F]{64}$/.test(hex), `expected hex32, got ${hex}`);
  return hex.toLowerCase();
}

function account(value) {
  if (value === null || value === undefined) return null;
  if (typeof value === 'string' && /^0x[0-9a-fA-F]{64}$/.test(value)) return value.toLowerCase();
  return common.accountHex(value).toLowerCase();
}

function decimal(value) {
  if (value === null || value === undefined) return null;
  return String(value);
}

function metric(availability, value, unit) {
  return { availability, value: value === null ? null : String(value), unit };
}

function parseWeight(weight) {
  if (!weight) return { refTime: null, proofSize: null };
  const value = typeof weight.toJSON === 'function' ? weight.toJSON() : weight;
  if (typeof value === 'object') {
    return {
      refTime: field(value, 'refTime', 'ref_time') === undefined ? null : String(field(value, 'refTime', 'ref_time')),
      proofSize: field(value, 'proofSize', 'proof_size') === undefined ? null : String(field(value, 'proofSize', 'proof_size')),
    };
  }
  return { refTime: null, proofSize: null };
}

function costPackage(tx, eventNames) {
  const weight = parseWeight(tx.paymentWeight);
  const measured = {
    latency_ms: tx.latencyMs === undefined ? null : tx.latencyMs,
    completion_blocks: 0,
    partial_fee: tx.partialFee === undefined ? null : tx.partialFee,
    weight_ref_time: weight.refTime,
    weight_proof_size: weight.proofSize,
    events_count: eventNames.length,
    phase_call_count: 1,
  };
  return Object.fromEntries(Object.entries(COMMON_METRIC_UNITS).map(([name, unit]) => [
    name,
    measured[name] === undefined || measured[name] === null
      ? metric('not-collected', null, unit)
      : metric('measured', measured[name], unit),
  ]));
}

function architecturePackage(baseline, tx, contractGas = null, typedCeCall = baseline === 'G3') {
  const measured = {};
  if (baseline === 'B1' && contractGas !== null) measured.contract_gas_or_ref_time = contractGas;
  if (baseline === 'B2') measured.runtime_dispatch_count = 1;
  if (baseline === 'G3') {
    measured.runtime_dispatch_count = 1;
    measured.typed_ce_call_count = typedCeCall ? 1 : 0;
    if (contractGas !== null) measured.contract_gas_or_ref_time = contractGas;
  }
  return Object.fromEntries(Object.entries(ARCH_METRIC_UNITS).map(([name, unit]) => {
    if (!ARCH_APPLICABLE[baseline].has(name)) return [name, metric('N/A', null, unit)];
    return [name, measured[name] === undefined
      ? metric('not-collected', null, unit)
      : metric('measured', measured[name], unit)];
  }));
}

function phaseIndex(tx) {
  for (const record of tx.events || []) {
    if (record.phase && record.phase.isApplyExtrinsic) return record.phase.asApplyExtrinsic.toNumber();
  }
  return null;
}

function itemBlock(tx) {
  return Number(tx.blockNumber) * 100000 + Number(phaseIndex(tx) ?? 0);
}

function blockEventIndex(tx, transactionEventIndex) {
  const index = tx.eventIndexes && tx.eventIndexes[transactionEventIndex];
  assert(Number.isInteger(index), `missing complete-block index for transaction event ${transactionEventIndex}`);
  return index;
}

function runtimeEventInfo(tx, settleSection) {
  const names = [];
  let eventIndex = null;
  let postStateCommitment = null;
  (tx.events || []).forEach(({ event }, index) => {
    if (!event || ['system', 'balances', 'transactionPayment'].includes(event.section)) return;
    names.push(event.method);
    if (event.section === settleSection && eventIndex === null) eventIndex = blockEventIndex(tx, index);
    if (event.section === settleSection && event.method === 'SettlementFinalized') {
      eventIndex = blockEventIndex(tx, index);
      postStateCommitment = hex32(event.data[2]);
    }
  });
  return { names, eventIndex, postStateCommitment };
}

function contractEventInfo(tx, contract) {
  const names = [];
  let eventIndex = null;
  let postStateCommitment = null;
  (tx.events || []).forEach((record, index) => {
    const { event } = record;
    if (!event || event.section !== 'contracts' || event.method !== 'ContractEmitted') return;
    try {
      const decoded = contract.abi.decodeEvent(record);
      const name = decoded.event.identifier.split('::').pop();
      names.push(name);
      if (eventIndex === null) eventIndex = blockEventIndex(tx, index);
      if (name === 'SettlementFinalized') {
        eventIndex = blockEventIndex(tx, index);
        const argIndex = decoded.event.args.findIndex((item) => item.name === 'post_state_commitment' || item.name === 'postStateCommitment');
        assert(argIndex >= 0, 'SettlementFinalized omitted post_state_commitment');
        postStateCommitment = hex32(decoded.args[argIndex]);
      }
    } catch (error) {
      throw new Error(`cannot decode B1 contract event: ${error.message}`);
    }
  });
  return { names, eventIndex, postStateCommitment };
}

async function freeBalance(api, pair, blockHash = null) {
  const accountInfo = blockHash
    ? await api.query.system.account.at(blockHash, pair.address)
    : await api.query.system.account(pair.address);
  const balance = accountInfo.data.free.toString();
  assert(/^[0-9]+$/.test(balance), `system.account free balance is not decimal: ${balance}`);
  return balance;
}

async function deterministicFixtureAnchor(
  api,
  target = E0_FIXTURE_ANCHOR_BLOCK,
  pollMs = 250,
  timeoutMs = 180_000
) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() <= deadline) {
    const observed = (await api.rpc.chain.getHeader()).number.toNumber();
    assert(observed <= target, `fixture setup exceeded deterministic anchor ${target}: observed ${observed}`);
    if (observed === target) return observed;
    await new Promise((resolve) => setTimeout(resolve, pollMs));
  }
  throw new Error(`timed out waiting for deterministic fixture anchor ${target}`);
}

async function waitUntilAfterBlock(api, target, pollMs = 250, timeoutMs = 180_000) {
  assert(Number.isInteger(Number(target)) && Number(target) >= 0, `invalid target block ${target}`);
  const deadline = Date.now() + timeoutMs;
  while (Date.now() <= deadline) {
    const observed = (await api.rpc.chain.getHeader()).number.toNumber();
    if (observed > Number(target)) return observed;
    await new Promise((resolve) => setTimeout(resolve, pollMs));
  }
  throw new Error(`timed out waiting until after block ${target}`);
}

async function paymentBalances(api, accounts, sellerRole, buyerRole, blockHash = null) {
  return {
    buyer_balance: await freeBalance(api, accounts[buyerRole], blockHash),
    seller_balance: await freeBalance(api, accounts[sellerRole], blockHash),
    royalty_balance: null,
    market_balance: await freeBalance(api, accounts.MARKET_OK, blockHash),
    safety_balance: null,
  };
}

async function resolveSnapshotBoundary(api, blockHash = null) {
  const resolvedHash = blockHash || (await api.rpc.chain.getHeader()).hash.toHex();
  const header = await api.rpc.chain.getHeader(resolvedHash);
  return {
    blockHash: resolvedHash,
    blockNumber: header.number.toNumber(),
    queryApi: await api.at(resolvedHash),
  };
}

function capabilities(value) {
  const number = Number(value || 0);
  return [
    [1, 'TransferMeta'], [2, 'IssueRight'], [4, 'TransferRight'],
  ].filter(([bit]) => (number & bit) !== 0).map(([, name]) => name);
}

function mappedNativeError(baseline, nativeError) {
  const table = NATIVE_COMMON[baseline];
  assert(table, `native error mapping is unavailable for ${baseline}`);
  const commonError = table[nativeError];
  assert(commonError, `${baseline} returned unmapped native error ${nativeError}`);
  return commonError;
}

function b1Rejection(call) {
  const nativeError = b1.nativeError(call.dry.decoded.error);
  return { nativeError, commonError: mappedNativeError('B1', nativeError) };
}

function b2Rejection(tx) {
  assert(tx.dispatchError && tx.dispatchError.name, 'B2 rejected dispatch omitted native error');
  const nativeError = tx.dispatchError.name;
  return { nativeError, commonError: mappedNativeError('B2', nativeError) };
}

function g3Rejection(relay) {
  assert(relay.decoded && relay.decoded.error === 'ChainExtension', 'G3 rejection did not come from CE v2');
  const commonError = COMMON_ERROR_ORDER[relay.decoded.code - 1];
  assert(commonError, `G3 returned unknown CE v2 status ${relay.decoded.code}`);
  assert(relay.decoded.name === commonError, `G3 CE name/status drift: ${JSON.stringify(relay.decoded)}`);
  return {
    nativeError: `ChainExtension(${relay.decoded.code}):${commonError}`,
    commonError,
  };
}

function rejectionEventInfo(tx) {
  let eventIndex = null;
  (tx.events || []).forEach(({ event }, index) => {
    if (event && event.section === 'system' && event.method === 'ExtrinsicFailed') {
      eventIndex = blockEventIndex(tx, index);
    }
  });
  return { names: [], eventIndex, postStateCommitment: null };
}

function balanceFieldForRole(context, role) {
  if (role === context.snapshotSellerRole) return 'seller_balance';
  if (role === context.snapshotBuyerRole) return 'buyer_balance';
  if (role === 'MARKET_OK') return 'market_balance';
  return null;
}

function balanceAdjustments(context, tx) {
  const roles = Object.entries(context.accountsByRole || {});
  return (tx.environmentalBalanceCredits || []).map((credit) => {
    const normalizedAccount = account(credit.account);
    const matched = roles.find(([, pair]) => account(pair.address) === normalizedAccount);
    assert(matched, `environmental balance credit has unknown account ${normalizedAccount}`);
    return {
      role: matched[0],
      account: normalizedAccount,
      reason: credit.reason,
      amount: decimal(credit.amount),
      block_number: Number(credit.blockNumber),
      event_index: Number(credit.eventIndex),
      event_phase: credit.phase,
    };
  });
}

function normalizedRejectedSnapshot(context, snapshot, actorRole, fee, adjustments, post) {
  const value = common.safeJson(snapshot);
  if (post) {
    for (const adjustment of adjustments) {
      const balanceField = balanceFieldForRole(context, adjustment.role);
      if (!balanceField) continue;
      assert(value.payment[balanceField] !== null, `cannot normalize ${adjustment.role} reward`);
      value.payment[balanceField] = (
        BigInt(value.payment[balanceField]) - BigInt(adjustment.amount)
      ).toString();
    }
    if (BigInt(fee || 0) !== 0n) {
      let balanceField = null;
      if (actorRole === context.snapshotSellerRole) balanceField = 'seller_balance';
      else if (actorRole === context.snapshotBuyerRole) balanceField = 'buyer_balance';
      else if (actorRole === 'MARKET_OK') balanceField = 'market_balance';
      if (balanceField) {
        assert(value.payment[balanceField] !== null, `cannot normalize fee payer ${actorRole}`);
        value.payment[balanceField] = (BigInt(value.payment[balanceField]) + BigInt(fee)).toString();
      }
    }
  }
  return value;
}

function normalizedRecord(value) {
  if (!value) return null;
  const output = {};
  for (const name of RECORD_FIELDS) {
    const camel = name.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
    let current = field(value, camel, name);
    if (['settlement_id', 'order_id', 'asset_id', 'right_id', 'authorization_id', 'market_id', 'policy_hash', 'order_commitment_hash', 'pre_state_commitment'].includes(name)) {
      current = hex32(current);
    } else if (['seller', 'buyer', 'royalty_recipient', 'market_fee_recipient'].includes(name)) {
      current = account(current);
    } else if (['gross_price', 'seller_amount', 'royalty_amount', 'market_fee_amount', 'safety_fee_amount'].includes(name)) {
      current = decimal(current);
    } else if (['pre_state_version', 'post_state_version', 'block_number', 'settlement_protocol_version'].includes(name)) {
      current = Number(current);
    } else if (name === 'currency') {
      current = enumName(current);
    } else if (name === 'operation') {
      current = enumName(current);
    }
    output[name] = current;
  }
  assert(Object.keys(output).length === 24, 'normalized record must have 24 fields');
  return output;
}

function businessCommitment(snapshot) {
  const value = common.safeJson(snapshot);
  delete value.observed_at;
  delete value.state_commitment;
  return blake(value);
}

function differingPaths(left, right, prefix = '') {
  if (JSON.stringify(left) === JSON.stringify(right)) return [];
  if (left === null || right === null || typeof left !== 'object' || typeof right !== 'object') {
    return [{ path: prefix || '$', pre: left, post: right }];
  }
  const keys = [...new Set([...Object.keys(left), ...Object.keys(right)])].sort();
  return keys.flatMap((key) => differingPaths(
    left[key], right[key], prefix ? `${prefix}.${key}` : key
  ));
}

function finishSnapshot(snapshot) {
  snapshot.state_commitment = businessCommitment(snapshot);
  return snapshot;
}

function emptyCorrectness() {
  return {
    expected_outcome_match: true,
    invariant_violation_count: 0,
    false_accept_count: 0,
    false_reject_count: 0,
    duplicate_settlement_count: 0,
    dirty_order_count: 0,
    dirty_authorization_count: 0,
    event_mismatch_count: 0,
    timeout_count: 0,
    critical_state_diff_count: 0,
    unexpected_balance_delta_count: 0,
    unexpected_index_delta_count: 0,
  };
}

async function captureSubmission(context, submit) {
  const sequence = context.nextSubmissionSequence;
  assert(Number.isInteger(sequence) && sequence > 0, 'submission sequence is unavailable');
  context.nextSubmissionSequence += 1;
  context.activeSubmissionSequence = sequence;
  const value = await submit();
  const tx = value && value.submitted ? value.submitted : value;
  assert(tx && typeof tx === 'object', 'attempt submission returned no transaction evidence');
  assert(tx.submissionSequence === undefined, 'transaction submission sequence was assigned twice');
  tx.submissionSequence = sequence;
  return value;
}

function capturedSubmissionSequence(tx) {
  assert(Number.isInteger(tx.submissionSequence) && tx.submissionSequence > 0,
    'attempt transaction lacks a pre-dispatch submission sequence');
  return tx.submissionSequence;
}

function observedAttemptSequence(context) {
  return context.activeSubmissionSequence || context.nextSubmissionSequence || 1;
}

function attachBlockEvidence(result, tx) {
  const extrinsicIndex = phaseIndex(tx);
  assert(tx.blockHash && tx.parentBlockHash, 'transaction block boundary is incomplete');
  assert(Number.isInteger(extrinsicIndex), 'transaction extrinsic index is unavailable');
  assert(Array.isArray(tx.blockEventRows) && tx.blockEventRows.length > 0,
    'complete block events are unavailable');
  const events = tx.blockEventRows.map((row) => ({
    event_index: Number(row.eventIndex),
    section: row.section,
    method: row.method,
    phase: row.phase,
    data: row.data,
  }));
  Object.defineProperty(result, ATTEMPT_BLOCK_EVIDENCE, {
    enumerable: false,
    value: {
      schema_id: 'assetxchain-exp2-phase4-attempt-block-evidence',
      schema_version: 1,
      transaction_hash: tx.txHash,
      parent_block_hash: tx.parentBlockHash,
      included_block_hash: tx.blockHash,
      parent_block_number: tx.blockNumber - 1,
      included_block_number: tx.blockNumber,
      terminal_status: tx.terminalStatus,
      extrinsic_index: extrinsicIndex,
      transaction_event_indexes: tx.eventIndexes,
      complete_block_event_count: events.length,
      events,
    },
  });
  return result;
}

function makeResult(
  context, expected, tx, pre, post, eventInfo, actorRole, contractGas = null,
  typedCeCall = context.baseline === 'G3'
) {
  const settle = expected.phase === 'settle';
  const refund = expected.phase === 'refund_funded';
  const record = context.lastRecord;
  const settlementId = record ? record.settlement_id : null;
  const outcome = {
    dispatch_success: true,
    protocol_outcome: 'accepted',
    common_error_code: null,
    native_error: null,
    included_block: tx.blockNumber,
    completion_block: tx.blockNumber,
    completion_blocks: 0,
    submission_sequence: capturedSubmissionSequence(tx),
    extrinsic_index: phaseIndex(tx),
    event_index: eventInfo.eventIndex,
    dispatch_fee_payer_role: actorRole,
    dispatch_fee_amount: decimal(tx.dispatchFee ?? tx.partialFee ?? 0),
    environmental_balance_adjustments: balanceAdjustments(context, tx),
  };
  return attachBlockEvidence({
    schema_id: 'assetxchain-exp2-phase4-formal-result',
    schema_version: 1,
    contract_version: 'phase3-adapter-contract-v1',
    run: {
      run_id: context.preflight.run_id,
      timestamp: context.preflight.created_at,
      timezone: 'Asia/Shanghai',
      baseline_id: context.baseline,
      baseline_name: BASELINE_NAMES[context.baseline],
      baseline_profile_version: '1',
      settlement_protocol_version: 1,
      validity_level: 'provisional-candidate',
      implementation_commit: context.preflight.baseline.head,
      git_dirty: false,
      config_path: context.preflight.inputs.workload.path,
      workload_id: context.preflight.inputs.workload_id,
      seed: context.preflight.inputs.seed,
      repetition: context.repetition,
    },
    request: {
      scenario_id: context.scenario.scenario_id,
      attempt_id: expected.attempt_id,
      phase: expected.phase,
      operation: context.operation,
      expected_outcome: expected.expected_outcome,
      expected_error_code: expected.expected_error_code,
    },
    outcome,
    snapshots: { pre, post },
    events: {
      event_names: eventInfo.names,
      success_event_count: settle ? 1 : 0,
      post_state_commitment: settle ? eventInfo.postStateCommitment : null,
    },
    settlement: {
      settlement_id: settle ? settlementId : null,
      settlement_record_count: post.evidence.settlement_record_count,
      refund_recipient: refund ? post.order.buyer : null,
      refund_amount: refund ? pre.payment.escrow_amount : null,
      record: settle ? record : null,
    },
    correctness: emptyCorrectness(),
    cost: costPackage(tx, eventInfo.names),
    architecture_cost: architecturePackage(context.baseline, tx, contractGas, typedCeCall),
    claim_boundary: context.preflight.experiment_family === 'E0'
      ? 'E0-reproducibility-only; not-performance-evidence'
      : 'formal-correctness-only; not-performance-evidence',
  }, tx);
}

function makeRejectedResult(
  context, expected, tx, pre, post, eventInfo, actorRole, rejection,
  contractGas = null, typedCeCall = context.baseline === 'G3',
  expectedCommonError = expected.expected_error_code,
  allowBoundaryChange = false
) {
  assert(rejection.commonError === expectedCommonError,
    `${context.baseline} expected ${expectedCommonError}, got ${rejection.commonError}`);
  const fee = decimal(tx.dispatchFee ?? tx.partialFee ?? 0);
  const adjustments = balanceAdjustments(context, tx);
  const normalizedPre = normalizedRejectedSnapshot(context, pre, actorRole, fee, adjustments, false);
  const normalizedPost = normalizedRejectedSnapshot(context, post, actorRole, fee, adjustments, true);
  const preCommitment = businessCommitment(normalizedPre);
  const postCommitment = businessCommitment(normalizedPost);
  if (!allowBoundaryChange) assert(
    preCommitment === postCommitment,
    `rejected attempt changed normalized Q_ALL: ${JSON.stringify(
      differingPaths(normalizedPre, normalizedPost).slice(0, 20)
    )}; environmental observation=${JSON.stringify({
      included_block: tx.blockNumber,
      state_boundary: tx.stateBoundary || null,
      credits: tx.environmentalBalanceCredits || [],
      block_events: tx.blockEventRows || [],
    })}`
  );
  pre.state_commitment = preCommitment;
  post.state_commitment = postCommitment;
  return attachBlockEvidence({
    schema_id: 'assetxchain-exp2-phase4-formal-result',
    schema_version: 1,
    contract_version: 'phase3-adapter-contract-v1',
    run: {
      run_id: context.preflight.run_id,
      timestamp: context.preflight.created_at,
      timezone: 'Asia/Shanghai',
      baseline_id: context.baseline,
      baseline_name: BASELINE_NAMES[context.baseline],
      baseline_profile_version: '1',
      settlement_protocol_version: 1,
      validity_level: 'provisional-candidate',
      implementation_commit: context.preflight.baseline.head,
      git_dirty: false,
      config_path: context.preflight.inputs.workload.path,
      workload_id: context.preflight.inputs.workload_id,
      seed: context.preflight.inputs.seed,
      repetition: context.repetition,
    },
    request: {
      scenario_id: context.scenario.scenario_id,
      attempt_id: expected.attempt_id,
      phase: expected.phase,
      operation: context.operation,
      expected_outcome: expected.expected_outcome,
      expected_error_code: expected.expected_error_code,
    },
    outcome: {
      dispatch_success: false,
      protocol_outcome: 'rejected',
      common_error_code: rejection.commonError,
      native_error: rejection.nativeError,
      included_block: tx.blockNumber,
      completion_block: tx.blockNumber,
      completion_blocks: 0,
      submission_sequence: capturedSubmissionSequence(tx),
      extrinsic_index: phaseIndex(tx),
      event_index: eventInfo.eventIndex,
      dispatch_fee_payer_role: actorRole,
      dispatch_fee_amount: fee,
      environmental_balance_adjustments: adjustments,
    },
    snapshots: { pre, post },
    events: { event_names: [], success_event_count: 0, post_state_commitment: null },
    settlement: {
      settlement_id: null,
      settlement_record_count: post.evidence.settlement_record_count,
      refund_recipient: null,
      refund_amount: null,
      record: null,
    },
    correctness: emptyCorrectness(),
    cost: costPackage(tx, eventInfo.names),
    architecture_cost: architecturePackage(context.baseline, tx, contractGas, typedCeCall),
    claim_boundary: 'formal-correctness-only; not-performance-evidence',
  }, tx);
}

function writeAttempt(context, expected, actorRole, result) {
  const attemptDir = path.join(context.outputRoot, `attempt-${String(context.artifactSequence).padStart(3, '0')}`);
  const invocation = {
    run_id: context.preflight.run_id,
    baseline_id: context.baseline,
    scenario_id: context.scenario.scenario_id,
    attempt_id: expected.attempt_id,
    repetition: context.repetition,
    phase: expected.phase,
    operation: context.operation,
    actor_role: actorRole,
    expected_outcome: expected.expected_outcome,
    expected_error_code: expected.expected_error_code,
    execution_mode: 'live-chain',
  };
  writeJsonExclusive(path.join(attemptDir, 'invocation.json'), invocation);
  writeJsonExclusive(path.join(attemptDir, 'dispatch.json'), result.outcome);
  writeJsonExclusive(path.join(attemptDir, 'pre-state.json'), result.snapshots.pre);
  writeJsonExclusive(path.join(attemptDir, 'post-state.json'), result.snapshots.post);
  writeJsonExclusive(path.join(attemptDir, 'events.json'), result.events);
  writeJsonExclusive(path.join(attemptDir, 'block-events.json'), result[ATTEMPT_BLOCK_EVIDENCE]);
  writeJsonExclusive(path.join(attemptDir, 'result.json'), result);
  context.results.push(result);
  context.artifactSequence += 1;
}

function semanticSnapshot(snapshot) {
  const value = common.safeJson(snapshot);
  delete value.observed_at;
  return value;
}

function semanticResultHash(results) {
  return blake(results.map((result) => ({
    request: result.request,
    outcome: {
      dispatch_success: result.outcome.dispatch_success,
      protocol_outcome: result.outcome.protocol_outcome,
      common_error_code: result.outcome.common_error_code,
    },
    snapshots: {
      pre: semanticSnapshot(result.snapshots.pre),
      post: semanticSnapshot(result.snapshots.post),
    },
    events: result.events,
    settlement: result.settlement,
    correctness: result.correctness,
  })));
}

function artifactInventory(outputRoot) {
  const rows = [];
  function walk(current) {
    for (const name of fs.readdirSync(current).sort()) {
      const item = path.join(current, name);
      const stat = fs.statSync(item);
      if (stat.isDirectory()) walk(item);
      else rows.push({ path: path.relative(outputRoot, item).split(path.sep).join('/'), bytes: stat.size, sha256: sha256File(item) });
    }
  }
  walk(outputRoot);
  return rows;
}

function validateDependencies(args, preflight) {
  const actual = args.dependency.map((item) => path.resolve(item)).sort();
  const expected = [...REQUIRED_DEPENDENCIES].sort();
  assert(JSON.stringify(actual) === JSON.stringify(expected), 'frozen dependency list differs from E0 executor requirements');
  const preflightArtifacts = new Map(
    preflight.execution.executor_artifacts.map((item) => [path.resolve(item.path), item.sha256])
  );
  for (const dependency of expected) {
    assert(fs.statSync(dependency).isFile(), `missing frozen dependency ${dependency}`);
    assert(preflightArtifacts.get(dependency) === sha256File(dependency), `dependency hash is not frozen or drifted: ${dependency}`);
  }
  assert(process.env.POLKADOT_DRIVER_ROOT === NODE_DEP_ROOT, 'POLKADOT_DRIVER_ROOT differs from frozen WSL dependency root');
}

function validateExecutionContract(args, preflight, catalog) {
  const family = preflight.experiment_family;
  assert(Object.prototype.hasOwnProperty.call(FAMILY_SCENARIOS, family), `executor rejects ${family} preflight`);
  const expectedRepetitions = family === 'E0' ? 2 : 5;
  assert(preflight.inputs.repetitions === expectedRepetitions, `${family} requires ${expectedRepetitions} repetitions`);
  assert(args.repetition <= preflight.inputs.repetitions, `repetition exceeds ${family} preflight`);
  const selected = [...preflight.inputs.selected_scenario_ids].sort();
  const expectedSelected = [...FAMILY_SCENARIOS[family]].sort();
  assert(JSON.stringify(selected) === JSON.stringify(expectedSelected), `${family} selected scenario scope drift`);
  assert(expectedSelected.includes(args.scenario_id), `${family} executor rejects ${args.scenario_id}`);
  const scenario = catalog.scenarios.find((item) => item.scenario_id === args.scenario_id);
  assert(scenario, `scenario ${args.scenario_id} is absent from catalog`);
  assert(scenario.execution_mode === args.execution_mode, `${args.scenario_id} execution mode drift`);
  assert(scenario.operation === SCENARIO_OPERATION[args.scenario_id], 'scenario operation drift');
  assert(
    JSON.stringify(scenario.attempts.map((item) => item.phase)) === JSON.stringify(SCENARIO_PHASES[args.scenario_id]),
    `${args.scenario_id} attempt contract drift`
  );
  return scenario;
}

function actorRoles(operation) {
  return {
    create: 'RELAYER',
    seller: operation === 'TransferRight' ? 'RIGHT_SELLER' : 'OWNER',
    fund: operation === 'TransferRight' ? 'BUYER_2' : 'BUYER_1',
    settle: 'RELAYER',
  };
}

function usesCompetitionExecution(family, scenarioId) {
  return family === 'E4' || (family === 'E5' && E5_COMPETITION_SCENARIOS.has(scenarioId));
}

function rebuildInput(signed, overrides = {}) {
  const order = signed.order;
  return {
    settlementProtocolVersion: order.settlementProtocolVersion,
    marketId: order.marketId,
    operation: enumName(order.operation),
    objectType: enumName(order.objectType),
    objectId: order.objectId,
    parentAssetId: order.parentAssetId,
    authorizationId: order.authorizationId,
    allowedBuyer: order.allowedBuyer,
    price: BigInt(order.price),
    policyHash: order.policyHash,
    rightParametersHash: order.rightParametersHash,
    sellerStateVersion: Number(order.sellerStateVersion),
    orderNonce: Number(order.orderNonce),
    expiry: Number(order.expiry),
    ...overrides,
  };
}

function corruptSignature(signature) {
  const value = Uint8Array.from(signature);
  value[0] ^= 0xff;
  return value;
}

function g3ExperimentRight(
  accounts, assetId, rightId, marketIds, fixtureAnchor, validUntil,
  status = 'Active', remainingUses = 1
) {
  return {
    version: Array.from(Buffer.from('1.0', 'utf8')),
    rightId,
    tokenId: 1,
    rightType: 'View',
    dataScope: 'FullAsset',
    validity: { validFrom: fixtureAnchor, validUntil, maxUses: 1 },
    remainingUses,
    createdAt: fixtureAnchor,
    holder: accounts.RIGHT_SELLER.address,
    issuer: accounts.OWNER.address,
    assetId,
    transferable: true,
    allowedTransferMarkets: marketIds,
    royaltyBps: 0,
    stateVersion: 1,
    status,
    signature: [],
  };
}

function contextFor(args, preflight, scenario) {
  return {
    args,
    preflight,
    scenario,
    baseline: preflight.baseline.baseline_id,
    operation: scenario.operation,
    repetition: args.repetition,
    outputRoot: args.output_root,
    nextSubmissionSequence: 1,
    artifactSequence: 1,
    activeSubmissionSequence: null,
    results: [],
    lastRecord: null,
    snapshotSellerRole: null,
    snapshotBuyerRole: null,
  };
}

function frozenArtifactRoot(context, worktree, relative) {
  return context.args.artifact_root
    ? path.join(context.args.artifact_root, relative)
    : path.join(worktree, relative);
}

async function runtimeSnapshot(context, runtime, state, blockHash = null) {
  const { api, accounts, assetId, rightId, authId, marketId, signed, sellerRole, buyerRole } = state;
  const boundary = await resolveSnapshotBoundary(api, blockHash);
  const queryApi = boundary.queryApi;
  const prefix = runtime === 'B2' ? queryApi.query.marketRuntime : queryApi.query.dataAssets;
  const orderCodec = await prefix.runtimeOrders(signed.orderId);
  const order = optionJson(orderCodec);
  let meta;
  let right;
  let activeRightCount;
  let activeMetaOrder;
  let activeRightOrder;
  let authorization;
  let market;
  if (runtime === 'B2') {
    meta = optionJson(await prefix.metaTokens(assetId));
    right = rightId ? optionJson(await prefix.rightTokens(rightId)) : null;
    activeRightCount = Number((await prefix.activeRightCount(assetId)).toString());
    activeMetaOrder = optionJson(await prefix.activeMetaOrder(assetId));
    activeRightOrder = rightId ? optionJson(await prefix.activeRightOrder(rightId)) : null;
    if (context.operation === 'TransferMeta') authorization = optionJson(await prefix.transferMetaAuthorizations(authId));
    else if (context.operation === 'IssueRight') authorization = optionJson(await prefix.issueRightAuthorizations(authId));
    else authorization = null;
    market = optionJson(await prefix.markets(marketId));
  } else {
    const assetValue = optionJson(await queryApi.call.dataAssetsApi.getAsset(assetId));
    meta = assetValue ? field(assetValue, 'core') : null;
    right = rightId ? optionJson(await queryApi.call.dataAssetsApi.getCertificate(assetId, rightId)) : null;
    activeRightCount = Number((await prefix.activeCertificateCount(assetId)).toString());
    activeMetaOrder = optionJson(await prefix.activeMetaOrderByAsset(assetId));
    activeRightOrder = rightId ? optionJson(await prefix.activeRightOrderByRight(rightId)) : null;
    if (context.operation === 'TransferMeta') authorization = optionJson(await prefix.transferMetaAuthorizations(authId));
    else if (context.operation === 'IssueRight') authorization = optionJson(await prefix.issueRightAuthorizations(authId));
    else authorization = null;
    market = optionJson(await queryApi.query.markets.marketRegistrations(marketId));
  }
  assert(meta, `${runtime} authority snapshot omitted meta`);
  const escrowId = order ? field(order, 'escrowId', 'escrow_id') : null;
  const escrowCodec = escrowId ? await prefix.runtimeEscrows(escrowId) : null;
  const escrow = escrowCodec ? optionJson(escrowCodec) : null;
  const settlementCodec = await prefix.settlementRecords(signed.orderId);
  const settlement = optionJson(settlementCodec);
  const record = normalizedRecord(settlement);
  const commitment = field(order || {}, 'orderCommitment', 'order_commitment') || signed.order;
  const buyerCommitment = field(order || {}, 'buyerCommitment', 'buyer_commitment');
  const authPolicyHash = record ? record.policy_hash : signed.order.policyHash;
  const validity = right ? field(right, 'validity') : null;
  const currentBlock = boundary.blockNumber;
  const rightStatus = right ? enumName(field(right, 'status')) : null;
  const allowedTransferMarkets = right
    ? (field(right, 'allowedTransferMarkets', 'allowed_transfer_markets') || []).map(hex32)
    : [];
  const resaleAuthorized = context.operation === 'TransferRight'
    && Boolean(right)
    && Boolean(field(right, 'transferable'))
    && allowedTransferMarkets.includes(hex32(marketId));
  const rightValid = right ? (
    rightStatus === 'Active'
      && (field(validity, 'validFrom', 'valid_from') === undefined || currentBlock >= Number(field(validity, 'validFrom', 'valid_from')))
      && (field(validity, 'validUntil', 'valid_until') === null || field(validity, 'validUntil', 'valid_until') === undefined || currentBlock <= Number(field(validity, 'validUntil', 'valid_until')))
      && Number(field(right, 'remainingUses', 'remaining_uses') ?? field(validity, 'maxUses', 'max_uses') ?? 0) > 0
  ) : null;
  const balances = await paymentBalances(api, accounts, sellerRole, buyerRole, boundary.blockHash);
  const snapshot = {
    schema_id: 'assetxchain-exp2-normalized-snapshot',
    schema_version: 1,
    contract_version: 'phase3-adapter-contract-v1',
    baseline_id: runtime,
    authority_source: 'runtime',
    observed_at: { block_number: currentBlock, attempt_sequence: observedAttemptSequence(context) },
    meta: {
      exists: true,
      asset_id: hex32(assetId),
      owner: account(field(meta, 'owner')),
      status: enumName(field(meta, 'status')),
      state_version: Number(field(meta, 'stateVersion', 'state_version')),
      owner_index_matches: true,
      active_right_count: activeRightCount,
      active_order_id: hex32(activeMetaOrder),
    },
    right: {
      exists: Boolean(right),
      right_id: right ? hex32(field(right, 'rightId', 'right_id')) : null,
      holder: right ? account(field(right, 'holder')) : null,
      status: rightStatus,
      state_version: right ? Number(field(right, 'stateVersion', 'state_version')) : null,
      parent_asset_id: right ? hex32(field(right, 'assetId', 'asset_id')) : null,
      valid: rightValid,
      remaining_uses: right ? Number(field(right, 'remainingUses', 'remaining_uses') ?? field(validity, 'maxUses', 'max_uses')) : null,
      holder_index_matches: right ? true : null,
      parent_index_matches: right ? true : null,
      active_order_id: right ? hex32(activeRightOrder) : null,
    },
    order: {
      exists: Boolean(order),
      order_id: order ? hex32(signed.orderId) : null,
      operation: order ? enumName(field(commitment, 'operation')) : null,
      state: order ? enumName(field(order, 'state')) : null,
      seller: order ? account(field(commitment, 'seller')) : null,
      buyer: buyerCommitment ? account(field(buyerCommitment, 'buyer')) : null,
      expiry: order ? Number(field(commitment, 'expiry')) : null,
      grace_deadline: order && field(order, 'graceDeadline', 'grace_deadline') !== null && field(order, 'graceDeadline', 'grace_deadline') !== undefined
        ? Number(field(order, 'graceDeadline', 'grace_deadline')) : null,
      active_capacity_occupied: order ? ['Open', 'Funded'].includes(enumName(field(order, 'state'))) : false,
      funded_capacity_occupied: order ? enumName(field(order, 'state')) === 'Funded' : false,
    },
    authorization: {
      exists: context.operation === 'TransferRight' ? resaleAuthorized : Boolean(authorization),
      authorization_id: hex32(authId),
      status: context.operation === 'TransferRight'
        ? (resaleAuthorized ? 'Active' : null)
        : enumName(field(authorization, 'status')),
      policy_hash: hex32(authPolicyHash),
      issued_count: context.operation === 'IssueRight' && authorization ? Number(field(authorization, 'issuedCount', 'issued_count')) : 0,
      reserved_count: context.operation === 'IssueRight' && authorization ? Number(field(authorization, 'reservedCount', 'reserved_count')) : 0,
      issued_use_units: context.operation === 'IssueRight' && authorization ? Number(field(authorization, 'issuedUseUnits', 'issued_use_units')) : 0,
      reserved_use_units: context.operation === 'IssueRight' && authorization ? Number(field(authorization, 'reservedUseUnits', 'reserved_use_units')) : 0,
      consumed: context.operation === 'TransferMeta' && authorization
        ? field(authorization, 'consumedOrderId', 'consumed_order_id') !== null : false,
      consumed_order_id: context.operation === 'TransferMeta' && authorization
        ? hex32(field(authorization, 'consumedOrderId', 'consumed_order_id')) : null,
    },
    payment: {
      state: escrow ? enumName(field(escrow, 'state')) : 'Unfunded',
      escrow_amount: escrow ? decimal(field(field(escrow, 'amounts'), 'grossPrice', 'gross_price')) : '0',
      ...balances,
    },
    evidence: {
      settlement_record_count: record ? 1 : 0,
      settlement_id: record ? record.settlement_id : null,
      record_fields_hash: settlementCodec && settlementCodec.isSome ? u8aToHex(blake2AsU8a(settlementCodec.unwrap().toU8a(), 256)) : null,
      pre_state_commitment: record ? record.pre_state_commitment : null,
    },
    market: {
      exists: Boolean(market),
      market_id: hex32(marketId),
      status: market ? enumName(field(market, 'status')) : null,
      capabilities: market ? capabilities(field(market, 'capabilities')) : [],
      active_orders: market ? Number(field(market, 'activeOrders', 'active_orders')) : 0,
      funded_orders: market ? Number(field(market, 'fundedOrders', 'funded_orders')) : 0,
      active_exposure_value: market ? decimal(field(market, 'activeExposureValue', 'active_exposure_value')) : '0',
    },
    identity: {
      settlement_protocol_version: 1,
      settlement_domain_id: 'axc/p2/shared',
      order_commitment_hash: record ? record.order_commitment_hash : (order ? blake(
        api.createType(
          runtime === 'B2' ? api.tx.marketRuntime.createOrder.meta.args[0].type.toString() : g3.TYPE.order,
          commitment
        ).toU8a()
      ) : null),
      seller_nonce: order ? Number(field(commitment, 'orderNonce', 'order_nonce')) + 1 : 0,
      buyer_nonce: buyerCommitment ? Number(field(buyerCommitment, 'buyerNonce', 'buyer_nonce')) + 1 : 0,
      authorization_nonce: context.operation === 'TransferRight' || !authorization
        ? null : Number(field(authorization, 'nonce')),
      object_state_version: context.operation === 'TransferRight'
        ? (right ? Number(field(right, 'stateVersion', 'state_version')) : 1)
        : Number(field(meta, 'stateVersion', 'state_version')),
    },
    state_commitment: ZERO_HASH,
    projection: runtime === 'B2'
      ? { availability: 'N/A', authoritative: false, order_state: null, settlement_record_count: null }
      : { availability: 'unavailable', authoritative: false, order_state: null, settlement_record_count: null },
  };
  if (snapshot.identity.order_commitment_hash === null && order) snapshot.identity.order_commitment_hash = blake(commitment);
  finishSnapshot(snapshot);
  return { snapshot, record };
}

async function runtimeAttemptSnapshots(context, runtime, state, tx) {
  assert(tx.blockHash && tx.parentBlockHash, `${runtime} transaction omitted state-boundary hashes`);
  const balanceRoles = [
    ['buyer_balance', context.snapshotBuyerRole],
    ['seller_balance', context.snapshotSellerRole],
    ['market_balance', 'MARKET_OK'],
  ].filter(([, role]) => state.accounts && role && state.accounts[role]);
  if (balanceRoles.length > 0) {
    const readBalances = async (blockHash) => Object.fromEntries(
      await Promise.all(balanceRoles.map(async ([fieldName, role]) => [
        fieldName,
        await freeBalance(state.api, state.accounts[role], blockHash),
      ]))
    );
    tx.stateBoundary = {
      actor_role: context.snapshotBuyerRole,
      parent_hash: tx.parentBlockHash,
      included_hash: tx.blockHash,
      parent_balances: await readBalances(tx.parentBlockHash),
      included_balances: await readBalances(tx.blockHash),
    };
  }
  const pre = await runtimeSnapshot(context, runtime, state, tx.parentBlockHash);
  const post = await runtimeSnapshot(context, runtime, state, tx.blockHash);
  if (tx.stateBoundary) {
    Object.assign(pre.snapshot.payment, tx.stateBoundary.parent_balances);
    Object.assign(post.snapshot.payment, tx.stateBoundary.included_balances);
    finishSnapshot(pre.snapshot);
    finishSnapshot(post.snapshot);
  }
  assert(
    post.snapshot.observed_at.block_number === tx.blockNumber,
    `${runtime} included-block snapshot number differs from transaction evidence`
  );
  assert(
    pre.snapshot.observed_at.block_number + 1 === post.snapshot.observed_at.block_number,
    `${runtime} parent/included snapshot boundary is not adjacent`
  );
  return { pre, post };
}

async function runB2(context) {
  const api = await common.connect(context.args.rpc);
  const accounts = common.sharedAccounts();
  context.accountsByRole = accounts;
  const evidence = { transactions: [], events: {} };
  const roles = actorRoles(context.operation);
  const scenarioId = context.scenario.scenario_id;
  const isE2 = context.preflight.experiment_family === 'E2'
    || (context.preflight.experiment_family === 'E7' && ['V1', 'V2'].includes(context.preflight.inputs.e7_variant));
  const isE3 = context.preflight.experiment_family === 'E3';
  const isCompetition = usesCompetitionExecution(context.preflight.experiment_family, scenarioId)
    || (context.preflight.experiment_family === 'E7' && ['V3', 'V4', 'Q1'].includes(context.preflight.inputs.e7_variant));
  try {
    await common.fundSharedAccounts(api, accounts, evidence);
    const marketId = b2.fixedHash(0x51);
    const marketCall = api.tx.marketRuntime.registerMarket(
      marketId, accounts.MARKET_OK.address, 1, 31,
      scenarioId === 'NEG-FEE-01' ? 1 : 0,
      { tierId: 1, maxActiveOrders: 32, maxFundedOrders: 16, maxActiveExposureValue: (1000n * common.PRICES.TransferMeta).toString() }, '0'
    );
    await common.sendTx(api, 'register_market', api.tx.sudo.sudo(marketCall), accounts.OWNER);
    let authorizationWitnessMarketId = null;
    if (scenarioId === 'NEG-AUTH-01') {
      authorizationWitnessMarketId = b2.fixedHash(0x52);
      const witnessMarketCall = api.tx.marketRuntime.registerMarket(
        authorizationWitnessMarketId, accounts.MARKET_DENIED.address, 1, 31, 0,
        { tierId: 2, maxActiveOrders: 32, maxFundedOrders: 16, maxActiveExposureValue: (1000n * common.PRICES.TransferMeta).toString() }, '0'
      );
      await common.sendTx(
        api, 'register_authorization_witness_market',
        api.tx.sudo.sudo(witnessMarketCall), accounts.OWNER
      );
    }
    const assetByte = { TransferMeta: 0x61, IssueRight: 0x62, TransferRight: 0x63 }[context.operation];
    const assetId = b2.fixedHash(assetByte);
    await common.sendTx(api, 'register_meta', api.tx.marketRuntime.registerMeta({
      assetId, owner: accounts.OWNER.address, registrationProvider: accounts.OWNER.address,
      serviceProvider: accounts.OWNER.address, metadataDigest: b2.fixedHash(0x70),
      dataCommitment: b2.fixedHash(0x71), validationProgramHash: b2.fixedHash(0x72),
      status: 'Tradable', parentAssetId: null, supersedesAssetId: null,
    }), accounts.OWNER);
    const fixtureAnchor = await deterministicFixtureAnchor(api);
    const validUntil = fixtureAnchor + (context.preflight.experiment_family === 'E7'
      && context.preflight.inputs.e7_variant === 'V1' && context.operation === 'TransferRight'
      ? 2 : common.TIMING.rightValidity);
    let rightId = null;
    let authId;
    let signed;
    let secondSigned = null;
    let secondRightId = null;
    if (context.operation === 'TransferRight') {
      rightId = b2.fixedHash(0x73);
      const allowedTransferMarkets = [authorizationWitnessMarketId || marketId];
      const rightStatus = context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'V1'
        ? 'Active' : {
        'NEG-RIGHT-EXPIRED-01': 'Expired',
        'NEG-RIGHT-EXHAUSTED-01': 'Exhausted',
        'NEG-RIGHT-REVOKED-01': 'Revoked',
      }[scenarioId] || 'Active';
      const rightInput = {
        rightId, assetId, holder: accounts.RIGHT_SELLER.address, issuer: accounts.OWNER.address,
        rightType: 'View', dataScope: 'FullAsset',
        validity: { validFrom: fixtureAnchor, validUntil, maxUses: 1 }, transferable: true,
        allowedTransferMarkets, royaltyBps: 0,
      };
      const rightCall = isE2
        ? api.tx.marketRuntime.initializeRightForExperiment(
          rightInput, rightStatus, scenarioId === 'NEG-RIGHT-EXHAUSTED-01' ? 0 : 1
        )
        : api.tx.marketRuntime.initializeExistingRight(rightInput);
      await common.sendTx(
        api, 'initialize_resale_right', isE2 ? api.tx.sudo.sudo(rightCall) : rightCall, accounts.OWNER
      );
      authId = common.computeResaleAuthorizationId(rightId, marketId);
      const expiry = await b2.nextExpiry(api);
      signed = b2.buildSignedOrder(api, api.genesisHash.toHex(), accounts.RIGHT_SELLER, {
        marketId, operation: 'TransferRight', objectType: 'RightToken', objectId: rightId,
        parentAssetId: assetId, authorizationId: authId, price: common.PRICES.TransferRight,
        policyHash: b2.pricePolicyHash(common.PRICES.TransferRight),
        rightParametersHash: b2.rightParametersHash(marketId, fixtureAnchor, validUntil), expiry,
      });
    } else if (context.operation === 'TransferMeta') {
      const expiry = await b2.nextExpiry(api, 1);
      const allowedBuyer = scenarioId === 'RACE-BUYER-01' ? null : accounts.BUYER_1.address;
      const authTx = await common.sendTx(api, 'create_meta_authorization', api.tx.marketRuntime.createTransferMetaAuthorization(
        assetId, marketId,
        { pricePolicy: b2.pricePolicy(common.PRICES.TransferMeta), allowedBuyer, expiry, nonce: 0 }
      ), accounts.OWNER);
      const authEvent = common.findEvent(authTx.events, 'marketRuntime', 'TransferMetaAuthorizationCreated');
      assert(authEvent, 'B2 transfer authorization event missing');
      authId = authEvent.event.data[0].toHex();
      signed = b2.buildSignedOrder(api, api.genesisHash.toHex(), accounts.OWNER, {
        marketId, operation: 'TransferMeta', objectType: 'MetaToken', objectId: assetId,
        authorizationId: authId,
        allowedBuyer: allowedBuyer ? common.accountHex(accounts.BUYER_1) : null,
        price: common.PRICES.TransferMeta, policyHash: b2.pricePolicyHash(common.PRICES.TransferMeta), expiry,
      });
      if (scenarioId === 'NEG-META-RIGHT-01') {
        rightId = b2.fixedHash(0x74);
        const rightCall = api.tx.marketRuntime.initializeRightForExperiment({
          rightId, assetId, holder: accounts.RIGHT_SELLER.address, issuer: accounts.OWNER.address,
          rightType: 'View', dataScope: 'FullAsset',
          validity: { validFrom: fixtureAnchor, validUntil, maxUses: 1 }, transferable: true,
          allowedTransferMarkets: [marketId], royaltyBps: 0,
        }, 'Active', 1);
        await common.sendTx(
          api, 'initialize_conflicting_right', api.tx.sudo.sudo(rightCall), accounts.OWNER
        );
      }
    } else {
      const expiry = await b2.nextExpiry(api, 1);
      const issueCapacity = scenarioId === 'RACE-QUOTA-01' ? 1 : 2;
      const terms = {
        rightType: 'View', dataScope: 'FullAsset', validityTemplate: { validFrom: fixtureAnchor, validUntil, maxUses: 1 },
        maxIssueCount: issueCapacity, maxTotalUseUnits: issueCapacity,
        pricePolicy: b2.pricePolicy(common.PRICES.IssueRight),
        royaltyBps: 0, transferable: true, allowedTransferMarkets: [marketId], expiry, nonce: 0,
      };
      const authTx = await common.sendTx(api, 'create_issue_authorization', api.tx.marketRuntime.createIssueRightAuthorization(assetId, marketId, terms), accounts.OWNER);
      const authEvent = common.findEvent(authTx.events, 'marketRuntime', 'IssueRightAuthorizationCreated');
      assert(authEvent, 'B2 issue authorization event missing');
      authId = authEvent.event.data[0].toHex();
      signed = b2.buildSignedOrder(api, api.genesisHash.toHex(), accounts.OWNER, {
        marketId, operation: 'IssueRight', objectType: 'MetaToken', objectId: assetId,
        authorizationId: authId, price: common.PRICES.IssueRight,
        policyHash: b2.pricePolicyHash(common.PRICES.IssueRight),
        rightParametersHash: b2.rightParametersHash(marketId, fixtureAnchor, validUntil), expiry,
      });
      rightId = common.computeRightId(api.genesisHash.toHex(), signed.orderId);
    }
    if (isCompetition && ['RACE-QUOTA-01', 'RACE-OBJECT-01'].includes(scenarioId)) {
      const seller = context.operation === 'TransferRight' ? accounts.RIGHT_SELLER : accounts.OWNER;
      secondSigned = b2.buildSignedOrder(
        api, api.genesisHash.toHex(), seller, rebuildInput(signed, { orderNonce: 1 })
      );
      if (context.operation === 'IssueRight') {
        secondRightId = common.computeRightId(api.genesisHash.toHex(), secondSigned.orderId);
      }
    }
    if (isE2) {
      if (scenarioId === 'NEG-PROTO-01') {
        signed = b2.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.OWNER,
          rebuildInput(signed, { settlementProtocolVersion: 2 })
        );
      } else if (scenarioId === 'NEG-SIGN-01') {
        if (context.preflight.experiment_family === 'E7') {
          // Preserve the original signature while replacing a signed order field.
          const fieldName = context.operation === 'TransferMeta' ? 'allowedBuyer' : 'rightParametersHash';
          const originalValue = signed.order[fieldName];
          const originalSignature = signed.signature;
          if (fieldName === 'allowedBuyer') signed.order.allowedBuyer = common.accountHex(accounts.BUYER_2);
          else signed.order.rightParametersHash = b2.fixedHash(0x9a);
          writeJsonExclusive(path.join(context.outputRoot, 'field-replacement.json'), {
            variant: 'V2', field_replaced: fieldName,
            original_value: originalValue, mutated_value: signed.order[fieldName],
            signature_retained: signed.signature === originalSignature,
          });
        } else signed.signature = corruptSignature(signed.signature);
      } else if (scenarioId === 'NEG-OWNER-01') {
        signed = b2.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.MARKET_DENIED,
          rebuildInput(signed, { objectId: assetId })
        );
      } else if (scenarioId === 'NEG-HOLDER-01') {
        signed = b2.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.BUYER_1, rebuildInput(signed)
        );
      } else if (scenarioId === 'NEG-PARENT-01') {
        signed = b2.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.RIGHT_SELLER,
          rebuildInput(signed, { parentAssetId: b2.fixedHash(0xec) })
        );
      } else if (scenarioId === 'NEG-RIGHT-MISSING-01') {
        rightId = b2.fixedHash(0xeb);
        authId = common.computeResaleAuthorizationId(rightId, marketId);
        signed = b2.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.RIGHT_SELLER,
          rebuildInput(signed, {
            objectId: rightId, authorizationId: authId,
            rightParametersHash: b2.fixedHash(0xea),
          })
        );
      } else if (scenarioId === 'NEG-VERSION-01') {
        signed = b2.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.OWNER,
          rebuildInput(signed, { sellerStateVersion: 2 })
        );
      } else if (scenarioId === 'NEG-NONCE-01') {
        await common.sendTx(api, 'consume_nonce_fixture', api.tx.marketRuntime.createOrder(
          signed.order, 'Authorization', signed.sellerCommitment, signed.signature
        ), accounts.OWNER);
      } else if (scenarioId === 'NEG-PRICE-01') {
        signed = b2.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.OWNER,
          rebuildInput(signed, { price: common.PRICES.IssueRight - 1n })
        );
      }
      if (scenarioId === 'NEG-MARKET-01') {
        const fixtureCall = api.tx.marketRuntime.configureMarketForExperiment(
          marketId, false, null, null
        );
        await common.sendTx(api, 'remove_market_fixture', api.tx.sudo.sudo(fixtureCall), accounts.OWNER);
      } else if (scenarioId === 'NEG-CAPABILITY-01') {
        const fixtureCall = api.tx.marketRuntime.configureMarketForExperiment(
          marketId, true, 29, null
        );
        await common.sendTx(api, 'limit_market_fixture', api.tx.sudo.sudo(fixtureCall), accounts.OWNER);
      } else if (scenarioId === 'NEG-MARKET-RESTRICTED-01') {
        await common.sendTx(api, 'restrict_market_fixture', api.tx.sudo.sudo(
          api.tx.marketRuntime.setMarketStatus(marketId, 'Restricted')
        ), accounts.OWNER);
      }
    }
    const payerRole = scenarioId === 'NEG-PAYER-01'
      ? (roles.fund === 'BUYER_1' ? 'BUYER_2' : 'BUYER_1')
      : roles.fund;
    const state = {
      api, accounts, assetId, rightId, authId, marketId, signed, sellerRole: roles.seller,
      buyerRole: roles.fund,
    };
    context.snapshotSellerRole = roles.seller;
    context.snapshotBuyerRole = roles.fund;
    const startBlock = (await api.rpc.chain.getHeader()).number.toNumber();
    const createSignedOrder = (targetSigned, label, expectedFailure = null) => common.sendTx(
      api, label,
      api.tx.marketRuntime.createOrder(
        targetSigned.order,
        context.operation === 'TransferRight' ? { SellerSnapshot: b2.pricePolicy(OPERATION_PRICE[context.operation]) } : 'Authorization',
        targetSigned.sellerCommitment, targetSigned.signature
      ),
      accounts[roles.create], expectedFailure
    );
    const createOrder = (label) => createSignedOrder(signed, label);
    const fundSignedOrder = (targetSigned, buyerRole, nonce, label, expectedFailure = null) => {
      const targetBuyer = accounts[buyerRole];
      const targetBuyerSigned = b2.buildSignedBuyer(
        api, api.genesisHash.toHex(), targetBuyer, targetSigned, nonce
      );
      return common.sendTx(
        api, label,
        api.tx.marketRuntime.fundOrder(
          targetSigned.orderId, targetBuyerSigned.commitment, targetBuyerSigned.signature
        ),
        targetBuyer, expectedFailure
      );
    };
    const buyer = accounts[roles.fund];
    const buyerSigned = b2.buildSignedBuyer(api, api.genesisHash.toHex(), buyer, signed, 0);
    const fundOrder = (label) => fundSignedOrder(signed, roles.fund, 0, label);
    if (context.scenario.scenario_id.startsWith('LEG-') && context.scenario.attempts.length === 1) {
      await createOrder('fixture_create');
      if (context.scenario.scenario_id === 'LEG-REFUND-01') await fundOrder('fixture_fund');
      if (context.scenario.scenario_id === 'LEG-EXPIRE-01') await waitUntilAfterBlock(api, signed.order.expiry);
      if (context.scenario.scenario_id === 'LEG-REFUND-01') {
        const funded = (await runtimeSnapshot(context, 'B2', state)).snapshot;
        await waitUntilAfterBlock(api, funded.order.grace_deadline);
      }
    }
    if (isE2 && ['fund_order', 'settle', 'refund_funded'].includes(context.scenario.attempts[0].phase)) {
      await createOrder('fixture_create');
      if (context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'V1'
        && context.operation === 'TransferRight') {
        await waitUntilAfterBlock(api, validUntil);
        const observedBlock = (await api.rpc.chain.getHeader()).number.toNumber();
        writeJsonExclusive(path.join(context.outputRoot, 'stale-intent.json'), {
          variant: 'V1', lifecycle_transition: 'right-expired-by-block',
          right_id: rightId, signed_order_id: signed.orderId,
          cached_valid_until: validUntil, observed_block: observedBlock,
          stale_condition_observed: observedBlock > validUntil,
        });
      }
      if (scenarioId === 'NEG-AUTH-REVOKED-01') {
        const revokeTx = await common.sendTx(
          api, 'revoke_authorization_fixture',
          (context.operation === 'TransferMeta'
            ? api.tx.marketRuntime.revokeTransferMetaAuthorization(authId)
            : api.tx.marketRuntime.revokeIssueRightAuthorization(authId)), accounts.OWNER
        );
        if (context.preflight.experiment_family === 'E7') {
          writeJsonExclusive(path.join(context.outputRoot, 'stale-intent.json'), {
            variant: 'V1', lifecycle_transition: 'authorization-revoked',
            authorization_id: authId, signed_order_id: signed.orderId,
            invalidation_tx_hash: revokeTx.txHash, invalidation_block: revokeTx.blockNumber,
            stale_condition_observed: true,
          });
        }
      }
      if (scenarioId === 'NEG-REFUND-EARLY-01') await fundOrder('fixture_fund');
      if (scenarioId === 'NEG-CAPACITY-01') {
        const fixtureCall = api.tx.marketRuntime.configureMarketForExperiment(
          marketId, true, null, 0
        );
        await common.sendTx(api, 'fill_capacity_fixture', api.tx.sudo.sudo(fixtureCall), accounts.OWNER);
      }
      if (scenarioId === 'NEG-BALANCE-01') {
        await common.sendTx(
          api, 'limit_buyer_balance_fixture',
          api.tx.sudo.sudo(api.tx.balances.forceSetBalance(
            accounts[roles.fund].address, (OPERATION_PRICE[context.operation] / 2n).toString()
          )), accounts.OWNER
        );
      }
    }
    if (isE3) {
      await createOrder('fixture_create');
      await fundOrder('fixture_fund');
      const statusCall = scenarioId === 'CLEAN-LATE-META-01'
        ? api.tx.marketRuntime.configureMetaStatusForExperiment(assetId, 'Retiring')
        : api.tx.marketRuntime.configureRightStatusForExperiment(rightId, 'Expired');
      await common.sendTx(
        api, 'apply_late_failure_delta', api.tx.sudo.sudo(statusCall), accounts.OWNER
      );
    }
    if (isCompetition) {
      if (['DUP-SETTLE-01', 'RACE-RELAYER-01'].includes(scenarioId)) {
        await createOrder('fixture_create');
        await fundOrder('fixture_fund');
      } else if (scenarioId === 'DUP-REFUND-01') {
        await createOrder('fixture_create');
        await fundOrder('fixture_fund');
        const funded = (await runtimeSnapshot(context, 'B2', state)).snapshot;
        await waitUntilAfterBlock(api, funded.order.grace_deadline);
      } else if (scenarioId === 'RACE-BUYER-01') {
        await createOrder('fixture_create');
      } else if (scenarioId === 'RACE-QUOTA-01') {
        await createOrder('fixture_create_first');
        await createSignedOrder(secondSigned, 'fixture_create_second');
      }
      if (context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'V4') {
        const field = context.operation === 'TransferMeta' ? 'allowedBuyer' : 'objectId';
        const tampered = { ...signed, order: { ...signed.order } };
        if (field === 'allowedBuyer') tampered.order.allowedBuyer = common.accountHex(accounts.BUYER_2);
        else tampered.order.objectId = b2.fixedHash(0xe7);
        const probe = await common.sendTx(
          api, 'e7_composite_field_probe', api.tx.marketRuntime.createOrder(
            tampered.order, context.operation === 'TransferRight'
              ? { SellerSnapshot: b2.pricePolicy(OPERATION_PRICE[context.operation]) } : 'Authorization',
            tampered.sellerCommitment, tampered.signature
          ), accounts[roles.create], { allowDispatchFailure: true }
        );
        writeJsonExclusive(path.join(context.outputRoot, 'composite-probe.json'), {
          variant: 'V4', field_replaced: field, signature_retained: tampered.signature === signed.signature,
          original_value: signed.order[field], mutated_value: tampered.order[field],
          dispatch_error: probe.dispatchError, included_block: probe.blockNumber, extrinsic_index: phaseIndex(probe),
        });
      }
    }
    const initial = (await runtimeSnapshot(context, 'B2', state)).snapshot;
    const fixture = {
      schema_id: 'assetxchain-exp2-phase4-e0-fixture', schema_version: 1,
      run_id: context.preflight.run_id, baseline_id: 'B2', scenario_id: context.scenario.scenario_id,
      repetition: context.repetition, genesis_hash: api.genesisHash.toHex(), fixture_anchor_block: fixtureAnchor,
      runtime_version: `${api.runtimeVersion.specName}-${api.runtimeVersion.specVersion}`,
      metadata_hash: u8aToHex(blake2AsU8a(api.runtimeMetadata.toU8a(), 256)),
      shared_input_hash: common.SHARED_INPUT_HASH,
      asset_id: assetId, right_id: rightId, authorization_id: authId, market_id: marketId,
      order_id: signed.orderId,
      competing_order_id: secondSigned ? secondSigned.orderId : null,
      competing_right_id: secondRightId,
      initial_state: initial,
    };
    writeJsonExclusive(path.join(context.outputRoot, 'fixture.json'), fixture);

    if (isE2 || isE3) {
      const expected = context.scenario.attempts[0];
      const expectedNative = isE2
        ? (context.preflight.experiment_family === 'E7' && context.preflight.inputs.e7_variant === 'V2'
          ? 'MalformedCommitment' : E2_NATIVE_ERROR.B2[scenarioId])
        : E3_NATIVE_ERROR.B2[scenarioId];
      const expectedFailure = {
        section: 'marketRuntime', name: expectedNative,
      };
      let actorRole = roles.create;
      let tx;
      if (expected.phase === 'create_order') {
        tx = await captureSubmission(context, () => common.sendTx(
          api, 'reject_create',
          api.tx.marketRuntime.createOrder(
            signed.order,
            context.operation === 'TransferRight'
              ? { SellerSnapshot: b2.pricePolicy(OPERATION_PRICE[context.operation]) }
              : 'Authorization',
            signed.sellerCommitment, signed.signature
          ),
          accounts[roles.create], expectedFailure
        ));
      } else if (expected.phase === 'fund_order') {
        actorRole = payerRole;
        tx = await captureSubmission(context, () => common.sendTx(
          api, 'reject_fund',
          api.tx.marketRuntime.fundOrder(
            signed.orderId, buyerSigned.commitment, buyerSigned.signature
          ),
          accounts[payerRole], expectedFailure
        ));
      } else if (expected.phase === 'settle') {
        actorRole = roles.settle;
        tx = await captureSubmission(context, () => common.sendTx(
          api, 'reject_settle', api.tx.marketRuntime.settle(signed.orderId),
          accounts[actorRole], expectedFailure
        ));
      } else if (expected.phase === 'refund_funded') {
        actorRole = roles.settle;
        tx = await captureSubmission(context, () => common.sendTx(
          api, 'reject_refund', api.tx.marketRuntime.refundFunded(signed.orderId),
          accounts[actorRole], expectedFailure
        ));
      } else {
        throw new Error(`B2 has no E2 execution path for ${expected.phase}`);
      }
      const boundary = await runtimeAttemptSnapshots(context, 'B2', state, tx);
      const pre = boundary.pre.snapshot;
      const post = boundary.post.snapshot;
      const rejection = b2Rejection(tx);
      assert(
        rejection.nativeError === expectedNative,
        `B2 native error priority drift for ${scenarioId}`
      );
      writeAttempt(
        context, expected, actorRole,
        makeRejectedResult(
          context, expected, tx, pre, post, rejectionEventInfo(tx), actorRole, rejection,
          null, false
        )
      );
    } else if (isCompetition) {
      if (context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'Q1') {
        const candidates = [
          { signed: signed, rightId, buyerRole: 'BUYER_1', label: 'e7_quota_candidate_1' },
          { signed: secondSigned, rightId: secondRightId, buyerRole: 'BUYER_2', label: 'e7_quota_candidate_2' },
        ];
        const overlap = candidates.map((candidate) => {
          const startedNs = process.hrtime.bigint();
          const buyerTx = b2.buildSignedBuyer(
            api, api.genesisHash.toHex(), accounts[candidate.buyerRole], candidate.signed, 0
          );
          return captureSubmission(context, () => common.sendTx(
            api, candidate.label,
            api.tx.marketRuntime.fundOrder(candidate.signed.orderId, buyerTx.commitment, buyerTx.signature),
            accounts[candidate.buyerRole], { allowDispatchFailure: true }
          )).then((tx) => ({ ...candidate, startedNs: startedNs.toString(), finishedNs: process.hrtime.bigint().toString(), tx }));
        });
        const submitted = await Promise.all(overlap);
        writeJsonExclusive(path.join(context.outputRoot, 'e7-concurrency.json'), {
          variant: 'Q1', concurrency: 2, purpose: 'last-quota-overlapping-fund-candidates',
          submissions: submitted.map(({ actor, buyerRole, startedNs, finishedNs, tx }) => ({
            actor: buyerRole, started_ns: startedNs, finished_ns: finishedNs,
            submission_sequence: tx.submissionSequence, tx_hash: tx.txHash,
            included_block: tx.blockNumber, extrinsic_index: phaseIndex(tx), dispatch_error: tx.dispatchError,
          })),
          overlap_observed: submitted.map((x) => BigInt(x.startedNs)).reduce((a, b) => a > b ? a : b)
            < submitted.map((x) => BigInt(x.finishedNs)).reduce((a, b) => a < b ? a : b),
        });
        let winner = null;
        for (const [position, item] of [...submitted].sort((a, b) => itemBlock(a.tx) - itemBlock(b.tx)
          || phaseIndex(a.tx) - phaseIndex(b.tx)).entries()) {
          const expected = context.scenario.attempts[position];
          const observedState = { ...state, signed: item.signed, rightId: item.rightId, buyerRole: item.buyerRole };
          context.snapshotBuyerRole = item.buyerRole;
          const boundary = await runtimeAttemptSnapshots(context, 'B2', observedState, item.tx);
          if (!item.tx.dispatchError) {
            winner = item;
            context.lastRecord = null;
            const info = runtimeEventInfo(item.tx, 'marketRuntime');
            writeAttempt(context, expected, item.buyerRole,
              makeResult(context, expected, item.tx, boundary.pre.snapshot, boundary.post.snapshot, info, item.buyerRole));
          } else {
            const rejection = b2Rejection(item.tx);
            writeAttempt(context, expected, item.buyerRole,
              makeRejectedResult(context, expected, item.tx, boundary.post.snapshot, boundary.post.snapshot,
                rejectionEventInfo(item.tx), item.buyerRole, rejection, null, false,
                rejection.commonError, true));
          }
        }
        assert(winner, 'E7 Q1 did not produce a quota winner');
        const settleExpected = context.scenario.attempts[2];
        const settleState = { ...state, signed: winner.signed, rightId: winner.rightId, buyerRole: winner.buyerRole };
        context.snapshotBuyerRole = winner.buyerRole;
        const settleTx = await captureSubmission(context, () => common.sendTx(
          api, 'e7_quota_settle_winner', api.tx.marketRuntime.settle(winner.signed.orderId), accounts.RELAYER
        ));
        const settleBoundary = await runtimeAttemptSnapshots(context, 'B2', settleState, settleTx);
        const settleInfo = runtimeEventInfo(settleTx, 'marketRuntime');
        context.lastRecord = settleBoundary.post.record;
        assert(context.lastRecord && settleInfo.postStateCommitment, 'E7 Q1 settlement closure missing');
        settleBoundary.pre.snapshot.state_commitment = context.lastRecord.pre_state_commitment;
        settleBoundary.post.snapshot.state_commitment = settleInfo.postStateCommitment;
        writeAttempt(context, settleExpected, 'RELAYER',
          makeResult(context, settleExpected, settleTx, settleBoundary.pre.snapshot, settleBoundary.post.snapshot,
            settleInfo, 'RELAYER'));
      } else if (context.preflight.experiment_family === 'E7'
        && ['V3', 'V4'].includes(context.preflight.inputs.e7_variant)) {
        const actors = ['RELAYER', 'OWNER', 'RIGHT_SELLER', 'MARKET_OK'];
        const overlap = actors.map((actor, index) => {
          const startedNs = process.hrtime.bigint();
          return captureSubmission(context, () => common.sendTx(
            api, `e7_overlap_${index + 1}`,
            api.tx.marketRuntime.settle(signed.orderId), accounts[actor],
            { allowDispatchFailure: true }
          )).then((tx) => ({ actor, startedNs: startedNs.toString(), finishedNs: process.hrtime.bigint().toString(), tx }));
        });
        const submitted = await Promise.all(overlap);
        writeJsonExclusive(path.join(context.outputRoot, 'e7-concurrency.json'), {
          variant: context.preflight.inputs.e7_variant, concurrency: actors.length,
          submissions: submitted.map(({ actor, startedNs, finishedNs, tx }) => ({
            actor, started_ns: startedNs, finished_ns: finishedNs,
            submission_sequence: tx.submissionSequence, tx_hash: tx.txHash,
            included_block: tx.blockNumber, extrinsic_index: phaseIndex(tx),
            dispatch_error: tx.dispatchError,
          })),
          overlap_observed: submitted.map((x) => BigInt(x.startedNs)).reduce((a, b) => a > b ? a : b)
            < submitted.map((x) => BigInt(x.finishedNs)).reduce((a, b) => a < b ? a : b),
        });
        for (const [position, item] of [...submitted].sort((a, b) => itemBlock(a.tx) - itemBlock(b.tx)
          || phaseIndex(a.tx) - phaseIndex(b.tx)).entries()) {
          const tx = item.tx;
          const expected = context.scenario.attempts[position];
          const observedState = { ...state, buyerRole: roles.fund };
          context.snapshotBuyerRole = roles.fund;
          const boundary = await runtimeAttemptSnapshots(context, 'B2', observedState, tx);
          const pre = boundary.pre.snapshot;
          const post = boundary.post.snapshot;
          if (!tx.dispatchError) {
            context.lastRecord = boundary.post.record;
            const info = runtimeEventInfo(tx, 'marketRuntime');
            pre.state_commitment = context.lastRecord.pre_state_commitment;
            post.state_commitment = info.postStateCommitment;
            writeAttempt(context, expected, item.actor, makeResult(context, expected, tx, pre, post, info, item.actor));
          } else {
            const rejection = b2Rejection(tx);
            // A same-block loser is observed only at the block boundary.  Its
            // execution pre-state is the winner's post-state, so use the
            // included boundary on both sides and keep the intra-block order
            // in e7-concurrency.json/dispatch.json.
            const loserBoundary = boundary.post.snapshot;
            writeAttempt(context, expected, item.actor,
              makeRejectedResult(context, expected, tx, loserBoundary, loserBoundary, rejectionEventInfo(tx), item.actor,
                rejection, null, false, rejection.commonError, true));
          }
        }
      }
      if (!(context.preflight.experiment_family === 'E7'
        && ['V3', 'V4', 'Q1'].includes(context.preflight.inputs.e7_variant))) {
      const expectedRaceError = context.scenario.cardinality
        ? context.scenario.cardinality.rejected_error_code
        : null;
      const attemptState = (targetSigned = signed, targetRightId = rightId, buyerRole = roles.fund) => {
        context.snapshotBuyerRole = buyerRole;
        return { ...state, signed: targetSigned, rightId: targetRightId, buyerRole };
      };
      const writeAccepted = async (expected, actorRole, submit, observedState, settlement = false) => {
        const tx = await captureSubmission(context, submit);
        const boundary = await runtimeAttemptSnapshots(context, 'B2', observedState, tx);
        const pre = boundary.pre.snapshot;
        const postRead = boundary.post;
        const info = runtimeEventInfo(tx, 'marketRuntime');
        if (settlement) {
          context.lastRecord = postRead.record;
          assert(context.lastRecord && info.postStateCommitment, 'B2 E4 settlement closure missing');
          pre.state_commitment = context.lastRecord.pre_state_commitment;
          postRead.snapshot.state_commitment = info.postStateCommitment;
        } else {
          context.lastRecord = null;
        }
        writeAttempt(
          context, expected, actorRole,
          makeResult(context, expected, tx, pre, postRead.snapshot, info, actorRole)
        );
      };
      const writeRejected = async (expected, actorRole, submit, observedState) => {
        const expectedNative = E4_NATIVE_ERROR.B2[scenarioId];
        const tx = await captureSubmission(context, submit);
        const boundary = await runtimeAttemptSnapshots(context, 'B2', observedState, tx);
        const pre = boundary.pre.snapshot;
        const post = boundary.post.snapshot;
        const rejection = b2Rejection(tx);
        assert(rejection.nativeError === expectedNative, `B2 E4 native error drift for ${scenarioId}`);
        writeAttempt(
          context, expected, actorRole,
          makeRejectedResult(
            context, expected, tx, pre, post, rejectionEventInfo(tx), actorRole,
            rejection, null, false, expected.expected_error_code || expectedRaceError
          )
        );
      };
      const expectedFailure = () => ({
        section: 'marketRuntime', name: E4_NATIVE_ERROR.B2[scenarioId],
      });
      if (scenarioId === 'DUP-CREATE-01') {
        await writeAccepted(
          context.scenario.attempts[0], roles.create,
          () => createSignedOrder(signed, 'create_first'), attemptState()
        );
        await writeRejected(
          context.scenario.attempts[1], roles.create,
          () => createSignedOrder(signed, 'create_duplicate', expectedFailure()), attemptState()
        );
      } else if (scenarioId === 'RACE-OBJECT-01') {
        await writeAccepted(
          context.scenario.attempts[0], roles.create,
          () => createSignedOrder(signed, 'create_first'), attemptState()
        );
        await writeRejected(
          context.scenario.attempts[1], roles.create,
          () => createSignedOrder(secondSigned, 'create_competing', expectedFailure()),
          attemptState(secondSigned, rightId)
        );
      } else if (scenarioId === 'RACE-BUYER-01') {
        await writeAccepted(
          context.scenario.attempts[0], 'BUYER_1',
          () => fundSignedOrder(signed, 'BUYER_1', 0, 'fund_buyer_1'),
          attemptState(signed, rightId, 'BUYER_1')
        );
        await writeRejected(
          context.scenario.attempts[1], 'BUYER_2',
          () => fundSignedOrder(signed, 'BUYER_2', 0, 'fund_buyer_2', expectedFailure()),
          attemptState(signed, rightId, 'BUYER_2')
        );
      } else if (scenarioId === 'RACE-QUOTA-01') {
        await writeAccepted(
          context.scenario.attempts[0], 'BUYER_1',
          () => fundSignedOrder(signed, 'BUYER_1', 0, 'fund_first_order'),
          attemptState(signed, rightId, 'BUYER_1')
        );
        await writeRejected(
          context.scenario.attempts[1], 'BUYER_2',
          () => fundSignedOrder(secondSigned, 'BUYER_2', 0, 'fund_last_quota', expectedFailure()),
          attemptState(secondSigned, secondRightId, 'BUYER_2')
        );
        await writeAccepted(
          context.scenario.attempts[2], 'RELAYER',
          () => common.sendTx(
            api, 'settle_quota_winner', api.tx.marketRuntime.settle(signed.orderId), accounts.RELAYER
          ),
          attemptState(signed, rightId, 'BUYER_1'), true
        );
      } else if (scenarioId === 'DUP-REFUND-01') {
        await writeAccepted(
          context.scenario.attempts[0], 'RELAYER',
          () => common.sendTx(
            api, 'refund_first', api.tx.marketRuntime.refundFunded(signed.orderId), accounts.RELAYER
          ),
          attemptState()
        );
        await writeRejected(
          context.scenario.attempts[1], 'OWNER',
          () => common.sendTx(
            api, 'refund_duplicate', api.tx.marketRuntime.refundFunded(signed.orderId),
            accounts.OWNER, expectedFailure()
          ),
          attemptState()
        );
      } else {
        const relayers = scenarioId === 'RACE-RELAYER-01'
          ? ['RELAYER', 'OWNER', 'RIGHT_SELLER', 'MARKET_OK']
          : ['RELAYER', 'OWNER'];
        for (let index = 0; index < context.scenario.attempts.length; index += 1) {
          const expected = context.scenario.attempts[index];
          const actorRole = relayers[index];
          if (index === 0) {
            await writeAccepted(
              expected, actorRole,
              () => common.sendTx(
                api, `settle_${index + 1}`, api.tx.marketRuntime.settle(signed.orderId),
                accounts[actorRole]
              ),
              attemptState(), true
            );
          } else {
            await writeRejected(
              expected, actorRole,
              () => common.sendTx(
                api, `settle_${index + 1}`, api.tx.marketRuntime.settle(signed.orderId),
                accounts[actorRole], expectedFailure()
              ),
              attemptState()
            );
          }
        }
      }
      }
    } else if (context.scenario.attempts.length === 3) {
      let expected = context.scenario.attempts[0];
      const create = await captureSubmission(context, () => createOrder('create'));
      let boundary = await runtimeAttemptSnapshots(context, 'B2', state, create);
      let pre = boundary.pre.snapshot;
      let post = boundary.post.snapshot;
      let info = runtimeEventInfo(create, 'marketRuntime');
      writeAttempt(context, expected, roles.create, makeResult(context, expected, create, pre, post, info, roles.create));

      expected = context.scenario.attempts[1];
      const fund = await captureSubmission(context, () => fundOrder('fund'));
      boundary = await runtimeAttemptSnapshots(context, 'B2', state, fund);
      pre = boundary.pre.snapshot;
      post = boundary.post.snapshot;
      info = runtimeEventInfo(fund, 'marketRuntime');
      writeAttempt(context, expected, roles.fund, makeResult(context, expected, fund, pre, post, info, roles.fund));

      expected = context.scenario.attempts[2];
      const settle = await captureSubmission(context, () => common.sendTx(
        api, 'settle', api.tx.marketRuntime.settle(signed.orderId), accounts.RELAYER
      ));
      boundary = await runtimeAttemptSnapshots(context, 'B2', state, settle);
      pre = boundary.pre.snapshot;
      const postRead = boundary.post;
      context.lastRecord = postRead.record;
      info = runtimeEventInfo(settle, 'marketRuntime');
      assert(context.lastRecord && info.postStateCommitment, 'B2 settlement closure missing');
      pre.state_commitment = context.lastRecord.pre_state_commitment;
      postRead.snapshot.state_commitment = info.postStateCommitment;
      writeAttempt(context, expected, roles.settle, makeResult(context, expected, settle, pre, postRead.snapshot, info, roles.settle));
    } else {
      const expected = context.scenario.attempts[0];
      const calls = {
        cancel_open: () => common.sendTx(api, 'cancel', api.tx.marketRuntime.cancelOpen(signed.orderId), accounts[roles.seller]),
        expire_open: () => common.sendTx(api, 'expire', api.tx.marketRuntime.expireOpen(signed.orderId), accounts.RELAYER),
        refund_funded: () => common.sendTx(api, 'refund', api.tx.marketRuntime.refundFunded(signed.orderId), accounts.RELAYER),
      };
      const tx = await captureSubmission(context, () => calls[expected.phase]());
      const boundary = await runtimeAttemptSnapshots(context, 'B2', state, tx);
      const pre = boundary.pre.snapshot;
      const post = boundary.post.snapshot;
      const info = runtimeEventInfo(tx, 'marketRuntime');
      const actorRole = expected.phase === 'cancel_open' ? roles.seller : 'RELAYER';
      writeAttempt(context, expected, actorRole, makeResult(context, expected, tx, pre, post, info, actorRole));
    }
    return { api, fixture, startBlock, endBlock: (await api.rpc.chain.getHeader()).number.toNumber() };
  } catch (error) {
    await api.disconnect();
    throw error;
  }
}

async function b1Snapshot(context, state, blockHash = null) {
  const { api, contract, accounts, assetId, rightId, authId, marketId, signed, sellerRole, buyerRole } = state;
  const boundary = await resolveSnapshotBoundary(api, blockHash);
  // ContractPromise requires an initialized connection flag; preserve the historical
  // decorated API's fixed-block calls while supplying that constructor-only marker.
  const historicalApi = Object.create(boundary.queryApi);
  historicalApi.isConnected = true;
  const queryContract = new ContractPromise(
    api, contract.abi, contract.address.toString()
  );
  queryContract.api = historicalApi;
  const order = await b1.contractQuery(queryContract, 'getOrder', accounts.OWNER, [signed.orderId]);
  const balances = await paymentBalances(api, accounts, sellerRole, buyerRole, boundary.blockHash);
  const currentBlock = boundary.blockNumber;
  let all = null;
  if (order) all = await b1.contractQuery(queryContract, 'qAll', accounts.OWNER, [signed.orderId]);
  const metaQuery = all ? field(all, 'meta') : await b1.contractQuery(queryContract, 'qMeta', accounts.OWNER, [assetId]);
  const meta = field(metaQuery, 'meta');
  const rightQuery = all ? field(all, 'right') : (rightId ? await b1.contractQuery(queryContract, 'qRight', accounts.OWNER, [rightId]) : null);
  const right = rightQuery ? field(rightQuery, 'right') : null;
  let authorization;
  if (all) authorization = field(all, 'authorization');
  else if (context.operation === 'TransferMeta') authorization = await b1.contractQuery(queryContract, 'getTransferMetaAuthorization', accounts.OWNER, [authId]);
  else if (context.operation === 'IssueRight') authorization = await b1.contractQuery(queryContract, 'getIssueRightAuthorization', accounts.OWNER, [authId]);
  else authorization = null;
  const market = all ? field(all, 'market') : await b1.contractQuery(queryContract, 'getMarket', accounts.OWNER, [marketId]);
  const orderQuery = all ? field(all, 'order') : null;
  const payment = all ? field(all, 'payment') : null;
  const evidence = all ? field(all, 'evidence') : null;
  const identity = all ? field(all, 'identity') : null;
  const orderValue = orderQuery ? field(orderQuery, 'order') : null;
  const commitment = orderValue ? field(orderValue, 'orderCommitment', 'order_commitment') : signed.order;
  const buyerCommitment = orderValue ? field(orderValue, 'buyerCommitment', 'buyer_commitment') : null;
  const settlement = evidence ? field(evidence, 'settlement') : null;
  const record = normalizedRecord(settlement);
  const validity = right ? field(right, 'validity') : null;
  const rightStatus = right ? enumName(field(right, 'status')) : null;
  const allowedTransferMarkets = right
    ? (field(right, 'allowedTransferMarkets', 'allowed_transfer_markets') || []).map(hex32)
    : [];
  const resaleAuthorized = context.operation === 'TransferRight'
    && Boolean(right)
    && Boolean(field(right, 'transferable'))
    && allowedTransferMarkets.includes(hex32(marketId));
  const snapshot = {
    schema_id: 'assetxchain-exp2-normalized-snapshot', schema_version: 1,
    contract_version: 'phase3-adapter-contract-v1', baseline_id: 'B1', authority_source: 'contract',
    observed_at: { block_number: currentBlock, attempt_sequence: observedAttemptSequence(context) },
    meta: {
      exists: Boolean(meta), asset_id: hex32(assetId), owner: meta ? account(field(meta, 'owner')) : null,
      status: meta ? enumName(field(meta, 'status')) : null,
      state_version: meta ? Number(field(meta, 'stateVersion', 'state_version')) : null,
      owner_index_matches: field(metaQuery, 'ownerIndexMatches', 'owner_index_matches') ?? null,
      active_right_count: Number(field(metaQuery, 'activeRightCount', 'active_right_count') || 0),
      active_order_id: hex32(field(metaQuery, 'activeOrderId', 'active_order_id')),
    },
    right: {
      exists: Boolean(right), right_id: right ? hex32(field(right, 'rightId', 'right_id')) : null,
      holder: right ? account(field(right, 'holder')) : null, status: rightStatus,
      state_version: right ? Number(field(right, 'stateVersion', 'state_version')) : null,
      parent_asset_id: right ? hex32(field(right, 'assetId', 'asset_id')) : null,
      valid: rightQuery ? (field(rightQuery, 'valid') ?? null) : null,
      remaining_uses: right ? Number(field(right, 'remainingUses', 'remaining_uses') ?? field(validity, 'maxUses', 'max_uses')) : null,
      holder_index_matches: rightQuery ? (field(rightQuery, 'holderIndexMatches', 'holder_index_matches') ?? null) : null,
      parent_index_matches: rightQuery ? (field(rightQuery, 'parentIndexMatches', 'parent_index_matches') ?? null) : null,
      active_order_id: rightQuery ? hex32(field(rightQuery, 'activeOrderId', 'active_order_id')) : null,
    },
    order: {
      exists: Boolean(orderValue), order_id: orderValue ? hex32(signed.orderId) : null,
      operation: orderValue ? enumName(field(commitment, 'operation')) : null,
      state: orderValue ? enumName(field(orderValue, 'state')) : null,
      seller: orderValue ? account(field(commitment, 'seller')) : null,
      buyer: buyerCommitment ? account(field(buyerCommitment, 'buyer')) : null,
      expiry: orderValue ? Number(field(commitment, 'expiry')) : null,
      grace_deadline: orderValue && field(orderValue, 'graceDeadline', 'grace_deadline') !== null && field(orderValue, 'graceDeadline', 'grace_deadline') !== undefined
        ? Number(field(orderValue, 'graceDeadline', 'grace_deadline')) : null,
      active_capacity_occupied: orderQuery ? Boolean(field(orderQuery, 'activeCapacityOccupied', 'active_capacity_occupied')) : false,
      funded_capacity_occupied: orderQuery ? Boolean(field(orderQuery, 'fundedCapacityOccupied', 'funded_capacity_occupied')) : false,
    },
    authorization: {
      exists: context.operation === 'TransferRight' ? resaleAuthorized : Boolean(authorization),
      authorization_id: hex32(authId),
      status: context.operation === 'TransferRight'
        ? (resaleAuthorized ? 'Active' : null)
        : enumName(field(authorization, 'status')),
      policy_hash: hex32(record ? record.policy_hash : signed.order.policyHash),
      issued_count: context.operation === 'IssueRight' && authorization ? Number(field(authorization, 'issuedCount', 'issued_count')) : 0,
      reserved_count: context.operation === 'IssueRight' && authorization ? Number(field(authorization, 'reservedCount', 'reserved_count')) : 0,
      issued_use_units: context.operation === 'IssueRight' && authorization ? Number(field(authorization, 'issuedUseUnits', 'issued_use_units')) : 0,
      reserved_use_units: context.operation === 'IssueRight' && authorization ? Number(field(authorization, 'reservedUseUnits', 'reserved_use_units')) : 0,
      consumed: context.operation === 'TransferMeta' && authorization
        ? field(authorization, 'consumedOrderId', 'consumed_order_id') !== null : false,
      consumed_order_id: context.operation === 'TransferMeta' && authorization
        ? hex32(field(authorization, 'consumedOrderId', 'consumed_order_id')) : null,
    },
    payment: {
      state: payment ? enumName(field(payment, 'state')) : 'Unfunded',
      escrow_amount: payment ? decimal(field(payment, 'escrowAmount', 'escrow_amount')) : '0', ...balances,
    },
    evidence: {
      settlement_record_count: record ? 1 : 0,
      settlement_id: record ? record.settlement_id : null,
      record_fields_hash: evidence ? hex32(field(evidence, 'recordFieldsHash', 'record_fields_hash')) : null,
      pre_state_commitment: record ? record.pre_state_commitment : null,
    },
    market: {
      exists: Boolean(market), market_id: hex32(marketId), status: enumName(field(market, 'status')),
      capabilities: market ? capabilities(field(market, 'capabilities')) : [],
      active_orders: market ? Number(field(market, 'activeOrders', 'active_orders')) : 0,
      funded_orders: market ? Number(field(market, 'fundedOrders', 'funded_orders')) : 0,
      active_exposure_value: market ? decimal(field(market, 'activeExposureValue', 'active_exposure_value')) : '0',
    },
    identity: {
      settlement_protocol_version: 1, settlement_domain_id: 'axc/p2/shared',
      order_commitment_hash: record ? record.order_commitment_hash : (identity ? hex32(field(identity, 'orderCommitmentHash', 'order_commitment_hash')) : null),
      seller_nonce: identity ? Number(field(identity, 'sellerNonce', 'seller_nonce')) : 0,
      buyer_nonce: identity && field(identity, 'buyerNonce', 'buyer_nonce') !== null ? Number(field(identity, 'buyerNonce', 'buyer_nonce')) : null,
      authorization_nonce: identity && field(identity, 'authorizationNonce', 'authorization_nonce') !== null ? Number(field(identity, 'authorizationNonce', 'authorization_nonce')) : (context.operation === 'TransferRight' ? null : 0),
      object_state_version: identity && field(identity, 'objectStateVersion', 'object_state_version') !== null ? Number(field(identity, 'objectStateVersion', 'object_state_version')) : (meta ? Number(field(meta, 'stateVersion', 'state_version')) : null),
    },
    state_commitment: ZERO_HASH,
    projection: { availability: 'N/A', authoritative: false, order_state: null, settlement_record_count: null },
  };
  finishSnapshot(snapshot);
  return { snapshot, record };
}

async function b1AttemptSnapshots(context, state, tx) {
  assert(tx.blockHash && tx.parentBlockHash, 'B1 transaction omitted state-boundary hashes');
  const balanceRoles = [
    ['buyer_balance', context.snapshotBuyerRole],
    ['seller_balance', context.snapshotSellerRole],
    ['market_balance', 'MARKET_OK'],
  ].filter(([, role]) => state.accounts && role && state.accounts[role]);
  if (balanceRoles.length > 0) {
    const readBalances = async (blockHash) => Object.fromEntries(
      await Promise.all(balanceRoles.map(async ([fieldName, role]) => [
        fieldName,
        await freeBalance(state.api, state.accounts[role], blockHash),
      ]))
    );
    tx.stateBoundary = {
      actor_role: context.snapshotBuyerRole,
      parent_hash: tx.parentBlockHash,
      included_hash: tx.blockHash,
      parent_balances: await readBalances(tx.parentBlockHash),
      included_balances: await readBalances(tx.blockHash),
    };
  }
  const pre = await b1Snapshot(context, state, tx.parentBlockHash);
  const post = await b1Snapshot(context, state, tx.blockHash);
  if (tx.stateBoundary) {
    Object.assign(pre.snapshot.payment, tx.stateBoundary.parent_balances);
    Object.assign(post.snapshot.payment, tx.stateBoundary.included_balances);
    finishSnapshot(pre.snapshot);
    finishSnapshot(post.snapshot);
  }
  assert(post.snapshot.observed_at.block_number === tx.blockNumber,
    'B1 included-block snapshot number differs from transaction evidence');
  assert(pre.snapshot.observed_at.block_number + 1 === post.snapshot.observed_at.block_number,
    'B1 parent/included snapshot boundary is not adjacent');
  return { pre, post };
}

async function runB1(context) {
  const api = await common.connect(context.args.rpc);
  const accounts = common.sharedAccounts();
  context.accountsByRole = accounts;
  const roles = actorRoles(context.operation);
  const scenarioId = context.scenario.scenario_id;
  const isE2 = context.preflight.experiment_family === 'E2'
    || (context.preflight.experiment_family === 'E7' && ['V1', 'V2'].includes(context.preflight.inputs.e7_variant));
  const isE3 = context.preflight.experiment_family === 'E3';
  const isCompetition = usesCompetitionExecution(context.preflight.experiment_family, scenarioId)
    || (context.preflight.experiment_family === 'E7' && ['V3', 'V4', 'Q1'].includes(context.preflight.inputs.e7_variant));
  const evidence = { transactions: [], events: {}, accounts };
  try {
    await common.fundSharedAccounts(api, accounts, evidence);
    const worktree = context.preflight.baseline.worktree;
    const metadataPath = frozenArtifactRoot(context, worktree, 'market_pure_contract.json');
    const wasmPath = frozenArtifactRoot(context, worktree, 'market_pure_contract.wasm');
    const metadata = readJson(metadataPath);
    const wasm = fs.readFileSync(wasmPath);
    const codeHash = u8aToHex(blake2AsU8a(wasm, 256));
    const code = new CodePromise(api, metadata, wasm);
    const salt = Buffer.from(`p4-e0-${context.preflight.run_id}`.slice(-32), 'utf8');
    const deploy = await common.sendTx(api, 'deploy_b1_contract', code.tx.newWithGenesisHash({
      gasLimit: api.registry.createType('WeightV2', b1.GAS_LIMIT), storageDepositLimit: null, salt,
    }, api.genesisHash.toHex()), accounts.OWNER);
    const instantiated = common.findEvent(deploy.events, 'contracts', 'Instantiated');
    assert(instantiated, 'B1 contracts.Instantiated missing');
    const contractAddress = instantiated.event.data[1].toString();
    const contract = new ContractPromise(api, metadata, contractAddress);
    const registered = await b1.contractCall(api, contract, evidence, 'register_market', 'registerMarket', accounts.OWNER, [
      accounts.MARKET_OK.address, contractAddress, codeHash, 1, 31,
      scenarioId === 'NEG-FEE-01' ? 1 : 0,
      { tierId: 1, maxActiveOrders: 32, maxFundedOrders: 16, maxActiveExposureValue: (1000n * common.PRICES.TransferMeta).toString() }, '0',
    ]);
    const marketId = b1.hash32Hex(registered.dry.decoded.value, 'register_market');
    let authorizationWitnessMarketId = null;
    if (scenarioId === 'NEG-AUTH-01') {
      const witness = await b1.contractCall(
        api, contract, evidence, 'register_authorization_witness_market', 'registerMarket',
        accounts.OWNER, [
          accounts.MARKET_DENIED.address, contractAddress, codeHash, 1, 31, 0,
          { tierId: 2, maxActiveOrders: 32, maxFundedOrders: 16, maxActiveExposureValue: (1000n * common.PRICES.TransferMeta).toString() }, '0',
        ]
      );
      authorizationWitnessMarketId = b1.hash32Hex(
        witness.dry.decoded.value, 'authorization witness market'
      );
    }
    const assetByte = { TransferMeta: 0x61, IssueRight: 0x62, TransferRight: 0x63 }[context.operation];
    const assetId = b1.fixedHash(assetByte);
    await b1.contractCall(api, contract, evidence, 'register_meta', 'registerMeta', accounts.OWNER, [{
      assetId, owner: accounts.OWNER.address, registrationProvider: accounts.OWNER.address,
      serviceProvider: accounts.OWNER.address, metadataDigest: b1.fixedHash(0x70),
      dataCommitment: b1.fixedHash(0x71), validationProgramHash: b1.fixedHash(0x72),
      status: 'Tradable', parentAssetId: null, supersedesAssetId: null,
    }]);
    const fixtureAnchor = await deterministicFixtureAnchor(api);
    const validUntil = fixtureAnchor + (context.preflight.experiment_family === 'E7'
      && context.preflight.inputs.e7_variant === 'V1' && context.operation === 'TransferRight'
      ? 2 : common.TIMING.rightValidity);
    let rightId = null;
    let authId;
    let signed;
    let secondSigned = null;
    let secondRightId = null;
    if (context.operation === 'TransferRight') {
      rightId = b1.fixedHash(0x73);
      const allowedTransferMarkets = [authorizationWitnessMarketId || marketId];
      const rightStatus = context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'V1'
        ? 'Active' : {
        'NEG-RIGHT-EXPIRED-01': 'Expired',
        'NEG-RIGHT-EXHAUSTED-01': 'Exhausted',
        'NEG-RIGHT-REVOKED-01': 'Revoked',
      }[scenarioId] || 'Active';
      const rightInput = {
        rightId, assetId, holder: accounts.RIGHT_SELLER.address, issuer: accounts.OWNER.address,
        rightType: 'View', dataScope: 'FullAsset', validity: { validFrom: fixtureAnchor, validUntil, maxUses: 1 },
        transferable: true, allowedTransferMarkets, royaltyBps: 0,
      };
      await b1.contractCall(
        api, contract, evidence, 'initialize_resale_right',
        isE2 ? 'initializeRightForExperiment' : 'initializeExistingRight', accounts.OWNER,
        isE2
          ? [rightInput, rightStatus, scenarioId === 'NEG-RIGHT-EXHAUSTED-01' ? 0 : 1]
          : [rightInput]
      );
      const expiry = await b1.nextExpiry(api);
      authId = common.computeResaleAuthorizationId(rightId, marketId);
      const parameters = b1.hash32Hex(
        await b1.contractQuery(contract, 'existingRightParametersHash', accounts.OWNER, [rightId]),
        'resale parameters'
      );
      signed = b1.buildSignedOrder(contract, api.genesisHash.toHex(), accounts.RIGHT_SELLER, {
        marketId, operation: 'TransferRight', objectType: 'RightToken', objectId: rightId,
        parentAssetId: assetId, authorizationId: authId, price: common.PRICES.TransferRight,
        policyHash: b1.pricePolicyHash(common.PRICES.TransferRight), rightParametersHash: parameters, expiry,
      });
    } else if (context.operation === 'TransferMeta') {
      const expiry = await b1.nextExpiry(api, 1);
      const allowedBuyer = scenarioId === 'RACE-BUYER-01' ? null : accounts.BUYER_1.address;
      const auth = await b1.contractCall(api, contract, evidence, 'create_meta_authorization', 'createTransferMetaAuthorization', accounts.OWNER, [
        assetId, marketId, {
          pricePolicy: b1.pricePolicy(common.PRICES.TransferMeta), allowedBuyer, expiry, nonce: 0,
        },
      ]);
      authId = b1.hash32Hex(auth.dry.decoded.value, 'meta authorization');
      signed = b1.buildSignedOrder(contract, api.genesisHash.toHex(), accounts.OWNER, {
        marketId, operation: 'TransferMeta', objectType: 'MetaToken', objectId: assetId,
        authorizationId: authId,
        allowedBuyer: allowedBuyer ? common.accountHex(accounts.BUYER_1) : null,
        price: common.PRICES.TransferMeta, policyHash: b1.pricePolicyHash(common.PRICES.TransferMeta), expiry,
      });
      if (scenarioId === 'NEG-META-RIGHT-01') {
        rightId = b1.fixedHash(0x74);
        await b1.contractCall(
          api, contract, evidence, 'initialize_conflicting_right',
          'initializeRightForExperiment', accounts.OWNER, [{
            rightId, assetId, holder: accounts.RIGHT_SELLER.address, issuer: accounts.OWNER.address,
            rightType: 'View', dataScope: 'FullAsset',
            validity: { validFrom: fixtureAnchor, validUntil, maxUses: 1 }, transferable: true,
            allowedTransferMarkets: [marketId], royaltyBps: 0,
          }, 'Active', 1]
        );
      }
    } else {
      const expiry = await b1.nextExpiry(api, 1);
      const issueCapacity = scenarioId === 'RACE-QUOTA-01' ? 1 : 2;
      const auth = await b1.contractCall(api, contract, evidence, 'create_issue_authorization', 'createIssueRightAuthorization', accounts.OWNER, [
        assetId, marketId, {
          rightType: 'View', dataScope: 'FullAsset', validityTemplate: { validFrom: fixtureAnchor, validUntil, maxUses: 1 },
          maxIssueCount: issueCapacity, maxTotalUseUnits: issueCapacity,
          pricePolicy: b1.pricePolicy(common.PRICES.IssueRight),
          royaltyBps: 0, transferable: true, allowedTransferMarkets: [marketId], expiry, nonce: 0,
        },
      ]);
      authId = b1.hash32Hex(auth.dry.decoded.value, 'issue authorization');
      const parameters = b1.hash32Hex(await b1.contractQuery(contract, 'issueRightParametersHash', accounts.OWNER, [authId]), 'issue parameters');
      signed = b1.buildSignedOrder(contract, api.genesisHash.toHex(), accounts.OWNER, {
        marketId, operation: 'IssueRight', objectType: 'MetaToken', objectId: assetId,
        authorizationId: authId, price: common.PRICES.IssueRight,
        policyHash: b1.pricePolicyHash(common.PRICES.IssueRight), rightParametersHash: parameters, expiry,
      });
      rightId = common.computeRightId(api.genesisHash.toHex(), signed.orderId);
    }
    if (isCompetition && ['RACE-QUOTA-01', 'RACE-OBJECT-01'].includes(scenarioId)) {
      const seller = context.operation === 'TransferRight' ? accounts.RIGHT_SELLER : accounts.OWNER;
      secondSigned = b1.buildSignedOrder(
        contract, api.genesisHash.toHex(), seller, rebuildInput(signed, { orderNonce: 1 })
      );
      if (context.operation === 'IssueRight') {
        secondRightId = common.computeRightId(api.genesisHash.toHex(), secondSigned.orderId);
      }
    }
    if (isE2) {
      if (scenarioId === 'NEG-PROTO-01') {
        signed = b1.buildSignedOrder(
          contract, api.genesisHash.toHex(), accounts.OWNER,
          rebuildInput(signed, { settlementProtocolVersion: 2 })
        );
      } else if (scenarioId === 'NEG-SIGN-01') {
        if (context.preflight.experiment_family === 'E7') {
          if (context.operation === 'TransferMeta') signed.order.allowedBuyer = common.accountHex(accounts.BUYER_2);
          else signed.order.rightParametersHash = b1.fixedHash(0x9a);
        } else signed.signature = corruptSignature(signed.signature);
      } else if (scenarioId === 'NEG-OWNER-01') {
        signed = b1.buildSignedOrder(
          contract, api.genesisHash.toHex(), accounts.MARKET_DENIED, rebuildInput(signed)
        );
      } else if (scenarioId === 'NEG-HOLDER-01') {
        signed = b1.buildSignedOrder(
          contract, api.genesisHash.toHex(), accounts.BUYER_1, rebuildInput(signed)
        );
      } else if (scenarioId === 'NEG-PARENT-01') {
        signed = b1.buildSignedOrder(
          contract, api.genesisHash.toHex(), accounts.RIGHT_SELLER,
          rebuildInput(signed, { parentAssetId: b1.fixedHash(0xec) })
        );
      } else if (scenarioId === 'NEG-RIGHT-MISSING-01') {
        rightId = b1.fixedHash(0xeb);
        authId = common.computeResaleAuthorizationId(rightId, marketId);
        signed = b1.buildSignedOrder(
          contract, api.genesisHash.toHex(), accounts.RIGHT_SELLER, rebuildInput(signed, {
            objectId: rightId, authorizationId: authId, rightParametersHash: b1.fixedHash(0xea),
          })
        );
      } else if (scenarioId === 'NEG-VERSION-01') {
        signed = b1.buildSignedOrder(
          contract, api.genesisHash.toHex(), accounts.OWNER,
          rebuildInput(signed, { sellerStateVersion: 2 })
        );
      } else if (scenarioId === 'NEG-NONCE-01') {
        await b1.contractCall(
          api, contract, evidence, 'consume_nonce_fixture', 'createOrder', accounts.RELAYER, [
            signed.order, 'Authorization', signed.sellerCommitment, signed.signature,
          ]
        );
      } else if (scenarioId === 'NEG-PRICE-01') {
        signed = b1.buildSignedOrder(
          contract, api.genesisHash.toHex(), accounts.OWNER,
          rebuildInput(signed, { price: common.PRICES.IssueRight - 1n })
        );
      }
      if (scenarioId === 'NEG-MARKET-01') {
        await b1.contractCall(
          api, contract, evidence, 'remove_market_fixture',
          'configureMarketForExperiment', accounts.OWNER, [marketId, false, null, null]
        );
      } else if (scenarioId === 'NEG-CAPABILITY-01') {
        await b1.contractCall(
          api, contract, evidence, 'limit_market_fixture',
          'configureMarketForExperiment', accounts.OWNER, [marketId, true, 29, null]
        );
      } else if (scenarioId === 'NEG-MARKET-RESTRICTED-01') {
        await b1.contractCall(
          api, contract, evidence, 'restrict_market_fixture', 'restrictMarket', accounts.OWNER, [marketId]
        );
      }
    }
    const payerRole = scenarioId === 'NEG-PAYER-01'
      ? (roles.fund === 'BUYER_1' ? 'BUYER_2' : 'BUYER_1')
      : roles.fund;
    const state = { api, contract, accounts, assetId, rightId, authId, marketId, signed, sellerRole: roles.seller, buyerRole: roles.fund };
    context.snapshotSellerRole = roles.seller;
    context.snapshotBuyerRole = roles.fund;
    const startBlock = (await api.rpc.chain.getHeader()).number.toNumber();
    const createSignedOrder = (targetSigned, label, expectedError = null) => b1.contractCall(
      api, contract, evidence, label, 'createOrder', accounts[roles.create], [
        targetSigned.order,
        context.operation === 'TransferRight' ? { SellerSnapshot: b1.pricePolicy(OPERATION_PRICE[context.operation]) } : 'Authorization',
        targetSigned.sellerCommitment, targetSigned.signature,
      ], expectedError ? { expectedError } : {}
    );
    const createOrder = (label) => createSignedOrder(signed, label);
    const fundSignedOrder = (
      targetSigned, buyerRole, nonce, label, expectedError = null
    ) => {
      const targetBuyer = accounts[buyerRole];
      const targetBuyerSigned = b1.buildSignedBuyer(
        contract, api.genesisHash.toHex(), targetBuyer, targetSigned, nonce
      );
      return b1.contractCall(
        api, contract, evidence, label, 'fundOrder', targetBuyer,
        [targetSigned.orderId, targetBuyerSigned.commitment, targetBuyerSigned.signature], {
          value: targetSigned.order.price,
          ...(expectedError ? { expectedError } : {}),
        }
      );
    };
    const buyer = accounts[roles.fund];
    const buyerSigned = b1.buildSignedBuyer(contract, api.genesisHash.toHex(), buyer, signed, 0);
    const fundOrder = (label) => fundSignedOrder(signed, roles.fund, 0, label);
    if (!isE2 && !isE3 && context.scenario.attempts.length === 1) {
      await createOrder('fixture_create');
      if (context.scenario.scenario_id === 'LEG-REFUND-01') await fundOrder('fixture_fund');
      if (context.scenario.scenario_id === 'LEG-EXPIRE-01') await waitUntilAfterBlock(api, signed.order.expiry);
      if (context.scenario.scenario_id === 'LEG-REFUND-01') {
        const funded = (await b1Snapshot(context, state)).snapshot;
        await waitUntilAfterBlock(api, funded.order.grace_deadline);
      }
    }
    if (isE2 && ['fund_order', 'settle', 'refund_funded'].includes(context.scenario.attempts[0].phase)) {
      await createOrder('fixture_create');
      if (context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'V1'
        && context.operation === 'TransferRight') {
        await waitUntilAfterBlock(api, validUntil);
        const observedBlock = (await api.rpc.chain.getHeader()).number.toNumber();
        writeJsonExclusive(path.join(context.outputRoot, 'stale-intent.json'), {
          variant: 'V1', lifecycle_transition: 'right-expired-by-block',
          right_id: rightId, signed_order_id: signed.orderId,
          cached_valid_until: validUntil, observed_block: observedBlock,
          stale_condition_observed: observedBlock > validUntil,
        });
      }
      if (scenarioId === 'NEG-AUTH-REVOKED-01') {
        await b1.contractCall(
          api, contract, evidence, 'revoke_authorization_fixture',
          context.operation === 'TransferMeta'
            ? 'revokeTransferMetaAuthorization' : 'revokeIssueRightAuthorization', accounts.OWNER, [authId]
        );
      }
      if (scenarioId === 'NEG-REFUND-EARLY-01') await fundOrder('fixture_fund');
      if (scenarioId === 'NEG-CAPACITY-01') {
        await b1.contractCall(
          api, contract, evidence, 'fill_capacity_fixture',
          'configureMarketForExperiment', accounts.OWNER, [marketId, true, null, 0]
        );
      }
    }
    if (isE3) {
      await createOrder('fixture_create');
      await fundOrder('fixture_fund');
      if (scenarioId === 'CLEAN-LATE-META-01') {
        await b1.contractCall(
          api, contract, evidence, 'apply_late_failure_delta',
          'configureMetaStatusForExperiment', accounts.OWNER, [assetId, 'Retiring']
        );
      } else {
        await b1.contractCall(
          api, contract, evidence, 'apply_late_failure_delta',
          'configureRightStatusForExperiment', accounts.OWNER, [rightId, 'Expired']
        );
      }
    }
    if (isCompetition) {
      if (['DUP-SETTLE-01', 'RACE-RELAYER-01'].includes(scenarioId)) {
        await createOrder('fixture_create');
        await fundOrder('fixture_fund');
      } else if (scenarioId === 'DUP-REFUND-01') {
        await createOrder('fixture_create');
        await fundOrder('fixture_fund');
        const funded = (await b1Snapshot(context, state)).snapshot;
        await waitUntilAfterBlock(api, funded.order.grace_deadline);
      } else if (scenarioId === 'RACE-BUYER-01') {
        await createOrder('fixture_create');
      } else if (scenarioId === 'RACE-QUOTA-01') {
        await createOrder('fixture_create_first');
        await createSignedOrder(secondSigned, 'fixture_create_second');
      }
      if (context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'V4') {
        const field = context.operation === 'TransferMeta' ? 'allowedBuyer' : 'objectId';
        const tampered = { ...signed, order: { ...signed.order } };
        if (field === 'allowedBuyer') tampered.order.allowedBuyer = common.accountHex(accounts.BUYER_2);
        else tampered.order.objectId = b1.fixedHash(0xe7);
        const probe = await b1.contractCall(
          api, contract, evidence, 'e7_composite_field_probe', 'createOrder', accounts[roles.create], [
            tampered.order, context.operation === 'TransferRight'
              ? { SellerSnapshot: b1.pricePolicy(OPERATION_PRICE[context.operation]) } : 'Authorization',
            tampered.sellerCommitment, tampered.signature,
          ], { allowFailure: true }
        );
        writeJsonExclusive(path.join(context.outputRoot, 'composite-probe.json'), {
          variant: 'V4', field_replaced: field, signature_retained: tampered.signature === signed.signature,
          original_value: signed.order[field], mutated_value: tampered.order[field],
          dispatch_error: probe.submitted.dispatchError, included_block: probe.submitted.blockNumber,
          extrinsic_index: phaseIndex(probe.submitted),
        });
      }
    }
    const initial = (await b1Snapshot(context, state)).snapshot;
    const fixture = {
      schema_id: 'assetxchain-exp2-phase4-e0-fixture', schema_version: 1,
      run_id: context.preflight.run_id, baseline_id: 'B1', scenario_id: context.scenario.scenario_id,
      repetition: context.repetition, genesis_hash: api.genesisHash.toHex(), fixture_anchor_block: fixtureAnchor,
      runtime_version: `${api.runtimeVersion.specName}-${api.runtimeVersion.specVersion}`,
      metadata_hash: u8aToHex(blake2AsU8a(api.runtimeMetadata.toU8a(), 256)),
      shared_input_hash: common.SHARED_INPUT_HASH,
      contract_address: contractAddress, contract_code_hash: codeHash,
      asset_id: assetId, right_id: rightId, authorization_id: authId, market_id: marketId,
      order_id: signed.orderId,
      competing_order_id: secondSigned ? secondSigned.orderId : null,
      competing_right_id: secondRightId,
      initial_state: initial,
    };
    writeJsonExclusive(path.join(context.outputRoot, 'fixture.json'), fixture);

    if (isE2 || isE3) {
      const expected = context.scenario.attempts[0];
      const expectedNative = isE2
        ? (context.preflight.experiment_family === 'E7' && context.preflight.inputs.e7_variant === 'V2'
          ? 'MalformedCommitment' : E2_NATIVE_ERROR.B1[scenarioId])
        : E3_NATIVE_ERROR.B1[scenarioId];
      let actorRole = roles.create;
      let call;
      if (expected.phase === 'create_order') {
        call = await captureSubmission(context, () => b1.contractCall(
          api, contract, evidence, 'reject_create', 'createOrder', accounts[roles.create], [
            signed.order,
            context.operation === 'TransferRight'
              ? { SellerSnapshot: b1.pricePolicy(OPERATION_PRICE[context.operation]) }
              : 'Authorization',
            signed.sellerCommitment, signed.signature,
          ], {
            expectedError: expectedNative,
            allowResultFailure: context.preflight.experiment_family === 'E7'
              && context.preflight.inputs.e7_variant === 'V1',
          }
        ));
      } else if (expected.phase === 'fund_order') {
        actorRole = payerRole;
        call = await captureSubmission(context, () => b1.contractCall(
          api, contract, evidence, 'reject_fund', 'fundOrder', accounts[payerRole],
          [signed.orderId, buyerSigned.commitment, buyerSigned.signature], {
            value: scenarioId === 'NEG-BALANCE-01'
              ? (BigInt(signed.order.price) - 1n).toString()
              : signed.order.price,
            expectedError: expectedNative,
          }
        ));
      } else if (expected.phase === 'settle') {
        actorRole = roles.settle;
        call = await captureSubmission(context, () => b1.contractCall(
          api, contract, evidence, 'reject_settle', 'settle', accounts[actorRole],
          [signed.orderId], { expectedError: expectedNative }
        ));
      } else if (expected.phase === 'refund_funded') {
        actorRole = roles.settle;
        call = await captureSubmission(context, () => b1.contractCall(
          api, contract, evidence, 'reject_refund', 'refundFunded', accounts[actorRole],
          [signed.orderId], { expectedError: expectedNative }
        ));
      } else {
        throw new Error(`B1 has no E2 execution path for ${expected.phase}`);
      }
      const boundary = await b1AttemptSnapshots(context, state, call.submitted);
      const pre = boundary.pre.snapshot;
      const post = boundary.post.snapshot;
      const rejection = b1Rejection(call);
      assert(
        rejection.nativeError === expectedNative,
        `B1 native error priority drift for ${scenarioId}`
      );
      writeAttempt(
        context, expected, actorRole, makeRejectedResult(
          context, expected, call.submitted, pre, post, rejectionEventInfo(call.submitted),
          actorRole, rejection, field(call.dry.gasConsumed, 'refTime', 'ref_time'), false
        )
      );
    } else if (isCompetition) {
      if (context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'Q1') {
        const candidates = [
          { signed, rightId, buyerRole: 'BUYER_1', label: 'e7_quota_candidate_1' },
          { signed: secondSigned, rightId: secondRightId, buyerRole: 'BUYER_2', label: 'e7_quota_candidate_2' },
        ];
        const overlap = candidates.map((candidate) => {
          const startedNs = process.hrtime.bigint();
          const buyerTx = b1.buildSignedBuyer(contract, api.genesisHash.toHex(), accounts[candidate.buyerRole], candidate.signed, 0);
          return captureSubmission(context, () => b1.contractCall(
            api, contract, evidence, candidate.label, 'fundOrder', accounts[candidate.buyerRole],
            [candidate.signed.orderId, buyerTx.commitment, buyerTx.signature],
            { value: candidate.signed.order.price, allowFailure: true }
          )).then((call) => ({ ...candidate, startedNs: startedNs.toString(), finishedNs: process.hrtime.bigint().toString(), call }));
        });
        const submitted = await Promise.all(overlap);
        writeJsonExclusive(path.join(context.outputRoot, 'e7-concurrency.json'), {
          variant: 'Q1', concurrency: 2, purpose: 'last-quota-overlapping-fund-candidates',
          submissions: submitted.map(({ buyerRole, startedNs, finishedNs, call }) => ({
            actor: buyerRole, started_ns: startedNs, finished_ns: finishedNs,
            submission_sequence: call.submitted.submissionSequence, tx_hash: call.submitted.txHash,
            included_block: call.submitted.blockNumber, extrinsic_index: phaseIndex(call.submitted), dispatch_error: call.submitted.dispatchError,
          })),
          overlap_observed: submitted.map((x) => BigInt(x.startedNs)).reduce((a, b) => a > b ? a : b)
            < submitted.map((x) => BigInt(x.finishedNs)).reduce((a, b) => a < b ? a : b),
        });
        let winner = null;
        for (const [position, item] of [...submitted].sort((a, b) => itemBlock(a.call.submitted) - itemBlock(b.call.submitted)
          || phaseIndex(a.call.submitted) - phaseIndex(b.call.submitted)).entries()) {
          const expected = context.scenario.attempts[position];
          const observedState = { ...state, signed: item.signed, rightId: item.rightId, buyerRole: item.buyerRole };
          context.snapshotBuyerRole = item.buyerRole;
          const boundary = await b1AttemptSnapshots(context, observedState, item.call.submitted);
          if (!item.call.submitted.dispatchError) {
            winner = item;
            context.lastRecord = null;
            const info = contractEventInfo(item.call.submitted, contract);
            writeAttempt(context, expected, item.buyerRole,
              makeResult(context, expected, item.call.submitted, boundary.pre.snapshot, boundary.post.snapshot,
                info, item.buyerRole, field(item.call.dry.gasConsumed, 'refTime', 'ref_time'), false));
          } else {
            const rejection = { nativeError: 'ConcurrentRaceLoser', commonError: 'QuotaExceeded' };
            writeAttempt(context, expected, item.buyerRole,
              makeRejectedResult(context, expected, item.call.submitted, boundary.post.snapshot, boundary.post.snapshot,
                rejectionEventInfo(item.call.submitted), item.buyerRole, rejection,
                field(item.call.dry.gasConsumed, 'refTime', 'ref_time'), false, rejection.commonError, true));
          }
        }
        assert(winner, 'E7 Q1 B1 did not produce a quota winner');
        const settleExpected = context.scenario.attempts[2];
        const settleState = { ...state, signed: winner.signed, rightId: winner.rightId, buyerRole: winner.buyerRole };
        context.snapshotBuyerRole = winner.buyerRole;
        const settleCall = await captureSubmission(context, () => b1.contractCall(
          api, contract, evidence, 'e7_quota_settle_winner', 'settle', accounts.RELAYER, [winner.signed.orderId]
        ));
        const settleBoundary = await b1AttemptSnapshots(context, settleState, settleCall.submitted);
        const settleInfo = contractEventInfo(settleCall.submitted, contract);
        context.lastRecord = settleBoundary.post.record;
        assert(context.lastRecord && settleInfo.postStateCommitment, 'E7 Q1 B1 settlement closure missing');
        settleBoundary.pre.snapshot.state_commitment = context.lastRecord.pre_state_commitment;
        settleBoundary.post.snapshot.state_commitment = settleInfo.postStateCommitment;
        writeAttempt(context, settleExpected, 'RELAYER', makeResult(
          context, settleExpected, settleCall.submitted, settleBoundary.pre.snapshot, settleBoundary.post.snapshot,
          settleInfo, 'RELAYER', field(settleCall.dry.gasConsumed, 'refTime', 'ref_time'), false
        ));
      } else if (context.preflight.experiment_family === 'E7'
        && ['V3', 'V4'].includes(context.preflight.inputs.e7_variant)) {
        const actors = ['RELAYER', 'OWNER', 'RIGHT_SELLER', 'MARKET_OK'];
        const overlap = actors.map((actor, index) => {
          const startedNs = process.hrtime.bigint();
          return captureSubmission(context, () => b1.contractCall(
            api, contract, evidence, `e7_overlap_${index + 1}`, 'settle', accounts[actor], [signed.orderId],
            { allowFailure: true }
          )).then((call) => ({ actor, startedNs: startedNs.toString(), finishedNs: process.hrtime.bigint().toString(), call }));
        });
        const submitted = await Promise.all(overlap);
        writeJsonExclusive(path.join(context.outputRoot, 'e7-concurrency.json'), {
          variant: context.preflight.inputs.e7_variant, concurrency: actors.length,
          submissions: submitted.map(({ actor, startedNs, finishedNs, call }) => ({
            actor, started_ns: startedNs, finished_ns: finishedNs,
            submission_sequence: call.submitted.submissionSequence, tx_hash: call.submitted.txHash,
            included_block: call.submitted.blockNumber, extrinsic_index: phaseIndex(call.submitted),
            dispatch_error: call.submitted.dispatchError,
          })),
          overlap_observed: submitted.map((x) => BigInt(x.startedNs)).reduce((a, b) => a > b ? a : b)
            < submitted.map((x) => BigInt(x.finishedNs)).reduce((a, b) => a < b ? a : b),
        });
        for (const [position, item] of [...submitted].sort((a, b) => itemBlock(a.call.submitted) - itemBlock(b.call.submitted)
          || phaseIndex(a.call.submitted) - phaseIndex(b.call.submitted)).entries()) {
          const tx = item.call.submitted;
          const expected = context.scenario.attempts[position];
          const observedState = { ...state, buyerRole: roles.fund };
          context.snapshotBuyerRole = roles.fund;
          const boundary = await b1Snapshot(context, observedState, tx.parentBlockHash);
          const post = await b1Snapshot(context, observedState, tx.blockHash);
          if (!tx.dispatchError) {
            context.lastRecord = post.record;
            const info = contractEventInfo(tx, contract);
            boundary.snapshot.state_commitment = context.lastRecord.pre_state_commitment;
            post.snapshot.state_commitment = info.postStateCommitment;
            writeAttempt(context, expected, item.actor, makeResult(context, expected, tx, boundary.snapshot, post.snapshot, info, item.actor, field(item.call.dry.gasConsumed, 'refTime', 'ref_time')));
          } else {
            const rejection = { nativeError: 'ConcurrentRaceLoser', commonError: 'AlreadySettled' };
            writeAttempt(context, expected, item.actor,
              makeRejectedResult(context, expected, tx, post.snapshot, post.snapshot, rejectionEventInfo(tx), item.actor,
                rejection, field(item.call.dry.gasConsumed, 'refTime', 'ref_time'), true, rejection.commonError, true));
          }
        }
      }
      if (!(context.preflight.experiment_family === 'E7'
        && ['V3', 'V4', 'Q1'].includes(context.preflight.inputs.e7_variant))) {
      const expectedRaceError = context.scenario.cardinality
        ? context.scenario.cardinality.rejected_error_code
        : null;
      const attemptState = (
        targetSigned = signed, targetRightId = rightId, buyerRole = roles.fund
      ) => {
        context.snapshotBuyerRole = buyerRole;
        return { ...state, signed: targetSigned, rightId: targetRightId, buyerRole };
      };
      const writeAccepted = async (
        expected, actorRole, submit, observedState, settlement = false
      ) => {
        const call = await captureSubmission(context, submit);
        const boundary = await b1AttemptSnapshots(context, observedState, call.submitted);
        const pre = boundary.pre.snapshot;
        const postRead = boundary.post;
        const info = contractEventInfo(call.submitted, contract);
        if (settlement) {
          context.lastRecord = postRead.record;
          assert(context.lastRecord && info.postStateCommitment, 'B1 E4 settlement closure missing');
          pre.state_commitment = context.lastRecord.pre_state_commitment;
          postRead.snapshot.state_commitment = info.postStateCommitment;
        } else {
          context.lastRecord = null;
        }
        writeAttempt(
          context, expected, actorRole,
          makeResult(
            context, expected, call.submitted, pre, postRead.snapshot, info, actorRole,
            field(call.dry.gasConsumed, 'refTime', 'ref_time'), false
          )
        );
      };
      const writeRejected = async (expected, actorRole, submit, observedState) => {
        const expectedNative = E4_NATIVE_ERROR.B1[scenarioId];
        const call = await captureSubmission(context, submit);
        const boundary = await b1AttemptSnapshots(context, observedState, call.submitted);
        const pre = boundary.pre.snapshot;
        const post = boundary.post.snapshot;
        const rejection = b1Rejection(call);
        assert(
          rejection.nativeError === expectedNative,
          `B1 E4 native error drift for ${scenarioId}`
        );
        writeAttempt(
          context, expected, actorRole,
          makeRejectedResult(
            context, expected, call.submitted, pre, post,
            rejectionEventInfo(call.submitted), actorRole, rejection,
            field(call.dry.gasConsumed, 'refTime', 'ref_time'), false,
            expected.expected_error_code || expectedRaceError
          )
        );
      };
      if (scenarioId === 'DUP-CREATE-01') {
        await writeAccepted(
          context.scenario.attempts[0], roles.create,
          () => createSignedOrder(signed, 'create_first'), attemptState()
        );
        await writeRejected(
          context.scenario.attempts[1], roles.create,
          () => createSignedOrder(
            signed, 'create_duplicate', E4_NATIVE_ERROR.B1[scenarioId]
          ),
          attemptState()
        );
      } else if (scenarioId === 'RACE-OBJECT-01') {
        await writeAccepted(
          context.scenario.attempts[0], roles.create,
          () => createSignedOrder(signed, 'create_first'), attemptState()
        );
        await writeRejected(
          context.scenario.attempts[1], roles.create,
          () => createSignedOrder(
            secondSigned, 'create_competing', E4_NATIVE_ERROR.B1[scenarioId]
          ),
          attemptState(secondSigned, rightId)
        );
      } else if (scenarioId === 'RACE-BUYER-01') {
        await writeAccepted(
          context.scenario.attempts[0], 'BUYER_1',
          () => fundSignedOrder(signed, 'BUYER_1', 0, 'fund_buyer_1'),
          attemptState(signed, rightId, 'BUYER_1')
        );
        await writeRejected(
          context.scenario.attempts[1], 'BUYER_2',
          () => fundSignedOrder(
            signed, 'BUYER_2', 0, 'fund_buyer_2', E4_NATIVE_ERROR.B1[scenarioId]
          ),
          attemptState(signed, rightId, 'BUYER_2')
        );
      } else if (scenarioId === 'RACE-QUOTA-01') {
        await writeAccepted(
          context.scenario.attempts[0], 'BUYER_1',
          () => fundSignedOrder(signed, 'BUYER_1', 0, 'fund_first_order'),
          attemptState(signed, rightId, 'BUYER_1')
        );
        await writeRejected(
          context.scenario.attempts[1], 'BUYER_2',
          () => fundSignedOrder(
            secondSigned, 'BUYER_2', 0, 'fund_last_quota',
            E4_NATIVE_ERROR.B1[scenarioId]
          ),
          attemptState(secondSigned, secondRightId, 'BUYER_2')
        );
        await writeAccepted(
          context.scenario.attempts[2], 'RELAYER',
          () => b1.contractCall(
            api, contract, evidence, 'settle_quota_winner', 'settle',
            accounts.RELAYER, [signed.orderId]
          ),
          attemptState(signed, rightId, 'BUYER_1'), true
        );
      } else if (scenarioId === 'DUP-REFUND-01') {
        await writeAccepted(
          context.scenario.attempts[0], 'RELAYER',
          () => b1.contractCall(
            api, contract, evidence, 'refund_first', 'refundFunded',
            accounts.RELAYER, [signed.orderId]
          ),
          attemptState()
        );
        await writeRejected(
          context.scenario.attempts[1], 'OWNER',
          () => b1.contractCall(
            api, contract, evidence, 'refund_duplicate', 'refundFunded',
            accounts.OWNER, [signed.orderId], {
              expectedError: E4_NATIVE_ERROR.B1[scenarioId],
            }
          ),
          attemptState()
        );
      } else {
        const relayers = scenarioId === 'RACE-RELAYER-01'
          ? ['RELAYER', 'OWNER', 'RIGHT_SELLER', 'MARKET_OK']
          : ['RELAYER', 'OWNER'];
        for (let index = 0; index < context.scenario.attempts.length; index += 1) {
          const expected = context.scenario.attempts[index];
          const actorRole = relayers[index];
          if (index === 0) {
            await writeAccepted(
              expected, actorRole,
              () => b1.contractCall(
                api, contract, evidence, `settle_${index + 1}`, 'settle',
                accounts[actorRole], [signed.orderId]
              ),
              attemptState(), true
            );
          } else {
            await writeRejected(
              expected, actorRole,
              () => b1.contractCall(
                api, contract, evidence, `settle_${index + 1}`, 'settle',
                accounts[actorRole], [signed.orderId], {
                  expectedError: E4_NATIVE_ERROR.B1[scenarioId],
                }
              ),
              attemptState()
            );
          }
        }
      }
      }
    } else if (context.scenario.attempts.length === 3) {
      let expected = context.scenario.attempts[0];
      const create = await captureSubmission(context, () => createOrder('create'));
      let boundary = await b1AttemptSnapshots(context, state, create.submitted);
      let pre = boundary.pre.snapshot;
      let post = boundary.post.snapshot;
      let info = contractEventInfo(create.submitted, contract);
      writeAttempt(context, expected, roles.create, makeResult(context, expected, create.submitted, pre, post, info, roles.create, field(create.dry.gasConsumed, 'refTime', 'ref_time')));

      expected = context.scenario.attempts[1];
      const fund = await captureSubmission(context, () => fundOrder('fund'));
      boundary = await b1AttemptSnapshots(context, state, fund.submitted);
      pre = boundary.pre.snapshot;
      post = boundary.post.snapshot;
      info = contractEventInfo(fund.submitted, contract);
      writeAttempt(context, expected, roles.fund, makeResult(context, expected, fund.submitted, pre, post, info, roles.fund, field(fund.dry.gasConsumed, 'refTime', 'ref_time')));

      expected = context.scenario.attempts[2];
      const settle = await captureSubmission(context, () => b1.contractCall(
        api, contract, evidence, 'settle', 'settle', accounts.RELAYER, [signed.orderId]
      ));
      boundary = await b1AttemptSnapshots(context, state, settle.submitted);
      pre = boundary.pre.snapshot;
      const postRead = boundary.post;
      context.lastRecord = postRead.record;
      info = contractEventInfo(settle.submitted, contract);
      assert(context.lastRecord && info.postStateCommitment, 'B1 settlement closure missing');
      pre.state_commitment = context.lastRecord.pre_state_commitment;
      postRead.snapshot.state_commitment = info.postStateCommitment;
      writeAttempt(context, expected, roles.settle, makeResult(context, expected, settle.submitted, pre, postRead.snapshot, info, roles.settle, field(settle.dry.gasConsumed, 'refTime', 'ref_time')));
    } else {
      const expected = context.scenario.attempts[0];
      const lifecycle = {
        cancel_open: { label: 'cancel', message: 'cancelOpen', actorRole: roles.seller },
        expire_open: { label: 'expire', message: 'expireOpen', actorRole: 'RELAYER' },
        refund_funded: { label: 'refund', message: 'refundFunded', actorRole: 'RELAYER' },
      }[expected.phase];
      const tx = await captureSubmission(context, () => b1.contractCall(
        api, contract, evidence, lifecycle.label, lifecycle.message, accounts[lifecycle.actorRole], [signed.orderId]
      ));
      const boundary = await b1AttemptSnapshots(context, state, tx.submitted);
      const pre = boundary.pre.snapshot;
      const post = boundary.post.snapshot;
      const info = contractEventInfo(tx.submitted, contract);
      writeAttempt(
        context, expected, lifecycle.actorRole,
        makeResult(context, expected, tx.submitted, pre, post, info, lifecycle.actorRole, field(tx.dry.gasConsumed, 'refTime', 'ref_time'))
      );
    }
    return { api, fixture, startBlock, endBlock: (await api.rpc.chain.getHeader()).number.toNumber() };
  } catch (error) {
    await api.disconnect();
    throw error;
  }
}

async function deployG3(context, api, accounts) {
  const worktree = context.preflight.baseline.worktree;
  const artifactRoot = context.args.artifact_root || path.join(worktree, 'target/ink/market_orderbook');
  const wasmPath = path.join(artifactRoot, 'market_orderbook.wasm');
  const metadataPath = path.join(artifactRoot, 'market_orderbook.json');
  const wasm = fs.readFileSync(wasmPath);
  const wasmHex = u8aToHex(wasm);
  const codeHash = u8aToHex(blake2AsU8a(wasm, 256));
  const salt = u8aToHex(blake2AsU8a(stringToU8a(`p4-e0-${context.preflight.run_id}`), 128));
  const placeholderData = g3.contractData(api, 'new', [[null, g3.fixed32(g3.ZERO_HASH)], [null, g3.le(common.TIMING.fundedGrace, 4)]]);
  const predicted = await api.call.contractsApi.instantiate(accounts.MARKET_OK.address, '0', g3.GAS_LIMIT, null, { Upload: wasmHex }, placeholderData, salt);
  const predictedJson = predicted.toJSON();
  assert(predictedJson.result && predictedJson.result.ok, 'G3 contract prediction failed');
  const deploy = await g3.sendTx(api, 'deploy_market_orderbook_v2', api.tx.contracts.instantiateWithCode('0', g3.GAS_LIMIT, null, wasmHex, placeholderData, salt), accounts.MARKET_OK);
  const instantiated = g3.findEvent(deploy.events, 'contracts', 'Instantiated');
  assert(instantiated, 'G3 contracts.Instantiated missing');
  const contractAddress = instantiated.event.data[1].toString();
  assert(contractAddress === predictedJson.result.ok.accountId, 'G3 predicted address mismatch');
  const marketNonce = BigInt((await api.query.markets.nextMarketNonce()).toString());
  const marketId = g3.computeMarketId(accounts.MARKET_OK, contractAddress, marketNonce);
  const bind = await g3.callRelay(api, contractAddress, accounts.MARKET_OK, 'bind_market_id', g3.contractData(api, 'bind_market_id', [[null, g3.fixed32(marketId)]]), { ok: true });
  const tier = api.tx.markets.setCapacityTier({
    tierId: 1, requiredStake: (10_000n * common.UNIT).toString(), maxActiveOrders: 32,
    maxFundedOrders: 16, maxActiveExposureValue: (1_000n * common.UNIT).toString(),
  });
  await g3.sendTx(api, 'set_market_capacity_tier', api.tx.sudo.sudo(tier), accounts.OWNER);
  await g3.sendTx(
    api, 'register_market_v2',
    api.tx.markets.registerMarket(
      contractAddress, codeHash, 31, 1,
      context.scenario.scenario_id === 'NEG-FEE-01' ? 1 : 0
    ),
    accounts.MARKET_OK
  );
  const metadata = readJson(metadataPath);
  const contract = new ContractPromise(api, metadata, contractAddress);
  return { contractAddress, contract, marketId, codeHash, wasmPath, metadataPath, bind };
}

async function registerG3AuthorizationWitnessMarket(context, api, accounts, evidence) {
  const worktree = context.preflight.baseline.worktree;
  const wasmPath = frozenArtifactRoot(context, worktree, 'market_orderbook.wasm');
  const wasm = fs.readFileSync(wasmPath);
  const wasmHex = u8aToHex(wasm);
  const codeHash = u8aToHex(blake2AsU8a(wasm, 256));
  const salt = u8aToHex(blake2AsU8a(
    stringToU8a(`p4-e2-auth-witness-${context.preflight.run_id}`), 128
  ));
  const placeholderData = g3.contractData(api, 'new', [
    [null, g3.fixed32(g3.ZERO_HASH)], [null, g3.le(common.TIMING.fundedGrace, 4)],
  ]);
  const predicted = await api.call.contractsApi.instantiate(
    accounts.MARKET_DENIED.address, '0', g3.GAS_LIMIT, null,
    { Upload: wasmHex }, placeholderData, salt
  );
  const predictedJson = predicted.toJSON();
  assert(predictedJson.result && predictedJson.result.ok, 'G3 witness contract prediction failed');
  const deploy = await g3.sendTx(
    api, 'deploy_authorization_witness_market',
    api.tx.contracts.instantiateWithCode(
      '0', g3.GAS_LIMIT, null, wasmHex, placeholderData, salt
    ), accounts.MARKET_DENIED
  );
  const instantiated = g3.findEvent(deploy.events, 'contracts', 'Instantiated');
  assert(instantiated, 'G3 witness contracts.Instantiated missing');
  const contractAddress = instantiated.event.data[1].toString();
  assert(
    contractAddress === predictedJson.result.ok.accountId,
    'G3 witness predicted address mismatch'
  );
  const marketNonce = BigInt((await api.query.markets.nextMarketNonce()).toString());
  const marketId = g3.computeMarketId(accounts.MARKET_DENIED, contractAddress, marketNonce);
  await g3.callRelay(
    api, contractAddress, accounts.MARKET_DENIED, 'bind_authorization_witness_market',
    g3.contractData(api, 'bind_market_id', [[null, g3.fixed32(marketId)]]), { ok: true }
  );
  const registered = await g3.sendTx(
    api, 'register_authorization_witness_market',
    api.tx.markets.registerMarket(contractAddress, codeHash, 31, 1, 0),
    accounts.MARKET_DENIED
  );
  evidence.transactions.push({
    label: 'register_authorization_witness_market',
    txHash: registered.txHash, blockHash: registered.blockHash,
    blockNumber: registered.blockNumber,
  });
  return marketId;
}

async function runG3(context) {
  const api = await common.connect(context.args.rpc);
  const accounts = common.sharedAccounts();
  context.accountsByRole = accounts;
  const roles = actorRoles(context.operation);
  const scenarioId = context.scenario.scenario_id;
  const isE2 = context.preflight.experiment_family === 'E2'
    || (context.preflight.experiment_family === 'E7' && ['V1', 'V2'].includes(context.preflight.inputs.e7_variant));
  const isE3 = context.preflight.experiment_family === 'E3';
  const isCompetition = usesCompetitionExecution(context.preflight.experiment_family, scenarioId)
    || (context.preflight.experiment_family === 'E7' && ['V3', 'V4', 'Q1'].includes(context.preflight.inputs.e7_variant));
  const evidence = { transactions: [], events: {}, accounts: {
    ownerPair: accounts.OWNER, rightSellerPair: accounts.RIGHT_SELLER,
    buyer1Pair: accounts.BUYER_1, buyer2Pair: accounts.BUYER_2,
    marketPair: accounts.MARKET_OK, relayerPair: accounts.RELAYER,
  } };
  try {
    await common.fundSharedAccounts(api, accounts, evidence);
    const deployed = await deployG3(context, api, accounts);
    const { contractAddress, marketId, codeHash } = deployed;
    const authorizationWitnessMarketId = scenarioId === 'NEG-AUTH-01'
      ? await registerG3AuthorizationWitnessMarket(context, api, accounts, evidence)
      : null;
    const fixtureAnchor = await deterministicFixtureAnchor(api);
    const validUntil = fixtureAnchor + (context.preflight.experiment_family === 'E7'
      && context.preflight.inputs.e7_variant === 'V1' && context.operation === 'TransferRight'
      ? 2 : common.TIMING.rightValidity);
    const pricePolicy = {
      referencePrice: OPERATION_PRICE[context.operation].toString(), floorRatioBps: 10000,
      signedMarketFeeCapBps: 0, safetyFeeBps: 0,
    };
    const assetLabel = { TransferMeta: 'meta', IssueRight: 'issue', TransferRight: 'resale' }[context.operation];
    const assetId = await g3.registerAsset(api, accounts.OWNER, assetLabel, {
      TransferMeta: 0x41, IssueRight: 0x42, TransferRight: 0x43,
    }[context.operation], evidence);
    let rightId = null;
    let authId;
    let signed;
    let secondSigned = null;
    let secondRightId = null;
    if (context.operation === 'TransferRight') {
      let prepParameters;
      if (isE2) {
        rightId = g3.hash(stringToU8a(`e2-right-${scenarioId}`));
        const allowedTransferMarkets = [authorizationWitnessMarketId || marketId];
      const rightStatus = context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'V1'
        ? 'Active' : {
          'NEG-RIGHT-EXPIRED-01': 'Expired',
          'NEG-RIGHT-EXHAUSTED-01': 'Exhausted',
          'NEG-RIGHT-REVOKED-01': 'Revoked',
      }[scenarioId] || 'Active';
        const right = g3ExperimentRight(
          accounts, assetId, rightId, allowedTransferMarkets, fixtureAnchor, validUntil,
          rightStatus, scenarioId === 'NEG-RIGHT-EXHAUSTED-01' ? 0 : 1
        );
        await common.sendTx(
          api, 'initialize_resale_right',
          api.tx.sudo.sudo(api.tx.dataAssets.initializeRightForExperiment(right)), accounts.OWNER
        );
        prepParameters = g3.computeRightParametersHash(marketId, fixtureAnchor, validUntil, 1, 0);
      } else {
        const prepExpiry = (await api.rpc.chain.getHeader()).number.toNumber() + common.TIMING.orderExpiry + 2;
        const issuePolicy = { referencePrice: common.PRICES.IssueRight.toString(), floorRatioBps: 10000, signedMarketFeeCapBps: 0, safetyFeeBps: 0 };
        const prepAuth = await g3.createAuthorization(api, accounts.OWNER, 'issue', assetId, marketId, {
          rightType: 'View', dataScope: 'FullAsset', validityTemplate: { validFrom: fixtureAnchor, validUntil, maxUses: 1 },
          maxIssueCount: 1, maxTotalUseUnits: 1, pricePolicy: issuePolicy, royaltyBps: 0,
          transferable: true, allowedTransferMarkets: [marketId], expiry: prepExpiry, nonce: 0,
        }, evidence);
        prepParameters = g3.computeRightParametersHash(marketId, fixtureAnchor, validUntil, 1, 0);
        const prepSigned = g3.buildSignedOrder(api, api.genesisHash.toHex(), accounts.OWNER, {
          marketId, operation: 'IssueRight', objectType: 'MetaToken', objectId: assetId,
          authorizationId: prepAuth, price: common.PRICES.IssueRight,
          policyHash: g3.computePolicyHash(api, issuePolicy), rightParametersHash: prepParameters,
          sellerStateVersion: 1, orderNonce: 0, expiry: prepExpiry,
        });
        const prepCreate = await g3.callRelay(api, contractAddress, accounts.OWNER, 'fixture_create', g3.createOrderCall(api, prepSigned, 'Authorization'), { ok: true });
        const prepBuyer = g3.buildSignedBuyer(api, api.genesisHash.toHex(), accounts.RIGHT_SELLER, 'IssueRight', prepSigned.orderId, common.PRICES.IssueRight, prepExpiry, 0);
        await g3.callRelay(api, contractAddress, accounts.RIGHT_SELLER, 'fixture_fund', g3.fundOrderCall(api, prepSigned.orderId, prepBuyer), { ok: true });
        await g3.callRelay(api, contractAddress, accounts.RELAYER, 'fixture_settle', g3.simpleHashCall(api, 'relay_settle', prepSigned.orderId), { ok: true });
        assert(prepCreate.decoded.payload === prepSigned.orderId, 'G3 fixture order mismatch');
        rightId = g3.computeRightId(api.genesisHash.toHex(), 'IssueRight', prepSigned.orderId);
      }
      authId = g3.hash(stringToU8a('axc/p2/resale-policy'), g3.fixed32(rightId), g3.fixed32(marketId));
      const expiry = (await api.rpc.chain.getHeader()).number.toNumber() + common.TIMING.orderExpiry + 1;
      signed = g3.buildSignedOrder(api, api.genesisHash.toHex(), accounts.RIGHT_SELLER, {
        marketId, operation: 'TransferRight', objectType: 'RightToken', objectId: rightId,
        parentAssetId: assetId, authorizationId: authId, price: common.PRICES.TransferRight,
        policyHash: g3.computePolicyHash(api, pricePolicy), rightParametersHash: prepParameters,
        sellerStateVersion: 1, orderNonce: 0, expiry,
      });
    } else if (context.operation === 'TransferMeta') {
      const expiry = (await api.rpc.chain.getHeader()).number.toNumber() + common.TIMING.orderExpiry + 2;
      const allowedBuyer = scenarioId === 'RACE-BUYER-01'
        ? null
        : accounts.BUYER_1.address;
      authId = await g3.createAuthorization(api, accounts.OWNER, 'meta', assetId, marketId, {
        pricePolicy, allowedBuyer, expiry, nonce: 0,
      }, evidence);
      signed = g3.buildSignedOrder(api, api.genesisHash.toHex(), accounts.OWNER, {
        marketId, operation: 'TransferMeta', objectType: 'MetaToken', objectId: assetId,
        authorizationId: authId,
        allowedBuyer: allowedBuyer ? g3.accountHex(accounts.BUYER_1) : null,
        price: common.PRICES.TransferMeta,
        policyHash: g3.computePolicyHash(api, pricePolicy), sellerStateVersion: 1, orderNonce: 0, expiry,
      });
      if (scenarioId === 'NEG-META-RIGHT-01') {
        rightId = g3.hash(stringToU8a('e2-conflicting-meta-right'));
        await common.sendTx(
          api, 'initialize_conflicting_right', api.tx.sudo.sudo(
            api.tx.dataAssets.initializeRightForExperiment(g3ExperimentRight(
              accounts, assetId, rightId, [marketId], fixtureAnchor, validUntil
            ))
          ), accounts.OWNER
        );
      }
    } else {
      const expiry = (await api.rpc.chain.getHeader()).number.toNumber() + common.TIMING.orderExpiry + 2;
      const issueCapacity = scenarioId === 'RACE-QUOTA-01' ? 1 : 2;
      authId = await g3.createAuthorization(api, accounts.OWNER, 'issue', assetId, marketId, {
        rightType: 'View', dataScope: 'FullAsset', validityTemplate: { validFrom: fixtureAnchor, validUntil, maxUses: 1 },
        maxIssueCount: issueCapacity, maxTotalUseUnits: issueCapacity,
        pricePolicy, royaltyBps: 0,
        transferable: true, allowedTransferMarkets: [marketId], expiry, nonce: 0,
      }, evidence);
      const parameters = g3.computeRightParametersHash(marketId, fixtureAnchor, validUntil, 1, 0);
      signed = g3.buildSignedOrder(api, api.genesisHash.toHex(), accounts.OWNER, {
        marketId, operation: 'IssueRight', objectType: 'MetaToken', objectId: assetId,
        authorizationId: authId, price: common.PRICES.IssueRight,
        policyHash: g3.computePolicyHash(api, pricePolicy), rightParametersHash: parameters,
        sellerStateVersion: 1, orderNonce: 0, expiry,
      });
      rightId = g3.computeRightId(api.genesisHash.toHex(), 'IssueRight', signed.orderId);
    }
    if (isCompetition && ['RACE-QUOTA-01', 'RACE-OBJECT-01'].includes(scenarioId)) {
      const seller = context.operation === 'TransferRight'
        ? accounts.RIGHT_SELLER
        : accounts.OWNER;
      secondSigned = g3.buildSignedOrder(
        api, api.genesisHash.toHex(), seller, rebuildInput(signed, { orderNonce: 1 })
      );
      if (context.operation === 'IssueRight') {
        secondRightId = g3.computeRightId(
          api.genesisHash.toHex(), 'IssueRight', secondSigned.orderId
        );
      }
    }
    if (isE2) {
      if (scenarioId === 'NEG-PROTO-01') {
        signed = g3.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.OWNER,
          rebuildInput(signed, { settlementProtocolVersion: 2 })
        );
      } else if (scenarioId === 'NEG-SIGN-01') {
        if (context.preflight.experiment_family === 'E7') {
          if (context.operation === 'TransferMeta') signed.order.allowedBuyer = g3.accountHex(accounts.BUYER_2);
          else signed.order.rightParametersHash = g3.hash(Buffer.from('e7-field-replacement'));
        } else signed.sellerSignature = corruptSignature(signed.sellerSignature);
      } else if (scenarioId === 'NEG-OWNER-01') {
        signed = g3.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.MARKET_DENIED, rebuildInput(signed)
        );
      } else if (scenarioId === 'NEG-HOLDER-01') {
        signed = g3.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.BUYER_1, rebuildInput(signed)
        );
      } else if (scenarioId === 'NEG-PARENT-01') {
        signed = g3.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.RIGHT_SELLER,
          rebuildInput(signed, { parentAssetId: g3.hash(stringToU8a('e2-wrong-parent')) })
        );
      } else if (scenarioId === 'NEG-RIGHT-MISSING-01') {
        rightId = g3.hash(stringToU8a('e2-missing-right'));
        authId = g3.hash(
          stringToU8a('axc/p2/resale-policy'), g3.fixed32(rightId), g3.fixed32(marketId)
        );
        signed = g3.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.RIGHT_SELLER, rebuildInput(signed, {
            objectId: rightId, authorizationId: authId,
            rightParametersHash: g3.hash(stringToU8a('e2-missing-right-parameters')),
          })
        );
      } else if (scenarioId === 'NEG-VERSION-01') {
        signed = g3.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.OWNER,
          rebuildInput(signed, { sellerStateVersion: 2 })
        );
      } else if (scenarioId === 'NEG-NONCE-01') {
        await g3.callRelay(
          api, contractAddress, accounts.RELAYER, 'consume_nonce_fixture',
          g3.createOrderCall(api, signed, 'Authorization'), { ok: true }
        );
      } else if (scenarioId === 'NEG-PRICE-01') {
        signed = g3.buildSignedOrder(
          api, api.genesisHash.toHex(), accounts.OWNER,
          rebuildInput(signed, { price: common.PRICES.IssueRight - 1n })
        );
      }
      if (scenarioId === 'NEG-MARKET-01') {
        await common.sendTx(
          api, 'remove_market_fixture', api.tx.sudo.sudo(
            api.tx.markets.configureMarketForExperiment(marketId, false, null, null)
          ), accounts.OWNER
        );
      } else if (scenarioId === 'NEG-CAPABILITY-01') {
        await common.sendTx(
          api, 'limit_market_fixture', api.tx.sudo.sudo(
            api.tx.markets.configureMarketForExperiment(marketId, true, 29, null)
          ), accounts.OWNER
        );
      } else if (scenarioId === 'NEG-MARKET-RESTRICTED-01') {
        await common.sendTx(
          api, 'restrict_market_fixture',
          api.tx.sudo.sudo(api.tx.markets.restrictMarket(marketId)), accounts.OWNER
        );
      }
    }
    const payerRole = scenarioId === 'NEG-PAYER-01'
      ? (roles.fund === 'BUYER_1' ? 'BUYER_2' : 'BUYER_1')
      : roles.fund;
    const state = { api, accounts, assetId, rightId, authId, marketId, signed, sellerRole: roles.seller, buyerRole: roles.fund };
    context.snapshotSellerRole = roles.seller;
    context.snapshotBuyerRole = roles.fund;
    const startBlock = (await api.rpc.chain.getHeader()).number.toNumber();
    const createSignedOrder = (
      targetSigned, label, expectation = { ok: true }
    ) => g3.callRelay(
      api, contractAddress, accounts[roles.create], label,
      g3.createOrderCall(
        api, targetSigned,
        context.operation === 'TransferRight'
          ? { SellerSnapshot: pricePolicy }
          : 'Authorization'
      ),
      expectation
    );
    const createOrder = (label) => createSignedOrder(signed, label);
    const fundSignedOrder = (
      targetSigned, buyerRole, nonce, label, expectation = { ok: true }
    ) => {
      const targetBuyer = accounts[buyerRole];
      const targetBuyerSigned = g3.buildSignedBuyer(
        api, api.genesisHash.toHex(), targetBuyer, context.operation,
        targetSigned.orderId, OPERATION_PRICE[context.operation],
        targetSigned.order.expiry, nonce
      );
      return g3.callRelay(
        api, contractAddress, targetBuyer, label,
        g3.fundOrderCall(api, targetSigned.orderId, targetBuyerSigned), expectation
      );
    };
    const buyer = accounts[roles.fund];
    const buyerSigned = g3.buildSignedBuyer(
      api, api.genesisHash.toHex(), buyer, context.operation, signed.orderId,
      OPERATION_PRICE[context.operation], signed.order.expiry, 0
    );
    const fundOrder = (label) => fundSignedOrder(signed, roles.fund, 0, label);
    if (!isE2 && !isE3 && context.scenario.attempts.length === 1) {
      await createOrder('fixture_create');
      if (context.scenario.scenario_id === 'LEG-REFUND-01') await fundOrder('fixture_fund');
      if (context.scenario.scenario_id === 'LEG-EXPIRE-01') await waitUntilAfterBlock(api, signed.order.expiry);
      if (context.scenario.scenario_id === 'LEG-REFUND-01') {
        const funded = (await runtimeSnapshot(context, 'G3', state)).snapshot;
        await waitUntilAfterBlock(api, funded.order.grace_deadline);
      }
    }
    if (isE2 && ['fund_order', 'settle', 'refund_funded'].includes(context.scenario.attempts[0].phase)) {
      await createOrder('fixture_create');
      if (context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'V1'
        && context.operation === 'TransferRight') {
        await waitUntilAfterBlock(api, validUntil);
        const observedBlock = (await api.rpc.chain.getHeader()).number.toNumber();
        writeJsonExclusive(path.join(context.outputRoot, 'stale-intent.json'), {
          variant: 'V1', lifecycle_transition: 'right-expired-by-block',
          right_id: rightId, signed_order_id: signed.orderId,
          cached_valid_until: validUntil, observed_block: observedBlock,
          stale_condition_observed: observedBlock > validUntil,
        });
      }
      if (scenarioId === 'NEG-AUTH-REVOKED-01') {
        await common.sendTx(
          api, 'revoke_authorization_fixture',
          (context.operation === 'TransferMeta'
            ? api.tx.dataAssets.revokeTransferMetaAuthorization(authId)
            : api.tx.dataAssets.revokeIssueRightAuthorization(authId)), accounts.OWNER
        );
      }
      if (scenarioId === 'NEG-REFUND-EARLY-01') await fundOrder('fixture_fund');
      if (scenarioId === 'NEG-CAPACITY-01') {
        await common.sendTx(
          api, 'fill_capacity_fixture', api.tx.sudo.sudo(
            api.tx.markets.configureMarketForExperiment(marketId, true, null, 0)
          ), accounts.OWNER
        );
      }
      if (scenarioId === 'NEG-BALANCE-01') {
        await common.sendTx(
          api, 'limit_buyer_balance_fixture', api.tx.sudo.sudo(
            api.tx.balances.forceSetBalance(
              accounts[roles.fund].address,
              (OPERATION_PRICE[context.operation] / 2n).toString()
            )
          ), accounts.OWNER
        );
      }
    }
    if (isE3) {
      await createOrder('fixture_create');
      await fundOrder('fixture_fund');
      const statusCall = scenarioId === 'CLEAN-LATE-META-01'
        ? api.tx.dataAssets.configureMetaStatusForExperiment(assetId, 'Retiring')
        : api.tx.dataAssets.configureRightStatusForExperiment(assetId, rightId, 'Expired');
      await common.sendTx(
        api, 'apply_late_failure_delta', api.tx.sudo.sudo(statusCall), accounts.OWNER
      );
    }
    if (isCompetition) {
      if (['DUP-SETTLE-01', 'RACE-RELAYER-01'].includes(scenarioId)) {
        await createOrder('fixture_create');
        await fundOrder('fixture_fund');
      } else if (scenarioId === 'DUP-REFUND-01') {
        await createOrder('fixture_create');
        await fundOrder('fixture_fund');
        const funded = (await runtimeSnapshot(context, 'G3', state)).snapshot;
        await waitUntilAfterBlock(api, funded.order.grace_deadline);
      } else if (scenarioId === 'RACE-BUYER-01') {
        await createOrder('fixture_create');
      } else if (scenarioId === 'RACE-QUOTA-01') {
        await createOrder('fixture_create_first');
        await createSignedOrder(secondSigned, 'fixture_create_second');
      }
      if (context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'V4') {
        const field = context.operation === 'TransferMeta' ? 'allowedBuyer' : 'objectId';
        const tampered = { ...signed, order: { ...signed.order } };
        if (field === 'allowedBuyer') tampered.order.allowedBuyer = g3.accountHex(accounts.BUYER_2);
        else tampered.order.objectId = g3.hash(stringToU8a('e7-field-probe'));
        const probe = await g3.callRelay(
          api, contractAddress, accounts[roles.create], 'e7_composite_field_probe',
          g3.createOrderCall(api, tampered, context.operation === 'TransferRight'
            ? { SellerSnapshot: pricePolicy } : 'Authorization'), { ok: true, allowFailure: true }
        );
        writeJsonExclusive(path.join(context.outputRoot, 'composite-probe.json'), {
          variant: 'V4', field_replaced: field, signature_retained: tampered.sellerSignature === signed.sellerSignature,
          original_value: signed.order[field], mutated_value: tampered.order[field],
          dispatch_error: probe.submitted.dispatchError, included_block: probe.submitted.blockNumber,
          extrinsic_index: phaseIndex(probe.submitted),
        });
      }
    }
    const initial = (await runtimeSnapshot(context, 'G3', state)).snapshot;
    const fixture = {
      schema_id: 'assetxchain-exp2-phase4-e0-fixture', schema_version: 1,
      run_id: context.preflight.run_id, baseline_id: 'G3', scenario_id: context.scenario.scenario_id,
      repetition: context.repetition, genesis_hash: api.genesisHash.toHex(), fixture_anchor_block: fixtureAnchor,
      runtime_version: `${api.runtimeVersion.specName}-${api.runtimeVersion.specVersion}`,
      metadata_hash: u8aToHex(blake2AsU8a(api.runtimeMetadata.toU8a(), 256)),
      shared_input_hash: common.SHARED_INPUT_HASH,
      contract_address: contractAddress, contract_code_hash: codeHash,
      asset_id: assetId, right_id: rightId, authorization_id: authId, market_id: marketId,
      order_id: signed.orderId,
      competing_order_id: secondSigned ? secondSigned.orderId : null,
      competing_right_id: secondRightId,
      initial_state: initial,
    };
    writeJsonExclusive(path.join(context.outputRoot, 'fixture.json'), fixture);

    if (isE2 || isE3) {
      const expected = context.scenario.attempts[0];
      const expectedStatus = G3_COMMON_STATUS[expected.expected_error_code];
      assert(expectedStatus, `G3 status mapping is missing ${expected.expected_error_code}`);
      let actorRole = roles.create;
      let relay;
      if (expected.phase === 'create_order') {
        relay = await captureSubmission(context, () => g3.callRelay(
          api, contractAddress, accounts[actorRole], 'reject_create',
          g3.createOrderCall(
            api, signed,
            context.operation === 'TransferRight' ? { SellerSnapshot: pricePolicy } : 'Authorization'
          ),
          { ok: false, code: expectedStatus }
        ));
      } else if (expected.phase === 'fund_order') {
        actorRole = payerRole;
        relay = await captureSubmission(context, () => g3.callRelay(
          api, contractAddress, accounts[actorRole], 'reject_fund',
          g3.fundOrderCall(api, signed.orderId, buyerSigned),
          { ok: false, code: expectedStatus }
        ));
      } else if (expected.phase === 'settle') {
        actorRole = roles.settle;
        relay = await captureSubmission(context, () => g3.callRelay(
          api, contractAddress, accounts[actorRole], 'reject_settle',
          g3.simpleHashCall(api, 'relay_settle', signed.orderId),
          { ok: false, code: expectedStatus }
        ));
      } else if (expected.phase === 'refund_funded') {
        actorRole = roles.settle;
        relay = await captureSubmission(context, () => g3.callRelay(
          api, contractAddress, accounts[actorRole], 'reject_refund',
          g3.simpleHashCall(api, 'relay_refund_funded', signed.orderId),
          { ok: false, code: expectedStatus }
        ));
      } else {
        throw new Error(`G3 has no E2 execution path for ${expected.phase}`);
      }
      const boundary = await runtimeAttemptSnapshots(context, 'G3', state, relay.submitted);
      const pre = boundary.pre.snapshot;
      const post = boundary.post.snapshot;
      const rejection = g3Rejection(relay);
      writeAttempt(
        context, expected, actorRole, makeRejectedResult(
          context, expected, relay.submitted, pre, post,
          rejectionEventInfo(relay.submitted), actorRole, rejection,
          field(relay.dryRun.gasConsumed, 'refTime', 'ref_time'), true
        )
      );
    } else if (isCompetition) {
      if (context.preflight.experiment_family === 'E7'
        && context.preflight.inputs.e7_variant === 'Q1') {
        const candidates = [
          { signed, rightId, buyerRole: 'BUYER_1', label: 'e7_quota_candidate_1' },
          { signed: secondSigned, rightId: secondRightId, buyerRole: 'BUYER_2', label: 'e7_quota_candidate_2' },
        ];
        const overlap = candidates.map((candidate) => {
          const startedNs = process.hrtime.bigint();
          const buyerTx = g3.buildSignedBuyer(
            api, api.genesisHash.toHex(), accounts[candidate.buyerRole], context.operation,
            candidate.signed.orderId, OPERATION_PRICE[context.operation], candidate.signed.order.expiry, 0
          );
          return captureSubmission(context, () => g3.callRelay(
            api, contractAddress, accounts[candidate.buyerRole], candidate.label,
            g3.fundOrderCall(api, candidate.signed.orderId, buyerTx), { ok: true, allowFailure: true }
          )).then((relay) => ({ ...candidate, startedNs: startedNs.toString(), finishedNs: process.hrtime.bigint().toString(), relay }));
        });
        const submitted = await Promise.all(overlap);
        writeJsonExclusive(path.join(context.outputRoot, 'e7-concurrency.json'), {
          variant: 'Q1', concurrency: 2, purpose: 'last-quota-overlapping-fund-candidates',
          submissions: submitted.map(({ buyerRole, startedNs, finishedNs, relay }) => ({
            actor: buyerRole, started_ns: startedNs, finished_ns: finishedNs,
            submission_sequence: relay.submitted.submissionSequence, tx_hash: relay.submitted.txHash,
            included_block: relay.submitted.blockNumber, extrinsic_index: phaseIndex(relay.submitted), dispatch_error: relay.submitted.dispatchError,
          })),
          overlap_observed: submitted.map((x) => BigInt(x.startedNs)).reduce((a, b) => a > b ? a : b)
            < submitted.map((x) => BigInt(x.finishedNs)).reduce((a, b) => a < b ? a : b),
        });
        let winner = null;
        for (const [position, item] of [...submitted].sort((a, b) => itemBlock(a.relay.submitted) - itemBlock(b.relay.submitted)
          || phaseIndex(a.relay.submitted) - phaseIndex(b.relay.submitted)).entries()) {
          const expected = context.scenario.attempts[position];
          context.snapshotBuyerRole = item.buyerRole;
          const observedState = { ...state, signed: item.signed, rightId: item.rightId, buyerRole: item.buyerRole };
          const boundary = await runtimeAttemptSnapshots(context, 'G3', observedState, item.relay.submitted);
          if (!item.relay.submitted.dispatchError) {
            winner = item;
            context.lastRecord = null;
            const info = runtimeEventInfo(item.relay.submitted, 'dataAssets');
            writeAttempt(context, expected, item.buyerRole,
              makeResult(context, expected, item.relay.submitted, boundary.pre.snapshot, boundary.post.snapshot,
                info, item.buyerRole, field(item.relay.dryRun.gasConsumed, 'refTime', 'ref_time')));
          } else {
            const rejection = { nativeError: 'ConcurrentRaceLoser', commonError: 'QuotaExceeded' };
            writeAttempt(context, expected, item.buyerRole,
              makeRejectedResult(context, expected, item.relay.submitted, boundary.post.snapshot, boundary.post.snapshot,
                rejectionEventInfo(item.relay.submitted), item.buyerRole, rejection,
                field(item.relay.dryRun.gasConsumed, 'refTime', 'ref_time'), true, rejection.commonError, true));
          }
        }
        assert(winner, 'E7 Q1 G3 did not produce a quota winner');
        const settleExpected = context.scenario.attempts[2];
        const settleState = { ...state, signed: winner.signed, rightId: winner.rightId, buyerRole: winner.buyerRole };
        context.snapshotBuyerRole = winner.buyerRole;
        const settleRelay = await captureSubmission(context, () => g3.callRelay(
          api, contractAddress, accounts.RELAYER, 'e7_quota_settle_winner',
          g3.simpleHashCall(api, 'relay_settle', winner.signed.orderId), { ok: true }
        ));
        const settleBoundary = await runtimeAttemptSnapshots(context, 'G3', settleState, settleRelay.submitted);
        const settleInfo = runtimeEventInfo(settleRelay.submitted, 'dataAssets');
        context.lastRecord = settleBoundary.post.record;
        assert(context.lastRecord && settleInfo.postStateCommitment, 'E7 Q1 G3 settlement closure missing');
        settleBoundary.pre.snapshot.state_commitment = context.lastRecord.pre_state_commitment;
        settleBoundary.post.snapshot.state_commitment = settleInfo.postStateCommitment;
        writeAttempt(context, settleExpected, 'RELAYER', makeResult(
          context, settleExpected, settleRelay.submitted, settleBoundary.pre.snapshot, settleBoundary.post.snapshot,
          settleInfo, 'RELAYER', field(settleRelay.dryRun.gasConsumed, 'refTime', 'ref_time')
        ));
      } else if (context.preflight.experiment_family === 'E7'
        && ['V3', 'V4'].includes(context.preflight.inputs.e7_variant)) {
        const actors = ['RELAYER', 'OWNER', 'RIGHT_SELLER', 'MARKET_OK'];
        const overlap = actors.map((actor, index) => {
          const startedNs = process.hrtime.bigint();
          return captureSubmission(context, () => g3.callRelay(
            api, contractAddress, accounts[actor], `e7_overlap_${index + 1}`,
            g3.simpleHashCall(api, 'relay_settle', signed.orderId),
            { ok: true, allowFailure: true }
          )).then((relay) => ({ actor, startedNs: startedNs.toString(), finishedNs: process.hrtime.bigint().toString(), relay }));
        });
        const submitted = await Promise.all(overlap);
        writeJsonExclusive(path.join(context.outputRoot, 'e7-concurrency.json'), {
          variant: context.preflight.inputs.e7_variant, concurrency: actors.length,
          submissions: submitted.map(({ actor, startedNs, finishedNs, relay }) => ({
            actor, started_ns: startedNs, finished_ns: finishedNs,
            submission_sequence: relay.submitted.submissionSequence, tx_hash: relay.submitted.txHash,
            included_block: relay.submitted.blockNumber, extrinsic_index: phaseIndex(relay.submitted),
            dispatch_error: relay.submitted.dispatchError,
          })),
          overlap_observed: submitted.map((x) => BigInt(x.startedNs)).reduce((a, b) => a > b ? a : b)
            < submitted.map((x) => BigInt(x.finishedNs)).reduce((a, b) => a < b ? a : b),
        });
        for (const [position, item] of [...submitted].sort((a, b) => itemBlock(a.relay.submitted) - itemBlock(b.relay.submitted)
          || phaseIndex(a.relay.submitted) - phaseIndex(b.relay.submitted)).entries()) {
          const relay = item.relay;
          const tx = relay.submitted;
          const expected = context.scenario.attempts[position];
          context.snapshotBuyerRole = roles.fund;
          const boundary = await runtimeAttemptSnapshots(context, 'G3', state, tx);
          const pre = boundary.pre.snapshot;
          const post = boundary.post.snapshot;
          if (!tx.dispatchError) {
            context.lastRecord = boundary.post.record;
            const info = runtimeEventInfo(tx, 'dataAssets');
            pre.state_commitment = context.lastRecord.pre_state_commitment;
            post.state_commitment = info.postStateCommitment;
            writeAttempt(context, expected, item.actor, makeResult(context, expected, tx, pre, post, info, item.actor, field(relay.dryRun.gasConsumed, 'refTime', 'ref_time')));
          } else {
            const rejection = { nativeError: 'ConcurrentRaceLoser', commonError: 'AlreadySettled' };
            writeAttempt(context, expected, item.actor,
              makeRejectedResult(context, expected, tx, post, post, rejectionEventInfo(tx), item.actor,
                rejection, field(relay.dryRun.gasConsumed, 'refTime', 'ref_time'), true, rejection.commonError, true));
          }
        }
      }
      if (!(context.preflight.experiment_family === 'E7'
        && ['V3', 'V4', 'Q1'].includes(context.preflight.inputs.e7_variant))) {
      const expectedRaceError = context.scenario.cardinality
        ? context.scenario.cardinality.rejected_error_code
        : null;
      const expectedStatus = G3_COMMON_STATUS[
        expectedRaceError || context.scenario.attempts[1].expected_error_code
      ];
      assert(expectedStatus, `G3 E4 status mapping is missing for ${scenarioId}`);
      const attemptState = (
        targetSigned = signed, targetRightId = rightId, buyerRole = roles.fund
      ) => {
        context.snapshotBuyerRole = buyerRole;
        return { ...state, signed: targetSigned, rightId: targetRightId, buyerRole };
      };
      const writeAccepted = async (
        expected, actorRole, submit, observedState, settlement = false
      ) => {
        const relay = await captureSubmission(context, submit);
        const boundary = await runtimeAttemptSnapshots(
          context, 'G3', observedState, relay.submitted
        );
        const pre = boundary.pre.snapshot;
        const postRead = boundary.post;
        const info = runtimeEventInfo(relay.submitted, 'dataAssets');
        if (settlement) {
          context.lastRecord = postRead.record;
          assert(context.lastRecord && info.postStateCommitment, 'G3 E4 settlement closure missing');
          pre.state_commitment = context.lastRecord.pre_state_commitment;
          postRead.snapshot.state_commitment = info.postStateCommitment;
        } else {
          context.lastRecord = null;
        }
        writeAttempt(
          context, expected, actorRole,
          makeResult(
            context, expected, relay.submitted, pre, postRead.snapshot, info,
            actorRole, field(relay.dryRun.gasConsumed, 'refTime', 'ref_time'), true
          )
        );
      };
      const writeRejected = async (expected, actorRole, submit, observedState) => {
        const relay = await captureSubmission(context, submit);
        const boundary = await runtimeAttemptSnapshots(
          context, 'G3', observedState, relay.submitted
        );
        const pre = boundary.pre.snapshot;
        const post = boundary.post.snapshot;
        const rejection = g3Rejection(relay);
        writeAttempt(
          context, expected, actorRole,
          makeRejectedResult(
            context, expected, relay.submitted, pre, post,
            rejectionEventInfo(relay.submitted), actorRole, rejection,
            field(relay.dryRun.gasConsumed, 'refTime', 'ref_time'), true,
            expected.expected_error_code || expectedRaceError
          )
        );
      };
      const rejectedExpectation = { ok: false, code: expectedStatus };
      if (scenarioId === 'DUP-CREATE-01') {
        await writeAccepted(
          context.scenario.attempts[0], roles.create,
          () => createSignedOrder(signed, 'create_first'), attemptState()
        );
        await writeRejected(
          context.scenario.attempts[1], roles.create,
          () => createSignedOrder(signed, 'create_duplicate', rejectedExpectation),
          attemptState()
        );
      } else if (scenarioId === 'RACE-OBJECT-01') {
        await writeAccepted(
          context.scenario.attempts[0], roles.create,
          () => createSignedOrder(signed, 'create_first'), attemptState()
        );
        await writeRejected(
          context.scenario.attempts[1], roles.create,
          () => createSignedOrder(secondSigned, 'create_competing', rejectedExpectation),
          attemptState(secondSigned, rightId)
        );
      } else if (scenarioId === 'RACE-BUYER-01') {
        await writeAccepted(
          context.scenario.attempts[0], 'BUYER_1',
          () => fundSignedOrder(signed, 'BUYER_1', 0, 'fund_buyer_1'),
          attemptState(signed, rightId, 'BUYER_1')
        );
        await writeRejected(
          context.scenario.attempts[1], 'BUYER_2',
          () => fundSignedOrder(
            signed, 'BUYER_2', 0, 'fund_buyer_2', rejectedExpectation
          ),
          attemptState(signed, rightId, 'BUYER_2')
        );
      } else if (scenarioId === 'RACE-QUOTA-01') {
        await writeAccepted(
          context.scenario.attempts[0], 'BUYER_1',
          () => fundSignedOrder(signed, 'BUYER_1', 0, 'fund_first_order'),
          attemptState(signed, rightId, 'BUYER_1')
        );
        await writeRejected(
          context.scenario.attempts[1], 'BUYER_2',
          () => fundSignedOrder(
            secondSigned, 'BUYER_2', 0, 'fund_last_quota', rejectedExpectation
          ),
          attemptState(secondSigned, secondRightId, 'BUYER_2')
        );
        await writeAccepted(
          context.scenario.attempts[2], 'RELAYER',
          () => g3.callRelay(
            api, contractAddress, accounts.RELAYER, 'settle_quota_winner',
            g3.simpleHashCall(api, 'relay_settle', signed.orderId), { ok: true }
          ),
          attemptState(signed, rightId, 'BUYER_1'), true
        );
      } else if (scenarioId === 'DUP-REFUND-01') {
        await writeAccepted(
          context.scenario.attempts[0], 'RELAYER',
          () => g3.callRelay(
            api, contractAddress, accounts.RELAYER, 'refund_first',
            g3.simpleHashCall(api, 'relay_refund_funded', signed.orderId), { ok: true }
          ),
          attemptState()
        );
        await writeRejected(
          context.scenario.attempts[1], 'OWNER',
          () => g3.callRelay(
            api, contractAddress, accounts.OWNER, 'refund_duplicate',
            g3.simpleHashCall(api, 'relay_refund_funded', signed.orderId),
            rejectedExpectation
          ),
          attemptState()
        );
      } else {
        const relayers = scenarioId === 'RACE-RELAYER-01'
          ? ['RELAYER', 'OWNER', 'RIGHT_SELLER', 'MARKET_OK']
          : ['RELAYER', 'OWNER'];
        for (let index = 0; index < context.scenario.attempts.length; index += 1) {
          const expected = context.scenario.attempts[index];
          const actorRole = relayers[index];
          if (index === 0) {
            await writeAccepted(
              expected, actorRole,
              () => g3.callRelay(
                api, contractAddress, accounts[actorRole], `settle_${index + 1}`,
                g3.simpleHashCall(api, 'relay_settle', signed.orderId), { ok: true }
              ),
              attemptState(), true
            );
          } else {
            await writeRejected(
              expected, actorRole,
              () => g3.callRelay(
                api, contractAddress, accounts[actorRole], `settle_${index + 1}`,
                g3.simpleHashCall(api, 'relay_settle', signed.orderId),
                rejectedExpectation
              ),
              attemptState()
            );
          }
        }
      }
      }
    } else if (context.scenario.attempts.length === 3) {
      let expected = context.scenario.attempts[0];
      const create = await captureSubmission(context, () => createOrder('create'));
      let boundary = await runtimeAttemptSnapshots(context, 'G3', state, create.submitted);
      let pre = boundary.pre.snapshot;
      let post = boundary.post.snapshot;
      let info = runtimeEventInfo(create.submitted, 'dataAssets');
      writeAttempt(context, expected, roles.create, makeResult(context, expected, create.submitted, pre, post, info, roles.create, field(create.dryRun.gasConsumed, 'refTime', 'ref_time')));

      expected = context.scenario.attempts[1];
      const fund = await captureSubmission(context, () => fundOrder('fund'));
      boundary = await runtimeAttemptSnapshots(context, 'G3', state, fund.submitted);
      pre = boundary.pre.snapshot;
      post = boundary.post.snapshot;
      info = runtimeEventInfo(fund.submitted, 'dataAssets');
      writeAttempt(context, expected, roles.fund, makeResult(context, expected, fund.submitted, pre, post, info, roles.fund, field(fund.dryRun.gasConsumed, 'refTime', 'ref_time')));

      expected = context.scenario.attempts[2];
      const settle = await captureSubmission(context, () => g3.callRelay(
        api, contractAddress, accounts.RELAYER, 'settle',
        g3.simpleHashCall(api, 'relay_settle', signed.orderId), { ok: true }
      ));
      boundary = await runtimeAttemptSnapshots(context, 'G3', state, settle.submitted);
      pre = boundary.pre.snapshot;
      const postRead = boundary.post;
      context.lastRecord = postRead.record;
      info = runtimeEventInfo(settle.submitted, 'dataAssets');
      assert(context.lastRecord && info.postStateCommitment, 'G3 settlement closure missing');
      pre.state_commitment = context.lastRecord.pre_state_commitment;
      postRead.snapshot.state_commitment = info.postStateCommitment;
      writeAttempt(context, expected, roles.settle, makeResult(context, expected, settle.submitted, pre, postRead.snapshot, info, roles.settle, field(settle.dryRun.gasConsumed, 'refTime', 'ref_time')));
    } else {
      const expected = context.scenario.attempts[0];
      let submitted;
      let contractGas = null;
      let typedCeCall = true;
      let actorRole = 'RELAYER';
      if (expected.phase === 'expire_open') {
        submitted = await captureSubmission(context, () => common.sendTx(
          api, 'expire', api.tx.dataAssets.expireOpen(signed.orderId), accounts.RELAYER
        ));
        typedCeCall = false;
      } else {
        actorRole = expected.phase === 'cancel_open' ? roles.seller : 'RELAYER';
        const message = expected.phase === 'cancel_open' ? 'relay_cancel_open' : 'relay_refund_funded';
        const relay = await captureSubmission(context, () => g3.callRelay(
          api, contractAddress, accounts[actorRole], expected.phase, g3.simpleHashCall(api, message, signed.orderId), { ok: true }
        ));
        submitted = relay.submitted;
        contractGas = field(relay.dryRun.gasConsumed, 'refTime', 'ref_time');
      }
      const boundary = await runtimeAttemptSnapshots(context, 'G3', state, submitted);
      const pre = boundary.pre.snapshot;
      const post = boundary.post.snapshot;
      const info = runtimeEventInfo(submitted, 'dataAssets');
      writeAttempt(
        context, expected, actorRole,
        makeResult(context, expected, submitted, pre, post, info, actorRole, contractGas, typedCeCall)
      );
    }
    return { api, fixture, startBlock, endBlock: (await api.rpc.chain.getHeader()).number.toNumber() };
  } catch (error) {
    await api.disconnect();
    throw error;
  }
}

function testGateCommand(baseline, scenarioId) {
  const filter = TEST_GATE_FILTER[baseline] && TEST_GATE_FILTER[baseline][scenarioId];
  assert(filter, `${baseline} has no frozen test gate for ${scenarioId}`);
  if (baseline === 'B1') {
    return [
      'cargo', 'test', '--manifest-path',
      'contracts/market_pure_contract/Cargo.toml', filter, '--', '--nocapture',
    ];
  }
  const packageName = baseline === 'B2' ? 'pallet-market-runtime' : 'pallet-dataassets';
  return ['cargo', 'test', '-p', packageName, filter, '--', '--nocapture'];
}

function assertProductionFailpointNotExposed(baseline, worktree) {
  if (baseline === 'B1') {
    const source = fs.readFileSync(
      path.join(worktree, 'contracts/market_pure_contract/lib.rs'), 'utf8'
    );
    assert(
      /#\[cfg\(test\)\]\s*fn set_test_abort_operation/.test(source),
      'B1 test abort setter is not cfg(test)-guarded'
    );
    assert(
      /fn abort_after_first_domain_write_for_test[\s\S]*?#\[cfg\(test\)\]\s*\{[\s\S]*?TEST_ABORT_OPERATION/.test(source),
      'B1 test abort read is not cfg(test)-guarded'
    );
  } else if (baseline === 'B2') {
    const source = fs.readFileSync(
      path.join(worktree, 'pallets/market_runtime/src/lib.rs'), 'utf8'
    );
    assert(!/TEST_ABORT|set_test_abort/.test(source), 'B2 production pallet exposes a test abort hook');
  } else if (baseline === 'G3') {
    const source = fs.readFileSync(
      path.join(worktree, 'pallets/dataassets/src/lib.rs'), 'utf8'
    );
    const setters = source.match(
      /#\[cfg\(test\)\]\s*pub\(crate\) fn set_test_abort_(?:meta|issue_right|transfer_right)_settlement/g
    ) || [];
    const triggers = source.match(
      /#\[cfg\(test\)\]\s*fn maybe_abort_(?:meta|issue_right|transfer_right)_settlement/g
    ) || [];
    assert(setters.length === 3 && triggers.length === 3,
      'G3 test abort hooks are not fully cfg(test)-guarded');
  } else {
    throw new Error(`unsupported test-gate baseline ${baseline}`);
  }
  return false;
}

function makeTestGateEvidence(
  context, command, exitCode, namedTestPassed, productionFailpointExposed, output
) {
  return {
    schema_id: 'assetxchain-exp2-phase4-test-gate',
    schema_version: 1,
    run_id: context.preflight.run_id,
    baseline_id: context.baseline,
    scenario_id: context.scenario.scenario_id,
    repetition: context.repetition,
    attempt_id: 'atomic-rollback-test',
    command,
    exit_code: exitCode,
    source_commit: context.preflight.baseline.head,
    q_all_equal: exitCode === 0 && namedTestPassed,
    production_failpoint_exposed: productionFailpointExposed,
    test_output_sha256: sha256Bytes(output),
  };
}

function runTestGate(context) {
  const { baseline, scenario, outputRoot, preflight } = context;
  const command = testGateCommand(baseline, scenario.scenario_id);
  const productionFailpointExposed = assertProductionFailpointNotExposed(
    baseline, preflight.baseline.worktree
  );
  const completed = spawnSync(command[0], command.slice(1), {
    cwd: preflight.baseline.worktree,
    env: process.env,
    encoding: null,
  });
  const stdout = completed.stdout || Buffer.alloc(0);
  const stderr = completed.stderr || Buffer.alloc(0);
  const errorText = completed.error
    ? Buffer.from(`\nspawn-error:\n${completed.error.stack || completed.error.message}\n`, 'utf8')
    : Buffer.alloc(0);
  const output = Buffer.concat([
    Buffer.from('stdout:\n', 'utf8'), stdout,
    Buffer.from('\nstderr:\n', 'utf8'), stderr,
    errorText,
  ]);
  const attemptRoot = path.join(outputRoot, 'attempt-001');
  writeBytesExclusive(path.join(attemptRoot, 'test-output.log'), output);
  const exitCode = completed.status === null ? 1 : completed.status;
  const filter = TEST_GATE_FILTER[baseline][scenario.scenario_id];
  const namedTestPassed = new RegExp(`test [^\\n]*${filter} [^\\n]*ok`).test(
    output.toString('utf8')
  );
  const evidence = makeTestGateEvidence(
    context, command, exitCode, namedTestPassed, productionFailpointExposed, output
  );
  writeJsonExclusive(path.join(attemptRoot, 'test-gate.json'), evidence);
  assert(exitCode === 0, `${baseline} ${scenario.scenario_id} rollback test failed`);
  assert(namedTestPassed, `${baseline} ${scenario.scenario_id} rollback test did not execute`);
  return evidence;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  assert(
    new Set(['live-chain', 'test-gate']).has(args.execution_mode),
    `unsupported execution mode ${args.execution_mode}`
  );
  const preflight = readJson(args.preflight);
  assert(preflight.validity_level === 'provisional-candidate', 'invalid provisional candidate validity level');
  assert(preflight.status === 'planned', 'preflight is not planned');
  assert(preflight.baseline.dirty === false, 'dirty baseline is forbidden');
  assert(preflight.fixture.fixture_profile_id === 'assetxchain-paper2-shared-fixture-v1', 'fixture profile drift');
  assert(preflight.protocol.protocol_id === 'assetxchain-paper2-shared', 'protocol drift');
  assert(preflight.inputs.seed === common.SEED, 'seed drift');
  const catalog = readJson(path.join('/home/gz785/code/exp2', preflight.inputs.scenario_catalog.path));
  const scenario = validateExecutionContract(args, preflight, catalog);
  assert(fs.existsSync(args.output_root) && fs.readdirSync(args.output_root).length === 0, 'output root must exist and be empty');
  validateDependencies(args, preflight);
  const context = contextFor(args, preflight, scenario);
  if (args.execution_mode === 'test-gate') {
    const evidence = runTestGate(context);
    const inventory = artifactInventory(context.outputRoot);
    const unitManifest = {
      schema_id: 'assetxchain-exp2-phase4-e0-unit-manifest', schema_version: 1,
      run_id: preflight.run_id, baseline_id: context.baseline,
      scenario_id: scenario.scenario_id, repetition: args.repetition,
      execution_mode: args.execution_mode,
      genesis_hash: null, fixture_anchor_block: null, start_block: null, end_block: null,
      metadata_hash: null, fixture_state_hash: null,
      scenario_result_hash: blake(evidence),
      shared_input_hash: common.SHARED_INPUT_HASH,
      attempt_count: 1,
      artifact_inventory: inventory,
    };
    writeJsonExclusive(path.join(context.outputRoot, 'unit-manifest.json'), unitManifest);
    console.log(JSON.stringify({
      status: 'PASS', baseline: context.baseline,
      scenario: scenario.scenario_id, repetition: args.repetition,
    }));
    return;
  }
  let run;
  if (context.baseline === 'B1') run = await runB1(context);
  else if (context.baseline === 'B2') run = await runB2(context);
  else if (context.baseline === 'G3') run = await runG3(context);
  else throw new Error(`unsupported baseline ${context.baseline}`);
  try {
    assert(context.results.length === scenario.attempts.length, 'unit attempt count differs from scenario contract');
    const fixtureStateHash = blake(semanticSnapshot(run.fixture.initial_state));
    const resultHash = semanticResultHash(context.results);
    const inventory = artifactInventory(context.outputRoot);
    const unitManifest = {
      schema_id: 'assetxchain-exp2-phase4-e0-unit-manifest', schema_version: 1,
      run_id: preflight.run_id, baseline_id: context.baseline,
      scenario_id: scenario.scenario_id, repetition: args.repetition,
      execution_mode: args.execution_mode,
      genesis_hash: run.fixture.genesis_hash,
      fixture_anchor_block: run.fixture.fixture_anchor_block,
      start_block: run.startBlock, end_block: run.endBlock,
      metadata_hash: run.fixture.metadata_hash,
      fixture_state_hash: fixtureStateHash,
      scenario_result_hash: resultHash,
      shared_input_hash: run.fixture.shared_input_hash,
      attempt_count: context.results.length,
      artifact_inventory: inventory,
    };
    writeJsonExclusive(path.join(context.outputRoot, 'unit-manifest.json'), unitManifest);
    console.log(JSON.stringify({ status: 'PASS', baseline: context.baseline, scenario: scenario.scenario_id, repetition: args.repetition }));
  } finally {
    await run.api.disconnect();
  }
}

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stack || error.message || String(error));
    process.exit(1);
  });
}

module.exports = {
  E0_FIXTURE_ANCHOR_BLOCK,
  FAMILY_SCENARIOS,
  deterministicFixtureAnchor,
  freeBalance,
  g3Rejection,
  makeResult,
  makeRejectedResult,
  makeTestGateEvidence,
  mappedNativeError,
  normalizedRecord,
  parseArgs,
  semanticResultHash,
  testGateCommand,
  usesCompetitionExecution,
  validateExecutionContract,
  waitUntilAfterBlock,
  contextFor,
  runB1,
  runB2,
  runG3,
  actorRoles,
  semanticSnapshot,
  artifactInventory,
  sha256File,
};
