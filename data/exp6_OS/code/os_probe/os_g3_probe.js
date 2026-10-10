'use strict';

// OS-G3-PROBE: behavior probe for OpenSea-style expired/cancelled listing attack on G3.
// Each scenario uses an independent asset. create -> fund -> [wait] -> settle via relay contract.

const fs = require('fs');
const path = require('path');
const Module = require('module');

const DRIVER_ROOT = process.env.POLKADOT_DRIVER_ROOT;
const requireFromDriver = Module.createRequire(path.join(DRIVER_ROOT, 'package.json'));

function dep(name) {
  try { return require(name); } catch (_error) { return requireFromDriver(name); }
}

const { ApiPromise, WsProvider, Keyring } = dep('@polkadot/api');
const { blake2AsU8a, cryptoWaitReady, decodeAddress } = dep('@polkadot/util-crypto');
const { hexToU8a, stringToU8a, u8aConcat, u8aToHex } = dep('@polkadot/util');
const {
  TIMING,
  fundSharedAccounts,
  sendTx,
  sharedAccounts,
} = require('../scenarios/s314_live_common');

const RPC = process.env.ASSETX_RPC || 'ws://127.0.0.1:9944';
const WASM_PATH = process.env.G3_V2_WASM;
const UNIT = 1_000_000_000_000n;
const GAS_LIMIT = { refTime: '1000000000000', proofSize: '104857600' };
const ZERO_HASH = `0x${'00'.repeat(32)}`;
const PROTOCOL_VERSION = 1;
const DOMAIN_ID = stringToU8a('axc/p2/shared');
const MARKET_DOMAIN_ID = stringToU8a('axc/p2/market');
const COMMON_ERRORS = Object.fromEntries([
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
].map((name, index) => [index + 1, name]));
const TYPE = {
  order: 'PalletDataassetsProtocolOrderCommitment',
  witness: 'PalletDataassetsProtocolPricePolicyWitness',
  seller: 'PalletDataassetsProtocolSellerCommitment',
  buyer: 'PalletDataassetsProtocolBuyerCommitment',
  policy: 'PalletDataassetsPricePolicy',
};
const OPERATION_INDEX = { TransferMeta: 0, IssueRight: 1, TransferRight: 2 };
const MESSAGE_INDEX = { Order: 0, Seller: 1, Buyer: 2, Settlement: 3, Right: 4 };

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

function encode(api, type, value) {
  return api.createType(type, value).toU8a();
}

function hash(...parts) {
  return u8aToHex(blake2AsU8a(u8aConcat(...parts), 256));
}

function contractData(api, name, args = []) {
  return u8aToHex(u8aConcat(
    selector(name),
    ...args.map(([type, value]) => (type ? encode(api, type, value) : value))
  ));
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

function computeOrderId(api, genesisHash, order) {
  return hash(fixed32(domainSeparator(genesisHash, order.operation, 'Order')), encode(api, TYPE.order, order));
}

function sellerSigningPayload(api, genesisHash, operation, commitment) {
  return u8aConcat(fixed32(domainSeparator(genesisHash, operation, 'Seller')), encode(api, TYPE.seller, commitment));
}

function buyerSigningPayload(api, genesisHash, operation, commitment) {
  return u8aConcat(fixed32(domainSeparator(genesisHash, operation, 'Buyer')), encode(api, TYPE.buyer, commitment));
}

function computeMarketId(operator, contractAddress, nonce) {
  return hash(
    MARKET_DOMAIN_ID,
    decodeAddress(operator.address || operator),
    decodeAddress(contractAddress),
    le(nonce, 8)
  );
}

function computePolicyHash(api, policy) {
  return hash(encode(api, TYPE.policy, policy));
}

function field(value, camel, snake = camel) {
  if (value && Object.prototype.hasOwnProperty.call(value, camel)) return value[camel];
  if (value && Object.prototype.hasOwnProperty.call(value, snake)) return value[snake];
  return undefined;
}

function optionJson(option, label) {
  assert(option.isSome, `${label} is missing`);
  return option.unwrap().toJSON();
}

function findEvent(records, section, method) {
  return records.find(({ event }) => event.section === section && event.method === method);
}

function countEvent(records, section, method) {
  return records.filter(({ event }) => event.section === section && event.method === method).length;
}

function buildSignedOrder(api, genesisHash, seller, input) {
  const sellerStateVersion = input.sellerStateVersion ?? 1;
  const orderNonce = input.orderNonce ?? 0;
  const order = {
    settlementProtocolVersion: input.settlementProtocolVersion ?? PROTOCOL_VERSION,
    marketId: input.marketId,
    operation: input.operation,
    objectType: input.objectType,
    objectId: input.objectId,
    parentAssetId: input.parentAssetId || null,
    authorizationId: input.authorizationId,
    seller: accountHex(seller),
    allowedBuyer: input.allowedBuyer || null,
    price: input.price.toString(),
    currency: 'Native',
    policyHash: input.policyHash,
    rightParametersHash: input.rightParametersHash || null,
    sellerStateVersion,
    orderNonce,
    expiry: input.expiry,
  };
  const orderId = computeOrderId(api, genesisHash, order);
  const sellerCommitment = {
    orderId,
    objectId: input.objectId,
    operation: input.operation,
    seller: accountHex(seller),
    stateVersion: sellerStateVersion,
    pricePolicyHash: input.policyHash,
    rightParametersHash: input.rightParametersHash || null,
    expiresAt: input.expiry,
    sellerNonce: orderNonce,
  };
  const signature = seller.sign(sellerSigningPayload(api, genesisHash, input.operation, sellerCommitment));
  return { order, orderId, sellerCommitment, sellerSignature: signature };
}

function buildSignedBuyer(api, genesisHash, buyer, operation, orderId, price, expiry, nonce) {
  const buyerCommitment = {
    orderId,
    buyer: accountHex(buyer),
    price: price.toString(),
    currency: 'Native',
    expiresAt: expiry,
    buyerNonce: nonce,
  };
  const signature = buyer.sign(buyerSigningPayload(api, genesisHash, operation, buyerCommitment));
  return { buyerCommitment, buyerSignature: signature };
}

function createOrderCall(api, signed, witness) {
  return contractData(api, 'relay_create_order', [
    [TYPE.order, signed.order],
    [TYPE.witness, witness],
    [TYPE.seller, signed.sellerCommitment],
    [null, signed.sellerSignature],
  ]);
}

function fundOrderCall(api, orderId, signedBuyer) {
  return contractData(api, 'relay_fund_order', [
    [null, fixed32(orderId)],
    [TYPE.buyer, signedBuyer.buyerCommitment],
    [null, signedBuyer.buyerSignature],
  ]);
}

function simpleHashCall(api, message, orderId) {
  return contractData(api, message, [[null, fixed32(orderId)]]);
}

async function dryRunContract(api, origin, contractAddress, data) {
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
    data: json.result && json.result.ok ? json.result.ok.data : null,
    reverted: json.result && json.result.ok ? json.result.ok.flags.bits !== 0 : true,
    dispatch: json.result,
  };
}

function decodeRelayResult(dataHex) {
  const bytes = hexToU8a(dataHex);
  assert(bytes.length >= 2, `short ink return: ${dataHex}`);
  assert(bytes[0] === 0, `ink language error: ${dataHex}`);
  if (bytes[1] === 0) {
    return { ok: true, payload: u8aToHex(bytes.slice(2)) };
  }
  assert(bytes[1] === 1, `invalid relay result: ${dataHex}`);
  const contractError = bytes[2];
  if (contractError === 0) {
    const extensionError = bytes[3];
    if (extensionError === 0) {
      const code = bytes[4];
      return { ok: false, error: 'ChainExtension', code, name: COMMON_ERRORS[code] || `Common(${code})` };
    }
    if (extensionError === 1) {
      const code = Number(BigInt(bytes[4]) | (BigInt(bytes[5]) << 8n) | (BigInt(bytes[6]) << 16n) | (BigInt(bytes[7]) << 24n));
      return { ok: false, error: 'UnknownStatus', code };
    }
    return { ok: false, error: 'Decode' };
  }
  return { ok: false, error: 'ContractError' };
}

// probe relay call: dry-run decode; submit only on success; never hard-assert.
async function relayProbe(api, contractAddress, signer, label, data) {
  const dry = await dryRunContract(api, signer, contractAddress, data);
  let decoded = null;
  if (dry.data) {
    decoded = decodeRelayResult(dry.data);
  }
  let submitted = null;
  if (decoded && decoded.ok) {
    const tx = api.tx.contracts.call(contractAddress, '0', GAS_LIMIT, null, data);
    submitted = await sendTx(api, label, tx, signer, null);
  }
  const header = await api.rpc.chain.getHeader();
  return {
    label,
    block: header.number.toNumber(),
    reverted: dry.reverted,
    decoded,
    submitted: submitted ? {
      ok: Boolean(findEvent(submitted.events, 'dataAssets', 'SettlementFinalized') || findEvent(submitted.events, 'contracts', 'ContractEmitted')),
      eventCount: countEvent(submitted.events, 'dataAssets', 'SettlementFinalized') + countEvent(submitted.events, 'contracts', 'ContractEmitted'),
      block: submitted.blockNumber,
    } : null,
  };
}

async function waitForOrderState(api, orderId, expectState, timeoutMs = 90000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const stored = await api.query.dataAssets.runtimeOrders(orderId);
    const state = stored.isSome ? field(stored.unwrap().toJSON(), 'state') : null;
    if (state === expectState) return stored.unwrap().toJSON();
    if (Date.now() > deadline) throw new Error(`order ${orderId} not in state ${expectState} (got ${state})`);
    await sleep(1000);
  }
}

async function waitForBlock(api, targetBlock, timeoutMs = 240000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const header = await api.rpc.chain.getHeader();
    if (header.number.toNumber() > targetBlock) return header.number.toNumber();
    if (Date.now() > deadline) throw new Error(`block ${targetBlock} not reached within timeout`);
    await sleep(2000);
  }
}

async function snapshotOrder(api, orderId) {
  const stored = await api.query.dataAssets.runtimeOrders(orderId);
  const order = stored.isSome ? stored.unwrap().toJSON() : null;
  let record = null;
  let escrow = null;
  try {
    const r = await api.query.dataAssets.settlementRecords(orderId);
    if (r.isSome) record = r.unwrap().toJSON();
  } catch (e) { /* none */ }
  if (order && field(order, 'escrowId', 'escrow_id')) {
    const esc = await api.query.dataAssets.runtimeEscrows(field(order, 'escrowId', 'escrow_id'));
    if (esc.isSome) escrow = esc.unwrap().toJSON();
  }
  return { order, record, escrow };
}

function summarizeOrder(order) {
  if (!order) return null;
  return {
    state: field(order, 'state'),
    expiry: field(order, 'expiry'),
    grace_deadline: field(order, 'grace_deadline', 'graceDeadline'),
    price: field(order, 'price'),
    seller: field(order, 'seller'),
    buyer: field(order, 'buyer'),
  };
}

async function createAuthAndOrder(api, accounts, contractAddress, assetId, marketId, price, expiry, buyerNonce, label) {
  const authResult = await sendTx(
    api,
    `${label}_auth`,
    api.tx.dataAssets.createTransferMetaAuthorization(assetId, marketId, {
      pricePolicy: {
        referencePrice: price.toString(),
        floorRatioBps: 10000,
        signedMarketFeeCapBps: 0,
        safetyFeeBps: 0,
      },
      allowedBuyer: accounts.BUYER_1.address,
      expiry,
      nonce: 0,
    }),
    accounts.OWNER
  );
  const authEvent = findEvent(authResult.events, 'dataAssets', 'TransferMetaAuthorizationCreated');
  assert(authEvent, `${label} TransferMetaAuthorizationCreated missing`);
  const authId = authEvent.event.data[0].toHex();

  const genesisHash = api.genesisHash.toHex();
  const signed = buildSignedOrder(api, genesisHash, accounts.OWNER, {
    marketId,
    operation: 'TransferMeta',
    objectType: 'MetaToken',
    objectId: assetId,
    authorizationId: authId,
    allowedBuyer: accountHex(accounts.BUYER_1),
    price: price.toString(),
    policyHash: computePolicyHash(api, {
      referencePrice: price.toString(),
      floorRatioBps: 10000,
      signedMarketFeeCapBps: 0,
      safetyFeeBps: 0,
    }),
    sellerStateVersion: 1,
    orderNonce: 0,
    expiry,
  });
  const create = await relayProbe(api, contractAddress, accounts.OWNER, `${label}_create`, createOrderCall(api, signed, 'Authorization'));
  assert(create.decoded && create.decoded.ok, `${label} create failed: ${JSON.stringify(create.decoded)}`);
  await waitForOrderState(api, signed.orderId, 'Open');
  const signedBuyer = buildSignedBuyer(api, genesisHash, accounts.BUYER_1, 'TransferMeta', signed.orderId, price, expiry, buyerNonce);
  const fund = await relayProbe(api, contractAddress, accounts.BUYER_1, `${label}_fund`, fundOrderCall(api, signed.orderId, signedBuyer));
  assert(fund.decoded && fund.decoded.ok, `${label} fund failed: ${JSON.stringify(fund.decoded)}`);
  await waitForOrderState(api, signed.orderId, 'Funded');
  return { authId, signed, signedBuyer, create, fund };
}

async function main() {
  assert(WASM_PATH, 'G3_V2_WASM required');
  await cryptoWaitReady();
  const api = await ApiPromise.create({ provider: new WsProvider(RPC) });
  await api.isReady;
  const accounts = sharedAccounts();
  const results = {};

  const wasm = fs.readFileSync(WASM_PATH);
  const wasmHex = u8aToHex(wasm);
  const codeHash = u8aToHex(blake2AsU8a(wasm, 256));
  const salt = u8aToHex(blake2AsU8a(stringToU8a('os-probe-g3-20261003'), 128));
  const genesisHash = api.genesisHash.toHex();
  await fundSharedAccounts(api, accounts, { transactions: [], events: {} });

  const placeholderData = contractData(api, 'new', [
    [null, fixed32(ZERO_HASH)],
    [null, le(TIMING.fundedGrace, 4)],
  ]);
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
  assert(predictedJson.result && predictedJson.result.ok, `contract address prediction failed: ${JSON.stringify(predictedJson.result)}`);
  const predictedAddress = predictedJson.result.ok.accountId;

  const deploy = await sendTx(
    api,
    'deploy_market_orderbook',
    api.tx.contracts.instantiateWithCode('0', GAS_LIMIT, null, wasmHex, placeholderData, salt),
    accounts.MARKET_OK
  );
  const instantiated = findEvent(deploy.events, 'contracts', 'Instantiated');
  assert(instantiated, 'contracts.Instantiated missing');
  const contractAddress = instantiated.event.data[1].toString();
  assert(contractAddress === predictedAddress, 'deployed contract address differs from prediction');
  const marketNonce = BigInt((await api.query.markets.nextMarketNonce()).toString());
  const marketId = computeMarketId(accounts.MARKET_OK, contractAddress, marketNonce);

  const bind = await relayProbe(
    api,
    contractAddress,
    accounts.MARKET_OK,
    'bind_market_id',
    contractData(api, 'bind_market_id', [[null, fixed32(marketId)]]),
  );
  assert(bind.decoded && bind.decoded.ok, `bind failed: ${JSON.stringify(bind.decoded)}`);

  const tierCall = api.tx.markets.setCapacityTier({
    tierId: 1,
    requiredStake: (10_000n * UNIT).toString(),
    maxActiveOrders: 32,
    maxFundedOrders: 16,
    maxActiveExposureValue: (1_000n * UNIT).toString(),
  });
  await sendTx(api, 'set_capacity_tier', api.tx.sudo.sudo(tierCall), accounts.OWNER);
  await sendTx(
    api,
    'register_market',
    api.tx.markets.registerMarket(contractAddress, codeHash, 31, 1, 0),
    accounts.MARKET_OK
  );

  const assetIds = [];
  for (const [index, byte] of [0x61, 0x62, 0x63, 0x64].entries()) {
    const label = `osasset${index}`;
    const result = await sendTx(
      api,
      `register_asset_${index}`,
      api.tx.dataAssets.registerAssetCore(
        Array.from(Buffer.from(label, 'utf8')),
        Array.from(Buffer.from(`os-${label}`, 'utf8')),
        `0x${byte.toString(16).padStart(2, '0').repeat(32)}`,
        1024
      ),
      accounts.OWNER
    );
    const event = findEvent(result.events, 'dataAssets', 'AssetRegistered');
    assert(event, `AssetRegistered missing for ${label}`);
    assetIds.push(event.event.data[0].toHex());
  }

  const price = 10n * UNIT;
  const attacker = accounts.BUYER_1;

  // ============ OS-1a: expired listing, settle within grace window ============
  {
    const h0 = await api.rpc.chain.getHeader();
    const expiry = h0.number.toNumber() + 3;
    const { signed, create, fund } = await createAuthAndOrder(api, accounts, contractAddress, assetIds[0], marketId, price, expiry, 0, 'os1a');
    const graceDeadline = fund.block + Number(TIMING.fundedGrace);
    const expiredAt = await waitForBlock(api, expiry);
    const settle = await relayProbe(api, contractAddress, accounts.RELAYER, 'os1a_settle', simpleHashCall(api, 'relay_settle', signed.orderId));
    const snap = await snapshotOrder(api, signed.orderId);
    results.os1a = {
      scenario: 'OS-1a expired listing, settle within grace window',
      expiry, createBlock: create.block, fundBlock: fund.block, settleBlock: settle.block,
      expiredAtSettle: expiredAt > expiry,
      withinGraceAtSettle: settle.block <= graceDeadline,
      graceDeadline,
      settle, orderState: summarizeOrder(snap.order),
      settlementRecord: snap.record ? field(snap.record, 'orderId', 'order_id') : null,
      escrowState: snap.escrow ? field(snap.escrow, 'state') : null,
    };
    console.log('RESULT_OS1A', JSON.stringify(results.os1a));
  }

  // ============ OS-1b (control): settle after expiry AND after grace ============
  {
    const h0 = await api.rpc.chain.getHeader();
    const expiry = h0.number.toNumber() + 3;
    const { signed, create, fund } = await createAuthAndOrder(api, accounts, contractAddress, assetIds[1], marketId, price, expiry, 1, 'os1b');
    const graceDeadline = fund.block + Number(TIMING.fundedGrace);
    const afterGrace = await waitForBlock(api, graceDeadline);
    const settle = await relayProbe(api, contractAddress, accounts.RELAYER, 'os1b_settle', simpleHashCall(api, 'relay_settle', signed.orderId));
    const snap = await snapshotOrder(api, signed.orderId);
    results.os1b = {
      scenario: 'OS-1b (control) expired listing, settle AFTER grace deadline',
      expiry, createBlock: create.block, fundBlock: fund.block, settleBlock: settle.block,
      afterGraceAtSettle: afterGrace > graceDeadline,
      graceDeadline,
      settle, orderState: summarizeOrder(snap.order),
      settlementRecord: snap.record ? field(snap.record, 'orderId', 'order_id') : null,
      escrowState: snap.escrow ? field(snap.escrow, 'state') : null,
    };
    console.log('RESULT_OS1B', JSON.stringify(results.os1b));
  }

  // ============ OS-2a: off-chain cancellation, attacker funds + settles ============
  {
    const h0 = await api.rpc.chain.getHeader();
    const expiry = h0.number.toNumber() + 20;
    const { signed, create, fund } = await createAuthAndOrder(api, accounts, contractAddress, assetIds[2], marketId, price, expiry, 2, 'os2a');
    // NO on-chain cancel: listing invalidation is off-chain only.
    const settle = await relayProbe(api, contractAddress, accounts.RELAYER, 'os2a_settle', simpleHashCall(api, 'relay_settle', signed.orderId));
    const snap = await snapshotOrder(api, signed.orderId);
    results.os2a = {
      scenario: 'OS-2a off-chain cancelled listing (no on-chain cancel), attacker funds + settles',
      expiry, createBlock: create.block, fundBlock: fund.block, settleBlock: settle.block,
      settle, orderState: summarizeOrder(snap.order),
      settlementRecord: snap.record ? field(snap.record, 'orderId', 'order_id') : null,
      escrowState: snap.escrow ? field(snap.escrow, 'state') : null,
    };
    console.log('RESULT_OS2A', JSON.stringify(results.os2a));
  }

  // ============ OS-2b (control): on-chain cancel_open before fund ============
  {
    const h0 = await api.rpc.chain.getHeader();
    const expiry = h0.number.toNumber() + 20;
    const authResult = await sendTx(
      api,
      'os2b_auth',
      api.tx.dataAssets.createTransferMetaAuthorization(assetIds[3], marketId, {
        pricePolicy: { referencePrice: price.toString(), floorRatioBps: 10000, signedMarketFeeCapBps: 0, safetyFeeBps: 0 },
        allowedBuyer: attacker.address,
        expiry,
        nonce: 0,
      }),
      accounts.OWNER
    );
    const authEvent = findEvent(authResult.events, 'dataAssets', 'TransferMetaAuthorizationCreated');
    const authId = authEvent.event.data[0].toHex();
    const signed = buildSignedOrder(api, genesisHash, accounts.OWNER, {
      marketId, operation: 'TransferMeta', objectType: 'MetaToken', objectId: assetIds[3],
      authorizationId: authId, allowedBuyer: accountHex(attacker), price: price.toString(),
      policyHash: computePolicyHash(api, { referencePrice: price.toString(), floorRatioBps: 10000, signedMarketFeeCapBps: 0, safetyFeeBps: 0 }),
      sellerStateVersion: 1, orderNonce: 0, expiry,
    });
    const create = await relayProbe(api, contractAddress, accounts.OWNER, 'os2b_create', createOrderCall(api, signed, 'Authorization'));
    assert(create.decoded && create.decoded.ok, `os2b create failed: ${JSON.stringify(create.decoded)}`);
    await waitForOrderState(api, signed.orderId, 'Open');
    const cancel = await sendTx(api, 'os2b_cancel', api.tx.dataAssets.cancelOpen(signed.orderId), accounts.OWNER, null);
    const cancelEvent = countEvent(cancel.events, 'dataAssets', 'OrderCancelled');
    const signedBuyer = buildSignedBuyer(api, genesisHash, attacker, 'TransferMeta', signed.orderId, price, expiry, 3);
    const fund = await relayProbe(api, contractAddress, attacker, 'os2b_fund', fundOrderCall(api, signed.orderId, signedBuyer));
    const settle = await relayProbe(api, contractAddress, accounts.RELAYER, 'os2b_settle', simpleHashCall(api, 'relay_settle', signed.orderId));
    const snap = await snapshotOrder(api, signed.orderId);
    results.os2b = {
      scenario: 'OS-2b (control) on-chain cancel_open, then fund/settle attempts',
      expiry, createBlock: create.block, cancel: { ok: cancelEvent === 1 }, fund, settle,
      orderState: summarizeOrder(snap.order),
      settlementRecord: snap.record ? field(snap.record, 'orderId', 'order_id') : null,
      escrowState: snap.escrow ? field(snap.escrow, 'state') : null,
    };
    console.log('RESULT_OS2B', JSON.stringify(results.os2b));
  }

  const summary = {
    os1a_settle_ok: results.os1a.settle.decoded ? results.os1a.settle.decoded.ok : null,
    os1b_error: results.os1b.settle.decoded ? results.os1b.settle.decoded.name || results.os1b.settle.decoded.error : null,
    os2a_settle_ok: results.os2a.settle.decoded ? results.os2a.settle.decoded.ok : null,
    os2b_fund_error: results.os2b.fund.decoded ? (results.os2b.fund.decoded.name || results.os2b.fund.decoded.error) : null,
    os2b_settle_error: results.os2b.settle.decoded ? (results.os2b.settle.decoded.name || results.os2b.settle.decoded.error) : null,
  };
  console.log('SUMMARY', JSON.stringify(summary));
  process.exit(0);
}

main().catch((error) => {
  console.error('PROBE_FAILED', error && error.stack ? error.stack : String(error));
  process.exit(1);
});
