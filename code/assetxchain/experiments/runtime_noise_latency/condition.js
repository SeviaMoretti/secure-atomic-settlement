'use strict';
const fs = require('fs');
const path = require('path');
const DRIVER_ROOT = process.env.POLKADOT_DRIVER_ROOT || '/home/gz785/code/exp2/scripts/phase4/node';
const polkadot = require(path.join(DRIVER_ROOT, 'node_modules/@polkadot/api'));
const ApiPromise = polkadot.ApiPromise;
const WsProvider = polkadot.WsProvider;
const Keyring = polkadot.Keyring;
const RPC = process.env.ASSETX_RPC || 'ws://127.0.0.1:9944';
const VARIANT = process.env.STATE_VARIANT || 'dual_prefix';
const NOISE_COUNT = Number(process.env.NOISE_COUNT || '0');
const REPETITIONS = Number(process.env.REPETITIONS || '3');
const SEED = Number(process.env.EXPERIMENT_SEED || '20260923');
const OUTPUT = process.env.OUTPUT || '/tmp/assetxchain-noise-' + VARIANT + '-' + NOISE_COUNT + '.json';
const GAS_TIMEOUT_MS = 45000;

function assert(condition, message) { if (!condition) throw new Error(message); }
function sleep(ms) { return new Promise(function(resolve) { setTimeout(resolve, ms); }); }
function jsonSafe(value) {
  return JSON.parse(JSON.stringify(value, function(_, v) {
    return typeof v === 'bigint' ? v.toString() : v;
  }));
}
function eventBy(events, section, method) {
  return events.find(function(item) { return item.event.section === section && item.event.method === method; });
}
function txEvents(events) {
  return events.map(function(item) {
    return {
      section: item.event.section,
      method: item.event.method,
      data: item.event.data.map(function(x) { return x.toHex(); }),
      phase: item.phase.toString()
    };
  });
}
function waitWithTimeout(promise, label) {
  return Promise.race([
    promise,
    new Promise(function(_, reject) {
      setTimeout(function() { reject(new Error(label + ' timeout')); }, GAS_TIMEOUT_MS);
    })
  ]);
}

async function main() {
  assert(NOISE_COUNT >= 0 && REPETITIONS >= 1, 'invalid noise/repetitions');
  const api = await ApiPromise.create({ provider: new WsProvider(RPC) });
  await api.isReady;
  const keyring = new Keyring({ type: 'sr25519' });
  const accounts = {
    alice: keyring.addFromUri('//Alice'),
    bob: keyring.addFromUri('//Bob'),
    charlie: keyring.addFromUri('//Charlie'),
    dave: keyring.addFromUri('//Dave'),
    eve: keyring.addFromUri('//Eve'),
    ferdie: keyring.addFromUri('//Ferdie'),
    one: keyring.addFromUri('//One'),
    two: keyring.addFromUri('//Two')
  };
  const noiseSigners = [accounts.charlie, accounts.dave, accounts.eve, accounts.ferdie, accounts.one, accounts.two];
  const nextNonce = new Map();
  for (const account of Object.values(accounts)) {
    nextNonce.set(account.address, (await api.rpc.system.accountNextIndex(account.address)).toNumber());
  }

  async function sendAndWait(label, tx, signer) {
    const nonce = nextNonce.get(signer.address);
    assert(nonce !== undefined, 'nonce missing');
    nextNonce.set(signer.address, nonce + 1);
    const startHeader = await api.rpc.chain.getHeader();
    const startBlock = startHeader.number.toNumber();
    const started = process.hrtime.bigint();
    return waitWithTimeout(new Promise(function(resolve, reject) {
      let unsub = null;
      tx.signAndSend(signer, { nonce: nonce }, async function(result) {
        if (!(result.status.isInBlock || result.status.isFinalized)) return;
        try {
          const blockHash = result.status.isInBlock ? result.status.asInBlock : result.status.asFinalized;
          const header = await api.rpc.chain.getHeader(blockHash);
          const block = await api.rpc.chain.getBlock(blockHash);
          const extrinsics = block.block.extrinsics || [];
          const targetHash = tx.hash.toHex();
          const extrinsicIndex = extrinsics.findIndex(function(item) { return item.hash.toHex() === targetHash; });
          const events = result.events || [];
          const failed = eventBy(events, 'system', 'ExtrinsicFailed');
          const row = {
            label: label,
            tx_hash: tx.hash.toHex(),
            start_block: startBlock,
            included_block: header.number.toNumber(),
            completion_blocks: header.number.toNumber() - startBlock,
            block_extrinsic_count: extrinsics.length,
            extrinsic_index: extrinsicIndex,
            latency_ms: Number((Number(process.hrtime.bigint() - started) / 1e6).toFixed(3)),
            success: !failed,
            dispatch_error: failed ? failed.event.data.toString() : '',
            events: txEvents(events)
          };
          if (unsub) unsub();
          resolve(row);
        } catch (error) { reject(error); }
      }).then(function(unsubscribe) { unsub = unsubscribe; }).catch(reject);
    }), label);
  }

  function submitNoWait(label, tx, signer) {
    const nonce = nextNonce.get(signer.address);
    assert(nonce !== undefined, 'noise nonce missing');
    nextNonce.set(signer.address, nonce + 1);
    return new Promise(function(resolve, reject) {
      let unsub = null;
      tx.signAndSend(signer, { nonce: nonce }, async function(result) {
        if (!(result.status.isInBlock || result.status.isFinalized)) return;
        const events = result.events || [];
        const failed = eventBy(events, 'system', 'ExtrinsicFailed');
        const row = {
          label: label,
          tx_hash: tx.hash.toHex(),
          block_hash: result.status.isInBlock ? result.status.asInBlock.toHex() : result.status.asFinalized.toHex(),
          success: !failed,
          dispatch_error: failed ? failed.event.data.toString() : ''
        };
        if (unsub) unsub();
        if (failed) reject(new Error(label + ' failed: ' + row.dispatch_error));
        else resolve(row);
      }).then(function(unsubscribe) { unsub = unsubscribe; }).catch(reject);
    });
  }

  const fundingPromises = [];
  for (const [name, account] of Object.entries(accounts)) {
    if (name === 'alice') continue;
    fundingPromises.push(submitNoWait('fund-' + name, api.tx.balances.transferAllowDeath(account.address, '1000000000000000'), accounts.alice));
  }
  const funded = await waitWithTimeout(Promise.all(fundingPromises), 'account funding');
  assert(funded.every(function(row) { return row.success; }), 'account funding failed');

  const rows = [];
  const noiseRows = [];
  try {
    for (let repetition = 0; repetition < REPETITIONS; repetition += 1) {
      const label = VARIANT + '-noise' + NOISE_COUNT + '-rep' + repetition;
      const rawHash = '0x' + ((SEED + repetition) % 256).toString(16).padStart(2, '0').repeat(32);
      const registered = await sendAndWait(
        label + '-setup-asset',
        api.tx.dataAssets.registerAssetCore(
          Array.from(Buffer.from('noise-target-' + repetition)),
          Array.from(Buffer.from(label)),
          rawHash,
          1024
        ),
        accounts.alice
      );
      assert(registered.success, label + ' asset registration failed');
      const assetEvent = registered.events.find(function(e) {
        return e.section === 'dataAssets' && e.method === 'AssetRegistered';
      });
      assert(assetEvent && assetEvent.data[0].length === 66, label + ' missing asset id');
      const assetId = assetEvent.data[0];

      const issued = await sendAndWait(
        label + '-setup-certificate',
        api.tx.dataAssets.issueCertificate(assetId, accounts.bob.address, 1, null),
        accounts.alice
      );
      assert(issued.success, label + ' certificate issue failed');
      const issueEvent = issued.events.find(function(e) {
        return e.section === 'dataAssets' && e.method === 'CertificateIssued';
      });
      assert(issueEvent && issueEvent.data[1].length === 66, label + ' missing certificate id');
      const certificateId = issueEvent.data[1];

      const noisePromises = [];
      for (let i = 0; i < NOISE_COUNT; i += 1) {
        const signer = noiseSigners[i % noiseSigners.length];
        const noiseHash = '0x' + ((SEED + 17 + repetition * 31 + i) % 256).toString(16).padStart(2, '0').repeat(32);
        noisePromises.push(submitNoWait(
          label + '-noise-' + i,
          api.tx.dataAssets.registerAssetCore(
            Array.from(Buffer.from('background-' + repetition + '-' + i)),
            Array.from(Buffer.from(label + '-' + i)),
            noiseHash,
            256
          ),
          signer
        ));
      }
      await sleep(25);
      const target = await sendAndWait(
        label + '-target-transfer-certificate',
        api.tx.dataAssets.transferCertificate(assetId, certificateId, accounts.charlie.address),
        accounts.bob
      );
      assert(target.success, label + ' target failed: ' + target.dispatch_error);
      const settledNoise = await waitWithTimeout(Promise.all(noisePromises), label + ' noise');
      rows.push({
        repetition: repetition,
        noise_count: NOISE_COUNT,
        variant: VARIANT,
        setup_asset: registered,
        setup_certificate: issued,
        target: target,
        noise_success_count: settledNoise.length,
        target_latency_ms: target.latency_ms,
        target_completion_blocks: target.completion_blocks
      });
      noiseRows.push.apply(noiseRows, settledNoise.map(function(item) {
        return Object.assign({ repetition: repetition }, item);
      }));
    }
  } finally {
    await api.disconnect();
  }

  const result = {
    schema_version: 'assetxchain-runtime-noise-v1',
    variant: VARIANT,
    noise_count: NOISE_COUNT,
    repetitions: REPETITIONS,
    seed: SEED,
    rpc: RPC,
    target_operation: 'dataAssets.transferCertificate',
    noise_operation: 'dataAssets.registerAssetCore',
    noise_injection: 'concurrent pool submissions before target submission',
    rows: rows,
    noise_rows: noiseRows
  };
  fs.mkdirSync(path.dirname(OUTPUT), { recursive: true });
  fs.writeFileSync(OUTPUT, JSON.stringify(jsonSafe(result), null, 2) + '\n');
  console.log(JSON.stringify({
    output: OUTPUT,
    variant: VARIANT,
    noise_count: NOISE_COUNT,
    repetitions: REPETITIONS,
    target_latency_ms: rows.map(function(row) { return row.target_latency_ms; }),
    target_completion_blocks: rows.map(function(row) { return row.target_completion_blocks; }),
    noise_success_count: rows.map(function(row) { return row.noise_success_count; })
  }, null, 2));
}
main().catch(function(error) {
  console.error(error.stack || error.message || String(error));
  process.exitCode = 1;
});






