use crate as pallet_dataassets;
use crate::protocol::{
    self, BuyerCommitment, CurrencyId, MessageType, ObjectType, OrderCommitment, OrderState,
    PaymentState, ProtocolError, SellerCommitment, SETTLEMENT_PROTOCOL_VERSION,
};
use crate::types::{
    decode_data_asset_compat, decode_right_token_compat, AssetStatus, AuthorizationStatus,
    CertificateStatus, DataAsset, DataScope, IssueQuotaReservationStatus,
    IssueRightAuthorizationTerms, LegacyAssetCore, LegacyAssetStatus, LegacyCertificateStatus,
    LegacyDataAsset, LegacyRightToken, LegacyRightType, MarketOrder, MarketOrderStatus,
    PricePolicy, RightToken, RightType, RightValidity, TradeAssetType,
    TransferMetaAuthorizationTerms,
};
use codec::{Decode, Encode};
use frame_support::{
    assert_noop, assert_ok, construct_runtime, derive_impl,
    dispatch::DispatchResult,
    parameter_types,
    traits::{ConstU128, ConstU32},
    weights::Weight,
    BoundedVec,
};
use pallet_shared_traits::{
    MarketAccessError, MarketOperation, MarketPaymentTerms, MarketRegistryProvider,
    MarketStateSnapshot, ProtocolAccountIdConverter,
};
use sp_core::{sr25519, Pair, H256};
use sp_runtime::BuildStorage;
use std::cell::RefCell;
use std::collections::HashMap;

mod invariant_scenarios;

type AccountId = u64;
type Balance = u128;
type Block = frame_system::mocking::MockBlock<Test>;

construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        Timestamp: pallet_timestamp,
        DataAssets: pallet_dataassets,
    }
);

pub struct TestWeightInfo;

impl crate::pallet::WeightInfo for TestWeightInfo {
    fn register_asset() -> Weight {
        Weight::zero()
    }
    fn register_asset_core() -> Weight {
        Weight::zero()
    }
    fn issue_certificate() -> Weight {
        Weight::zero()
    }
    fn transfer_certificate() -> Weight {
        Weight::zero()
    }
    fn transfer_asset() -> Weight {
        Weight::zero()
    }
    fn revoke_certificate() -> Weight {
        Weight::zero()
    }
    fn lock_asset() -> Weight {
        Weight::zero()
    }
    fn unlock_asset() -> Weight {
        Weight::zero()
    }
    fn slash_collateral() -> Weight {
        Weight::zero()
    }
    fn authorize_operator() -> Weight {
        Weight::zero()
    }
    fn revoke_authorization() -> Weight {
        Weight::zero()
    }
    fn create_order() -> Weight {
        Weight::zero()
    }
    fn cancel_open() -> Weight {
        Weight::zero()
    }
    fn expire_open() -> Weight {
        Weight::zero()
    }
    fn fund_order() -> Weight {
        Weight::zero()
    }
    fn refund_funded() -> Weight {
        Weight::zero()
    }
    fn settle() -> Weight {
        Weight::zero()
    }
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountData = pallet_balances::AccountData<Balance>;
}

impl pallet_balances::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type Balance = Balance;
    type DustRemoval = ();
    type ExistentialDeposit = ConstU128<1>;
    type AccountStore = System;
    type WeightInfo = ();
    type MaxLocks = ConstU32<50>;
    type MaxReserves = ();
    type ReserveIdentifier = [u8; 8];
    type FreezeIdentifier = ();
    type MaxFreezes = ();
    type RuntimeHoldReason = ();
    type RuntimeFreezeReason = ();
    type DoneSlashHandler = ();
}

parameter_types! {
    pub const MinimumPeriod: u64 = 1;
    pub const BaseCollateral: Balance = 10;
    pub const CollateralPerMb: Balance = 1;
    pub const MaxCollateral: Balance = 100;
    pub const FundedGraceBlocks: u32 = 10;
}

impl pallet_timestamp::Config for Test {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = MinimumPeriod;
    type WeightInfo = ();
}

thread_local! {
    static FIRST_CREATE_REWARDS: RefCell<Vec<([u8; 32], AccountId)>> = RefCell::new(Vec::new());
    static TRADE_MEASUREMENTS: RefCell<Vec<[u8; 32]>> = RefCell::new(Vec::new());
    static MARKETS: RefCell<HashMap<[u8; 32], (bool, u8, u32)>> = RefCell::new(HashMap::new());
    static ACTIVE_ORDERS: RefCell<HashMap<[u8; 32], (u32, u32)>> = RefCell::new(HashMap::new());
    static MARKET_PAYMENTS: RefCell<HashMap<[u8; 32], MockMarketPayment>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy)]
struct MockMarketPayment {
    market_fee_bps: u16,
    recipient: AccountId,
    funded_orders: u32,
    max_funded_orders: u32,
    exposure: u128,
    max_exposure: u128,
}

pub struct MockMarketRegistry;

impl MarketRegistryProvider<AccountId> for MockMarketRegistry {
    fn market_state(
        market_id: &[u8; 32],
    ) -> Result<MarketStateSnapshot<AccountId>, MarketAccessError> {
        let (active, capabilities, active_authorizations) = MARKETS.with(|markets| {
            markets
                .borrow()
                .get(market_id)
                .copied()
                .ok_or(MarketAccessError::NotRegistered)
        })?;
        let active_orders = ACTIVE_ORDERS.with(|orders| {
            orders
                .borrow()
                .get(market_id)
                .map(|value| value.0)
                .ok_or(MarketAccessError::NotRegistered)
        })?;
        let payment = MARKET_PAYMENTS.with(|payments| {
            payments
                .borrow()
                .get(market_id)
                .copied()
                .ok_or(MarketAccessError::NotRegistered)
        })?;
        Ok(MarketStateSnapshot {
            market_id: *market_id,
            operator: payment.recipient,
            protocol_version: SETTLEMENT_PROTOCOL_VERSION,
            capabilities,
            capacity_tier: 0,
            active_orders,
            funded_orders: payment.funded_orders,
            active_exposure_value: payment.exposure,
            active_authorizations,
            status: if active { 0 } else { 1 },
            market_fee_bps: payment.market_fee_bps,
        })
    }

    fn ensure_market_operation(
        market_id: &[u8; 32],
        operation: MarketOperation,
    ) -> Result<(), MarketAccessError> {
        MARKETS.with(|markets| {
            let markets = markets.borrow();
            let (active, capabilities, _) = markets
                .get(market_id)
                .copied()
                .ok_or(MarketAccessError::NotRegistered)?;
            if !active {
                return Err(MarketAccessError::Restricted);
            }
            let required = match operation {
                MarketOperation::TransferMeta => 1 << 0,
                MarketOperation::IssueRight => 1 << 1,
                MarketOperation::TransferRight => 1 << 2,
            };
            if capabilities & required == 0 {
                return Err(MarketAccessError::CapabilityDenied);
            }
            Ok(())
        })
    }

    fn note_authorization_created(market_id: &[u8; 32]) -> Result<(), MarketAccessError> {
        MARKETS.with(|markets| {
            let mut markets = markets.borrow_mut();
            let entry = markets
                .get_mut(market_id)
                .ok_or(MarketAccessError::NotRegistered)?;
            entry.2 = entry
                .2
                .checked_add(1)
                .ok_or(MarketAccessError::AuthorizationCounterOverflow)?;
            Ok(())
        })
    }

    fn note_authorization_closed(market_id: &[u8; 32]) -> Result<(), MarketAccessError> {
        MARKETS.with(|markets| {
            let mut markets = markets.borrow_mut();
            let entry = markets
                .get_mut(market_id)
                .ok_or(MarketAccessError::NotRegistered)?;
            entry.2 = entry
                .2
                .checked_sub(1)
                .ok_or(MarketAccessError::AuthorizationCounterUnderflow)?;
            Ok(())
        })
    }

    fn note_active_order_created(market_id: &[u8; 32]) -> Result<(), MarketAccessError> {
        Self::ensure_market_operation(market_id, MarketOperation::TransferMeta).or_else(
            |error| {
                if error == MarketAccessError::CapabilityDenied {
                    Ok(())
                } else {
                    Err(error)
                }
            },
        )?;
        ACTIVE_ORDERS.with(|orders| {
            let mut orders = orders.borrow_mut();
            let entry = orders
                .get_mut(market_id)
                .ok_or(MarketAccessError::NotRegistered)?;
            let next = entry
                .0
                .checked_add(1)
                .ok_or(MarketAccessError::ActiveOrderCounterOverflow)?;
            if next > entry.1 {
                return Err(MarketAccessError::ActiveOrderCapacityExceeded);
            }
            entry.0 = next;
            Ok(())
        })
    }

    fn note_active_order_closed(market_id: &[u8; 32]) -> Result<(), MarketAccessError> {
        ACTIVE_ORDERS.with(|orders| {
            let mut orders = orders.borrow_mut();
            let entry = orders
                .get_mut(market_id)
                .ok_or(MarketAccessError::NotRegistered)?;
            entry.0 = entry
                .0
                .checked_sub(1)
                .ok_or(MarketAccessError::ActiveOrderCounterUnderflow)?;
            Ok(())
        })
    }

    fn market_payment_terms(
        market_id: &[u8; 32],
    ) -> Result<MarketPaymentTerms<AccountId>, MarketAccessError> {
        let active = MARKETS.with(|markets| {
            markets
                .borrow()
                .get(market_id)
                .map(|market| market.0)
                .ok_or(MarketAccessError::NotRegistered)
        })?;
        if !active {
            return Err(MarketAccessError::Restricted);
        }
        MARKET_PAYMENTS.with(|payments| {
            let payment = payments
                .borrow()
                .get(market_id)
                .copied()
                .ok_or(MarketAccessError::NotRegistered)?;
            Ok(MarketPaymentTerms {
                market_fee_bps: payment.market_fee_bps,
                market_fee_recipient: payment.recipient,
            })
        })
    }

    fn ensure_funding_capacity(
        market_id: &[u8; 32],
        gross_price: u128,
    ) -> Result<(), MarketAccessError> {
        MARKET_PAYMENTS.with(|payments| {
            let payment = payments
                .borrow()
                .get(market_id)
                .copied()
                .ok_or(MarketAccessError::NotRegistered)?;
            let next_funded = payment
                .funded_orders
                .checked_add(1)
                .ok_or(MarketAccessError::FundedOrderCounterOverflow)?;
            if next_funded > payment.max_funded_orders {
                return Err(MarketAccessError::FundedOrderCapacityExceeded);
            }
            let next_exposure = payment
                .exposure
                .checked_add(gross_price)
                .ok_or(MarketAccessError::ExposureCounterOverflow)?;
            if next_exposure > payment.max_exposure {
                return Err(MarketAccessError::ExposureCapacityExceeded);
            }
            Ok(())
        })
    }

    fn note_order_funded(market_id: &[u8; 32], gross_price: u128) -> Result<(), MarketAccessError> {
        MARKET_PAYMENTS.with(|payments| {
            let mut payments = payments.borrow_mut();
            let payment = payments
                .get_mut(market_id)
                .ok_or(MarketAccessError::NotRegistered)?;
            let next_funded = payment
                .funded_orders
                .checked_add(1)
                .ok_or(MarketAccessError::FundedOrderCounterOverflow)?;
            if next_funded > payment.max_funded_orders {
                return Err(MarketAccessError::FundedOrderCapacityExceeded);
            }
            let next_exposure = payment
                .exposure
                .checked_add(gross_price)
                .ok_or(MarketAccessError::ExposureCounterOverflow)?;
            if next_exposure > payment.max_exposure {
                return Err(MarketAccessError::ExposureCapacityExceeded);
            }
            payment.funded_orders = next_funded;
            payment.exposure = next_exposure;
            Ok(())
        })
    }

    fn note_funded_order_closed(
        market_id: &[u8; 32],
        gross_price: u128,
    ) -> Result<(), MarketAccessError> {
        MARKET_PAYMENTS.with(|payments| {
            let mut payments = payments.borrow_mut();
            let payment = payments
                .get_mut(market_id)
                .ok_or(MarketAccessError::NotRegistered)?;
            payment.funded_orders = payment
                .funded_orders
                .checked_sub(1)
                .ok_or(MarketAccessError::FundedOrderCounterUnderflow)?;
            payment.exposure = payment
                .exposure
                .checked_sub(gross_price)
                .ok_or(MarketAccessError::ExposureCounterUnderflow)?;
            Ok(())
        })
    }
}

pub struct TestProtocolAccountIdConverter;

impl ProtocolAccountIdConverter<AccountId> for TestProtocolAccountIdConverter {
    fn to_protocol_account_id(account: &AccountId) -> Option<[u8; 32]> {
        let pair = sr25519::Pair::from_seed(&[*account as u8; 32]);
        let mut public = [0u8; 32];
        public.copy_from_slice(pair.public().as_ref());
        Some(public)
    }

    fn from_protocol_account_id(account: &[u8; 32]) -> Option<AccountId> {
        (0u8..=u8::MAX).find_map(|seed| {
            let pair = sr25519::Pair::from_seed(&[seed; 32]);
            let public = pair.public();
            let bytes: &[u8] = public.as_ref();
            (bytes == account).then_some(u64::from(seed))
        })
    }
}

pub struct TradeRecorder;

impl pallet_shared_traits::IncentiveHandler<AccountId, [u8; 32], Balance> for TradeRecorder {
    fn distribute_first_create_reward(
        recipient: &AccountId,
        asset_id: &[u8; 32],
    ) -> Result<(), &'static str> {
        FIRST_CREATE_REWARDS.with(|rewards| rewards.borrow_mut().push((*asset_id, *recipient)));
        Ok(())
    }

    fn register_asset_trade(asset_id: &[u8; 32]) {
        TRADE_MEASUREMENTS.with(|measurements| measurements.borrow_mut().push(*asset_id));
    }

    fn distribute_liquidity_reward(
        _recipient: &AccountId,
        _order_amount: Balance,
    ) -> Result<(), &'static str> {
        Ok(())
    }

    fn distribute_proposal_reward(_recipient: &AccountId) -> Result<(), &'static str> {
        Ok(())
    }
}

impl pallet_dataassets::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type BaseCollateral = BaseCollateral;
    type CollateralPerMB = CollateralPerMb;
    type MaxCollateral = MaxCollateral;
    type MaxNameLength = ConstU32<256>;
    type MaxDescriptionLength = ConstU32<1024>;
    type IncentiveHandler = TradeRecorder;
    type MarketRegistry = MockMarketRegistry;
    type ProtocolAccountIdConverter = TestProtocolAccountIdConverter;
    type FundedGraceBlocks = FundedGraceBlocks;
    type LegacyMutationEnabled = frame_support::traits::ConstBool<true>;
    type WeightInfo = TestWeightInfo;
}

pub(crate) fn new_test_ext() -> sp_io::TestExternalities {
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    pallet_balances::GenesisConfig::<Test> {
        balances: vec![(1, 1_000), (2, 1_000), (3, 1_000), (4, 1_000)],
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .unwrap();

    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| {
        System::set_block_number(1);
        Timestamp::set_timestamp(42);
        FIRST_CREATE_REWARDS.with(|rewards| rewards.borrow_mut().clear());
        TRADE_MEASUREMENTS.with(|measurements| measurements.borrow_mut().clear());
        MARKETS.with(|markets| markets.borrow_mut().clear());
        ACTIVE_ORDERS.with(|orders| orders.borrow_mut().clear());
        MARKET_PAYMENTS.with(|payments| payments.borrow_mut().clear());
    });
    ext
}

fn first_create_rewards() -> Vec<([u8; 32], AccountId)> {
    FIRST_CREATE_REWARDS.with(|rewards| rewards.borrow().clone())
}

fn trade_measurements() -> Vec<[u8; 32]> {
    TRADE_MEASUREMENTS.with(|measurements| measurements.borrow().clone())
}

fn register_test_asset(owner: AccountId) -> ([u8; 32], H256) {
    let raw_data_hash = H256::repeat_byte(0x11);
    assert_ok!(DataAssets::register_asset(
        RuntimeOrigin::signed(owner),
        b"asset".to_vec(),
        b"description".to_vec(),
        raw_data_hash,
        1024 * 1024,
    ));

    let asset_id = DataAsset::generate_asset_id(&owner, Timestamp::get(), &raw_data_hash);
    assert!(DataAssets::get_asset(&asset_id).is_some());
    (asset_id, raw_data_hash)
}

fn certificate_id(asset_id: &[u8; 32], issuer: AccountId, token_id: u32) -> [u8; 32] {
    RightToken::generate_certificate_id(asset_id, System::block_number(), &issuer, token_id)
}

fn structured_right(
    asset_id: [u8; 32],
    right_id: [u8; 32],
    holder: AccountId,
    issuer: AccountId,
    valid_until: Option<u64>,
) -> RightToken<AccountId> {
    let mut right = RightToken::minimal(
        0,
        RightType::Train,
        holder,
        issuer,
        asset_id,
        System::block_number(),
        valid_until,
    );
    right.right_id = right_id;
    right.data_scope = DataScope::FieldSetHash(H256::repeat_byte(0x5a));
    right.validity.max_uses = Some(5);
    right.remaining_uses = Some(5);
    right.transferable = true;
    right.allowed_transfer_markets =
        BoundedVec::try_from(vec![[7u8; 32], [8u8; 32]]).expect("bounded markets");
    right.royalty_bps = 250;
    right
}

fn trade_settled_event_count() -> usize {
    System::events()
        .iter()
        .filter(|record| {
            matches!(
                record.event,
                RuntimeEvent::DataAssets(pallet_dataassets::Event::TradeSettled { .. })
            )
        })
        .count()
}

fn settlement_finalized_events() -> Vec<([u8; 32], [u8; 32], [u8; 32])> {
    System::events()
        .iter()
        .filter_map(|record| match &record.event {
            RuntimeEvent::DataAssets(pallet_dataassets::Event::SettlementFinalized {
                settlement_id,
                order_id,
                post_state_commitment,
            }) => Some((*settlement_id, *order_id, *post_state_commitment)),
            _ => None,
        })
        .collect()
}

type TestBalance = Balance;
type TestBlockNumber = u64;

fn insert_market_order(
    order_id: [u8; 32],
    seller: AccountId,
    object_type: TradeAssetType,
    object_id: [u8; 32],
) {
    let order = MarketOrder::<AccountId, TestBalance, TestBlockNumber> {
        order_id,
        order_digest: order_id, // 测试中用 order_id 本身作为 digest
        market: 3,              // 约定市场 = 3
        seller,
        buyer: None,
        object_type,
        object_id,
        parent_asset_id: None,
        price: 0,
        status: MarketOrderStatus::Open,
        created_at: System::block_number(),
    };
    pallet_dataassets::MarketOrders::<Test>::insert(order_id, order);
}

fn insert_market_order_with_parent(
    order_id: [u8; 32],
    seller: AccountId,
    object_type: TradeAssetType,
    object_id: [u8; 32],
    parent_asset_id: [u8; 32],
) {
    let order = MarketOrder::<AccountId, TestBalance, TestBlockNumber> {
        order_id,
        order_digest: order_id,
        market: 3,
        seller,
        buyer: None,
        object_type,
        object_id,
        parent_asset_id: Some(parent_asset_id),
        price: 0,
        status: MarketOrderStatus::Open,
        created_at: System::block_number(),
    };
    pallet_dataassets::MarketOrders::<Test>::insert(order_id, order);
}

fn protocol_public(pair: &sr25519::Pair) -> [u8; 32] {
    let mut public = [0u8; 32];
    public.copy_from_slice(pair.public().as_ref());
    public
}

fn protocol_signature(signature: sr25519::Signature) -> [u8; 64] {
    let mut bytes = [0u8; 64];
    bytes.copy_from_slice(signature.as_ref());
    bytes
}

fn protocol_order(seller: [u8; 32], buyer: [u8; 32]) -> OrderCommitment {
    OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id: [0x21; 32],
        operation: MarketOperation::TransferRight,
        object_type: ObjectType::RightToken,
        object_id: [0x31; 32],
        parent_asset_id: Some([0x41; 32]),
        authorization_id: [0x51; 32],
        seller,
        allowed_buyer: Some(buyer),
        price: 4_000_000_000_000,
        currency: CurrencyId::Native,
        policy_hash: [0x61; 32],
        right_parameters_hash: Some([0x71; 32]),
        seller_state_version: 7,
        order_nonce: 0,
        expiry: 100,
    }
}

fn protocol_seller_commitment(genesis_hash: [u8; 32], order: &OrderCommitment) -> SellerCommitment {
    SellerCommitment {
        order_id: protocol::order_id(genesis_hash, order),
        object_id: order.object_id,
        operation: order.operation,
        seller: order.seller,
        state_version: order.seller_state_version,
        price_policy_hash: order.policy_hash,
        right_parameters_hash: order.right_parameters_hash,
        expires_at: order.expiry,
        seller_nonce: order.order_nonce,
    }
}

fn protocol_buyer_commitment(
    genesis_hash: [u8; 32],
    order: &OrderCommitment,
    buyer: [u8; 32],
) -> BuyerCommitment {
    BuyerCommitment {
        order_id: protocol::order_id(genesis_hash, order),
        buyer,
        price: order.price,
        currency: order.currency,
        expires_at: order.expiry,
        buyer_nonce: 0,
    }
}

#[test]
fn register_asset_core_uses_deterministic_block_identity_without_collateral_or_incentive() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 1;
        let raw_data_hash = H256::repeat_byte(0x22);
        System::set_block_number(7);
        Timestamp::set_timestamp(42_000);

        assert_ok!(DataAssets::register_asset_core(
            RuntimeOrigin::signed(owner),
            b"asset".to_vec(),
            b"description".to_vec(),
            raw_data_hash,
            1024 * 1024,
        ));

        let asset_id = DataAsset::generate_asset_id(&owner, 7, &raw_data_hash);
        let asset = DataAssets::get_asset(&asset_id).unwrap();
        assert_eq!(asset.core.owner, owner);
        assert_eq!(asset.core.token_id, 0);
        assert!(DataAssets::asset_collateral(asset_id).is_none());
        assert_eq!(Balances::reserved_balance(1), 0);
        assert!(first_create_rewards().is_empty());
        assert!(trade_measurements().is_empty());
    });
}

#[test]
fn issue_certificate_registers_certificate_trade_measurement() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);

        assert_ok!(DataAssets::issue_certificate(
            RuntimeOrigin::signed(1),
            asset_id,
            2,
            1,
            None,
        ));

        assert_eq!(trade_measurements(), vec![asset_id]);
    });
}

#[test]
fn authorized_market_can_issue_certificate_to_buyer() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 1;
        let market: AccountId = 2;
        let buyer: AccountId = 3;
        let (asset_id, _) = register_test_asset(owner);

        assert_ok!(DataAssets::authorize_market(
            RuntimeOrigin::signed(owner),
            asset_id,
            market,
        ));

        let cert_id = DataAssets::issue_certificate_by_market_internal(
            &asset_id, &market, &owner, &buyer, 1, None,
        )
        .expect("authorized market issues certificate");

        let cert = DataAssets::get_certificate(&asset_id, &cert_id).unwrap();
        assert_eq!(cert.holder, buyer);
        assert_eq!(cert.issuer, owner);
        assert_eq!(cert.asset_id, asset_id);
        assert_eq!(trade_measurements(), vec![asset_id]);
    });
}

#[test]
fn market_asset_settlement_records_trade_evidence() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 1;
        let market: AccountId = 3;
        let buyer: AccountId = 2;
        let price: Balance = 250;
        let (asset_id, _) = register_test_asset(owner);

        assert_ok!(DataAssets::authorize_market(
            RuntimeOrigin::signed(owner),
            asset_id,
            market,
        ));

        let order_id = [1u8; 32];
        insert_market_order(order_id, owner, TradeAssetType::DataAsset, asset_id);

        let trade_id = DataAssets::settle_asset_trade_by_market_internal(
            &asset_id, &market, &buyer, price, &order_id, &order_id,
        )
        .expect("authorized market settles asset trade");

        let asset = DataAssets::get_asset(&asset_id).unwrap();
        assert_eq!(asset.core.owner, buyer);

        let settlement =
            DataAssets::trade_settlements(trade_id).expect("trade settlement is stored");
        assert_eq!(settlement.trade_id, trade_id);
        assert_eq!(settlement.market, market);
        assert_eq!(settlement.seller, owner);
        assert_eq!(settlement.buyer, buyer);
        assert_eq!(settlement.asset_id, asset_id);
        assert_eq!(settlement.certificate_id, [0u8; 32]);
        assert_eq!(settlement.asset_type, TradeAssetType::DataAsset);
        assert_eq!(settlement.price, price);
        assert_eq!(settlement.settled_at, System::block_number());

        System::assert_has_event(RuntimeEvent::DataAssets(
            pallet_dataassets::Event::AssetTransferred {
                asset_id,
                from: owner,
                to: buyer,
            },
        ));
        System::assert_has_event(RuntimeEvent::DataAssets(
            pallet_dataassets::Event::TradeSettled {
                trade_id,
                market,
                seller: owner,
                buyer,
                asset_id,
                certificate_id: [0u8; 32],
                asset_type: TradeAssetType::DataAsset,
                price,
                settled_at: System::block_number(),
            },
        ));
    });
}

#[test]
fn failed_market_asset_settlement_does_not_record_trade_evidence() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 1;
        let market: AccountId = 3;
        let buyer: AccountId = 2;
        let price: Balance = 250;
        let (asset_id, _) = register_test_asset(owner);

        let order_id = [2u8; 32];
        insert_market_order(order_id, owner, TradeAssetType::DataAsset, asset_id);
        // verify_settle_prerequisites 会在失败时 emit SettlementRejected 事件，
        // 因此不能用 assert_noop!（它要求存储完全不变）
        assert_eq!(
            DataAssets::settle_asset_trade_by_market_internal(
                &asset_id, &market, &buyer, price, &order_id, &order_id,
            )
            .unwrap_err(),
            pallet_dataassets::Error::<Test>::NotAuthorized.into(),
        );

        let asset = DataAssets::get_asset(&asset_id).unwrap();
        assert_eq!(asset.core.owner, owner);
        assert_eq!(trade_settled_event_count(), 0);
    });
}

#[test]
fn market_certificate_settlement_records_trade_evidence() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 1;
        let market: AccountId = 3;
        let buyer: AccountId = 2;
        let price: Balance = 75;
        let (asset_id, _) = register_test_asset(owner);

        assert_ok!(DataAssets::issue_certificate(
            RuntimeOrigin::signed(owner),
            asset_id,
            market,
            1,
            None,
        ));
        let cert_id = certificate_id(&asset_id, owner, 0);

        let order_id = [3u8; 32];
        // 权证结算需要父资产的授权 + MarketOrder 投影（含 parent_asset_id）
        assert_ok!(DataAssets::authorize_market(
            RuntimeOrigin::signed(owner),
            asset_id,
            market,
        ));
        insert_market_order_with_parent(
            order_id,
            market,
            TradeAssetType::Certificate,
            cert_id,
            asset_id,
        );

        let trade_id = DataAssets::settle_certificate_trade_internal(
            &asset_id, &cert_id, &market, &buyer, price, &order_id, &order_id,
        )
        .expect("certificate holder market settles certificate trade");

        let cert = DataAssets::get_certificate(&asset_id, &cert_id).unwrap();
        assert_eq!(cert.holder, buyer);

        let settlement =
            DataAssets::trade_settlements(trade_id).expect("trade settlement is stored");
        assert_eq!(settlement.trade_id, trade_id);
        assert_eq!(settlement.market, market);
        assert_eq!(settlement.seller, market);
        assert_eq!(settlement.buyer, buyer);
        assert_eq!(settlement.asset_id, asset_id);
        assert_eq!(settlement.certificate_id, cert_id);
        assert_eq!(settlement.asset_type, TradeAssetType::Certificate);
        assert_eq!(settlement.price, price);
        assert_eq!(settlement.settled_at, System::block_number());

        System::assert_has_event(RuntimeEvent::DataAssets(
            pallet_dataassets::Event::CertificateTransferred {
                asset_id,
                certificate_id: cert_id,
                from: market,
                to: buyer,
            },
        ));
        System::assert_has_event(RuntimeEvent::DataAssets(
            pallet_dataassets::Event::TradeSettled {
                trade_id,
                market,
                seller: market,
                buyer,
                asset_id,
                certificate_id: cert_id,
                asset_type: TradeAssetType::Certificate,
                price,
                settled_at: System::block_number(),
            },
        ));
    });
}

#[test]
fn failed_market_certificate_settlement_does_not_record_trade_evidence() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 1;
        let market: AccountId = 3;
        let buyer: AccountId = 2;
        let price: Balance = 75;
        let (asset_id, _) = register_test_asset(owner);
        let missing_cert_id = [9u8; 32];

        let order_id = [4u8; 32];
        // 即使权证不存在，也需要 MarketOrder 投影（verify_settle_prerequisites 先查订单）
        // 这里用 asset_id 作为 parent_asset_id 来避免 AssetNotFound，让验证走到 Certificate 分支
        assert_ok!(DataAssets::authorize_market(
            RuntimeOrigin::signed(owner),
            asset_id,
            market,
        ));
        insert_market_order_with_parent(
            order_id,
            market,
            TradeAssetType::Certificate,
            missing_cert_id,
            asset_id,
        );

        // verify_settle_prerequisites 失败时 emit SettlementRejected，不能用 assert_noop!
        assert_eq!(
            DataAssets::settle_certificate_trade_internal(
                &asset_id,
                &missing_cert_id,
                &market,
                &buyer,
                price,
                &order_id,
                &order_id,
            )
            .unwrap_err(),
            pallet_dataassets::Error::<Test>::CertificateNotFound.into(),
        );

        assert_eq!(trade_settled_event_count(), 0);
    });
}

#[test]
fn unauthorized_market_cannot_issue_certificate() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 1;
        let market: AccountId = 2;
        let buyer: AccountId = 3;
        let (asset_id, _) = register_test_asset(owner);

        assert_noop!(
            DataAssets::issue_certificate_by_market_internal(
                &asset_id, &market, &owner, &buyer, 1, None,
            ),
            pallet_dataassets::Error::<Test>::NotAuthorized,
        );

        let cert_id = certificate_id(&asset_id, owner, 0);
        assert!(DataAssets::get_certificate(&asset_id, &cert_id).is_none());
        assert!(trade_measurements().is_empty());
    });
}

#[test]
fn authorized_market_cannot_issue_certificate_for_non_owner_seller() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 1;
        let market: AccountId = 2;
        let buyer: AccountId = 3;
        let fake_seller: AccountId = 4;
        let (asset_id, _) = register_test_asset(owner);

        assert_ok!(DataAssets::authorize_market(
            RuntimeOrigin::signed(owner),
            asset_id,
            market,
        ));

        assert_noop!(
            DataAssets::issue_certificate_by_market_internal(
                &asset_id,
                &market,
                &fake_seller,
                &buyer,
                1,
                None,
            ),
            pallet_dataassets::Error::<Test>::NotOwner,
        );

        let cert_id = certificate_id(&asset_id, owner, 0);
        assert!(DataAssets::get_certificate(&asset_id, &cert_id).is_none());
        assert!(trade_measurements().is_empty());
    });
}

#[test]
fn transfer_certificate_updates_owner_nonce_and_registers_trade_measurement() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);

        assert_ok!(DataAssets::issue_certificate(
            RuntimeOrigin::signed(1),
            asset_id,
            2,
            1,
            None,
        ));

        let cert_id = certificate_id(&asset_id, 1, 0);
        let cert = DataAssets::get_certificate(&asset_id, &cert_id).unwrap();
        assert_eq!(cert.holder, 2);
        assert_eq!(cert.state_version, 1);

        assert_ok!(DataAssets::transfer_certificate(
            RuntimeOrigin::signed(2),
            asset_id,
            cert_id,
            3,
        ));

        let cert = DataAssets::get_certificate(&asset_id, &cert_id).unwrap();
        assert_eq!(cert.holder, 3);
        assert_eq!(cert.state_version, 2);
        assert_eq!(trade_measurements(), vec![asset_id, asset_id]);
    });
}

#[test]
fn transfer_certificate_rejects_non_owner_without_measurement() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);

        assert_ok!(DataAssets::issue_certificate(
            RuntimeOrigin::signed(1),
            asset_id,
            2,
            1,
            None,
        ));

        let cert_id = certificate_id(&asset_id, 1, 0);

        assert_noop!(
            DataAssets::transfer_certificate(RuntimeOrigin::signed(3), asset_id, cert_id, 4),
            pallet_dataassets::Error::<Test>::NotOwner,
        );

        let cert = DataAssets::get_certificate(&asset_id, &cert_id).unwrap();
        assert_eq!(cert.holder, 2);
        assert_eq!(cert.state_version, 1);
        assert_eq!(trade_measurements(), vec![asset_id]);
    });
}

#[test]
fn transfer_certificate_rejects_expired_certificate_without_state_change() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);

        assert_ok!(DataAssets::issue_certificate(
            RuntimeOrigin::signed(1),
            asset_id,
            2,
            1,
            Some(System::block_number() - 1),
        ));

        let cert_id = certificate_id(&asset_id, 1, 0);

        assert_noop!(
            DataAssets::transfer_certificate(RuntimeOrigin::signed(2), asset_id, cert_id, 3),
            pallet_dataassets::Error::<Test>::CertificateNotActive,
        );

        let cert = DataAssets::get_certificate(&asset_id, &cert_id).unwrap();
        assert_eq!(cert.holder, 2);
        assert_eq!(cert.state_version, 1);
        assert_eq!(trade_measurements(), vec![asset_id]);
    });
}

#[test]
fn transfer_certificate_rejects_revoked_certificate_without_measurement() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);

        assert_ok!(DataAssets::issue_certificate(
            RuntimeOrigin::signed(1),
            asset_id,
            2,
            1,
            None,
        ));

        let cert_id = certificate_id(&asset_id, 1, 0);
        assert_ok!(DataAssets::revoke_certificate(
            RuntimeOrigin::signed(1),
            asset_id,
            cert_id,
        ));

        assert_noop!(
            DataAssets::transfer_certificate(RuntimeOrigin::signed(2), asset_id, cert_id, 3),
            pallet_dataassets::Error::<Test>::CertificateNotFound,
        );
        assert!(DataAssets::get_certificate(&asset_id, &cert_id).is_none());
        assert_eq!(trade_measurements(), vec![asset_id]);
    });
}

fn register_mock_market(market_id: [u8; 32], active: bool, capabilities: u8) {
    MARKETS.with(|markets| {
        markets
            .borrow_mut()
            .insert(market_id, (active, capabilities, 0));
    });
    ACTIVE_ORDERS.with(|orders| {
        orders.borrow_mut().insert(market_id, (0, 8));
    });
    MARKET_PAYMENTS.with(|payments| {
        payments.borrow_mut().insert(
            market_id,
            MockMarketPayment {
                market_fee_bps: 0,
                recipient: 4,
                funded_orders: 0,
                max_funded_orders: 8,
                exposure: 0,
                max_exposure: 100_000,
            },
        );
    });
}

fn market_authorization_count(market_id: [u8; 32]) -> u32 {
    MARKETS.with(|markets| markets.borrow().get(&market_id).unwrap().2)
}

fn market_active_order_count(market_id: [u8; 32]) -> u32 {
    ACTIVE_ORDERS.with(|orders| orders.borrow().get(&market_id).unwrap().0)
}

fn market_funding_state(market_id: [u8; 32]) -> (u32, u128) {
    MARKET_PAYMENTS.with(|payments| {
        let payment = payments.borrow().get(&market_id).copied().unwrap();
        (payment.funded_orders, payment.exposure)
    })
}

fn price_policy() -> PricePolicy {
    PricePolicy {
        reference_price: 101,
        floor_ratio_bps: 5_000,
        signed_market_fee_cap_bps: 300,
        safety_fee_bps: 0,
    }
}

fn price_policy_witness(order: &OrderCommitment) -> protocol::PricePolicyWitness {
    match order.operation {
        MarketOperation::TransferRight => {
            protocol::PricePolicyWitness::SellerSnapshot(price_policy())
        }
        MarketOperation::TransferMeta | MarketOperation::IssueRight => {
            protocol::PricePolicyWitness::Authorization
        }
    }
}

fn create_runtime_order(
    origin: RuntimeOrigin,
    order: OrderCommitment,
    seller_commitment: SellerCommitment,
    seller_signature: [u8; 64],
) -> DispatchResult {
    let witness = price_policy_witness(&order);
    DataAssets::create_order(origin, order, witness, seller_commitment, seller_signature)
}

fn issue_authorization_terms(expiry: u64, nonce: u64) -> IssueRightAuthorizationTerms<u64> {
    IssueRightAuthorizationTerms {
        right_type: RightType::Train,
        data_scope: DataScope::FieldSetHash(H256::repeat_byte(0x5a)),
        validity_template: RightValidity {
            valid_from: System::block_number(),
            valid_until: Some(100),
            max_uses: Some(5),
        },
        max_issue_count: 2,
        max_total_use_units: 10,
        price_policy: price_policy(),
        royalty_bps: 250,
        transferable: true,
        allowed_transfer_markets: BoundedVec::try_from(vec![[7u8; 32], [8u8; 32]])
            .expect("bounded markets"),
        expiry,
        nonce,
    }
}

fn transfer_authorization_terms(
    expiry: u64,
    nonce: u64,
) -> TransferMetaAuthorizationTerms<AccountId, u64> {
    TransferMetaAuthorizationTerms {
        price_policy: price_policy(),
        allowed_buyer: Some(2),
        expiry,
        nonce,
    }
}

#[test]
fn authorization_creation_binds_asset_market_and_operation() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let (other_asset_id, _) = register_test_asset(2);
        let market_id = [7; 32];
        let other_market_id = [8; 32];
        register_mock_market(market_id, true, (1 << 0) | (1 << 1));
        register_mock_market(other_market_id, true, (1 << 0) | (1 << 1));

        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            issue_authorization_terms(10, 0),
        ));
        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id)
            .expect("active issue authorization");
        let authorization =
            DataAssets::validate_issue_right_authorization(&auth_id, &asset_id, &market_id)
                .expect("valid issue authorization");
        assert_eq!(authorization.owner, 1);
        assert_eq!(authorization.operation, MarketOperation::IssueRight);
        assert_eq!(authorization.status, AuthorizationStatus::Active);

        assert_eq!(
            DataAssets::validate_issue_right_authorization(&auth_id, &other_asset_id, &market_id,),
            Err(pallet_dataassets::Error::<Test>::AuthorizationMissing.into())
        );
        assert_eq!(
            DataAssets::validate_issue_right_authorization(&auth_id, &asset_id, &other_market_id,),
            Err(pallet_dataassets::Error::<Test>::AuthorizationMissing.into())
        );
        assert_eq!(
            DataAssets::validate_transfer_meta_authorization(&auth_id, &asset_id, &market_id,),
            Err(pallet_dataassets::Error::<Test>::AuthorizationMissing.into())
        );
    });
}

#[test]
fn authorization_creation_rejects_unregistered_restricted_and_incapable_markets() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let missing_market = [10; 32];
        assert_noop!(
            DataAssets::create_transfer_meta_authorization(
                RuntimeOrigin::signed(1),
                asset_id,
                missing_market,
                transfer_authorization_terms(10, 0),
            ),
            pallet_dataassets::Error::<Test>::MarketNotRegistered
        );

        let restricted_market = [11; 32];
        register_mock_market(restricted_market, false, 1 << 0);
        assert_noop!(
            DataAssets::create_transfer_meta_authorization(
                RuntimeOrigin::signed(1),
                asset_id,
                restricted_market,
                transfer_authorization_terms(10, 0),
            ),
            pallet_dataassets::Error::<Test>::MarketRestricted
        );

        let incapable_market = [12; 32];
        register_mock_market(incapable_market, true, 1 << 0);
        assert_noop!(
            DataAssets::create_issue_right_authorization(
                RuntimeOrigin::signed(1),
                asset_id,
                incapable_market,
                issue_authorization_terms(10, 0),
            ),
            pallet_dataassets::Error::<Test>::CapabilityDenied
        );
    });
}

#[test]
fn missing_and_revoked_authorizations_are_rejected() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [13; 32];
        register_mock_market(market_id, true, 1 << 0);

        assert_eq!(
            DataAssets::validate_transfer_meta_authorization(&[99; 32], &asset_id, &market_id,),
            Err(pallet_dataassets::Error::<Test>::AuthorizationMissing.into())
        );

        assert_ok!(DataAssets::create_transfer_meta_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            transfer_authorization_terms(10, 0),
        ));
        let auth_id = DataAssets::active_transfer_meta_authorization(asset_id, market_id)
            .expect("active transfer authorization");
        assert_eq!(market_authorization_count(market_id), 1);
        assert_ok!(DataAssets::revoke_transfer_meta_authorization(
            RuntimeOrigin::signed(1),
            auth_id,
        ));
        assert_eq!(
            DataAssets::transfer_meta_authorizations(auth_id)
                .unwrap()
                .status,
            AuthorizationStatus::Revoked
        );
        assert_eq!(market_authorization_count(market_id), 0);
        assert_eq!(
            DataAssets::validate_transfer_meta_authorization(&auth_id, &asset_id, &market_id,),
            Err(pallet_dataassets::Error::<Test>::AuthorizationRevoked.into())
        );
    });
}

#[test]
fn expired_authorization_is_rejected_and_can_be_closed() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [14; 32];
        register_mock_market(market_id, true, 1 << 1);
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            issue_authorization_terms(2, 0),
        ));
        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id)
            .expect("active issue authorization");

        System::set_block_number(3);
        assert_eq!(
            DataAssets::validate_issue_right_authorization(&auth_id, &asset_id, &market_id),
            Err(pallet_dataassets::Error::<Test>::AuthorizationExpired.into())
        );
        assert_ok!(DataAssets::expire_issue_right_authorization(
            RuntimeOrigin::signed(4),
            auth_id,
        ));
        assert_eq!(
            DataAssets::issue_right_authorizations(auth_id)
                .unwrap()
                .status,
            AuthorizationStatus::Expired
        );
        assert_eq!(market_authorization_count(market_id), 0);
    });
}

#[test]
fn authorization_terms_are_snapshotted_and_nonce_advances_only_on_success() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [15; 32];
        register_mock_market(market_id, true, 1 << 1);

        let mut terms = issue_authorization_terms(10, 0);
        let expected_policy = terms.price_policy;
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            terms.clone(),
        ));
        terms.price_policy.reference_price = 999;

        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id).unwrap();
        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!(authorization.price_policy, expected_policy);
        assert_eq!(authorization.right_type, RightType::Train);
        assert_eq!(authorization.validity_template.max_uses, Some(5));
        assert_eq!(authorization.max_issue_count, 2);
        assert_eq!(authorization.max_total_use_units, 10);
        assert_eq!(authorization.nonce, 0);
        assert_eq!(
            DataAssets::authorization_nonce(1, (asset_id, MarketOperation::IssueRight)),
            1
        );

        assert_ok!(DataAssets::revoke_issue_right_authorization(
            RuntimeOrigin::signed(1),
            auth_id,
        ));
        assert_noop!(
            DataAssets::create_issue_right_authorization(
                RuntimeOrigin::signed(1),
                asset_id,
                market_id,
                issue_authorization_terms(10, 0),
            ),
            pallet_dataassets::Error::<Test>::AuthorizationNonceMismatch
        );
        assert_eq!(
            DataAssets::authorization_nonce(1, (asset_id, MarketOperation::IssueRight)),
            1
        );
        assert_eq!(market_authorization_count(market_id), 0);
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            issue_authorization_terms(10, 1),
        ));
    });
}

#[test]
fn invalid_policy_and_quota_do_not_consume_authorization_nonce() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [16; 32];
        register_mock_market(market_id, true, 1 << 1);

        let mut invalid_policy = issue_authorization_terms(10, 0);
        invalid_policy.price_policy.floor_ratio_bps = 0;
        assert_noop!(
            DataAssets::create_issue_right_authorization(
                RuntimeOrigin::signed(1),
                asset_id,
                market_id,
                invalid_policy,
            ),
            pallet_dataassets::Error::<Test>::InvalidPricePolicy
        );
        let mut invalid_quota = issue_authorization_terms(10, 0);
        invalid_quota.max_total_use_units = 4;
        assert_noop!(
            DataAssets::create_issue_right_authorization(
                RuntimeOrigin::signed(1),
                asset_id,
                market_id,
                invalid_quota,
            ),
            pallet_dataassets::Error::<Test>::InvalidIssueQuota
        );
        assert_eq!(
            DataAssets::authorization_nonce(1, (asset_id, MarketOperation::IssueRight)),
            0
        );
        assert_eq!(market_authorization_count(market_id), 0);
        assert!(DataAssets::active_issue_right_authorization(asset_id, market_id).is_none());
    });
}

#[test]
fn open_validation_checks_snapshot_without_reserving_quota() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [17; 32];
        register_mock_market(market_id, true, 1 << 1);
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            issue_authorization_terms(10, 0),
        ));
        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id).unwrap();

        assert_eq!(
            DataAssets::validate_issue_right_open(&auth_id, &asset_id, &market_id, 50, 300),
            Err(pallet_dataassets::Error::<Test>::BelowPriceFloor.into())
        );
        assert_eq!(
            DataAssets::validate_issue_right_open(&auth_id, &asset_id, &market_id, 51, 301),
            Err(pallet_dataassets::Error::<Test>::FeeOutOfBounds.into())
        );
        assert!(
            DataAssets::validate_issue_right_open(&auth_id, &asset_id, &market_id, 51, 300).is_ok()
        );
        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!(authorization.reserved_count, 0);
        assert_eq!(authorization.reserved_use_units, 0);
        assert_eq!(authorization.issued_count, 0);
        assert_eq!(authorization.issued_use_units, 0);
        assert!(DataAssets::issue_quota_reservations([1u8; 32]).is_none());
    });
}

#[test]
fn transfer_meta_policy_buyer_and_nonce_are_bound() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [18; 32];
        register_mock_market(market_id, true, 1 << 0);
        assert_ok!(DataAssets::create_transfer_meta_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            transfer_authorization_terms(10, 0),
        ));
        let auth_id = DataAssets::active_transfer_meta_authorization(asset_id, market_id).unwrap();
        let authorization = DataAssets::transfer_meta_authorizations(auth_id).unwrap();
        assert_eq!(authorization.price_policy, price_policy());
        assert_eq!(authorization.allowed_buyer, Some(2));
        assert_eq!(authorization.nonce, 0);
        assert_eq!(
            DataAssets::authorization_nonce(1, (asset_id, MarketOperation::TransferMeta)),
            1
        );
        assert!(DataAssets::validate_transfer_meta_open(
            &auth_id,
            &asset_id,
            &market_id,
            Some(&2),
            51,
            300,
        )
        .is_ok());
        assert_eq!(
            DataAssets::validate_transfer_meta_open(
                &auth_id,
                &asset_id,
                &market_id,
                Some(&3),
                51,
                300,
            ),
            Err(pallet_dataassets::Error::<Test>::AuthorizationPolicyMismatch.into())
        );
    });
}

#[test]
fn quota_reservation_enforces_count_and_use_limits_without_partial_writes() {
    new_test_ext().execute_with(|| {
        let market_id = [19; 32];
        register_mock_market(market_id, true, 1 << 1);

        let (count_asset_id, _) = register_test_asset(1);
        let mut count_terms = issue_authorization_terms(10, 0);
        count_terms.max_issue_count = 1;
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            count_asset_id,
            market_id,
            count_terms,
        ));
        let count_auth_id =
            DataAssets::active_issue_right_authorization(count_asset_id, market_id).unwrap();
        assert_ok!(DataAssets::reserve_issue_quota(
            &count_auth_id,
            &count_asset_id,
            &market_id,
            [21u8; 32],
        ));
        let before = DataAssets::issue_right_authorizations(count_auth_id).unwrap();
        assert_noop!(
            DataAssets::reserve_issue_quota(
                &count_auth_id,
                &count_asset_id,
                &market_id,
                [22u8; 32],
            ),
            pallet_dataassets::Error::<Test>::QuotaExceeded
        );
        assert_eq!(
            DataAssets::issue_right_authorizations(count_auth_id).unwrap(),
            before
        );
        assert!(DataAssets::issue_quota_reservations([22u8; 32]).is_none());

        let (unit_asset_id, _) = register_test_asset(2);
        let mut unit_terms = issue_authorization_terms(10, 0);
        unit_terms.max_issue_count = 2;
        unit_terms.max_total_use_units = 5;
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(2),
            unit_asset_id,
            market_id,
            unit_terms,
        ));
        let unit_auth_id =
            DataAssets::active_issue_right_authorization(unit_asset_id, market_id).unwrap();
        assert_ok!(DataAssets::reserve_issue_quota(
            &unit_auth_id,
            &unit_asset_id,
            &market_id,
            [23u8; 32],
        ));
        let before = DataAssets::issue_right_authorizations(unit_auth_id).unwrap();
        assert_noop!(
            DataAssets::reserve_issue_quota(&unit_auth_id, &unit_asset_id, &market_id, [24u8; 32],),
            pallet_dataassets::Error::<Test>::QuotaExceeded
        );
        assert_eq!(
            DataAssets::issue_right_authorizations(unit_auth_id).unwrap(),
            before
        );
        assert!(DataAssets::issue_quota_reservations([24u8; 32]).is_none());
    });
}

#[test]
fn reserved_quota_can_settle_after_revoke_but_cannot_be_reused() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [20; 32];
        let meta_market_id = [24; 32];
        let order_id = [31u8; 32];
        register_mock_market(market_id, true, 1 << 1);
        register_mock_market(meta_market_id, true, 1 << 0);
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            issue_authorization_terms(10, 0),
        ));
        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id).unwrap();
        assert_ok!(DataAssets::reserve_issue_quota(
            &auth_id, &asset_id, &market_id, order_id,
        ));
        assert_ok!(DataAssets::revoke_issue_right_authorization(
            RuntimeOrigin::signed(1),
            auth_id,
        ));
        assert_eq!(DataAssets::outstanding_issue_quota_count(asset_id), 1);
        assert_noop!(
            DataAssets::reserve_issue_quota(&auth_id, &asset_id, &market_id, [32u8; 32]),
            pallet_dataassets::Error::<Test>::AuthorizationRevoked
        );
        assert_noop!(
            DataAssets::create_transfer_meta_authorization(
                RuntimeOrigin::signed(1),
                asset_id,
                meta_market_id,
                transfer_authorization_terms(10, 0),
            ),
            pallet_dataassets::Error::<Test>::MutualExclusionConflict
        );
        assert_eq!(
            DataAssets::authorization_nonce(1, (asset_id, MarketOperation::TransferMeta)),
            0
        );

        assert_ok!(DataAssets::convert_issue_quota_to_issued(
            &auth_id, &order_id,
        ));
        assert_eq!(DataAssets::outstanding_issue_quota_count(asset_id), 0);
        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!(authorization.reserved_count, 0);
        assert_eq!(authorization.reserved_use_units, 0);
        assert_eq!(authorization.issued_count, 1);
        assert_eq!(authorization.issued_use_units, 5);
        assert_eq!(
            DataAssets::issue_quota_reservations(order_id)
                .unwrap()
                .status,
            IssueQuotaReservationStatus::Issued
        );
        let before = authorization;
        assert_noop!(
            DataAssets::convert_issue_quota_to_issued(&auth_id, &order_id),
            pallet_dataassets::Error::<Test>::QuotaReservationStateInvalid
        );
        assert_noop!(
            DataAssets::release_issue_quota(&auth_id, &order_id),
            pallet_dataassets::Error::<Test>::QuotaReservationStateInvalid
        );
        assert_eq!(
            DataAssets::issue_right_authorizations(auth_id).unwrap(),
            before
        );
    });
}

#[test]
fn reserved_quota_can_be_released_after_expiry_exactly_once() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [21; 32];
        let order_id = [41u8; 32];
        register_mock_market(market_id, true, 1 << 1);
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            issue_authorization_terms(2, 0),
        ));
        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id).unwrap();
        assert_ok!(DataAssets::reserve_issue_quota(
            &auth_id, &asset_id, &market_id, order_id,
        ));
        assert_eq!(DataAssets::outstanding_issue_quota_count(asset_id), 1);
        System::set_block_number(3);
        assert_noop!(
            DataAssets::reserve_issue_quota(&auth_id, &asset_id, &market_id, [42u8; 32]),
            pallet_dataassets::Error::<Test>::AuthorizationExpired
        );
        assert_ok!(DataAssets::release_issue_quota(&auth_id, &order_id));
        assert_eq!(DataAssets::outstanding_issue_quota_count(asset_id), 0);
        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!(authorization.reserved_count, 0);
        assert_eq!(authorization.reserved_use_units, 0);
        assert_eq!(authorization.issued_count, 0);
        assert_eq!(
            DataAssets::issue_quota_reservations(order_id)
                .unwrap()
                .status,
            IssueQuotaReservationStatus::Released
        );
        assert_noop!(
            DataAssets::release_issue_quota(&auth_id, &order_id),
            pallet_dataassets::Error::<Test>::QuotaReservationStateInvalid
        );
    });
}

#[test]
fn issue_authorization_blocks_meta_authorization_and_owner_transfer() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let issue_market_id = [22; 32];
        let meta_market_id = [23; 32];
        register_mock_market(issue_market_id, true, 1 << 1);
        register_mock_market(meta_market_id, true, 1 << 0);
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            issue_market_id,
            issue_authorization_terms(10, 0),
        ));
        let auth_id =
            DataAssets::active_issue_right_authorization(asset_id, issue_market_id).unwrap();
        assert_noop!(
            DataAssets::create_transfer_meta_authorization(
                RuntimeOrigin::signed(1),
                asset_id,
                meta_market_id,
                transfer_authorization_terms(10, 0),
            ),
            pallet_dataassets::Error::<Test>::MutualExclusionConflict
        );
        assert_noop!(
            DataAssets::transfer_asset(RuntimeOrigin::signed(1), asset_id, 2),
            pallet_dataassets::Error::<Test>::MutualExclusionConflict
        );
        assert_eq!(
            DataAssets::authorization_nonce(1, (asset_id, MarketOperation::TransferMeta)),
            0
        );
        assert_ok!(DataAssets::revoke_issue_right_authorization(
            RuntimeOrigin::signed(1),
            auth_id,
        ));
        assert_ok!(DataAssets::create_transfer_meta_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            meta_market_id,
            transfer_authorization_terms(10, 0),
        ));
    });
}

#[test]
fn meta_owner_index_and_state_version_follow_authoritative_owner() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let asset = DataAssets::get_asset(&asset_id).expect("registered asset");
        assert_eq!(asset.core.status, AssetStatus::Tradable);
        assert_eq!(asset.core.state_version, 1);
        assert!(DataAssets::is_asset_indexed_for_owner(&1, &asset_id));
        assert!(!DataAssets::is_asset_indexed_for_owner(&2, &asset_id));

        assert_ok!(DataAssets::transfer_asset(
            RuntimeOrigin::signed(1),
            asset_id,
            2,
        ));

        let asset = DataAssets::get_asset(&asset_id).expect("transferred asset");
        assert_eq!(asset.core.owner, 2);
        assert_eq!(asset.core.state_version, 2);
        assert!(!DataAssets::is_asset_indexed_for_owner(&1, &asset_id));
        assert!(DataAssets::is_asset_indexed_for_owner(&2, &asset_id));
    });
}

#[test]
fn structured_right_fields_and_reverse_indexes_are_authoritative() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let right_id = [31u8; 32];
        let right = structured_right(asset_id, right_id, 2, 1, Some(100));

        assert_ok!(DataAssets::create_right_object(right));
        let stored = DataAssets::get_certificate(&asset_id, &right_id).expect("stored right");
        assert_eq!(stored.right_type, RightType::Train);
        assert_eq!(
            stored.data_scope,
            DataScope::FieldSetHash(H256::repeat_byte(0x5a))
        );
        assert_eq!(stored.validity.max_uses, Some(5));
        assert_eq!(stored.remaining_uses, Some(5));
        assert_eq!(stored.royalty_bps, 250);
        assert_eq!(stored.state_version, 1);
        assert_eq!(stored.status, CertificateStatus::Active);
        assert!(stored.allows_transfer_on(&[7u8; 32]));
        assert!(!stored.allows_transfer_on(&[9u8; 32]));
        assert!(DataAssets::is_right_indexed_for_asset(&asset_id, &right_id));
        assert!(DataAssets::is_right_indexed_for_holder(&2, &right_id));
    });
}

#[test]
fn right_holder_transfer_updates_version_and_reverse_index_together() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        assert_ok!(DataAssets::issue_certificate(
            RuntimeOrigin::signed(1),
            asset_id,
            2,
            1,
            None,
        ));
        let right_id = certificate_id(&asset_id, 1, 0);
        assert!(DataAssets::is_right_indexed_for_holder(&2, &right_id));

        assert_ok!(DataAssets::transfer_certificate(
            RuntimeOrigin::signed(2),
            asset_id,
            right_id,
            3,
        ));

        let right = DataAssets::get_certificate(&asset_id, &right_id).expect("transferred right");
        assert_eq!(right.holder, 3);
        assert_eq!(right.state_version, 2);
        assert!(!DataAssets::is_right_indexed_for_holder(&2, &right_id));
        assert!(DataAssets::is_right_indexed_for_holder(&3, &right_id));
        assert!(DataAssets::is_right_indexed_for_asset(&asset_id, &right_id));
    });
}

#[test]
fn valid_right_blocks_meta_transfer_while_expired_right_does_not() {
    new_test_ext().execute_with(|| {
        let (blocked_asset, _) = register_test_asset(1);
        let valid_right = structured_right(blocked_asset, [41u8; 32], 2, 1, Some(100));
        assert_ok!(DataAssets::create_right_object(valid_right));
        assert_noop!(
            DataAssets::transfer_asset(RuntimeOrigin::signed(1), blocked_asset, 3),
            pallet_dataassets::Error::<Test>::MutualExclusionConflict
        );
        assert_noop!(
            DataAssets::reserve_meta_order(&blocked_asset, [1u8; 32]),
            pallet_dataassets::Error::<Test>::MutualExclusionConflict
        );

        let (transferable_asset, _) = register_test_asset(4);
        let expired_right = structured_right(transferable_asset, [42u8; 32], 5, 4, Some(43));
        assert_ok!(DataAssets::create_right_object(expired_right));
        System::set_block_number(44);
        assert_ok!(DataAssets::transfer_asset(
            RuntimeOrigin::signed(4),
            transferable_asset,
            6,
        ));
        assert_eq!(
            DataAssets::get_asset(&transferable_asset)
                .unwrap()
                .core
                .owner,
            6
        );
    });
}

#[test]
fn right_validity_uses_block_height_not_timestamp() {
    new_test_ext().execute_with(|| {
        Timestamp::set_timestamp(10_000);
        System::set_block_number(10);
        let (asset_id, _) = register_test_asset(1);
        let right_id = [43u8; 32];
        let right = structured_right(asset_id, right_id, 2, 1, Some(11));
        assert_ok!(DataAssets::create_right_object(right));

        let first_order = [44u8; 32];
        assert_ok!(DataAssets::reserve_right_order(
            &asset_id,
            &right_id,
            first_order,
        ));
        assert_ok!(DataAssets::release_right_order(&right_id, &first_order));

        System::set_block_number(12);
        assert_noop!(
            DataAssets::reserve_right_order(&asset_id, &right_id, [45u8; 32]),
            pallet_dataassets::Error::<Test>::CertificateNotActive
        );
    });
}

#[test]
fn terminal_or_exhausted_rights_are_not_valid() {
    new_test_ext().execute_with(|| {
        let mut right = structured_right([43u8; 32], [44u8; 32], 2, 1, Some(100));
        assert!(right.is_valid(42));

        right.status = CertificateStatus::Expired;
        assert!(!right.is_valid(42));
        right.status = CertificateStatus::Exhausted;
        assert!(!right.is_valid(42));
        right.status = CertificateStatus::Revoked;
        assert!(!right.is_valid(42));

        right.status = CertificateStatus::Active;
        right.remaining_uses = Some(0);
        assert!(!right.is_valid(42));
    });
}

#[test]
fn active_object_order_locks_are_unique_and_owner_checked_on_release() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let first_order = [51u8; 32];
        let second_order = [52u8; 32];
        assert_ok!(DataAssets::reserve_meta_order(&asset_id, first_order));
        assert_noop!(
            DataAssets::reserve_meta_order(&asset_id, second_order),
            pallet_dataassets::Error::<Test>::ActiveObjectOrderExists
        );
        assert_noop!(
            DataAssets::release_meta_order(&asset_id, &second_order),
            pallet_dataassets::Error::<Test>::ActiveObjectOrderMismatch
        );
        assert_ok!(DataAssets::release_meta_order(&asset_id, &first_order));
        assert_ok!(DataAssets::reserve_meta_order(&asset_id, second_order));

        let (right_asset, _) = register_test_asset(2);
        assert_ok!(DataAssets::issue_certificate(
            RuntimeOrigin::signed(2),
            right_asset,
            3,
            1,
            None,
        ));
        let right_id = certificate_id(&right_asset, 2, 0);
        assert_ok!(DataAssets::reserve_right_order(
            &right_asset,
            &right_id,
            first_order,
        ));
        assert_noop!(
            DataAssets::reserve_right_order(&right_asset, &right_id, second_order),
            pallet_dataassets::Error::<Test>::ActiveObjectOrderExists
        );
        assert_noop!(
            DataAssets::transfer_certificate(RuntimeOrigin::signed(3), right_asset, right_id, 4,),
            pallet_dataassets::Error::<Test>::ActiveObjectOrderExists
        );
        assert_ok!(DataAssets::release_right_order(&right_id, &first_order));
        assert_ok!(DataAssets::transfer_certificate(
            RuntimeOrigin::signed(3),
            right_asset,
            right_id,
            4,
        ));
    });
}

#[test]
fn invalid_structured_right_parameters_are_rejected_without_index_writes() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let right_id = [61u8; 32];
        let mut right = structured_right(asset_id, right_id, 2, 1, Some(100));
        right.royalty_bps = 10_001;

        assert_noop!(
            DataAssets::create_right_object(right),
            pallet_dataassets::Error::<Test>::InvalidRightParameters
        );
        assert!(DataAssets::get_certificate(&asset_id, &right_id).is_none());
        assert!(!DataAssets::is_right_indexed_for_asset(
            &asset_id, &right_id
        ));
        assert!(!DataAssets::is_right_indexed_for_holder(&2, &right_id));
    });
}

#[test]
fn legacy_scale_objects_decode_without_changing_v1_meaning() {
    let current_asset = DataAsset::minimal(
        7u64,
        b"legacy".to_vec(),
        b"legacy asset".to_vec(),
        H256::repeat_byte(0x22),
        10,
    );
    let legacy_asset: LegacyDataAsset<AccountId> = LegacyDataAsset {
        version: current_asset.version.clone(),
        core: LegacyAssetCore {
            asset_id: [71u8; 32],
            token_id: 4,
            owner: 7,
            raw_data_hash: current_asset.core.raw_data_hash,
            timestamp: 10,
            nonce: 3,
            status: LegacyAssetStatus::Private,
            updated_at: 12,
        },
        metadata: current_asset.metadata,
        characteristics: current_asset.characteristics,
        statistics: current_asset.statistics,
        encryption_info: current_asset.encryption_info,
        signature: current_asset.signature,
        pricing_config: current_asset.pricing_config,
    };
    let legacy_asset_bytes = legacy_asset.encode();
    let mut direct_input = &legacy_asset_bytes[..];
    LegacyDataAsset::<AccountId>::decode(&mut direct_input)
        .expect("legacy layout decodes directly");
    assert!(direct_input.is_empty());
    let decoded_asset =
        decode_data_asset_compat::<AccountId>(&legacy_asset_bytes).expect("legacy asset decodes");
    assert_eq!(decoded_asset.core.owner, 7);
    assert_eq!(decoded_asset.core.state_version, 4);
    assert_eq!(decoded_asset.core.status, AssetStatus::Private);

    let legacy_right: LegacyRightToken<AccountId> = LegacyRightToken {
        version: BoundedVec::truncate_from(b"1.0".to_vec()),
        certificate_id: [72u8; 32],
        token_id: 8,
        right_type: LegacyRightType::Usage,
        create_time: 10,
        valid_from: 10,
        valid_until: Some(20),
        owner: 9,
        issuer: 7,
        nonce: 2,
        parent_asset_id: [71u8; 32],
        status: LegacyCertificateStatus::Active,
        signature: BoundedVec::default(),
    };
    assert_eq!(
        LegacyAssetStatus::Private.encode(),
        AssetStatus::Private.encode()
    );
    assert_eq!(
        LegacyRightType::Usage.encode(),
        RightType::LegacyUsage.encode()
    );
    assert_eq!(
        LegacyCertificateStatus::Active.encode(),
        CertificateStatus::Active.encode()
    );
    let decoded_right = decode_right_token_compat::<AccountId>(&legacy_right.encode())
        .expect("legacy right decodes");
    assert_eq!(decoded_right.right_id, [72u8; 32]);
    assert_eq!(decoded_right.holder, 9);
    assert_eq!(decoded_right.right_type, RightType::LegacyUsage);
    assert_eq!(decoded_right.data_scope, DataScope::FullAsset);
    assert_eq!(decoded_right.state_version, 3);
    assert!(decoded_right.transferable);
}

#[test]
fn protocol_enum_discriminants_and_commitment_vectors_are_stable() {
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
    assert_eq!(MessageType::Order.encode(), vec![0]);
    assert_eq!(MessageType::Seller.encode(), vec![1]);
    assert_eq!(MessageType::Buyer.encode(), vec![2]);
    assert_eq!(MessageType::Settlement.encode(), vec![3]);
    assert_eq!(MessageType::Right.encode(), vec![4]);

    let seller_pair = sr25519::Pair::from_seed(&[1u8; 32]);
    let buyer_pair = sr25519::Pair::from_seed(&[2u8; 32]);
    let genesis_hash = [0x11; 32];
    let order = protocol_order(protocol_public(&seller_pair), protocol_public(&buyer_pair));
    let seller = protocol_seller_commitment(genesis_hash, &order);
    let buyer = protocol_buyer_commitment(genesis_hash, &order, protocol_public(&buyer_pair));
    let domain = protocol::domain_separator(
        genesis_hash,
        MarketOperation::TransferRight,
        MessageType::Order,
    );
    let order_id = protocol::order_id(genesis_hash, &order);
    let settlement_id =
        protocol::settlement_id(genesis_hash, MarketOperation::TransferRight, order_id);
    let right_id = protocol::right_id(genesis_hash, MarketOperation::TransferRight, order_id);
    let seller_payload_hash = sp_io::hashing::blake2_256(&protocol::seller_signing_payload(
        genesis_hash,
        order.operation,
        &seller,
    ));
    let buyer_payload_hash = sp_io::hashing::blake2_256(&protocol::buyer_signing_payload(
        genesis_hash,
        order.operation,
        &buyer,
    ));

    let encoded_order = order.encode();
    assert_eq!(encoded_order.len(), 300);
    let mut encoded_order_input = &encoded_order[..];
    assert_eq!(
        OrderCommitment::decode(&mut encoded_order_input).expect("canonical order decodes"),
        order
    );
    assert!(encoded_order_input.is_empty());
    assert_eq!(
        domain,
        [
            56, 105, 2, 197, 237, 214, 9, 134, 17, 121, 209, 171, 108, 102, 73, 98, 45, 116, 222,
            59, 121, 147, 150, 86, 128, 154, 210, 11, 3, 209, 21, 183,
        ]
    );
    assert_eq!(
        order_id,
        [
            161, 130, 196, 148, 188, 125, 122, 135, 146, 167, 21, 9, 76, 42, 67, 206, 89, 72, 104,
            198, 98, 204, 140, 119, 142, 14, 246, 19, 109, 139, 128, 98,
        ]
    );
    assert_eq!(
        settlement_id,
        [
            88, 102, 136, 77, 201, 218, 68, 130, 134, 158, 211, 71, 168, 95, 229, 45, 90, 53, 128,
            14, 234, 27, 214, 101, 172, 225, 107, 94, 185, 255, 254, 97,
        ]
    );
    assert_eq!(
        right_id,
        [
            29, 101, 126, 249, 102, 125, 136, 211, 171, 151, 195, 49, 251, 158, 125, 2, 223, 247,
            246, 193, 42, 111, 1, 201, 170, 126, 132, 18, 10, 174, 120, 11,
        ]
    );
    assert_eq!(
        seller_payload_hash,
        [
            242, 34, 80, 154, 1, 8, 163, 171, 88, 224, 181, 20, 246, 194, 130, 204, 132, 62, 54,
            97, 172, 155, 161, 150, 2, 71, 63, 164, 194, 160, 239, 191,
        ]
    );
    assert_eq!(
        buyer_payload_hash,
        [
            140, 162, 115, 109, 86, 183, 166, 149, 108, 18, 249, 147, 248, 126, 227, 8, 146, 165,
            49, 246, 87, 236, 58, 227, 170, 68, 29, 45, 122, 127, 107, 9,
        ]
    );
}

#[test]
fn every_frozen_order_field_substitution_changes_the_runtime_order_id() {
    new_test_ext().execute_with(|| {
        let seller_pair = sr25519::Pair::from_seed(&[1u8; 32]);
        let buyer_pair = sr25519::Pair::from_seed(&[2u8; 32]);
        let genesis_hash = [0x11; 32];
        let order = protocol_order(protocol_public(&seller_pair), protocol_public(&buyer_pair));
        let commitment = protocol_seller_commitment(genesis_hash, &order);
        let signature = protocol_signature(seller_pair.sign(&protocol::seller_signing_payload(
            genesis_hash,
            order.operation,
            &commitment,
        )));
        let original_id = protocol::order_id(genesis_hash, &order);

        let mut substitutions = Vec::new();
        let mut changed = order.clone();
        changed.market_id[0] ^= 1;
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.object_id[0] ^= 1;
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.object_type = ObjectType::MetaToken;
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.parent_asset_id = Some([0x42; 32]);
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.authorization_id[0] ^= 1;
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.seller = protocol_public(&buyer_pair);
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.allowed_buyer = None;
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.price += 1;
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.policy_hash[0] ^= 1;
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.right_parameters_hash = Some([0x72; 32]);
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.seller_state_version += 1;
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.order_nonce += 1;
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.expiry += 1;
        substitutions.push(changed);
        let mut changed = order.clone();
        changed.operation = MarketOperation::IssueRight;
        changed.object_type = ObjectType::MetaToken;
        changed.parent_asset_id = None;
        substitutions.push(changed);

        for changed in substitutions {
            assert_ne!(protocol::order_id(genesis_hash, &changed), original_id);
            assert_eq!(
                protocol::verify_seller_commitment(
                    genesis_hash,
                    &changed,
                    &commitment,
                    &signature,
                    0,
                    order.seller_state_version,
                    1,
                ),
                Err(ProtocolError::MalformedCommitment)
            );
        }
    });
}

#[test]
fn wrong_signatures_versions_domains_and_state_versions_are_rejected() {
    new_test_ext().execute_with(|| {
        let seller_pair = sr25519::Pair::from_seed(&[1u8; 32]);
        let buyer_pair = sr25519::Pair::from_seed(&[2u8; 32]);
        let genesis_hash = [0x11; 32];
        let wrong_genesis_hash = [0x12; 32];
        let order = protocol_order(protocol_public(&seller_pair), protocol_public(&buyer_pair));
        let seller = protocol_seller_commitment(genesis_hash, &order);
        let buyer = protocol_buyer_commitment(genesis_hash, &order, protocol_public(&buyer_pair));
        let wrong_seller_signature = protocol_signature(buyer_pair.sign(
            &protocol::seller_signing_payload(genesis_hash, order.operation, &seller),
        ));
        assert_eq!(
            protocol::verify_seller_commitment(
                genesis_hash,
                &order,
                &seller,
                &wrong_seller_signature,
                0,
                order.seller_state_version,
                1,
            ),
            Err(ProtocolError::InvalidSignature)
        );

        let wrong_domain_signature = protocol_signature(seller_pair.sign(
            &protocol::seller_signing_payload(wrong_genesis_hash, order.operation, &seller),
        ));
        assert_eq!(
            protocol::verify_seller_commitment(
                genesis_hash,
                &order,
                &seller,
                &wrong_domain_signature,
                0,
                order.seller_state_version,
                1,
            ),
            Err(ProtocolError::InvalidSignature)
        );

        let wrong_buyer_signature = protocol_signature(seller_pair.sign(
            &protocol::buyer_signing_payload(genesis_hash, order.operation, &buyer),
        ));
        assert_eq!(
            protocol::verify_buyer_commitment(
                genesis_hash,
                &order,
                &buyer,
                &wrong_buyer_signature,
                0,
                1,
            ),
            Err(ProtocolError::InvalidSignature)
        );

        let valid_signature = protocol_signature(seller_pair.sign(
            &protocol::seller_signing_payload(genesis_hash, order.operation, &seller),
        ));
        assert_eq!(
            protocol::verify_seller_commitment(
                genesis_hash,
                &order,
                &seller,
                &valid_signature,
                0,
                order.seller_state_version + 1,
                1,
            ),
            Err(ProtocolError::StateVersionMismatch)
        );

        let mut unsupported = order.clone();
        unsupported.settlement_protocol_version += 1;
        assert_eq!(
            protocol::validate_order_commitment(genesis_hash, &unsupported, 1),
            Err(ProtocolError::UnsupportedProtocolVersion)
        );
        assert_eq!(
            protocol::validate_order_commitment(genesis_hash, &order, order.expiry + 1),
            Err(ProtocolError::OrderExpired)
        );

        assert_ne!(
            protocol::domain_separator(
                genesis_hash,
                MarketOperation::TransferRight,
                MessageType::Seller,
            ),
            protocol::domain_separator(
                genesis_hash,
                MarketOperation::TransferRight,
                MessageType::Buyer,
            )
        );
        assert_ne!(
            protocol::domain_separator(
                genesis_hash,
                MarketOperation::TransferRight,
                MessageType::Seller,
            ),
            protocol::domain_separator(
                genesis_hash,
                MarketOperation::IssueRight,
                MessageType::Seller,
            )
        );
        assert_ne!(
            protocol::domain_separator(
                genesis_hash,
                MarketOperation::TransferRight,
                MessageType::Seller,
            ),
            protocol::domain_separator(
                wrong_genesis_hash,
                MarketOperation::TransferRight,
                MessageType::Seller,
            )
        );
    });
}

#[test]
fn seller_and_buyer_nonces_are_consumed_once_only_after_valid_proofs() {
    new_test_ext().execute_with(|| {
        let seller_pair = sr25519::Pair::from_seed(&[1u8; 32]);
        let buyer_pair = sr25519::Pair::from_seed(&[2u8; 32]);
        let genesis_hash = [0x11; 32];
        let order = protocol_order(protocol_public(&seller_pair), protocol_public(&buyer_pair));
        let seller = protocol_seller_commitment(genesis_hash, &order);
        let buyer = protocol_buyer_commitment(genesis_hash, &order, protocol_public(&buyer_pair));
        let seller_signature = protocol_signature(seller_pair.sign(
            &protocol::seller_signing_payload(genesis_hash, order.operation, &seller),
        ));
        let buyer_signature = protocol_signature(buyer_pair.sign(
            &protocol::buyer_signing_payload(genesis_hash, order.operation, &buyer),
        ));

        assert_eq!(
            DataAssets::validate_seller_protocol_commitment(
                genesis_hash,
                &order,
                &seller,
                &seller_signature,
                order.seller_state_version,
                1,
            )
            .unwrap(),
            seller.order_id
        );
        assert_eq!(DataAssets::seller_nonce(seller.seller, seller.object_id), 0);
        assert_ok!(DataAssets::consume_seller_nonce(
            &seller.seller,
            &seller.object_id,
            seller.seller_nonce,
        ));
        assert_eq!(DataAssets::seller_nonce(seller.seller, seller.object_id), 1);
        assert_eq!(
            DataAssets::validate_seller_protocol_commitment(
                genesis_hash,
                &order,
                &seller,
                &seller_signature,
                order.seller_state_version,
                1,
            )
            .unwrap_err(),
            pallet_dataassets::Error::<Test>::NonceUsed.into()
        );
        assert_noop!(
            DataAssets::consume_seller_nonce(
                &seller.seller,
                &seller.object_id,
                seller.seller_nonce,
            ),
            pallet_dataassets::Error::<Test>::NonceUsed
        );
        assert_eq!(DataAssets::seller_nonce(seller.seller, seller.object_id), 1);

        assert_eq!(
            DataAssets::validate_buyer_protocol_commitment(
                genesis_hash,
                &order,
                &buyer,
                &buyer_signature,
                1,
            )
            .unwrap(),
            buyer.order_id
        );
        assert_ok!(DataAssets::consume_buyer_nonce(
            &buyer.buyer,
            buyer.buyer_nonce
        ));
        assert_eq!(DataAssets::buyer_nonce(buyer.buyer), 1);
        assert_eq!(
            DataAssets::validate_buyer_protocol_commitment(
                genesis_hash,
                &order,
                &buyer,
                &buyer_signature,
                1,
            )
            .unwrap_err(),
            pallet_dataassets::Error::<Test>::NonceUsed.into()
        );

        let unused_buyer = [0x91; 32];
        let mut malformed_buyer = buyer.clone();
        malformed_buyer.buyer = unused_buyer;
        assert!(DataAssets::validate_buyer_protocol_commitment(
            genesis_hash,
            &order,
            &malformed_buyer,
            &buyer_signature,
            1,
        )
        .is_err());
        assert_eq!(DataAssets::buyer_nonce(unused_buyer), 0);

        crate::pallet::SellerNonce::<Test>::insert(seller.seller, seller.object_id, u64::MAX);
        assert_noop!(
            DataAssets::consume_seller_nonce(&seller.seller, &seller.object_id, u64::MAX),
            pallet_dataassets::Error::<Test>::ProtocolNonceOverflow
        );
        assert_eq!(
            DataAssets::seller_nonce(seller.seller, seller.object_id),
            u64::MAX
        );
    });
}

fn runtime_genesis_hash() -> [u8; 32] {
    System::block_hash(0).0
}

fn test_protocol_account(account: AccountId) -> [u8; 32] {
    TestProtocolAccountIdConverter::to_protocol_account_id(&account)
        .expect("test account has a protocol identity")
}

fn signed_seller_input(
    order: &OrderCommitment,
    seller_account: AccountId,
) -> (SellerCommitment, [u8; 64], [u8; 32]) {
    let pair = sr25519::Pair::from_seed(&[seller_account as u8; 32]);
    let commitment = protocol_seller_commitment(runtime_genesis_hash(), order);
    let signature = protocol_signature(pair.sign(&protocol::seller_signing_payload(
        runtime_genesis_hash(),
        order.operation,
        &commitment,
    )));
    let order_id = protocol::order_id(runtime_genesis_hash(), order);
    (commitment, signature, order_id)
}

fn signed_buyer_input(
    order: &OrderCommitment,
    buyer_account: AccountId,
    nonce: u64,
) -> (BuyerCommitment, [u8; 64]) {
    let pair = sr25519::Pair::from_seed(&[buyer_account as u8; 32]);
    let commitment = BuyerCommitment {
        order_id: protocol::order_id(runtime_genesis_hash(), order),
        buyer: test_protocol_account(buyer_account),
        price: order.price,
        currency: order.currency,
        expires_at: order.expiry,
        buyer_nonce: nonce,
    };
    let signature = protocol_signature(pair.sign(&protocol::buyer_signing_payload(
        runtime_genesis_hash(),
        order.operation,
        &commitment,
    )));
    (commitment, signature)
}

fn transfer_meta_order(
    asset_id: [u8; 32],
    market_id: [u8; 32],
    auth_id: [u8; 32],
    nonce: u64,
    expiry: u32,
) -> OrderCommitment {
    let asset = DataAssets::get_asset(&asset_id).expect("registered asset");
    OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id,
        operation: MarketOperation::TransferMeta,
        object_type: ObjectType::MetaToken,
        object_id: asset_id,
        parent_asset_id: None,
        authorization_id: auth_id,
        seller: test_protocol_account(asset.core.owner),
        allowed_buyer: Some(test_protocol_account(2)),
        price: 101,
        currency: CurrencyId::Native,
        policy_hash: DataAssets::price_policy_commitment_hash(&price_policy()),
        right_parameters_hash: None,
        seller_state_version: asset.core.state_version,
        order_nonce: nonce,
        expiry,
    }
}

fn create_transfer_meta_fixture(expiry: u32) -> ([u8; 32], [u8; 32], [u8; 32], OrderCommitment) {
    let (asset_id, _) = register_test_asset(1);
    let market_id = [0x21; 32];
    register_mock_market(market_id, true, 1 << 0);
    assert_ok!(DataAssets::create_transfer_meta_authorization(
        RuntimeOrigin::signed(1),
        asset_id,
        market_id,
        transfer_authorization_terms(u64::from(expiry), 0),
    ));
    let auth_id = DataAssets::active_transfer_meta_authorization(asset_id, market_id)
        .expect("active transfer authorization");
    let order = transfer_meta_order(asset_id, market_id, auth_id, 0, expiry);
    (asset_id, market_id, auth_id, order)
}

fn fund_transfer_meta_fixture(
    expiry: u32,
    market_fee_bps: u16,
) -> ([u8; 32], [u8; 32], [u8; 32], [u8; 32]) {
    let (asset_id, market_id, auth_id, order) = create_transfer_meta_fixture(expiry);
    MARKET_PAYMENTS.with(|payments| {
        payments
            .borrow_mut()
            .get_mut(&market_id)
            .unwrap()
            .market_fee_bps = market_fee_bps;
    });
    let (seller, seller_signature, order_id) = signed_seller_input(&order, 1);
    assert_ok!(create_runtime_order(
        RuntimeOrigin::signed(3),
        order.clone(),
        seller,
        seller_signature,
    ));
    let (buyer, buyer_signature) = signed_buyer_input(&order, 2, 0);
    assert_ok!(DataAssets::fund_order(
        RuntimeOrigin::signed(2),
        order_id,
        buyer,
        buyer_signature,
    ));
    (asset_id, market_id, auth_id, order_id)
}

fn issue_right_order(
    asset_id: [u8; 32],
    market_id: [u8; 32],
    auth_id: [u8; 32],
    authorization: &crate::types::IssueRightAuthorization<AccountId, u64>,
    nonce: u64,
    expiry: u32,
) -> OrderCommitment {
    let asset = DataAssets::get_asset(&asset_id).expect("registered asset");
    OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id,
        operation: MarketOperation::IssueRight,
        object_type: ObjectType::MetaToken,
        object_id: asset_id,
        parent_asset_id: None,
        authorization_id: auth_id,
        seller: test_protocol_account(asset.core.owner),
        allowed_buyer: None,
        price: 101,
        currency: CurrencyId::Native,
        policy_hash: DataAssets::price_policy_commitment_hash(&authorization.price_policy),
        right_parameters_hash: Some(DataAssets::issue_right_parameters_commitment_hash(
            authorization,
        )),
        seller_state_version: asset.core.state_version,
        order_nonce: nonce,
        expiry,
    }
}

fn create_issue_right_fixture(expiry: u32) -> ([u8; 32], [u8; 32], [u8; 32], OrderCommitment) {
    let (asset_id, _) = register_test_asset(1);
    let market_id = [0x83; 32];
    register_mock_market(market_id, true, 1 << 1);
    assert_ok!(DataAssets::create_issue_right_authorization(
        RuntimeOrigin::signed(1),
        asset_id,
        market_id,
        issue_authorization_terms(u64::from(expiry), 0),
    ));
    let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id)
        .expect("active issue authorization");
    let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
    let order = issue_right_order(asset_id, market_id, auth_id, &authorization, 0, expiry);
    (asset_id, market_id, auth_id, order)
}

fn fund_issue_right_fixture(
    expiry: u32,
    market_fee_bps: u16,
) -> ([u8; 32], [u8; 32], [u8; 32], [u8; 32], [u8; 32]) {
    let (asset_id, market_id, auth_id, order) = create_issue_right_fixture(expiry);
    MARKET_PAYMENTS.with(|payments| {
        payments
            .borrow_mut()
            .get_mut(&market_id)
            .unwrap()
            .market_fee_bps = market_fee_bps;
    });
    let (seller, seller_signature, order_id) = signed_seller_input(&order, 1);
    assert_ok!(create_runtime_order(
        RuntimeOrigin::signed(3),
        order.clone(),
        seller,
        seller_signature,
    ));
    let (buyer, buyer_signature) = signed_buyer_input(&order, 2, 0);
    assert_ok!(DataAssets::fund_order(
        RuntimeOrigin::signed(2),
        order_id,
        buyer,
        buyer_signature,
    ));
    let right_id = protocol::right_id(
        runtime_genesis_hash(),
        MarketOperation::IssueRight,
        order_id,
    );
    (asset_id, market_id, auth_id, order_id, right_id)
}

fn create_transfer_right_fixture(
    expiry: u32,
    market_fee_bps: u16,
) -> ([u8; 32], [u8; 32], [u8; 32], [u8; 32], OrderCommitment) {
    let (asset_id, _) = register_test_asset(1);
    let market_id = [7; 32];
    let right_id = [0x86; 32];
    register_mock_market(market_id, true, 1 << 2);
    assert_ok!(DataAssets::create_right_object(structured_right(
        asset_id,
        right_id,
        2,
        1,
        Some(100),
    )));
    MARKET_PAYMENTS.with(|payments| {
        payments
            .borrow_mut()
            .get_mut(&market_id)
            .unwrap()
            .market_fee_bps = market_fee_bps;
    });
    let right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
    let order = OrderCommitment {
        settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
        market_id,
        operation: MarketOperation::TransferRight,
        object_type: ObjectType::RightToken,
        object_id: right_id,
        parent_asset_id: Some(asset_id),
        authorization_id: right_id,
        seller: test_protocol_account(2),
        allowed_buyer: Some(test_protocol_account(3)),
        price: 101,
        currency: CurrencyId::Native,
        policy_hash: DataAssets::price_policy_commitment_hash(&price_policy()),
        right_parameters_hash: Some(DataAssets::right_parameters_commitment_hash(&right)),
        seller_state_version: right.state_version,
        order_nonce: 0,
        expiry,
    };
    let (seller, seller_signature, order_id) = signed_seller_input(&order, 2);
    let _ = (seller, seller_signature);
    (asset_id, market_id, right_id, order_id, order)
}

fn fund_transfer_right_fixture(
    expiry: u32,
    market_fee_bps: u16,
) -> ([u8; 32], [u8; 32], [u8; 32], [u8; 32], OrderCommitment) {
    let (asset_id, market_id, right_id, order_id, order) =
        create_transfer_right_fixture(expiry, market_fee_bps);
    let (seller, seller_signature, _) = signed_seller_input(&order, 2);
    assert_ok!(create_runtime_order(
        RuntimeOrigin::signed(4),
        order.clone(),
        seller,
        seller_signature,
    ));
    let (buyer, buyer_signature) = signed_buyer_input(&order, 3, 0);
    assert_ok!(DataAssets::fund_order(
        RuntimeOrigin::signed(3),
        order_id,
        buyer,
        buyer_signature,
    ));
    (asset_id, market_id, right_id, order_id, order)
}

fn assert_frozen_record_field_order(record: &protocol::SettlementRecord) {
    let mut expected = Vec::new();
    record.settlement_id.encode_to(&mut expected);
    record.order_id.encode_to(&mut expected);
    record.operation.encode_to(&mut expected);
    record.asset_id.encode_to(&mut expected);
    record.right_id.encode_to(&mut expected);
    record.authorization_id.encode_to(&mut expected);
    record.market_id.encode_to(&mut expected);
    record.policy_hash.encode_to(&mut expected);
    record.order_commitment_hash.encode_to(&mut expected);
    record.seller.encode_to(&mut expected);
    record.buyer.encode_to(&mut expected);
    record.currency.encode_to(&mut expected);
    record.gross_price.encode_to(&mut expected);
    record.seller_amount.encode_to(&mut expected);
    record.royalty_recipient.encode_to(&mut expected);
    record.royalty_amount.encode_to(&mut expected);
    record.market_fee_recipient.encode_to(&mut expected);
    record.market_fee_amount.encode_to(&mut expected);
    record.safety_fee_amount.encode_to(&mut expected);
    record.pre_state_version.encode_to(&mut expected);
    record.post_state_version.encode_to(&mut expected);
    record.pre_state_commitment.encode_to(&mut expected);
    record.block_number.encode_to(&mut expected);
    record.settlement_protocol_version.encode_to(&mut expected);
    assert_eq!(record.encode(), expected);
}

fn assert_authoritative_settlement(
    order_id: [u8; 32],
    operation: MarketOperation,
    asset_id: [u8; 32],
    right_id: Option<[u8; 32]>,
    authorization_id: [u8; 32],
    pre_state_version: u64,
    post_state_version: u64,
    pre_state_commitment: [u8; 32],
) {
    let order = DataAssets::query_order(&order_id).expect("settled order query");
    let payment = DataAssets::query_payment(&order_id).expect("released payment query");
    let record = DataAssets::query_settlement(&order_id).expect("authoritative settlement");
    let market = DataAssets::query_market(&order.order_commitment.market_id)
        .expect("authoritative market query");

    assert_eq!(
        DataAssets::settlement_record(order_id),
        Some(record.clone())
    );
    assert_eq!(record.order_id, order_id);
    assert_eq!(record.operation, operation);
    assert_eq!(record.asset_id, asset_id);
    assert_eq!(record.right_id, right_id);
    assert_eq!(record.authorization_id, authorization_id);
    assert_eq!(record.market_id, order.order_commitment.market_id);
    assert_eq!(record.policy_hash, order.order_commitment.policy_hash);
    assert_eq!(
        record.order_commitment_hash,
        protocol::order_commitment_hash(&order.order_commitment)
    );
    assert_eq!(record.seller, order.order_commitment.seller);
    assert_eq!(record.buyer, order.buyer_commitment.as_ref().unwrap().buyer);
    assert_eq!(record.currency, order.payment_asset);
    assert_eq!(record.gross_price, payment.amounts.gross_price);
    assert_eq!(record.seller_amount, payment.amounts.seller_net);
    assert_eq!(record.royalty_recipient, payment.royalty_recipient);
    assert_eq!(record.royalty_amount, payment.amounts.royalty);
    assert_eq!(record.market_fee_recipient, payment.market_fee_recipient);
    assert_eq!(record.market_fee_amount, payment.amounts.market_fee);
    assert_eq!(record.safety_fee_amount, payment.amounts.safety_fee);
    assert_eq!(record.pre_state_version, pre_state_version);
    assert_eq!(record.post_state_version, post_state_version);
    assert_eq!(record.pre_state_commitment, pre_state_commitment);
    assert_eq!(record.block_number, System::block_number() as u32);
    assert_eq!(
        record.settlement_protocol_version,
        SETTLEMENT_PROTOCOL_VERSION
    );
    assert_eq!(
        record.settlement_id,
        protocol::settlement_id(runtime_genesis_hash(), operation, order_id)
    );
    assert_eq!(
        record.gross_price,
        record.seller_amount
            + record.royalty_amount
            + record.market_fee_amount
            + record.safety_fee_amount
    );
    assert_eq!(market.active_orders, 0);
    assert_eq!(market.funded_orders, 0);
    assert_eq!(market.active_exposure_value, 0);
    assert_frozen_record_field_order(&record);

    let post_state_commitment = DataAssets::normalized_settlement_commitment(&order_id)
        .expect("normalized post commitment");
    assert_eq!(
        settlement_finalized_events(),
        vec![(record.settlement_id, order_id, post_state_commitment)]
    );
}

#[test]
fn create_order_writes_one_authoritative_open_order_without_payment_or_quota() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order) = create_transfer_meta_fixture(100);
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        let balances_before = Balances::total_issuance();
        let free_balances_before = (1..=4).map(Balances::free_balance).collect::<Vec<_>>();

        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order.clone(),
            seller.clone(),
            signature,
        ));

        let stored = DataAssets::runtime_order(order_id).expect("authoritative order");
        assert_eq!(stored.order_id, order_id);
        assert_eq!(stored.order_commitment, order);
        assert_eq!(stored.authorization_id, auth_id);
        assert_eq!(stored.seller_commitment, seller);
        assert_eq!(stored.state, OrderState::Open);
        assert_eq!(stored.payment_asset, CurrencyId::Native);
        assert_eq!(
            DataAssets::query_order_price_policy(&order_id),
            Some(price_policy())
        );
        assert_eq!(stored.escrow_id, None);
        assert_eq!(stored.buyer_commitment, None);
        assert_eq!(stored.funded_at, None);
        assert_eq!(stored.grace_deadline, None);
        assert_eq!(DataAssets::active_meta_order(asset_id), Some(order_id));
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(
            DataAssets::seller_nonce(test_protocol_account(1), asset_id),
            1
        );
        assert_eq!(Balances::total_issuance(), balances_before);
        assert_eq!(
            (1..=4).map(Balances::free_balance).collect::<Vec<_>>(),
            free_balances_before
        );
        assert_eq!(
            crate::pallet::IssueQuotaReservations::<Test>::iter().count(),
            0
        );
        assert!(DataAssets::market_orders(order_id).is_none());
        assert!(DataAssets::order_settlements(order_id).is_none());
        assert!(DataAssets::query_settlement(&order_id).is_none());
        assert!(settlement_finalized_events().is_empty());

        assert_noop!(
            create_runtime_order(RuntimeOrigin::signed(4), order, seller, signature),
            pallet_dataassets::Error::<Test>::NonceUsed
        );
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(
            DataAssets::seller_nonce(test_protocol_account(1), asset_id),
            1
        );
    });
}

#[test]
fn create_order_conflict_and_capacity_failure_leave_all_prestate_unchanged() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, _, first_order) = create_transfer_meta_fixture(100);
        let (first_seller, first_signature, first_id) = signed_seller_input(&first_order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            first_order,
            first_seller,
            first_signature,
        ));

        let auth_id = DataAssets::active_transfer_meta_authorization(asset_id, market_id).unwrap();
        let conflicting = transfer_meta_order(asset_id, market_id, auth_id, 1, 100);
        let (seller, signature, conflicting_id) = signed_seller_input(&conflicting, 1);
        assert_noop!(
            create_runtime_order(RuntimeOrigin::signed(3), conflicting, seller, signature),
            pallet_dataassets::Error::<Test>::ActiveObjectOrderExists
        );
        assert!(DataAssets::runtime_order(conflicting_id).is_none());
        assert_eq!(DataAssets::active_meta_order(asset_id), Some(first_id));
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(
            DataAssets::seller_nonce(test_protocol_account(1), asset_id),
            1
        );

        assert_ok!(DataAssets::cancel_open(RuntimeOrigin::signed(1), first_id));
        ACTIVE_ORDERS.with(|orders| {
            orders.borrow_mut().insert(market_id, (0, 0));
        });
        let next = transfer_meta_order(asset_id, market_id, auth_id, 1, 100);
        let (seller, signature, next_id) = signed_seller_input(&next, 1);
        assert_noop!(
            create_runtime_order(RuntimeOrigin::signed(3), next, seller, signature),
            pallet_dataassets::Error::<Test>::MarketActiveOrderCapacityExceeded
        );
        assert!(DataAssets::runtime_order(next_id).is_none());
        assert_eq!(DataAssets::active_meta_order(asset_id), None);
        assert_eq!(market_active_order_count(market_id), 0);
        assert!(DataAssets::query_settlement(&first_id).is_none());
        assert!(DataAssets::query_settlement(&conflicting_id).is_none());
        assert!(DataAssets::query_settlement(&next_id).is_none());
        assert!(settlement_finalized_events().is_empty());
        assert_eq!(
            DataAssets::seller_nonce(test_protocol_account(1), asset_id),
            1
        );
    });
}

#[test]
fn current_owner_can_cancel_open_without_market_consent_and_terminal_is_irreversible() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, _, order) = create_transfer_meta_fixture(100);
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order,
            seller,
            signature,
        ));

        assert_noop!(
            DataAssets::cancel_open(RuntimeOrigin::signed(2), order_id),
            pallet_dataassets::Error::<Test>::NotOwner
        );
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Open
        );
        assert_eq!(DataAssets::active_meta_order(asset_id), Some(order_id));
        assert_eq!(market_active_order_count(market_id), 1);

        MARKETS.with(|markets| markets.borrow_mut().get_mut(&market_id).unwrap().0 = false);
        assert_ok!(DataAssets::cancel_open(RuntimeOrigin::signed(1), order_id));
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Cancelled
        );
        assert_eq!(DataAssets::active_meta_order(asset_id), None);
        assert_eq!(market_active_order_count(market_id), 0);
        assert!(DataAssets::query_settlement(&order_id).is_none());
        assert!(settlement_finalized_events().is_empty());
        assert_noop!(
            DataAssets::cancel_open(RuntimeOrigin::signed(1), order_id),
            pallet_dataassets::Error::<Test>::OrderStatusInvalid
        );
        System::set_block_number(101);
        assert_noop!(
            DataAssets::expire_open(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::OrderStatusInvalid
        );
    });
}

#[test]
fn any_signed_caller_can_expire_only_after_expiry() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, _, order) = create_transfer_meta_fixture(10);
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order,
            seller,
            signature,
        ));

        System::set_block_number(10);
        assert_noop!(
            DataAssets::expire_open(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::OrderNotExpired
        );
        assert_eq!(DataAssets::active_meta_order(asset_id), Some(order_id));
        assert_eq!(market_active_order_count(market_id), 1);

        System::set_block_number(11);
        assert_ok!(DataAssets::expire_open(RuntimeOrigin::signed(4), order_id));
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Expired
        );
        assert_eq!(DataAssets::active_meta_order(asset_id), None);
        assert_eq!(market_active_order_count(market_id), 0);
        assert_noop!(
            DataAssets::cancel_open(RuntimeOrigin::signed(1), order_id),
            pallet_dataassets::Error::<Test>::OrderStatusInvalid
        );
    });
}

#[test]
fn issue_right_open_orders_do_not_lock_meta_or_reserve_quota() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [7; 32];
        register_mock_market(market_id, true, 1 << 1);
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            issue_authorization_terms(100, 0),
        ));
        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id).unwrap();
        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();

        let make_order = |nonce| OrderCommitment {
            settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
            market_id,
            operation: MarketOperation::IssueRight,
            object_type: ObjectType::MetaToken,
            object_id: asset_id,
            parent_asset_id: None,
            authorization_id: auth_id,
            seller: test_protocol_account(1),
            allowed_buyer: None,
            price: 101,
            currency: CurrencyId::Native,
            policy_hash: DataAssets::price_policy_commitment_hash(&authorization.price_policy),
            right_parameters_hash: Some(DataAssets::issue_right_parameters_commitment_hash(
                &authorization,
            )),
            seller_state_version: 1,
            order_nonce: nonce,
            expiry: 100,
        };

        let first = make_order(0);
        let (first_seller, first_signature, first_id) = signed_seller_input(&first, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            first,
            first_seller,
            first_signature,
        ));
        let second = make_order(1);
        let (second_seller, second_signature, second_id) = signed_seller_input(&second, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(4),
            second,
            second_seller,
            second_signature,
        ));

        assert_eq!(
            DataAssets::runtime_order(first_id).unwrap().state,
            OrderState::Open
        );
        assert_eq!(
            DataAssets::runtime_order(second_id).unwrap().state,
            OrderState::Open
        );
        assert_eq!(DataAssets::active_meta_order(asset_id), None);
        assert_eq!(market_active_order_count(market_id), 2);
        assert_eq!(authorization.reserved_count, 0);
        assert_eq!(authorization.reserved_use_units, 0);
        assert_eq!(
            crate::pallet::IssueQuotaReservations::<Test>::iter().count(),
            0
        );

        assert_ok!(DataAssets::cancel_open(RuntimeOrigin::signed(1), first_id));
        assert_ok!(DataAssets::cancel_open(RuntimeOrigin::signed(1), second_id));
        assert_eq!(market_active_order_count(market_id), 0);
    });
}

#[test]
fn transfer_right_open_uses_holder_permission_and_unique_right_lock() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [7; 32];
        let right_id = [0x31; 32];
        register_mock_market(market_id, true, 1 << 2);
        assert_ok!(DataAssets::create_right_object(structured_right(
            asset_id,
            right_id,
            2,
            1,
            Some(100),
        )));
        let right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        let order = OrderCommitment {
            settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
            market_id,
            operation: MarketOperation::TransferRight,
            object_type: ObjectType::RightToken,
            object_id: right_id,
            parent_asset_id: Some(asset_id),
            authorization_id: right_id,
            seller: test_protocol_account(2),
            allowed_buyer: Some(test_protocol_account(3)),
            price: 101,
            currency: CurrencyId::Native,
            policy_hash: DataAssets::price_policy_commitment_hash(&price_policy()),
            right_parameters_hash: Some(DataAssets::right_parameters_commitment_hash(&right)),
            seller_state_version: right.state_version,
            order_nonce: 0,
            expiry: 100,
        };
        let (seller, signature, order_id) = signed_seller_input(&order, 2);

        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(4),
            order,
            seller,
            signature,
        ));
        assert_eq!(DataAssets::active_right_order(right_id), Some(order_id));
        assert_noop!(
            DataAssets::cancel_open(RuntimeOrigin::signed(1), order_id),
            pallet_dataassets::Error::<Test>::NotOwner
        );
        assert_ok!(DataAssets::cancel_open(RuntimeOrigin::signed(2), order_id));
        assert_eq!(DataAssets::active_right_order(right_id), None);
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id)
                .unwrap()
                .holder,
            2
        );
    });
}

#[test]
fn cancel_and_expire_reject_every_non_open_state_before_releasing_capacity() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, _, order) = create_transfer_meta_fixture(10);
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order,
            seller,
            signature,
        ));
        let original = DataAssets::runtime_order(order_id).unwrap();

        for state in [
            OrderState::Funded,
            OrderState::Settled,
            OrderState::Cancelled,
            OrderState::Expired,
            OrderState::Refunded,
        ] {
            let mut stored = original.clone();
            stored.state = state;
            crate::pallet::RuntimeOrders::<Test>::insert(order_id, stored);
            System::set_block_number(11);
            assert_noop!(
                DataAssets::cancel_open(RuntimeOrigin::signed(1), order_id),
                pallet_dataassets::Error::<Test>::OrderStatusInvalid
            );
            assert_noop!(
                DataAssets::expire_open(RuntimeOrigin::signed(4), order_id),
                pallet_dataassets::Error::<Test>::OrderStatusInvalid
            );
            assert_eq!(DataAssets::active_meta_order(asset_id), Some(order_id));
            assert_eq!(market_active_order_count(market_id), 1);
        }
    });
}

#[test]
fn deterministic_amount_split_uses_floor_and_assigns_remainder_to_seller() {
    let split = protocol::amount_split(101, 333, 250, 100).expect("valid fee split");
    assert_eq!(split.royalty, 3);
    assert_eq!(split.market_fee, 2);
    assert_eq!(split.safety_fee, 1);
    assert_eq!(split.seller_net, 95);
    assert_eq!(
        split.seller_net + split.royalty + split.market_fee + split.safety_fee,
        split.gross_price
    );
    assert_eq!(
        protocol::amount_split(101, 5_000, 5_000, 1),
        Err(ProtocolError::FeeOutOfBounds)
    );
}

#[test]
fn funding_reserves_only_buyer_balance_and_records_exact_escrow() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, _, order) = create_transfer_meta_fixture(100);
        MARKET_PAYMENTS.with(|payments| {
            payments
                .borrow_mut()
                .get_mut(&market_id)
                .unwrap()
                .market_fee_bps = 250;
        });
        let (seller, seller_signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order.clone(),
            seller,
            seller_signature,
        ));
        let (buyer, buyer_signature) = signed_buyer_input(&order, 2, 0);
        let buyer_free_before = Balances::free_balance(2);
        let buyer_reserved_before = Balances::reserved_balance(2);
        let relayer_free_before = Balances::free_balance(4);

        assert_ok!(DataAssets::fund_order(
            RuntimeOrigin::signed(2),
            order_id,
            buyer.clone(),
            buyer_signature,
        ));

        let funded = DataAssets::runtime_order(order_id).expect("funded order");
        assert_eq!(funded.state, OrderState::Funded);
        assert_eq!(funded.buyer_commitment, Some(buyer.clone()));
        assert_eq!(funded.funded_at, Some(1));
        assert_eq!(funded.grace_deadline, Some(11));
        let escrow_id = funded.escrow_id.expect("escrow id");
        let escrow = DataAssets::runtime_escrow(escrow_id).expect("runtime escrow");
        assert_eq!(escrow.state, PaymentState::Escrowed);
        assert_eq!(escrow.payer, buyer.buyer);
        assert_eq!(escrow.buyer, buyer.buyer);
        assert_eq!(escrow.amounts.gross_price, 101);
        assert_eq!(escrow.amounts.market_fee, 2);
        assert_eq!(escrow.amounts.safety_fee, 0);
        assert_eq!(escrow.amounts.royalty, 0);
        assert_eq!(escrow.amounts.seller_net, 99);
        assert_eq!(escrow.market_fee_recipient, test_protocol_account(4));
        assert_eq!(Balances::free_balance(2), buyer_free_before - 101);
        assert_eq!(Balances::reserved_balance(2), buyer_reserved_before + 101);
        assert_eq!(Balances::free_balance(4), relayer_free_before);
        assert_eq!(market_funding_state(market_id), (1, 101));
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(DataAssets::buyer_nonce(test_protocol_account(2)), 1);
        assert_eq!(DataAssets::active_meta_order(asset_id), Some(order_id));
        assert_eq!(
            crate::pallet::IssueQuotaReservations::<Test>::iter().count(),
            0
        );
    });
}

#[test]
fn right_resale_funding_snapshots_royalty_and_market_fee_without_transferring_right() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [7; 32];
        let right_id = [0x74; 32];
        register_mock_market(market_id, true, 1 << 2);
        MARKET_PAYMENTS.with(|payments| {
            payments
                .borrow_mut()
                .get_mut(&market_id)
                .unwrap()
                .market_fee_bps = 250;
        });
        assert_ok!(DataAssets::create_right_object(structured_right(
            asset_id,
            right_id,
            2,
            1,
            Some(100),
        )));
        let right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        let order = OrderCommitment {
            settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
            market_id,
            operation: MarketOperation::TransferRight,
            object_type: ObjectType::RightToken,
            object_id: right_id,
            parent_asset_id: Some(asset_id),
            authorization_id: right_id,
            seller: test_protocol_account(2),
            allowed_buyer: Some(test_protocol_account(3)),
            price: 101,
            currency: CurrencyId::Native,
            policy_hash: DataAssets::price_policy_commitment_hash(&price_policy()),
            right_parameters_hash: Some(DataAssets::right_parameters_commitment_hash(&right)),
            seller_state_version: right.state_version,
            order_nonce: 0,
            expiry: 100,
        };
        let (seller, signature, order_id) = signed_seller_input(&order, 2);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(4),
            order.clone(),
            seller,
            signature,
        ));
        let (buyer, buyer_signature) = signed_buyer_input(&order, 3, 0);
        assert_ok!(DataAssets::fund_order(
            RuntimeOrigin::signed(3),
            order_id,
            buyer,
            buyer_signature,
        ));

        let funded = DataAssets::runtime_order(order_id).unwrap();
        let escrow = DataAssets::runtime_escrow(funded.escrow_id.unwrap()).unwrap();
        assert_eq!(escrow.amounts.royalty, 2);
        assert_eq!(escrow.amounts.market_fee, 2);
        assert_eq!(escrow.amounts.safety_fee, 0);
        assert_eq!(escrow.amounts.seller_net, 97);
        assert_eq!(escrow.royalty_recipient, Some(test_protocol_account(1)));
        assert_eq!(escrow.market_fee_recipient, test_protocol_account(4));
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id)
                .unwrap()
                .holder,
            2
        );
        assert_eq!(Balances::reserved_balance(3), 101);
    });
}

#[test]
fn underfunded_buyer_cannot_fund_from_an_unrelated_balance() {
    new_test_ext().execute_with(|| {
        let (_, market_id, _, order) = create_transfer_meta_fixture(100);
        let (seller, seller_signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order.clone(),
            seller,
            seller_signature,
        ));
        assert_ok!(Balances::force_set_balance(RuntimeOrigin::root(), 2, 50));
        let (buyer, buyer_signature) = signed_buyer_input(&order, 2, 0);
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let buyer_before = (Balances::free_balance(2), Balances::reserved_balance(2));
        let relayer_before = (Balances::free_balance(4), Balances::reserved_balance(4));

        assert_noop!(
            DataAssets::fund_order(RuntimeOrigin::signed(2), order_id, buyer, buyer_signature,),
            pallet_dataassets::Error::<Test>::InsufficientBalance
        );
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(
            (Balances::free_balance(2), Balances::reserved_balance(2)),
            buyer_before
        );
        assert_eq!(
            (Balances::free_balance(4), Balances::reserved_balance(4)),
            relayer_before
        );
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert_eq!(DataAssets::buyer_nonce(test_protocol_account(2)), 0);
        assert_eq!(crate::pallet::RuntimeEscrows::<Test>::iter().count(), 0);
    });
}

#[test]
fn low_price_and_fee_cap_rejections_leave_open_payment_prestate_clean() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, _, order) = create_transfer_meta_fixture(100);
        let mut low_price = order.clone();
        low_price.price = 50;
        let (seller, signature, low_order_id) = signed_seller_input(&low_price, 1);
        assert_noop!(
            create_runtime_order(RuntimeOrigin::signed(3), low_price, seller, signature,),
            pallet_dataassets::Error::<Test>::BelowPriceFloor
        );
        assert!(DataAssets::runtime_order(low_order_id).is_none());
        assert_eq!(market_active_order_count(market_id), 0);

        MARKET_PAYMENTS.with(|payments| {
            payments
                .borrow_mut()
                .get_mut(&market_id)
                .unwrap()
                .market_fee_bps = 301;
        });
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        assert_noop!(
            create_runtime_order(RuntimeOrigin::signed(3), order, seller, signature),
            pallet_dataassets::Error::<Test>::FeeOutOfBounds
        );
        assert!(DataAssets::runtime_order(order_id).is_none());
        assert!(DataAssets::query_order_price_policy(&order_id).is_none());
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert_eq!(market_active_order_count(market_id), 0);
        assert_eq!(
            DataAssets::seller_nonce(test_protocol_account(1), asset_id),
            0
        );
        assert_eq!(DataAssets::buyer_nonce(test_protocol_account(2)), 0);
        assert_eq!(crate::pallet::RuntimeEscrows::<Test>::iter().count(), 0);
    });
}

#[test]
fn transfer_right_create_requires_signed_policy_and_rejects_fee_above_cap_cleanly() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, order_id, order) =
            create_transfer_right_fixture(100, 301);
        let right_before = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        let seller_nonce_before = DataAssets::seller_nonce(test_protocol_account(2), right_id);
        let event_count_before = System::events().len();
        let balances_before = (1..=4).map(Balances::free_balance).collect::<Vec<_>>();
        let (seller, signature, _) = signed_seller_input(&order, 2);

        assert_noop!(
            DataAssets::create_order(
                RuntimeOrigin::signed(4),
                order.clone(),
                protocol::PricePolicyWitness::Authorization,
                seller.clone(),
                signature,
            ),
            pallet_dataassets::Error::<Test>::AuthorizationPolicyMismatch
        );
        let mut mismatched = price_policy();
        mismatched.reference_price += 1;
        assert_noop!(
            DataAssets::create_order(
                RuntimeOrigin::signed(4),
                order.clone(),
                protocol::PricePolicyWitness::SellerSnapshot(mismatched),
                seller.clone(),
                signature,
            ),
            pallet_dataassets::Error::<Test>::AuthorizationPolicyMismatch
        );
        assert_noop!(
            DataAssets::create_order(
                RuntimeOrigin::signed(4),
                order,
                protocol::PricePolicyWitness::SellerSnapshot(price_policy()),
                seller,
                signature,
            ),
            pallet_dataassets::Error::<Test>::FeeOutOfBounds
        );

        assert!(DataAssets::runtime_order(order_id).is_none());
        assert!(DataAssets::query_order_price_policy(&order_id).is_none());
        assert_eq!(DataAssets::active_right_order(right_id), None);
        assert_eq!(market_active_order_count(market_id), 0);
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert_eq!(
            DataAssets::seller_nonce(test_protocol_account(2), right_id),
            seller_nonce_before
        );
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id),
            Some(right_before)
        );
        assert_eq!(System::events().len(), event_count_before);
        assert_eq!(
            (1..=4).map(Balances::free_balance).collect::<Vec<_>>(),
            balances_before
        );
        assert!(DataAssets::query_settlement(&order_id).is_none());
    });
}

#[test]
fn fund_revalidates_the_immutable_order_policy_after_market_fee_change() {
    new_test_ext().execute_with(|| {
        let (_asset_id, market_id, right_id, order_id, order) =
            create_transfer_right_fixture(100, 100);
        let (seller, signature, _) = signed_seller_input(&order, 2);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(4),
            order.clone(),
            seller,
            signature,
        ));
        assert_eq!(
            DataAssets::query_order_price_policy(&order_id),
            Some(price_policy())
        );
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let balance_before = (Balances::free_balance(3), Balances::reserved_balance(3));
        let event_count_before = System::events().len();
        MARKET_PAYMENTS.with(|payments| {
            payments
                .borrow_mut()
                .get_mut(&market_id)
                .unwrap()
                .market_fee_bps = 301;
        });
        let (buyer, buyer_signature) = signed_buyer_input(&order, 3, 0);

        assert_noop!(
            DataAssets::fund_order(
                RuntimeOrigin::signed(3),
                order_id,
                buyer.clone(),
                buyer_signature,
            ),
            pallet_dataassets::Error::<Test>::FeeOutOfBounds
        );
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(DataAssets::active_right_order(right_id), Some(order_id));
        assert_eq!(
            (Balances::free_balance(3), Balances::reserved_balance(3)),
            balance_before
        );
        assert_eq!(DataAssets::buyer_nonce(test_protocol_account(3)), 0);
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert_eq!(System::events().len(), event_count_before);

        MARKET_PAYMENTS.with(|payments| {
            payments
                .borrow_mut()
                .get_mut(&market_id)
                .unwrap()
                .market_fee_bps = 250;
        });
        let (_, buyer_signature) = signed_buyer_input(&order, 3, 0);
        assert_ok!(DataAssets::fund_order(
            RuntimeOrigin::signed(3),
            order_id,
            buyer,
            buyer_signature,
        ));
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Funded
        );
    });
}

#[test]
fn funded_capacity_failure_rolls_back_issue_quota_balance_and_nonce() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [0x71; 32];
        register_mock_market(market_id, true, 1 << 1);
        MARKET_PAYMENTS.with(|payments| {
            payments
                .borrow_mut()
                .get_mut(&market_id)
                .unwrap()
                .max_funded_orders = 0;
        });
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            issue_authorization_terms(100, 0),
        ));
        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id).unwrap();
        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        let order = OrderCommitment {
            settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
            market_id,
            operation: MarketOperation::IssueRight,
            object_type: ObjectType::MetaToken,
            object_id: asset_id,
            parent_asset_id: None,
            authorization_id: auth_id,
            seller: test_protocol_account(1),
            allowed_buyer: None,
            price: 101,
            currency: CurrencyId::Native,
            policy_hash: DataAssets::price_policy_commitment_hash(&authorization.price_policy),
            right_parameters_hash: Some(DataAssets::issue_right_parameters_commitment_hash(
                &authorization,
            )),
            seller_state_version: 1,
            order_nonce: 0,
            expiry: 100,
        };
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order.clone(),
            seller,
            signature,
        ));
        let (buyer, buyer_signature) = signed_buyer_input(&order, 2, 0);
        let balance_before = (Balances::free_balance(2), Balances::reserved_balance(2));

        assert_noop!(
            DataAssets::fund_order(RuntimeOrigin::signed(2), order_id, buyer, buyer_signature,),
            pallet_dataassets::Error::<Test>::MarketCapacityExceeded
        );
        let stored = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!((stored.reserved_count, stored.reserved_use_units), (0, 0));
        assert!(DataAssets::issue_quota_reservations(order_id).is_none());
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Open
        );
        assert_eq!(
            (Balances::free_balance(2), Balances::reserved_balance(2)),
            balance_before
        );
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert_eq!(DataAssets::buyer_nonce(test_protocol_account(2)), 0);
    });
}

#[test]
fn revoked_issue_authorization_rejects_funding_without_reserving_anything() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [0x73; 32];
        register_mock_market(market_id, true, 1 << 1);
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            issue_authorization_terms(100, 0),
        ));
        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id).unwrap();
        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        let order = OrderCommitment {
            settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
            market_id,
            operation: MarketOperation::IssueRight,
            object_type: ObjectType::MetaToken,
            object_id: asset_id,
            parent_asset_id: None,
            authorization_id: auth_id,
            seller: test_protocol_account(1),
            allowed_buyer: None,
            price: 101,
            currency: CurrencyId::Native,
            policy_hash: DataAssets::price_policy_commitment_hash(&authorization.price_policy),
            right_parameters_hash: Some(DataAssets::issue_right_parameters_commitment_hash(
                &authorization,
            )),
            seller_state_version: 1,
            order_nonce: 0,
            expiry: 100,
        };
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order.clone(),
            seller,
            signature,
        ));
        assert_ok!(DataAssets::revoke_issue_right_authorization(
            RuntimeOrigin::signed(1),
            auth_id,
        ));
        let (buyer, buyer_signature) = signed_buyer_input(&order, 2, 0);
        let balance_before = (Balances::free_balance(2), Balances::reserved_balance(2));

        assert_noop!(
            DataAssets::fund_order(RuntimeOrigin::signed(2), order_id, buyer, buyer_signature,),
            pallet_dataassets::Error::<Test>::AuthorizationRevoked
        );
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Open
        );
        let revoked = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!(revoked.status, AuthorizationStatus::Revoked);
        assert_eq!((revoked.reserved_count, revoked.reserved_use_units), (0, 0));
        assert!(DataAssets::issue_quota_reservations(order_id).is_none());
        assert_eq!(
            (Balances::free_balance(2), Balances::reserved_balance(2)),
            balance_before
        );
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert_eq!(DataAssets::buyer_nonce(test_protocol_account(2)), 0);
        assert_eq!(crate::pallet::RuntimeEscrows::<Test>::iter().count(), 0);
    });
}

#[test]
fn refund_after_grace_returns_full_issue_payment_only_to_recorded_buyer() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [0x72; 32];
        register_mock_market(market_id, true, 1 << 1);
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            issue_authorization_terms(100, 0),
        ));
        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id).unwrap();
        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        let order = OrderCommitment {
            settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
            market_id,
            operation: MarketOperation::IssueRight,
            object_type: ObjectType::MetaToken,
            object_id: asset_id,
            parent_asset_id: None,
            authorization_id: auth_id,
            seller: test_protocol_account(1),
            allowed_buyer: None,
            price: 101,
            currency: CurrencyId::Native,
            policy_hash: DataAssets::price_policy_commitment_hash(&authorization.price_policy),
            right_parameters_hash: Some(DataAssets::issue_right_parameters_commitment_hash(
                &authorization,
            )),
            seller_state_version: 1,
            order_nonce: 0,
            expiry: 100,
        };
        let (seller, signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order.clone(),
            seller,
            signature,
        ));
        let (buyer, buyer_signature) = signed_buyer_input(&order, 2, 0);
        let buyer_before_fund = (Balances::free_balance(2), Balances::reserved_balance(2));
        assert_ok!(DataAssets::fund_order(
            RuntimeOrigin::signed(2),
            order_id,
            buyer,
            buyer_signature,
        ));
        let funded = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = funded.escrow_id.unwrap();
        let reserved = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!(
            (reserved.reserved_count, reserved.reserved_use_units),
            (1, 5)
        );
        assert_eq!(market_funding_state(market_id), (1, 101));
        assert_eq!(market_active_order_count(market_id), 1);

        System::set_block_number(11);
        let early_order = DataAssets::runtime_order(order_id).unwrap();
        let early_escrow = DataAssets::runtime_escrow(escrow_id).unwrap();
        let early_balance = (Balances::free_balance(2), Balances::reserved_balance(2));
        let early_quota = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_noop!(
            DataAssets::refund_funded(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::GracePeriodActive
        );
        assert_eq!(DataAssets::runtime_order(order_id), Some(early_order));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(early_escrow));
        assert_eq!(
            DataAssets::issue_right_authorizations(auth_id),
            Some(early_quota)
        );
        assert_eq!(
            (Balances::free_balance(2), Balances::reserved_balance(2)),
            early_balance
        );
        assert_eq!(market_funding_state(market_id), (1, 101));

        let seller_before_refund = (Balances::free_balance(1), Balances::reserved_balance(1));
        let caller_before_refund = (Balances::free_balance(4), Balances::reserved_balance(4));
        System::set_block_number(12);
        assert_ok!(DataAssets::refund_funded(
            RuntimeOrigin::signed(4),
            order_id,
        ));
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Refunded
        );
        assert_eq!(
            DataAssets::runtime_escrow(escrow_id).unwrap().state,
            PaymentState::Refunded
        );
        assert_eq!(
            (Balances::free_balance(2), Balances::reserved_balance(2)),
            buyer_before_fund
        );
        assert_eq!(
            (Balances::free_balance(1), Balances::reserved_balance(1)),
            seller_before_refund
        );
        assert_eq!(
            (Balances::free_balance(4), Balances::reserved_balance(4)),
            caller_before_refund
        );
        let released = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!(
            (released.reserved_count, released.reserved_use_units),
            (0, 0)
        );
        assert_eq!(
            DataAssets::issue_quota_reservations(order_id)
                .unwrap()
                .status,
            IssueQuotaReservationStatus::Released
        );
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert_eq!(market_active_order_count(market_id), 0);
        let right_id = protocol::right_id(
            runtime_genesis_hash(),
            MarketOperation::IssueRight,
            order_id,
        );
        assert!(DataAssets::get_certificate(&asset_id, &right_id).is_none());
        assert!(DataAssets::query_settlement(&order_id).is_none());
        assert!(settlement_finalized_events().is_empty());
        assert_noop!(
            DataAssets::refund_funded(RuntimeOrigin::signed(3), order_id),
            pallet_dataassets::Error::<Test>::TerminalOrder
        );
        assert_eq!(market_funding_state(market_id), (0, 0));
    });
}

#[test]
fn nonzero_safety_fee_rejects_at_create_without_side_effects() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [0x81; 32];
        register_mock_market(market_id, true, 1 << 0);
        let mut terms = transfer_authorization_terms(100, 0);
        terms.price_policy.safety_fee_bps = 100;
        assert_ok!(DataAssets::create_transfer_meta_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            terms,
        ));
        let auth_id = DataAssets::active_transfer_meta_authorization(asset_id, market_id).unwrap();
        let authorization = DataAssets::transfer_meta_authorizations(auth_id).unwrap();
        let mut order = transfer_meta_order(asset_id, market_id, auth_id, 0, 100);
        order.policy_hash = DataAssets::price_policy_commitment_hash(&authorization.price_policy);
        let (seller, seller_signature, order_id) = signed_seller_input(&order, 1);
        let buyer_balance_before = (Balances::free_balance(2), Balances::reserved_balance(2));
        let event_count_before = System::events().len();

        assert_noop!(
            create_runtime_order(RuntimeOrigin::signed(3), order, seller, seller_signature,),
            pallet_dataassets::Error::<Test>::FeeOutOfBounds
        );
        assert!(DataAssets::runtime_order(order_id).is_none());
        assert_eq!(
            (Balances::free_balance(2), Balances::reserved_balance(2)),
            buyer_balance_before
        );
        assert_eq!(
            DataAssets::seller_nonce(test_protocol_account(1), asset_id),
            0
        );
        assert_eq!(DataAssets::buyer_nonce(test_protocol_account(2)), 0);
        assert_eq!(crate::pallet::RuntimeEscrows::<Test>::iter().count(), 0);
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert_eq!(market_active_order_count(market_id), 0);
        assert_eq!(market_authorization_count(market_id), 1);
        assert_eq!(DataAssets::active_meta_order(asset_id), None);
        assert_eq!(System::events().len(), event_count_before);
        assert_eq!(
            DataAssets::transfer_meta_authorizations(auth_id),
            Some(authorization)
        );
    });
}

#[test]
fn transfer_meta_settlement_atomically_releases_payment_and_consumes_authorization() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id) = fund_transfer_meta_fixture(100, 250);
        let escrow_id = DataAssets::runtime_order(order_id)
            .unwrap()
            .escrow_id
            .unwrap();
        let asset_before = DataAssets::get_asset(&asset_id).unwrap();
        let seller_free_before = Balances::free_balance(1);
        let market_free_before = Balances::free_balance(4);
        assert!(DataAssets::query_settlement(&order_id).is_none());
        let pre_state_commitment = DataAssets::normalized_settlement_commitment(&order_id).unwrap();

        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(3), order_id));

        let asset = DataAssets::get_asset(&asset_id).unwrap();
        assert_eq!(asset.core.owner, 2);
        assert_eq!(
            asset.core.state_version,
            asset_before.core.state_version + 1
        );
        assert!(!DataAssets::is_asset_indexed_for_owner(&1, &asset_id));
        assert!(DataAssets::is_asset_indexed_for_owner(&2, &asset_id));
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Settled
        );
        assert_eq!(
            DataAssets::runtime_escrow(escrow_id).unwrap().state,
            PaymentState::Released
        );
        assert_eq!(Balances::reserved_balance(2), 0);
        assert_eq!(Balances::free_balance(1), seller_free_before + 99);
        assert_eq!(Balances::free_balance(4), market_free_before + 2);
        let authorization = DataAssets::transfer_meta_authorizations(auth_id).unwrap();
        assert_eq!(authorization.status, AuthorizationStatus::Consumed);
        assert_eq!(authorization.consumed_order_id, Some(order_id));
        assert_eq!(
            DataAssets::active_transfer_meta_authorization(asset_id, market_id),
            None
        );
        assert_eq!(DataAssets::active_meta_order(asset_id), None);
        assert_eq!(market_authorization_count(market_id), 0);
        assert_eq!(market_active_order_count(market_id), 0);
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert!(DataAssets::order_settlements(order_id).is_none());
        assert_eq!(DataAssets::query_meta_token(&asset_id), Some(asset.clone()));
        assert_eq!(
            DataAssets::query_transfer_meta_authorization(&auth_id),
            Some(authorization)
        );
        assert_authoritative_settlement(
            order_id,
            MarketOperation::TransferMeta,
            asset_id,
            None,
            auth_id,
            asset_before.core.state_version,
            asset.core.state_version,
            pre_state_commitment,
        );
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::AlreadySettled
        );
    });
}

#[test]
fn funded_transfer_meta_uses_snapshot_after_authorization_revoke_or_expiry() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id) = fund_transfer_meta_fixture(100, 0);
        assert_ok!(DataAssets::revoke_transfer_meta_authorization(
            RuntimeOrigin::signed(1),
            auth_id,
        ));
        MARKETS.with(|markets| markets.borrow_mut().get_mut(&market_id).unwrap().0 = false);
        assert_eq!(market_authorization_count(market_id), 0);
        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(4), order_id));
        assert_eq!(DataAssets::get_asset(&asset_id).unwrap().core.owner, 2);
        assert_eq!(
            DataAssets::transfer_meta_authorizations(auth_id)
                .unwrap()
                .status,
            AuthorizationStatus::Consumed
        );
        assert_eq!(market_authorization_count(market_id), 0);
    });

    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id) = fund_transfer_meta_fixture(2, 0);
        System::set_block_number(3);
        assert_ok!(DataAssets::expire_transfer_meta_authorization(
            RuntimeOrigin::signed(4),
            auth_id,
        ));
        assert_eq!(market_authorization_count(market_id), 0);
        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(3), order_id));
        assert_eq!(DataAssets::get_asset(&asset_id).unwrap().core.owner, 2);
        let authorization = DataAssets::transfer_meta_authorizations(auth_id).unwrap();
        assert_eq!(authorization.status, AuthorizationStatus::Consumed);
        assert_eq!(authorization.consumed_order_id, Some(order_id));
        assert_eq!(market_authorization_count(market_id), 0);
    });
}

#[test]
fn transfer_meta_late_risk_rejections_preserve_funded_prestate() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id) = fund_transfer_meta_fixture(100, 0);
        let mut asset = DataAssets::get_asset(&asset_id).unwrap();
        asset.core.status = AssetStatus::Locked;
        assert_ok!(DataAssets::insert_asset(&asset_id, &asset));
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = order_before.escrow_id.unwrap();
        let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();
        let authorization_before = DataAssets::transfer_meta_authorizations(auth_id).unwrap();
        let balance_before = (Balances::free_balance(2), Balances::reserved_balance(2));

        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::AssetNotActive
        );
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
        assert_eq!(
            DataAssets::transfer_meta_authorizations(auth_id),
            Some(authorization_before)
        );
        assert_eq!(
            (Balances::free_balance(2), Balances::reserved_balance(2)),
            balance_before
        );
        assert_eq!(DataAssets::active_meta_order(asset_id), Some(order_id));
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (1, 101));
        assert!(DataAssets::query_settlement(&order_id).is_none());
        assert!(settlement_finalized_events().is_empty());
    });

    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id) = fund_transfer_meta_fixture(100, 0);
        let right_id = [0x82; 32];
        assert_ok!(DataAssets::create_right_object(structured_right(
            asset_id,
            right_id,
            3,
            1,
            Some(100),
        )));
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_before = DataAssets::runtime_escrow(order_before.escrow_id.unwrap()).unwrap();
        let authorization_before = DataAssets::transfer_meta_authorizations(auth_id).unwrap();
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::MutualExclusionConflict
        );
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(
            DataAssets::runtime_escrow(escrow_before.escrow_id),
            Some(escrow_before)
        );
        assert_eq!(
            DataAssets::transfer_meta_authorizations(auth_id),
            Some(authorization_before)
        );
        assert_eq!(DataAssets::get_asset(&asset_id).unwrap().core.owner, 1);
        assert_eq!(DataAssets::active_meta_order(asset_id), Some(order_id));
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (1, 101));
        assert!(DataAssets::query_settlement(&order_id).is_none());
        assert!(settlement_finalized_events().is_empty());
    });
}

#[test]
fn test_only_abort_rolls_back_transfer_meta_atomic_write_set() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id) = fund_transfer_meta_fixture(100, 0);
        let asset_before = DataAssets::get_asset(&asset_id).unwrap();
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = order_before.escrow_id.unwrap();
        let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();
        let authorization_before = DataAssets::transfer_meta_authorizations(auth_id).unwrap();
        let balances_before = (
            Balances::free_balance(1),
            Balances::free_balance(2),
            Balances::reserved_balance(2),
            Balances::free_balance(4),
        );
        DataAssets::set_test_abort_meta_settlement(true);

        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::InvalidInput
        );
        assert_eq!(DataAssets::get_asset(&asset_id), Some(asset_before));
        assert!(DataAssets::is_asset_indexed_for_owner(&1, &asset_id));
        assert!(!DataAssets::is_asset_indexed_for_owner(&2, &asset_id));
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
        assert_eq!(
            DataAssets::transfer_meta_authorizations(auth_id),
            Some(authorization_before)
        );
        assert_eq!(
            (
                Balances::free_balance(1),
                Balances::free_balance(2),
                Balances::reserved_balance(2),
                Balances::free_balance(4),
            ),
            balances_before
        );
        assert_eq!(DataAssets::active_meta_order(asset_id), Some(order_id));
        assert_eq!(market_authorization_count(market_id), 1);
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (1, 101));
        assert!(DataAssets::query_settlement(&order_id).is_none());
        assert!(settlement_finalized_events().is_empty());

        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(3), order_id));
        assert_eq!(DataAssets::get_asset(&asset_id).unwrap().core.owner, 2);
    });
}

#[test]
fn transfer_meta_settlement_requires_funded_matching_version_and_live_grace() {
    new_test_ext().execute_with(|| {
        let (_, market_id, _, order) = create_transfer_meta_fixture(100);
        let (seller, seller_signature, order_id) = signed_seller_input(&order, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            order,
            seller,
            seller_signature,
        ));
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::OrderNotFunded
        );
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Open
        );
        assert_eq!(market_active_order_count(market_id), 1);
    });

    new_test_ext().execute_with(|| {
        let (asset_id, market_id, _, order_id) = fund_transfer_meta_fixture(100, 0);
        let mut asset = DataAssets::get_asset(&asset_id).unwrap();
        asset.core.state_version += 1;
        assert_ok!(DataAssets::insert_asset(&asset_id, &asset));
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = order_before.escrow_id.unwrap();
        let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::StateVersionMismatch
        );
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
        assert_eq!(DataAssets::get_asset(&asset_id).unwrap().core.owner, 1);
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (1, 101));
        assert!(DataAssets::query_settlement(&order_id).is_none());
        assert!(settlement_finalized_events().is_empty());
    });

    new_test_ext().execute_with(|| {
        let (asset_id, market_id, _, order_id) = fund_transfer_meta_fixture(100, 0);
        System::set_block_number(12);
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = order_before.escrow_id.unwrap();
        let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(3), order_id),
            pallet_dataassets::Error::<Test>::OrderExpired
        );
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
        assert_eq!(DataAssets::get_asset(&asset_id).unwrap().core.owner, 1);
        assert_eq!(DataAssets::active_meta_order(asset_id), Some(order_id));
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (1, 101));
    });
}

#[test]
fn issue_right_settlement_creates_deterministic_right_and_converts_quota_atomically() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id, right_id) = fund_issue_right_fixture(100, 250);
        let funded_authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        let funded_order = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = funded_order.escrow_id.unwrap();
        let seller_free_before = Balances::free_balance(1);
        let market_free_before = Balances::free_balance(4);

        assert!(DataAssets::get_certificate(&asset_id, &right_id).is_none());
        assert!(!DataAssets::is_right_indexed_for_asset(
            &asset_id, &right_id
        ));
        assert!(!DataAssets::is_right_indexed_for_holder(&2, &right_id));
        assert_eq!(DataAssets::active_certificate_count(asset_id), 0);
        assert_eq!(
            (
                funded_authorization.reserved_count,
                funded_authorization.reserved_use_units,
                funded_authorization.issued_count,
                funded_authorization.issued_use_units,
            ),
            (1, 5, 0, 0)
        );
        assert!(DataAssets::query_settlement(&order_id).is_none());
        let pre_state_commitment = DataAssets::normalized_settlement_commitment(&order_id).unwrap();

        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(3), order_id));

        let right = DataAssets::get_certificate(&asset_id, &right_id).expect("issued right");
        assert_eq!(right.right_id, right_id);
        assert_eq!(right.token_id, 0);
        assert_eq!(right.asset_id, asset_id);
        assert_eq!(right.holder, 2);
        assert_eq!(right.issuer, 1);
        assert_eq!(right.right_type, funded_authorization.right_type);
        assert_eq!(right.data_scope, funded_authorization.data_scope);
        assert_eq!(right.validity, funded_authorization.validity_template);
        assert_eq!(right.remaining_uses, Some(5));
        assert_eq!(right.transferable, funded_authorization.transferable);
        assert_eq!(
            right.allowed_transfer_markets,
            funded_authorization.allowed_transfer_markets
        );
        assert_eq!(right.royalty_bps, funded_authorization.royalty_bps);
        assert_eq!(right.state_version, 1);
        assert_eq!(right.status, CertificateStatus::Active);
        assert_eq!(right.created_at, System::block_number());
        assert!(DataAssets::is_right_indexed_for_asset(&asset_id, &right_id));
        assert!(DataAssets::is_right_indexed_for_holder(&2, &right_id));
        assert_eq!(DataAssets::active_certificate_count(asset_id), 1);

        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!(
            (
                authorization.reserved_count,
                authorization.reserved_use_units,
                authorization.issued_count,
                authorization.issued_use_units,
            ),
            (0, 0, 1, 5)
        );
        assert_eq!(DataAssets::outstanding_issue_quota_count(asset_id), 0);
        assert_eq!(
            DataAssets::issue_quota_reservations(order_id)
                .unwrap()
                .status,
            IssueQuotaReservationStatus::Issued
        );
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Settled
        );
        assert_eq!(
            DataAssets::runtime_escrow(escrow_id).unwrap().state,
            PaymentState::Released
        );
        assert_eq!(Balances::reserved_balance(2), 0);
        assert_eq!(Balances::free_balance(1), seller_free_before + 99);
        assert_eq!(Balances::free_balance(4), market_free_before + 2);
        assert_eq!(market_active_order_count(market_id), 0);
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert_eq!(market_authorization_count(market_id), 1);
        assert!(DataAssets::order_settlements(order_id).is_none());
        assert_eq!(
            DataAssets::query_right_token(&asset_id, &right_id),
            Some(right.clone())
        );
        assert_eq!(
            DataAssets::query_issue_right_authorization(&auth_id),
            Some(authorization)
        );
        assert_authoritative_settlement(
            order_id,
            MarketOperation::IssueRight,
            asset_id,
            Some(right_id),
            auth_id,
            0,
            right.state_version,
            pre_state_commitment,
        );
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::AlreadySettled
        );
        assert_eq!(DataAssets::active_certificate_count(asset_id), 1);
    });
}

#[test]
fn last_issue_quota_competition_funds_and_issues_only_one_right() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let market_id = [0x84; 32];
        register_mock_market(market_id, true, 1 << 1);
        let mut terms = issue_authorization_terms(100, 0);
        terms.max_issue_count = 1;
        terms.max_total_use_units = 5;
        assert_ok!(DataAssets::create_issue_right_authorization(
            RuntimeOrigin::signed(1),
            asset_id,
            market_id,
            terms,
        ));
        let auth_id = DataAssets::active_issue_right_authorization(asset_id, market_id).unwrap();
        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        let first = issue_right_order(asset_id, market_id, auth_id, &authorization, 0, 100);
        let second = issue_right_order(asset_id, market_id, auth_id, &authorization, 1, 100);
        let (first_seller, first_signature, first_id) = signed_seller_input(&first, 1);
        let (second_seller, second_signature, second_id) = signed_seller_input(&second, 1);
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(3),
            first.clone(),
            first_seller,
            first_signature,
        ));
        assert_ok!(create_runtime_order(
            RuntimeOrigin::signed(4),
            second.clone(),
            second_seller,
            second_signature,
        ));

        let (first_buyer, first_buyer_signature) = signed_buyer_input(&first, 2, 0);
        assert_ok!(DataAssets::fund_order(
            RuntimeOrigin::signed(2),
            first_id,
            first_buyer,
            first_buyer_signature,
        ));
        let (second_buyer, second_buyer_signature) = signed_buyer_input(&second, 3, 0);
        assert_noop!(
            DataAssets::fund_order(
                RuntimeOrigin::signed(3),
                second_id,
                second_buyer,
                second_buyer_signature,
            ),
            pallet_dataassets::Error::<Test>::QuotaExceeded
        );

        let first_right_id = protocol::right_id(
            runtime_genesis_hash(),
            MarketOperation::IssueRight,
            first_id,
        );
        let second_right_id = protocol::right_id(
            runtime_genesis_hash(),
            MarketOperation::IssueRight,
            second_id,
        );
        assert!(DataAssets::get_certificate(&asset_id, &first_right_id).is_none());
        assert!(DataAssets::get_certificate(&asset_id, &second_right_id).is_none());
        assert_eq!(
            DataAssets::runtime_order(second_id).unwrap().state,
            OrderState::Open
        );
        assert!(DataAssets::runtime_order(second_id)
            .unwrap()
            .escrow_id
            .is_none());
        assert_eq!(DataAssets::buyer_nonce(test_protocol_account(3)), 0);

        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(3), first_id));
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &first_right_id)
                .unwrap()
                .holder,
            2
        );
        assert!(DataAssets::get_certificate(&asset_id, &second_right_id).is_none());
        let settled_authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!(
            (
                settled_authorization.reserved_count,
                settled_authorization.issued_count,
                settled_authorization.issued_use_units,
            ),
            (0, 1, 5)
        );
        assert_eq!(DataAssets::active_certificate_count(asset_id), 1);
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert_ok!(DataAssets::cancel_open(RuntimeOrigin::signed(1), second_id,));
        assert_eq!(market_active_order_count(market_id), 0);
    });
}

#[test]
fn funded_issue_uses_authorization_snapshot_after_revoke_and_market_restriction() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id, right_id) = fund_issue_right_fixture(100, 0);
        assert_ok!(DataAssets::revoke_issue_right_authorization(
            RuntimeOrigin::signed(1),
            auth_id,
        ));
        MARKETS.with(|markets| markets.borrow_mut().get_mut(&market_id).unwrap().0 = false);
        assert_eq!(market_authorization_count(market_id), 0);

        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(4), order_id));

        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id)
                .unwrap()
                .holder,
            2
        );
        let authorization = DataAssets::issue_right_authorizations(auth_id).unwrap();
        assert_eq!(authorization.status, AuthorizationStatus::Revoked);
        assert_eq!(
            (
                authorization.reserved_count,
                authorization.issued_count,
                authorization.issued_use_units,
            ),
            (0, 1, 5)
        );
        assert_eq!(market_authorization_count(market_id), 0);
        assert_eq!(market_active_order_count(market_id), 0);
        assert_eq!(market_funding_state(market_id), (0, 0));
    });
}

#[test]
fn issue_right_rejections_preserve_funded_state_and_never_leave_an_orphan() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id, right_id) = fund_issue_right_fixture(100, 0);
        crate::pallet::IssueQuotaReservations::<Test>::remove(order_id);
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = order_before.escrow_id.unwrap();
        let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();
        let authorization_before = DataAssets::issue_right_authorizations(auth_id).unwrap();
        let balances_before = (
            Balances::free_balance(1),
            Balances::free_balance(2),
            Balances::reserved_balance(2),
            Balances::free_balance(4),
        );

        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::QuotaReservationMissing
        );
        assert!(DataAssets::get_certificate(&asset_id, &right_id).is_none());
        assert_eq!(DataAssets::active_certificate_count(asset_id), 0);
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
        assert_eq!(
            DataAssets::issue_right_authorizations(auth_id),
            Some(authorization_before)
        );
        assert_eq!(
            (
                Balances::free_balance(1),
                Balances::free_balance(2),
                Balances::reserved_balance(2),
                Balances::free_balance(4),
            ),
            balances_before
        );
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (1, 101));
    });

    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id, right_id) = fund_issue_right_fixture(100, 0);
        let existing = structured_right(asset_id, right_id, 3, 1, Some(100));
        assert_ok!(DataAssets::create_right_object(existing.clone()));
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = order_before.escrow_id.unwrap();
        let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();
        let authorization_before = DataAssets::issue_right_authorizations(auth_id).unwrap();
        let reservation_before = DataAssets::issue_quota_reservations(order_id).unwrap();
        let balances_before = (
            Balances::free_balance(1),
            Balances::free_balance(2),
            Balances::reserved_balance(2),
            Balances::free_balance(4),
        );

        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::RightAlreadyExists
        );
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id),
            Some(existing)
        );
        assert_eq!(DataAssets::active_certificate_count(asset_id), 1);
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
        assert_eq!(
            DataAssets::issue_right_authorizations(auth_id),
            Some(authorization_before)
        );
        assert_eq!(
            DataAssets::issue_quota_reservations(order_id),
            Some(reservation_before)
        );
        assert_eq!(
            (
                Balances::free_balance(1),
                Balances::free_balance(2),
                Balances::reserved_balance(2),
                Balances::free_balance(4),
            ),
            balances_before
        );
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (1, 101));
    });
}

#[test]
fn test_only_abort_rolls_back_issue_right_atomic_write_set() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, auth_id, order_id, right_id) = fund_issue_right_fixture(100, 0);
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = order_before.escrow_id.unwrap();
        let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();
        let authorization_before = DataAssets::issue_right_authorizations(auth_id).unwrap();
        let reservation_before = DataAssets::issue_quota_reservations(order_id).unwrap();
        let balances_before = (
            Balances::free_balance(1),
            Balances::free_balance(2),
            Balances::reserved_balance(2),
            Balances::free_balance(4),
        );
        DataAssets::set_test_abort_issue_right_settlement(true);

        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::InvalidInput
        );
        assert!(DataAssets::get_certificate(&asset_id, &right_id).is_none());
        assert!(!DataAssets::is_right_indexed_for_asset(
            &asset_id, &right_id
        ));
        assert!(!DataAssets::is_right_indexed_for_holder(&2, &right_id));
        assert_eq!(DataAssets::active_certificate_count(asset_id), 0);
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
        assert_eq!(
            DataAssets::issue_right_authorizations(auth_id),
            Some(authorization_before)
        );
        assert_eq!(
            DataAssets::issue_quota_reservations(order_id),
            Some(reservation_before)
        );
        assert_eq!(
            (
                Balances::free_balance(1),
                Balances::free_balance(2),
                Balances::reserved_balance(2),
                Balances::free_balance(4),
            ),
            balances_before
        );
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (1, 101));

        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(3), order_id));
        let right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        assert_eq!(right.token_id, 0);
        assert_eq!(right.holder, 2);
    });
}

#[test]
fn right_resale_settlement_transfers_holder_pays_royalty_and_closes_state() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, order_id, _) = fund_transfer_right_fixture(100, 250);
        let right_before = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = order_before.escrow_id.unwrap();
        let owner_free_before = Balances::free_balance(1);
        let seller_free_before = Balances::free_balance(2);
        let buyer_free_before = Balances::free_balance(3);
        let market_free_before = Balances::free_balance(4);
        assert!(DataAssets::query_settlement(&order_id).is_none());
        let pre_state_commitment = DataAssets::normalized_settlement_commitment(&order_id).unwrap();

        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(4), order_id));

        let right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        assert_eq!(right.holder, 3);
        assert_eq!(right.state_version, right_before.state_version + 1);
        assert!(DataAssets::is_right_indexed_for_asset(&asset_id, &right_id));
        assert!(!DataAssets::is_right_indexed_for_holder(&2, &right_id));
        assert!(DataAssets::is_right_indexed_for_holder(&3, &right_id));
        assert_eq!(DataAssets::active_right_order(right_id), None);
        assert_eq!(
            DataAssets::runtime_order(order_id).unwrap().state,
            OrderState::Settled
        );
        assert_eq!(
            DataAssets::runtime_escrow(escrow_id).unwrap().state,
            PaymentState::Released
        );
        assert_eq!(Balances::reserved_balance(3), 0);
        assert_eq!(Balances::free_balance(1), owner_free_before + 2);
        assert_eq!(Balances::free_balance(2), seller_free_before + 97);
        assert_eq!(Balances::free_balance(3), buyer_free_before);
        assert_eq!(Balances::free_balance(4), market_free_before + 2);
        assert_eq!(market_active_order_count(market_id), 0);
        assert_eq!(market_funding_state(market_id), (0, 0));
        assert!(DataAssets::order_settlements(order_id).is_none());
        assert_eq!(
            DataAssets::query_right_token(&asset_id, &right_id),
            Some(right.clone())
        );
        assert!(DataAssets::query_issue_right_authorization(&right_id).is_none());
        assert!(DataAssets::query_transfer_meta_authorization(&right_id).is_none());
        assert_authoritative_settlement(
            order_id,
            MarketOperation::TransferRight,
            asset_id,
            Some(right_id),
            right_id,
            right_before.state_version,
            right.state_version,
            pre_state_commitment,
        );
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::AlreadySettled
        );
    });
}

#[test]
fn right_resale_rejects_missing_expired_exhausted_and_revoked_rights_cleanly() {
    for status in [
        CertificateStatus::Expired,
        CertificateStatus::Exhausted,
        CertificateStatus::Revoked,
    ] {
        new_test_ext().execute_with(|| {
            let (asset_id, market_id, right_id, order_id, _) = fund_transfer_right_fixture(100, 0);
            let mut right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
            right.status = status;
            if status == CertificateStatus::Exhausted {
                right.remaining_uses = Some(0);
            }
            assert_ok!(DataAssets::test_update_certificate(&asset_id, &right));
            let order_before = DataAssets::runtime_order(order_id).unwrap();
            let escrow_id = order_before.escrow_id.unwrap();
            let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();
            let balances_before = (
                Balances::free_balance(1),
                Balances::free_balance(2),
                Balances::free_balance(3),
                Balances::free_balance(4),
                Balances::reserved_balance(3),
            );

            assert_noop!(
                DataAssets::settle(RuntimeOrigin::signed(4), order_id),
                pallet_dataassets::Error::<Test>::CertificateNotActive
            );
            assert_eq!(
                DataAssets::get_certificate(&asset_id, &right_id),
                Some(right)
            );
            assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
            assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
            assert_eq!(DataAssets::active_right_order(right_id), Some(order_id));
            assert_eq!(market_active_order_count(market_id), 1);
            assert_eq!(market_funding_state(market_id), (1, 101));
            assert_eq!(
                (
                    Balances::free_balance(1),
                    Balances::free_balance(2),
                    Balances::free_balance(3),
                    Balances::free_balance(4),
                    Balances::reserved_balance(3),
                ),
                balances_before
            );
        });
    }

    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, order_id, _) = fund_transfer_right_fixture(100, 0);
        let mut stored = DataAssets::runtime_order(order_id).unwrap();
        stored.order_commitment.object_id = [0x99; 32];
        crate::pallet::RuntimeOrders::<Test>::insert(order_id, stored.clone());
        let escrow_id = stored.escrow_id.unwrap();
        let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();

        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::RightInvalid
        );
        assert_eq!(DataAssets::runtime_order(order_id), Some(stored));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
        assert!(DataAssets::get_certificate(&asset_id, &right_id).is_some());
        assert_eq!(DataAssets::active_right_order(right_id), Some(order_id));
        assert_eq!(market_active_order_count(market_id), 1);
    });
}

#[test]
fn right_resale_rechecks_holder_parent_version_and_transfer_policy_at_settlement() {
    new_test_ext().execute_with(|| {
        let (_asset_id, _market_id, _right_id, order_id, _) = fund_transfer_right_fixture(100, 0);
        let mut stored = DataAssets::runtime_order(order_id).unwrap();
        stored.order_commitment.seller = test_protocol_account(1);
        crate::pallet::RuntimeOrders::<Test>::insert(order_id, stored.clone());
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::OwnershipMismatch
        );
    });

    new_test_ext().execute_with(|| {
        let (_asset_id, _market_id, _right_id, order_id, _) = fund_transfer_right_fixture(100, 0);
        let mut stored = DataAssets::runtime_order(order_id).unwrap();
        stored.order_commitment.parent_asset_id = Some([0x98; 32]);
        crate::pallet::RuntimeOrders::<Test>::insert(order_id, stored.clone());
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::ParentMismatch
        );
    });

    new_test_ext().execute_with(|| {
        let (asset_id, _market_id, right_id, order_id, _) = fund_transfer_right_fixture(100, 0);
        let mut right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        right.state_version += 1;
        assert_ok!(DataAssets::test_update_certificate(&asset_id, &right));
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::StateVersionMismatch
        );
    });

    for disallowed in [false, true] {
        new_test_ext().execute_with(|| {
            let (asset_id, _market_id, right_id, order_id, _) = fund_transfer_right_fixture(100, 0);
            let mut right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
            if disallowed {
                right.allowed_transfer_markets =
                    BoundedVec::try_from(vec![[8u8; 32]]).expect("bounded market");
            } else {
                right.transferable = false;
            }
            assert_ok!(DataAssets::test_update_certificate(&asset_id, &right));
            assert_noop!(
                DataAssets::settle(RuntimeOrigin::signed(4), order_id),
                pallet_dataassets::Error::<Test>::AuthorizationMissing
            );
        });
    }
}

#[test]
fn late_right_expiry_leaves_funded_resale_unchanged() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, order_id, _) = fund_transfer_right_fixture(100, 0);
        let mut expired_right = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        expired_right.status = CertificateStatus::Expired;
        assert_ok!(DataAssets::test_update_certificate(
            &asset_id,
            &expired_right,
        ));
        let right_before = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = order_before.escrow_id.unwrap();
        let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();
        let balances_before = (
            Balances::free_balance(1),
            Balances::free_balance(2),
            Balances::free_balance(3),
            Balances::free_balance(4),
            Balances::reserved_balance(3),
        );

        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::CertificateNotActive
        );
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id),
            Some(right_before)
        );
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
        assert_eq!(DataAssets::active_right_order(right_id), Some(order_id));
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (1, 101));
        assert_eq!(
            (
                Balances::free_balance(1),
                Balances::free_balance(2),
                Balances::free_balance(3),
                Balances::free_balance(4),
                Balances::reserved_balance(3),
            ),
            balances_before
        );
        assert!(DataAssets::query_settlement(&order_id).is_none());
        assert!(settlement_finalized_events().is_empty());
    });
}

#[test]
fn competing_right_resale_orders_and_duplicate_settlement_allow_only_one_success() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, first_order_id, order) =
            fund_transfer_right_fixture(100, 0);
        let mut competing = order.clone();
        competing.order_nonce = 1;
        let (seller, seller_signature, second_order_id) = signed_seller_input(&competing, 2);
        assert_noop!(
            create_runtime_order(
                RuntimeOrigin::signed(4),
                competing,
                seller,
                seller_signature,
            ),
            pallet_dataassets::Error::<Test>::ActiveObjectOrderExists
        );
        assert!(DataAssets::runtime_order(second_order_id).is_none());
        assert_eq!(
            DataAssets::active_right_order(right_id),
            Some(first_order_id)
        );
        assert_eq!(market_active_order_count(market_id), 1);
        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(4), first_order_id));
        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), first_order_id),
            pallet_dataassets::Error::<Test>::AlreadySettled
        );
        assert!(DataAssets::query_settlement(&second_order_id).is_none());
        assert!(DataAssets::query_settlement(&first_order_id).is_some());
        assert_eq!(crate::pallet::SettlementRecords::<Test>::iter().count(), 1);
        assert_eq!(settlement_finalized_events().len(), 1);
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id)
                .unwrap()
                .holder,
            3
        );
    });
}

#[test]
fn experiment_right_fixture_requires_root_and_preserves_indexes() {
    new_test_ext().execute_with(|| {
        let (asset_id, _) = register_test_asset(1);
        let right_id = [0xe3; 32];
        let right = structured_right(asset_id, right_id, 2, 1, Some(100));

        assert_noop!(
            DataAssets::initialize_right_for_experiment(RuntimeOrigin::signed(1), right.clone(),),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_ok!(DataAssets::initialize_right_for_experiment(
            RuntimeOrigin::root(),
            right.clone(),
        ));
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id),
            Some(right.clone())
        );
        assert!(DataAssets::is_right_indexed_for_holder(&2, &right_id));
        assert!(DataAssets::is_right_indexed_for_asset(&asset_id, &right_id));
        assert_eq!(DataAssets::active_certificate_count(asset_id), 1);
        assert_noop!(
            DataAssets::initialize_right_for_experiment(RuntimeOrigin::root(), right),
            pallet_dataassets::Error::<Test>::RightAlreadyExists
        );

        let asset_before = DataAssets::get_asset(&asset_id).unwrap();
        let right_before = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        assert_noop!(
            DataAssets::configure_meta_status_for_experiment(
                RuntimeOrigin::signed(1),
                asset_id,
                AssetStatus::Retiring,
            ),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_noop!(
            DataAssets::configure_right_status_for_experiment(
                RuntimeOrigin::signed(1),
                asset_id,
                right_id,
                CertificateStatus::Expired,
            ),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_ok!(DataAssets::configure_meta_status_for_experiment(
            RuntimeOrigin::root(),
            asset_id,
            AssetStatus::Retiring,
        ));
        assert_ok!(DataAssets::configure_right_status_for_experiment(
            RuntimeOrigin::root(),
            asset_id,
            right_id,
            CertificateStatus::Expired,
        ));
        let mut expected_asset = asset_before;
        expected_asset.core.status = AssetStatus::Retiring;
        let mut expected_right = right_before;
        expected_right.status = CertificateStatus::Expired;
        assert_eq!(DataAssets::get_asset(&asset_id), Some(expected_asset));
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id),
            Some(expected_right)
        );
        assert!(DataAssets::is_asset_indexed_for_owner(&1, &asset_id));
        assert!(DataAssets::is_right_indexed_for_holder(&2, &right_id));
        assert!(DataAssets::is_right_indexed_for_asset(&asset_id, &right_id));
        assert_eq!(DataAssets::active_certificate_count(asset_id), 1);
        assert_noop!(
            DataAssets::configure_meta_status_for_experiment(
                RuntimeOrigin::root(),
                [0xef; 32],
                AssetStatus::Retiring,
            ),
            pallet_dataassets::Error::<Test>::AssetNotFound
        );
        assert_noop!(
            DataAssets::configure_right_status_for_experiment(
                RuntimeOrigin::root(),
                asset_id,
                [0xef; 32],
                CertificateStatus::Expired,
            ),
            pallet_dataassets::Error::<Test>::CertificateNotFound
        );
        assert_eq!(DataAssets::active_certificate_count(asset_id), 1);
    });
}

#[test]
fn test_only_abort_rolls_back_right_resale_atomic_write_set() {
    new_test_ext().execute_with(|| {
        let (asset_id, market_id, right_id, order_id, _) = fund_transfer_right_fixture(100, 250);
        let right_before = DataAssets::get_certificate(&asset_id, &right_id).unwrap();
        let order_before = DataAssets::runtime_order(order_id).unwrap();
        let escrow_id = order_before.escrow_id.unwrap();
        let escrow_before = DataAssets::runtime_escrow(escrow_id).unwrap();
        let balances_before = (
            Balances::free_balance(1),
            Balances::free_balance(2),
            Balances::free_balance(3),
            Balances::free_balance(4),
            Balances::reserved_balance(3),
        );
        DataAssets::set_test_abort_transfer_right_settlement(true);

        assert_noop!(
            DataAssets::settle(RuntimeOrigin::signed(4), order_id),
            pallet_dataassets::Error::<Test>::InvalidInput
        );
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id),
            Some(right_before)
        );
        assert!(DataAssets::is_right_indexed_for_holder(&2, &right_id));
        assert!(!DataAssets::is_right_indexed_for_holder(&3, &right_id));
        assert_eq!(DataAssets::runtime_order(order_id), Some(order_before));
        assert_eq!(DataAssets::runtime_escrow(escrow_id), Some(escrow_before));
        assert_eq!(DataAssets::active_right_order(right_id), Some(order_id));
        assert_eq!(market_active_order_count(market_id), 1);
        assert_eq!(market_funding_state(market_id), (1, 101));
        assert_eq!(
            (
                Balances::free_balance(1),
                Balances::free_balance(2),
                Balances::free_balance(3),
                Balances::free_balance(4),
                Balances::reserved_balance(3),
            ),
            balances_before
        );
        assert!(DataAssets::query_settlement(&order_id).is_none());
        assert!(settlement_finalized_events().is_empty());

        assert_ok!(DataAssets::settle(RuntimeOrigin::signed(4), order_id));
        assert_eq!(
            DataAssets::get_certificate(&asset_id, &right_id)
                .unwrap()
                .holder,
            3
        );
    });
}
