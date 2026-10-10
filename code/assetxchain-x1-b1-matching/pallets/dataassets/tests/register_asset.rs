use frame_support::{
    assert_noop, assert_ok, construct_runtime, derive_impl, parameter_types,
    traits::{ConstU128, ConstU32},
};
use pallet_dataassets::types::{AssetStatus, CollateralStatus, DataAsset, EncryptionInfo};
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
        balances: vec![(1, 1_000), (5, 5)],
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

fn first_create_rewards() -> Vec<([u8; 32], AccountId)> {
    FIRST_CREATE_REWARDS.with(|rewards| rewards.borrow().clone())
}

fn trade_measurements() -> Vec<[u8; 32]> {
    TRADE_MEASUREMENTS.with(|measurements| measurements.borrow().clone())
}

#[test]
fn register_asset_stores_asset_locks_collateral_and_emits_events() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 1;
        let raw_data_hash = H256::repeat_byte(0x33);
        let expected_collateral = BaseCollateral::get() + CollateralPerMb::get();

        assert_ok!(DataAssets::register_asset(
            RuntimeOrigin::signed(owner),
            b"asset".to_vec(),
            b"description".to_vec(),
            raw_data_hash,
            1024 * 1024,
        ));

        let asset_id = DataAsset::generate_asset_id(&owner, Timestamp::get(), &raw_data_hash);
        let asset = DataAssets::get_asset(&asset_id).expect("registered asset should be stored");
        assert_eq!(asset.core.asset_id, asset_id);
        assert_eq!(asset.core.token_id, 0);
        assert_eq!(asset.core.owner, owner);
        assert_eq!(asset.core.raw_data_hash, raw_data_hash);
        assert_eq!(asset.core.timestamp, Timestamp::get());
        assert_eq!(asset.core.updated_at, Timestamp::get());
        assert_eq!(asset.core.status, AssetStatus::Private);
        assert_eq!(asset.metadata.name.to_vec(), b"asset".to_vec());
        assert_eq!(asset.metadata.description.to_vec(), b"description".to_vec());

        let by_token_id = DataAssets::get_asset_by_token_id(0).unwrap();
        assert_eq!(by_token_id.core.asset_id, asset_id);

        let collateral = DataAssets::asset_collateral(asset_id).unwrap();
        assert_eq!(collateral.depositor, owner);
        assert_eq!(collateral.total_amount, expected_collateral);
        assert_eq!(collateral.reserved_amount, expected_collateral);
        assert_eq!(collateral.released_amount, 0);
        assert_eq!(collateral.release_schedule.len(), 3);
        assert_eq!(collateral.status, CollateralStatus::FullyLocked);
        assert_eq!(Balances::reserved_balance(owner), expected_collateral);

        assert_eq!(first_create_rewards(), vec![(asset_id, owner)]);
        assert!(trade_measurements().is_empty());

        System::assert_has_event(RuntimeEvent::DataAssets(
            pallet_dataassets::Event::CollateralLocked {
                asset_id,
                depositor: owner,
                amount: expected_collateral,
            },
        ));
        System::assert_has_event(RuntimeEvent::DataAssets(
            pallet_dataassets::Event::AssetRegistered {
                asset_id,
                token_id: 0,
                owner,
                collateral: expected_collateral,
            },
        ));
    });
}

#[test]
fn register_asset_with_metadata_stores_offchain_location_metadata() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 1;
        let raw_data_hash = H256::repeat_byte(0x99);
        let metadata_cid = b"bafybeimetadata".to_vec();
        let data_cid = b"bafybeidata".to_vec();
        let data_size_bytes = 7 * 1024 * 1024;
        let encryption_info = EncryptionInfo {
            algorithm: b"AES-256-GCM".to_vec().try_into().unwrap(),
            key_length: 256,
            parameters_hash: H256::repeat_byte(0x55),
            is_encrypted: true,
        };

        assert_ok!(DataAssets::register_asset_with_metadata(
            RuntimeOrigin::signed(owner),
            b"asset".to_vec(),
            b"description".to_vec(),
            raw_data_hash,
            data_size_bytes,
            metadata_cid.clone(),
            data_cid.clone(),
            encryption_info.clone(),
        ));

        let asset_id = DataAsset::generate_asset_id(&owner, Timestamp::get(), &raw_data_hash);
        let asset = DataAssets::get_asset(&asset_id).expect("registered asset should be stored");

        assert_eq!(asset.metadata.metadata_cid.to_vec(), metadata_cid);
        assert_eq!(asset.metadata.data_cid.to_vec(), data_cid);
        assert_eq!(asset.metadata.data_size_bytes, data_size_bytes);
        assert_eq!(asset.encryption_info, encryption_info);

        assert_eq!(asset.core.owner, owner);
        assert_eq!(asset.core.status, AssetStatus::Private);
    });
}

#[test]
fn register_asset_rejects_name_that_exceeds_configured_limit() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            DataAssets::register_asset(
                RuntimeOrigin::signed(1),
                vec![b'a'; 257],
                b"description".to_vec(),
                H256::repeat_byte(0x44),
                1024 * 1024,
            ),
            pallet_dataassets::Error::<Test>::NameTooLong,
        );

        assert_eq!(Balances::reserved_balance(1), 0);
        assert!(first_create_rewards().is_empty());
        assert!(System::events().is_empty());
    });
}

#[test]
fn register_asset_rejects_description_that_exceeds_configured_limit() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            DataAssets::register_asset(
                RuntimeOrigin::signed(1),
                b"asset".to_vec(),
                vec![b'd'; 1025],
                H256::repeat_byte(0x55),
                1024 * 1024,
            ),
            pallet_dataassets::Error::<Test>::DescriptionTooLong,
        );

        assert_eq!(Balances::reserved_balance(1), 0);
        assert!(first_create_rewards().is_empty());
        assert!(System::events().is_empty());
    });
}

#[test]
fn register_asset_rejects_account_with_insufficient_balance() {
    new_test_ext().execute_with(|| {
        let owner: AccountId = 5;
        let raw_data_hash = H256::repeat_byte(0x66);
        let asset_id = DataAsset::generate_asset_id(&owner, Timestamp::get(), &raw_data_hash);

        assert_noop!(
            DataAssets::register_asset(
                RuntimeOrigin::signed(owner),
                b"asset".to_vec(),
                b"description".to_vec(),
                raw_data_hash,
                1024 * 1024,
            ),
            pallet_dataassets::Error::<Test>::InsufficientBalance,
        );

        assert!(DataAssets::get_asset(&asset_id).is_none());
        assert!(DataAssets::asset_collateral(asset_id).is_none());
        assert_eq!(Balances::reserved_balance(owner), 0);
        assert!(first_create_rewards().is_empty());
        assert!(System::events().is_empty());
    });
}
