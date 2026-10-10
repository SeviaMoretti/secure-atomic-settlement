use crate as pallet_incentive;
use frame_support::{
    derive_impl, parameter_types,
    traits::{ConstU128, ConstU32},
    weights::Weight,
};
use pallet_shared_traits::AssetQueryError;
use sp_core::H256;
use sp_runtime::{BuildStorage, Perbill};
use std::cell::RefCell;

pub type AccountId = u64;
pub type Balance = u128;
type Block = frame_system::mocking::MockBlock<Test>;

pub const ALICE: AccountId = 1;
pub const BOB: AccountId = 2;

thread_local! {
    static ASSET_OWNER: RefCell<Option<AccountId>> = RefCell::new(Some(ALICE));
}

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        Incentive: pallet_incentive,
    }
);

pub struct TestWeightInfo;

impl pallet_incentive::pallet::WeightInfo for TestWeightInfo {
    fn trigger_dynamic_release() -> Weight {
        Weight::zero()
    }

    fn distribute_quality_data_reward() -> Weight {
        Weight::zero()
    }

    fn register_market_monthly_volume() -> Weight {
        Weight::zero()
    }

    fn register_voting_weight() -> Weight {
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

pub struct TestDataAssetProvider;

impl pallet_shared_traits::DataAssetProvider<AccountId, [u8; 32]> for TestDataAssetProvider {
    fn get_asset_owner(_asset_id: &[u8; 32]) -> Result<AccountId, AssetQueryError> {
        ASSET_OWNER.with(|owner| owner.borrow().ok_or(AssetQueryError::AssetNotFound))
    }
}

parameter_types! {
    pub const InitialIncentivePool: Balance = 1_100;
    pub const DynamicReleaseRatio: Perbill = Perbill::from_percent(10);
    pub const FirstCreateReward: Balance = 100;
    pub const QualityDataReward: Balance = 300;
    pub const LongTermShareRatio: Perbill = Perbill::from_perthousand(5);
    pub const QualityDataTradeThreshold: u32 = 3;
    pub const TopMarketMonthlyReward: Balance = 50;
    pub const TraderRebateThreshold: Balance = 1_000;
    pub const TraderRebateRatio: Perbill = Perbill::from_percent(10);
    pub const LiquidityRewardRatio: Perbill = Perbill::from_percent(1);
    pub const GovernanceVotingRewardTotal: Balance = 50;
    pub const GovernanceProposalReward: Balance = 25;
    pub const ValidatorVerificationReward: Balance = 5;
}

impl pallet_incentive::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type DataAssetProvider = TestDataAssetProvider;
    type InitialIncentivePool = InitialIncentivePool;
    type DynamicReleaseRatio = DynamicReleaseRatio;
    type FirstCreateReward = FirstCreateReward;
    type QualityDataReward = QualityDataReward;
    type LongTermShareRatio = LongTermShareRatio;
    type QualityDataTradeThreshold = QualityDataTradeThreshold;
    type TopMarketMonthlyReward = TopMarketMonthlyReward;
    type TraderRebateThreshold = TraderRebateThreshold;
    type TraderRebateRatio = TraderRebateRatio;
    type LiquidityRewardRatio = LiquidityRewardRatio;
    type GovernanceVotingRewardTotal = GovernanceVotingRewardTotal;
    type GovernanceProposalReward = GovernanceProposalReward;
    type ValidatorVerificationReward = ValidatorVerificationReward;
    type WeightInfo = TestWeightInfo;
}

pub fn pool_account() -> AccountId {
    crate::incentive_pool_account::<Test>()
}

pub fn asset_id(seed: u8) -> [u8; 32] {
    H256::repeat_byte(seed).into()
}

pub fn new_test_ext(pool_balance: Balance) -> sp_io::TestExternalities {
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();

    pallet_balances::GenesisConfig::<Test> {
        balances: vec![(pool_account(), pool_balance), (ALICE, 10), (BOB, 10)],
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .unwrap();

    pallet_incentive::GenesisConfig::<Test> {
        _marker: Default::default(),
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
