use std::collections::BTreeSet;

const S38_I1: u16 = 1 << 0;
const S38_I2: u16 = 1 << 1;
const S38_I3: u16 = 1 << 2;
const S38_I4: u16 = 1 << 3;
const S38_I5: u16 = 1 << 4;
const S38_I6: u16 = 1 << 5;
const S38_I7: u16 = 1 << 6;
const S38_I8: u16 = 1 << 7;
const S38_I9: u16 = 1 << 8;
const S38_I10: u16 = 1 << 9;
const S38_REQUIRED_INVARIANTS: u16 =
    S38_I1 | S38_I2 | S38_I3 | S38_I4 | S38_I5 | S38_I6 | S38_I7 | S38_I8 | S38_I9 | S38_I10;

const S38_Q_META: u16 = 1 << 0;
const S38_Q_RIGHT: u16 = 1 << 1;
const S38_Q_ORDER: u16 = 1 << 2;
const S38_Q_AUTH: u16 = 1 << 3;
const S38_Q_PAY: u16 = 1 << 4;
const S38_Q_EVID: u16 = 1 << 5;
const S38_Q_EVENT: u16 = 1 << 6;
const S38_Q_MARKET: u16 = 1 << 7;
const S38_Q_ID: u16 = 1 << 8;
const S38_Q_ALL: u16 = S38_Q_META
    | S38_Q_RIGHT
    | S38_Q_ORDER
    | S38_Q_AUTH
    | S38_Q_PAY
    | S38_Q_EVID
    | S38_Q_EVENT
    | S38_Q_MARKET
    | S38_Q_ID;

const S38_M_ACCEPT: u8 = 1 << 0;
const S38_M_REJECT_CLEAN: u8 = 1 << 1;
const S38_M_AUDIT: u8 = 1 << 2;
const S38_M_ONCE: u8 = 1 << 3;
const S38_M_NO_SETTLEMENT: u8 = 1 << 4;
const S38_M_ROLLBACK: u8 = 1 << 5;
const S38_ALL_METRICS: u8 = S38_M_ACCEPT
    | S38_M_REJECT_CLEAN
    | S38_M_AUDIT
    | S38_M_ONCE
    | S38_M_NO_SETTLEMENT
    | S38_M_ROLLBACK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum S38Category {
    Legal,
    Illegal,
    Clean,
    Duplicate,
    Race,
}

#[derive(Clone, Copy)]
struct S38ScenarioSpec {
    id: &'static str,
    category: S38Category,
    invariants: u16,
    queries: u16,
    metrics: u8,
    expected_error: Option<crate::CommonErrorCode>,
}

#[derive(Clone)]
struct S38Fixture {
    operation: MarketOperation,
    seller: AccountId,
    asset_id: [u8; 32],
    right_id: Option<[u8; 32]>,
    auth_id: [u8; 32],
    order: OrderCommitment,
    order_id: [u8; 32],
    price_policy: PricePolicy,
}

impl S38Fixture {
    fn refresh_order_id(&mut self) {
        self.order_id = order_id(genesis_hash(), &self.order);
    }
}

#[derive(Clone, Copy)]
struct S38ProbeContext {
    asset_id: [u8; 32],
    right_id: Option<[u8; 32]>,
    auth_id: [u8; 32],
    market_id: [u8; 32],
    order_id: [u8; 32],
    object_id: [u8; 32],
    seller: [u8; 32],
}

fn s38_context(fixture: &S38Fixture) -> S38ProbeContext {
    S38ProbeContext {
        asset_id: fixture.asset_id,
        right_id: fixture.right_id,
        auth_id: fixture.auth_id,
        market_id: fixture.order.market_id,
        order_id: fixture.order_id,
        object_id: fixture.order.object_id,
        seller: fixture.order.seller,
    }
}

fn s38_event_probe(order_id: [u8; 32]) -> Vec<u8> {
    let mut names = Vec::<u8>::new();
    let mut finalized = 0u32;
    let mut operation_success = 0u32;
    let mut post_state_commitment = None;
    for record in System::events() {
        if let RuntimeEvent::MarketRuntime(event) = record.event {
            match event {
                crate::Event::OrderCreated {
                    order_id: observed, ..
                } if observed == order_id => names.push(1),
                crate::Event::OrderFunded {
                    order_id: observed, ..
                } if observed == order_id => names.push(2),
                crate::Event::OrderCancelled { order_id: observed } if observed == order_id => {
                    names.push(3);
                    operation_success += 1;
                }
                crate::Event::OrderExpired { order_id: observed } if observed == order_id => {
                    names.push(4);
                    operation_success += 1;
                }
                crate::Event::OrderRefunded { order_id: observed } if observed == order_id => {
                    names.push(5);
                    operation_success += 1;
                }
                crate::Event::PaymentEscrowed {
                    order_id: observed, ..
                } if observed == order_id => names.push(6),
                crate::Event::PaymentReleased {
                    order_id: observed, ..
                } if observed == order_id => names.push(7),
                crate::Event::PaymentRefunded {
                    order_id: observed, ..
                } if observed == order_id => names.push(8),
                crate::Event::SettlementFinalized {
                    order_id: observed,
                    post_state_commitment: commitment,
                    ..
                } if observed == order_id => {
                    names.push(9);
                    finalized += 1;
                    operation_success += 1;
                    post_state_commitment = Some(commitment);
                }
                _ => {}
            }
        }
    }
    (
        names,
        finalized,
        operation_success,
        post_state_commitment,
    )
        .encode()
}

fn s38_probe(context: S38ProbeContext) -> Vec<u8> {
    let order = RuntimeOrders::<Test>::get(context.order_id);
    let escrow = order
        .as_ref()
        .and_then(|value| value.escrow_id)
        .and_then(RuntimeEscrows::<Test>::get);
    let accounts = [OWNER, RIGHT_SELLER, BUYER, MARKET_OPERATOR, OTHER];
    let free = accounts.map(Balances::free_balance);
    let reserved = accounts.map(Balances::reserved_balance);
    let meta_indexes =
        accounts.map(|account| MetaOwnerIndex::<Test>::contains_key(account, context.asset_id));
    let right_indexes = context.right_id.map(|right_id| {
        accounts.map(|account| RightHolderIndex::<Test>::contains_key(account, right_id))
    });
    (
        (
            System::block_number(),
            Pallet::<Test>::q_meta(context.asset_id),
            context.right_id.map(Pallet::<Test>::q_right),
            Pallet::<Test>::q_order(context.order_id),
            IssueRightAuthorizations::<Test>::get(context.auth_id),
            TransferMetaAuthorizations::<Test>::get(context.auth_id),
            IssueQuotaReservations::<Test>::get(context.order_id),
            escrow,
            Pallet::<Test>::q_evidence(context.order_id),
        ),
        (
            Pallet::<Test>::q_market(context.market_id).ok(),
            crate::ActiveIssueAuthorization::<Test>::get(context.asset_id, context.market_id),
            ActiveIssueAuthorizationCount::<Test>::get(context.asset_id),
            crate::ActiveTransferAuthorizationByAsset::<Test>::get(context.asset_id),
            ActiveIssueOrderCount::<Test>::get(context.asset_id),
            ActiveMetaOrder::<Test>::get(context.asset_id),
            ActiveRightOrder::<Test>::get(context.object_id),
            crate::OutstandingIssueQuotaCount::<Test>::get(context.asset_id),
            crate::SellerNonce::<Test>::get((context.seller, context.object_id)),
        ),
        (
            BuyerNonce::<Test>::get(protocol_account(BUYER)),
            BuyerNonce::<Test>::get(protocol_account(OTHER)),
            AuthorizationNonce::<Test>::get((
                OWNER,
                context.asset_id,
                MarketOperation::TransferMeta,
            )),
            AuthorizationNonce::<Test>::get((
                OWNER,
                context.asset_id,
                MarketOperation::IssueRight,
            )),
            AssetRightIds::<Test>::get(context.asset_id),
            meta_indexes,
            right_indexes,
            context.right_id.map(|right_id| {
                RightParentIndex::<Test>::contains_key(context.asset_id, right_id)
            }),
        ),
        (free, reserved, s38_event_probe(context.order_id)),
    )
        .encode()
}

fn s38_assert_clean(
    context: S38ProbeContext,
    expected_native: Error<Test>,
    expected_common: crate::CommonErrorCode,
    operation: impl FnOnce() -> frame_support::dispatch::DispatchResult,
) {
    let before = s38_probe(context);
    assert_eq!(expected_native.common_error_code(), expected_common);
    let error = operation().expect_err("scenario must reject");
    let expected: sp_runtime::DispatchError = expected_native.into();
    assert_eq!(error, expected);
    assert_eq!(s38_probe(context), before);
}

fn s38_order(
    operation: MarketOperation,
    object_id: [u8; 32],
    parent_asset_id: Option<[u8; 32]>,
    authorization_id: [u8; 32],
    seller: AccountId,
    price: u128,
    price_policy: PricePolicy,
    parameters_hash: Option<[u8; 32]>,
) -> OrderCommitment {
    OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id: MARKET,
        operation,
        object_type: if operation == MarketOperation::TransferRight {
            ObjectType::RightToken
        } else {
            ObjectType::MetaToken
        },
        object_id,
        parent_asset_id,
        authorization_id,
        seller: protocol_account(seller),
        allowed_buyer: None,
        price,
        currency: CurrencyId::Native,
        policy_hash: price_policy_hash(&price_policy),
        right_parameters_hash: parameters_hash,
        seller_state_version: 1,
        order_nonce: 0,
        expiry: 20,
    }
}

fn s38_meta_fixture() -> S38Fixture {
    register_meta();
    register_market(ALL_CAPABILITIES);
    assert_ok!(MarketRuntime::create_transfer_meta_authorization(
        RuntimeOrigin::signed(OWNER),
        ASSET,
        MARKET,
        TransferMetaAuthorizationTerms {
            price_policy: policy(META_PRICE),
            allowed_buyer: None,
            expiry: 20,
            nonce: 0,
        },
    ));
    let auth_id = sole_transfer_auth_id();
    let price_policy = policy(META_PRICE);
    let order = s38_order(
        MarketOperation::TransferMeta,
        ASSET,
        None,
        auth_id,
        OWNER,
        META_PRICE,
        price_policy,
        None,
    );
    S38Fixture {
        operation: MarketOperation::TransferMeta,
        seller: OWNER,
        asset_id: ASSET,
        right_id: None,
        auth_id,
        order_id: order_id(genesis_hash(), &order),
        order,
        price_policy,
    }
}

fn s38_issue_fixture(max_issue_count: u32) -> S38Fixture {
    register_meta();
    register_market(ALL_CAPABILITIES);
    let mut terms = issue_terms(20);
    terms.max_issue_count = max_issue_count;
    terms.max_total_use_units = u64::from(max_issue_count) * 3;
    assert_ok!(MarketRuntime::create_issue_right_authorization(
        RuntimeOrigin::signed(OWNER),
        ASSET,
        MARKET,
        terms,
    ));
    let auth_id = sole_issue_auth_id();
    let auth = IssueRightAuthorizations::<Test>::get(auth_id).unwrap();
    let parameters = right_parameters_hash(
        auth.right_type,
        auth.data_scope,
        auth.validity_template,
        auth.transferable,
        &auth.allowed_transfer_markets,
        auth.royalty_bps,
    );
    let price_policy = policy(ISSUE_PRICE);
    let order = s38_order(
        MarketOperation::IssueRight,
        ASSET,
        None,
        auth_id,
        OWNER,
        ISSUE_PRICE,
        price_policy,
        Some(parameters),
    );
    let issued_right_id = right_id(genesis_hash(), MarketOperation::IssueRight, order_id(genesis_hash(), &order));
    S38Fixture {
        operation: MarketOperation::IssueRight,
        seller: OWNER,
        asset_id: ASSET,
        right_id: Some(issued_right_id),
        auth_id,
        order_id: order_id(genesis_hash(), &order),
        order,
        price_policy,
    }
}

fn s38_right_fixture() -> S38Fixture {
    register_meta();
    register_market(ALL_CAPABILITIES);
    initialize_resale_right(0);
    let right = RightTokens::<Test>::get(RIGHT).unwrap();
    let parameters = right_parameters_hash(
        right.right_type,
        right.data_scope,
        right.validity,
        right.transferable,
        &right.allowed_transfer_markets,
        right.royalty_bps,
    );
    let price_policy = policy(RESALE_PRICE);
    let auth_id = [0x81; 32];
    let order = s38_order(
        MarketOperation::TransferRight,
        RIGHT,
        Some(ASSET),
        auth_id,
        RIGHT_SELLER,
        RESALE_PRICE,
        price_policy,
        Some(parameters),
    );
    S38Fixture {
        operation: MarketOperation::TransferRight,
        seller: RIGHT_SELLER,
        asset_id: ASSET,
        right_id: Some(RIGHT),
        auth_id,
        order_id: order_id(genesis_hash(), &order),
        order,
        price_policy,
    }
}

fn s38_create(
    fixture: &S38Fixture,
    signer: AccountId,
    invalid_signature: bool,
) -> frame_support::dispatch::DispatchResult {
    let commitment = SellerCommitment {
        order_id: fixture.order_id,
        object_id: fixture.order.object_id,
        operation: fixture.order.operation,
        seller: fixture.order.seller,
        state_version: fixture.order.seller_state_version,
        price_policy_hash: fixture.order.policy_hash,
        right_parameters_hash: fixture.order.right_parameters_hash,
        expires_at: fixture.order.expiry,
        seller_nonce: fixture.order.order_nonce,
    };
    let mut signature = test_pair(signer)
        .sign(&seller_signing_payload(
            genesis_hash(),
            fixture.order.operation,
            &commitment,
        ))
        .0;
    if invalid_signature {
        signature[0] ^= 0xff;
    }
    let witness = match fixture.operation {
        MarketOperation::TransferMeta | MarketOperation::IssueRight => {
            PricePolicyWitness::Authorization
        }
        MarketOperation::TransferRight => PricePolicyWitness::SellerSnapshot(fixture.price_policy),
    };
    MarketRuntime::create_order(
        RuntimeOrigin::signed(OTHER),
        fixture.order.clone(),
        witness,
        commitment,
        signature,
    )
}

fn s38_fund(
    fixture: &S38Fixture,
    buyer: AccountId,
    payer: AccountId,
    nonce: u64,
) -> frame_support::dispatch::DispatchResult {
    let commitment = BuyerCommitment {
        order_id: fixture.order_id,
        buyer: protocol_account(buyer),
        price: fixture.order.price,
        currency: fixture.order.currency,
        expires_at: fixture.order.expiry,
        buyer_nonce: nonce,
    };
    let signature = test_pair(buyer)
        .sign(&buyer_signing_payload(
            genesis_hash(),
            fixture.operation,
            &commitment,
        ))
        .0;
    MarketRuntime::fund_order(
        RuntimeOrigin::signed(payer),
        fixture.order_id,
        commitment,
        signature,
    )
}

fn s38_assert_q_all(fixture: &S38Fixture) {
    let all = Pallet::<Test>::q_all(fixture.order_id).expect("Q_ALL must read Runtime authority");
    assert_eq!(
        all.order.order.as_ref().map(|value| value.order_id),
        Some(fixture.order_id)
    );
    assert_eq!(all.payment.order_id, fixture.order_id);
    assert_eq!(all.evidence.order_id, fixture.order_id);
    assert_eq!(all.identity.settlement_protocol_version, 1);
    assert_eq!(all.identity.settlement_domain_id, *b"axc/p2/shared");
    assert_ne!(all.state_commitment, [0u8; 32]);
    assert_eq!(Pallet::<Test>::q_meta(fixture.asset_id), all.meta);
    if let Some(right_id) = fixture.right_id {
        assert_eq!(Pallet::<Test>::q_right(right_id), all.right.unwrap());
    }
    assert_eq!(Pallet::<Test>::q_order(fixture.order_id), all.order);
    assert_eq!(
        Pallet::<Test>::q_authorization(fixture.order_id).unwrap(),
        all.authorization
    );
    assert_eq!(
        Pallet::<Test>::q_payment(fixture.order_id).unwrap(),
        all.payment
    );
    assert_eq!(Pallet::<Test>::q_evidence(fixture.order_id), all.evidence);
    assert_eq!(
        Pallet::<Test>::q_market(fixture.order.market_id).unwrap(),
        all.market
    );
    assert_eq!(
        Pallet::<Test>::q_identity(fixture.order_id).unwrap(),
        all.identity
    );
    assert!(!s38_event_probe(fixture.order_id).is_empty());
}

fn s38_run_legal_meta() {
    let fixture = s38_meta_fixture();
    assert_ok!(s38_create(&fixture, OWNER, false));
    assert_ok!(s38_fund(&fixture, BUYER, MARKET_OPERATOR, 0));
    assert_ok!(MarketRuntime::settle(
        RuntimeOrigin::signed(MARKET_OPERATOR),
        fixture.order_id,
    ));
    s38_assert_q_all(&fixture);
    assert_eq!(MetaTokens::<Test>::get(ASSET).unwrap().owner, BUYER);
    assert_eq!(Pallet::<Test>::q_evidence(fixture.order_id).settlement_record_count, 1);
}

fn s38_run_legal_issue() {
    let fixture = s38_issue_fixture(2);
    assert_ok!(s38_create(&fixture, OWNER, false));
    assert_ok!(s38_fund(&fixture, BUYER, BUYER, 0));
    assert_eq!(
        IssueRightAuthorizations::<Test>::get(fixture.auth_id)
            .unwrap()
            .reserved_count,
        1
    );
    assert_ok!(MarketRuntime::settle(
        RuntimeOrigin::signed(MARKET_OPERATOR),
        fixture.order_id,
    ));
    s38_assert_q_all(&fixture);
    let right_id = fixture.right_id.unwrap();
    assert_eq!(RightTokens::<Test>::get(right_id).unwrap().holder, BUYER);
    let auth = IssueRightAuthorizations::<Test>::get(fixture.auth_id).unwrap();
    assert_eq!((auth.reserved_count, auth.issued_count), (0, 1));
}

fn s38_run_legal_resale() {
    let fixture = s38_right_fixture();
    assert_ok!(s38_create(&fixture, RIGHT_SELLER, false));
    assert_ok!(s38_fund(&fixture, BUYER, BUYER, 0));
    assert_ok!(MarketRuntime::settle(
        RuntimeOrigin::signed(MARKET_OPERATOR),
        fixture.order_id,
    ));
    s38_assert_q_all(&fixture);
    assert_eq!(RightTokens::<Test>::get(RIGHT).unwrap().holder, BUYER);
}

fn s38_run_cancel() {
    let fixture = s38_meta_fixture();
    assert_ok!(s38_create(&fixture, OWNER, false));
    assert_ok!(MarketRuntime::cancel_open(
        RuntimeOrigin::signed(OWNER),
        fixture.order_id,
    ));
    assert_eq!(
        Pallet::<Test>::q_order(fixture.order_id).order.unwrap().state,
        OrderState::Cancelled
    );
    assert_eq!(Pallet::<Test>::q_evidence(fixture.order_id).settlement_record_count, 0);
}

fn s38_run_expire() {
    let fixture = s38_right_fixture();
    assert_ok!(s38_create(&fixture, RIGHT_SELLER, false));
    System::set_block_number(21);
    assert_ok!(MarketRuntime::expire_open(
        RuntimeOrigin::signed(OTHER),
        fixture.order_id,
    ));
    assert_eq!(
        Pallet::<Test>::q_order(fixture.order_id).order.unwrap().state,
        OrderState::Expired
    );
    assert_eq!(Pallet::<Test>::q_evidence(fixture.order_id).settlement_record_count, 0);
}

fn s38_run_refund(check_early: bool, duplicate: bool) {
    let fixture = s38_issue_fixture(2);
    assert_ok!(s38_create(&fixture, OWNER, false));
    assert_ok!(s38_fund(&fixture, BUYER, MARKET_OPERATOR, 0));
    if check_early {
        s38_assert_clean(
            s38_context(&fixture),
            Error::<Test>::GracePeriodActive,
            crate::CommonErrorCode::GracePeriodActive,
            || MarketRuntime::refund_funded(RuntimeOrigin::signed(OTHER), fixture.order_id),
        );
    }
    System::set_block_number(12);
    assert_ok!(MarketRuntime::refund_funded(
        RuntimeOrigin::signed(OTHER),
        fixture.order_id,
    ));
    assert_eq!(
        Pallet::<Test>::q_payment(fixture.order_id).unwrap().state,
        PaymentState::Refunded
    );
    assert_eq!(Pallet::<Test>::q_evidence(fixture.order_id).settlement_record_count, 0);
    assert_eq!(Balances::free_balance(BUYER), INITIAL_BALANCE);
    if duplicate {
        s38_assert_clean(
            s38_context(&fixture),
            Error::<Test>::TerminalOrder,
            crate::CommonErrorCode::TerminalOrder,
            || MarketRuntime::refund_funded(RuntimeOrigin::signed(OTHER), fixture.order_id),
        );
    }
}

fn s38_run_duplicate_settle() {
    let fixture = s38_meta_fixture();
    assert_ok!(s38_create(&fixture, OWNER, false));
    assert_ok!(s38_fund(&fixture, BUYER, MARKET_OPERATOR, 0));
    assert_ok!(MarketRuntime::settle(
        RuntimeOrigin::signed(MARKET_OPERATOR),
        fixture.order_id,
    ));
    s38_assert_clean(
        s38_context(&fixture),
        Error::<Test>::AlreadySettled,
        crate::CommonErrorCode::AlreadySettled,
        || MarketRuntime::settle(RuntimeOrigin::signed(OTHER), fixture.order_id),
    );
    assert_eq!(SettlementRecords::<Test>::iter().count(), 1);
    assert_eq!(
        System::events()
            .iter()
            .filter(|record| matches!(
                record.event,
                RuntimeEvent::MarketRuntime(crate::Event::SettlementFinalized {
                    order_id,
                    ..
                }) if order_id == fixture.order_id
            ))
            .count(),
        1
    );
}

fn s38_run_duplicate_create() {
    let fixture = s38_meta_fixture();
    assert_ok!(s38_create(&fixture, OWNER, false));
    s38_assert_clean(
        s38_context(&fixture),
        Error::<Test>::NonceUsed,
        crate::CommonErrorCode::NonceUsed,
        || s38_create(&fixture, OWNER, false),
    );
}

fn s38_run_race_buyer() {
    let fixture = s38_meta_fixture();
    assert_ok!(s38_create(&fixture, OWNER, false));
    assert_ok!(s38_fund(&fixture, BUYER, BUYER, 0));
    s38_assert_clean(
        s38_context(&fixture),
        Error::<Test>::OrderNotOpen,
        crate::CommonErrorCode::OrderNotOpen,
        || s38_fund(&fixture, OTHER, OTHER, 0),
    );
}

fn s38_run_race_quota() {
    let fixture = s38_issue_fixture(1);
    assert_ok!(s38_create(&fixture, OWNER, false));
    let mut second = fixture.clone();
    second.order.order_nonce = 1;
    second.refresh_order_id();
    second.right_id = Some(right_id(
        genesis_hash(),
        MarketOperation::IssueRight,
        second.order_id,
    ));
    assert_ok!(s38_create(&second, OWNER, false));
    assert_ok!(s38_fund(&fixture, BUYER, BUYER, 0));
    s38_assert_clean(
        s38_context(&second),
        Error::<Test>::QuotaExceeded,
        crate::CommonErrorCode::QuotaExceeded,
        || s38_fund(&second, BUYER, BUYER, 1),
    );
    assert_ok!(MarketRuntime::settle(
        RuntimeOrigin::signed(OTHER),
        fixture.order_id,
    ));
    assert_eq!(
        IssueRightAuthorizations::<Test>::get(fixture.auth_id)
            .unwrap()
            .issued_count,
        1
    );
}

fn s38_run_race_object() {
    let fixture = s38_right_fixture();
    assert_ok!(s38_create(&fixture, RIGHT_SELLER, false));
    let mut second = fixture.clone();
    second.order.order_nonce = 1;
    second.refresh_order_id();
    s38_assert_clean(
        s38_context(&second),
        Error::<Test>::ActiveOrderConflict,
        crate::CommonErrorCode::MutualExclusionConflict,
        || s38_create(&second, RIGHT_SELLER, false),
    );
}

fn s38_run_negative_meta_create(id: &str) {
    let mut fixture = s38_meta_fixture();
    let (expected_native, expected_common, signer, bad_signature) = match id {
        "NEG-PROTO-01" => {
            fixture.order.settlement_protocol_version += 1;
            (
                Error::<Test>::UnsupportedProtocolVersion,
                crate::CommonErrorCode::UnsupportedProtocolVersion,
                OWNER,
                false,
            )
        }
        "NEG-SIGN-01" => (
            Error::<Test>::InvalidSignature,
            crate::CommonErrorCode::InvalidSignature,
            OWNER,
            true,
        ),
        "NEG-MARKET-01" => {
            fixture.order.market_id = [0xee; 32];
            (
                Error::<Test>::MarketNotRegistered,
                crate::CommonErrorCode::MarketNotRegistered,
                OWNER,
                false,
            )
        }
        "NEG-OWNER-01" => {
            fixture.order.seller = protocol_account(OTHER);
            (
                Error::<Test>::OwnershipMismatch,
                crate::CommonErrorCode::OwnershipMismatch,
                OTHER,
                false,
            )
        }
        "NEG-VERSION-01" => {
            fixture.order.seller_state_version = 2;
            (
                Error::<Test>::StateVersionMismatch,
                crate::CommonErrorCode::StateVersionMismatch,
                OWNER,
                false,
            )
        }
        "NEG-PRICE-01" => {
            fixture.order.price -= 1;
            (
                Error::<Test>::BelowPriceFloor,
                crate::CommonErrorCode::BelowPriceFloor,
                OWNER,
                false,
            )
        }
        "NEG-FEE-01" => {
            Markets::<Test>::mutate(MARKET, |market| {
                market.as_mut().unwrap().market_fee_bps = 1;
            });
            (
                Error::<Test>::FeeOutOfBounds,
                crate::CommonErrorCode::FeeOutOfBounds,
                OWNER,
                false,
            )
        }
        _ => unreachable!(),
    };
    fixture.refresh_order_id();
    s38_assert_clean(
        s38_context(&fixture),
        expected_native,
        expected_common,
        || s38_create(&fixture, signer, bad_signature),
    );
}

fn s38_run_negative_capability() {
    let fixture = s38_issue_fixture(2);
    Markets::<Test>::mutate(MARKET, |market| {
        market.as_mut().unwrap().capabilities &= !CAP_CREATE_RIGHT_ISSUE_ORDER;
    });
    s38_assert_clean(
        s38_context(&fixture),
        Error::<Test>::CapabilityDenied,
        crate::CommonErrorCode::CapabilityDenied,
        || s38_create(&fixture, OWNER, false),
    );
}

fn s38_run_negative_auth() {
    let fixture = s38_right_fixture();
    RightTokens::<Test>::mutate(RIGHT, |right| {
        right.as_mut().unwrap().allowed_transfer_markets.clear();
    });
    s38_assert_clean(
        s38_context(&fixture),
        Error::<Test>::AuthorizationMissing,
        crate::CommonErrorCode::AuthorizationMissing,
        || s38_create(&fixture, RIGHT_SELLER, false),
    );
}

fn s38_run_negative_right(id: &str) {
    let mut fixture = s38_right_fixture();
    let (expected_native, expected_common, signer) = match id {
        "NEG-HOLDER-01" => {
            fixture.order.seller = protocol_account(BUYER);
            (
                Error::<Test>::OwnershipMismatch,
                crate::CommonErrorCode::OwnershipMismatch,
                BUYER,
            )
        }
        "NEG-PARENT-01" => {
            fixture.order.parent_asset_id = Some([0xec; 32]);
            (
                Error::<Test>::ParentMismatch,
                crate::CommonErrorCode::ParentMismatch,
                RIGHT_SELLER,
            )
        }
        "NEG-RIGHT-MISSING-01" => {
            fixture.order.object_id = [0xeb; 32];
            fixture.order.authorization_id = [0xeb; 32];
            fixture.order.right_parameters_hash = Some([0xea; 32]);
            fixture.right_id = Some([0xeb; 32]);
            (
                Error::<Test>::RightNotFound,
                crate::CommonErrorCode::RightInvalid,
                RIGHT_SELLER,
            )
        }
        "NEG-RIGHT-EXPIRED-01" => {
            RightTokens::<Test>::mutate(RIGHT, |right| {
                right.as_mut().unwrap().status = RightStatus::Expired;
            });
            (
                Error::<Test>::RightInvalid,
                crate::CommonErrorCode::RightInvalid,
                RIGHT_SELLER,
            )
        }
        "NEG-RIGHT-EXHAUSTED-01" => {
            RightTokens::<Test>::mutate(RIGHT, |right| {
                let right = right.as_mut().unwrap();
                right.status = RightStatus::Exhausted;
                right.remaining_uses = Some(0);
            });
            (
                Error::<Test>::RightInvalid,
                crate::CommonErrorCode::RightInvalid,
                RIGHT_SELLER,
            )
        }
        "NEG-RIGHT-REVOKED-01" => {
            RightTokens::<Test>::mutate(RIGHT, |right| {
                right.as_mut().unwrap().status = RightStatus::Revoked;
            });
            (
                Error::<Test>::RightInvalid,
                crate::CommonErrorCode::RightInvalid,
                RIGHT_SELLER,
            )
        }
        _ => unreachable!(),
    };
    fixture.refresh_order_id();
    s38_assert_clean(
        s38_context(&fixture),
        expected_native,
        expected_common,
        || s38_create(&fixture, signer, false),
    );
}

fn s38_run_negative_meta_right() {
    let fixture = s38_right_fixture();
    let context = s38_context(&fixture);
    let before = s38_probe(context);
    let result = MarketRuntime::create_transfer_meta_authorization(
        RuntimeOrigin::signed(OWNER),
        ASSET,
        MARKET,
        TransferMetaAuthorizationTerms {
            price_policy: policy(META_PRICE),
            allowed_buyer: None,
            expiry: 20,
            nonce: 0,
        },
    );
    assert_eq!(
        result,
        Err(Error::<Test>::MutualExclusionConflict.into())
    );
    assert_eq!(
        Error::<Test>::MutualExclusionConflict.common_error_code(),
        crate::CommonErrorCode::MutualExclusionConflict
    );
    assert_eq!(s38_probe(context), before);
}

fn s38_run_negative_lifecycle(id: &str) {
    let fixture = if id == "NEG-AUTH-REVOKED-01" || id == "NEG-CAPACITY-01" {
        s38_issue_fixture(2)
    } else {
        s38_meta_fixture()
    };
    if id == "NEG-MARKET-RESTRICTED-01" {
        assert_ok!(MarketRuntime::set_market_status(
            RuntimeOrigin::root(),
            MARKET,
            MarketStatus::Restricted,
        ));
        s38_assert_clean(
            s38_context(&fixture),
            Error::<Test>::MarketRestricted,
            crate::CommonErrorCode::MarketRestricted,
            || s38_create(&fixture, OWNER, false),
        );
        return;
    }
    assert_ok!(s38_create(&fixture, fixture.seller, false));
    match id {
        "NEG-BALANCE-01" => {
            assert_ok!(<Balances as ReservableCurrency<AccountId>>::reserve(
                &BUYER,
                INITIAL_BALANCE - 1,
            ));
            s38_assert_clean(
                s38_context(&fixture),
                Error::<Test>::InsufficientBalance,
                crate::CommonErrorCode::InsufficientBalance,
                || s38_fund(&fixture, BUYER, BUYER, 0),
            );
        }
        "NEG-PAYER-01" => s38_assert_clean(
            s38_context(&fixture),
            Error::<Test>::PayerBuyerMismatch,
            crate::CommonErrorCode::PayerBuyerMismatch,
            || s38_fund(&fixture, BUYER, OTHER, 0),
        ),
        "NEG-ORDER-PHASE-01" => s38_assert_clean(
            s38_context(&fixture),
            Error::<Test>::OrderNotFunded,
            crate::CommonErrorCode::OrderNotFunded,
            || MarketRuntime::settle(RuntimeOrigin::signed(OTHER), fixture.order_id),
        ),
        "NEG-AUTH-REVOKED-01" => {
            assert_ok!(MarketRuntime::revoke_issue_right_authorization(
                RuntimeOrigin::signed(OWNER),
                fixture.auth_id,
            ));
            s38_assert_clean(
                s38_context(&fixture),
                Error::<Test>::AuthorizationRevoked,
                crate::CommonErrorCode::AuthorizationRevoked,
                || s38_fund(&fixture, BUYER, BUYER, 0),
            );
        }
        "NEG-CAPACITY-01" => {
            Markets::<Test>::mutate(MARKET, |market| {
                market.as_mut().unwrap().capacity_tier.max_funded_orders = 0;
            });
            s38_assert_clean(
                s38_context(&fixture),
                Error::<Test>::CapacityExceeded,
                crate::CommonErrorCode::CapacityExceeded,
                || s38_fund(&fixture, BUYER, BUYER, 0),
            );
        }
        _ => unreachable!(),
    }
}

fn s38_run_clean_late_meta() {
    let fixture = s38_meta_fixture();
    assert_ok!(s38_create(&fixture, OWNER, false));
    assert_ok!(s38_fund(&fixture, BUYER, BUYER, 0));
    MetaTokens::<Test>::mutate(ASSET, |meta| {
        meta.as_mut().unwrap().status = crate::AssetStatus::Retiring;
    });
    s38_assert_clean(
        s38_context(&fixture),
        Error::<Test>::AssetNotTradable,
        crate::CommonErrorCode::AssetNotTradable,
        || MarketRuntime::settle(RuntimeOrigin::signed(OTHER), fixture.order_id),
    );
}

fn s38_run_clean_late_resale() {
    let fixture = s38_right_fixture();
    assert_ok!(s38_create(&fixture, RIGHT_SELLER, false));
    assert_ok!(s38_fund(&fixture, BUYER, BUYER, 0));
    RightTokens::<Test>::mutate(RIGHT, |right| {
        right.as_mut().unwrap().status = RightStatus::Expired;
    });
    s38_assert_clean(
        s38_context(&fixture),
        Error::<Test>::RightInvalid,
        crate::CommonErrorCode::RightInvalid,
        || MarketRuntime::settle(RuntimeOrigin::signed(OTHER), fixture.order_id),
    );
}

fn s38_run_abort(operation: MarketOperation) {
    let fixture = match operation {
        MarketOperation::TransferMeta => s38_meta_fixture(),
        MarketOperation::IssueRight => s38_issue_fixture(2),
        MarketOperation::TransferRight => s38_right_fixture(),
    };
    assert_ok!(s38_create(&fixture, fixture.seller, false));
    assert_ok!(s38_fund(&fixture, BUYER, BUYER, 0));
    let escrow_id = RuntimeOrders::<Test>::get(fixture.order_id)
        .unwrap()
        .escrow_id
        .unwrap();
    RuntimeEscrows::<Test>::mutate(escrow_id, |escrow| {
        escrow.as_mut().unwrap().amounts.seller_net -= 1;
    });
    s38_assert_clean(
        s38_context(&fixture),
        Error::<Test>::EscrowMismatch,
        crate::CommonErrorCode::EscrowMismatch,
        || MarketRuntime::settle(RuntimeOrigin::signed(OTHER), fixture.order_id),
    );
}

fn s38_run_scenario(id: &str) {
    new_test_ext().execute_with(|| match id {
        "LEG-META-01" => s38_run_legal_meta(),
        "LEG-ISSUE-01" => s38_run_legal_issue(),
        "LEG-RESALE-01" => s38_run_legal_resale(),
        "LEG-CANCEL-01" => s38_run_cancel(),
        "LEG-EXPIRE-01" => s38_run_expire(),
        "LEG-REFUND-01" => s38_run_refund(false, false),
        "NEG-PROTO-01" | "NEG-SIGN-01" | "NEG-MARKET-01" | "NEG-OWNER-01"
        | "NEG-VERSION-01" | "NEG-PRICE-01" | "NEG-FEE-01" => {
            s38_run_negative_meta_create(id)
        }
        "NEG-CAPABILITY-01" => s38_run_negative_capability(),
        "NEG-AUTH-01" => s38_run_negative_auth(),
        "NEG-HOLDER-01" | "NEG-PARENT-01" | "NEG-RIGHT-MISSING-01"
        | "NEG-RIGHT-EXPIRED-01" | "NEG-RIGHT-EXHAUSTED-01"
        | "NEG-RIGHT-REVOKED-01" => s38_run_negative_right(id),
        "NEG-META-RIGHT-01" => s38_run_negative_meta_right(),
        "NEG-NONCE-01" | "DUP-CREATE-01" => s38_run_duplicate_create(),
        "NEG-BALANCE-01" | "NEG-PAYER-01" | "NEG-ORDER-PHASE-01"
        | "NEG-AUTH-REVOKED-01" | "NEG-MARKET-RESTRICTED-01"
        | "NEG-CAPACITY-01" => s38_run_negative_lifecycle(id),
        "NEG-REFUND-EARLY-01" => s38_run_refund(true, false),
        "CLEAN-LATE-META-01" => s38_run_clean_late_meta(),
        "CLEAN-LATE-RESALE-01" => s38_run_clean_late_resale(),
        "CLEAN-ABORT-META-01" => s38_run_abort(MarketOperation::TransferMeta),
        "CLEAN-ABORT-ISSUE-01" => s38_run_abort(MarketOperation::IssueRight),
        "CLEAN-ABORT-RESALE-01" => s38_run_abort(MarketOperation::TransferRight),
        "DUP-SETTLE-01" | "RACE-RELAYER-01" => s38_run_duplicate_settle(),
        "DUP-REFUND-01" => s38_run_refund(false, true),
        "RACE-BUYER-01" => s38_run_race_buyer(),
        "RACE-QUOTA-01" => s38_run_race_quota(),
        "RACE-OBJECT-01" => s38_run_race_object(),
        _ => panic!("missing frozen scenario {id}"),
    });
}

const S38_SCENARIOS: &[S38ScenarioSpec] = &[
    S38ScenarioSpec { id: "LEG-META-01", category: S38Category::Legal, invariants: S38_I1 | S38_I3 | S38_I7, queries: S38_Q_ALL & !S38_Q_RIGHT, metrics: S38_M_ACCEPT | S38_M_AUDIT, expected_error: None },
    S38ScenarioSpec { id: "LEG-ISSUE-01", category: S38Category::Legal, invariants: S38_I1 | S38_I3 | S38_I7 | S38_I10, queries: S38_Q_ALL, metrics: S38_M_ACCEPT | S38_M_AUDIT, expected_error: None },
    S38ScenarioSpec { id: "LEG-RESALE-01", category: S38Category::Legal, invariants: S38_I1 | S38_I3 | S38_I6 | S38_I7, queries: S38_Q_ALL, metrics: S38_M_ACCEPT | S38_M_AUDIT, expected_error: None },
    S38ScenarioSpec { id: "LEG-CANCEL-01", category: S38Category::Legal, invariants: S38_I3, queries: S38_Q_META | S38_Q_ORDER | S38_Q_AUTH | S38_Q_PAY | S38_Q_EVID | S38_Q_EVENT | S38_Q_MARKET, metrics: S38_M_ACCEPT | S38_M_NO_SETTLEMENT, expected_error: None },
    S38ScenarioSpec { id: "LEG-EXPIRE-01", category: S38Category::Legal, invariants: S38_I3, queries: S38_Q_RIGHT | S38_Q_ORDER | S38_Q_PAY | S38_Q_EVID | S38_Q_EVENT | S38_Q_MARKET, metrics: S38_M_ACCEPT | S38_M_NO_SETTLEMENT, expected_error: None },
    S38ScenarioSpec { id: "LEG-REFUND-01", category: S38Category::Legal, invariants: S38_I3 | S38_I8 | S38_I10, queries: S38_Q_META | S38_Q_RIGHT | S38_Q_ORDER | S38_Q_AUTH | S38_Q_PAY | S38_Q_EVID | S38_Q_EVENT | S38_Q_MARKET, metrics: S38_M_ACCEPT | S38_M_NO_SETTLEMENT, expected_error: None },
    S38ScenarioSpec { id: "NEG-PROTO-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::UnsupportedProtocolVersion) },
    S38ScenarioSpec { id: "NEG-SIGN-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::InvalidSignature) },
    S38ScenarioSpec { id: "NEG-MARKET-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I4 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::MarketNotRegistered) },
    S38ScenarioSpec { id: "NEG-CAPABILITY-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I4 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::CapabilityDenied) },
    S38ScenarioSpec { id: "NEG-AUTH-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I4 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::AuthorizationMissing) },
    S38ScenarioSpec { id: "NEG-OWNER-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::OwnershipMismatch) },
    S38ScenarioSpec { id: "NEG-HOLDER-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I6 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::OwnershipMismatch) },
    S38ScenarioSpec { id: "NEG-PARENT-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I6 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::ParentMismatch) },
    S38ScenarioSpec { id: "NEG-RIGHT-MISSING-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I6 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::RightInvalid) },
    S38ScenarioSpec { id: "NEG-RIGHT-EXPIRED-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I6 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::RightInvalid) },
    S38ScenarioSpec { id: "NEG-RIGHT-EXHAUSTED-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I6 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::RightInvalid) },
    S38ScenarioSpec { id: "NEG-RIGHT-REVOKED-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I6 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::RightInvalid) },
    S38ScenarioSpec { id: "NEG-META-RIGHT-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8 | S38_I9, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::MutualExclusionConflict) },
    S38ScenarioSpec { id: "NEG-VERSION-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::StateVersionMismatch) },
    S38ScenarioSpec { id: "NEG-NONCE-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I5 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::NonceUsed) },
    S38ScenarioSpec { id: "NEG-PRICE-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::BelowPriceFloor) },
    S38ScenarioSpec { id: "NEG-FEE-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::FeeOutOfBounds) },
    S38ScenarioSpec { id: "NEG-BALANCE-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::InsufficientBalance) },
    S38ScenarioSpec { id: "NEG-PAYER-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::PayerBuyerMismatch) },
    S38ScenarioSpec { id: "NEG-ORDER-PHASE-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::OrderNotFunded) },
    S38ScenarioSpec { id: "NEG-AUTH-REVOKED-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8 | S38_I10, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::AuthorizationRevoked) },
    S38ScenarioSpec { id: "NEG-REFUND-EARLY-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::GracePeriodActive) },
    S38ScenarioSpec { id: "NEG-MARKET-RESTRICTED-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I4 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::MarketRestricted) },
    S38ScenarioSpec { id: "NEG-CAPACITY-01", category: S38Category::Illegal, invariants: S38_I2 | S38_I8 | S38_I10, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::CapacityExceeded) },
    S38ScenarioSpec { id: "CLEAN-LATE-META-01", category: S38Category::Clean, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::AssetNotTradable) },
    S38ScenarioSpec { id: "CLEAN-LATE-RESALE-01", category: S38Category::Clean, invariants: S38_I2 | S38_I6 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::RightInvalid) },
    S38ScenarioSpec { id: "CLEAN-ABORT-META-01", category: S38Category::Clean, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_ROLLBACK, expected_error: None },
    S38ScenarioSpec { id: "CLEAN-ABORT-ISSUE-01", category: S38Category::Clean, invariants: S38_I2 | S38_I8 | S38_I10, queries: S38_Q_ALL, metrics: S38_M_ROLLBACK, expected_error: None },
    S38ScenarioSpec { id: "CLEAN-ABORT-RESALE-01", category: S38Category::Clean, invariants: S38_I2 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_ROLLBACK, expected_error: None },
    S38ScenarioSpec { id: "DUP-SETTLE-01", category: S38Category::Duplicate, invariants: S38_I3 | S38_I5 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_ONCE | S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::AlreadySettled) },
    S38ScenarioSpec { id: "DUP-CREATE-01", category: S38Category::Duplicate, invariants: S38_I5 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::NonceUsed) },
    S38ScenarioSpec { id: "DUP-REFUND-01", category: S38Category::Duplicate, invariants: S38_I5 | S38_I8 | S38_I10, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::TerminalOrder) },
    S38ScenarioSpec { id: "RACE-BUYER-01", category: S38Category::Race, invariants: S38_I5 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::OrderNotOpen) },
    S38ScenarioSpec { id: "RACE-RELAYER-01", category: S38Category::Race, invariants: S38_I1 | S38_I3 | S38_I5 | S38_I7 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_ONCE | S38_M_AUDIT, expected_error: Some(crate::CommonErrorCode::AlreadySettled) },
    S38ScenarioSpec { id: "RACE-QUOTA-01", category: S38Category::Race, invariants: S38_I3 | S38_I5 | S38_I8 | S38_I10, queries: S38_Q_ALL, metrics: S38_M_ONCE | S38_M_AUDIT, expected_error: Some(crate::CommonErrorCode::QuotaExceeded) },
    S38ScenarioSpec { id: "RACE-OBJECT-01", category: S38Category::Race, invariants: S38_I5 | S38_I8, queries: S38_Q_ALL, metrics: S38_M_REJECT_CLEAN, expected_error: Some(crate::CommonErrorCode::MutualExclusionConflict) },
];

#[test]
fn s38_frozen_catalog_queries_errors_and_i1_i10_are_complete() {
    assert_eq!(S38_SCENARIOS.len(), 42);
    assert_eq!(
        S38_SCENARIOS
            .iter()
            .map(|scenario| scenario.id)
            .collect::<BTreeSet<_>>()
            .len(),
        42
    );
    assert_eq!(S38_SCENARIOS.iter().filter(|s| s.category == S38Category::Legal).count(), 6);
    assert_eq!(S38_SCENARIOS.iter().filter(|s| s.category == S38Category::Illegal).count(), 24);
    assert_eq!(S38_SCENARIOS.iter().filter(|s| s.category == S38Category::Clean).count(), 5);
    assert_eq!(S38_SCENARIOS.iter().filter(|s| s.category == S38Category::Duplicate).count(), 3);
    assert_eq!(S38_SCENARIOS.iter().filter(|s| s.category == S38Category::Race).count(), 4);
    assert_eq!(
        S38_SCENARIOS
            .iter()
            .fold(0, |mask, scenario| mask | scenario.invariants),
        S38_REQUIRED_INVARIANTS
    );
    assert!(S38_SCENARIOS
        .iter()
        .all(|scenario| scenario.queries != 0 && scenario.queries & !S38_Q_ALL == 0));
    assert!(S38_SCENARIOS
        .iter()
        .all(|scenario| scenario.metrics != 0 && scenario.metrics & !S38_ALL_METRICS == 0));
    assert!(S38_SCENARIOS
        .iter()
        .filter(|scenario| scenario.category == S38Category::Illegal)
        .all(|scenario| scenario.expected_error.is_some()));
    assert_eq!(crate::CommonErrorCode::ALL.len(), 33);
    for (index, error) in crate::CommonErrorCode::ALL.iter().enumerate() {
        assert_eq!(*error as u32, (index + 1) as u32);
    }
}

macro_rules! s38_scenario_tests {
    ($($name:ident => $id:literal),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                s38_run_scenario($id);
            }
        )+
    };
}

s38_scenario_tests! {
    s38_leg_meta_01 => "LEG-META-01", s38_leg_issue_01 => "LEG-ISSUE-01",
    s38_leg_resale_01 => "LEG-RESALE-01", s38_leg_cancel_01 => "LEG-CANCEL-01",
    s38_leg_expire_01 => "LEG-EXPIRE-01", s38_leg_refund_01 => "LEG-REFUND-01",
    s38_neg_proto_01 => "NEG-PROTO-01", s38_neg_sign_01 => "NEG-SIGN-01",
    s38_neg_market_01 => "NEG-MARKET-01", s38_neg_capability_01 => "NEG-CAPABILITY-01",
    s38_neg_auth_01 => "NEG-AUTH-01", s38_neg_owner_01 => "NEG-OWNER-01",
    s38_neg_holder_01 => "NEG-HOLDER-01", s38_neg_parent_01 => "NEG-PARENT-01",
    s38_neg_right_missing_01 => "NEG-RIGHT-MISSING-01",
    s38_neg_right_expired_01 => "NEG-RIGHT-EXPIRED-01",
    s38_neg_right_exhausted_01 => "NEG-RIGHT-EXHAUSTED-01",
    s38_neg_right_revoked_01 => "NEG-RIGHT-REVOKED-01",
    s38_neg_meta_right_01 => "NEG-META-RIGHT-01", s38_neg_version_01 => "NEG-VERSION-01",
    s38_neg_nonce_01 => "NEG-NONCE-01", s38_neg_price_01 => "NEG-PRICE-01",
    s38_neg_fee_01 => "NEG-FEE-01", s38_neg_balance_01 => "NEG-BALANCE-01",
    s38_neg_payer_01 => "NEG-PAYER-01", s38_neg_order_phase_01 => "NEG-ORDER-PHASE-01",
    s38_neg_auth_revoked_01 => "NEG-AUTH-REVOKED-01",
    s38_neg_refund_early_01 => "NEG-REFUND-EARLY-01",
    s38_neg_market_restricted_01 => "NEG-MARKET-RESTRICTED-01",
    s38_neg_capacity_01 => "NEG-CAPACITY-01",
    s38_clean_late_meta_01 => "CLEAN-LATE-META-01",
    s38_clean_late_resale_01 => "CLEAN-LATE-RESALE-01",
    s38_clean_abort_meta_01 => "CLEAN-ABORT-META-01",
    s38_clean_abort_issue_01 => "CLEAN-ABORT-ISSUE-01",
    s38_clean_abort_resale_01 => "CLEAN-ABORT-RESALE-01",
    s38_dup_settle_01 => "DUP-SETTLE-01", s38_dup_create_01 => "DUP-CREATE-01",
    s38_dup_refund_01 => "DUP-REFUND-01", s38_race_buyer_01 => "RACE-BUYER-01",
    s38_race_relayer_01 => "RACE-RELAYER-01", s38_race_quota_01 => "RACE-QUOTA-01",
    s38_race_object_01 => "RACE-OBJECT-01",
}
