const fs = require('fs');
const path = require('path');
const Module = require('module');

const DRIVER_ROOT = process.env.POLKADOT_DRIVER_ROOT || 'C:\\Users\\22004\\Documents\\paper2-chain\\outputs\\g3-rpc-driver';
const requireFromDriver = Module.createRequire(path.join(DRIVER_ROOT, 'package.json'));

function dep(name) {
  try {
    return require(name);
  } catch (_error) {
    return requireFromDriver(name);
  }
}

const { ApiPromise, WsProvider, Keyring } = dep('@polkadot/api');
const { cryptoWaitReady, blake2AsU8a, decodeAddress } = dep('@polkadot/util-crypto');
const { u8aConcat, u8aToHex, hexToU8a } = dep('@polkadot/util');

const RPC = process.env.ASSETX_RPC || 'ws://127.0.0.1:9944';
const DEFAULT_STAMP = (() => {
  const d = new Date();
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}_g1_full`;
})();
const RUN_ID = process.env.RUN_ID || DEFAULT_STAMP;
const CANONICAL_EXP_ROOT = '/home/gz785/code/exp2';
const EXP_ROOT = process.env.EXP_ROOT || (process.platform === 'win32'
  ? '\\\\wsl.localhost\\Ubuntu\\home\\gz785\\code\\exp2'
  : CANONICAL_EXP_ROOT);
const CONTRACT_ROOT = process.env.G1_CONTRACT_ROOT || path.join(EXP_ROOT, 'contracts', 'g1_pure_market');
const WASM_PATH = process.env.G1_WASM || path.join(
  CONTRACT_ROOT,
  'target',
  'wasm32v1-none',
  'release',
  'g1_pure_market.import_memory.no_bulk.mem16_stack64k.deploy_call_only.wasm'
);
const RAW_DIR = path.join(EXP_ROOT, 'results', 'raw', `exp1_g1_full_${RUN_ID}`);
const PROCESSED_DIR = path.join(EXP_ROOT, 'results', 'processed');
const REPORT_DIR = path.join(EXP_ROOT, 'reports');
const UNIT = 1_000_000_000_000n;

const GAS_LIMIT = { refTime: '1000000000000', proofSize: '104857600' };
const CONTRACT_VALUE_ZERO = 0n;

function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
}

function selector(name) {
  return blake2AsU8a(name, 256).slice(0, 4);
}

function le(value, bytes) {
  let v = BigInt(value);
  const out = new Uint8Array(bytes);
  for (let i = 0; i < bytes; i += 1) {
    out[i] = Number(v & 0xffn);
    v >>= 8n;
  }
  return out;
}

function fixed32(hexOrU8a) {
  const u8a = typeof hexOrU8a === 'string' ? hexToU8a(hexOrU8a) : hexOrU8a;
  if (u8a.length !== 32) {
    throw new Error(`expected 32 bytes, got ${u8a.length}`);
  }
  return u8a;
}

function accountId(address) {
  return decodeAddress(address);
}

function optionU64(value) {
  if (value === null || value === undefined) return new Uint8Array([0]);
  return u8aConcat(new Uint8Array([1]), le(value, 8));
}

function contractData(name, ...parts) {
  return u8aToHex(u8aConcat(selector(name), ...parts));
}

function safeJson(value) {
  return JSON.parse(JSON.stringify(value, (_, v) => typeof v === 'bigint' ? v.toString() : v));
}

function eventRows(events) {
  return events.map(({ event, phase }) => ({
    section: event.section,
    method: event.method,
    phase: phase.toString(),
    data: event.data.map((d) => d.toString()),
    human: event.toHuman(),
  }));
}

function findEvent(events, section, method) {
  return events.find(({ event }) => event.section === section && event.method === method);
}

function dispatchInfoFromEvents(events) {
  const success = findEvent(events, 'system', 'ExtrinsicSuccess');
  if (!success) return {};
  const info = success.event.data[0];
  const json = info.toJSON();
  const weight = json.weight || {};
  return {
    dispatch_class: json.class || '',
    pays_fee: json.paysFee || '',
    weight_ref_time: weight.refTime ? weight.refTime.toString() : '',
    weight_proof_size: weight.proofSize ? weight.proofSize.toString() : '',
  };
}

async function paymentInfo(tx, signer) {
  const info = await tx.paymentInfo(signer);
  return {
    partial_fee: info.partialFee.toString(),
    payment_weight: info.weight ? info.weight.toString() : '',
    payment_class: info.class ? info.class.toString() : '',
  };
}

async function dryRunCall(api, origin, dest, value, data) {
  try {
    const result = await api.call.contractsApi.call(
      origin.address,
      dest,
      value.toString(),
      GAS_LIMIT,
      null,
      data
    );
    const json = result.toJSON();
    const gasConsumed = json.gasConsumed || {};
    const gasRequired = json.gasRequired || {};
    return {
      dry_run_ok: json.result && json.result.ok !== undefined,
      dry_run_result: JSON.stringify(json.result),
      contract_gas_consumed_ref_time: gasConsumed.refTime ? gasConsumed.refTime.toString() : '',
      contract_gas_consumed_proof_size: gasConsumed.proofSize ? gasConsumed.proofSize.toString() : '',
      contract_gas_required_ref_time: gasRequired.refTime ? gasRequired.refTime.toString() : '',
      contract_gas_required_proof_size: gasRequired.proofSize ? gasRequired.proofSize.toString() : '',
      storage_deposit: JSON.stringify(json.storageDeposit || {}),
    };
  } catch (error) {
    return {
      dry_run_ok: false,
      dry_run_result: error.message,
      contract_gas_consumed_ref_time: '',
      contract_gas_consumed_proof_size: '',
      contract_gas_required_ref_time: '',
      contract_gas_required_proof_size: '',
      storage_deposit: '',
    };
  }
}

async function contractRead(api, origin, dest, data) {
  const result = await api.call.contractsApi.call(
    origin.address,
    dest,
    '0',
    GAS_LIMIT,
    null,
    data
  );
  const json = result.toJSON();
  return json.result && json.result.ok ? json.result.ok.data : '';
}

function decodeOptionAccount(dataHex) {
  const bytes = hexToU8a(dataHex);
  if (bytes.length === 34 && bytes[0] === 0 && bytes[1] === 1) {
    return u8aToHex(bytes.slice(2));
  }
  if (bytes.length !== 33 || bytes[0] !== 1) return null;
  return u8aToHex(bytes.slice(1));
}

function accountHex(address) {
  return u8aToHex(accountId(address));
}

function makeCertificateId(assetId, nonce) {
  const bytes = fixed32(assetId).slice();
  bytes.set(le(nonce, 8), 0);
  return u8aToHex(bytes);
}

async function sendTx(api, label, tx, signer, extra = {}) {
  const start = Date.now();
  const startHeader = await api.rpc.chain.getHeader();
  const startBlock = startHeader.number.toNumber();
  const pay = await paymentInfo(tx, signer);

  return new Promise((resolve, reject) => {
    let unsub = null;
    tx.signAndSend(signer, async (result) => {
      if (!result.status.isInBlock && !result.status.isFinalized) return;

      const blockHash = result.status.isInBlock ? result.status.asInBlock : result.status.asFinalized;
      const header = await api.rpc.chain.getHeader(blockHash);
      const includedBlock = header.number.toNumber();
      const events = result.events || [];
      const dispatchError = findEvent(events, 'system', 'ExtrinsicFailed');
      const elapsed = Date.now() - start;

      const row = {
        run_id: RUN_ID,
        group: 'G1',
        scenario: label,
        signer: signer.address,
        tx_hash: tx.hash.toHex(),
        block_hash: blockHash.toHex(),
        start_block: startBlock,
        included_block: includedBlock,
        completion_blocks: includedBlock - startBlock,
        latency_ms: elapsed,
        partial_fee: pay.partial_fee,
        payment_weight: pay.payment_weight,
        events_count: events.length,
        success: dispatchError ? false : true,
        dispatch_error: dispatchError ? dispatchError.event.data.toString() : '',
        ...dispatchInfoFromEvents(events),
        ...extra,
      };
      if (unsub) unsub();
      console.log(JSON.stringify({
        scenario: label,
        success: row.success,
        included_block: row.included_block,
        latency_ms: row.latency_ms,
      }));
      resolve({ row, events: eventRows(events) });
    }).then((u) => {
      unsub = u;
    }).catch(reject);
  });
}

function writeCsv(file, rows) {
  const headers = Array.from(rows.reduce((set, row) => {
    Object.keys(row).forEach((k) => set.add(k));
    return set;
  }, new Set()));
  const esc = (v) => {
    const s = v === undefined || v === null ? '' : String(v);
    return /[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  fs.writeFileSync(file, [headers.join(','), ...rows.map((r) => headers.map((h) => esc(r[h])).join(','))].join('\n') + '\n', 'utf8');
}

async function main() {
  ensureDir(RAW_DIR);
  ensureDir(PROCESSED_DIR);
  ensureDir(REPORT_DIR);

  await cryptoWaitReady();
  const api = await ApiPromise.create({ provider: new WsProvider(RPC) });
  await api.isReady;

  const keyring = new Keyring({ type: 'sr25519' });
  const alice = keyring.addFromUri('//Alice');
  const bob = keyring.addFromUri('//Bob');
  const charlie = keyring.addFromUri('//Charlie');

  const rows = [];
  const eventDump = {};
  const assets = {
    meta_purchase: '0xabcacc47b171f200d8ec9648f1dd5458932e1c67f200c70b0d45d85fae183b61',
    right_issue: '0xda646e813f175e1930c088795ba9b3a16fd0816da77fc3150fcceec16275b2b6',
    right_transfer: '0x2bc7d3033a97b05433c5a1553f854a7d4abf33dcb84a7a6716aa48de77e2a763',
    cancel_asset: '0xd91d4df50b256fc39bb7537fc179b8bbc9b344795779b3f2e0e90852bcbb1495',
  };
  const certs = {};
  const invariants = [];
  let certificateNonce = 0n;

  const wasm = fs.readFileSync(WASM_PATH);
  const wasmHex = u8aToHex(wasm);
  const constructorData = contractData('new', le(30, 4));
  const salt = u8aToHex(le(Date.now(), 16));

  const chain = (await api.rpc.system.chain()).toString();
  const metadata = {
    run_id: RUN_ID,
    chain,
    runtime: `${api.runtimeVersion.specName.toString()}-${api.runtimeVersion.specVersion.toString()}`,
    rpc: RPC,
    canonical_exp_root: CANONICAL_EXP_ROOT,
    exp_root: EXP_ROOT,
    contract_root: CONTRACT_ROOT,
    wasm_path: WASM_PATH,
    wasm_bytes: wasm.length,
    started_at: new Date().toISOString(),
    note: 'Full G1 RPC experiment: pure contract market with order, asset, certificate, and settlement state in contract storage.',
  };

  const charlieFunding = 100n * UNIT;
  const fundCharlie = await sendTx(
    api,
    'fund_charlie_contract_setup',
    api.tx.balances.transferAllowDeath(charlie.address, charlieFunding.toString()),
    alice,
    {
      operation_type: 'runtime_setup',
      pallet: 'balances',
      call: 'transferAllowDeath',
      recipient: charlie.address,
      amount: charlieFunding.toString(),
    }
  );
  rows.push(fundCharlie.row);
  eventDump.fund_charlie_contract_setup = fundCharlie.events;

  const instantiateTx = api.tx.contracts.instantiateWithCode(
    CONTRACT_VALUE_ZERO.toString(),
    GAS_LIMIT,
    null,
    wasmHex,
    constructorData,
    salt
  );
  const deploy = await sendTx(api, 'deploy_pure_market_contract', instantiateTx, alice, {
    operation_type: 'contract_instantiate_with_code',
    contract_message: 'new',
    contract_value: '0',
    wasm_bytes: wasm.length,
  });
  rows.push(deploy.row);
  eventDump.deploy_pure_market_contract = deploy.events;
  const instantiated = deploy.events.find((e) => e.section === 'contracts' && e.method === 'Instantiated');
  const marketAddress = instantiated ? instantiated.data[1] : '';
  if (!marketAddress) throw new Error('Could not find contracts.Instantiated contract address');
  metadata.market_contract = marketAddress;

  async function contractCall(label, signer, message, data, value = 0n, extra = {}) {
    const dry = await dryRunCall(api, signer, marketAddress, value, data);
    const tx = api.tx.contracts.call(marketAddress, value.toString(), GAS_LIMIT, null, data);
    const res = await sendTx(api, label, tx, signer, {
      operation_type: 'contract_call',
      contract_address: marketAddress,
      contract_message: message,
      contract_value: value.toString(),
      ...dry,
      ...extra,
    });
    rows.push(res.row);
    eventDump[label] = res.events;
    return res;
  }

  async function registerAsset(label, assetId, metadataByte) {
    const metadataHash = `0x${metadataByte.toString(16).padStart(2, '0').repeat(32)}`;
    await contractCall(
      `register_asset_${label}`,
      alice,
      'register_asset',
      contractData('register_asset', fixed32(assetId), fixed32(metadataHash), le(1024 * 1024, 8)),
      0n,
      { asset_id: assetId, metadata_hash: metadataHash, size: 1024 * 1024 }
    );
  }

  async function checkOwner(label, assetId, expectedAddress) {
    const data = await contractRead(api, alice, marketAddress, contractData('get_asset_owner', fixed32(assetId)));
    const actual = decodeOptionAccount(data);
    const expected = accountHex(expectedAddress);
    invariants.push({
      id: label,
      expected_account_hex: expected,
      actual_account_hex: actual || '',
      ok: actual === expected,
      raw_return: data,
    });
  }

  async function checkHolder(label, assetId, certificateId, expectedAddress) {
    const data = await contractRead(
      api,
      alice,
      marketAddress,
      contractData('get_certificate_holder', fixed32(assetId), fixed32(certificateId))
    );
    const actual = decodeOptionAccount(data);
    const expected = accountHex(expectedAddress);
    invariants.push({
      id: label,
      expected_account_hex: expected,
      actual_account_hex: actual || '',
      ok: actual === expected,
      raw_return: data,
    });
  }

  // MetaToken ownership purchase.
  const metaAsset = assets.meta_purchase;
  await registerAsset('meta_purchase', metaAsset, 0x11);
  const metaPrice = 10n * UNIT;
  await contractCall(
    'list_meta_order',
    alice,
    'list_asset',
    contractData('list_asset', fixed32(metaAsset), le(metaPrice, 16)),
    0n,
    { asset_id: metaAsset, price: metaPrice.toString() }
  );
  await contractCall(
    'buy_meta_order',
    bob,
    'buy_asset',
    contractData('buy_asset', fixed32(metaAsset)),
    metaPrice,
    { asset_id: metaAsset, price: metaPrice.toString(), buyer: bob.address }
  );
  await checkOwner('I1_meta_asset_owner_equals_buyer', metaAsset, bob.address);

  // RightToken issue-type purchase.
  const issueAsset = assets.right_issue;
  await registerAsset('right_issue', issueAsset, 0x22);
  const issuePrice = 3n * UNIT;
  const issueCert = makeCertificateId(issueAsset, certificateNonce);
  certificateNonce += 1n;
  certs.right_issue = issueCert;
  await contractCall(
    'list_right_issue_order',
    alice,
    'list_certificate_issue',
    contractData('list_certificate_issue', fixed32(issueAsset), new Uint8Array([1]), optionU64(null), le(issuePrice, 16)),
    0n,
    { asset_id: issueAsset, right_type: 1, valid_until: '', price: issuePrice.toString() }
  );
  await contractCall(
    'buy_right_issue_order',
    bob,
    'buy_certificate_issue',
    contractData('buy_certificate_issue', fixed32(issueAsset)),
    issuePrice,
    { asset_id: issueAsset, certificate_id: issueCert, price: issuePrice.toString(), buyer: bob.address }
  );
  await checkHolder('I1_right_issue_holder_equals_buyer', issueAsset, issueCert, bob.address);

  // RightToken transfer-type purchase. Prepare a pure contract certificate held by Alice.
  const transferAsset = assets.right_transfer;
  await registerAsset('right_transfer', transferAsset, 0x33);
  const transferCert = makeCertificateId(transferAsset, certificateNonce);
  certificateNonce += 1n;
  certs.right_transfer = transferCert;
  await contractCall(
    'prepare_righttoken_for_transfer_order',
    alice,
    'issue_certificate',
    contractData('issue_certificate', fixed32(transferAsset), accountId(alice.address), new Uint8Array([1]), optionU64(null)),
    0n,
    { asset_id: transferAsset, certificate_id: transferCert, holder: alice.address, right_type: 1 }
  );
  const transferPrice = 4n * UNIT;
  await contractCall(
    'list_right_transfer_order',
    alice,
    'list_certificate',
    contractData('list_certificate', fixed32(transferAsset), fixed32(transferCert), le(transferPrice, 16)),
    0n,
    { asset_id: transferAsset, certificate_id: transferCert, price: transferPrice.toString() }
  );
  await contractCall(
    'buy_right_transfer_order',
    charlie,
    'buy_certificate',
    contractData('buy_certificate', fixed32(transferAsset), fixed32(transferCert)),
    transferPrice,
    { asset_id: transferAsset, certificate_id: transferCert, price: transferPrice.toString(), buyer: charlie.address }
  );
  await checkHolder('I1_right_transfer_holder_equals_buyer', transferAsset, transferCert, charlie.address);

  // Cancel asset order.
  const cancelAsset = assets.cancel_asset;
  await registerAsset('cancel_asset', cancelAsset, 0x44);
  const cancelPrice = 5n * UNIT;
  await contractCall(
    'list_cancel_asset_order',
    alice,
    'list_asset',
    contractData('list_asset', fixed32(cancelAsset), le(cancelPrice, 16)),
    0n,
    { asset_id: cancelAsset, price: cancelPrice.toString() }
  );
  await contractCall(
    'cancel_asset_order',
    alice,
    'cancel_asset_order',
    contractData('cancel_asset_order', fixed32(cancelAsset)),
    0n,
    { asset_id: cancelAsset }
  );

  metadata.assets = assets;
  metadata.certificates = certs;
  metadata.correctness = {
    invariant_checks: invariants,
    invariant_violation_count: invariants.filter((i) => !i.ok).length,
  };
  metadata.finished_at = new Date().toISOString();

  const jsonOut = {
    metadata,
    rows,
    events: eventDump,
  };
  const rawJson = path.join(RAW_DIR, `exp1_g1_full_${RUN_ID}_raw.json`);
  const csvOut = path.join(PROCESSED_DIR, `exp1_g1_full_${RUN_ID}_transactions.csv`);
  const summaryJson = path.join(PROCESSED_DIR, `exp1_g1_full_${RUN_ID}_summary.json`);
  const report = path.join(REPORT_DIR, `exp1_g1_full_${RUN_ID}_report.md`);
  fs.writeFileSync(rawJson, JSON.stringify(safeJson(jsonOut), null, 2), 'utf8');
  writeCsv(csvOut, rows);
  fs.writeFileSync(summaryJson, JSON.stringify(safeJson({ metadata, rows }), null, 2), 'utf8');

  const md = [
    '# Experiment 1 G1 Full RPC Data',
    '',
    `run_id: \`${RUN_ID}\``,
    `pure_market_contract: \`${marketAddress}\``,
    `rows: ${rows.length}`,
    `invariant_violation_count: ${metadata.correctness.invariant_violation_count}`,
    '',
    '| scenario | success | latency_ms | completion_blocks | partial_fee | weight_ref_time | contract_gas_required_ref_time | events_count |',
    '| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |',
    ...rows.map((r) => `| ${r.scenario} | ${r.success} | ${r.latency_ms} | ${r.completion_blocks} | ${r.partial_fee} | ${r.weight_ref_time || ''} | ${r.contract_gas_required_ref_time || ''} | ${r.events_count} |`),
    '',
    'Notes:',
    '- Data are signed RPC transactions on a temporary `--dev --tmp` chain.',
    '- G1 keeps market order, asset owner, certificate holder, and settlement evidence in contract storage.',
    '- Contract call gas fields come from `contractsApi.call` dry-runs immediately before submission.',
    '- State consistency checks are stored in the summary JSON under `metadata.correctness.invariant_checks`.',
    '',
  ].join('\n');
  fs.writeFileSync(report, md, 'utf8');

  console.log(JSON.stringify({ run_id: RUN_ID, raw: rawJson, csv: csvOut, json: summaryJson, report }, null, 2));
  await api.disconnect();
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
