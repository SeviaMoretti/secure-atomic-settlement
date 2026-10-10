use super::*;
use crate::protocol::CommonErrorCode;
use sp_core::storage::StateVersion;
use sp_runtime::DispatchError;
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
    execute: fn(),
}

#[derive(Clone, Copy)]
struct QueryContext {
    asset_id: [u8; 32],
    right_id: Option<[u8; 32]>,
    order_id: [u8; 32],
    auth_id: [u8; 32],
    market_id: [u8; 32],
    seller: AccountId,
    seller_object_id: [u8; 32],
    buyer: [u8; 32],
}

#[derive(Debug, PartialEq, Eq)]
struct QueryAllSnapshot {
    storage_root: Vec<u8>,
    meta: Option<Vec<u8>>,
    right: Option<Vec<u8>>,
    order: Option<Vec<u8>>,
    order_policy: Option<Vec<u8>>,
    issue_authorization: Option<Vec<u8>>,
    transfer_authorization: Option<Vec<u8>>,
    quota_reservation: Option<Vec<u8>>,
    payment: Option<Vec<u8>>,
    balances: Vec<(AccountId, Balance, Balance)>,
    settlement: Option<Vec<u8>>,
    settlement_count: usize,
    success_events: Vec<([u8; 32], [u8; 32], [u8; 32])>,
    all_events: Vec<u8>,
    market: Option<Vec<u8>>,
    seller_nonce: u64,
    buyer_nonce: u64,
    active_meta_order: Option<[u8; 32]>,
    active_right_order: Option<[u8; 32]>,
    market_active_orders: Option<u32>,
    market_funding: Option<(u32, u128)>,
}

fn query_all(context: QueryContext) -> QueryAllSnapshot {
    let right = context
        .right_id
        .and_then(|right_id| DataAssets::query_right_token(&context.asset_id, &right_id));
    let market = DataAssets::query_market(&context.market_id).ok();
    QueryAllSnapshot {
        storage_root: sp_io::storage::root(StateVersion::V1),
        meta: DataAssets::query_meta_token(&context.asset_id).map(|value| value.encode()),
        right: right.map(|value| value.encode()),
        order: DataAssets::query_order(&context.order_id).map(|value| value.encode()),
        order_policy: DataAssets::query_order_price_policy(&context.order_id)
            .map(|value| value.encode()),
        issue_authorization: DataAssets::query_issue_right_authorization(&context.auth_id)
            .map(|value| value.encode()),
        transfer_authorization: DataAssets::query_transfer_meta_authorization(&context.auth_id)
            .map(|value| value.encode()),
        quota_reservation: DataAssets::issue_quota_reservations(context.order_id)
            .map(|value| value.encode()),
        payment: DataAssets::query_payment(&context.order_id).map(|value| value.encode()),
        balances: (1..=4)
            .map(|account| {
                (
                    account,
                    Balances::free_balance(account),
                    Balances::reserved_balance(account),
                )
            })
            .collect(),
        settlement: DataAssets::query_settlement(&context.order_id).map(|value| value.encode()),
        settlement_count: crate::pallet::SettlementRecords::<Test>::iter().count(),
        success_events: settlement_finalized_events(),
        all_events: System::events().encode(),
        market: market.map(|value| value.encode()),
        seller_nonce: DataAssets::seller_nonce(
            test_protocol_account(context.seller),
            context.seller_object_id,
        ),
        buyer_nonce: DataAssets::buyer_nonce(context.buyer),
        active_meta_order: DataAssets::active_meta_order(context.asset_id),
        active_right_order: context.right_id.and_then(DataAssets::active_right_order),
        market_active_orders: ACTIVE_ORDERS
            .with(|orders| orders.borrow().get(&context.market_id).map(|value| value.0)),
        market_funding: MARKET_PAYMENTS.with(|payments| {
            payments
                .borrow()
                .get(&context.market_id)
                .map(|value| (value.funded_orders, value.exposure))
        }),
    }
}

fn assert_m_reject_clean(
    context: QueryContext,
    expected: pallet_dataassets::Error<Test>,
    operation: impl FnOnce() -> DispatchResult,
) {
    let before = query_all(context);
    let expected: DispatchError = expected.into();
    assert_eq!(operation(), Err(expected));
    assert_eq!(query_all(context), before);
}

fn assert_m_no_settlement(order_id: [u8; 32]) {
    assert!(DataAssets::query_settlement(&order_id).is_none());
    assert!(settlement_finalized_events().is_empty());
}

fn assert_m_once(order_id: [u8; 32]) {
    assert_eq!(crate::pallet::SettlementRecords::<Test>::iter().count(), 1);
    assert!(DataAssets::query_settlement(&order_id).is_some());
    assert_eq!(settlement_finalized_events().len(), 1);
}

fn meta_context(
    asset_id: [u8; 32],
    market_id: [u8; 32],
    auth_id: [u8; 32],
    order_id: [u8; 32],
) -> QueryContext {
    QueryContext {
        asset_id,
        right_id: None,
        order_id,
        auth_id,
        market_id,
        seller: 1,
        seller_object_id: asset_id,
        buyer: test_protocol_account(2),
    }
}

fn resale_context(
    asset_id: [u8; 32],
    market_id: [u8; 32],
    right_id: [u8; 32],
    order_id: [u8; 32],
) -> QueryContext {
    QueryContext {
        asset_id,
        right_id: Some(right_id),
        order_id,
        auth_id: right_id,
        market_id,
        seller: 2,
        seller_object_id: right_id,
        buyer: test_protocol_account(3),
    }
}

fn run_neg_proto() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, mut order) = create_transfer_meta_fixture(100);
        order.settlement_protocol_version = SETTLEMENT_PROTOCOL_VERSION + 1;
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        let context = meta_context(asset_id, market_id, auth_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::UnsupportedProtocolVersion,
            || create_runtime_order(RuntimeOrigin::signed(3), order, seller, signature),
        );
    });
}

fn run_neg_signature() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order) = create_transfer_meta_fixture(100);
        let (seller, _, order_id) = signed_seller_input(&order, 1);
        let context = meta_context(asset_id, market_id, auth_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::InvalidSignature,
            || create_runtime_order(RuntimeOrigin::signed(3), order, seller, [0; 64]),
        );
    });
}

fn run_neg_market() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order) = create_transfer_meta_fixture(100);
        MARKETS.with(|markets| {
            markets.borrow_mut().remove(&market_id);
        });
        ACTIVE_ORDERS.with(|orders| {
            orders.borrow_mut().remove(&market_id);
        });
        MARKET_PAYMENTS.with(|payments| {
            payments.borrow_mut().remove(&market_id);
        });
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        let context = meta_context(asset_id, market_id, auth_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::MarketNotRegistered,
            || create_runtime_order(RuntimeOrigin::signed(3), order, seller, signature),
        );
    });
}

fn run_neg_capability() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order) = create_issue_right_fixture(100);
        MARKETS.with(|markets| {
            markets.borrow_mut().get_mut(&market_id).unwrap().1 = 0;
        });
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        let context = QueryContext {
            asset_id,
            right_id: None,
            order_id,
            auth_id,
            market_id,
            seller: 1,
            seller_object_id: asset_id,
            buyer: test_protocol_account(2),
        };
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::CapabilityDenied,
            || create_runtime_order(RuntimeOrigin::signed(3), order, seller, signature),
        );
    });
}

fn run_neg_resale_authorization() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, order_id, order) =
            create_transfer_right_fixture(100, 0);
        let mut right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        right.allowed_transfer_markets =
            BoundedVec::try_from(vec![[8; 32]]).expect("bounded denied market");
        assert_ok!(DataAssets::test_update_certificate(&asset_id, &right));
        let (seller, signature, _) = signed_seller_input(&order, 2);
        let context = resale_context(asset_id, market_id, right_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::AuthorizationMissing,
            || create_runtime_order(RuntimeOrigin::signed(4), order, seller, signature),
        );
    });
}

fn run_neg_owner() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, mut order) = create_transfer_meta_fixture(100);
        order.seller = test_protocol_account(2);
        let (seller, signature, order_id) = signed_seller_input(&order, 2);
        let context = meta_context(asset_id, market_id, auth_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::OwnershipMismatch,
            || create_runtime_order(RuntimeOrigin::signed(3), order, seller, signature),
        );
    });
}

fn run_neg_holder() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, _, mut order) = create_transfer_right_fixture(100, 0);
        order.seller = test_protocol_account(1);
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        let context = resale_context(asset_id, market_id, right_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::OwnershipMismatch,
            || create_runtime_order(RuntimeOrigin::signed(4), order, seller, signature),
        );
    });
}

fn run_neg_parent() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, _, mut order) = create_transfer_right_fixture(100, 0);
        order.parent_asset_id = Some([0x99; 32]);
        let (seller, signature, order_id) = signed_seller_input(&order, 2);
        let context = resale_context(asset_id, market_id, right_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::ParentMismatch,
            || create_runtime_order(RuntimeOrigin::signed(4), order, seller, signature),
        );
    });
}

#[derive(Clone, Copy)]
enum InvalidRight {
    Missing,
    Expired,
    Exhausted,
    Revoked,
}

fn run_neg_right(kind: InvalidRight) {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, _, mut order) = create_transfer_right_fixture(100, 0);
        match kind {
            InvalidRight::Missing => {
                order.object_id = [0xa1; 32];
                order.authorization_id = order.object_id;
                order.right_parameters_hash = Some([0xa2; 32]);
            }
            InvalidRight::Expired => {
                let mut right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
                right.status = CertificateStatus::Expired;
                assert_ok!(DataAssets::test_update_certificate(&asset_id, &right));
            }
            InvalidRight::Exhausted => {
                let mut right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
                right.status = CertificateStatus::Exhausted;
                right.remaining_uses = Some(0);
                assert_ok!(DataAssets::test_update_certificate(&asset_id, &right));
            }
            InvalidRight::Revoked => {
                let mut right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
                right.status = CertificateStatus::Revoked;
                assert_ok!(DataAssets::test_update_certificate(&asset_id, &right));
            }
        }
        let queried_right_id = order.object_id;
        let (seller, signature, order_id) = signed_seller_input(&order, 2);
        let context = resale_context(asset_id, market_id, queried_right_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::RightInvalid,
            || create_runtime_order(RuntimeOrigin::signed(4), order, seller, signature),
        );
    });
}

fn run_neg_meta_right() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order) = create_transfer_meta_fixture(100);
        let right_id = [0xa3; 32];
        assert_ok!(DataAssets::create_right_object(structured_right(
            asset_id,
            right_id,
            2,
            1,
            Some(100),
        )));
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        let mut context = meta_context(asset_id, market_id, auth_id, order_id);
        context.right_id = Some(right_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::MutualExclusionConflict,
            || create_runtime_order(RuntimeOrigin::signed(3), order, seller, signature),
        );
    });
}

fn run_neg_version() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, mut order) = create_transfer_meta_fixture(100);
        order.seller_state_version = 0;
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        let context = meta_context(asset_id, market_id, auth_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::StateVersionMismatch,
            || create_runtime_order(RuntimeOrigin::signed(3), order, seller, signature),
        );
    });
}

fn run_neg_payer() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order) = create_transfer_meta_fixture(100);
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order.clone(),
            seller,
            signature,
        ));
        let (commitment, signature) = signed_buyer_input(&order, 2, 0);
        let buyer = commitment.buyer;
        let mut context = meta_context(asset_id, market_id, auth_id, order_id);
        context.buyer = buyer;
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::PayerBuyerMismatch,
            || DataAssets::fund_order(RuntimeOrigin::signed(4), order_id, commitment, signature),
        );
    });
}

fn run_neg_order_phase() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order) = create_transfer_meta_fixture(100);
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order,
            seller,
            signature,
        ));
        let context = meta_context(asset_id, market_id, auth_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::OrderNotFunded,
            || DataAssets::settle(RuntimeOrigin::signed(4), order_id),
        );
    });
}

fn run_neg_market_restricted() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order) = create_transfer_meta_fixture(100);
        MARKETS.with(|markets| {
            markets.borrow_mut().get_mut(&market_id).unwrap().0 = false;
        });
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        let context = meta_context(asset_id, market_id, auth_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::MarketRestricted,
            || create_runtime_order(RuntimeOrigin::signed(3), order, seller, signature),
        );
    });
}

fn run_leg_expire_resale() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, order_id, order) = create_transfer_right_fixture(10, 0);
        let (seller, signature, _) = signed_seller_input(&order, 2);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(4),
            order,
            seller,
            signature,
        ));
        System::set_block_number(11);
        assert_ok!(DataAssets::expire_open(RuntimeOrigin::signed(1), order_id));
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Expired
        );
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id)
                .unwrap()
                .holder,
            2
        );
        assert_eq!(DataAssets::active_right_order(right_id), None);
        assert_eq!(market_active_order_count(market_id), 0);
        assert_m_no_settlement(order_id);
    });
}

fn run_duplicate_settle(first_relayer: AccountId, second_relayer: AccountId) {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id) = fund_transfer_meta_fixture(100, 0);
        assert_ok!(DataAssets::settle(
            RuntimeOrigin::signed(first_relayer),
            order_id,
        ));
        let context = meta_context(asset_id, market_id, auth_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::AlreadySettled,
            || DataAssets::settle(RuntimeOrigin::signed(second_relayer), order_id),
        );
        assert_m_once(order_id);
        assert_eq!(DataAssets::get_asset(&asset_id).unwrap().core.owner, 2);
    });
}

fn run_race_buyer() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [0xb1; 32];
        register_mock_market(market_id, true, 1 << 0);
        let mut terms = transfer_authorization_terms(100, 0);
        terms.allowed_buyer = None;
        assert_ok!(DataAssets::create_transfer_meta_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            terms,
        ));
        let auth_id = DataAssets::active_transfer_meta_authorization(asset_id, market_id).unwrap();
        let mut order = transfer_meta_order(asset_id, market_id, auth_id, 0, 100);
        order.allowed_buyer = None;
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(4),
            order.clone(),
            seller,
            signature,
        ));
        let (buyer_one, buyer_one_signature) = signed_buyer_input(&order, 2, 0);
        let (buyer_two, buyer_two_signature) = signed_buyer_input(&order, 3, 0);
        assert_ok!(DataAssets::fund_order(
            RuntimeOrigin::signed(2),
            order_id,
            buyer_one,
            buyer_one_signature,
        ));
        let context = meta_context(asset_id, market_id, auth_id, order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::OrderStatusInvalid,
            || {
                DataAssets::fund_order(
                    RuntimeOrigin::signed(3),
                    order_id,
                    buyer_two,
                    buyer_two_signature,
                )
            },
        );
        let stored = DataAssets::runtime_order(order_id).unwrap();
        assert_eq!(
            stored.buyer_commitment.unwrap().buyer,
            test_protocol_account(2)
        );
        assert_eq!(crate::pallet::RuntimeEscrows::<Test>::iter().count(), 1);
        assert_eq!(DataAssets::buyer_nonce(test_protocol_account(2)), 1);
        assert_eq!(DataAssets::buyer_nonce(test_protocol_account(3)), 0);
    });
}

fn run_race_object() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, first_order_id, first_order) =
            create_transfer_right_fixture(100, 0);
        let (seller, signature, _) = signed_seller_input(&first_order, 2);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(4),
            first_order.clone(),
            seller,
            signature,
        ));
        let mut second_order = first_order;
        second_order.order_nonce = 1;
        second_order.allowed_buyer = Some(test_protocol_account(4));
        let (seller, signature, second_order_id) = signed_seller_input(&second_order, 2);
        let context = resale_context(asset_id, market_id, right_id, second_order_id);
        assert_m_reject_clean(
            context,
            pallet_dataassets::Error::<Test>::ActiveObjectOrderExists,
            || create_runtime_order(RuntimeOrigin::signed(1), second_order, seller, signature),
        );
        assert_eq!(
            DataAssets::active_right_order(right_id),
            Some(first_order_id)
        );
        assert_eq!(market_active_order_count(market_id), 1);
    });
}

fn run_neg_right_missing() {
    run_neg_right(InvalidRight::Missing);
}

fn run_neg_right_expired() {
    run_neg_right(InvalidRight::Expired);
}

fn run_neg_right_exhausted() {
    run_neg_right(InvalidRight::Exhausted);
}

fn run_neg_right_revoked() {
    run_neg_right(InvalidRight::Revoked);
}

fn run_dup_settle() {
    run_duplicate_settle(3, 4);
}

fn run_race_relayer() {
    run_duplicate_settle(3, 1);
}

const SCENARIOS: &[ScenarioSpec] = &[
    ScenarioSpec {
        id: "LEG-META-01",
        category: Category::Legal,
        invariants: I1 | I3 | I7,
        queries: Q_ALL & !Q_RIGHT,
        metrics: M_ACCEPT | M_AUDIT,
        expected_error: None,
        execute: transfer_meta_settlement_atomically_releases_payment_and_consumes_authorization,
    },
    ScenarioSpec {
        id: "LEG-ISSUE-01",
        category: Category::Legal,
        invariants: I1 | I3 | I7 | I10,
        queries: Q_ALL,
        metrics: M_ACCEPT | M_AUDIT,
        expected_error: None,
        execute: issue_right_settlement_creates_deterministic_right_and_converts_quota_atomically,
    },
    ScenarioSpec {
        id: "LEG-RESALE-01",
        category: Category::Legal,
        invariants: I1 | I3 | I6 | I7,
        queries: Q_ALL,
        metrics: M_ACCEPT | M_AUDIT,
        expected_error: None,
        execute: right_resale_settlement_transfers_holder_pays_royalty_and_closes_state,
    },
    ScenarioSpec {
        id: "LEG-CANCEL-01",
        category: Category::Legal,
        invariants: I3,
        queries: Q_META | Q_ORDER | Q_AUTH | Q_PAY | Q_EVID | Q_EVENT | Q_MARKET,
        metrics: M_ACCEPT | M_NO_SETTLEMENT,
        expected_error: None,
        execute: current_owner_can_cancel_open_without_market_consent_and_terminal_is_irreversible,
    },
    ScenarioSpec {
        id: "LEG-EXPIRE-01",
        category: Category::Legal,
        invariants: I3,
        queries: Q_RIGHT | Q_ORDER | Q_PAY | Q_EVID | Q_EVENT | Q_MARKET,
        metrics: M_ACCEPT | M_NO_SETTLEMENT,
        expected_error: None,
        execute: run_leg_expire_resale,
    },
    ScenarioSpec {
        id: "LEG-REFUND-01",
        category: Category::Legal,
        invariants: I3 | I8 | I10,
        queries: Q_META | Q_RIGHT | Q_ORDER | Q_AUTH | Q_PAY | Q_EVID | Q_EVENT | Q_MARKET,
        metrics: M_ACCEPT | M_NO_SETTLEMENT,
        expected_error: None,
        execute: refund_after_grace_returns_full_issue_payment_only_to_recorded_buyer,
    },
    ScenarioSpec {
        id: "NEG-PROTO-01",
        category: Category::Illegal,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::UnsupportedProtocolVersion),
        execute: run_neg_proto,
    },
    ScenarioSpec {
        id: "NEG-SIGN-01",
        category: Category::Illegal,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::InvalidSignature),
        execute: run_neg_signature,
    },
    ScenarioSpec {
        id: "NEG-MARKET-01",
        category: Category::Illegal,
        invariants: I2 | I4 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::MarketNotRegistered),
        execute: run_neg_market,
    },
    ScenarioSpec {
        id: "NEG-CAPABILITY-01",
        category: Category::Illegal,
        invariants: I2 | I4 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::CapabilityDenied),
        execute: run_neg_capability,
    },
    ScenarioSpec {
        id: "NEG-AUTH-01",
        category: Category::Illegal,
        invariants: I2 | I4 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::AuthorizationMissing),
        execute: run_neg_resale_authorization,
    },
    ScenarioSpec {
        id: "NEG-OWNER-01",
        category: Category::Illegal,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::OwnershipMismatch),
        execute: run_neg_owner,
    },
    ScenarioSpec {
        id: "NEG-HOLDER-01",
        category: Category::Illegal,
        invariants: I2 | I6 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::OwnershipMismatch),
        execute: run_neg_holder,
    },
    ScenarioSpec {
        id: "NEG-PARENT-01",
        category: Category::Illegal,
        invariants: I2 | I6 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::ParentMismatch),
        execute: run_neg_parent,
    },
    ScenarioSpec {
        id: "NEG-RIGHT-MISSING-01",
        category: Category::Illegal,
        invariants: I2 | I6 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::RightInvalid),
        execute: run_neg_right_missing,
    },
    ScenarioSpec {
        id: "NEG-RIGHT-EXPIRED-01",
        category: Category::Illegal,
        invariants: I2 | I6 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::RightInvalid),
        execute: run_neg_right_expired,
    },
    ScenarioSpec {
        id: "NEG-RIGHT-EXHAUSTED-01",
        category: Category::Illegal,
        invariants: I2 | I6 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::RightInvalid),
        execute: run_neg_right_exhausted,
    },
    ScenarioSpec {
        id: "NEG-RIGHT-REVOKED-01",
        category: Category::Illegal,
        invariants: I2 | I6 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::RightInvalid),
        execute: run_neg_right_revoked,
    },
    ScenarioSpec {
        id: "NEG-META-RIGHT-01",
        category: Category::Illegal,
        invariants: I2 | I8 | I9,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::MutualExclusionConflict),
        execute: run_neg_meta_right,
    },
    ScenarioSpec {
        id: "NEG-VERSION-01",
        category: Category::Illegal,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::StateVersionMismatch),
        execute: run_neg_version,
    },
    ScenarioSpec {
        id: "NEG-NONCE-01",
        category: Category::Illegal,
        invariants: I2 | I5 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::NonceUsed),
        execute: create_order_writes_one_authoritative_open_order_without_payment_or_quota,
    },
    ScenarioSpec {
        id: "NEG-PRICE-01",
        category: Category::Illegal,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::BelowPriceFloor),
        execute: low_price_and_fee_cap_rejections_leave_open_payment_prestate_clean,
    },
    ScenarioSpec {
        id: "NEG-FEE-01",
        category: Category::Illegal,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::FeeOutOfBounds),
        execute: transfer_right_create_requires_signed_policy_and_rejects_fee_above_cap_cleanly,
    },
    ScenarioSpec {
        id: "NEG-BALANCE-01",
        category: Category::Illegal,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::InsufficientBalance),
        execute: underfunded_buyer_cannot_fund_from_an_unrelated_balance,
    },
    ScenarioSpec {
        id: "NEG-PAYER-01",
        category: Category::Illegal,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::PayerBuyerMismatch),
        execute: run_neg_payer,
    },
    ScenarioSpec {
        id: "NEG-ORDER-PHASE-01",
        category: Category::Illegal,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::OrderNotFunded),
        execute: run_neg_order_phase,
    },
    ScenarioSpec {
        id: "NEG-AUTH-REVOKED-01",
        category: Category::Illegal,
        invariants: I2 | I8 | I10,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::AuthorizationRevoked),
        execute: revoked_issue_authorization_rejects_funding_without_reserving_anything,
    },
    ScenarioSpec {
        id: "NEG-REFUND-EARLY-01",
        category: Category::Illegal,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::GracePeriodActive),
        execute: refund_after_grace_returns_full_issue_payment_only_to_recorded_buyer,
    },
    ScenarioSpec {
        id: "NEG-MARKET-RESTRICTED-01",
        category: Category::Illegal,
        invariants: I2 | I4 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::MarketRestricted),
        execute: run_neg_market_restricted,
    },
    ScenarioSpec {
        id: "NEG-CAPACITY-01",
        category: Category::Illegal,
        invariants: I2 | I8 | I10,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::CapacityExceeded),
        execute: funded_capacity_failure_rolls_back_issue_quota_balance_and_nonce,
    },
    ScenarioSpec {
        id: "CLEAN-LATE-META-01",
        category: Category::Clean,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::AssetNotTradable),
        execute: transfer_meta_late_risk_rejections_preserve_funded_prestate,
    },
    ScenarioSpec {
        id: "CLEAN-LATE-RESALE-01",
        category: Category::Clean,
        invariants: I2 | I6 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::RightInvalid),
        execute: late_right_expiry_leaves_funded_resale_unchanged,
    },
    ScenarioSpec {
        id: "CLEAN-ABORT-META-01",
        category: Category::Clean,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_ROLLBACK,
        expected_error: None,
        execute: test_only_abort_rolls_back_transfer_meta_atomic_write_set,
    },
    ScenarioSpec {
        id: "CLEAN-ABORT-ISSUE-01",
        category: Category::Clean,
        invariants: I2 | I8 | I10,
        queries: Q_ALL,
        metrics: M_ROLLBACK,
        expected_error: None,
        execute: test_only_abort_rolls_back_issue_right_atomic_write_set,
    },
    ScenarioSpec {
        id: "CLEAN-ABORT-RESALE-01",
        category: Category::Clean,
        invariants: I2 | I8,
        queries: Q_ALL,
        metrics: M_ROLLBACK,
        expected_error: None,
        execute: test_only_abort_rolls_back_right_resale_atomic_write_set,
    },
    ScenarioSpec {
        id: "DUP-SETTLE-01",
        category: Category::Duplicate,
        invariants: I3 | I5 | I8,
        queries: Q_ALL,
        metrics: M_ONCE | M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::AlreadySettled),
        execute: run_dup_settle,
    },
    ScenarioSpec {
        id: "DUP-CREATE-01",
        category: Category::Duplicate,
        invariants: I5 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::NonceUsed),
        execute: create_order_writes_one_authoritative_open_order_without_payment_or_quota,
    },
    ScenarioSpec {
        id: "DUP-REFUND-01",
        category: Category::Duplicate,
        invariants: I5 | I8 | I10,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::TerminalOrder),
        execute: refund_after_grace_returns_full_issue_payment_only_to_recorded_buyer,
    },
    ScenarioSpec {
        id: "RACE-BUYER-01",
        category: Category::Race,
        invariants: I5 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::OrderNotOpen),
        execute: run_race_buyer,
    },
    ScenarioSpec {
        id: "RACE-RELAYER-01",
        category: Category::Race,
        invariants: I1 | I3 | I5 | I7 | I8,
        queries: Q_ALL,
        metrics: M_ONCE | M_AUDIT,
        expected_error: Some(CommonErrorCode::AlreadySettled),
        execute: run_race_relayer,
    },
    ScenarioSpec {
        id: "RACE-QUOTA-01",
        category: Category::Race,
        invariants: I3 | I5 | I8 | I10,
        queries: Q_ALL,
        metrics: M_ONCE | M_AUDIT,
        expected_error: Some(CommonErrorCode::QuotaExceeded),
        execute: last_issue_quota_competition_funds_and_issues_only_one_right,
    },
    ScenarioSpec {
        id: "RACE-OBJECT-01",
        category: Category::Race,
        invariants: I5 | I8,
        queries: Q_ALL,
        metrics: M_REJECT_CLEAN,
        expected_error: Some(CommonErrorCode::MutualExclusionConflict),
        execute: run_race_object,
    },
];

fn run_scenario(id: &str) {
    let scenario = SCENARIOS
        .iter()
        .find(|scenario| scenario.id == id)
        .unwrap_or_else(|| panic!("missing frozen scenario {id}"));
    (scenario.execute)();
}

#[test]
fn frozen_catalog_has_exact_counts_queries_metrics_and_i1_i10_coverage() {
    assert_eq!(SCENARIOS.len(), 42);
    let ids = SCENARIOS
        .iter()
        .map(|scenario| scenario.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 42);
    assert_eq!(
        SCENARIOS
            .iter()
            .filter(|s| s.category == Category::Legal)
            .count(),
        6
    );
    assert_eq!(
        SCENARIOS
            .iter()
            .filter(|s| s.category == Category::Illegal)
            .count(),
        24
    );
    assert_eq!(
        SCENARIOS
            .iter()
            .filter(|s| s.category == Category::Clean)
            .count(),
        5
    );
    assert_eq!(
        SCENARIOS
            .iter()
            .filter(|s| s.category == Category::Duplicate)
            .count(),
        3
    );
    assert_eq!(
        SCENARIOS
            .iter()
            .filter(|s| s.category == Category::Race)
            .count(),
        4
    );
    assert_eq!(
        SCENARIOS.iter().fold(0, |mask, s| mask | s.invariants),
        REQUIRED_INVARIANTS
    );
    assert!(SCENARIOS.iter().all(|scenario| scenario.queries != 0));
    assert!(SCENARIOS
        .iter()
        .all(|scenario| scenario.queries & !Q_ALL == 0));
    assert!(SCENARIOS.iter().all(|scenario| scenario.metrics != 0));
    assert!(SCENARIOS
        .iter()
        .all(|scenario| scenario.metrics & !ALL_METRICS == 0));
    assert!(SCENARIOS
        .iter()
        .filter(|scenario| scenario.category == Category::Illegal)
        .all(|scenario| scenario.expected_error.is_some()));
}

#[test]
fn q_and_m_packages_match_the_frozen_definitions() {
    let query_packages = [
        (Q_META, 4usize),
        (Q_RIGHT, 8),
        (Q_ORDER, 7),
        (Q_AUTH, 7),
        (Q_PAY, 7),
        (Q_EVID, 4),
        (Q_EVENT, 4),
        (Q_MARKET, 4),
        (Q_ID, 5),
    ];
    assert_eq!(
        query_packages.iter().fold(0, |mask, item| mask | item.0),
        Q_ALL
    );
    assert!(query_packages
        .iter()
        .all(|(_, required_fields)| *required_fields > 0));
    assert_eq!(ALL_METRICS.count_ones(), 6);
    assert_eq!((M_ACCEPT | M_AUDIT).count_ones(), 2);
    assert_eq!((M_ONCE | M_REJECT_CLEAN).count_ones(), 2);
}

macro_rules! scenario_tests {
    ($($name:ident => $id:literal),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                run_scenario($id);
            }
        )+
    };
}

scenario_tests! {
    leg_meta_01 => "LEG-META-01",
    leg_issue_01 => "LEG-ISSUE-01",
    leg_resale_01 => "LEG-RESALE-01",
    leg_cancel_01 => "LEG-CANCEL-01",
    leg_expire_01 => "LEG-EXPIRE-01",
    leg_refund_01 => "LEG-REFUND-01",
    neg_proto_01 => "NEG-PROTO-01",
    neg_sign_01 => "NEG-SIGN-01",
    neg_market_01 => "NEG-MARKET-01",
    neg_capability_01 => "NEG-CAPABILITY-01",
    neg_auth_01 => "NEG-AUTH-01",
    neg_owner_01 => "NEG-OWNER-01",
    neg_holder_01 => "NEG-HOLDER-01",
    neg_parent_01 => "NEG-PARENT-01",
    neg_right_missing_01 => "NEG-RIGHT-MISSING-01",
    neg_right_expired_01 => "NEG-RIGHT-EXPIRED-01",
    neg_right_exhausted_01 => "NEG-RIGHT-EXHAUSTED-01",
    neg_right_revoked_01 => "NEG-RIGHT-REVOKED-01",
    neg_meta_right_01 => "NEG-META-RIGHT-01",
    neg_version_01 => "NEG-VERSION-01",
    neg_nonce_01 => "NEG-NONCE-01",
    neg_price_01 => "NEG-PRICE-01",
    neg_fee_01 => "NEG-FEE-01",
    neg_balance_01 => "NEG-BALANCE-01",
    neg_payer_01 => "NEG-PAYER-01",
    neg_order_phase_01 => "NEG-ORDER-PHASE-01",
    neg_auth_revoked_01 => "NEG-AUTH-REVOKED-01",
    neg_refund_early_01 => "NEG-REFUND-EARLY-01",
    neg_market_restricted_01 => "NEG-MARKET-RESTRICTED-01",
    neg_capacity_01 => "NEG-CAPACITY-01",
    clean_late_meta_01 => "CLEAN-LATE-META-01",
    clean_late_resale_01 => "CLEAN-LATE-RESALE-01",
    clean_abort_meta_01 => "CLEAN-ABORT-META-01",
    clean_abort_issue_01 => "CLEAN-ABORT-ISSUE-01",
    clean_abort_resale_01 => "CLEAN-ABORT-RESALE-01",
    dup_settle_01 => "DUP-SETTLE-01",
    dup_create_01 => "DUP-CREATE-01",
    dup_refund_01 => "DUP-REFUND-01",
    race_buyer_01 => "RACE-BUYER-01",
    race_relayer_01 => "RACE-RELAYER-01",
    race_quota_01 => "RACE-QUOTA-01",
    race_object_01 => "RACE-OBJECT-01",
}
