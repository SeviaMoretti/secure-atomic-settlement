use frame_support::{
    assert_noop, assert_ok, construct_runtime, derive_impl, parameter_types,
    traits::{ConstU128, ConstU32, Hooks},
};
use pallet_dataassets::types::{DataAsset, RightToken};
use sp_core::H256;
use sp_runtime::BuildStorage;
use std::cell::RefCell;

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
}

pub struct TestIncentives;

impl pallet_shared_traits::IncentiveHandler<AccountId, [u8; 32], Balance> for TestIncentives {
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
    type IncentiveHandler = TestIncentives;
    type WeightInfo = pallet_dataassets::weights::WeightInfo<Test>;
}

fn new_test_ext() -> sp_io::TestExternalities {
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    pallet_balances::GenesisConfig::<Test> {
        balances: vec![(1, 1_000), (2, 1_000), (3, 1_000)],
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .unwrap();

    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| {
        System::set_block_number(1);
        Timestamp::set_timestamp(42);
        System::reset_events();
        FIRST_CREATE_REWARDS.with(|rewards| rewards.borrow_mut().clear());
        TRADE_MEASUREMENTS.with(|measurements| measurements.borrow_mut().clear());
    });
    ext
}

fn trade_measurements() -> Vec<[u8; 32]> {
    TRADE_MEASUREMENTS.with(|measurements| measurements.borrow().clone())
}

fn register_test_asset(owner: AccountId) -> [u8; 32] {
    let raw_data_hash = H256::repeat_byte(0x88);

    assert_ok!(DataAssets::register_asset(
        RuntimeOrigin::signed(owner),
        b"asset".to_vec(),
        b"description".to_vec(),
        raw_data_hash,
        1024 * 1024,
    ));

    let asset_id = DataAsset::generate_asset_id(&owner, Timestamp::get(), &raw_data_hash);
    assert!(DataAssets::get_asset(&asset_id).is_some());
    asset_id
}

fn issue_test_certificate(asset_id: [u8; 32], holder: AccountId) -> [u8; 32] {
    let issuer: AccountId = 1;
    assert_ok!(DataAssets::issue_certificate(
        RuntimeOrigin::signed(issuer),
        asset_id,
        holder,
        1,
        None,
    ));
    RightToken::generate_certificate_id(&asset_id, Timestamp::get(), &issuer, 0)
}

#[test]
fn revoke_certificate_removes_certificate_when_called_by_asset_owner() {
    new_test_ext().execute_with(|| {
        let asset_id = register_test_asset(1);
        let certificate_id = issue_test_certificate(asset_id, 2);

        assert!(DataAssets::get_certificate(&asset_id, &certificate_id).is_some());

        assert_ok!(DataAssets::revoke_certificate(
            RuntimeOrigin::signed(1),
            asset_id,
            certificate_id,
        ));

        assert!(DataAssets::get_certificate(&asset_id, &certificate_id).is_none());
        assert_eq!(trade_measurements(), vec![asset_id]);
        System::assert_has_event(RuntimeEvent::DataAssets(
            pallet_dataassets::Event::CertificateRevoked {
                asset_id,
                certificate_id,
            },
        ));
    });
}

#[test]
fn revoke_certificate_rejects_unrelated_account_and_allows_holder() {
    new_test_ext().execute_with(|| {
        let asset_id = register_test_asset(1);
        let certificate_id = issue_test_certificate(asset_id, 2);

        assert_noop!(
            DataAssets::revoke_certificate(RuntimeOrigin::signed(3), asset_id, certificate_id),
            pallet_dataassets::Error::<Test>::NotOwner,
        );
        assert!(DataAssets::get_certificate(&asset_id, &certificate_id).is_some());

        assert_ok!(DataAssets::revoke_certificate(
            RuntimeOrigin::signed(2),
            asset_id,
            certificate_id,
        ));

        assert!(DataAssets::get_certificate(&asset_id, &certificate_id).is_none());
    });
}

#[test]
fn on_finalize_writes_asset_and_certificate_roots_to_digest() {
    new_test_ext().execute_with(|| {
        let asset_id = register_test_asset(1);
        let _certificate_id = issue_test_certificate(asset_id, 2);

        assert!(pallet_dataassets::TrieModified::<Test>::get());
        let expected_asset_root = DataAssets::compute_asset_root();
        let expected_certificate_root = DataAssets::compute_certificate_root();

        <DataAssets as Hooks<u64>>::on_finalize(1);

        assert!(!pallet_dataassets::TrieModified::<Test>::get());
        let digest = System::digest();
        assert_eq!(
            DataAssets::get_asset_root_from_digest(&digest),
            Some(expected_asset_root),
        );
        assert_eq!(
            pallet_dataassets::digest_item::extract_certificate_root(&digest),
            Some(expected_certificate_root),
        );
        System::assert_has_event(RuntimeEvent::DataAssets(
            pallet_dataassets::Event::AssetRootUpdated {
                root: expected_asset_root,
            },
        ));
        System::assert_has_event(RuntimeEvent::DataAssets(
            pallet_dataassets::Event::CertificateRootUpdated {
                root: expected_certificate_root,
            },
        ));
    });
}
