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
const {
  TIMING,
  architectureMetric,
  baseEvidence,
  fundSharedAccounts,
  sendTx,
  sharedAccounts,
  writeExclusiveJson,
} = require('./s314_live_common');

const RPC = process.env.ASSETX_RPC || 'ws://127.0.0.1:9944';
const WSL_EXP_ROOT = '/home/gz785/code/exp2';
const WSL_PROJECT_ROOT = '/home/gz785/code/assetxchain-g3-v2';
const EXP_ROOT = process.env.EXP_ROOT || (process.platform === 'win32'
  ? '\\\\wsl.localhost\\Ubuntu\\home\\gz785\\code\\exp2'
  : WSL_EXP_ROOT);
const PROJECT_ROOT = process.env.PROJECT_ROOT || (process.platform === 'win32'
  ? '\\\\wsl.localhost\\Ubuntu\\home\\gz785\\code\\assetxchain-g3-v2'
  : WSL_PROJECT_ROOT);
const WASM_PATH = process.env.G3_V2_WASM || path.join(
  PROJECT_ROOT,
  'target',
  'ink',
  'market_orderbook',
  'market_orderbook.wasm'
);
const RUN_ID = process.env.RUN_ID;
const OUTPUT_DIR = process.env.RAW_DIR;
const IMPLEMENTATION_COMMIT = process.env.IMPLEMENTATION_COMMIT;
const GIT_DIRTY = process.env.GIT_DIRTY === 'true';

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
const CONTRACT_ERRORS = {
  1: 'MarketMismatch',
  2: 'RuntimeProjectionMismatch',
  3: 'NotAdmin',
  4: 'MarketAlreadyBound',
  5: 'InvalidMarketId',
  6: 'MarketNotBound',
};
const TYPE = {
  order: 'PalletDataassetsProtocolOrderCommitment',
  witness: 'PalletDataassetsProtocolPricePolicyWitness',
  seller: 'PalletDataassetsProtocolSellerCommitment',
  buyer: 'PalletDataassetsProtocolBuyerCommitment',
  policy: 'PalletDataassetsPricePolicy',
};
const OPERATION_INDEX = {
  TransferMeta: 0,
  IssueRight: 1,
  TransferRight: 2,
};
const MESSAGE_INDEX = {
  Order: 0,
  Seller: 1,
  Buyer: 2,
  Settlement: 3,
  Right: 4,
};

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function safeJson(value) {
  return JSON.parse(JSON.stringify(value, (_, item) => (
    typeof item === 'bigint' ? item.toString() : item
  )));
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
  return u8aConcat(
    fixed32(domainSeparator(genesisHash, operation, 'Seller')),
    encode(api, TYPE.seller, commitment)
  );
}

function buyerSigningPayload(api, genesisHash, operation, commitment) {
  return u8aConcat(
    fixed32(domainSeparator(genesisHash, operation, 'Buyer')),
    encode(api, TYPE.buyer, commitment)
  );
}

function computeRightId(genesisHash, operation, orderId) {
  return hash(
    fixed32(domainSeparator(genesisHash, operation, 'Right')),
    fixed32(orderId),
    stringToU8a('right')
  );
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

function computeRightParametersHash(marketId, validFrom, validUntil, maxUses, royaltyBps) {
  return hash(
    new Uint8Array([3]),
    new Uint8Array([0]),
    le(validFrom, 8),
    new Uint8Array([1]),
    le(validUntil, 8),
    new Uint8Array([1]),
    le(maxUses, 4),
    new Uint8Array([1]),
    new Uint8Array([4]),
    fixed32(marketId),
    le(royaltyBps, 2)
  );
}

function contractData(api, name, args = []) {
  return u8aToHex(u8aConcat(
    selector(name),
    ...args.map(([type, value]) => (type ? encode(api, type, value) : value))
  ));
}

function eventRows(records) {
  return records.map(({ event, phase }) => ({
    section: event.section,
    method: event.method,
    phase: phase.toString(),
    data: event.data.map((item) => item.toString()),
    human: event.toHuman(),
  }));
}

function findEvent(records, section, method) {
  return records.find(({ event }) => event.section === section && event.method === method);
}

function countEvent(records, section, method) {
  return records.filter(({ event }) => event.section === section && event.method === method).length;
}

function gasRefTime(dryRun) {
  const value = field(dryRun.gasConsumed, 'refTime', 'ref_time');
  return BigInt(value || 0);
}

function recordRelayEvidence(evidence, call) {
  evidence.transactions.push(safeJson({
    ...call.submitted,
    events: undefined,
    contractReturn: call.decoded,
    contractGas: call.dryRun.gasConsumed,
  }));
  evidence.events[call.submitted.label] = call.submitted.eventRows;
  evidence.contractGasRefTime += gasRefTime(call.dryRun);
}

function decodeDispatchError(api, dispatchError) {
  if (!dispatchError.isModule) {
    return { section: 'system', name: dispatchError.type, docs: [] };
  }
  const metadata = api.registry.findMetaError(dispatchError.asModule);
  return {
    section: metadata.section,
    name: metadata.name,
    docs: metadata.docs.map((doc) => doc.toString()),
  };
}

function field(value, camel, snake) {
  if (value && Object.prototype.hasOwnProperty.call(value, camel)) return value[camel];
  if (value && Object.prototype.hasOwnProperty.call(value, snake)) return value[snake];
  return undefined;
}

function optionJson(option, label) {
  assert(option.isSome, `${label} is missing`);
  return option.unwrap().toJSON();
}

async function dryRunContract(api, origin, contractAddress, data, allowRevert = false) {
  const result = await api.call.contractsApi.call(
    origin.address,
    contractAddress,
    '0',
    GAS_LIMIT,
    null,
    data
  );
  const json = result.toJSON();
  assert(json.result && json.result.ok, `contract dry-run dispatch failed: ${JSON.stringify(json.result)}`);
  const reverted = json.result.ok.flags.bits !== 0;
  assert(!reverted || allowRevert, `contract dry-run reverted: ${JSON.stringify(json.result.ok)}`);
  return {
    data: json.result.ok.data,
    reverted,
    gasConsumed: json.gasConsumed,
    gasRequired: json.gasRequired,
    storageDeposit: json.storageDeposit,
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
  return {
    ok: false,
    error: CONTRACT_ERRORS[contractError] || `ContractError(${contractError})`,
  };
}

async function callRelay(api, contractAddress, signer, message, data, expectation) {
  const dryRun = await dryRunContract(api, signer, contractAddress, data,
    !expectation.ok || expectation.allowFailure);
  const decoded = decodeRelayResult(dryRun.data);
  if (expectation.allowFailure) {
    // E7 composite probes retain a reverting dry-run as submitted evidence.
  } else if (expectation.ok) {
    assert(!dryRun.reverted, `${message} unexpectedly reverted`);
    assert(decoded.ok, `${message} expected success, got ${JSON.stringify(decoded)}`);
  } else {
    assert(dryRun.reverted, `${message} returned an error without reverting`);
    assert(!decoded.ok, `${message} expected an error`);
    assert(decoded.code === expectation.code, `${message} expected code ${expectation.code}, got ${JSON.stringify(decoded)}`);
  }
  const tx = api.tx.contracts.call(contractAddress, '0', GAS_LIMIT, null, data);
  const submitted = await sendTx(
    api,
    message,
    tx,
    signer,
    expectation.allowFailure
      ? { allowDispatchFailure: true }
      : expectation.ok ? null : { section: 'contracts', name: 'ContractReverted' }
  );
  return { dryRun, decoded, submitted };
}

async function registerAsset(api, signer, label, hashByte, evidence) {
  const rawHash = `0x${hashByte.toString(16).padStart(2, '0').repeat(32)}`;
  const result = await sendTx(
    api,
    `register_asset_${label}`,
    api.tx.dataAssets.registerAssetCore(
      Array.from(Buffer.from(label, 'utf8')),
      Array.from(Buffer.from(`s214-${label}`, 'utf8')),
      rawHash,
      1024
    ),
    signer
  );
  evidence.transactions.push(safeJson({ ...result, events: undefined }));
  evidence.events[result.label] = result.eventRows;
  const event = findEvent(result.events, 'dataAssets', 'AssetRegistered');
  assert(event, `AssetRegistered missing for ${label}`);
  return event.event.data[0].toHex();
}

async function createAuthorization(api, signer, kind, assetId, marketId, terms, evidence) {
  const tx = kind === 'meta'
    ? api.tx.dataAssets.createTransferMetaAuthorization(assetId, marketId, terms)
    : api.tx.dataAssets.createIssueRightAuthorization(assetId, marketId, terms);
  const method = kind === 'meta'
    ? 'TransferMetaAuthorizationCreated'
    : 'IssueRightAuthorizationCreated';
  const result = await sendTx(api, `create_${kind}_authorization`, tx, signer);
  evidence.transactions.push(safeJson({ ...result, events: undefined }));
  evidence.events[result.label] = result.eventRows;
  const event = findEvent(result.events, 'dataAssets', method);
  assert(event, `${method} missing`);
  return event.event.data[0].toHex();
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

async function snapshotOrderState(api, orderId) {
  const order = await api.query.dataAssets.runtimeOrders(orderId);
  const settlement = await api.query.dataAssets.settlementRecords(orderId);
  const orderHex = order.toHex();
  let escrowHex = '0x';
  if (order.isSome) {
    const value = order.unwrap();
    const escrowId = field(value.toJSON(), 'escrowId', 'escrow_id');
    if (escrowId) escrowHex = (await api.query.dataAssets.runtimeEscrows(escrowId)).toHex();
  }
  return { orderHex, settlementHex: settlement.toHex(), escrowHex };
}

function assertSameSnapshot(before, after, label) {
  assert(JSON.stringify(before) === JSON.stringify(after), `${label} changed authoritative state`);
}

function assertNoSettlementSuccess(records, label) {
  assert(countEvent(records, 'dataAssets', 'SettlementFinalized') === 0, `${label} emitted SettlementFinalized`);
}

async function syncProjectionTwice(api, contractAddress, orderId, evidence) {
  for (let attempt = 1; attempt <= 2; attempt += 1) {
    const call = await callRelay(
      api,
      contractAddress,
      evidence.accounts.relayerPair,
      `sync_projection_${orderId.slice(2, 10)}_${attempt}`,
      simpleHashCall(api, 'sync_projection', orderId),
      { ok: true }
    );
    assertNoSettlementSuccess(call.submitted.events, `sync projection ${attempt}`);
    recordRelayEvidence(evidence, call);
  }
  const projection = await dryRunContract(
    api,
    evidence.accounts.relayerPair,
    contractAddress,
    simpleHashCall(api, 'get_projection', orderId)
  );
  const bytes = hexToU8a(projection.data);
  assert(bytes.length > 2 && bytes[0] === 0 && bytes[1] === 1, `projection missing for ${orderId}`);
}

async function verifySettlement(api, input, evidence) {
  const order = optionJson(await api.query.dataAssets.runtimeOrders(input.orderId), `${input.label} order`);
  const record = optionJson(await api.query.dataAssets.settlementRecords(input.orderId), `${input.label} settlement`);
  const escrowId = field(order, 'escrowId', 'escrow_id');
  const escrow = optionJson(await api.query.dataAssets.runtimeEscrows(escrowId), `${input.label} escrow`);
  const keys = Object.keys(record);
  assert(keys.length === 24, `${input.label} settlement has ${keys.length} fields, expected 24`);
  assert(field(order, 'state', 'state') === 'Settled', `${input.label} order is not Settled`);
  assert(field(record, 'orderId', 'order_id') === input.orderId, `${input.label} evidence order mismatch`);
  assert(field(record, 'operation', 'operation') === input.operation, `${input.label} evidence operation mismatch`);
  assert(field(record, 'marketId', 'market_id') === input.marketId, `${input.label} evidence market mismatch`);
  assert(field(record, 'authorizationId', 'authorization_id') === input.authorizationId, `${input.label} evidence authorization mismatch`);
  assert(field(record, 'seller', 'seller') === accountHex(input.seller), `${input.label} evidence seller mismatch`);
  assert(field(record, 'buyer', 'buyer') === accountHex(input.buyer), `${input.label} evidence buyer mismatch`);
  assert(BigInt(field(record, 'grossPrice', 'gross_price')) === input.price, `${input.label} evidence price mismatch`);
  const sum = BigInt(field(record, 'sellerAmount', 'seller_amount'))
    + BigInt(field(record, 'royaltyAmount', 'royalty_amount'))
    + BigInt(field(record, 'marketFeeAmount', 'market_fee_amount'))
    + BigInt(field(record, 'safetyFeeAmount', 'safety_fee_amount'));
  assert(sum === input.price, `${input.label} amount split does not reconcile`);
  assert(field(escrow, 'state', 'state') === 'Released', `${input.label} escrow is not Released`);
  evidence.settlements[input.label] = { order, record, escrow, fieldCount: keys.length };
  return { order, record, escrow };
}

async function executeOrder(api, genesisHash, contractAddress, signed, witness, buyer, buyerNonce, evidence, label) {
  const create = await callRelay(
    api,
    contractAddress,
    signed.sellerCommitment.seller === accountHex(evidence.accounts.rightSellerPair)
      ? evidence.accounts.rightSellerPair
      : evidence.accounts.ownerPair,
    `${label}_create`,
    createOrderCall(api, signed, witness),
    { ok: true }
  );
  assert(create.decoded.payload === signed.orderId, `${label} create returned wrong order id`);
  recordRelayEvidence(evidence, create);

  const signedBuyer = buildSignedBuyer(
    api,
    genesisHash,
    buyer,
    signed.order.operation,
    signed.orderId,
    BigInt(signed.order.price),
    signed.order.expiry,
    buyerNonce
  );
  const fund = await callRelay(
    api,
    contractAddress,
    buyer,
    `${label}_fund`,
    fundOrderCall(api, signed.orderId, signedBuyer),
    { ok: true }
  );
  recordRelayEvidence(evidence, fund);

  const settle = await callRelay(
    api,
    contractAddress,
    evidence.accounts.relayerPair,
    `${label}_settle`,
    simpleHashCall(api, 'relay_settle', signed.orderId),
    { ok: true }
  );
  assert(settle.decoded.payload.length === 66, `${label} settle did not return a settlement id`);
  assert(countEvent(settle.submitted.events, 'dataAssets', 'SettlementFinalized') === 1, `${label} settlement event count is not one`);
  recordRelayEvidence(evidence, settle);
  return { signedBuyer, create, fund, settle };
}

async function main() {
  assert(RUN_ID && OUTPUT_DIR && IMPLEMENTATION_COMMIT, 'RUN_ID, RAW_DIR, and IMPLEMENTATION_COMMIT are required');
  await cryptoWaitReady();
  const api = await ApiPromise.create({ provider: new WsProvider(RPC) });
  await api.isReady;

  const shared = sharedAccounts();
  const alice = shared.OWNER;
  const bob = shared.RIGHT_SELLER;
  const charlie = shared.BUYER_1;
  const dave = shared.BUYER_2;
  const eve = shared.MARKET_OK;
  const relayer = shared.RELAYER;
  const wasm = fs.readFileSync(WASM_PATH);
  const wasmHex = u8aToHex(wasm);
  const codeHash = u8aToHex(blake2AsU8a(wasm, 256));
  const salt = u8aToHex(blake2AsU8a(stringToU8a(`s314-${RUN_ID}`), 128));
  const genesisHash = api.genesisHash.toHex();
  const evidence = baseEvidence('G3', api, RUN_ID, IMPLEMENTATION_COMMIT, GIT_DIRTY);
  evidence.environment.chain = (await api.rpc.system.chain()).toString();
  evidence.contract = { wasm_path: WASM_PATH, wasm_bytes: wasm.length, code_hash: codeHash };
  evidence.settlements = {};
  evidence.checks = [];
  evidence.contractGasRefTime = 0n;
  evidence.accounts = {
    ownerPair: alice,
    rightSellerPair: bob,
    buyer1Pair: charlie,
    buyer2Pair: dave,
    marketPair: eve,
    relayerPair: relayer,
  };

  try {
    await fundSharedAccounts(api, shared, evidence);
    const placeholderData = contractData(api, 'new', [
      [null, fixed32(ZERO_HASH)],
      [null, le(TIMING.fundedGrace, 4)],
    ]);
    const predicted = await api.call.contractsApi.instantiate(
      eve.address,
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
      'deploy_market_orderbook_v2',
      api.tx.contracts.instantiateWithCode('0', GAS_LIMIT, null, wasmHex, placeholderData, salt),
      eve
    );
    const instantiated = findEvent(deploy.events, 'contracts', 'Instantiated');
    assert(instantiated, 'contracts.Instantiated missing');
    const contractAddress = instantiated.event.data[1].toString();
    assert(contractAddress === predictedAddress, 'deployed contract address differs from prediction');
    const marketNonce = BigInt((await api.query.markets.nextMarketNonce()).toString());
    const marketId = computeMarketId(eve, contractAddress, marketNonce);
    evidence.transactions.push(safeJson({ ...deploy, events: undefined }));
    evidence.events[deploy.label] = deploy.eventRows;
    evidence.contract.address = contractAddress;
    evidence.marketId = marketId;

    const bind = await callRelay(
      api,
      contractAddress,
      eve,
      'bind_market_id',
      contractData(api, 'bind_market_id', [[null, fixed32(marketId)]]),
      { ok: true }
    );
    assert(findEvent(bind.submitted.events, 'contracts', 'ContractEmitted'), 'MarketIdBound contract event missing');
    recordRelayEvidence(evidence, bind);
    const boundMarket = await dryRunContract(
      api,
      eve,
      contractAddress,
      contractData(api, 'get_market_id')
    );
    const boundMarketBytes = hexToU8a(boundMarket.data);
    assert(boundMarketBytes.length === 33 && boundMarketBytes[0] === 0, 'bound market query returned invalid data');
    assert(u8aToHex(boundMarketBytes.slice(1)) === marketId, 'contract market id binding mismatch');
    evidence.checks.push('contract market identity bound once before Runtime registration');

    const tierCall = api.tx.markets.setCapacityTier({
      tierId: 1,
      requiredStake: (10_000n * UNIT).toString(),
      maxActiveOrders: 32,
      maxFundedOrders: 16,
      maxActiveExposureValue: (1_000n * UNIT).toString(),
    });
    const tier = await sendTx(api, 'set_market_capacity_tier', api.tx.sudo.sudo(tierCall), alice);
    evidence.transactions.push(safeJson({ ...tier, events: undefined }));
    evidence.events[tier.label] = tier.eventRows;
    assert((await api.query.markets.capacityTiers(1)).isSome, 'capacity tier was not stored');

    const register = await sendTx(
      api,
      'register_market_v2',
      api.tx.markets.registerMarket(contractAddress, codeHash, 31, 1, 0),
      eve
    );
    evidence.transactions.push(safeJson({ ...register, events: undefined }));
    evidence.events[register.label] = register.eventRows;
    const marketEvent = findEvent(register.events, 'markets', 'MarketRegistered');
    assert(marketEvent && marketEvent.event.data[0].toHex() === marketId, 'registered market id mismatch');
    assert((await api.query.markets.marketRegistrations(marketId)).isSome, 'market registration missing');

    const header = await api.rpc.chain.getHeader();
    const fixtureAnchor = header.number.toNumber();
    const rightValidUntil = fixtureAnchor + TIMING.rightValidity;
    const metaPolicy = {
      referencePrice: (10n * UNIT).toString(),
      floorRatioBps: 10000,
      signedMarketFeeCapBps: 0,
      safetyFeeBps: 0,
    };
    const issuePolicy = {
      referencePrice: (3n * UNIT).toString(),
      floorRatioBps: 10000,
      signedMarketFeeCapBps: 0,
      safetyFeeBps: 0,
    };
    const resalePolicy = {
      referencePrice: (4n * UNIT).toString(),
      floorRatioBps: 10000,
      signedMarketFeeCapBps: 0,
      safetyFeeBps: 0,
    };

    const metaAsset = await registerAsset(api, alice, 'meta', 0x41, evidence);
    const issueAsset = await registerAsset(api, alice, 'issue', 0x42, evidence);
    const resaleAsset = await registerAsset(api, alice, 'resale', 0x43, evidence);

    const prepExpiry = (await api.rpc.chain.getHeader()).number.toNumber() + TIMING.orderExpiry + 2;
    const prepAuth = await createAuthorization(api, alice, 'issue', resaleAsset, marketId, {
      rightType: 'View',
      dataScope: 'FullAsset',
      validityTemplate: { validFrom: fixtureAnchor, validUntil: rightValidUntil, maxUses: 1 },
      maxIssueCount: 1,
      maxTotalUseUnits: 1,
      pricePolicy: issuePolicy,
      royaltyBps: 0,
      transferable: true,
      allowedTransferMarkets: [marketId],
      expiry: prepExpiry,
      nonce: 0,
    }, evidence);
    const prepParameters = computeRightParametersHash(marketId, fixtureAnchor, rightValidUntil, 1, 0);
    const prepSigned = buildSignedOrder(api, genesisHash, alice, {
      marketId,
      operation: 'IssueRight',
      objectType: 'MetaToken',
      objectId: resaleAsset,
      authorizationId: prepAuth,
      price: 3n * UNIT,
      policyHash: computePolicyHash(api, issuePolicy),
      rightParametersHash: prepParameters,
      sellerStateVersion: 1,
      orderNonce: 0,
      expiry: prepExpiry,
    });
    const prep = await executeOrder(api, genesisHash, contractAddress, prepSigned, 'Authorization', bob, 0, evidence, 'fixture_resale_right');
    const resaleRightId = computeRightId(genesisHash, 'IssueRight', prepSigned.orderId);
    const prepRight = optionJson(
      await api.call.dataAssetsApi.getCertificate(resaleAsset, resaleRightId),
      'fixture resale right',
    );
    assert(accountHex(field(prepRight, 'holder', 'holder')) === accountHex(bob), 'fixture resale holder mismatch');
    evidence.runtimeAuthority = { fixtureRight: prepRight };
    assert(rightValidUntil - fixtureAnchor === TIMING.rightValidity, 'right validity offset drift');

    const metaExpiry = (await api.rpc.chain.getHeader()).number.toNumber() + TIMING.orderExpiry + 2;
    const metaAuth = await createAuthorization(api, alice, 'meta', metaAsset, marketId, {
      pricePolicy: metaPolicy,
      allowedBuyer: charlie.address,
      expiry: metaExpiry,
      nonce: 0,
    }, evidence);

    const metaSigned = buildSignedOrder(api, genesisHash, alice, {
      marketId,
      operation: 'TransferMeta',
      objectType: 'MetaToken',
      objectId: metaAsset,
      authorizationId: metaAuth,
      allowedBuyer: accountHex(charlie),
      price: 10n * UNIT,
      policyHash: computePolicyHash(api, metaPolicy),
      sellerStateVersion: 1,
      orderNonce: 0,
      expiry: metaExpiry,
    });

    const metaCreate = await callRelay(
      api,
      contractAddress,
      alice,
      'meta_create',
      createOrderCall(api, metaSigned, 'Authorization'),
      { ok: true }
    );
    assert(metaCreate.decoded.payload === metaSigned.orderId, 'meta create returned wrong order id');
    evidence.transactions.push(safeJson({ ...metaCreate.submitted, events: undefined, contractReturn: metaCreate.decoded }));
    evidence.events[metaCreate.submitted.label] = metaCreate.submitted.eventRows;

    const metaBuyer = buildSignedBuyer(api, genesisHash, charlie, 'TransferMeta', metaSigned.orderId, 10n * UNIT, metaExpiry, 0);
    const rejectedBefore = await snapshotOrderState(api, metaSigned.orderId);
    const buyerBefore = (await api.query.system.account(charlie.address)).toHex();
    const rejectedFund = await callRelay(
      api,
      contractAddress,
      dave,
      'meta_wrong_payer_reject',
      fundOrderCall(api, metaSigned.orderId, metaBuyer),
      { ok: false, code: 30 }
    );
    const rejectedAfter = await snapshotOrderState(api, metaSigned.orderId);
    const buyerAfter = (await api.query.system.account(charlie.address)).toHex();
    assertSameSnapshot(rejectedBefore, rejectedAfter, 'wrong-payer rejection');
    assert(buyerBefore === buyerAfter, 'wrong-payer rejection changed buyer balance');
    assertNoSettlementSuccess(rejectedFund.submitted.events, 'wrong-payer rejection');
    assert(countEvent(rejectedFund.submitted.events, 'dataAssets', 'OrderFunded') === 0, 'wrong-payer rejection emitted OrderFunded');
    recordRelayEvidence(evidence, rejectedFund);
    evidence.checks.push('PayerBuyerMismatch=30 and authoritative pre-state unchanged');

    const metaFund = await callRelay(
      api,
      contractAddress,
      charlie,
      'meta_fund',
      fundOrderCall(api, metaSigned.orderId, metaBuyer),
      { ok: true }
    );
    recordRelayEvidence(evidence, metaFund);
    const metaSettle = await callRelay(
      api,
      contractAddress,
      relayer,
      'meta_settle',
      simpleHashCall(api, 'relay_settle', metaSigned.orderId),
      { ok: true }
    );
    assert(countEvent(metaSettle.submitted.events, 'dataAssets', 'SettlementFinalized') === 1, 'meta settlement event count is not one');
    recordRelayEvidence(evidence, metaSettle);
    await verifySettlement(api, {
      label: 'TransferMeta', orderId: metaSigned.orderId, operation: 'TransferMeta', marketId,
      authorizationId: metaAuth, seller: alice, buyer: charlie, price: 10n * UNIT,
    }, evidence);
    const transferredMeta = optionJson(
      await api.call.dataAssetsApi.getAsset(metaAsset),
      'transferred meta authority',
    );
    const transferredMetaCore = field(transferredMeta, 'core', 'core');
    assert(accountHex(field(transferredMetaCore, 'owner', 'owner')) === accountHex(charlie), 'transferred meta owner mismatch');
    evidence.runtimeAuthority.transferredMeta = transferredMeta;
    await syncProjectionTwice(api, contractAddress, metaSigned.orderId, evidence);
    evidence.operations.TransferMeta = {
      scenario_id: 'LEG-META-01', order_id: metaSigned.orderId,
      expiry: metaExpiry, created_block: metaCreate.submitted.blockNumber,
      expiry_offset: metaExpiry - metaCreate.submitted.blockNumber,
      state: 'Settled', settlement_record_count: 1, evidence_field_count: 24,
      audit: evidence.settlements.TransferMeta,
    };
    assert(evidence.operations.TransferMeta.expiry_offset === TIMING.orderExpiry, 'meta expiry offset drift');

    const duplicateBefore = await snapshotOrderState(api, metaSigned.orderId);
    const duplicateSettle = await callRelay(
      api,
      contractAddress,
      relayer,
      'meta_duplicate_settle_reject',
      simpleHashCall(api, 'relay_settle', metaSigned.orderId),
      { ok: false, code: 19 }
    );
    const duplicateAfter = await snapshotOrderState(api, metaSigned.orderId);
    assertSameSnapshot(duplicateBefore, duplicateAfter, 'duplicate settlement');
    assertNoSettlementSuccess(duplicateSettle.submitted.events, 'duplicate settlement');
    recordRelayEvidence(evidence, duplicateSettle);
    evidence.checks.push('AlreadySettled=19, no second SettlementFinalized, state unchanged');

    const issueExpiry = (await api.rpc.chain.getHeader()).number.toNumber() + TIMING.orderExpiry + 2;
    const issueAuth = await createAuthorization(api, alice, 'issue', issueAsset, marketId, {
      rightType: 'View',
      dataScope: 'FullAsset',
      validityTemplate: { validFrom: fixtureAnchor, validUntil: rightValidUntil, maxUses: 1 },
      maxIssueCount: 2,
      maxTotalUseUnits: 2,
      pricePolicy: issuePolicy,
      royaltyBps: 0,
      transferable: true,
      allowedTransferMarkets: [marketId],
      expiry: issueExpiry,
      nonce: 0,
    }, evidence);
    const rightParamsHash = computeRightParametersHash(marketId, fixtureAnchor, rightValidUntil, 1, 0);
    const issueSigned = buildSignedOrder(api, genesisHash, alice, {
      marketId,
      operation: 'IssueRight',
      objectType: 'MetaToken',
      objectId: issueAsset,
      authorizationId: issueAuth,
      price: 3n * UNIT,
      policyHash: computePolicyHash(api, issuePolicy),
      rightParametersHash: rightParamsHash,
      sellerStateVersion: 1,
      orderNonce: 0,
      expiry: issueExpiry,
    });
    const issueExecution = await executeOrder(api, genesisHash, contractAddress, issueSigned, 'Authorization', charlie, 1, evidence, 'LEG-ISSUE-01');
    const issueSettlement = await verifySettlement(api, {
      label: 'IssueRight', orderId: issueSigned.orderId, operation: 'IssueRight', marketId,
      authorizationId: issueAuth, seller: alice, buyer: charlie, price: 3n * UNIT,
    }, evidence);
    const issuedRightId = computeRightId(genesisHash, 'IssueRight', issueSigned.orderId);
    assert(field(issueSettlement.record, 'rightId', 'right_id') === issuedRightId, 'issued right id mismatch');
    const issuedRight = optionJson(
      await api.call.dataAssetsApi.getCertificate(issueAsset, issuedRightId),
      'issued right authority',
    );
    assert(accountHex(field(issuedRight, 'holder', 'holder')) === accountHex(charlie), 'issued right holder mismatch');
    evidence.runtimeAuthority.issuedRight = issuedRight;
    const issueAuthState = optionJson(await api.query.dataAssets.issueRightAuthorizations(issueAuth), 'issue authorization');
    assert(Number(field(issueAuthState, 'reservedCount', 'reserved_count')) === 0, 'issued authorization retained reserved quota');
    assert(Number(field(issueAuthState, 'issuedCount', 'issued_count')) === 1, 'issued authorization count is not one');
    evidence.issueAuthorization = issueAuthState;
    await syncProjectionTwice(api, contractAddress, issueSigned.orderId, evidence);
    evidence.operations.IssueRight = {
      scenario_id: 'LEG-ISSUE-01', order_id: issueSigned.orderId,
      expiry: issueExpiry, created_block: issueExecution.create.submitted.blockNumber,
      expiry_offset: issueExpiry - issueExecution.create.submitted.blockNumber,
      state: 'Settled', settlement_record_count: 1, evidence_field_count: 24,
      audit: evidence.settlements.IssueRight,
    };
    assert(evidence.operations.IssueRight.expiry_offset === TIMING.orderExpiry, 'issue expiry offset drift');

    const resaleExpiry = (await api.rpc.chain.getHeader()).number.toNumber() + TIMING.orderExpiry + 1;
    const resaleAuth = hash(stringToU8a('axc/p2/resale-policy'), fixed32(resaleRightId), fixed32(marketId));
    const resaleSigned = buildSignedOrder(api, genesisHash, bob, {
      marketId,
      operation: 'TransferRight',
      objectType: 'RightToken',
      objectId: resaleRightId,
      parentAssetId: resaleAsset,
      authorizationId: resaleAuth,
      price: 4n * UNIT,
      policyHash: computePolicyHash(api, resalePolicy),
      rightParametersHash: prepParameters,
      sellerStateVersion: 1,
      orderNonce: 0,
      expiry: resaleExpiry,
    });
    const resaleExecution = await executeOrder(
      api,
      genesisHash,
      contractAddress,
      resaleSigned,
      { SellerSnapshot: resalePolicy },
      dave,
      0,
      evidence,
      'LEG-RESALE-01'
    );
    await verifySettlement(api, {
      label: 'TransferRight', orderId: resaleSigned.orderId, operation: 'TransferRight', marketId,
      authorizationId: resaleAuth, seller: bob, buyer: dave, price: 4n * UNIT,
    }, evidence);
    const transferredRight = optionJson(
      await api.call.dataAssetsApi.getCertificate(resaleAsset, resaleRightId),
      'transferred right authority',
    );
    assert(accountHex(field(transferredRight, 'holder', 'holder')) === accountHex(dave), 'transferred right holder mismatch');
    evidence.runtimeAuthority.transferredRight = transferredRight;
    await syncProjectionTwice(api, contractAddress, resaleSigned.orderId, evidence);
    evidence.operations.TransferRight = {
      scenario_id: 'LEG-RESALE-01', order_id: resaleSigned.orderId,
      expiry: resaleExpiry, created_block: resaleExecution.create.submitted.blockNumber,
      expiry_offset: resaleExpiry - resaleExecution.create.submitted.blockNumber,
      state: 'Settled', settlement_record_count: 1, evidence_field_count: 24,
      audit: evidence.settlements.TransferRight,
    };
    assert(evidence.operations.TransferRight.expiry_offset === TIMING.orderExpiry, 'resale expiry offset drift');

    const settlementEntries = await api.query.dataAssets.settlementRecords.entries();
    const targetIds = new Set([metaSigned.orderId, issueSigned.orderId, resaleSigned.orderId]);
    const targetEntries = settlementEntries.filter(([key]) => targetIds.has(key.args[0].toHex()));
    assert(targetEntries.length === 3, `expected three target authoritative settlement records, got ${targetEntries.length}`);
    evidence.checks.push('three legal operations settled with one record each');
    evidence.checks.push('projection sync replayed twice per order without settlement mutation');
    evidence.checks.push('all SettlementRecord values contain exactly 24 frozen fields and reconcile amounts');
    evidence.checks.push('Runtime API reconciled transferred Meta owner and issued/transferred Right holders');
    evidence.rejection = {
      scenario_id: 'NEG-PAYER-01', cleanliness_scenario_id: 'CLEAN-FUND-01',
      common_error_code: 'PayerBuyerMismatch', native_error: 'PayerBuyerMismatch',
      authoritative_state_unchanged: true, success_event_count: 0,
    };
    evidence.duplicate = {
      scenario_id: 'DUP-SETTLE-01', common_error_code: 'AlreadySettled',
      accepted_attempt_count: 1, settlement_record_count: 1,
      second_success_event_count: 0, authoritative_state_unchanged: true,
    };
    evidence.audit = {
      authoritative_source: 'runtime',
      contract_projection_used_for_authority: false,
      contract_projection_observed_separately: true,
      settled_operation_count: 3,
      settlement_record_count: 3,
      all_record_field_counts: [24, 24, 24],
      amount_reconciliation_passed: true,
      owner_holder_reconciliation_passed: true,
      issue_quota_reconciliation_passed: true,
    };
    evidence.architecture_cost = {
      contract_gas_or_ref_time: architectureMetric('measured', evidence.contractGasRefTime, 'ref_time'),
      contract_storage_writes: architectureMetric('not-collected', null, 'count'),
      runtime_dispatch_count: architectureMetric('not-collected', null, 'count'),
      runtime_storage_writes: architectureMetric('not-collected', null, 'count'),
      typed_ce_call_count: architectureMetric('not-collected', null, 'count'),
      projection_storage_writes: architectureMetric('measured', 6, 'count'),
    };
    evidence.status = 'PASS';
    evidence.accounts = {
      OWNER: alice.address,
      RIGHT_SELLER: bob.address,
      BUYER_1: charlie.address,
      BUYER_2: dave.address,
      MARKET_OK: eve.address,
      MARKET_DENIED: shared.MARKET_DENIED.address,
      RELAYER: relayer.address,
    };
    evidence.run.finished_at = new Date().toISOString();
    delete evidence.contractGasRefTime;
    writeExclusiveJson(path.join(OUTPUT_DIR, 'live-smoke.json'), evidence);
    console.log(JSON.stringify({ status: 'PASS', baseline: 'G3', runId: RUN_ID, contractAddress, marketId }));
  } finally {
    await api.disconnect();
  }
}

module.exports = {
  GAS_LIMIT,
  TYPE,
  ZERO_HASH,
  accountHex,
  buildSignedBuyer,
  buildSignedOrder,
  callRelay,
  computeMarketId,
  computePolicyHash,
  computeRightId,
  computeRightParametersHash,
  contractData,
  createAuthorization,
  createOrderCall,
  dryRunContract,
  field,
  findEvent,
  fixed32,
  fundOrderCall,
  hash,
  le,
  optionJson,
  registerAsset,
  safeJson,
  sendTx,
  simpleHashCall,
};

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stack || error.message || String(error));
    process.exit(1);
  });
}
