use crate as storage_ipfs;
use frame_support::{
    derive_impl, parameter_types,
    traits::{ConstU128, ConstU32},
};
use pallet_shared_traits::AssetQueryError;
use sp_runtime::BuildStorage;
use std::cell::RefCell;

pub type AccountId = u64;
pub type Balance = u128;
type Block = frame_system::mocking::MockBlock<Test>;

pub const ALICE: AccountId = 1;
pub const BOB: AccountId = 2;
pub const INCENTIVE_POOL: AccountId = 10;
pub const DESTRUCTION_POOL: AccountId = 11;
pub const IPFS_POOL: AccountId = 12;
pub const COMPENSATION_POOL: AccountId = 13;

thread_local! {
    static ASSET_OWNER: RefCell<Option<AccountId>> = RefCell::new(Some(ALICE));
}

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        Collaterals: pallet_collaterals,
        StorageIpfs: storage_ipfs,
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
    pub const MinMarketOperatorCollateral: Balance = 1_000;
    pub const MinIpfsProviderCollateral: Balance = 500;
    pub const MinGovernancePledge: Balance = 2_000;
    pub const IncentivePoolAccount: AccountId = INCENTIVE_POOL;
    pub const DestructionAccount: AccountId = DESTRUCTION_POOL;
    pub const IpfsPoolAccount: AccountId = IPFS_POOL;
    pub const CompensationPoolAccount: AccountId = COMPENSATION_POOL;
    pub const ProofPeriod: u64 = 10;
}

impl pallet_collaterals::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type MinMarketOperatorCollateral = MinMarketOperatorCollateral;
    type MinIpfsProviderCollateral = MinIpfsProviderCollateral;
    type MinGovernancePledge = MinGovernancePledge;
    type IncentivePoolAccount = IncentivePoolAccount;
    type DestructionAccount = DestructionAccount;
    type IpfsPoolAccount = IpfsPoolAccount;
    type CompensationPoolAccount = CompensationPoolAccount;
    type WeightInfo = pallet_collaterals::weights::WeightInfo<Test>;
}

pub struct TestDataAssetProvider;

impl pallet_shared_traits::DataAssetProvider<AccountId, [u8; 32]> for TestDataAssetProvider {
    fn get_asset_owner(_asset_id: &[u8; 32]) -> Result<AccountId, AssetQueryError> {
        ASSET_OWNER.with(|owner| owner.borrow().ok_or(AssetQueryError::AssetNotFound))
    }
}

impl storage_ipfs::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type DataAssetProvider = TestDataAssetProvider;
    type IpfsAvailabilityVerifier = ();
    type XcmAvailabilityVerifier = ();
    type ProofPeriod = ProofPeriod;
    type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();

    pallet_balances::GenesisConfig::<Test> {
        balances: vec![(ALICE, 10_000), (BOB, 10_000)],
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .unwrap();

    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| {
        System::set_block_number(1);
        System::reset_events();
        ASSET_OWNER.with(|owner| *owner.borrow_mut() = Some(ALICE));
    });
    ext
}

pub fn set_asset_owner(owner: Option<AccountId>) {
    ASSET_OWNER.with(|stored| *stored.borrow_mut() = owner);
}
