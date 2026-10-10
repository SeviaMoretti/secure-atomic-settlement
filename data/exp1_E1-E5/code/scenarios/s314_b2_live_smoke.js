'use strict';

const path = require('path');
const {
  PRICES,
  PROTOCOL_VERSION,
  TIMING,
  accountHex,
  architectureMetric,
  assert,
  baseEvidence,
  buyerSigningPayload,
  computeOrderId,
  computeResaleAuthorizationId,
  computeRightId,
  connect,
  countEvent,
  findEvent,
  fundSharedAccounts,
  hash,
  recordTx,
  sellerSigningPayload,
  sendTx,
  sharedAccounts,
  writeExclusiveJson,
} = require('./s314_live_common');

const RPC = process.env.ASSETX_RPC || 'ws://127.0.0.1:9944';
const RUN_ID = process.env.RUN_ID;
const RAW_DIR = process.env.RAW_DIR;
const IMPLEMENTATION_COMMIT = process.env.IMPLEMENTATION_COMMIT;
const GIT_DIRTY = process.env.GIT_DIRTY === 'true';

function fixedHash(byte) {
  return `0x${byte.toString(16).padStart(2, '0').repeat(32)}`;
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

function argType(api, decoratedCall, index) {
  return decoratedCall.meta.args[index].type.toString();
}

function encodeArg(api, decoratedCall, index, value) {
  return api.createType(argType(api, decoratedCall, index), value).toU8a();
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

function rightParametersHash(marketId, validFrom, validUntil) {
  const u64 = (value) => {
    const bytes = Buffer.alloc(8);
    bytes.writeBigUInt64LE(BigInt(value));
    return bytes;
  };
  const u32 = (value) => {
    const bytes = Buffer.alloc(4);
    bytes.writeUInt32LE(value);
    return bytes;
  };
  return hash(
    new Uint8Array([3]),
    new Uint8Array([0]),
    u64(validFrom),
    new Uint8Array([1]),
    u64(validUntil),
    new Uint8Array([1]),
    u32(1),
    new Uint8Array([1]),
    new Uint8Array([4]),
    Buffer.from(marketId.slice(2), 'hex'),
    new Uint8Array([0, 0])
  );
}

function buildSignedOrder(api, genesisHash, seller, input) {
  const call = api.tx.marketRuntime.createOrder;
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
  const orderId = computeOrderId(genesisHash, input.operation, encodeArg(api, call, 0, order));
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
  const signature = seller.sign(sellerSigningPayload(
    genesisHash,
    input.operation,
    encodeArg(api, call, 2, sellerCommitment)
  ));
  return { order, orderId, sellerCommitment, signature };
}

function buildSignedBuyer(api, genesisHash, buyer, signed, nonce) {
  const call = api.tx.marketRuntime.fundOrder;
  const commitment = {
    orderId: signed.orderId,
    buyer: accountHex(buyer),
    price: signed.order.price,
    currency: 'Native',
    expiresAt: signed.order.expiry,
    buyerNonce: nonce,
  };
  const signature = buyer.sign(buyerSigningPayload(
    genesisHash,
    signed.order.operation,
    encodeArg(api, call, 1, commitment)
  ));
  return { commitment, signature };
}

async function nextExpiry(api, preCreateExtrinsics = 0) {
  const header = await api.rpc.chain.getHeader();
  return header.number.toNumber() + TIMING.orderExpiry + preCreateExtrinsics + 1;
}

async function snapshotOrder(api, orderId) {
  const order = await api.query.marketRuntime.runtimeOrders(orderId);
  const settlement = await api.query.marketRuntime.settlementRecords(orderId);
  let escrow = '0x';
  if (order.isSome) {
    const orderJson = order.unwrap().toJSON();
    const escrowId = field(orderJson, 'escrowId', 'escrow_id');
    if (escrowId) escrow = (await api.query.marketRuntime.runtimeEscrows(escrowId)).toHex();
  }
  return { order: order.toHex(), escrow, settlement: settlement.toHex() };
}

async function verifySettlement(api, input) {
  const order = optionJson(await api.query.marketRuntime.runtimeOrders(input.orderId), `${input.label} order`);
  const record = optionJson(await api.query.marketRuntime.settlementRecords(input.orderId), `${input.label} settlement`);
  const escrowId = field(order, 'escrowId', 'escrow_id');
  const escrow = optionJson(await api.query.marketRuntime.runtimeEscrows(escrowId), `${input.label} escrow`);
  const keys = Object.keys(record);
  assert(keys.length === 24, `${input.label} settlement field count ${keys.length}`);
  assert(field(order, 'state') === 'Settled', `${input.label} order not Settled`);
  assert(field(record, 'orderId', 'order_id') === input.orderId, `${input.label} record order mismatch`);
  assert(field(record, 'operation') === input.operation, `${input.label} operation mismatch`);
  assert(field(record, 'seller') === accountHex(input.seller), `${input.label} seller mismatch`);
  assert(field(record, 'buyer') === accountHex(input.buyer), `${input.label} buyer mismatch`);
  assert(BigInt(field(record, 'grossPrice', 'gross_price')) === input.price, `${input.label} price mismatch`);
  const sum = BigInt(field(record, 'sellerAmount', 'seller_amount'))
    + BigInt(field(record, 'royaltyAmount', 'royalty_amount'))
    + BigInt(field(record, 'marketFeeAmount', 'market_fee_amount'))
    + BigInt(field(record, 'safetyFeeAmount', 'safety_fee_amount'));
  assert(sum === input.price, `${input.label} amount split mismatch`);
  assert(field(escrow, 'state') === 'Released', `${input.label} escrow not Released`);
  return { order, record, escrow, field_count: keys.length };
}

async function submitOrder(api, evidence, signed, witness, seller, buyer, buyerNonce, label) {
  const create = await sendTx(
    api,
    `${label}_create`,
    api.tx.marketRuntime.createOrder(
      signed.order,
      witness,
      signed.sellerCommitment,
      signed.signature
    ),
    evidence.accounts.RELAYER
  );
  recordTx(evidence, create);
  assert(countEvent(create.events, 'marketRuntime', 'OrderCreated') === 1, `${label} OrderCreated count`);

  const buyerSigned = buildSignedBuyer(api, api.genesisHash.toHex(), buyer, signed, buyerNonce);
  const fund = await sendTx(
    api,
    `${label}_fund`,
    api.tx.marketRuntime.fundOrder(signed.orderId, buyerSigned.commitment, buyerSigned.signature),
    buyer
  );
  recordTx(evidence, fund);
  assert(countEvent(fund.events, 'marketRuntime', 'OrderFunded') === 1, `${label} OrderFunded count`);

  const settle = await sendTx(
    api,
    `${label}_settle`,
    api.tx.marketRuntime.settle(signed.orderId),
    evidence.accounts.RELAYER
  );
  recordTx(evidence, settle);
  assert(countEvent(settle.events, 'marketRuntime', 'SettlementFinalized') === 1, `${label} SettlementFinalized count`);
  const audit = await verifySettlement(api, {
    label,
    orderId: signed.orderId,
    operation: signed.order.operation,
    seller,
    buyer,
    price: BigInt(signed.order.price),
  });
  evidence.operations[signed.order.operation] = {
    scenario_id: label,
    order_id: signed.orderId,
    expiry: signed.order.expiry,
    created_block: create.blockNumber,
    expiry_offset: signed.order.expiry - create.blockNumber,
    state: 'Settled',
    settlement_record_count: 1,
    evidence_field_count: audit.field_count,
    audit,
  };
  assert(evidence.operations[signed.order.operation].expiry_offset === TIMING.orderExpiry, `${label} expiry offset drift`);
  return { buyerSigned, audit };
}

async function main() {
  assert(RUN_ID && RAW_DIR && IMPLEMENTATION_COMMIT, 'RUN_ID, RAW_DIR, and IMPLEMENTATION_COMMIT are required');
  const api = await connect(RPC);
  const accounts = sharedAccounts();
  const evidence = baseEvidence('B2', api, RUN_ID, IMPLEMENTATION_COMMIT, GIT_DIRTY);
  evidence.accounts = accounts;
  try {
    evidence.environment.chain = (await api.rpc.system.chain()).toString();
    assert(api.tx.marketRuntime, 'marketRuntime pallet missing from metadata');
    assert(api.query.marketRuntime.runtimeOrders, 'marketRuntime storage missing from metadata');
    await fundSharedAccounts(api, accounts, evidence);

    const marketId = fixedHash(0x51);
    const marketCall = api.tx.marketRuntime.registerMarket(
      marketId,
      accounts.MARKET_OK.address,
      PROTOCOL_VERSION,
      31,
      0,
      { tierId: 1, maxActiveOrders: 32, maxFundedOrders: 16, maxActiveExposureValue: (1000n * PRICES.TransferMeta).toString() },
      '0'
    );
    const market = await sendTx(api, 'register_market', api.tx.sudo.sudo(marketCall), accounts.OWNER);
    recordTx(evidence, market);
    assert((await api.query.marketRuntime.markets(marketId)).isSome, 'market registration missing');

    const assets = { meta: fixedHash(0x61), issue: fixedHash(0x62), resale: fixedHash(0x63) };
    for (const [label, assetId] of Object.entries(assets)) {
      const result = await sendTx(api, `register_meta_${label}`, api.tx.marketRuntime.registerMeta({
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
      }), accounts.OWNER);
      recordTx(evidence, result);
    }

    const fixtureAnchor = (await api.rpc.chain.getHeader()).number.toNumber();
    const validUntil = fixtureAnchor + TIMING.rightValidity;
    const resaleRightId = fixedHash(0x73);
    const initialize = await sendTx(api, 'initialize_resale_right', api.tx.marketRuntime.initializeExistingRight({
      rightId: resaleRightId,
      assetId: assets.resale,
      holder: accounts.RIGHT_SELLER.address,
      issuer: accounts.OWNER.address,
      rightType: 'View',
      dataScope: 'FullAsset',
      validity: { validFrom: fixtureAnchor, validUntil, maxUses: 1 },
      transferable: true,
      allowedTransferMarkets: [marketId],
      royaltyBps: 0,
    }), accounts.OWNER);
    recordTx(evidence, initialize);
    assert(validUntil - fixtureAnchor === TIMING.rightValidity, 'right validity offset drift');

    const metaExpiry = await nextExpiry(api, 1);
    const metaAuthTx = await sendTx(api, 'create_meta_authorization', api.tx.marketRuntime.createTransferMetaAuthorization(
      assets.meta,
      marketId,
      { pricePolicy: pricePolicy(PRICES.TransferMeta), allowedBuyer: accounts.BUYER_1.address, expiry: metaExpiry, nonce: 0 }
    ), accounts.OWNER);
    recordTx(evidence, metaAuthTx);
    const metaAuthEvent = findEvent(metaAuthTx.events, 'marketRuntime', 'TransferMetaAuthorizationCreated');
    assert(metaAuthEvent, 'meta authorization event missing');
    const metaAuth = metaAuthEvent.event.data[0].toHex();
    const metaSigned = buildSignedOrder(api, api.genesisHash.toHex(), accounts.OWNER, {
      marketId,
      operation: 'TransferMeta',
      objectType: 'MetaToken',
      objectId: assets.meta,
      authorizationId: metaAuth,
      allowedBuyer: accountHex(accounts.BUYER_1),
      price: PRICES.TransferMeta,
      policyHash: pricePolicyHash(PRICES.TransferMeta),
      expiry: metaExpiry,
    });
    const metaCreate = await sendTx(api, 'LEG-META-01_create', api.tx.marketRuntime.createOrder(
      metaSigned.order, 'Authorization', metaSigned.sellerCommitment, metaSigned.signature
    ), accounts.RELAYER);
    recordTx(evidence, metaCreate);
    const metaBuyer = buildSignedBuyer(api, api.genesisHash.toHex(), accounts.BUYER_1, metaSigned, 0);
    const beforeReject = await snapshotOrder(api, metaSigned.orderId);
    const reject = await sendTx(
      api,
      'NEG-PAYER-01_wrong_payer',
      api.tx.marketRuntime.fundOrder(metaSigned.orderId, metaBuyer.commitment, metaBuyer.signature),
      accounts.BUYER_2,
      { section: 'marketRuntime', name: 'PayerBuyerMismatch' }
    );
    recordTx(evidence, reject);
    const afterReject = await snapshotOrder(api, metaSigned.orderId);
    assert(JSON.stringify(beforeReject) === JSON.stringify(afterReject), 'wrong-payer rejection dirtied Runtime authority');
    assert(countEvent(reject.events, 'marketRuntime', 'OrderFunded') === 0, 'rejection emitted OrderFunded');
    evidence.rejection = {
      scenario_id: 'NEG-PAYER-01',
      cleanliness_scenario_id: 'CLEAN-FUND-01',
      common_error_code: 'PayerBuyerMismatch',
      native_error: 'PayerBuyerMismatch',
      authoritative_state_unchanged: true,
      success_event_count: 0,
    };
    const metaFund = await sendTx(api, 'LEG-META-01_fund', api.tx.marketRuntime.fundOrder(
      metaSigned.orderId, metaBuyer.commitment, metaBuyer.signature
    ), accounts.BUYER_1);
    recordTx(evidence, metaFund);
    const metaSettle = await sendTx(api, 'LEG-META-01_settle', api.tx.marketRuntime.settle(metaSigned.orderId), accounts.RELAYER);
    recordTx(evidence, metaSettle);
    const metaAudit = await verifySettlement(api, {
      label: 'LEG-META-01', orderId: metaSigned.orderId, operation: 'TransferMeta',
      seller: accounts.OWNER, buyer: accounts.BUYER_1, price: PRICES.TransferMeta,
    });
    evidence.operations.TransferMeta = {
      scenario_id: 'LEG-META-01', order_id: metaSigned.orderId, expiry: metaExpiry,
      created_block: metaCreate.blockNumber, expiry_offset: metaExpiry - metaCreate.blockNumber,
      state: 'Settled', settlement_record_count: 1, evidence_field_count: metaAudit.field_count, audit: metaAudit,
    };
    assert(evidence.operations.TransferMeta.expiry_offset === TIMING.orderExpiry, 'meta expiry offset drift');

    const duplicateBefore = await snapshotOrder(api, metaSigned.orderId);
    const duplicate = await sendTx(
      api,
      'DUP-SETTLE-01_duplicate',
      api.tx.marketRuntime.settle(metaSigned.orderId),
      accounts.RELAYER,
      { section: 'marketRuntime', name: 'AlreadySettled' }
    );
    recordTx(evidence, duplicate);
    const duplicateAfter = await snapshotOrder(api, metaSigned.orderId);
    assert(JSON.stringify(duplicateBefore) === JSON.stringify(duplicateAfter), 'duplicate settlement dirtied Runtime authority');
    assert(countEvent(duplicate.events, 'marketRuntime', 'SettlementFinalized') === 0, 'duplicate emitted SettlementFinalized');
    evidence.duplicate = {
      scenario_id: 'DUP-SETTLE-01', common_error_code: 'AlreadySettled',
      accepted_attempt_count: 1, settlement_record_count: 1,
      second_success_event_count: 0, authoritative_state_unchanged: true,
    };

    const issueExpiry = await nextExpiry(api, 1);
    const issueTerms = {
      rightType: 'View', dataScope: 'FullAsset',
      validityTemplate: { validFrom: fixtureAnchor, validUntil, maxUses: 1 },
      maxIssueCount: 2, maxTotalUseUnits: 2,
      pricePolicy: pricePolicy(PRICES.IssueRight), royaltyBps: 0, transferable: true,
      allowedTransferMarkets: [marketId], expiry: issueExpiry, nonce: 0,
    };
    const issueAuthTx = await sendTx(api, 'create_issue_authorization', api.tx.marketRuntime.createIssueRightAuthorization(
      assets.issue, marketId, issueTerms
    ), accounts.OWNER);
    recordTx(evidence, issueAuthTx);
    const issueAuthEvent = findEvent(issueAuthTx.events, 'marketRuntime', 'IssueRightAuthorizationCreated');
    assert(issueAuthEvent, 'issue authorization event missing');
    const issueAuth = issueAuthEvent.event.data[0].toHex();
    const issueSigned = buildSignedOrder(api, api.genesisHash.toHex(), accounts.OWNER, {
      marketId, operation: 'IssueRight', objectType: 'MetaToken', objectId: assets.issue,
      authorizationId: issueAuth, price: PRICES.IssueRight,
      policyHash: pricePolicyHash(PRICES.IssueRight),
      rightParametersHash: rightParametersHash(marketId, fixtureAnchor, validUntil), expiry: issueExpiry,
    });
    const issueResult = await submitOrder(
      api, evidence, issueSigned, 'Authorization', accounts.OWNER, accounts.BUYER_1, 1, 'LEG-ISSUE-01'
    );
    const issuedRightId = computeRightId(api.genesisHash.toHex(), issueSigned.orderId);
    const issuedRight = optionJson(await api.query.marketRuntime.rightTokens(issuedRightId), 'issued right');
    assert(field(issuedRight, 'holder') === accounts.BUYER_1.address, 'issued right holder mismatch');
    const issueAuthState = optionJson(await api.query.marketRuntime.issueRightAuthorizations(issueAuth), 'issue auth');
    assert(Number(field(issueAuthState, 'reservedCount', 'reserved_count')) === 0, 'issue quota remained reserved');
    assert(Number(field(issueAuthState, 'issuedCount', 'issued_count')) === 1, 'issue quota was not converted');
    issueResult.audit.issue_authorization = issueAuthState;

    const resaleExpiry = await nextExpiry(api);
    const resaleSigned = buildSignedOrder(api, api.genesisHash.toHex(), accounts.RIGHT_SELLER, {
      marketId, operation: 'TransferRight', objectType: 'RightToken', objectId: resaleRightId,
      parentAssetId: assets.resale,
      authorizationId: computeResaleAuthorizationId(resaleRightId, marketId),
      price: PRICES.TransferRight, policyHash: pricePolicyHash(PRICES.TransferRight),
      rightParametersHash: rightParametersHash(marketId, fixtureAnchor, validUntil), expiry: resaleExpiry,
    });
    await submitOrder(
      api, evidence, resaleSigned, { SellerSnapshot: pricePolicy(PRICES.TransferRight) },
      accounts.RIGHT_SELLER, accounts.BUYER_2, 0, 'LEG-RESALE-01'
    );
    const resaleRight = optionJson(await api.query.marketRuntime.rightTokens(resaleRightId), 'resale right');
    assert(field(resaleRight, 'holder') === accounts.BUYER_2.address, 'resale right holder mismatch');

    evidence.audit = {
      authoritative_source: 'runtime',
      contract_projection_used_for_authority: false,
      settled_operation_count: 3,
      settlement_record_count: 3,
      all_record_field_counts: Object.values(evidence.operations).map((item) => item.evidence_field_count),
      amount_reconciliation_passed: true,
      owner_holder_reconciliation_passed: true,
      issue_quota_reconciliation_passed: true,
    };
    evidence.architecture_cost = {
      contract_gas_or_ref_time: architectureMetric('N/A', null, 'ref_time'),
      contract_storage_writes: architectureMetric('N/A', null, 'count'),
      runtime_dispatch_count: architectureMetric('measured', evidence.transactions.length, 'count'),
      runtime_storage_writes: architectureMetric('not-collected', null, 'count'),
      typed_ce_call_count: architectureMetric('N/A', null, 'count'),
      projection_storage_writes: architectureMetric('N/A', null, 'count'),
    };
    evidence.status = 'PASS';
    evidence.run.finished_at = new Date().toISOString();
    evidence.accounts = Object.fromEntries(Object.entries(accounts).map(([role, pair]) => [role, pair.address]));
    writeExclusiveJson(path.join(RAW_DIR, 'live-smoke.json'), evidence);
    console.log(JSON.stringify({ status: 'PASS', baseline: 'B2', runId: RUN_ID }));
  } finally {
    await api.disconnect();
  }
}

module.exports = {
  buildSignedBuyer,
  buildSignedOrder,
  field,
  fixedHash,
  nextExpiry,
  optionJson,
  pricePolicy,
  pricePolicyHash,
  rightParametersHash,
};

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stack || error.message || String(error));
    process.exit(1);
  });
}
