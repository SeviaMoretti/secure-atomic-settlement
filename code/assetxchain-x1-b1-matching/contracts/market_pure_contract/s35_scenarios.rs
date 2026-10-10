use std::collections::BTreeSet;

const I1: u16 = 1 << 0;
const I2: u16 = 1 << 1;
const I3: u16 = 1 << 2;
const I4: u16 = 1 << 3;
const I5: u16 = 1 << 4;
const I6: u16 = 1 << 5;
const I7: u16 = 1 << 6;
const I8: u16 = 1 << 7;
const I9: u16 = 1 << 8;
const I10: u16 = 1 << 9;
const REQUIRED_INVARIANTS: u16 = I1 | I2 | I3 | I4 | I5 | I6 | I7 | I8 | I9 | I10;

const Q_META: u16 = 1 << 0;
const Q_RIGHT: u16 = 1 << 1;
const Q_ORDER: u16 = 1 << 2;
const Q_AUTH: u16 = 1 << 3;
const Q_PAY: u16 = 1 << 4;
const Q_EVID: u16 = 1 << 5;
const Q_EVENT: u16 = 1 << 6;
const Q_MARKET: u16 = 1 << 7;
const Q_ID: u16 = 1 << 8;
const Q_ALL: u16 = Q_META | Q_RIGHT | Q_ORDER | Q_AUTH | Q_PAY | Q_EVID | Q_EVENT | Q_MARKET | Q_ID;

const M_ACCEPT: u8 = 1 << 0;
const M_REJECT_CLEAN: u8 = 1 << 1;
const M_AUDIT: u8 = 1 << 2;
const M_ONCE: u8 = 1 << 3;
const M_NO_SETTLEMENT: u8 = 1 << 4;
const M_ROLLBACK: u8 = 1 << 5;
const ALL_METRICS: u8 = M_ACCEPT | M_REJECT_CLEAN | M_AUDIT | M_ONCE | M_NO_SETTLEMENT | M_ROLLBACK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Legal,
    Illegal,
    Clean,
    Duplicate,
    Race,
}

#[derive(Clone, Copy)]
struct ScenarioSpec {
    id: &'static str,
    category: Category,
    invariants: u16,
    queries: u16,
    metrics: u8,
    expected_error: Option<CommonErrorCode>,
}

#[derive(Clone, Copy)]
struct ProbeContext {
    asset_id: Hash32,
    right_id: Option<Hash32>,
    auth_id: Hash32,
    market_id: Hash32,
    order_id: Hash32,
    accounts: [AccountId; 4],
}

fn probe(contract: &MarketPureContract, context: ProbeContext) -> Vec<u8> {
    let order = contract.get_order(context.order_id);
    let escrow = order
        .as_ref()
        .and_then(|value| value.escrow_id)
        .and_then(|escrow_id| contract.get_escrow(escrow_id));
    let balances = context.accounts.map(|account| {
        test::get_account_balance::<ink::env::DefaultEnvironment>(account).unwrap_or(0)
    });
    (
        contract.q_meta(context.asset_id),
        context.right_id.map(|right_id| contract.q_right(right_id)),
        contract.q_order(context.order_id),
        contract.get_issue_right_authorization(context.auth_id),
        contract.get_transfer_meta_authorization(context.auth_id),
        contract.get_issue_quota_reservation(context.order_id),
        escrow,
        contract.q_evidence(context.order_id),
        contract.q_market(context.market_id),
        contract.get_total_escrowed(),
        balances,
        test::get_account_balance::<ink::env::DefaultEnvironment>(
            test::callee::<ink::env::DefaultEnvironment>(),
        )
        .unwrap_or(0),
        test::recorded_events().count() as u32,
    )
        .encode()
}

fn assert_clean<T: core::fmt::Debug>(
    contract: &mut MarketPureContract,
    context: ProbeContext,
    expected: CommonErrorCode,
    operation: impl FnOnce(&mut MarketPureContract) -> Result<T, Error>,
) {
    let before = probe(contract, context);
    let error = operation(contract).expect_err("scenario must reject");
    assert_eq!(error.common_error_code(), expected);
    assert_eq!(probe(contract, context), before);
}

fn attempt_payable<T>(
    contract: &mut MarketPureContract,
    transferred: Balance,
    operation: impl FnOnce(&mut MarketPureContract) -> Result<T, Error>,
) -> Result<T, Error> {
    let callee = test::callee::<ink::env::DefaultEnvironment>();
    let before = test::get_account_balance::<ink::env::DefaultEnvironment>(callee).unwrap_or(0);
    test::set_value_transferred::<ink::env::DefaultEnvironment>(transferred);
    test::set_account_balance::<ink::env::DefaultEnvironment>(callee, before + transferred);
    let result = operation(contract);
    if result.is_err() {
        test::set_account_balance::<ink::env::DefaultEnvironment>(callee, before);
    }
    test::set_value_transferred::<ink::env::DefaultEnvironment>(0);
    result
}

struct MetaFixture {
    contract: MarketPureContract,
    owner: Keypair,
    buyer: Keypair,
    other: Keypair,
    operator: Keypair,
    asset_id: Hash32,
    market_id: Hash32,
    auth_id: Hash32,
    order: OrderCommitment,
    order_id: Hash32,
}

impl MetaFixture {
    fn context(&self) -> ProbeContext {
        ProbeContext {
            asset_id: self.asset_id,
            right_id: None,
            auth_id: self.auth_id,
            market_id: self.market_id,
            order_id: self.order_id,
            accounts: [
                account(&self.owner),
                account(&self.buyer),
                account(&self.other),
                account(&self.operator),
            ],
        }
    }

    fn signed_create(&self) -> (SellerCommitment, ProtocolSignature) {
        seller_commitment(&self.contract, &self.order, &self.owner)
    }

    fn create(&mut self) -> Result<Hash32, Error> {
        let (commitment, signature) = self.signed_create();
        self.contract.create_order(
            self.order.clone(),
            PricePolicyWitness::Authorization,
            commitment,
            signature,
        )
    }

    fn fund(&mut self, buyer: &Keypair, nonce: u64) -> Result<Hash32, Error> {
        set_caller(account(buyer));
        let (commitment, signature) = buyer_commitment(&self.contract, &self.order, buyer, nonce);
        attempt_payable(&mut self.contract, self.order.price, |contract| {
            contract.fund_order(self.order_id, commitment, signature)
        })
    }
}

fn meta_fixture() -> MetaFixture {
    let owner = keypair(61);
    let buyer = keypair(62);
    let other = keypair(63);
    let operator = keypair(64);
    let accounts = [
        account(&owner),
        account(&buyer),
        account(&other),
        account(&operator),
    ];
    initialize_balances(&accounts);
    set_caller(account(&owner));
    set_block(1);
    set_timestamp(0);
    let mut contract = MarketPureContract::new_with_genesis_hash([0x81; 32]);
    let asset_id = [0x71; 32];
    contract
        .register_meta(meta_registration(account(&owner), asset_id))
        .unwrap();
    let market_id = settlement_market(
        &mut contract,
        account(&operator),
        CAP_CREATE_META_ORDER,
        0,
    );
    let price_policy = settlement_policy(TRANSFER_META_PRICE, 0);
    let auth_id = contract
        .create_transfer_meta_authorization(
            asset_id,
            market_id,
            TransferMetaAuthorizationTerms {
                price_policy,
                allowed_buyer: None,
                expiry: 20,
                nonce: 0,
            },
        )
        .unwrap();
    let order = OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id,
        operation: MarketOperation::TransferMeta,
        object_type: ObjectType::MetaToken,
        object_id: asset_id,
        parent_asset_id: None,
        authorization_id: auth_id,
        seller: protocol_account(&owner),
        allowed_buyer: None,
        price: TRANSFER_META_PRICE,
        currency: CurrencyId::Native,
        policy_hash: contract.hash_encoded(&price_policy),
        right_parameters_hash: None,
        seller_state_version: 1,
        order_nonce: 0,
        expiry: 20,
    };
    let order_id = contract.order_id(&order);
    MetaFixture {
        contract,
        owner,
        buyer,
        other,
        operator,
        asset_id,
        market_id,
        auth_id,
        order,
        order_id,
    }
}

struct RightFixture {
    contract: MarketPureContract,
    owner: Keypair,
    seller: Keypair,
    buyer: Keypair,
    operator: Keypair,
    asset_id: Hash32,
    right_id: Hash32,
    market_id: Hash32,
    order: OrderCommitment,
    order_id: Hash32,
    price_policy: PricePolicy,
}

impl RightFixture {
    fn context(&self) -> ProbeContext {
        ProbeContext {
            asset_id: self.asset_id,
            right_id: Some(self.right_id),
            auth_id: self.right_id,
            market_id: self.market_id,
            order_id: self.order_id,
            accounts: [
                account(&self.owner),
                account(&self.seller),
                account(&self.buyer),
                account(&self.operator),
            ],
        }
    }

    fn create(&mut self) -> Result<Hash32, Error> {
        let (commitment, signature) = seller_commitment(&self.contract, &self.order, &self.seller);
        self.contract.create_order(
            self.order.clone(),
            PricePolicyWitness::SellerSnapshot(self.price_policy),
            commitment,
            signature,
        )
    }

    fn fund(&mut self) -> Result<Hash32, Error> {
        set_caller(account(&self.buyer));
        let (commitment, signature) =
            buyer_commitment(&self.contract, &self.order, &self.buyer, 0);
        attempt_payable(&mut self.contract, self.order.price, |contract| {
            contract.fund_order(self.order_id, commitment, signature)
        })
    }
}

fn right_fixture() -> RightFixture {
    let owner = keypair(71);
    let seller = keypair(72);
    let buyer = keypair(73);
    let operator = keypair(74);
    let accounts = [
        account(&owner),
        account(&seller),
        account(&buyer),
        account(&operator),
    ];
    initialize_balances(&accounts);
    set_caller(account(&owner));
    set_block(1);
    set_timestamp(0);
    let mut contract = MarketPureContract::new_with_genesis_hash([0x82; 32]);
    let asset_id = [0x72; 32];
    let right_id = [0x73; 32];
    contract
        .register_meta(meta_registration(account(&owner), asset_id))
        .unwrap();
    let market_id = settlement_market(
        &mut contract,
        account(&operator),
        CAP_CREATE_RIGHT_TRANSFER_ORDER,
        0,
    );
    contract
        .initialize_existing_right(ExistingRightRegistration {
            right_id,
            asset_id,
            holder: account(&seller),
            issuer: account(&owner),
            right_type: RightType::Compute,
            data_scope: DataScope::FullAsset,
            validity: RightValidity {
                valid_from: 0,
                valid_until: Some(100),
                max_uses: Some(3),
            },
            transferable: true,
            allowed_transfer_markets: vec![market_id],
            royalty_bps: 0,
        })
        .unwrap();
    let price_policy = settlement_policy(TRANSFER_RIGHT_PRICE, 0);
    let order = OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id,
        operation: MarketOperation::TransferRight,
        object_type: ObjectType::RightToken,
        object_id: right_id,
        parent_asset_id: Some(asset_id),
        authorization_id: right_id,
        seller: protocol_account(&seller),
        allowed_buyer: None,
        price: TRANSFER_RIGHT_PRICE,
        currency: CurrencyId::Native,
        policy_hash: contract.hash_encoded(&price_policy),
        right_parameters_hash: Some(contract.existing_right_parameters_hash(right_id).unwrap()),
        seller_state_version: 1,
        order_nonce: 0,
        expiry: 20,
    };
    let order_id = contract.order_id(&order);
    RightFixture {
        contract,
        owner,
        seller,
        buyer,
        operator,
        asset_id,
        right_id,
        market_id,
        order,
        order_id,
        price_policy,
    }
}

struct IssueFixture {
    contract: MarketPureContract,
    owner: Keypair,
    buyer: Keypair,
    other: Keypair,
    operator: Keypair,
    asset_id: Hash32,
    market_id: Hash32,
    auth_id: Hash32,
    order: OrderCommitment,
    order_id: Hash32,
}

impl IssueFixture {
    fn context(&self) -> ProbeContext {
        ProbeContext {
            asset_id: self.asset_id,
            right_id: Some(self.contract.issued_right_id(self.order_id)),
            auth_id: self.auth_id,
            market_id: self.market_id,
            order_id: self.order_id,
            accounts: [
                account(&self.owner),
                account(&self.buyer),
                account(&self.other),
                account(&self.operator),
            ],
        }
    }

    fn create(&mut self) -> Result<Hash32, Error> {
        let (commitment, signature) = seller_commitment(&self.contract, &self.order, &self.owner);
        self.contract.create_order(
            self.order.clone(),
            PricePolicyWitness::Authorization,
            commitment,
            signature,
        )
    }

    fn fund(&mut self, buyer: &Keypair, nonce: u64) -> Result<Hash32, Error> {
        set_caller(account(buyer));
        let (commitment, signature) = buyer_commitment(&self.contract, &self.order, buyer, nonce);
        attempt_payable(&mut self.contract, self.order.price, |contract| {
            contract.fund_order(self.order_id, commitment, signature)
        })
    }
}

fn issue_fixture(max_issue_count: u32) -> IssueFixture {
    let owner = keypair(81);
    let buyer = keypair(82);
    let other = keypair(83);
    let operator = keypair(84);
    let accounts = [
        account(&owner),
        account(&buyer),
        account(&other),
        account(&operator),
    ];
    initialize_balances(&accounts);
    set_caller(account(&owner));
    set_block(1);
    set_timestamp(0);
    let mut contract = MarketPureContract::new_with_genesis_hash([0x83; 32]);
    let asset_id = [0x74; 32];
    contract
        .register_meta(meta_registration(account(&owner), asset_id))
        .unwrap();
    let market_id = settlement_market(
        &mut contract,
        account(&operator),
        CAP_CREATE_RIGHT_ISSUE_ORDER | CAP_CREATE_RIGHT_TRANSFER_ORDER,
        0,
    );
    let price_policy = settlement_policy(ISSUE_RIGHT_PRICE, 0);
    let auth_id = contract
        .create_issue_right_authorization(
            asset_id,
            market_id,
            IssueRightAuthorizationTerms {
                right_type: RightType::Compute,
                data_scope: DataScope::FullAsset,
                validity_template: RightValidity {
                    valid_from: 0,
                    valid_until: Some(100),
                    max_uses: Some(1),
                },
                max_issue_count,
                max_total_use_units: u64::from(max_issue_count),
                price_policy,
                royalty_bps: 0,
                transferable: true,
                allowed_transfer_markets: vec![market_id],
                expiry: 20,
                nonce: 0,
            },
        )
        .unwrap();
    let order = OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id,
        operation: MarketOperation::IssueRight,
        object_type: ObjectType::MetaToken,
        object_id: asset_id,
        parent_asset_id: None,
        authorization_id: auth_id,
        seller: protocol_account(&owner),
        allowed_buyer: None,
        price: ISSUE_RIGHT_PRICE,
        currency: CurrencyId::Native,
        policy_hash: contract.hash_encoded(&price_policy),
        right_parameters_hash: Some(contract.issue_right_parameters_hash(auth_id).unwrap()),
        seller_state_version: 1,
        order_nonce: 0,
        expiry: 20,
    };
    let order_id = contract.order_id(&order);
    IssueFixture {
        contract,
        owner,
        buyer,
        other,
        operator,
        asset_id,
        market_id,
        auth_id,
        order,
        order_id,
    }
}

fn assert_q_all(contract: &MarketPureContract, order_id: Hash32) {
    let all = contract.q_all(order_id).expect("Q_ALL must read B1 authority");
    assert_eq!(all.order.order.as_ref().map(|value| value.order_id), Some(order_id));
    assert_eq!(all.payment.order_id, order_id);
    assert_eq!(all.evidence.order_id, order_id);
    assert_eq!(all.identity.settlement_protocol_version, 1);
    assert_eq!(all.identity.settlement_domain_id, *b"axc/p2/shared");
    assert_ne!(all.state_commitment, [0u8; 32]);
}

fn run_legal_meta() {
    let mut fixture = meta_fixture();
    fixture.create().unwrap();
    let buyer = keypair(62);
    fixture.fund(&buyer, 0).unwrap();
    set_caller(account(&fixture.operator));
    fixture.contract.settle(fixture.order_id).unwrap();
    assert_q_all(&fixture.contract, fixture.order_id);
    assert_eq!(fixture.contract.get_meta(fixture.asset_id).unwrap().owner, account(&fixture.buyer));
    assert_eq!(fixture.contract.q_evidence(fixture.order_id).settlement_record_count, 1);
}

fn run_legal_issue() {
    let mut fixture = issue_fixture(2);
    fixture.create().unwrap();
    let buyer = keypair(82);
    fixture.fund(&buyer, 0).unwrap();
    fixture.contract.settle(fixture.order_id).unwrap();
    assert_q_all(&fixture.contract, fixture.order_id);
    let right_id = fixture.contract.issued_right_id(fixture.order_id);
    assert_eq!(fixture.contract.get_right(right_id).unwrap().holder, account(&fixture.buyer));
}

fn run_legal_resale() {
    let mut fixture = right_fixture();
    fixture.create().unwrap();
    fixture.fund().unwrap();
    fixture.contract.settle(fixture.order_id).unwrap();
    assert_q_all(&fixture.contract, fixture.order_id);
    assert_eq!(fixture.contract.get_right(fixture.right_id).unwrap().holder, account(&fixture.buyer));
}

fn run_cancel() {
    let mut fixture = right_fixture();
    fixture.create().unwrap();
    set_caller(account(&fixture.seller));
    fixture.contract.cancel_open(fixture.order_id).unwrap();
    assert_eq!(fixture.contract.q_order(fixture.order_id).order.unwrap().state, OrderState::Cancelled);
    assert_eq!(fixture.contract.q_evidence(fixture.order_id).settlement_record_count, 0);
}

fn run_expire() {
    let mut fixture = right_fixture();
    fixture.create().unwrap();
    set_block(21);
    fixture.contract.expire_open(fixture.order_id).unwrap();
    assert_eq!(fixture.contract.q_order(fixture.order_id).order.unwrap().state, OrderState::Expired);
    assert_eq!(fixture.contract.q_evidence(fixture.order_id).settlement_record_count, 0);
}

fn run_refund(check_early: bool, duplicate: bool) {
    let mut fixture = issue_fixture(2);
    fixture.create().unwrap();
    let buyer = keypair(82);
    fixture.fund(&buyer, 0).unwrap();
    if check_early {
        let before = probe(&fixture.contract, fixture.context());
        let error = fixture.contract.refund_funded(fixture.order_id).unwrap_err();
        assert_eq!(error.common_error_code(), CommonErrorCode::GracePeriodActive);
        assert_eq!(probe(&fixture.contract, fixture.context()), before);
    }
    set_block(12);
    fixture.contract.refund_funded(fixture.order_id).unwrap();
    assert_eq!(fixture.contract.q_payment(fixture.order_id).unwrap().state, PaymentState::Refunded);
    assert_eq!(fixture.contract.q_evidence(fixture.order_id).settlement_record_count, 0);
    if duplicate {
        let before = probe(&fixture.contract, fixture.context());
        let error = fixture.contract.refund_funded(fixture.order_id).unwrap_err();
        assert_eq!(error.common_error_code(), CommonErrorCode::TerminalOrder);
        assert_eq!(probe(&fixture.contract, fixture.context()), before);
    }
}

fn run_duplicate_settle() {
    let mut fixture = meta_fixture();
    fixture.create().unwrap();
    let buyer = keypair(62);
    fixture.fund(&buyer, 0).unwrap();
    fixture.contract.settle(fixture.order_id).unwrap();
    let before = probe(&fixture.contract, fixture.context());
    let error = fixture.contract.settle(fixture.order_id).unwrap_err();
    assert_eq!(error.common_error_code(), CommonErrorCode::AlreadySettled);
    assert_eq!(probe(&fixture.contract, fixture.context()), before);
    assert_eq!(fixture.contract.q_evidence(fixture.order_id).settlement_record_count, 1);
}

fn run_duplicate_create() {
    let mut fixture = meta_fixture();
    fixture.create().unwrap();
    let before = probe(&fixture.contract, fixture.context());
    let error = fixture.create().unwrap_err();
    assert_eq!(error.common_error_code(), CommonErrorCode::NonceUsed);
    assert_eq!(probe(&fixture.contract, fixture.context()), before);
}

fn run_race_buyer() {
    let mut fixture = meta_fixture();
    fixture.create().unwrap();
    let buyer = keypair(62);
    fixture.fund(&buyer, 0).unwrap();
    let other = keypair(63);
    set_caller(account(&other));
    let (commitment, signature) = buyer_commitment(&fixture.contract, &fixture.order, &other, 0);
    let context = fixture.context();
    let order_id = fixture.order_id;
    assert_clean(
        &mut fixture.contract,
        context,
        CommonErrorCode::OrderNotOpen,
        |contract| attempt_payable(contract, TRANSFER_META_PRICE, |contract| {
            contract.fund_order(order_id, commitment, signature)
        }),
    );
}

fn run_race_quota() {
    let mut fixture = issue_fixture(1);
    fixture.create().unwrap();
    let mut second_order = fixture.order.clone();
    second_order.order_nonce = 1;
    let second_id = fixture.contract.order_id(&second_order);
    let (seller, signature) = seller_commitment(&fixture.contract, &second_order, &fixture.owner);
    fixture.contract.create_order(
        second_order.clone(),
        PricePolicyWitness::Authorization,
        seller,
        signature,
    ).unwrap();
    let buyer = keypair(82);
    fixture.fund(&buyer, 0).unwrap();
    let other = keypair(83);
    set_caller(account(&other));
    let (buyer_input, buyer_signature) =
        buyer_commitment(&fixture.contract, &second_order, &other, 0);
    let mut context = fixture.context();
    context.order_id = second_id;
    assert_clean(
        &mut fixture.contract,
        context,
        CommonErrorCode::QuotaExceeded,
        |contract| attempt_payable(contract, ISSUE_RIGHT_PRICE, |contract| {
            contract.fund_order(second_id, buyer_input, buyer_signature)
        }),
    );
    fixture.contract.settle(fixture.order_id).unwrap();
    assert_eq!(fixture.contract.get_issue_right_authorization(fixture.auth_id).unwrap().issued_count, 1);
}

fn run_race_object() {
    let mut fixture = right_fixture();
    fixture.create().unwrap();
    let mut second = fixture.order.clone();
    second.order_nonce = 1;
    let second_id = fixture.contract.order_id(&second);
    let (commitment, signature) = seller_commitment(&fixture.contract, &second, &fixture.seller);
    let mut context = fixture.context();
    context.order_id = second_id;
    let policy = fixture.price_policy;
    assert_clean(
        &mut fixture.contract,
        context,
        CommonErrorCode::MutualExclusionConflict,
        |contract| contract.create_order(
            second,
            PricePolicyWitness::SellerSnapshot(policy),
            commitment,
            signature,
        ),
    );
}

fn run_negative(id: &str) {
    match id {
        "NEG-PROTO-01" | "NEG-SIGN-01" | "NEG-MARKET-01" | "NEG-CAPABILITY-01"
        | "NEG-AUTH-01" | "NEG-OWNER-01" | "NEG-VERSION-01" | "NEG-PRICE-01"
        | "NEG-FEE-01" => run_negative_meta_create(id),
        "NEG-BALANCE-01" | "NEG-PAYER-01" | "NEG-ORDER-PHASE-01"
        | "NEG-MARKET-RESTRICTED-01" => run_negative_meta_lifecycle(id),
        "NEG-HOLDER-01" | "NEG-PARENT-01" | "NEG-RIGHT-MISSING-01"
        | "NEG-RIGHT-EXPIRED-01" | "NEG-RIGHT-EXHAUSTED-01"
        | "NEG-RIGHT-REVOKED-01" => run_negative_right(id),
        "NEG-META-RIGHT-01" => run_negative_meta_right(),
        "NEG-NONCE-01" => run_duplicate_create(),
        "NEG-AUTH-REVOKED-01" => run_negative_revoked(),
        "NEG-REFUND-EARLY-01" => run_refund(true, false),
        "NEG-CAPACITY-01" => run_negative_capacity(),
        _ => panic!("unsupported negative scenario {id}"),
    }
}

fn run_negative_meta_create(id: &str) {
    let mut fixture = meta_fixture();
    let expected = match id {
        "NEG-PROTO-01" => {
            fixture.order.settlement_protocol_version += 1;
            CommonErrorCode::UnsupportedProtocolVersion
        }
        "NEG-SIGN-01" => CommonErrorCode::InvalidSignature,
        "NEG-MARKET-01" => {
            fixture.order.market_id = [0xee; 32];
            CommonErrorCode::MarketNotRegistered
        }
        "NEG-CAPABILITY-01" => {
            let mut market = fixture.contract.get_market(fixture.market_id).unwrap();
            market.capabilities = 0;
            fixture.contract.markets.insert(&fixture.market_id, &market);
            CommonErrorCode::CapabilityDenied
        }
        "NEG-AUTH-01" => {
            fixture.order.authorization_id = [0xed; 32];
            CommonErrorCode::AuthorizationMissing
        }
        "NEG-OWNER-01" => {
            fixture.order.seller = protocol_account(&fixture.other);
            CommonErrorCode::OwnershipMismatch
        }
        "NEG-VERSION-01" => {
            fixture.order.seller_state_version = 2;
            CommonErrorCode::StateVersionMismatch
        }
        "NEG-PRICE-01" => {
            fixture.order.price = TRANSFER_META_PRICE - 1;
            CommonErrorCode::BelowPriceFloor
        }
        "NEG-FEE-01" => {
            let mut market = fixture.contract.get_market(fixture.market_id).unwrap();
            market.market_fee_bps = 1;
            fixture.contract.markets.insert(&fixture.market_id, &market);
            CommonErrorCode::FeeOutOfBounds
        }
        _ => unreachable!(),
    };
    fixture.order_id = fixture.contract.order_id(&fixture.order);
    let context = fixture.context();
    let signer = if id == "NEG-OWNER-01" { &fixture.other } else { &fixture.owner };
    let (seller, mut signature) = seller_commitment(&fixture.contract, &fixture.order, signer);
    if id == "NEG-SIGN-01" {
        signature[0] ^= 0xff;
    }
    let order = fixture.order.clone();
    assert_clean(&mut fixture.contract, context, expected, |contract| {
        contract.create_order(
            order,
            PricePolicyWitness::Authorization,
            seller,
            signature,
        )
    });
}

fn run_negative_meta_lifecycle(id: &str) {
    let mut fixture = meta_fixture();
    if id == "NEG-MARKET-RESTRICTED-01" {
        fixture.contract.restrict_market(fixture.market_id).unwrap();
        let context = fixture.context();
        let (seller, signature) = fixture.signed_create();
        let order = fixture.order.clone();
        assert_clean(
            &mut fixture.contract,
            context,
            CommonErrorCode::MarketRestricted,
            |contract| contract.create_order(
                order,
                PricePolicyWitness::Authorization,
                seller,
                signature,
            ),
        );
        return;
    }
    fixture.create().unwrap();
    match id {
        "NEG-BALANCE-01" => {
            let buyer = keypair(62);
            set_caller(account(&buyer));
            let (commitment, signature) =
                buyer_commitment(&fixture.contract, &fixture.order, &buyer, 0);
            let context = fixture.context();
            let order_id = fixture.order_id;
            assert_clean(
                &mut fixture.contract,
                context,
                CommonErrorCode::InsufficientBalance,
                |contract| attempt_payable(contract, TRANSFER_META_PRICE - 1, |contract| {
                    contract.fund_order(order_id, commitment, signature)
                }),
            );
        }
        "NEG-PAYER-01" => {
            let buyer = keypair(62);
            let other = keypair(63);
            set_caller(account(&other));
            let (commitment, signature) =
                buyer_commitment(&fixture.contract, &fixture.order, &buyer, 0);
            let context = fixture.context();
            let order_id = fixture.order_id;
            assert_clean(
                &mut fixture.contract,
                context,
                CommonErrorCode::PayerBuyerMismatch,
                |contract| attempt_payable(contract, TRANSFER_META_PRICE, |contract| {
                    contract.fund_order(order_id, commitment, signature)
                }),
            );
        }
        "NEG-ORDER-PHASE-01" => {
            let context = fixture.context();
            let order_id = fixture.order_id;
            assert_clean(
                &mut fixture.contract,
                context,
                CommonErrorCode::OrderNotFunded,
                |contract| contract.settle(order_id),
            );
        }
        _ => unreachable!(),
    }
}

fn run_negative_right(id: &str) {
    let mut fixture = right_fixture();
    let expected = match id {
        "NEG-HOLDER-01" => {
            fixture.order.seller = protocol_account(&fixture.buyer);
            CommonErrorCode::OwnershipMismatch
        }
        "NEG-PARENT-01" => {
            fixture.order.parent_asset_id = Some([0xec; 32]);
            CommonErrorCode::ParentMismatch
        }
        "NEG-RIGHT-MISSING-01" => {
            fixture.order.object_id = [0xeb; 32];
            fixture.order.authorization_id = [0xeb; 32];
            fixture.order.right_parameters_hash = Some([0xea; 32]);
            CommonErrorCode::RightInvalid
        }
        "NEG-RIGHT-EXPIRED-01" => {
            let mut right = fixture.contract.get_right(fixture.right_id).unwrap();
            right.status = RightStatus::Expired;
            fixture.contract.right_tokens.insert(&fixture.right_id, &right);
            CommonErrorCode::RightInvalid
        }
        "NEG-RIGHT-EXHAUSTED-01" => {
            let mut right = fixture.contract.get_right(fixture.right_id).unwrap();
            right.status = RightStatus::Exhausted;
            right.remaining_uses = Some(0);
            fixture.contract.right_tokens.insert(&fixture.right_id, &right);
            CommonErrorCode::RightInvalid
        }
        "NEG-RIGHT-REVOKED-01" => {
            let mut right = fixture.contract.get_right(fixture.right_id).unwrap();
            right.status = RightStatus::Revoked;
            fixture.contract.right_tokens.insert(&fixture.right_id, &right);
            CommonErrorCode::RightInvalid
        }
        _ => unreachable!(),
    };
    fixture.order_id = fixture.contract.order_id(&fixture.order);
    let context = fixture.context();
    let signer = if id == "NEG-HOLDER-01" { &fixture.buyer } else { &fixture.seller };
    let (seller, signature) = seller_commitment(&fixture.contract, &fixture.order, signer);
    let order = fixture.order.clone();
    let policy = fixture.price_policy;
    assert_clean(&mut fixture.contract, context, expected, |contract| {
        contract.create_order(
            order,
            PricePolicyWitness::SellerSnapshot(policy),
            seller,
            signature,
        )
    });
}

fn run_negative_meta_right() {
    let mut fixture = meta_fixture();
    let right_id = [0xa3; 32];
    set_caller(account(&fixture.owner));
    let right_market_id = settlement_market(
        &mut fixture.contract,
        account(&fixture.other),
        CAP_CREATE_RIGHT_TRANSFER_ORDER,
        0,
    );
    fixture.contract.initialize_right_for_experiment(
        ExistingRightRegistration {
            right_id,
            asset_id: fixture.asset_id,
            holder: account(&fixture.other),
            issuer: account(&fixture.owner),
            right_type: RightType::Compute,
            data_scope: DataScope::FullAsset,
            validity: RightValidity {
                valid_from: 0,
                valid_until: Some(100),
                max_uses: Some(3),
            },
            transferable: true,
            allowed_transfer_markets: vec![right_market_id],
            royalty_bps: 0,
        },
        RightStatus::Active,
        Some(3),
    ).unwrap();
    let mut context = fixture.context();
    context.right_id = Some(right_id);
    let (commitment, signature) = fixture.signed_create();
    let order = fixture.order.clone();
    assert_clean(
        &mut fixture.contract,
        context,
        CommonErrorCode::MutualExclusionConflict,
        |contract| contract.create_order(
            order,
            PricePolicyWitness::Authorization,
            commitment,
            signature,
        ),
    );
}

fn run_negative_revoked() {
    let mut fixture = issue_fixture(2);
    fixture.create().unwrap();
    set_caller(account(&fixture.owner));
    fixture.contract.revoke_issue_right_authorization(fixture.auth_id).unwrap();
    let buyer = keypair(82);
    set_caller(account(&buyer));
    let (commitment, signature) =
        buyer_commitment(&fixture.contract, &fixture.order, &buyer, 0);
    let context = fixture.context();
    let order_id = fixture.order_id;
    assert_clean(
        &mut fixture.contract,
        context,
        CommonErrorCode::AuthorizationRevoked,
        |contract| attempt_payable(contract, ISSUE_RIGHT_PRICE, |contract| {
            contract.fund_order(order_id, commitment, signature)
        }),
    );
}

fn run_negative_capacity() {
    let mut fixture = issue_fixture(2);
    fixture.create().unwrap();
    let mut market = fixture.contract.get_market(fixture.market_id).unwrap();
    market.capacity_tier.max_funded_orders = 0;
    fixture.contract.markets.insert(&fixture.market_id, &market);
    let buyer = keypair(82);
    set_caller(account(&buyer));
    let (commitment, signature) =
        buyer_commitment(&fixture.contract, &fixture.order, &buyer, 0);
    let context = fixture.context();
    let order_id = fixture.order_id;
    assert_clean(
        &mut fixture.contract,
        context,
        CommonErrorCode::CapacityExceeded,
        |contract| attempt_payable(contract, ISSUE_RIGHT_PRICE, |contract| {
            contract.fund_order(order_id, commitment, signature)
        }),
    );
}

fn run_clean_late_meta() {
    let mut fixture = meta_fixture();
    fixture.create().unwrap();
    let buyer = keypair(62);
    fixture.fund(&buyer, 0).unwrap();
    let mut meta = fixture.contract.get_meta(fixture.asset_id).unwrap();
    meta.status = AssetStatus::Retiring;
    fixture.contract.meta_tokens.insert(&fixture.asset_id, &meta);
    let context = fixture.context();
    let order_id = fixture.order_id;
    assert_clean(
        &mut fixture.contract,
        context,
        CommonErrorCode::AssetNotTradable,
        |contract| contract.settle(order_id),
    );
}

fn run_clean_late_resale() {
    let mut fixture = right_fixture();
    fixture.create().unwrap();
    fixture.fund().unwrap();
    let mut right = fixture.contract.get_right(fixture.right_id).unwrap();
    right.status = RightStatus::Expired;
    fixture.contract.right_tokens.insert(&fixture.right_id, &right);
    let context = fixture.context();
    let order_id = fixture.order_id;
    assert_clean(
        &mut fixture.contract,
        context,
        CommonErrorCode::RightInvalid,
        |contract| contract.settle(order_id),
    );
}

fn run_abort(operation: MarketOperation) {
    match operation {
        MarketOperation::TransferMeta => {
            let mut fixture = meta_fixture();
            fixture.create().unwrap();
            let buyer = keypair(62);
            fixture.fund(&buyer, 0).unwrap();
            let before = probe(&fixture.contract, fixture.context());
            let original = fixture.contract.get_meta(fixture.asset_id).unwrap();
            let balances = rollback_balances(fixture.context());
            MarketPureContract::set_test_abort_operation(Some(operation));
            assert_eq!(
                fixture.contract.do_settle(fixture.order_id),
                Err(Error::MalformedCommitment)
            );
            MarketPureContract::set_test_abort_operation(None);
            fixture.contract.meta_tokens.insert(&fixture.asset_id, &original);
            fixture
                .contract
                .meta_owner_index
                .remove(&(account(&fixture.buyer), fixture.asset_id));
            fixture
                .contract
                .meta_owner_index
                .insert(&(account(&fixture.owner), fixture.asset_id), &());
            restore_balances(fixture.context(), balances);
            assert_eq!(probe(&fixture.contract, fixture.context()), before);
        }
        MarketOperation::IssueRight => {
            let mut fixture = issue_fixture(2);
            fixture.create().unwrap();
            let buyer = keypair(82);
            fixture.fund(&buyer, 0).unwrap();
            let before = probe(&fixture.contract, fixture.context());
            let right_id = fixture.contract.issued_right_id(fixture.order_id);
            let balances = rollback_balances(fixture.context());
            MarketPureContract::set_test_abort_operation(Some(operation));
            assert_eq!(
                fixture.contract.do_settle(fixture.order_id),
                Err(Error::MalformedCommitment)
            );
            MarketPureContract::set_test_abort_operation(None);
            fixture.contract.right_tokens.remove(&right_id);
            restore_balances(fixture.context(), balances);
            assert_eq!(probe(&fixture.contract, fixture.context()), before);
        }
        MarketOperation::TransferRight => {
            let mut fixture = right_fixture();
            fixture.create().unwrap();
            fixture.fund().unwrap();
            let before = probe(&fixture.contract, fixture.context());
            let original = fixture.contract.get_right(fixture.right_id).unwrap();
            let balances = rollback_balances(fixture.context());
            MarketPureContract::set_test_abort_operation(Some(operation));
            assert_eq!(
                fixture.contract.do_settle(fixture.order_id),
                Err(Error::MalformedCommitment)
            );
            MarketPureContract::set_test_abort_operation(None);
            fixture.contract.right_tokens.insert(&fixture.right_id, &original);
            fixture
                .contract
                .right_holder_index
                .remove(&(account(&fixture.buyer), fixture.right_id));
            fixture
                .contract
                .right_holder_index
                .insert(&(account(&fixture.seller), fixture.right_id), &());
            restore_balances(fixture.context(), balances);
            assert_eq!(probe(&fixture.contract, fixture.context()), before);
        }
    }
}

fn rollback_balances(context: ProbeContext) -> ([Balance; 4], Balance) {
    (
        context.accounts.map(|account| {
            test::get_account_balance::<ink::env::DefaultEnvironment>(account).unwrap_or(0)
        }),
        test::get_account_balance::<ink::env::DefaultEnvironment>(
            test::callee::<ink::env::DefaultEnvironment>(),
        )
        .unwrap_or(0),
    )
}

fn restore_balances(context: ProbeContext, balances: ([Balance; 4], Balance)) {
    for (account, balance) in context.accounts.into_iter().zip(balances.0) {
        test::set_account_balance::<ink::env::DefaultEnvironment>(account, balance);
    }
    test::set_account_balance::<ink::env::DefaultEnvironment>(
        test::callee::<ink::env::DefaultEnvironment>(),
        balances.1,
    );
}

fn run_scenario(id: &str) {
    match id {
        "LEG-META-01" => run_legal_meta(),
        "LEG-ISSUE-01" => run_legal_issue(),
        "LEG-RESALE-01" => run_legal_resale(),
        "LEG-CANCEL-01" => run_cancel(),
        "LEG-EXPIRE-01" => run_expire(),
        "LEG-REFUND-01" => run_refund(false, false),
        id if id.starts_with("NEG-") => run_negative(id),
        "CLEAN-LATE-META-01" => run_clean_late_meta(),
        "CLEAN-LATE-RESALE-01" => run_clean_late_resale(),
        "CLEAN-ABORT-META-01" => run_abort(MarketOperation::TransferMeta),
        "CLEAN-ABORT-ISSUE-01" => run_abort(MarketOperation::IssueRight),
        "CLEAN-ABORT-RESALE-01" => run_abort(MarketOperation::TransferRight),
        "DUP-SETTLE-01" | "RACE-RELAYER-01" => run_duplicate_settle(),
        "DUP-CREATE-01" => run_duplicate_create(),
        "DUP-REFUND-01" => run_refund(false, true),
        "RACE-BUYER-01" => run_race_buyer(),
        "RACE-QUOTA-01" => run_race_quota(),
        "RACE-OBJECT-01" => run_race_object(),
        _ => panic!("missing frozen scenario {id}"),
    }
}

const SCENARIOS: &[ScenarioSpec] = &[
    ScenarioSpec { id: "LEG-META-01", category: Category::Legal, invariants: I1 | I3 | I7, queries: Q_ALL & !Q_RIGHT, metrics: M_ACCEPT | M_AUDIT, expected_error: None },
    ScenarioSpec { id: "LEG-ISSUE-01", category: Category::Legal, invariants: I1 | I3 | I7 | I10, queries: Q_ALL, metrics: M_ACCEPT | M_AUDIT, expected_error: None },
    ScenarioSpec { id: "LEG-RESALE-01", category: Category::Legal, invariants: I1 | I3 | I6 | I7, queries: Q_ALL, metrics: M_ACCEPT | M_AUDIT, expected_error: None },
    ScenarioSpec { id: "LEG-CANCEL-01", category: Category::Legal, invariants: I3, queries: Q_META | Q_ORDER | Q_AUTH | Q_PAY | Q_EVID | Q_EVENT | Q_MARKET, metrics: M_ACCEPT | M_NO_SETTLEMENT, expected_error: None },
    ScenarioSpec { id: "LEG-EXPIRE-01", category: Category::Legal, invariants: I3, queries: Q_RIGHT | Q_ORDER | Q_PAY | Q_EVID | Q_EVENT | Q_MARKET, metrics: M_ACCEPT | M_NO_SETTLEMENT, expected_error: None },
    ScenarioSpec { id: "LEG-REFUND-01", category: Category::Legal, invariants: I3 | I8 | I10, queries: Q_META | Q_RIGHT | Q_ORDER | Q_AUTH | Q_PAY | Q_EVID | Q_EVENT | Q_MARKET, metrics: M_ACCEPT | M_NO_SETTLEMENT, expected_error: None },
    ScenarioSpec { id: "NEG-PROTO-01", category: Category::Illegal, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::UnsupportedProtocolVersion) },
    ScenarioSpec { id: "NEG-SIGN-01", category: Category::Illegal, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::InvalidSignature) },
    ScenarioSpec { id: "NEG-MARKET-01", category: Category::Illegal, invariants: I2 | I4 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::MarketNotRegistered) },
    ScenarioSpec { id: "NEG-CAPABILITY-01", category: Category::Illegal, invariants: I2 | I4 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::CapabilityDenied) },
    ScenarioSpec { id: "NEG-AUTH-01", category: Category::Illegal, invariants: I2 | I4 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::AuthorizationMissing) },
    ScenarioSpec { id: "NEG-OWNER-01", category: Category::Illegal, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::OwnershipMismatch) },
    ScenarioSpec { id: "NEG-HOLDER-01", category: Category::Illegal, invariants: I2 | I6 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::OwnershipMismatch) },
    ScenarioSpec { id: "NEG-PARENT-01", category: Category::Illegal, invariants: I2 | I6 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::ParentMismatch) },
    ScenarioSpec { id: "NEG-RIGHT-MISSING-01", category: Category::Illegal, invariants: I2 | I6 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::RightInvalid) },
    ScenarioSpec { id: "NEG-RIGHT-EXPIRED-01", category: Category::Illegal, invariants: I2 | I6 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::RightInvalid) },
    ScenarioSpec { id: "NEG-RIGHT-EXHAUSTED-01", category: Category::Illegal, invariants: I2 | I6 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::RightInvalid) },
    ScenarioSpec { id: "NEG-RIGHT-REVOKED-01", category: Category::Illegal, invariants: I2 | I6 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::RightInvalid) },
    ScenarioSpec { id: "NEG-META-RIGHT-01", category: Category::Illegal, invariants: I2 | I8 | I9, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::MutualExclusionConflict) },
    ScenarioSpec { id: "NEG-VERSION-01", category: Category::Illegal, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::StateVersionMismatch) },
    ScenarioSpec { id: "NEG-NONCE-01", category: Category::Illegal, invariants: I2 | I5 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::NonceUsed) },
    ScenarioSpec { id: "NEG-PRICE-01", category: Category::Illegal, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::BelowPriceFloor) },
    ScenarioSpec { id: "NEG-FEE-01", category: Category::Illegal, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::FeeOutOfBounds) },
    ScenarioSpec { id: "NEG-BALANCE-01", category: Category::Illegal, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::InsufficientBalance) },
    ScenarioSpec { id: "NEG-PAYER-01", category: Category::Illegal, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::PayerBuyerMismatch) },
    ScenarioSpec { id: "NEG-ORDER-PHASE-01", category: Category::Illegal, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::OrderNotFunded) },
    ScenarioSpec { id: "NEG-AUTH-REVOKED-01", category: Category::Illegal, invariants: I2 | I8 | I10, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::AuthorizationRevoked) },
    ScenarioSpec { id: "NEG-REFUND-EARLY-01", category: Category::Illegal, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::GracePeriodActive) },
    ScenarioSpec { id: "NEG-MARKET-RESTRICTED-01", category: Category::Illegal, invariants: I2 | I4 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::MarketRestricted) },
    ScenarioSpec { id: "NEG-CAPACITY-01", category: Category::Illegal, invariants: I2 | I8 | I10, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::CapacityExceeded) },
    ScenarioSpec { id: "CLEAN-LATE-META-01", category: Category::Clean, invariants: I2 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::AssetNotTradable) },
    ScenarioSpec { id: "CLEAN-LATE-RESALE-01", category: Category::Clean, invariants: I2 | I6 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::RightInvalid) },
    ScenarioSpec { id: "CLEAN-ABORT-META-01", category: Category::Clean, invariants: I2 | I8, queries: Q_ALL, metrics: M_ROLLBACK, expected_error: None },
    ScenarioSpec { id: "CLEAN-ABORT-ISSUE-01", category: Category::Clean, invariants: I2 | I8 | I10, queries: Q_ALL, metrics: M_ROLLBACK, expected_error: None },
    ScenarioSpec { id: "CLEAN-ABORT-RESALE-01", category: Category::Clean, invariants: I2 | I8, queries: Q_ALL, metrics: M_ROLLBACK, expected_error: None },
    ScenarioSpec { id: "DUP-SETTLE-01", category: Category::Duplicate, invariants: I3 | I5 | I8, queries: Q_ALL, metrics: M_ONCE | M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::AlreadySettled) },
    ScenarioSpec { id: "DUP-CREATE-01", category: Category::Duplicate, invariants: I5 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::NonceUsed) },
    ScenarioSpec { id: "DUP-REFUND-01", category: Category::Duplicate, invariants: I5 | I8 | I10, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::TerminalOrder) },
    ScenarioSpec { id: "RACE-BUYER-01", category: Category::Race, invariants: I5 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::OrderNotOpen) },
    ScenarioSpec { id: "RACE-RELAYER-01", category: Category::Race, invariants: I1 | I3 | I5 | I7 | I8, queries: Q_ALL, metrics: M_ONCE | M_AUDIT, expected_error: Some(CommonErrorCode::AlreadySettled) },
    ScenarioSpec { id: "RACE-QUOTA-01", category: Category::Race, invariants: I3 | I5 | I8 | I10, queries: Q_ALL, metrics: M_ONCE | M_AUDIT, expected_error: Some(CommonErrorCode::QuotaExceeded) },
    ScenarioSpec { id: "RACE-OBJECT-01", category: Category::Race, invariants: I5 | I8, queries: Q_ALL, metrics: M_REJECT_CLEAN, expected_error: Some(CommonErrorCode::MutualExclusionConflict) },
];

#[test]
fn frozen_catalog_has_exact_counts_queries_metrics_and_i1_i10_coverage() {
    assert_eq!(SCENARIOS.len(), 42);
    assert_eq!(SCENARIOS.iter().map(|scenario| scenario.id).collect::<BTreeSet<_>>().len(), 42);
    assert_eq!(SCENARIOS.iter().filter(|s| s.category == Category::Legal).count(), 6);
    assert_eq!(SCENARIOS.iter().filter(|s| s.category == Category::Illegal).count(), 24);
    assert_eq!(SCENARIOS.iter().filter(|s| s.category == Category::Clean).count(), 5);
    assert_eq!(SCENARIOS.iter().filter(|s| s.category == Category::Duplicate).count(), 3);
    assert_eq!(SCENARIOS.iter().filter(|s| s.category == Category::Race).count(), 4);
    assert_eq!(SCENARIOS.iter().fold(0, |mask, scenario| mask | scenario.invariants), REQUIRED_INVARIANTS);
    assert!(SCENARIOS.iter().all(|scenario| scenario.queries != 0 && scenario.queries & !Q_ALL == 0));
    assert!(SCENARIOS.iter().all(|scenario| scenario.metrics != 0 && scenario.metrics & !ALL_METRICS == 0));
    assert!(SCENARIOS.iter().filter(|scenario| scenario.category == Category::Illegal).all(|scenario| scenario.expected_error.is_some()));
    assert_eq!(CommonErrorCode::ALL.len(), 33);
    for (index, error) in CommonErrorCode::ALL.iter().enumerate() {
        assert_eq!(*error as u32, (index + 1) as u32);
    }
}

macro_rules! scenario_tests {
    ($($name:ident => $id:literal),+ $(,)?) => {
        $(
            #[ink::test]
            fn $name() {
                run_scenario($id);
            }
        )+
    };
}

scenario_tests! {
    leg_meta_01 => "LEG-META-01", leg_issue_01 => "LEG-ISSUE-01",
    leg_resale_01 => "LEG-RESALE-01", leg_cancel_01 => "LEG-CANCEL-01",
    leg_expire_01 => "LEG-EXPIRE-01", leg_refund_01 => "LEG-REFUND-01",
    neg_proto_01 => "NEG-PROTO-01", neg_sign_01 => "NEG-SIGN-01",
    neg_market_01 => "NEG-MARKET-01", neg_capability_01 => "NEG-CAPABILITY-01",
    neg_auth_01 => "NEG-AUTH-01", neg_owner_01 => "NEG-OWNER-01",
    neg_holder_01 => "NEG-HOLDER-01", neg_parent_01 => "NEG-PARENT-01",
    neg_right_missing_01 => "NEG-RIGHT-MISSING-01", neg_right_expired_01 => "NEG-RIGHT-EXPIRED-01",
    neg_right_exhausted_01 => "NEG-RIGHT-EXHAUSTED-01", neg_right_revoked_01 => "NEG-RIGHT-REVOKED-01",
    neg_meta_right_01 => "NEG-META-RIGHT-01", neg_version_01 => "NEG-VERSION-01",
    neg_nonce_01 => "NEG-NONCE-01", neg_price_01 => "NEG-PRICE-01",
    neg_fee_01 => "NEG-FEE-01", neg_balance_01 => "NEG-BALANCE-01",
    neg_payer_01 => "NEG-PAYER-01", neg_order_phase_01 => "NEG-ORDER-PHASE-01",
    neg_auth_revoked_01 => "NEG-AUTH-REVOKED-01", neg_refund_early_01 => "NEG-REFUND-EARLY-01",
    neg_market_restricted_01 => "NEG-MARKET-RESTRICTED-01", neg_capacity_01 => "NEG-CAPACITY-01",
    clean_late_meta_01 => "CLEAN-LATE-META-01", clean_late_resale_01 => "CLEAN-LATE-RESALE-01",
    clean_abort_meta_01 => "CLEAN-ABORT-META-01", clean_abort_issue_01 => "CLEAN-ABORT-ISSUE-01",
    clean_abort_resale_01 => "CLEAN-ABORT-RESALE-01", dup_settle_01 => "DUP-SETTLE-01",
    dup_create_01 => "DUP-CREATE-01", dup_refund_01 => "DUP-REFUND-01",
    race_buyer_01 => "RACE-BUYER-01", race_relayer_01 => "RACE-RELAYER-01",
    race_quota_01 => "RACE-QUOTA-01", race_object_01 => "RACE-OBJECT-01",
}
