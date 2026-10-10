'use strict';

const fs = require('fs');
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
  dep,
  findEvent,
  fundSharedAccounts,
  hash,
  recordTx,
  sellerSigningPayload,
  sendTx,
  sharedAccounts,
  writeExclusiveJson,
} = require('./s314_live_common');

const { CodePromise, ContractPromise } = dep('@polkadot/api-contract');
const { blake2AsU8a } = dep('@polkadot/util-crypto');
const { u8aToHex } = dep('@polkadot/util');

const RPC = process.env.ASSETX_RPC || 'ws://127.0.0.1:9944';
const RUN_ID = process.env.RUN_ID;
const RAW_DIR = process.env.RAW_DIR;
const IMPLEMENTATION_COMMIT = process.env.IMPLEMENTATION_COMMIT;
const GIT_DIRTY = process.env.GIT_DIRTY === 'true';
const CONTRACT_METADATA = process.env.B1_CONTRACT_METADATA;
const CONTRACT_WASM = process.env.B1_CONTRACT_WASM;
const GAS_LIMIT = { refTime: '1000000000000', proofSize: '104857600' };

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
    if (Object.prototype.hasOwnProperty.call(value, 'ok')) {
      value = value.ok;
      continue;
    }
    if (Object.prototype.hasOwnProperty.call(value, 'Ok')) {
      value = value.Ok;
      continue;
    }
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
    return contract.abi.registry.createTypeUnsafe(
      outerType,
      [raw],
      { isPedantic: true },
    );
  } catch (outerError) {
    const inner = Array.isArray(returnType.sub) ? returnType.sub[0] : null;
    assert(
      reverted && returnType.namespace === 'Result'
        && inner && inner.name === 'Ok' && (inner.lookupName || inner.type),
      `cannot decode ${outerType}: ${outerError.message}`
    );
    const innerType = inner.lookupName || inner.type;
    // Native contract reverts may expose either the nested Result payload or
    // the same payload wrapped in the language-level Ok discriminant. Try the
    // native form first, then the explicitly wrapped form used by older nodes.
    const wrappedRaw = Uint8Array.from([0, ...raw]);
    let output;
    let innerError;
    for (const candidate of [raw, wrappedRaw]) {
      try {
        output = contract.abi.registry.createTypeUnsafe(
          innerType,
          [candidate],
          { isPedantic: true },
        );
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
    const returnType = messageMetadata.returnType.lookupName || messageMetadata.returnType.type;
    try {
      output = decodeMessageOutput(contract, messageMetadata, raw, reverted);
    } catch (error) {
      const observation = {
        message,
        return_type: returnType,
        raw_return_hex: u8aToHex(raw),
        reverted,
        execution_flags: typeof execution.flags.toJSON === 'function'
          ? execution.flags.toJSON()
          : execution.flags,
        dispatch,
      };
      throw new Error(
        `${message} ABI decode failed: ${error.message}; observation=${JSON.stringify(observation)}`
      );
    }
  }
  const decoded = output ? resultJson(output, message) : null;
  return {
    outcome,
    dispatch,
    decoded,
    reverted,
    gasConsumed: outcome.gasConsumed.toJSON(),
    gasRequired: outcome.gasRequired.toJSON(),
    storageDeposit: outcome.storageDeposit.toJSON(),
  };
}

async function contractCall(api, contract, evidence, label, message, caller, args = [], options = {}) {
  const value = options.value || '0';
  const expectedError = options.expectedError || null;
  const allowResultFailure = options.allowResultFailure || false;
  const dry = await dryRun(contract, message, caller, args, value);
  assert(dry.decoded, `${label} missing decoded contract result`);
  if (options.allowFailure) {
    // E7 composite probes intentionally submit a call whose dry-run may
    // revert; the submitted dispatch is retained as evidence below.
  } else if (allowResultFailure && expectedError) {
    assert(!dry.decoded.ok, `${label} unexpectedly succeeded in dry run`);
    assert(nativeError(dry.decoded.error) === expectedError, `${label} expected ${expectedError}, got ${nativeError(dry.decoded.error)}`);
  } else if (expectedError) {
    assert(dry.reverted, `${label} business error did not set the contract revert flag`);
    assert(!dry.decoded.ok, `${label} unexpectedly succeeded in dry run`);
    assert(nativeError(dry.decoded.error) === expectedError, `${label} expected ${expectedError}, got ${nativeError(dry.decoded.error)}`);
  } else {
    assert(!dry.reverted, `${label} unexpectedly set the contract revert flag`);
    assert(dry.decoded.ok, `${label} dry run failed: ${nativeError(dry.decoded.error)}`);
  }
  const tx = contract.tx[message]({
    gasLimit: dry.outcome.gasRequired,
    storageDepositLimit: null,
    value,
  }, ...args);
  const submitted = await sendTx(
    api,
    label,
    tx,
    caller,
    options.allowFailure
      ? { allowDispatchFailure: true }
      : allowResultFailure ? null
      : expectedError ? { section: 'contracts', name: 'ContractReverted' } : null
  );
  recordTx(evidence, submitted, {
    contract: {
      message,
      return: dry.decoded,
      gasConsumed: dry.gasConsumed,
      gasRequired: dry.gasRequired,
      storageDeposit: dry.storageDeposit,
    },
  });
  return { dry, submitted };
}

async function contractQuery(contract, message, caller, args = []) {
  const dry = await dryRun(contract, message, caller, args);
  assert(dry.decoded && dry.decoded.ok, `${message} query failed: ${JSON.stringify(dry.decoded)}`);
  return dry.decoded.value;
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
  const orderId = computeOrderId(
    genesisHash,
    input.operation,
    encodeMessageArg(contract, 'createOrder', 0, order)
  );
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
    encodeMessageArg(contract, 'createOrder', 2, sellerCommitment)
  ));
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
  const signature = buyer.sign(buyerSigningPayload(
    genesisHash,
    signed.order.operation,
    encodeMessageArg(contract, 'fundOrder', 1, commitment)
  ));
  return { commitment, signature };
}

async function nextExpiry(api, preCreateExtrinsics = 0) {
  const header = await api.rpc.chain.getHeader();
  return header.number.toNumber() + TIMING.orderExpiry + preCreateExtrinsics + 1;
}

async function snapshotOrder(contract, caller, orderId) {
  return {
    order: await contractQuery(contract, 'getOrder', caller, [orderId]),
    evidence: await contractQuery(contract, 'qEvidence', caller, [orderId]),
    totalEscrowed: await contractQuery(contract, 'getTotalEscrowed', caller),
  };
}

async function verifySettlement(contract, caller, input) {
  const order = await contractQuery(contract, 'getOrder', caller, [input.orderId]);
  const record = await contractQuery(contract, 'getSettlementRecord', caller, [input.orderId]);
  assert(order, `${input.label} order missing`);
  assert(record, `${input.label} settlement missing`);
  const keys = Object.keys(record);
  assert(keys.length === 24, `${input.label} settlement field count ${keys.length}`);
  assert(field(order, 'state') === 'Settled', `${input.label} order not Settled`);
  assert(field(record, 'orderId', 'order_id') === input.orderId, `${input.label} order id mismatch`);
  assert(field(record, 'operation') === input.operation, `${input.label} operation mismatch`);
  assert(field(record, 'seller') === accountHex(input.seller), `${input.label} seller mismatch`);
  assert(field(record, 'buyer') === accountHex(input.buyer), `${input.label} buyer mismatch`);
  assert(BigInt(field(record, 'grossPrice', 'gross_price')) === input.price, `${input.label} price mismatch`);
  const sum = BigInt(field(record, 'sellerAmount', 'seller_amount'))
    + BigInt(field(record, 'royaltyAmount', 'royalty_amount'))
    + BigInt(field(record, 'marketFeeAmount', 'market_fee_amount'))
    + BigInt(field(record, 'safetyFeeAmount', 'safety_fee_amount'));
  assert(sum === input.price, `${input.label} amount split mismatch`);
  const payment = await contractQuery(contract, 'qPayment', caller, [input.orderId]);
  assert(field(payment, 'state') === 'Released', `${input.label} payment not Released`);
  assert(field(payment, 'contractBalanceReconciles', 'contract_balance_reconciles') === true, `${input.label} contract balance mismatch`);
  return { order, record, payment, field_count: keys.length };
}

async function submitOrder(api, contract, evidence, signed, witness, seller, buyer, buyerNonce, label) {
  const create = await contractCall(api, contract, evidence, `${label}_create`, 'createOrder', evidence.accounts.RELAYER, [
    signed.order, witness, signed.sellerCommitment, signed.signature,
  ]);
  const buyerSigned = buildSignedBuyer(contract, api.genesisHash.toHex(), buyer, signed, buyerNonce);
  await contractCall(api, contract, evidence, `${label}_fund`, 'fundOrder', buyer, [
    signed.orderId, buyerSigned.commitment, buyerSigned.signature,
  ], { value: signed.order.price });
  const settle = await contractCall(api, contract, evidence, `${label}_settle`, 'settle', evidence.accounts.RELAYER, [signed.orderId]);
  assert(countEvent(settle.submitted.events, 'contracts', 'ContractEmitted') >= 1, `${label} emitted no contract event`);
  const audit = await verifySettlement(contract, evidence.accounts.OWNER, {
    label, orderId: signed.orderId, operation: signed.order.operation,
    seller, buyer, price: BigInt(signed.order.price),
  });
  evidence.operations[signed.order.operation] = {
    scenario_id: label,
    order_id: signed.orderId,
    expiry: signed.order.expiry,
    created_block: create.submitted.blockNumber,
    expiry_offset: signed.order.expiry - create.submitted.blockNumber,
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
  assert(CONTRACT_METADATA && CONTRACT_WASM, 'B1 contract artifact paths are required');
  const api = await connect(RPC);
  const accounts = sharedAccounts();
  const evidence = baseEvidence('B1', api, RUN_ID, IMPLEMENTATION_COMMIT, GIT_DIRTY);
  evidence.accounts = accounts;
  try {
    evidence.environment.chain = (await api.rpc.system.chain()).toString();
    await fundSharedAccounts(api, accounts, evidence);
    const metadata = JSON.parse(fs.readFileSync(CONTRACT_METADATA, 'utf8'));
    const wasm = fs.readFileSync(CONTRACT_WASM);
    const codeHash = u8aToHex(blake2AsU8a(wasm, 256));
    const code = new CodePromise(api, metadata, wasm);
    const salt = Buffer.from(`s314-${RUN_ID}`.slice(-32), 'utf8');
    const deploy = await sendTx(
      api,
      'deploy_b1_contract',
      code.tx.newWithGenesisHash({
        gasLimit: api.registry.createType('WeightV2', GAS_LIMIT),
        storageDepositLimit: null,
        salt,
      }, api.genesisHash.toHex()),
      accounts.OWNER
    );
    recordTx(evidence, deploy);
    const instantiated = findEvent(deploy.events, 'contracts', 'Instantiated');
    assert(instantiated, 'contracts.Instantiated missing');
    const contractAddress = instantiated.event.data[1].toString();
    const contract = new ContractPromise(api, metadata, contractAddress);
    evidence.contract = { address: contractAddress, wasm_path: CONTRACT_WASM, metadata_path: CONTRACT_METADATA, code_hash: codeHash };

    const registeredMarket = await contractCall(api, contract, evidence, 'register_market', 'registerMarket', accounts.OWNER, [
      accounts.MARKET_OK.address,
      contractAddress,
      codeHash,
      PROTOCOL_VERSION,
      31,
      0,
      { tierId: 1, maxActiveOrders: 32, maxFundedOrders: 16, maxActiveExposureValue: (1000n * PRICES.TransferMeta).toString() },
      '0',
    ]);
    const marketId = hash32Hex(registeredMarket.dry.decoded.value, 'register_market');
    assert(await contractQuery(contract, 'getMarket', accounts.OWNER, [marketId]), 'market registration missing');

    const assets = { meta: fixedHash(0x61), issue: fixedHash(0x62), resale: fixedHash(0x63) };
    for (const [label, assetId] of Object.entries(assets)) {
      await contractCall(api, contract, evidence, `register_meta_${label}`, 'registerMeta', accounts.OWNER, [{
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
    }

    const fixtureAnchor = (await api.rpc.chain.getHeader()).number.toNumber();
    const validUntil = fixtureAnchor + TIMING.rightValidity;
    const resaleRightId = fixedHash(0x73);
    await contractCall(api, contract, evidence, 'initialize_resale_right', 'initializeExistingRight', accounts.OWNER, [{
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
    }]);
    assert(validUntil - fixtureAnchor === TIMING.rightValidity, 'right validity offset drift');

    const metaExpiry = await nextExpiry(api, 1);
    const metaAuthResult = await contractCall(api, contract, evidence, 'create_meta_authorization', 'createTransferMetaAuthorization', accounts.OWNER, [
      assets.meta, marketId,
      { pricePolicy: pricePolicy(PRICES.TransferMeta), allowedBuyer: accounts.BUYER_1.address, expiry: metaExpiry, nonce: 0 },
    ]);
    const metaAuth = hash32Hex(metaAuthResult.dry.decoded.value, 'create_meta_authorization');
    const metaSigned = buildSignedOrder(contract, api.genesisHash.toHex(), accounts.OWNER, {
      marketId, operation: 'TransferMeta', objectType: 'MetaToken', objectId: assets.meta,
      authorizationId: metaAuth, allowedBuyer: accountHex(accounts.BUYER_1),
      price: PRICES.TransferMeta, policyHash: pricePolicyHash(PRICES.TransferMeta), expiry: metaExpiry,
    });
    const metaCreate = await contractCall(api, contract, evidence, 'LEG-META-01_create', 'createOrder', accounts.RELAYER, [
      metaSigned.order, 'Authorization', metaSigned.sellerCommitment, metaSigned.signature,
    ]);
    const metaBuyer = buildSignedBuyer(contract, api.genesisHash.toHex(), accounts.BUYER_1, metaSigned, 0);
    const rejectBefore = await snapshotOrder(contract, accounts.OWNER, metaSigned.orderId);
    const reject = await contractCall(api, contract, evidence, 'NEG-PAYER-01_wrong_payer', 'fundOrder', accounts.BUYER_2, [
      metaSigned.orderId, metaBuyer.commitment, metaBuyer.signature,
    ], { value: metaSigned.order.price, expectedError: 'PayerBuyerMismatch' });
    const rejectAfter = await snapshotOrder(contract, accounts.OWNER, metaSigned.orderId);
    assert(JSON.stringify(rejectBefore) === JSON.stringify(rejectAfter), 'wrong-payer rejection dirtied contract authority');
    assert(countEvent(reject.submitted.events, 'contracts', 'ContractEmitted') === 0, 'rejection emitted contract event');
    evidence.rejection = {
      scenario_id: 'NEG-PAYER-01', cleanliness_scenario_id: 'CLEAN-FUND-01',
      common_error_code: 'PayerBuyerMismatch', native_error: 'PayerBuyerMismatch',
      authoritative_state_unchanged: true, success_event_count: 0,
    };
    await contractCall(api, contract, evidence, 'LEG-META-01_fund', 'fundOrder', accounts.BUYER_1, [
      metaSigned.orderId, metaBuyer.commitment, metaBuyer.signature,
    ], { value: metaSigned.order.price });
    const metaSettle = await contractCall(api, contract, evidence, 'LEG-META-01_settle', 'settle', accounts.RELAYER, [metaSigned.orderId]);
    const metaAudit = await verifySettlement(contract, accounts.OWNER, {
      label: 'LEG-META-01', orderId: metaSigned.orderId, operation: 'TransferMeta',
      seller: accounts.OWNER, buyer: accounts.BUYER_1, price: PRICES.TransferMeta,
    });
    evidence.operations.TransferMeta = {
      scenario_id: 'LEG-META-01', order_id: metaSigned.orderId, expiry: metaExpiry,
      created_block: metaCreate.submitted.blockNumber, expiry_offset: metaExpiry - metaCreate.submitted.blockNumber,
      state: 'Settled', settlement_record_count: 1, evidence_field_count: metaAudit.field_count, audit: metaAudit,
    };
    assert(evidence.operations.TransferMeta.expiry_offset === TIMING.orderExpiry, 'meta expiry offset drift');
    assert(countEvent(metaSettle.submitted.events, 'contracts', 'ContractEmitted') >= 1, 'meta settle event missing');

    const duplicateBefore = await snapshotOrder(contract, accounts.OWNER, metaSigned.orderId);
    const duplicate = await contractCall(api, contract, evidence, 'DUP-SETTLE-01_duplicate', 'settle', accounts.RELAYER, [metaSigned.orderId], {
      expectedError: 'AlreadySettled',
    });
    const duplicateAfter = await snapshotOrder(contract, accounts.OWNER, metaSigned.orderId);
    assert(JSON.stringify(duplicateBefore) === JSON.stringify(duplicateAfter), 'duplicate settlement dirtied contract authority');
    assert(countEvent(duplicate.submitted.events, 'contracts', 'ContractEmitted') === 0, 'duplicate emitted contract event');
    evidence.duplicate = {
      scenario_id: 'DUP-SETTLE-01', common_error_code: 'AlreadySettled',
      accepted_attempt_count: 1, settlement_record_count: 1,
      second_success_event_count: 0, authoritative_state_unchanged: true,
    };

    const issueExpiry = await nextExpiry(api, 1);
    const issueAuthResult = await contractCall(api, contract, evidence, 'create_issue_authorization', 'createIssueRightAuthorization', accounts.OWNER, [
      assets.issue, marketId,
      {
        rightType: 'View', dataScope: 'FullAsset',
        validityTemplate: { validFrom: fixtureAnchor, validUntil, maxUses: 1 },
        maxIssueCount: 2, maxTotalUseUnits: 2, pricePolicy: pricePolicy(PRICES.IssueRight),
        royaltyBps: 0, transferable: true, allowedTransferMarkets: [marketId], expiry: issueExpiry, nonce: 0,
      },
    ]);
    const issueAuth = hash32Hex(issueAuthResult.dry.decoded.value, 'create_issue_authorization');
    const issueParameters = hash32Hex(
      await contractQuery(contract, 'issueRightParametersHash', accounts.OWNER, [issueAuth]),
      'issue_right_parameters_hash'
    );
    const issueSigned = buildSignedOrder(contract, api.genesisHash.toHex(), accounts.OWNER, {
      marketId, operation: 'IssueRight', objectType: 'MetaToken', objectId: assets.issue,
      authorizationId: issueAuth, price: PRICES.IssueRight, policyHash: pricePolicyHash(PRICES.IssueRight),
      rightParametersHash: issueParameters, expiry: issueExpiry,
    });
    const issueResult = await submitOrder(
      api, contract, evidence, issueSigned, 'Authorization', accounts.OWNER, accounts.BUYER_1, 1, 'LEG-ISSUE-01'
    );
    const issuedRightId = computeRightId(api.genesisHash.toHex(), issueSigned.orderId);
    const issuedRight = await contractQuery(contract, 'getRight', accounts.OWNER, [issuedRightId]);
    assert(issuedRight && field(issuedRight, 'holder') === accounts.BUYER_1.address, 'issued right holder mismatch');
    const issueAuthState = await contractQuery(contract, 'getIssueRightAuthorization', accounts.OWNER, [issueAuth]);
    assert(Number(field(issueAuthState, 'reservedCount', 'reserved_count')) === 0, 'issue quota remained reserved');
    assert(Number(field(issueAuthState, 'issuedCount', 'issued_count')) === 1, 'issue quota was not converted');
    issueResult.audit.issue_authorization = issueAuthState;

    const resaleExpiry = await nextExpiry(api);
    const resaleParameters = hash32Hex(
      await contractQuery(contract, 'existingRightParametersHash', accounts.OWNER, [resaleRightId]),
      'existing_right_parameters_hash'
    );
    const resaleSigned = buildSignedOrder(contract, api.genesisHash.toHex(), accounts.RIGHT_SELLER, {
      marketId, operation: 'TransferRight', objectType: 'RightToken', objectId: resaleRightId,
      parentAssetId: assets.resale, authorizationId: computeResaleAuthorizationId(resaleRightId, marketId),
      price: PRICES.TransferRight, policyHash: pricePolicyHash(PRICES.TransferRight),
      rightParametersHash: resaleParameters, expiry: resaleExpiry,
    });
    await submitOrder(
      api, contract, evidence, resaleSigned, { SellerSnapshot: pricePolicy(PRICES.TransferRight) },
      accounts.RIGHT_SELLER, accounts.BUYER_2, 0, 'LEG-RESALE-01'
    );
    const resaleRight = await contractQuery(contract, 'getRight', accounts.OWNER, [resaleRightId]);
    assert(resaleRight && field(resaleRight, 'holder') === accounts.BUYER_2.address, 'resale right holder mismatch');

    evidence.audit = {
      authoritative_source: 'contract', contract_projection_used_for_authority: false,
      settled_operation_count: 3, settlement_record_count: 3,
      all_record_field_counts: Object.values(evidence.operations).map((item) => item.evidence_field_count),
      amount_reconciliation_passed: true, owner_holder_reconciliation_passed: true,
      issue_quota_reconciliation_passed: true, contract_balance_reconciliation_passed: true,
    };
    const gas = evidence.transactions
      .filter((item) => item.contract)
      .reduce((sum, item) => sum + BigInt(field(item.contract.gasConsumed, 'refTime', 'ref_time') || 0), 0n);
    evidence.architecture_cost = {
      contract_gas_or_ref_time: architectureMetric('measured', gas, 'ref_time'),
      contract_storage_writes: architectureMetric('not-collected', null, 'count'),
      runtime_dispatch_count: architectureMetric('N/A', null, 'count'),
      runtime_storage_writes: architectureMetric('N/A', null, 'count'),
      typed_ce_call_count: architectureMetric('N/A', null, 'count'),
      projection_storage_writes: architectureMetric('N/A', null, 'count'),
    };
    evidence.status = 'PASS';
    evidence.run.finished_at = new Date().toISOString();
    evidence.accounts = Object.fromEntries(Object.entries(accounts).map(([role, pair]) => [role, pair.address]));
    writeExclusiveJson(path.join(RAW_DIR, 'live-smoke.json'), evidence);
    console.log(JSON.stringify({ status: 'PASS', baseline: 'B1', runId: RUN_ID, contractAddress }));
  } finally {
    await api.disconnect();
  }
}

module.exports = {
  GAS_LIMIT,
  buildSignedBuyer,
  buildSignedOrder,
  contractCall,
  contractQuery,
  decodeMessageOutput,
  field,
  fixedHash,
  hash32Hex,
  nativeError,
  nextExpiry,
  pricePolicy,
  pricePolicyHash,
};

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stack || error.message || String(error));
    process.exit(1);
  });
}
