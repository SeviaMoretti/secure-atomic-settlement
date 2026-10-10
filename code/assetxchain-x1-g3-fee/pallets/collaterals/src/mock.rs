use crate as pallet_collaterals;
use frame_support::{
    derive_impl, parameter_types,
    traits::{ConstU128, ConstU32},
};
use sp_runtime::BuildStorage;

pub type AccountId = u64;
pub type Balance = u128;
type Block = frame_system::mocking::MockBlock<Test>;

pub const ALICE: AccountId = 1;
pub const INCENTIVE_POOL: AccountId = 10;
pub const DESTRUCTION_POOL: AccountId = 11;
pub const IPFS_POOL: AccountId = 12;
pub const COMPENSATION_POOL: AccountId = 13;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        Collaterals: pallet_collaterals,
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

pub fn new_test_ext() -> sp_io::TestExternalities {
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();

    pallet_balances::GenesisConfig::<Test> {
        balances: vec![(ALICE, 10_000)],
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .unwrap();

    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| {
        System::set_block_number(1);
        System::reset_events();
    });
    ext
}
