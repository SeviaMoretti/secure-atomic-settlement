use crate as pallet_markets;
use crate::{Error, MarketCapabilities, MarketCapacityTier, MarketStatus};
use frame_support::{
    assert_noop, assert_ok, construct_runtime, derive_impl,
    storage::{with_transaction, TransactionOutcome},
    traits::ConstU16,
    weights::Weight,
};
use pallet_shared_traits::{
    MarketAccessError, MarketContractVerifier, MarketOperation, MarketRegistryProvider,
    MarketStakeHandler,
};
use sp_runtime::BuildStorage;
use std::cell::RefCell;

type AccountId = u64;
type Balance = u128;
type Block = frame_system::mocking::MockBlock<Test>;

construct_runtime!(
    pub enum Test {
        System: frame_system,
        Markets: pallet_markets,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
}

thread_local! {
    static STAKES: RefCell<Vec<(AccountId, Balance)>> = const { RefCell::new(Vec::new()) };
}

pub struct MockContractVerifier;

impl MarketContractVerifier<AccountId> for MockContractVerifier {
    fn is_market(_operator: &AccountId, contract_address: &AccountId) -> bool {
        *contract_address != 99
    }
}

pub struct MockStakeHandler;

impl MarketStakeHandler<AccountId, Balance> for MockStakeHandler {
    fn pledge(operator: &AccountId, amount: Balance) -> frame_support::dispatch::DispatchResult {
        STAKES.with(|stakes| stakes.borrow_mut().push((*operator, amount)));
        Ok(())
    }

    fn unbond(operator: &AccountId) -> frame_support::dispatch::DispatchResult {
        STAKES.with(|stakes| stakes.borrow_mut().retain(|(who, _)| who != operator));
        Ok(())
    }
}

pub struct TestWeightInfo;

impl crate::pallet::WeightInfo for TestWeightInfo {
    fn set_capacity_tier() -> Weight {
        Weight::zero()
    }

    fn register_market() -> Weight {
        Weight::zero()
    }

    fn restrict_market() -> Weight {
        Weight::zero()
    }

    fn activate_market() -> Weight {
        Weight::zero()
    }

    fn request_exit() -> Weight {
        Weight::zero()
    }

    fn deregister_market() -> Weight {
        Weight::zero()
    }
}

impl pallet_markets::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type Balance = Balance;
    type MarketAdminOrigin = frame_system::EnsureRoot<AccountId>;
    type ContractVerifier = MockContractVerifier;
    type StakeHandler = MockStakeHandler;
    type SettlementProtocolVersion = ConstU16<1>;
    type MarketWeightInfo = TestWeightInfo;
}

fn new_test_ext() -> sp_io::TestExternalities {
    let storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| {
        System::set_block_number(1);
        STAKES.with(|stakes| stakes.borrow_mut().clear());
    });
    ext
}

fn capacity_tier() -> MarketCapacityTier<Balance> {
    MarketCapacityTier {
        tier_id: 1,
        required_stake: 10,
        max_active_orders: 8,
        max_funded_orders: 4,
        max_active_exposure_value: 1_000,
    }
}

fn configure_tier() {
    assert_ok!(Markets::set_capacity_tier(
        RuntimeOrigin::root(),
        capacity_tier()
    ));
}

fn register_market(contract_address: AccountId, capabilities: MarketCapabilities) -> [u8; 32] {
    register_market_with_fee(contract_address, capabilities, 0)
}

fn register_market_with_fee(
    contract_address: AccountId,
    capabilities: MarketCapabilities,
    market_fee_bps: u16,
) -> [u8; 32] {
    assert_ok!(Markets::register_market(
        RuntimeOrigin::signed(1),
        contract_address,
        [7; 32],
        capabilities,
        1,
        market_fee_bps,
    ));
    Markets::contract_market_id(contract_address).expect("registered market id")
}

#[test]
fn registration_uses_stable_identity_and_capacity_tier_fields() {
    new_test_ext().execute_with(|| {
        configure_tier();
        let capabilities = MarketCapabilities::transaction_operations();
        let market_id = register_market(10, capabilities);
        let registration = Markets::market_registrations(market_id).unwrap();

        assert_eq!(registration.market_id, market_id);
        assert_eq!(registration.operator, 1);
        assert_eq!(registration.contract_address, 10);
        assert_eq!(registration.protocol_version, 1);
        assert_eq!(registration.capabilities, capabilities);
        assert_eq!(registration.capacity_tier, 1);
        assert_eq!(registration.stake, 10);
        assert_eq!(registration.active_orders, 0);
        assert_eq!(registration.funded_orders, 0);
        assert_eq!(registration.active_exposure_value, 0);
        assert_eq!(registration.status, MarketStatus::Active);
    });
}

#[test]
fn deregistered_identity_is_permanent_and_rejoin_gets_a_new_id() {
    new_test_ext().execute_with(|| {
        configure_tier();
        let first = register_market(10, MarketCapabilities::transaction_operations());
        assert_ok!(Markets::request_exit(RuntimeOrigin::signed(1), first));
        assert_ok!(Markets::deregister_market(RuntimeOrigin::signed(1), first));
        assert_eq!(
            Markets::market_registrations(first).unwrap().status,
            MarketStatus::Deregistered
        );

        let second = register_market(10, MarketCapabilities::transaction_operations());
        assert_ne!(first, second);
        assert_eq!(Markets::contract_market_id(10), Some(second));
    });
}

#[test]
fn lifecycle_restricts_new_business_and_enforces_transitions() {
    new_test_ext().execute_with(|| {
        configure_tier();
        let market_id = register_market(10, MarketCapabilities::transaction_operations());

        assert_ok!(Markets::restrict_market(RuntimeOrigin::root(), market_id));
        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::ensure_market_operation(
                &market_id,
                MarketOperation::TransferMeta,
            ),
            Err(MarketAccessError::Restricted)
        );
        assert_noop!(
            Markets::restrict_market(RuntimeOrigin::root(), market_id),
            Error::<Test>::InvalidMarketTransition
        );

        assert_ok!(Markets::activate_market(RuntimeOrigin::root(), market_id));
        assert_ok!(Markets::request_exit(RuntimeOrigin::signed(1), market_id));
        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::ensure_market_operation(
                &market_id,
                MarketOperation::IssueRight,
            ),
            Err(MarketAccessError::Restricted)
        );
        assert_ok!(Markets::deregister_market(
            RuntimeOrigin::signed(1),
            market_id
        ));
        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::ensure_market_operation(
                &market_id,
                MarketOperation::IssueRight,
            ),
            Err(MarketAccessError::NotRegistered)
        );
    });
}

#[test]
fn validation_distinguishes_unregistered_and_missing_capability() {
    new_test_ext().execute_with(|| {
        let missing = [42; 32];
        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::ensure_market_operation(
                &missing,
                MarketOperation::TransferMeta,
            ),
            Err(MarketAccessError::NotRegistered)
        );

        configure_tier();
        let issue_only =
            MarketCapabilities::from_bits(MarketCapabilities::CREATE_RIGHT_ISSUE_ORDER).unwrap();
        let market_id = register_market(10, issue_only);
        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::ensure_market_operation(
                &market_id,
                MarketOperation::TransferMeta,
            ),
            Err(MarketAccessError::CapabilityDenied)
        );
        assert_ok!(
            <Markets as MarketRegistryProvider<AccountId>>::ensure_market_operation(
                &market_id,
                MarketOperation::IssueRight,
            )
        );
    });
}

#[test]
fn active_authorization_blocks_final_deregistration() {
    new_test_ext().execute_with(|| {
        configure_tier();
        let market_id = register_market(10, MarketCapabilities::transaction_operations());
        assert_ok!(
            <Markets as MarketRegistryProvider<AccountId>>::note_authorization_created(&market_id)
        );
        assert_ok!(Markets::request_exit(RuntimeOrigin::signed(1), market_id));
        assert_noop!(
            Markets::deregister_market(RuntimeOrigin::signed(1), market_id),
            Error::<Test>::MarketHasActiveObligations
        );
        assert_ok!(
            <Markets as MarketRegistryProvider<AccountId>>::note_authorization_closed(&market_id)
        );
        assert_ok!(Markets::deregister_market(
            RuntimeOrigin::signed(1),
            market_id
        ));
    });
}

#[test]
fn active_order_capacity_is_checked_and_released_exactly_once() {
    new_test_ext().execute_with(|| {
        configure_tier();
        let market_id = register_market(10, MarketCapabilities::transaction_operations());

        for expected in 1..=8 {
            assert_ok!(
                <Markets as MarketRegistryProvider<AccountId>>::note_active_order_created(
                    &market_id
                )
            );
            assert_eq!(
                Markets::market_registrations(market_id)
                    .unwrap()
                    .active_orders,
                expected
            );
        }
        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::note_active_order_created(&market_id),
            Err(MarketAccessError::ActiveOrderCapacityExceeded)
        );
        assert_eq!(
            Markets::market_registrations(market_id)
                .unwrap()
                .active_orders,
            8
        );

        for _ in 0..8 {
            assert_ok!(
                <Markets as MarketRegistryProvider<AccountId>>::note_active_order_closed(
                    &market_id
                )
            );
        }
        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::note_active_order_closed(&market_id),
            Err(MarketAccessError::ActiveOrderCounterUnderflow)
        );
    });
}

#[test]
fn restricted_market_rejects_new_orders_but_allows_existing_order_release() {
    new_test_ext().execute_with(|| {
        configure_tier();
        let market_id = register_market(10, MarketCapabilities::transaction_operations());
        assert_ok!(
            <Markets as MarketRegistryProvider<AccountId>>::note_active_order_created(&market_id)
        );
        assert_ok!(Markets::restrict_market(RuntimeOrigin::root(), market_id));

        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::note_active_order_created(&market_id),
            Err(MarketAccessError::Restricted)
        );
        assert_ok!(
            <Markets as MarketRegistryProvider<AccountId>>::note_active_order_closed(&market_id)
        );
        assert_eq!(
            Markets::market_registrations(market_id)
                .unwrap()
                .active_orders,
            0
        );
    });
}

#[test]
fn funded_and_exposure_capacity_accounting_is_exact_and_releasable() {
    new_test_ext().execute_with(|| {
        configure_tier();
        let market_id = register_market_with_fee(
            10,
            MarketCapabilities::transaction_operations(),
            250,
        );
        let terms =
            <Markets as MarketRegistryProvider<AccountId>>::market_payment_terms(&market_id)
                .expect("active market payment terms");
        assert_eq!(terms.market_fee_bps, 250);
        assert_eq!(terms.market_fee_recipient, 1);

        assert_ok!(<Markets as MarketRegistryProvider<AccountId>>::note_order_funded(
            &market_id,
            400,
        ));
        assert_ok!(<Markets as MarketRegistryProvider<AccountId>>::note_order_funded(
            &market_id,
            600,
        ));
        let registration = Markets::market_registrations(market_id).unwrap();
        assert_eq!(registration.funded_orders, 2);
        assert_eq!(registration.active_exposure_value, 1_000);
        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::note_order_funded(&market_id, 1),
            Err(MarketAccessError::ExposureCapacityExceeded)
        );

        assert_ok!(<Markets as MarketRegistryProvider<AccountId>>::note_funded_order_closed(
            &market_id,
            400,
        ));
        assert_ok!(Markets::restrict_market(RuntimeOrigin::root(), market_id));
        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::market_payment_terms(&market_id),
            Err(MarketAccessError::Restricted)
        );
        assert_ok!(<Markets as MarketRegistryProvider<AccountId>>::note_funded_order_closed(
            &market_id,
            600,
        ));
        let registration = Markets::market_registrations(market_id).unwrap();
        assert_eq!(registration.funded_orders, 0);
        assert_eq!(registration.active_exposure_value, 0);
        assert_eq!(
            <Markets as MarketRegistryProvider<AccountId>>::note_funded_order_closed(
                &market_id,
                1,
            ),
            Err(MarketAccessError::FundedOrderCounterUnderflow)
        );
    });
}

#[test]
fn market_counters_join_the_settlement_storage_transaction() {
    new_test_ext().execute_with(|| {
        configure_tier();
        let market_id = register_market(10, MarketCapabilities::transaction_operations());
        type Registry = Markets;
        assert_ok!(Registry::note_authorization_created(&market_id));
        assert_ok!(Registry::note_active_order_created(&market_id));
        assert_ok!(Registry::note_order_funded(&market_id, 101));
        let before = Registry::market_state(&market_id).unwrap();
        let events_before = System::events();

        let result: Result<(), sp_runtime::DispatchError> = with_transaction(|| {
            assert_ok!(Registry::note_authorization_closed(&market_id));
            assert_ok!(Registry::note_funded_order_closed(&market_id, 101));
            assert_ok!(Registry::note_active_order_closed(&market_id));
            let closed = Registry::market_state(&market_id).unwrap();
            assert_eq!(closed.active_authorizations, 0);
            assert_eq!(closed.active_orders, 0);
            assert_eq!(closed.funded_orders, 0);
            assert_eq!(closed.active_exposure_value, 0);
            TransactionOutcome::Rollback(Err(sp_runtime::DispatchError::Other("injected")))
        });

        assert_eq!(result, Err(sp_runtime::DispatchError::Other("injected")));
        assert_eq!(Registry::market_state(&market_id).unwrap(), before);
        assert_eq!(System::events(), events_before);
    });
}

#[test]
fn experiment_market_fixture_requires_admin_and_updates_related_state() {
    new_test_ext().execute_with(|| {
        configure_tier();
        let all = MarketCapabilities::transaction_operations();
        let market_id = register_market_with_fee(10, all, 250);
        let issue_only =
            MarketCapabilities::from_bits(MarketCapabilities::CREATE_RIGHT_ISSUE_ORDER).unwrap();

        assert_noop!(
            Markets::configure_market_for_experiment(
                RuntimeOrigin::signed(1),
                market_id,
                true,
                Some(issue_only),
                Some(0),
            ),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_ok!(Markets::configure_market_for_experiment(
            RuntimeOrigin::root(),
            market_id,
            true,
            Some(issue_only),
            Some(0),
        ));
        assert_eq!(
            Markets::market_registrations(market_id)
                .unwrap()
                .capabilities,
            issue_only
        );
        assert_eq!(Markets::capacity_tiers(1).unwrap().max_funded_orders, 0);

        assert_ok!(Markets::configure_market_for_experiment(
            RuntimeOrigin::root(),
            market_id,
            false,
            None,
            None,
        ));
        assert!(Markets::market_registrations(market_id).is_none());
        assert_eq!(Markets::contract_market_id(10), None);
        assert!(crate::pallet::MarketFeeBps::<Test>::get(market_id).is_none());
    });
}
