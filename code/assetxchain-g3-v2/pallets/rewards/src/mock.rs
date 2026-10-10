use crate as pallet_rewards;
use frame_support::{
    parameter_types,
    derive_impl,
    traits::{ConstU128, ConstU32},
};
use sp_runtime::{
    BuildStorage,
};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        Rewards: pallet_rewards,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountData = pallet_balances::AccountData<u128>;
}

impl pallet_balances::Config for Test {
    type MaxLocks = ConstU32<50>;
    type MaxReserves = ();
    type ReserveIdentifier = [u8; 8];
    type Balance = u128;
    type RuntimeEvent = RuntimeEvent;
    type DustRemoval = ();
    type ExistentialDeposit = ConstU128<1>;
    type AccountStore = System;
    type WeightInfo = ();
    type FreezeIdentifier = ();
    type MaxFreezes = ();
    type RuntimeHoldReason = ();
    type RuntimeFreezeReason = ();
    type DoneSlashHandler = ();
}

parameter_types! {
    pub const RewardReceiverAccount: u64 = 123;
    pub const InitialReward: u128 = 5;
    pub const RewardAdjustmentThreshold: u128 = 250_000_000;
    pub const AdjustedReward: u128 = 1;
    pub const MaxSupply: u128 = 500_000_000;
}

impl pallet_rewards::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type RewardReceiver = RewardReceiverAccount;
    type InitialReward = InitialReward;
    type RewardAdjustmentThreshold = RewardAdjustmentThreshold;
    type AdjustedReward = AdjustedReward;
    type MaxSupply = MaxSupply;
    // 使用 lib.rs 中为 () 提供的默认 WeightInfo 实现
    type WeightInfo = crate::weights::WeightInfo<Test>;
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    let t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(1));
    ext
}