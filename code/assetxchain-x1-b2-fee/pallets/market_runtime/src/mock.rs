use crate as pallet_market_runtime;
use frame_support::{derive_impl, parameter_types};
use sp_core::{sr25519, Pair};
use sp_runtime::BuildStorage;

pub type AccountId = u64;
type Block = frame_system::mocking::MockBlock<Test>;

pub const OWNER: AccountId = 1;
pub const RIGHT_SELLER: AccountId = 2;
pub const BUYER: AccountId = 3;
pub const MARKET_OPERATOR: AccountId = 4;
pub const OTHER: AccountId = 5;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        MarketRuntime: pallet_market_runtime,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountData = pallet_balances::AccountData<u128>;
}

parameter_types! {
    pub const MaxRightsPerAsset: u32 = 32;
    pub const ExistentialDeposit: u128 = 1;
    pub const FundedGraceBlocks: u32 = 10;
}

impl pallet_balances::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type Balance = u128;
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
    type DustRemoval = ();
    type WeightInfo = ();
    type MaxLocks = frame_support::traits::ConstU32<0>;
    type MaxReserves = frame_support::traits::ConstU32<0>;
    type ReserveIdentifier = [u8; 8];
    type FreezeIdentifier = ();
    type MaxFreezes = frame_support::traits::ConstU32<0>;
    type RuntimeHoldReason = ();
    type RuntimeFreezeReason = ();
    type DoneSlashHandler = ();
}

pub struct TestAccountConverter;

pub fn test_pair(account: AccountId) -> sr25519::Pair {
    let uri = match account {
        OWNER => "//Alice",
        RIGHT_SELLER => "//Bob",
        BUYER => "//Charlie",
        MARKET_OPERATOR => "//Dave",
        OTHER => "//Eve",
        _ => return sr25519::Pair::from_seed(&[account as u8; 32]),
    };
    sr25519::Pair::from_string(uri, None).unwrap()
}

impl crate::ProtocolAccountIdConverter<AccountId> for TestAccountConverter {
    fn to_protocol_account_id(account: &AccountId) -> Option<crate::ProtocolAccountId> {
        Some(test_pair(*account).public().0)
    }

    fn from_protocol_account_id(account: &crate::ProtocolAccountId) -> Option<AccountId> {
        (OWNER..=OTHER).find(|candidate| test_pair(*candidate).public().0 == *account)
    }
}

impl pallet_market_runtime::Config for Test {
    type Currency = Balances;
    type ProtocolAccountIdConverter = TestAccountConverter;
    type FundedGraceBlocks = FundedGraceBlocks;
    type MaxRightsPerAsset = MaxRightsPerAsset;
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    pallet_balances::GenesisConfig::<Test> {
        balances: vec![
            (OWNER, 100_000_000_000_000),
            (RIGHT_SELLER, 100_000_000_000_000),
            (BUYER, 100_000_000_000_000),
            (MARKET_OPERATOR, 100_000_000_000_000),
            (OTHER, 100_000_000_000_000),
        ],
        dev_accounts: None,
    }
    .assimilate_storage(&mut storage)
    .unwrap();
    let mut ext: sp_io::TestExternalities = storage.into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}
