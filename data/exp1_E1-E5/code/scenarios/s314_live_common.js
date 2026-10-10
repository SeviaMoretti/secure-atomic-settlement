'use strict';

const fs = require('fs');
const path = require('path');
const Module = require('module');

const DRIVER_ROOT = process.env.POLKADOT_DRIVER_ROOT
  || 'C:\\Users\\22004\\Documents\\paper2-chain\\outputs\\g3-rpc-driver';
const requireFromDriver = Module.createRequire(path.join(DRIVER_ROOT, 'package.json'));

function dep(name) {
  try {
    return require(name);
  } catch (_error) {
    return requireFromDriver(name);
  }
}

const { ApiPromise, WsProvider, Keyring } = dep('@polkadot/api');
const { blake2AsU8a, cryptoWaitReady, decodeAddress } = dep('@polkadot/util-crypto');
const { hexToU8a, stringToU8a, u8aConcat, u8aToHex } = dep('@polkadot/util');

const UNIT = 1_000_000_000_000n;
const PROTOCOL_VERSION = 1;
const DOMAIN_ID = stringToU8a('axc/p2/shared');
const RESALE_DOMAIN_ID = stringToU8a('axc/p2/resale-policy');
const MESSAGE_INDEX = { Order: 0, Seller: 1, Buyer: 2, Settlement: 3, Right: 4 };
const OPERATION_INDEX = { TransferMeta: 0, IssueRight: 1, TransferRight: 2 };
const PRICES = {
  TransferMeta: 10n * UNIT,
  IssueRight: 3n * UNIT,
  TransferRight: 4n * UNIT,
};
const TIMING = { orderExpiry: 20, fundedGrace: 10, rightValidity: 100 };
const SHARED_INPUT_HASH = '0xc427552f41308fe6e7345a4af448ef3670caba676f1da46e433a1b0aa88e9c13';
const SEED = 20260726;

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function safeJson(value) {
  return JSON.parse(JSON.stringify(value, (_, item) => (
    typeof item === 'bigint' ? item.toString() : item
  )));
}

function fixed32(value) {
  const bytes = typeof value === 'string' ? hexToU8a(value) : value;
  assert(bytes.length === 32, `expected 32 bytes, got ${bytes.length}`);
  return bytes;
}

function le(value, bytes) {
  let current = BigInt(value);
  const result = new Uint8Array(bytes);
  for (let index = 0; index < bytes; index += 1) {
    result[index] = Number(current & 0xffn);
    current >>= 8n;
  }
  return result;
}

function hash(...parts) {
  return u8aToHex(blake2AsU8a(u8aConcat(...parts), 256));
}

function accountHex(account) {
  return u8aToHex(decodeAddress(account.address || account));
}

function domainSeparator(genesisHash, operation, messageType) {
  return hash(
    fixed32(genesisHash),
    DOMAIN_ID,
    le(PROTOCOL_VERSION, 2),
    new Uint8Array([MESSAGE_INDEX[messageType]]),
    new Uint8Array([OPERATION_INDEX[operation]])
  );
}

function computeOrderId(genesisHash, operation, encodedOrder) {
  return hash(fixed32(domainSeparator(genesisHash, operation, 'Order')), encodedOrder);
}

function sellerSigningPayload(genesisHash, operation, encodedCommitment) {
  return u8aConcat(
    fixed32(domainSeparator(genesisHash, operation, 'Seller')),
    encodedCommitment
  );
}

function buyerSigningPayload(genesisHash, operation, encodedCommitment) {
  return u8aConcat(
    fixed32(domainSeparator(genesisHash, operation, 'Buyer')),
    encodedCommitment
  );
}

function computeRightId(genesisHash, orderId) {
  return hash(
    fixed32(domainSeparator(genesisHash, 'IssueRight', 'Right')),
    fixed32(orderId),
    stringToU8a('right')
  );
}

function computeResaleAuthorizationId(rightId, marketId) {
  return hash(RESALE_DOMAIN_ID, fixed32(rightId), fixed32(marketId));
}

function eventRows(records) {
  return records.map(({ event, phase }) => ({
    section: event.section,
    method: event.method,
    phase: phase.toString(),
    data: event.data.map((item) => item.toString()),
  }));
}

function indexedEventRows(records) {
  return eventRows(records).map((row, eventIndex) => ({ eventIndex, ...row }));
}

function matchingBlockEventIndexes(records, blockRecords) {
  const transactionRows = eventRows(records);
  const blockRows = eventRows(blockRecords);
  let cursor = 0;
  return transactionRows.map((row) => {
    const encoded = JSON.stringify(row);
    const relative = blockRows.slice(cursor).findIndex(
      (candidate) => JSON.stringify(candidate) === encoded
    );
    assert(relative >= 0, `transaction event is absent from complete block events: ${encoded}`);
    const eventIndex = cursor + relative;
    cursor = eventIndex + 1;
    return eventIndex;
  });
}

function findEvent(records, section, method) {
  return records.find(({ event }) => event.section === section && event.method === method);
}

function countEvent(records, section, method) {
  return records.filter(({ event }) => event.section === section && event.method === method).length;
}

function eventJson(eventRecord, index) {
  assert(eventRecord && eventRecord.event && eventRecord.event.data[index],
    `event data ${index} is missing`);
  const value = eventRecord.event.data[index];
  return typeof value.toJSON === 'function' ? value.toJSON() : value;
}

function dispatchMetrics(records) {
  const success = findEvent(records, 'system', 'ExtrinsicSuccess');
  const failed = findEvent(records, 'system', 'ExtrinsicFailed');
  assert(Boolean(success) !== Boolean(failed),
    'transaction must contain exactly one terminal system dispatch event');
  const dispatchInfo = eventJson(success || failed, success ? 0 : 1);
  const weight = dispatchInfo && (dispatchInfo.weight || dispatchInfo.actualWeight);
  assert(weight, 'terminal system dispatch event omitted post-dispatch weight');
  const feeEvent = findEvent(records, 'transactionPayment', 'TransactionFeePaid');
  return {
    postDispatchWeight: safeJson(weight),
    actualFee: feeEvent ? feeEvent.event.data[1].toString() : null,
    dispatchEvent: {
      section: (success || failed).event.section,
      method: (success || failed).event.method,
    },
    feeEvent: feeEvent ? { section: feeEvent.event.section, method: feeEvent.event.method } : null,
  };
}

function declaredContractGasLimit(tx) {
  try {
    if (!tx.method || tx.method.section !== 'contracts' || tx.method.method !== 'call') return null;
    const value = tx.args && tx.args[2];
    return value && typeof value.toJSON === 'function' ? value.toJSON() : safeJson(value);
  } catch (_error) {
    return null;
  }
}

function decodeDispatchError(api, dispatchError) {
  if (!dispatchError.isModule) return { section: 'system', name: dispatchError.type, docs: [] };
  const metadata = api.registry.findMetaError(dispatchError.asModule);
  return {
    section: metadata.section,
    name: metadata.name,
    docs: metadata.docs.map((doc) => doc.toString()),
  };
}

async function blockEventsAt(api, blockHash, timeoutMs = 5000, retryMs = 100) {
  const deadline = Date.now() + timeoutMs;
  do {
    const records = await api.query.system.events.at(blockHash);
    if (records.length > 0) return records;
    if (Date.now() >= deadline) break;
    await new Promise((resolve) => setTimeout(resolve, retryMs));
  } while (true);
  throw new Error(`block ${blockHash} returned no system events after ${timeoutMs}ms`);
}

async function sendTx(api, label, tx, signer, expectedFailure = null) {
  const started = Date.now();
  const allowDispatchFailure = Boolean(expectedFailure && expectedFailure.allowDispatchFailure);
  const payment = await tx.paymentInfo(signer);
  return new Promise((resolve, reject) => {
    let unsub = null;
    let terminalSeen = false;
    tx.signAndSend(signer, async (result) => {
      try {
        if (!result.status.isInBlock && !result.status.isFinalized) return;
        if (terminalSeen) return;
        terminalSeen = true;
        const records = result.events || [];
        const failed = findEvent(records, 'system', 'ExtrinsicFailed');
        const dispatchError = failed ? decodeDispatchError(api, failed.event.data[0]) : null;
        const dispatch = dispatchMetrics(records);
        if (unsub) unsub();
        if (allowDispatchFailure) {
          // E7 competing submissions are intentionally decoded after inclusion;
          // the caller classifies the winner and loser from dispatchError.
        } else if (expectedFailure) {
          if (!dispatchError
            || dispatchError.section !== expectedFailure.section
            || dispatchError.name !== expectedFailure.name) {
            reject(new Error(`${label} expected ${JSON.stringify(expectedFailure)}, got ${JSON.stringify(dispatchError)}`));
            return;
          }
        } else if (dispatchError) {
          reject(new Error(`${label} dispatch failed: ${JSON.stringify(dispatchError)}`));
          return;
        }
        const blockHash = result.status.isInBlock
          ? result.status.asInBlock.toHex()
          : result.status.asFinalized.toHex();
        const header = await api.rpc.chain.getHeader(blockHash);
        const blockRecords = await blockEventsAt(api, blockHash);
        const eventIndexes = matchingBlockEventIndexes(records, blockRecords);
        const dispatchFee = dispatch.actualFee || payment.partialFee.toString();
        const environmentalBalanceCredits = [];
        blockRecords.forEach(({ event, phase }, index) => {
          if (event.section !== 'rewards' || event.method !== 'RewardPaid') return;
          environmentalBalanceCredits.push({
            account: event.data[0].toString(),
            amount: event.data[1].toString(),
            reason: 'block-author-reward',
            blockNumber: header.number.toNumber(),
            eventIndex: index,
            phase: phase.toString(),
          });
        });
        resolve({
          label,
          txHash: tx.hash.toHex(),
          blockHash,
          parentBlockHash: header.parentHash.toHex(),
          blockNumber: header.number.toNumber(),
          terminalStatus: result.status.isInBlock ? 'in-block' : 'finalized',
          latencyMs: Date.now() - started,
          estimatedPartialFee: payment.partialFee.toString(),
          partialFee: payment.partialFee.toString(),
          actualFee: dispatch.actualFee,
          postDispatchWeight: dispatch.postDispatchWeight,
          declaredPaymentWeight: payment.weight.toJSON(),
          declaredContractGasLimit: declaredContractGasLimit(tx),
          dispatchEvent: dispatch.dispatchEvent,
          feeEvent: dispatch.feeEvent,
          dispatchFee,
          paymentWeight: payment.weight.toJSON(),
          dispatchError,
          events: records,
          eventIndexes,
          eventRows: eventRows(records),
          blockEventRows: indexedEventRows(blockRecords),
          environmentalBalanceCredits,
        });
      } catch (error) {
        reject(error);
      }
    }).then((unsubscribe) => {
      unsub = unsubscribe;
      if (terminalSeen) unsub();
    }).catch(reject);
  });
}

function recordTx(evidence, result, extra = {}) {
  evidence.transactions.push(safeJson({ ...result, events: undefined, ...extra }));
  evidence.events[result.label] = result.eventRows;
}

async function connect(rpc) {
  await cryptoWaitReady();
  const api = await ApiPromise.create({ provider: new WsProvider(rpc) });
  await api.isReady;
  return api;
}

function sharedAccounts() {
  const keyring = new Keyring({ type: 'sr25519' });
  return {
    OWNER: keyring.addFromUri('//Alice'),
    RIGHT_SELLER: keyring.addFromUri('//Bob'),
    BUYER_1: keyring.addFromUri('//Charlie'),
    BUYER_2: keyring.addFromUri('//Dave'),
    MARKET_OK: keyring.addFromUri('//Eve'),
    MARKET_DENIED: keyring.addFromUri('//Ferdie'),
    RELAYER: keyring.addFromUri('//Alice//stash'),
  };
}

async function fundSharedAccounts(api, accounts, evidence) {
  const target = 100_000n * UNIT;
  for (const role of ['BUYER_1', 'BUYER_2', 'MARKET_OK', 'MARKET_DENIED']) {
    const result = await sendTx(
      api,
      `fund_${role.toLowerCase()}`,
      api.tx.balances.transferAllowDeath(accounts[role].address, target.toString()),
      accounts.OWNER
    );
    recordTx(evidence, result);
  }
}

function baseEvidence(baselineId, api, runId, implementationCommit, gitDirty) {
  return {
    schema_id: 'assetxchain-exp2-s314-live-smoke',
    schema_version: 1,
    run: {
      run_id: runId,
      baseline_id: baselineId,
      validity_level: 'legacy-pilot',
      started_at: new Date().toISOString(),
      timezone: 'Asia/Shanghai',
      implementation_commit: implementationCommit,
      git_dirty: gitDirty,
      fixture_profile_id: 'assetxchain-paper2-shared-fixture-v1',
      shared_input_hash: SHARED_INPUT_HASH,
      seed: SEED,
      claim_boundary: 'build-and-live-smoke-evidence-only; not a paper result',
    },
    environment: {
      rpc: process.env.ASSETX_RPC || 'ws://127.0.0.1:9944',
      chain: null,
      runtime: `${api.runtimeVersion.specName.toString()}-${api.runtimeVersion.specVersion.toString()}`,
      genesis_hash: api.genesisHash.toHex(),
      metadata_hash: u8aToHex(blake2AsU8a(api.runtimeMetadata.toU8a(), 256)),
      deterministic_reset: '--dev --tmp',
    },
    shared_input: {
      quantity: 1,
      prices_plancks: Object.fromEntries(Object.entries(PRICES).map(([key, value]) => [key, value.toString()])),
      timing_offsets_blocks: {
        order_expiry: TIMING.orderExpiry,
        funded_grace: TIMING.fundedGrace,
        right_validity: TIMING.rightValidity,
      },
      experiment_families: ['E0'],
      scenario_ids: [
        'LEG-META-01', 'LEG-ISSUE-01', 'LEG-RESALE-01',
        'NEG-PAYER-01', 'CLEAN-FUND-01', 'DUP-SETTLE-01',
      ],
    },
    transactions: [],
    events: {},
    operations: {},
    rejection: null,
    duplicate: null,
    audit: null,
    architecture_cost: {},
    status: 'RUNNING',
  };
}

function writeExclusiveJson(filePath, value) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, `${JSON.stringify(safeJson(value), null, 2)}\n`, { encoding: 'utf8', flag: 'wx' });
}

function architectureMetric(availability, value, unit) {
  assert(['measured', 'not-collected', 'N/A'].includes(availability), 'invalid availability');
  assert((availability === 'measured') === (value !== null), 'metric value/availability mismatch');
  return { availability, value: value === null ? null : String(value), unit };
}

module.exports = {
  PRICES,
  PROTOCOL_VERSION,
  SEED,
  SHARED_INPUT_HASH,
  TIMING,
  UNIT,
  accountHex,
  architectureMetric,
  assert,
  baseEvidence,
  blockEventsAt,
  buyerSigningPayload,
  computeOrderId,
  computeResaleAuthorizationId,
  computeRightId,
  connect,
  countEvent,
  dispatchMetrics,
  dep,
  findEvent,
  fixed32,
  fundSharedAccounts,
  hash,
  recordTx,
  safeJson,
  sellerSigningPayload,
  sendTx,
  sharedAccounts,
  writeExclusiveJson,
};
