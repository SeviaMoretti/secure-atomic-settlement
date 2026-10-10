use crate::{
    buyer_signing_payload, domain_separator, escrow_id, mock::*, order_id, price_policy_hash,
    right_id, right_parameters_hash, seller_signing_payload, settlement_id,
    ActiveIssueAuthorizationCount, ActiveIssueOrderCount, ActiveMetaOrder, ActiveRightCount,
    ActiveRightOrder, AllowedMarkets, AssetRightIds, AssetStatus, AuthorizationNonce,
    AuthorizationStatus, BuyerCommitment, BuyerNonce, CurrencyId, DataScope, Error,
    ExistingRightRegistration, IssueQuotaReservationStatus, IssueQuotaReservations,
    IssueRightAuthorizationTerms, IssueRightAuthorizations, MarketCapacityTier, MarketOperation,
    MarketStatus, Markets, MessageType, MetaOwnerIndex, MetaRegistration, MetaTokens, ObjectType,
    OrderCommitment, OrderState, Pallet, PaymentState, PricePolicy, PricePolicyWitness,
    R2X1LastPolicyDecision, RightHolderIndex, RightParentIndex, RightStatus, RightTokens,
    RightType, RightValidity, RuntimeEscrows, RuntimeOrders, SellerCommitment, SellerNonce,
    SettlementRecords, TransferMetaAuthorizationTerms, TransferMetaAuthorizations,
    ALL_CAPABILITIES, CAP_CREATE_META_ORDER, CAP_CREATE_RIGHT_ISSUE_ORDER,
    CAP_CREATE_RIGHT_TRANSFER_ORDER, SETTLEMENT_PROTOCOL_VERSION,
};
use codec::Encode;
use frame_support::traits::ReservableCurrency;
use frame_support::{assert_noop, assert_ok};
use sp_core::Pair;

const ASSET: [u8; 32] = [0x41; 32];
const RIGHT: [u8; 32] = [0x42; 32];
const MARKET: [u8; 32] = [0x43; 32];
const META_PRICE: u128 = 10_000_000_000_000;
const ISSUE_PRICE: u128 = 3_000_000_000_000;
const RESALE_PRICE: u128 = 4_000_000_000_000;
const INITIAL_BALANCE: u128 = 100_000_000_000_000;

fn capacity() -> MarketCapacityTier {
    MarketCapacityTier {
        tier_id: 1,
        max_active_orders: 8,
        max_funded_orders: 4,
        max_active_exposure_value: 100_000_000_000_000,
    }
}

fn policy(reference_price: u128) -> PricePolicy {
    PricePolicy {
        reference_price,
        floor_ratio_bps: 10_000,
        signed_market_fee_cap_bps: 0,
        safety_fee_bps: 0,
    }
}

fn register_meta() {
    assert_ok!(MarketRuntime::register_meta(
        RuntimeOrigin::signed(OWNER),
        MetaRegistration {
            asset_id: ASSET,
            owner: OWNER,
            registration_provider: OWNER,
            service_provider: OWNER,
            metadata_digest: [0x10; 32],
            data_commitment: [0x11; 32],
            validation_program_hash: [0x12; 32],
            status: crate::AssetStatus::Tradable,
            parent_asset_id: None,
            supersedes_asset_id: None,
        }
    ));
}

fn register_market(capabilities: u8) {
    assert_ok!(MarketRuntime::register_market(
        RuntimeOrigin::root(),
        MARKET,
        MARKET_OPERATOR,
        SETTLEMENT_PROTOCOL_VERSION,
        capabilities,
        0,
        capacity(),
        1_000,
    ));
}

fn allowed_markets() -> AllowedMarkets {
    vec![MARKET].try_into().unwrap()
}

fn issue_terms(expiry: u32) -> IssueRightAuthorizationTerms {
    IssueRightAuthorizationTerms {
        right_type: RightType::Compute,
        data_scope: DataScope::FullAsset,
        validity_template: RightValidity {
            valid_from: 1,
            valid_until: Some(101),
            max_uses: Some(3),
        },
        max_issue_count: 2,
        max_total_use_units: 6,
        price_policy: policy(3_000_000_000_000),
        royalty_bps: 0,
        transferable: true,
        allowed_transfer_markets: allowed_markets(),
        expiry,
        nonce: 0,
    }
}

fn sole_issue_auth_id() -> [u8; 32] {
    IssueRightAuthorizations::<Test>::iter_keys()
        .next()
        .unwrap()
}

fn sole_transfer_auth_id() -> [u8; 32] {
    TransferMetaAuthorizations::<Test>::iter_keys()
        .next()
        .unwrap()
}

fn protocol_account(account: AccountId) -> [u8; 32] {
    <TestAccountConverter as crate::ProtocolAccountIdConverter<AccountId>>::to_protocol_account_id(
        &account,
    )
    .unwrap()
}

fn genesis_hash() -> [u8; 32] {
    System::block_hash(0).0
}

fn create_signed_order(
    operation: MarketOperation,
    object_id: [u8; 32],
    parent_asset_id: Option<[u8; 32]>,
    authorization_id: [u8; 32],
    seller: AccountId,
    allowed_buyer: Option<AccountId>,
    price: u128,
    policy: PricePolicy,
    parameters_hash: Option<[u8; 32]>,
    nonce: u64,
    witness: PricePolicyWitness,
) -> ([u8; 32], OrderCommitment) {
    let order = OrderCommitment {
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
        allowed_buyer: allowed_buyer.map(protocol_account),
        price,
        currency: CurrencyId::Native,
        policy_hash: price_policy_hash(&policy),
        right_parameters_hash: parameters_hash,
        seller_state_version: 1,
        order_nonce: nonce,
        expiry: 20,
    };
    let id = order_id(genesis_hash(), &order);
    let commitment = SellerCommitment {
        order_id: id,
        object_id,
        operation,
        seller: order.seller,
        state_version: order.seller_state_version,
        price_policy_hash: order.policy_hash,
        right_parameters_hash: parameters_hash,
        expires_at: order.expiry,
        seller_nonce: nonce,
    };
    let signature = test_pair(seller)
        .sign(&seller_signing_payload(
            genesis_hash(),
            operation,
            &commitment,
        ))
        .0;
    assert_ok!(MarketRuntime::create_order(
        RuntimeOrigin::signed(OTHER),
        order.clone(),
        witness,
        commitment,
        signature,
    ));
    (id, order)
}

fn fund_signed_order(order_id: [u8; 32], order: &OrderCommitment, relayer: AccountId) {
    let buyer = order.allowed_buyer.unwrap_or(protocol_account(BUYER));
    let commitment = BuyerCommitment {
        order_id,
        buyer,
        price: order.price,
        currency: order.currency,
        expires_at: order.expiry,
        buyer_nonce: BuyerNonce::<Test>::get(buyer),
    };
    let signature = test_pair(BUYER)
        .sign(&buyer_signing_payload(
            genesis_hash(),
            order.operation,
            &commitment,
        ))
        .0;
    assert_ok!(MarketRuntime::fund_order(
        RuntimeOrigin::signed(relayer),
        order_id,
        commitment,
        signature,
    ));
}

fn create_transfer_authorization() -> [u8; 32] {
    assert_ok!(MarketRuntime::create_transfer_meta_authorization(
        RuntimeOrigin::signed(OWNER),
        ASSET,
        MARKET,
        TransferMetaAuthorizationTerms {
            price_policy: policy(META_PRICE),
            allowed_buyer: Some(BUYER),
            expiry: 20,
            nonce: 0,
        },
    ));
    sole_transfer_auth_id()
}

fn create_issue_authorization() -> [u8; 32] {
    assert_ok!(MarketRuntime::create_issue_right_authorization(
        RuntimeOrigin::signed(OWNER),
        ASSET,
        MARKET,
        issue_terms(20),
    ));
    sole_issue_auth_id()
}

fn initialize_resale_right(royalty_bps: u16) {
    assert_ok!(MarketRuntime::initialize_existing_right(
        RuntimeOrigin::signed(OWNER),
        ExistingRightRegistration {
            right_id: RIGHT,
            asset_id: ASSET,
            holder: RIGHT_SELLER,
            issuer: OWNER,
            right_type: RightType::Train,
            data_scope: DataScope::PartitionHash([0x55; 32]),
            validity: RightValidity {
                valid_from: 1,
                valid_until: Some(101),
                max_uses: Some(5),
            },
            transferable: true,
            allowed_transfer_markets: allowed_markets(),
            royalty_bps,
        },
    ));
}

#[test]
fn protocol_codec_and_hash_vector_matches_the_frozen_g3_reference() {
    assert_eq!(MarketOperation::TransferMeta.encode(), vec![0]);
    assert_eq!(MarketOperation::IssueRight.encode(), vec![1]);
    assert_eq!(MarketOperation::TransferRight.encode(), vec![2]);
    assert_eq!(ObjectType::MetaToken.encode(), vec![0]);
    assert_eq!(ObjectType::RightToken.encode(), vec![1]);
    assert_eq!(OrderState::Open.encode(), vec![0]);
    assert_eq!(OrderState::Funded.encode(), vec![1]);
    assert_eq!(OrderState::Settled.encode(), vec![2]);
    assert_eq!(OrderState::Cancelled.encode(), vec![3]);
    assert_eq!(OrderState::Expired.encode(), vec![4]);
    assert_eq!(OrderState::Refunded.encode(), vec![5]);
    assert_eq!(PaymentState::Unfunded.encode(), vec![0]);
    assert_eq!(PaymentState::Escrowed.encode(), vec![1]);
    assert_eq!(PaymentState::Released.encode(), vec![2]);
    assert_eq!(PaymentState::Refunded.encode(), vec![3]);
    assert_eq!(CurrencyId::Native.encode(), vec![0]);

    let seller = sp_core::sr25519::Pair::from_seed(&[1u8; 32]).public().0;
    let buyer = sp_core::sr25519::Pair::from_seed(&[2u8; 32]).public().0;
    let genesis = [0x11; 32];
    let order = OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id: [0x21; 32],
        operation: MarketOperation::TransferRight,
        object_type: ObjectType::RightToken,
        object_id: [0x31; 32],
        parent_asset_id: Some([0x41; 32]),
        authorization_id: [0x51; 32],
        seller,
        allowed_buyer: Some(buyer),
        price: RESALE_PRICE,
        currency: CurrencyId::Native,
        policy_hash: [0x61; 32],
        right_parameters_hash: Some([0x71; 32]),
        seller_state_version: 7,
        order_nonce: 0,
        expiry: 100,
    };
    let domain = domain_separator(genesis, MarketOperation::TransferRight, MessageType::Order);
    let id = order_id(genesis, &order);
    assert_eq!(order.encode().len(), 300);
    assert_eq!(
        domain,
        [
            56, 105, 2, 197, 237, 214, 9, 134, 17, 121, 209, 171, 108, 102, 73, 98, 45, 116, 222,
            59, 121, 147, 150, 86, 128, 154, 210, 11, 3, 209, 21, 183,
        ]
    );
    assert_eq!(
        id,
        [
            161, 130, 196, 148, 188, 125, 122, 135, 146, 167, 21, 9, 76, 42, 67, 206, 89, 72, 104,
            198, 98, 204, 140, 119, 142, 14, 246, 19, 109, 139, 128, 98,
        ]
    );
    assert_eq!(
        settlement_id(genesis, MarketOperation::TransferRight, id),
        [
            88, 102, 136, 77, 201, 218, 68, 130, 134, 158, 211, 71, 168, 95, 229, 45, 90, 53, 128,
            14, 234, 27, 214, 101, 172, 225, 107, 94, 185, 255, 254, 97,
        ]
    );
}

#[test]
fn objects_indexes_versions_and_structured_rights_are_authoritative() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(CAP_CREATE_RIGHT_TRANSFER_ORDER);
        assert_ok!(MarketRuntime::initialize_existing_right(
            RuntimeOrigin::signed(OWNER),
            ExistingRightRegistration {
                right_id: RIGHT,
                asset_id: ASSET,
                holder: RIGHT_SELLER,
                issuer: OWNER,
                right_type: RightType::Train,
                data_scope: DataScope::PartitionHash([0x55; 32]),
                validity: RightValidity {
                    valid_from: 1,
                    valid_until: Some(101),
                    max_uses: Some(5),
                },
                transferable: true,
                allowed_transfer_markets: allowed_markets(),
                royalty_bps: 250,
            }
        ));

        let meta = MetaTokens::<Test>::get(ASSET).unwrap();
        let right = RightTokens::<Test>::get(RIGHT).unwrap();
        assert_eq!((meta.owner, meta.state_version), (OWNER, 1));
        assert_eq!(
            (right.holder, right.issuer, right.state_version),
            (RIGHT_SELLER, OWNER, 1)
        );
        assert_eq!(right.remaining_uses, Some(5));
        assert!(MetaOwnerIndex::<Test>::contains_key(OWNER, ASSET));
        assert!(RightHolderIndex::<Test>::contains_key(RIGHT_SELLER, RIGHT));
        assert!(RightParentIndex::<Test>::contains_key(ASSET, RIGHT));
        assert_eq!(AssetRightIds::<Test>::get(ASSET).into_inner(), vec![RIGHT]);
        assert_eq!(ActiveRightCount::<Test>::get(ASSET), 1);
        assert_ok!(Pallet::<Test>::ensure_right_transfer_authorized(
            &RIGHT, &MARKET
        ));

        System::set_block_number(102);
        assert_ok!(MarketRuntime::expire_right(
            RuntimeOrigin::signed(OTHER),
            RIGHT
        ));
        let expired = RightTokens::<Test>::get(RIGHT).unwrap();
        assert_eq!(
            (expired.status, expired.state_version),
            (RightStatus::Expired, 2)
        );
        assert_eq!(ActiveRightCount::<Test>::get(ASSET), 0);
        assert_eq!(Pallet::<Test>::is_right_valid(&RIGHT), Ok(false));
    });
}

#[test]
fn market_capability_protocol_and_lifecycle_are_enforced() {
    new_test_ext().execute_with(|| {
        register_market(CAP_CREATE_META_ORDER | CAP_CREATE_RIGHT_ISSUE_ORDER);
        assert_ok!(Pallet::<Test>::ensure_market_operation(
            &MARKET,
            MarketOperation::TransferMeta
        ));
        assert_noop!(
            Pallet::<Test>::ensure_market_operation(&MARKET, MarketOperation::TransferRight),
            Error::<Test>::CapabilityDenied
        );
        assert_ok!(MarketRuntime::set_market_status(
            RuntimeOrigin::root(),
            MARKET,
            MarketStatus::Restricted
        ));
        assert_noop!(
            Pallet::<Test>::ensure_market_operation(&MARKET, MarketOperation::TransferMeta),
            Error::<Test>::MarketRestricted
        );
        assert_ok!(MarketRuntime::set_market_status(
            RuntimeOrigin::root(),
            MARKET,
            MarketStatus::Active
        ));
        assert_ok!(MarketRuntime::request_market_exit(
            RuntimeOrigin::signed(MARKET_OPERATOR),
            MARKET
        ));
        assert_ok!(MarketRuntime::deregister_market(
            RuntimeOrigin::root(),
            MARKET
        ));
        assert_eq!(
            Markets::<Test>::get(MARKET).unwrap().status,
            MarketStatus::Deregistered
        );
        assert_noop!(
            MarketRuntime::register_market(
                RuntimeOrigin::root(),
                [0x99; 32],
                MARKET_OPERATOR,
                2,
                ALL_CAPABILITIES,
                0,
                capacity(),
                1_000,
            ),
            Error::<Test>::UnsupportedProtocolVersion
        );
    });
}

#[test]
fn authorization_binds_owner_market_capability_policy_and_nonce() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(CAP_CREATE_RIGHT_ISSUE_ORDER | CAP_CREATE_RIGHT_TRANSFER_ORDER);
        assert_ok!(MarketRuntime::create_issue_right_authorization(
            RuntimeOrigin::signed(OWNER),
            ASSET,
            MARKET,
            issue_terms(20)
        ));
        let auth_id = sole_issue_auth_id();
        let auth = IssueRightAuthorizations::<Test>::get(auth_id).unwrap();
        assert_eq!(
            (auth.owner, auth.market_id, auth.operation),
            (OWNER, MARKET, MarketOperation::IssueRight)
        );
        assert_eq!((auth.reserved_count, auth.issued_count), (0, 0));
        assert_eq!(ActiveIssueAuthorizationCount::<Test>::get(ASSET), 1);
        assert_eq!(
            AuthorizationNonce::<Test>::get((OWNER, ASSET, MarketOperation::IssueRight)),
            1
        );
        assert_eq!(
            Markets::<Test>::get(MARKET).unwrap().active_authorizations,
            1
        );

        assert_noop!(
            MarketRuntime::create_issue_right_authorization(
                RuntimeOrigin::signed(OWNER),
                ASSET,
                MARKET,
                issue_terms(20)
            ),
            Error::<Test>::AuthorizationAlreadyActive
        );
        assert_eq!(IssueRightAuthorizations::<Test>::get(auth_id), Some(auth));
        assert_eq!(
            Pallet::<Test>::validate_price(&policy(3_000_000_000_000), 2_999_999_999_999, 0),
            Err(Error::<Test>::BelowPriceFloor.into())
        );
        assert_ok!(Pallet::<Test>::validate_price(
            &policy(3_000_000_000_000),
            3_000_000_000_000,
            0
        ));
    });
}

#[test]
fn invalid_authorization_is_failure_clean() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(CAP_CREATE_RIGHT_ISSUE_ORDER | CAP_CREATE_RIGHT_TRANSFER_ORDER);
        let mut invalid = issue_terms(20);
        invalid.price_policy.floor_ratio_bps = 0;
        assert_noop!(
            MarketRuntime::create_issue_right_authorization(
                RuntimeOrigin::signed(OWNER),
                ASSET,
                MARKET,
                invalid
            ),
            Error::<Test>::InvalidPricePolicy
        );
        assert_eq!(
            AuthorizationNonce::<Test>::get((OWNER, ASSET, MarketOperation::IssueRight)),
            0
        );
        assert_eq!(ActiveIssueAuthorizationCount::<Test>::get(ASSET), 0);
        assert_eq!(
            Markets::<Test>::get(MARKET).unwrap().active_authorizations,
            0
        );
    });
}

#[test]
fn asset_mutual_exclusion_blocks_conflicting_authorizations_and_rights() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(ALL_CAPABILITIES);
        assert_ok!(MarketRuntime::create_issue_right_authorization(
            RuntimeOrigin::signed(OWNER),
            ASSET,
            MARKET,
            issue_terms(20)
        ));
        let transfer_terms = TransferMetaAuthorizationTerms {
            price_policy: policy(10_000_000_000_000),
            allowed_buyer: Some(BUYER),
            expiry: 20,
            nonce: 0,
        };
        assert_noop!(
            MarketRuntime::create_transfer_meta_authorization(
                RuntimeOrigin::signed(OWNER),
                ASSET,
                MARKET,
                transfer_terms.clone()
            ),
            Error::<Test>::MutualExclusionConflict
        );
        let issue_auth = sole_issue_auth_id();
        assert_ok!(MarketRuntime::revoke_issue_right_authorization(
            RuntimeOrigin::signed(OWNER),
            issue_auth
        ));
        assert_ok!(MarketRuntime::initialize_existing_right(
            RuntimeOrigin::signed(OWNER),
            ExistingRightRegistration {
                right_id: RIGHT,
                asset_id: ASSET,
                holder: RIGHT_SELLER,
                issuer: OWNER,
                right_type: RightType::View,
                data_scope: DataScope::FullAsset,
                validity: RightValidity {
                    valid_from: 1,
                    valid_until: Some(10),
                    max_uses: None
                },
                transferable: true,
                allowed_transfer_markets: allowed_markets(),
                royalty_bps: 0,
            }
        ));
        assert_noop!(
            MarketRuntime::create_transfer_meta_authorization(
                RuntimeOrigin::signed(OWNER),
                ASSET,
                MARKET,
                transfer_terms.clone()
            ),
            Error::<Test>::MutualExclusionConflict
        );
        System::set_block_number(11);
        assert_ok!(MarketRuntime::create_transfer_meta_authorization(
            RuntimeOrigin::signed(OWNER),
            ASSET,
            MARKET,
            transfer_terms
        ));
        let transfer_auth = sole_transfer_auth_id();
        assert_eq!(
            TransferMetaAuthorizations::<Test>::get(transfer_auth)
                .unwrap()
                .status,
            AuthorizationStatus::Active
        );
        let mut conflicting = issue_terms(20);
        conflicting.nonce = 1;
        assert_noop!(
            MarketRuntime::create_issue_right_authorization(
                RuntimeOrigin::signed(OWNER),
                ASSET,
                MARKET,
                conflicting
            ),
            Error::<Test>::MutualExclusionConflict
        );
    });
}

#[test]
fn issued_and_reserved_quota_never_exceeds_limits() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(CAP_CREATE_RIGHT_ISSUE_ORDER | CAP_CREATE_RIGHT_TRANSFER_ORDER);
        assert_ok!(MarketRuntime::create_issue_right_authorization(
            RuntimeOrigin::signed(OWNER),
            ASSET,
            MARKET,
            issue_terms(20)
        ));
        let auth_id = sole_issue_auth_id();
        let order_1 = [0x91; 32];
        let order_2 = [0x92; 32];
        let order_3 = [0x93; 32];
        assert_ok!(Pallet::<Test>::reserve_issue_quota(&auth_id, order_1));
        assert_ok!(Pallet::<Test>::reserve_issue_quota(&auth_id, order_2));
        let before = IssueRightAuthorizations::<Test>::get(auth_id).unwrap();
        assert_eq!((before.reserved_count, before.reserved_use_units), (2, 6));
        assert_noop!(
            Pallet::<Test>::reserve_issue_quota(&auth_id, order_3),
            Error::<Test>::QuotaExceeded
        );
        assert_eq!(IssueRightAuthorizations::<Test>::get(auth_id), Some(before));
        assert!(!IssueQuotaReservations::<Test>::contains_key(order_3));

        assert_ok!(Pallet::<Test>::convert_issue_quota_to_issued(
            &auth_id, &order_1
        ));
        assert_ok!(Pallet::<Test>::release_issue_quota(&auth_id, &order_2));
        let final_auth = IssueRightAuthorizations::<Test>::get(auth_id).unwrap();
        assert_eq!(
            (
                final_auth.reserved_count,
                final_auth.reserved_use_units,
                final_auth.issued_count,
                final_auth.issued_use_units
            ),
            (0, 0, 1, 3)
        );
        assert_eq!(
            IssueQuotaReservations::<Test>::get(order_1).unwrap().status,
            IssueQuotaReservationStatus::Issued
        );
        assert_eq!(
            IssueQuotaReservations::<Test>::get(order_2).unwrap().status,
            IssueQuotaReservationStatus::Released
        );
        assert_noop!(
            Pallet::<Test>::convert_issue_quota_to_issued(&auth_id, &order_1),
            Error::<Test>::QuotaReservationStateInvalid
        );
    });
}

#[test]
fn expiry_closes_indexes_without_reusing_nonce() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(ALL_CAPABILITIES);
        assert_ok!(MarketRuntime::create_issue_right_authorization(
            RuntimeOrigin::signed(OWNER),
            ASSET,
            MARKET,
            issue_terms(5)
        ));
        let auth_id = sole_issue_auth_id();
        System::set_block_number(6);
        assert_ok!(MarketRuntime::expire_issue_right_authorization(
            RuntimeOrigin::signed(OTHER),
            auth_id
        ));
        assert_eq!(
            IssueRightAuthorizations::<Test>::get(auth_id)
                .unwrap()
                .status,
            AuthorizationStatus::Expired
        );
        assert_eq!(ActiveIssueAuthorizationCount::<Test>::get(ASSET), 0);
        assert_eq!(
            AuthorizationNonce::<Test>::get((OWNER, ASSET, MarketOperation::IssueRight)),
            1
        );
        let mut next = issue_terms(20);
        next.nonce = 1;
        assert_ok!(MarketRuntime::create_issue_right_authorization(
            RuntimeOrigin::signed(OWNER),
            ASSET,
            MARKET,
            next
        ));
    });
}

#[test]
fn transfer_meta_escrows_recorded_buyer_and_writes_one_frozen_record() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(ALL_CAPABILITIES);
        let auth_id = create_transfer_authorization();
        let (order_id, order) = create_signed_order(
            MarketOperation::TransferMeta,
            ASSET,
            None,
            auth_id,
            OWNER,
            Some(BUYER),
            META_PRICE,
            policy(META_PRICE),
            None,
            0,
            PricePolicyWitness::Authorization,
        );
        assert_eq!(Balances::free_balance(BUYER), INITIAL_BALANCE);
        assert_eq!(
            RuntimeOrders::<Test>::get(order_id).unwrap().state,
            OrderState::Open
        );

        fund_signed_order(order_id, &order, MARKET_OPERATOR);
        let funded = RuntimeOrders::<Test>::get(order_id).unwrap();
        let escrow_id = funded.escrow_id.unwrap();
        assert_eq!(Balances::free_balance(BUYER), INITIAL_BALANCE - META_PRICE);
        assert_eq!(Balances::reserved_balance(BUYER), META_PRICE);
        assert_eq!(Balances::free_balance(MARKET_OPERATOR), INITIAL_BALANCE);
        assert_eq!(
            RuntimeEscrows::<Test>::get(escrow_id).unwrap().state,
            PaymentState::Escrowed
        );

        assert_ok!(MarketRuntime::settle(
            RuntimeOrigin::signed(OTHER),
            order_id
        ));
        assert_eq!(MetaTokens::<Test>::get(ASSET).unwrap().owner, BUYER);
        assert!(MetaOwnerIndex::<Test>::contains_key(BUYER, ASSET));
        assert!(!MetaOwnerIndex::<Test>::contains_key(OWNER, ASSET));
        assert_eq!(Balances::reserved_balance(BUYER), 0);
        assert_eq!(Balances::free_balance(OWNER), INITIAL_BALANCE + META_PRICE);
        assert_eq!(
            RuntimeOrders::<Test>::get(order_id).unwrap().state,
            OrderState::Settled
        );
        assert_eq!(
            RuntimeEscrows::<Test>::get(escrow_id).unwrap().state,
            PaymentState::Released
        );
        assert_eq!(
            TransferMetaAuthorizations::<Test>::get(auth_id)
                .unwrap()
                .status,
            AuthorizationStatus::Consumed
        );
        assert!(!ActiveMetaOrder::<Test>::contains_key(ASSET));

        let record = SettlementRecords::<Test>::get(order_id).unwrap();
        assert_eq!(
            record.gross_price,
            record.seller_amount
                + record.royalty_amount
                + record.market_fee_amount
                + record.safety_fee_amount
        );
        assert_eq!(record.pre_state_version, 1);
        assert_eq!(record.post_state_version, 2);
        assert_eq!(record.right_id, None);
        assert_eq!(
            record.encode(),
            (
                (
                    record.settlement_id,
                    record.order_id,
                    record.operation,
                    record.asset_id,
                    record.right_id,
                    record.authorization_id,
                    record.market_id,
                    record.policy_hash,
                    record.order_commitment_hash,
                    record.seller,
                    record.buyer,
                    record.currency,
                ),
                (
                    record.gross_price,
                    record.seller_amount,
                    record.royalty_recipient,
                    record.royalty_amount,
                    record.market_fee_recipient,
                    record.market_fee_amount,
                    record.safety_fee_amount,
                    record.pre_state_version,
                    record.post_state_version,
                    record.pre_state_commitment,
                    record.block_number,
                    record.settlement_protocol_version,
                ),
            )
                .encode()
        );
        assert_noop!(
            MarketRuntime::settle(RuntimeOrigin::signed(OTHER), order_id),
            Error::<Test>::AlreadySettled
        );
        assert_eq!(SettlementRecords::<Test>::iter().count(), 1);
        assert_eq!(
            System::events()
                .iter()
                .filter(|record| matches!(
                    record.event,
                    RuntimeEvent::MarketRuntime(crate::Event::SettlementFinalized {
                        order_id: finalized,
                        ..
                    }) if finalized == order_id
                ))
                .count(),
            1
        );
    });
}

#[test]
fn issue_right_settlement_atomically_converts_quota_and_mints_to_buyer() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(ALL_CAPABILITIES);
        let auth_id = create_issue_authorization();
        let auth = IssueRightAuthorizations::<Test>::get(auth_id).unwrap();
        let parameters = right_parameters_hash(
            auth.right_type,
            auth.data_scope,
            auth.validity_template,
            auth.transferable,
            &auth.allowed_transfer_markets,
            auth.royalty_bps,
        );
        let (order_id, order) = create_signed_order(
            MarketOperation::IssueRight,
            ASSET,
            None,
            auth_id,
            OWNER,
            Some(BUYER),
            ISSUE_PRICE,
            policy(ISSUE_PRICE),
            Some(parameters),
            0,
            PricePolicyWitness::Authorization,
        );
        fund_signed_order(order_id, &order, BUYER);
        assert_eq!(
            IssueQuotaReservations::<Test>::get(order_id)
                .unwrap()
                .status,
            IssueQuotaReservationStatus::Reserved
        );
        assert_eq!(
            (
                IssueRightAuthorizations::<Test>::get(auth_id)
                    .unwrap()
                    .reserved_count,
                ActiveIssueOrderCount::<Test>::get(ASSET),
            ),
            (1, 1)
        );

        assert_ok!(MarketRuntime::revoke_issue_right_authorization(
            RuntimeOrigin::signed(OWNER),
            auth_id,
        ));
        assert_noop!(
            MarketRuntime::create_transfer_meta_authorization(
                RuntimeOrigin::signed(OWNER),
                ASSET,
                MARKET,
                TransferMetaAuthorizationTerms {
                    price_policy: policy(META_PRICE),
                    allowed_buyer: Some(BUYER),
                    expiry: 20,
                    nonce: 0,
                },
            ),
            Error::<Test>::MutualExclusionConflict
        );

        assert_ok!(MarketRuntime::settle(
            RuntimeOrigin::signed(OTHER),
            order_id
        ));
        let issued_id = right_id(genesis_hash(), MarketOperation::IssueRight, order_id);
        let right = RightTokens::<Test>::get(issued_id).unwrap();
        assert_eq!(
            (right.asset_id, right.holder, right.issuer),
            (ASSET, BUYER, OWNER)
        );
        assert!(RightHolderIndex::<Test>::contains_key(BUYER, issued_id));
        assert!(RightParentIndex::<Test>::contains_key(ASSET, issued_id));
        let final_auth = IssueRightAuthorizations::<Test>::get(auth_id).unwrap();
        assert_eq!(
            (
                final_auth.reserved_count,
                final_auth.reserved_use_units,
                final_auth.issued_count,
                final_auth.issued_use_units,
            ),
            (0, 0, 1, 3)
        );
        assert_eq!(
            IssueQuotaReservations::<Test>::get(order_id)
                .unwrap()
                .status,
            IssueQuotaReservationStatus::Issued
        );
        assert_eq!(
            SettlementRecords::<Test>::get(order_id).unwrap().right_id,
            Some(issued_id)
        );
        assert_eq!(ActiveIssueOrderCount::<Test>::get(ASSET), 0);
        assert_eq!(Balances::free_balance(OWNER), INITIAL_BALANCE + ISSUE_PRICE);
    });
}

#[test]
fn transfer_right_releases_royalty_and_updates_holder_index() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(ALL_CAPABILITIES);
        initialize_resale_right(250);
        let right = RightTokens::<Test>::get(RIGHT).unwrap();
        let parameters = right_parameters_hash(
            right.right_type,
            right.data_scope,
            right.validity,
            right.transferable,
            &right.allowed_transfer_markets,
            right.royalty_bps,
        );
        let resale_policy = policy(RESALE_PRICE);
        let (order_id, order) = create_signed_order(
            MarketOperation::TransferRight,
            RIGHT,
            Some(ASSET),
            [0x81; 32],
            RIGHT_SELLER,
            Some(BUYER),
            RESALE_PRICE,
            resale_policy,
            Some(parameters),
            0,
            PricePolicyWitness::SellerSnapshot(resale_policy),
        );
        fund_signed_order(order_id, &order, BUYER);
        assert_ok!(MarketRuntime::settle(
            RuntimeOrigin::signed(OTHER),
            order_id
        ));

        let right = RightTokens::<Test>::get(RIGHT).unwrap();
        assert_eq!((right.holder, right.state_version), (BUYER, 2));
        assert!(!RightHolderIndex::<Test>::contains_key(RIGHT_SELLER, RIGHT));
        assert!(RightHolderIndex::<Test>::contains_key(BUYER, RIGHT));
        assert_eq!(
            Balances::free_balance(BUYER),
            INITIAL_BALANCE - RESALE_PRICE
        );
        assert_eq!(
            Balances::free_balance(RIGHT_SELLER),
            INITIAL_BALANCE + 3_900_000_000_000
        );
        assert_eq!(
            Balances::free_balance(OWNER),
            INITIAL_BALANCE + 100_000_000_000
        );
        let record = SettlementRecords::<Test>::get(order_id).unwrap();
        assert_eq!(record.royalty_amount, 100_000_000_000);
        assert_eq!(record.royalty_recipient, Some(protocol_account(OWNER)));
        assert!(!ActiveRightOrder::<Test>::contains_key(RIGHT));
    });
}

#[test]
fn funding_failure_is_clean_and_refund_restores_only_recorded_buyer() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(ALL_CAPABILITIES);
        let auth_id = create_issue_authorization();
        let auth = IssueRightAuthorizations::<Test>::get(auth_id).unwrap();
        let parameters = right_parameters_hash(
            auth.right_type,
            auth.data_scope,
            auth.validity_template,
            auth.transferable,
            &auth.allowed_transfer_markets,
            auth.royalty_bps,
        );
        let (order_id, order) = create_signed_order(
            MarketOperation::IssueRight,
            ASSET,
            None,
            auth_id,
            OWNER,
            Some(BUYER),
            ISSUE_PRICE,
            policy(ISSUE_PRICE),
            Some(parameters),
            0,
            PricePolicyWitness::Authorization,
        );
        assert_ok!(<Balances as ReservableCurrency<AccountId>>::reserve(
            &BUYER,
            INITIAL_BALANCE - 1_000,
        ));
        let commitment = BuyerCommitment {
            order_id,
            buyer: protocol_account(BUYER),
            price: ISSUE_PRICE,
            currency: CurrencyId::Native,
            expires_at: order.expiry,
            buyer_nonce: 0,
        };
        let signature = test_pair(BUYER)
            .sign(&buyer_signing_payload(
                genesis_hash(),
                order.operation,
                &commitment,
            ))
            .0;
        assert_noop!(
            MarketRuntime::fund_order(
                RuntimeOrigin::signed(MARKET_OPERATOR),
                order_id,
                commitment,
                signature,
            ),
            Error::<Test>::InsufficientBalance
        );
        assert_eq!(
            RuntimeOrders::<Test>::get(order_id).unwrap().state,
            OrderState::Open
        );
        assert_eq!(BuyerNonce::<Test>::get(protocol_account(BUYER)), 0);
        assert!(!IssueQuotaReservations::<Test>::contains_key(order_id));
        assert_eq!(Markets::<Test>::get(MARKET).unwrap().funded_orders, 0);

        assert_eq!(Balances::unreserve(&BUYER, INITIAL_BALANCE - 1_000), 0);
        fund_signed_order(order_id, &order, MARKET_OPERATOR);
        let other_before = Balances::free_balance(OTHER);
        System::set_block_number(12);
        assert_ok!(MarketRuntime::refund_funded(
            RuntimeOrigin::signed(OTHER),
            order_id
        ));
        assert_eq!(
            RuntimeOrders::<Test>::get(order_id).unwrap().state,
            OrderState::Refunded
        );
        assert_eq!(Balances::free_balance(BUYER), INITIAL_BALANCE);
        assert_eq!(Balances::reserved_balance(BUYER), 0);
        assert_eq!(Balances::free_balance(OTHER), other_before);
        assert_eq!(
            IssueQuotaReservations::<Test>::get(order_id)
                .unwrap()
                .status,
            IssueQuotaReservationStatus::Released
        );
        assert!(!SettlementRecords::<Test>::contains_key(order_id));
        assert_eq!(MetaTokens::<Test>::get(ASSET).unwrap().owner, OWNER);
        assert_noop!(
            MarketRuntime::refund_funded(RuntimeOrigin::signed(OTHER), order_id),
            Error::<Test>::TerminalOrder
        );
    });
}

#[test]
fn cancellation_expiry_and_late_settlement_failure_keep_terminal_states_stable() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(ALL_CAPABILITIES);
        let auth_id = create_transfer_authorization();
        let (cancelled_id, _) = create_signed_order(
            MarketOperation::TransferMeta,
            ASSET,
            None,
            auth_id,
            OWNER,
            Some(BUYER),
            META_PRICE,
            policy(META_PRICE),
            None,
            0,
            PricePolicyWitness::Authorization,
        );
        assert_ok!(MarketRuntime::cancel_open(
            RuntimeOrigin::signed(OWNER),
            cancelled_id
        ));
        assert_eq!(
            RuntimeOrders::<Test>::get(cancelled_id).unwrap().state,
            OrderState::Cancelled
        );
        assert!(!SettlementRecords::<Test>::contains_key(cancelled_id));

        let (expired_id, _) = create_signed_order(
            MarketOperation::TransferMeta,
            ASSET,
            None,
            auth_id,
            OWNER,
            Some(BUYER),
            META_PRICE,
            policy(META_PRICE),
            None,
            1,
            PricePolicyWitness::Authorization,
        );
        System::set_block_number(21);
        assert_ok!(MarketRuntime::expire_open(
            RuntimeOrigin::signed(OTHER),
            expired_id
        ));
        assert_eq!(
            RuntimeOrders::<Test>::get(expired_id).unwrap().state,
            OrderState::Expired
        );
        assert_noop!(
            MarketRuntime::settle(RuntimeOrigin::signed(OTHER), expired_id),
            Error::<Test>::TerminalOrder
        );
        assert!(!SettlementRecords::<Test>::contains_key(expired_id));
    });
}

#[test]
fn settlement_failure_after_object_write_rolls_back_all_success_state() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(ALL_CAPABILITIES);
        let auth_id = create_transfer_authorization();
        let (order_id, order) = create_signed_order(
            MarketOperation::TransferMeta,
            ASSET,
            None,
            auth_id,
            OWNER,
            Some(BUYER),
            META_PRICE,
            policy(META_PRICE),
            None,
            0,
            PricePolicyWitness::Authorization,
        );
        fund_signed_order(order_id, &order, BUYER);
        let escrow_id = escrow_id(genesis_hash(), MarketOperation::TransferMeta, order_id);
        RuntimeEscrows::<Test>::mutate(escrow_id, |escrow| {
            escrow.as_mut().unwrap().amounts.seller_net -= 1;
        });
        let meta_before = MetaTokens::<Test>::get(ASSET).unwrap();
        let auth_before = TransferMetaAuthorizations::<Test>::get(auth_id).unwrap();
        let order_before = RuntimeOrders::<Test>::get(order_id).unwrap();
        let escrow_before = RuntimeEscrows::<Test>::get(escrow_id).unwrap();
        assert_noop!(
            MarketRuntime::settle(RuntimeOrigin::signed(OTHER), order_id),
            Error::<Test>::EscrowMismatch
        );
        assert_eq!(MetaTokens::<Test>::get(ASSET), Some(meta_before));
        assert_eq!(
            TransferMetaAuthorizations::<Test>::get(auth_id),
            Some(auth_before)
        );
        assert_eq!(RuntimeOrders::<Test>::get(order_id), Some(order_before));
        assert_eq!(RuntimeEscrows::<Test>::get(escrow_id), Some(escrow_before));
        assert!(!SettlementRecords::<Test>::contains_key(order_id));
        assert_eq!(Balances::reserved_balance(BUYER), META_PRICE);
    });
}

#[test]
fn experiment_fixture_apis_require_root_and_preserve_authoritative_indexes() {
    new_test_ext().execute_with(|| {
        register_meta();
        register_market(ALL_CAPABILITIES);
        let input = ExistingRightRegistration {
            right_id: RIGHT,
            asset_id: ASSET,
            holder: RIGHT_SELLER,
            issuer: OWNER,
            right_type: RightType::Compute,
            data_scope: DataScope::FullAsset,
            validity: RightValidity {
                valid_from: 1,
                valid_until: Some(101),
                max_uses: Some(5),
            },
            transferable: true,
            allowed_transfer_markets: allowed_markets(),
            royalty_bps: 0,
        };

        assert_noop!(
            MarketRuntime::initialize_right_for_experiment(
                RuntimeOrigin::signed(OWNER),
                input.clone(),
                RightStatus::Active,
                Some(2),
            ),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_ok!(MarketRuntime::initialize_right_for_experiment(
            RuntimeOrigin::root(),
            input.clone(),
            RightStatus::Active,
            Some(2),
        ));
        let right = RightTokens::<Test>::get(RIGHT).unwrap();
        assert_eq!(
            (right.holder, right.remaining_uses),
            (RIGHT_SELLER, Some(2))
        );
        assert_eq!(AssetRightIds::<Test>::get(ASSET).into_inner(), vec![RIGHT]);
        assert_eq!(ActiveRightCount::<Test>::get(ASSET), 1);
        assert!(RightHolderIndex::<Test>::contains_key(RIGHT_SELLER, RIGHT));
        assert!(RightParentIndex::<Test>::contains_key(ASSET, RIGHT));

        let meta_before = MetaTokens::<Test>::get(ASSET).unwrap();
        let right_before = RightTokens::<Test>::get(RIGHT).unwrap();
        assert_noop!(
            MarketRuntime::configure_meta_status_for_experiment(
                RuntimeOrigin::signed(OWNER),
                ASSET,
                AssetStatus::Retiring,
            ),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_noop!(
            MarketRuntime::configure_right_status_for_experiment(
                RuntimeOrigin::signed(OWNER),
                RIGHT,
                RightStatus::Expired,
            ),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_ok!(MarketRuntime::configure_meta_status_for_experiment(
            RuntimeOrigin::root(),
            ASSET,
            AssetStatus::Retiring,
        ));
        assert_ok!(MarketRuntime::configure_right_status_for_experiment(
            RuntimeOrigin::root(),
            RIGHT,
            RightStatus::Expired,
        ));
        let mut expected_meta = meta_before;
        expected_meta.status = AssetStatus::Retiring;
        let mut expected_right = right_before;
        expected_right.status = RightStatus::Expired;
        assert_eq!(MetaTokens::<Test>::get(ASSET), Some(expected_meta));
        assert_eq!(RightTokens::<Test>::get(RIGHT), Some(expected_right));
        assert!(MetaOwnerIndex::<Test>::contains_key(OWNER, ASSET));
        assert!(RightHolderIndex::<Test>::contains_key(RIGHT_SELLER, RIGHT));
        assert!(RightParentIndex::<Test>::contains_key(ASSET, RIGHT));
        assert_eq!(ActiveRightCount::<Test>::get(ASSET), 1);
        assert_noop!(
            MarketRuntime::configure_meta_status_for_experiment(
                RuntimeOrigin::root(),
                [0xef; 32],
                AssetStatus::Retiring,
            ),
            Error::<Test>::AssetNotFound
        );
        assert_noop!(
            MarketRuntime::configure_right_status_for_experiment(
                RuntimeOrigin::root(),
                [0xef; 32],
                RightStatus::Expired,
            ),
            Error::<Test>::RightNotFound
        );
        assert_noop!(
            MarketRuntime::initialize_right_for_experiment(
                RuntimeOrigin::root(),
                input,
                RightStatus::Revoked,
                Some(2),
            ),
            Error::<Test>::RightAlreadyExists
        );

        assert_noop!(
            MarketRuntime::configure_market_for_experiment(
                RuntimeOrigin::signed(OWNER),
                MARKET,
                true,
                Some(CAP_CREATE_META_ORDER),
                Some(0),
            ),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_ok!(MarketRuntime::configure_market_for_experiment(
            RuntimeOrigin::root(),
            MARKET,
            true,
            Some(CAP_CREATE_META_ORDER),
            Some(0),
        ));
        let market = Markets::<Test>::get(MARKET).unwrap();
        assert_eq!(market.capabilities, CAP_CREATE_META_ORDER);
        assert_eq!(market.capacity_tier.max_funded_orders, 0);
        assert_ok!(MarketRuntime::configure_market_for_experiment(
            RuntimeOrigin::root(),
            MARKET,
            false,
            None,
            None,
        ));
        assert!(!Markets::<Test>::contains_key(MARKET));
    });
}

#[test]
fn x1_policy_decision_is_bound_to_order_admission_and_consumed() {
    new_test_ext().execute_with(|| {
        register_meta();
        assert_ok!(MarketRuntime::r2_x1_evaluate_policy(
            RuntimeOrigin::signed(OTHER),
            META_PRICE,
            vec![].try_into().unwrap(),
        ));
        let decision = R2X1LastPolicyDecision::<Test>::get().unwrap();
        assert_eq!(decision.gross_price, META_PRICE);
        assert_eq!(decision.market_fee_bps, 100);
        assert_eq!(decision.market_fee_amount, META_PRICE * 100 / 10_000);
        assert_ok!(MarketRuntime::register_market(
            RuntimeOrigin::root(),
            MARKET,
            MARKET_OPERATOR,
            SETTLEMENT_PROTOCOL_VERSION,
            CAP_CREATE_META_ORDER,
            decision.market_fee_bps,
            capacity(),
            1_000,
        ));
        let mut order_policy = policy(decision.gross_price);
        order_policy.signed_market_fee_cap_bps = decision.market_fee_bps;
        assert_ok!(MarketRuntime::create_transfer_meta_authorization(
            RuntimeOrigin::signed(OWNER),
            ASSET,
            MARKET,
            TransferMetaAuthorizationTerms {
                price_policy: order_policy,
                allowed_buyer: Some(BUYER),
                expiry: 20,
                nonce: 0,
            },
        ));
        let authorization_id = sole_transfer_auth_id();
        let signed_order = |price| {
            let order = OrderCommitment {
                settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
                market_id: MARKET,
                operation: MarketOperation::TransferMeta,
                object_type: ObjectType::MetaToken,
                object_id: ASSET,
                parent_asset_id: None,
                authorization_id,
                seller: protocol_account(OWNER),
                allowed_buyer: Some(protocol_account(BUYER)),
                price,
                currency: CurrencyId::Native,
                policy_hash: price_policy_hash(&order_policy),
                right_parameters_hash: None,
                seller_state_version: 1,
                order_nonce: 0,
                expiry: 20,
            };
            let id = order_id(genesis_hash(), &order);
            let commitment = SellerCommitment {
                order_id: id,
                object_id: ASSET,
                operation: order.operation,
                seller: order.seller,
                state_version: 1,
                price_policy_hash: order.policy_hash,
                right_parameters_hash: None,
                expires_at: order.expiry,
                seller_nonce: 0,
            };
            let signature = test_pair(OWNER)
                .sign(&seller_signing_payload(
                    genesis_hash(),
                    order.operation,
                    &commitment,
                ))
                .0;
            (id, order, commitment, signature)
        };

        let (rejected_id, rejected_order, rejected_commitment, rejected_signature) =
            signed_order(decision.gross_price + 1);
        assert_noop!(
            MarketRuntime::create_order(
                RuntimeOrigin::signed(OTHER),
                rejected_order,
                PricePolicyWitness::Authorization,
                rejected_commitment,
                rejected_signature,
            ),
            Error::<Test>::InvalidPricePolicy
        );
        assert_eq!(R2X1LastPolicyDecision::<Test>::get(), Some(decision));
        assert!(!RuntimeOrders::<Test>::contains_key(rejected_id));
        assert_eq!(
            SellerNonce::<Test>::get((protocol_account(OWNER), ASSET)),
            0
        );
        assert_eq!(Markets::<Test>::get(MARKET).unwrap().active_orders, 0);

        let (order_id, order, commitment, signature) = signed_order(decision.gross_price);
        assert_ok!(MarketRuntime::create_order(
            RuntimeOrigin::signed(OTHER),
            order,
            PricePolicyWitness::Authorization,
            commitment,
            signature,
        ));
        assert_eq!(R2X1LastPolicyDecision::<Test>::get(), None);
        assert!(RuntimeOrders::<Test>::contains_key(order_id));
        assert_eq!(
            SellerNonce::<Test>::get((protocol_account(OWNER), ASSET)),
            1
        );
        assert_eq!(Markets::<Test>::get(MARKET).unwrap().active_orders, 1);
    });
}

include!("s38_scenarios.rs");
