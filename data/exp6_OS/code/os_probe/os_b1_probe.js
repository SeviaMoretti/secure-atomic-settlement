'use strict';

// OS-B1-PROBE v2: behavior probe for OpenSea-style expired/cancelled listing attack on B1.
// Each scenario uses an independent asset to avoid AuthorizationAlreadyActive.
// create -> (poll Open) -> fund -> (poll Funded) -> [wait] -> settle.

const fs = require('fs');
const {
  PRICES,
  PROTOCOL_VERSION,
  accountHex,
  assert,
  connect,
  countEvent,
  dep,
  findEvent,
  fundSharedAccounts,
  hash,
  sendTx,
  sharedAccounts,
} = require('../scenarios/s314_live_common');

const { CodePromise, ContractPromise } = dep('@polkadot/api-contract');
const { blake2AsU8a } = dep('@polkadot/util-crypto');
const { u8aToHex } = dep('@polkadot/util');

const RPC = process.env.ASSETX_RPC || 'ws://127.0.0.1:9944';
const CONTRACT_METADATA = process.env.B1_CONTRACT_METADATA;
const CONTRACT_WASM = process.env.B1_CONTRACT_WASM;
const GAS_LIMIT = { refTime: '1000000000000', proofSize: '104857600' };

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function fixedHash(byte) {
  return `0x${byte.toString(16).padStart(2, '0').repeat(32)}`;
}

function hash32Hex(value, label) {
  let bytes = value;
  if (value && typeof value === 'object' && !Array.isArray(value) && !ArrayBuffer.isView(value)) {
    bytes = Object.values(value);
  }
  const hex = typeof value === 'string' ? value : u8aToHex(Uint8Array.from(bytes || []));
  assert(/^0x[0-9a-f]{64}$/i.test(hex), `${label} returned invalid Hash32: ${JSON.stringify(value)}`);
  return hex;
}

function field(value, camel, snake = camel) {
  if (value && Object.prototype.hasOwnProperty.call(value, camel)) return value[camel];
  if (value && Object.prototype.hasOwnProperty.call(value, snake)) return value[snake];
  return undefined;
}

function resultJson(output, label) {
  assert(output, `${label} returned no output`);
  let value = output.toJSON();
  assert(value && typeof value === 'object', `${label} returned undecodable output`);
  for (let depth = 0; depth < 4; depth += 1) {
    if (!value || typeof value !== 'object') return { ok: true, value };
    if (Object.prototype.hasOwnProperty.call(value, 'err')) return { ok: false, error: value.err };
    if (Object.prototype.hasOwnProperty.call(value, 'Err')) return { ok: false, error: value.Err };
    if (Object.prototype.hasOwnProperty.call(value, 'ok')) { value = value.ok; continue; }
    if (Object.prototype.hasOwnProperty.call(value, 'Ok')) { value = value.Ok; continue; }
    return { ok: true, value };
  }
  throw new Error(`${label} returned excessively nested Result`);
}

function nativeError(value) {
  if (typeof value === 'string') return value;
  if (value && typeof value === 'object') return Object.keys(value)[0] || JSON.stringify(value);
  return String(value);
}

function returnFlagsBits(flags) {
  if (typeof flags.toNumber === 'function') return flags.toNumber();
  if (flags.bits !== undefined) return Number(flags.bits.toString());
  const value = typeof flags.toJSON === 'function' ? flags.toJSON() : flags;
  if (Array.isArray(value)) {
    assert(value.every((entry) => entry === 'Revert'), `unsupported contract return flags: ${JSON.stringify(value)}`);
    return value.includes('Revert') ? 1 : 0;
  }
  if (value && typeof value === 'object' && value.bits !== undefined) return Number(value.bits);
  if (typeof value === 'number') return value;
  if (typeof value === 'string' && /^(?:0x[0-9a-f]+|\d+)$/i.test(value)) return Number(value);
  throw new Error(`unsupported contract return flags: ${JSON.stringify(value)}`);
}

function messageType(contract, message, index) {
  const type = contract.abi.findMessage(message).args[index].type;
  return type.lookupName || type.type;
}

function encodeMessageArg(contract, message, index, value) {
  return contract.abi.registry.createType(messageType(contract, message, index), value).toU8a();
}

function pricePolicy(price) {
  return {
    referencePrice: price.toString(),
    floorRatioBps: 10000,
    signedMarketFeeCapBps: 0,
    safetyFeeBps: 0,
  };
}

function pricePolicyHash(price) {
  const bytes = Buffer.alloc(22);
  let current = price;
  for (let index = 0; index < 16; index += 1) {
    bytes[index] = Number(current & 0xffn);
    current >>= 8n;
  }
  bytes.writeUInt16LE(10000, 16);
  return hash(bytes);
}

function decodeMessageOutput(contract, messageMetadata, raw, reverted) {
  const returnType = messageMetadata.returnType;
  const outerType = returnType.lookupName || returnType.type;
  try {
    return contract.abi.registry.createTypeUnsafe(outerType, [raw], { isPedantic: true });
  } catch (outerError) {
    const inner = Array.isArray(returnType.sub) ? returnType.sub[0] : null;
    assert(
      reverted && returnType.namespace === 'Result'
        && inner && inner.name === 'Ok' && (inner.lookupName || inner.type),
      `cannot decode ${outerType}: ${outerError.message}`
    );
    const innerType = inner.lookupName || inner.type;
    const wrappedRaw = Uint8Array.from([0, ...raw]);
    let output;
    let innerError;
    for (const candidate of [raw, wrappedRaw]) {
      try {
        output = contract.abi.registry.createTypeUnsafe(innerType, [candidate], { isPedantic: true });
        const decoded = resultJson(output, 'contract revert payload');
        if (!decoded.ok) return output;
      } catch (error) {
        innerError = error;
      }
    }
    throw new Error(
      `cannot decode ${outerType}: ${outerError.message}; `
      + `revert payload ${innerType}: ${innerError?.message || 'decoded as success'}`
    );
  }
}

async function dryRun(contract, message, caller, args = [], value = '0') {
  const messageMetadata = contract.abi.findMessage(message);
  const outcome = await contract.api.call.contractsApi.call(
    caller.address,
    contract.address,
    value,
    contract.registry.createType('WeightV2', GAS_LIMIT),
    null,
    messageMetadata.toU8a(args),
  );
  const dispatch = outcome.result.toJSON();
  let output = null;
  let reverted = false;
  if (outcome.result.isOk && messageMetadata.returnType) {
    const execution = outcome.result.asOk;
    reverted = (returnFlagsBits(execution.flags) & 1) === 1;
    const raw = execution.data.toU8a(true);
    try {
      output = decodeMessageOutput(contract, messageMetadata, raw, reverted);
    } catch (error) {
      throw new Error(`${message} ABI decode failed: ${error.message}`);
    }
  }
  const decoded = output ? resultJson(output, message) : null;
  return { outcome, dispatch, decoded, reverted, gasConsumed: outcome.gasConsumed.toJSON() };
}

// probe-style call: returns { ok, error, block, events } without hard assertions.
// On dry-run rejection it does NOT submit; result.skipped = true.
async function probeCall(api, contract, label, message, caller, args = [], options = {}) {
  const value = options.value || '0';
  const dry = await dryRun(contract, message, caller, args, value);
  const outcome = {
    dry: {
      ok: Boolean(dry.decoded && dry.decoded.ok),
      error: dry.decoded && !dry.decoded.ok ? nativeError(dry.decoded.error) : null,
      reverted: dry.reverted,
      decoded: dry.decoded,
    },
  };
  if (options.dryOnly || (dry.decoded && !dry.decoded.ok)) {
    const header = await api.rpc.chain.getHeader();
    outcome.block = header.number.toNumber();
    outcome.skipped = true;
    outcome.error = outcome.dry.error;
    return outcome;
  }
  const tx = contract.tx[message]({ gasLimit: dry.outcome.gasRequired, storageDepositLimit: null, value }, ...args);
  const submitted = await sendTx(api, label, tx, caller, { allowDispatchFailure: true });
  const header = await api.rpc.chain.getHeader();
  outcome.block = header.number.toNumber();
  outcome.ok = Boolean(findEvent(submitted.events, 'contracts', 'ContractEmitted'));
  outcome.eventCount = countEvent(submitted.events, 'contracts', 'ContractEmitted');
  outcome.dispatchError = submitted.dispatchError || null;
  return outcome;
}

// smoke-style contract call with assertion (used where a successful tx is mandatory).
async function contractCallB1(api, contract, evidence, label, message, caller, args = [], options = {}) {
  const value = options.value || '0';
  const dry = await dryRun(contract, message, caller, args, value);
  assert(dry.decoded, `${label} missing decoded contract result`);
  assert(!dry.reverted, `${label} unexpectedly set the contract revert flag`);
  assert(dry.decoded.ok, `${label} dry run failed: ${nativeError(dry.decoded.error)}`);
  const tx = contract.tx[message]({ gasLimit: dry.outcome.gasRequired, storageDepositLimit: null, value }, ...args);
  const submitted = await sendTx(api, label, tx, caller, null);
  evidence.transactions.push({ label, block: submitted.blockNumber, events: countEvent(submitted.events, 'contracts', 'ContractEmitted') });
  return { dry, submitted };
}

async function contractQuery(contract, message, caller, args = []) {
  const dry = await dryRun(contract, message, caller, args);
  assert(dry.decoded && dry.decoded.ok, `${message} query failed: ${JSON.stringify(dry.decoded)}`);
  return dry.decoded.value;
}

function computeOrderIdLocal(contract, genesisHash, operation, order) {
  const { computeOrderId } = require('../scenarios/s314_live_common');
  return computeOrderId(genesisHash, operation, encodeMessageArg(contract, 'createOrder', 0, order));
}

function sellerSigningPayloadLocal(contract, genesisHash, operation, commitment) {
  const { sellerSigningPayload } = require('../scenarios/s314_live_common');
  return sellerSigningPayload(genesisHash, operation, encodeMessageArg(contract, 'createOrder', 2, commitment));
}

function buyerSigningPayloadLocal(contract, genesisHash, operation, commitment) {
  const { buyerSigningPayload } = require('../scenarios/s314_live_common');
  return buyerSigningPayload(genesisHash, operation, encodeMessageArg(contract, 'fundOrder', 1, commitment));
}

function buildSignedOrder(contract, genesisHash, seller, input) {
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
  const orderId = computeOrderIdLocal(contract, genesisHash, input.operation, order);
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
  const signature = seller.sign(sellerSigningPayloadLocal(contract, genesisHash, input.operation, sellerCommitment));
  return { order, orderId, sellerCommitment, signature };
}

function buildSignedBuyer(contract, genesisHash, buyer, signed, nonce) {
  const commitment = {
    orderId: signed.orderId,
    buyer: accountHex(buyer),
    price: signed.order.price,
    currency: 'Native',
    expiresAt: signed.order.expiry,
    buyerNonce: nonce,
  };
  const signature = buyer.sign(buyerSigningPayloadLocal(contract, genesisHash, signed.order.operation, commitment));
  return { commitment, signature };
}

async function waitForOrderState(contract, caller, orderId, expectState, timeoutMs = 90000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const order = await contractQuery(contract, 'getOrder', caller, [orderId]);
    const state = order ? field(order, 'state') : null;
    if (state === expectState) return order;
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

async function snapshotOrder(contract, caller, orderId) {
  const order = await contractQuery(contract, 'getOrder', caller, [orderId]);
  let record = null;
  try {
    record = await contractQuery(contract, 'getSettlementRecord', caller, [orderId]);
  } catch (e) {
    record = null;
  }
  return { order, record };
}

function summarizeOrder(order) {
  if (!order) return null;
  return {
    state: field(order, 'state'),
    expiry: field(order, 'expiry'),
    price: field(order, 'price'),
    seller: field(order, 'seller'),
    buyer: field(order, 'buyer'),
    grace_deadline: field(order, 'grace_deadline', 'graceDeadline'),
    escrow: field(order, 'escrow', 'escrowId'),
  };
}

async function main() {
  assert(CONTRACT_METADATA && CONTRACT_WASM, 'B1 contract artifact paths are required');
  const api = await connect(RPC);
  const accounts = sharedAccounts();
  const evidence = { transactions: [], events: {} };
  await fundSharedAccounts(api, accounts, evidence);
  const results = {};

  const metadata = JSON.parse(fs.readFileSync(CONTRACT_METADATA, 'utf8'));
  const wasm = fs.readFileSync(CONTRACT_WASM);
  const codeHash = u8aToHex(blake2AsU8a(wasm, 256));
  const code = new CodePromise(api, metadata, wasm);
  const salt = Buffer.from('os-probe-b1-20261003'.slice(-32), 'utf8');
  const deploy = await sendTx(api, 'deploy_b1', code.tx.newWithGenesisHash({
    gasLimit: api.registry.createType('WeightV2', GAS_LIMIT),
    storageDepositLimit: null,
    salt,
  }, api.genesisHash.toHex()), accounts.OWNER);
  const instantiated = findEvent(deploy.events, 'contracts', 'Instantiated');
  assert(instantiated, 'contracts.Instantiated missing');
  const contractAddress = instantiated.event.data[1].toString();
  const contract = new ContractPromise(api, metadata, contractAddress);

  const registeredMarket = await probeCall(api, contract, 'register_market', 'registerMarket', accounts.OWNER, [
    accounts.MARKET_OK.address,
    contractAddress,
    codeHash,
    PROTOCOL_VERSION,
    31,
    0,
    { tierId: 1, maxActiveOrders: 32, maxFundedOrders: 16, maxActiveExposureValue: (1000n * PRICES.TransferMeta).toString() },
    '0',
  ]);
  assert(registeredMarket.dry.ok, `registerMarket failed: ${registeredMarket.dry.error}`);
  const marketId = hash32Hex(registeredMarket.dry.decoded.value, 'register_market');

  const assetIds = [fixedHash(0x61), fixedHash(0x62), fixedHash(0x63), fixedHash(0x64)];
  for (const assetId of assetIds) {
    const regMeta = await probeCall(api, contract, 'register_meta', 'registerMeta', accounts.OWNER, [{
      assetId,
      owner: accounts.OWNER.address,
      registrationProvider: accounts.OWNER.address,
      serviceProvider: accounts.OWNER.address,
      metadataDigest: fixedHash(0x70),
      dataCommitment: fixedHash(0x71),
      validationProgramHash: fixedHash(0x72),
      status: 'Tradable',
      parentAssetId: null,
      supersedesAssetId: null,
    }]);
    assert(regMeta.dry.ok, `registerMeta failed: ${regMeta.dry.error}`);
  }

  const price = PRICES.TransferMeta;
  const attacker = accounts.BUYER_1;

  // ============ OS-1a: expired listing, settle within grace window ============
  {
    const assetId = assetIds[0];
    const h0 = await api.rpc.chain.getHeader();
    const expiry = h0.number.toNumber() + 3;
    const auth = await contractCallB1(api, contract, evidence, 'os1a_auth', 'createTransferMetaAuthorization', accounts.OWNER, [
      assetId, marketId,
      { pricePolicy: pricePolicy(price), allowedBuyer: attacker.address, expiry, nonce: 0 },
    ]);
    const authId = hash32Hex(auth.dry.decoded.value, 'auth');
    const signed = buildSignedOrder(contract, api.genesisHash.toHex(), accounts.OWNER, {
      marketId, operation: 'TransferMeta', objectType: 'MetaToken', objectId: assetId,
      authorizationId: authId, allowedBuyer: accountHex(attacker), price, policyHash: pricePolicyHash(price), expiry,
    });
    const create = await probeCall(api, contract, 'os1a_create', 'createOrder', accounts.RELAYER, [
      signed.order, 'Authorization', signed.sellerCommitment, signed.signature,
    ]);
    assert(create.ok, `os1a create failed: ${JSON.stringify(create)}`);
    await waitForOrderState(contract, accounts.OWNER, signed.orderId, 'Open');
    const buyerSigned = buildSignedBuyer(contract, api.genesisHash.toHex(), attacker, signed, 0);
    const fund = await probeCall(api, contract, 'os1a_fund', 'fundOrder', attacker, [
      signed.orderId, buyerSigned.commitment, buyerSigned.signature,
    ], { value: signed.order.price });
    assert(fund.ok, `os1a fund failed: ${JSON.stringify(fund)}`);
    const fundedOrder = await waitForOrderState(contract, accounts.OWNER, signed.orderId, 'Funded');
    const fundBlock = Number(field(fundedOrder, 'created_block', 'createdBlock') || fund.block);
    const graceDeadline = fund.block + 10;
    // wait until order expiry passed
    const expiredAt = await waitForBlock(api, expiry);
    const settle = await probeCall(api, contract, 'os1a_settle', 'settle', attacker, [signed.orderId]);
    const snap = await snapshotOrder(contract, accounts.OWNER, signed.orderId);
    results.os1a = {
      scenario: 'OS-1a expired listing, settle within grace window',
      expiry, createBlock: create.block, fundBlock: fund.block, settleBlock: settle.block,
      expiredAtSettle: expiredAt > expiry,
      withinGraceAtSettle: settle.block <= graceDeadline,
      graceDeadline,
      settle, orderState: summarizeOrder(snap.order),
      settlementRecord: snap.record ? field(snap.record, 'orderId', 'order_id') : null,
    };
    console.log('RESULT_OS1A', JSON.stringify(results.os1a));
  }

  // ============ OS-1b (control): settle after expiry AND after grace ============
  {
    const assetId = assetIds[1];
    const h0 = await api.rpc.chain.getHeader();
    const expiry = h0.number.toNumber() + 3;
    const auth = await contractCallB1(api, contract, evidence, 'os1b_auth', 'createTransferMetaAuthorization', accounts.OWNER, [
      assetId, marketId,
      { pricePolicy: pricePolicy(price), allowedBuyer: attacker.address, expiry, nonce: 0 },
    ]);
    const authId = hash32Hex(auth.dry.decoded.value, 'auth');
    const signed = buildSignedOrder(contract, api.genesisHash.toHex(), accounts.OWNER, {
      marketId, operation: 'TransferMeta', objectType: 'MetaToken', objectId: assetId,
      authorizationId: authId, allowedBuyer: accountHex(attacker), price, policyHash: pricePolicyHash(price), expiry,
    });
    const create = await probeCall(api, contract, 'os1b_create', 'createOrder', accounts.RELAYER, [
      signed.order, 'Authorization', signed.sellerCommitment, signed.signature,
    ]);
    assert(create.ok, `os1b create failed: ${JSON.stringify(create)}`);
    await waitForOrderState(contract, accounts.OWNER, signed.orderId, 'Open');
    const buyerSigned = buildSignedBuyer(contract, api.genesisHash.toHex(), attacker, signed, 1);
    const fund = await probeCall(api, contract, 'os1b_fund', 'fundOrder', attacker, [
      signed.orderId, buyerSigned.commitment, buyerSigned.signature,
    ], { value: signed.order.price });
    assert(fund.ok, `os1b fund failed: ${JSON.stringify(fund)}`);
    await waitForOrderState(contract, accounts.OWNER, signed.orderId, 'Funded');
    const graceDeadline = fund.block + 10;
    // wait until grace deadline passed
    const afterGrace = await waitForBlock(api, graceDeadline);
    const settle = await probeCall(api, contract, 'os1b_settle', 'settle', attacker, [signed.orderId]);
    const snap = await snapshotOrder(contract, accounts.OWNER, signed.orderId);
    results.os1b = {
      scenario: 'OS-1b (control) expired listing, settle AFTER grace deadline',
      expiry, createBlock: create.block, fundBlock: fund.block, settleBlock: settle.block,
      afterGraceAtSettle: afterGrace > graceDeadline,
      graceDeadline,
      settle, orderState: summarizeOrder(snap.order),
      settlementRecord: snap.record ? field(snap.record, 'orderId', 'order_id') : null,
    };
    console.log('RESULT_OS1B', JSON.stringify(results.os1b));
  }

  // ============ OS-2a: off-chain cancellation, attacker funds + settles ============
  {
    const assetId = assetIds[2];
    const h0 = await api.rpc.chain.getHeader();
    const expiry = h0.number.toNumber() + 20;
    const auth = await contractCallB1(api, contract, evidence, 'os2a_auth', 'createTransferMetaAuthorization', accounts.OWNER, [
      assetId, marketId,
      { pricePolicy: pricePolicy(price), allowedBuyer: attacker.address, expiry, nonce: 0 },
    ]);
    const authId = hash32Hex(auth.dry.decoded.value, 'auth');
    const signed = buildSignedOrder(contract, api.genesisHash.toHex(), accounts.OWNER, {
      marketId, operation: 'TransferMeta', objectType: 'MetaToken', objectId: assetId,
      authorizationId: authId, allowedBuyer: accountHex(attacker), price, policyHash: pricePolicyHash(price), expiry,
    });
    const create = await probeCall(api, contract, 'os2a_create', 'createOrder', accounts.RELAYER, [
      signed.order, 'Authorization', signed.sellerCommitment, signed.signature,
    ]);
    assert(create.ok, `os2a create failed: ${JSON.stringify(create)}`);
    await waitForOrderState(contract, accounts.OWNER, signed.orderId, 'Open');
    // NO on-chain cancel: listing invalidation is off-chain only (market-layer index).
    const buyerSigned = buildSignedBuyer(contract, api.genesisHash.toHex(), attacker, signed, 2);
    const fund = await probeCall(api, contract, 'os2a_fund', 'fundOrder', attacker, [
      signed.orderId, buyerSigned.commitment, buyerSigned.signature,
    ], { value: signed.order.price });
    const settle = await probeCall(api, contract, 'os2a_settle', 'settle', attacker, [signed.orderId]);
    const snap = await snapshotOrder(contract, accounts.OWNER, signed.orderId);
    results.os2a = {
      scenario: 'OS-2a off-chain cancelled listing (no on-chain cancel), attacker funds + settles',
      expiry, createBlock: create.block, fundBlock: fund.block, settleBlock: settle.block,
      fund, settle, orderState: summarizeOrder(snap.order),
      settlementRecord: snap.record ? field(snap.record, 'orderId', 'order_id') : null,
    };
    console.log('RESULT_OS2A', JSON.stringify(results.os2a));
  }

  // ============ OS-2b (control): on-chain cancel_open before fund ============
  {
    const assetId = assetIds[3];
    const h0 = await api.rpc.chain.getHeader();
    const expiry = h0.number.toNumber() + 20;
    const auth = await contractCallB1(api, contract, evidence, 'os2b_auth', 'createTransferMetaAuthorization', accounts.OWNER, [
      assetId, marketId,
      { pricePolicy: pricePolicy(price), allowedBuyer: attacker.address, expiry, nonce: 0 },
    ]);
    const authId = hash32Hex(auth.dry.decoded.value, 'auth');
    const signed = buildSignedOrder(contract, api.genesisHash.toHex(), accounts.OWNER, {
      marketId, operation: 'TransferMeta', objectType: 'MetaToken', objectId: assetId,
      authorizationId: authId, allowedBuyer: accountHex(attacker), price, policyHash: pricePolicyHash(price), expiry,
    });
    const create = await probeCall(api, contract, 'os2b_create', 'createOrder', accounts.RELAYER, [
      signed.order, 'Authorization', signed.sellerCommitment, signed.signature,
    ]);
    assert(create.ok, `os2b create failed: ${JSON.stringify(create)}`);
    await waitForOrderState(contract, accounts.OWNER, signed.orderId, 'Open');
    const cancel = await probeCall(api, contract, 'os2b_cancel', 'cancelOpen', accounts.OWNER, [signed.orderId]);
    const buyerSigned = buildSignedBuyer(contract, api.genesisHash.toHex(), attacker, signed, 3);
    const fund = await probeCall(api, contract, 'os2b_fund', 'fundOrder', attacker, [
      signed.orderId, buyerSigned.commitment, buyerSigned.signature,
    ], { value: signed.order.price });
    const settle = await probeCall(api, contract, 'os2b_settle', 'settle', attacker, [signed.orderId]);
    const snap = await snapshotOrder(contract, accounts.OWNER, signed.orderId);
    results.os2b = {
      scenario: 'OS-2b (control) on-chain cancel_open, then fund/settle attempts',
      expiry, createBlock: create.block, cancel, fund, settle,
      orderState: summarizeOrder(snap.order),
      settlementRecord: snap.record ? field(snap.record, 'orderId', 'order_id') : null,
    };
    console.log('RESULT_OS2B', JSON.stringify(results.os2b));
  }

  console.log('SUMMARY', JSON.stringify({
    os1a_attack_success: results.os1a.settle.ok === true,
    os1b_settle_rejected: results.os1b.settle.ok !== true,
    os1b_error: results.os1b.settle.error,
    os2a_attack_success: results.os2a.settle.ok === true,
    os2b_settle_rejected: results.os2b.settle.error === 'TerminalOrder',
  }));
  process.exit(0);
}

main().catch((error) => {
  console.error('PROBE_FAILED', error && error.stack ? error.stack : String(error));
  process.exit(1);
});
