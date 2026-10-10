// This is free and unencumbered software released into the public domain.
//
// Anyone is free to copy, modify, publish, use, compile, sell, or
// distribute this software, either in source code form or as a compiled
// binary, for any purpose, commercial or non-commercial, and by any
// means.
//
// In jurisdictions that recognize copyright laws, the author or authors
// of this software dedicate any and all copyright interest in the
// software to the public domain. We make this dedication for the benefit
// of the public at large and to the detriment of our heirs and
// successors. We intend this dedication to be an overt act of
// relinquishment in perpetuity of all present and future rights to this
// software under copyright law.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
// OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
// OTHER DEALINGS IN THE SOFTWARE.
//
// For more information, please refer to <http://unlicense.org>

// Substrate and Polkadot dependencies
use frame_support::{
	derive_impl, parameter_types,
    PalletId,
	traits::{ConstU128, ConstU32, ConstU64, ConstU8, ConstBool, VariantCountOf, WithdrawReasons, Get},
	weights::{
		constants::{RocksDbWeight, WEIGHT_REF_TIME_PER_SECOND},
		IdentityFee, Weight,
	},
};
use frame_system::pallet::Pallet as SystemPallet;
use frame_system::limits::{BlockLength, BlockWeights};
use frame_system::EnsureSigned;
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use sp_runtime::traits::OpaqueKeys;
use sp_runtime::{traits::One, Perbill};
use sp_version::RuntimeVersion;

mod market_runtime_adapter;
use pallet_shared_traits::{IncentiveHandler, DataAssetProvider};
use market_runtime_adapter::MarketRuntimeAdapter;

// Local module imports
use super::{
	AccountId, Balance, Balances, Block, BlockNumber, Hash, Nonce, PalletInfo, Runtime,
	RuntimeCall, RuntimeEvent, RuntimeFreezeReason, RuntimeHoldReason, RuntimeOrigin, RuntimeTask, UncheckedExtrinsic,
	System, EXISTENTIAL_DEPOSIT, SLOT_DURATION, VERSION, DAYS, HOURS, MILLI_SECS_PER_BLOCK,
	Babe, SessionKeys, Vesting, DataAssets, Contracts, Validator,
};
use crate::{Incentive, UNIT, asset_market_extension};

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

parameter_types! {
    pub const BlockHashCount: BlockNumber = 2400;
    pub const Version: RuntimeVersion = VERSION;

    /// We allow for 2 seconds of compute with a 6 second average block time.
    pub RuntimeBlockWeights: BlockWeights = BlockWeights::with_sensible_defaults(
        Weight::from_parts(2u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX),
        NORMAL_DISPATCH_RATIO,
    );
    pub RuntimeBlockLength: BlockLength = BlockLength::max_with_normal_ratio(5 * 1024 * 1024, NORMAL_DISPATCH_RATIO);
    pub const SS58Prefix: u8 = 42;
}

/// The default types are being injected by [`derive_impl`](`frame_support::derive_impl`) from
/// [`SoloChainDefaultConfig`](`struct@frame_system::config_preludes::SolochainDefaultConfig`),
/// but overridden as needed.
#[derive_impl(frame_system::config_preludes::SolochainDefaultConfig)]
impl frame_system::Config for Runtime {
    /// The block type for the runtime.
    type Block = Block;
    /// Block & extrinsics weights: base values and limits.
    type BlockWeights = RuntimeBlockWeights;
    /// The maximum length of a block (in bytes).
    type BlockLength = RuntimeBlockLength;
    /// The identifier used to distinguish between accounts.
    type AccountId = AccountId;
    /// The type for storing how many extrinsics an account has signed.
    type Nonce = Nonce;
    /// The type for hashing blocks and tries.
    type Hash = Hash;
    /// Maximum number of block number to block hash mappings to keep (oldest pruned first).
    type BlockHashCount = BlockHashCount;
    /// The weight of database operations that the runtime can invoke.
    type DbWeight = RocksDbWeight;
    /// Version of the runtime.
    type Version = Version;
    /// The data to be stored in an account.
    type AccountData = pallet_balances::AccountData<Balance>;
    /// This is used as an identifier of the chain. 42 is the generic substrate prefix.
    type SS58Prefix = SS58Prefix;
    type MaxConsumers = frame_support::traits::ConstU32<16>;
}

// BABE参数
parameter_types! {
    // Epoch 持续时间（slot 数量）
    // 2400 个 slot × 6 秒 = 4 小时一个 epoch
    pub const EpochDuration: u64 = (6 * HOURS) as u64;
    // pub const EpochDuration: u64 = 10_000_000;

    // 期望的区块时间（毫秒）
    pub const ExpectedBlockTime: u64 = MILLI_SECS_PER_BLOCK;

    // 报告延迟
    pub const ReportLongevity: u64 =
        ((DAYS as u64) * 7) as u64;
}

// Session
parameter_types! {
    // pub const Period: u32 = 10_000_000;
    pub const Period: u32 = 6 * HOURS; // 一个 session 6 小时
    pub const Offset: u32 = 0;
}

impl pallet_babe::Config for Runtime {
    type EpochDuration = EpochDuration;
    type ExpectedBlockTime = ExpectedBlockTime;
    type EpochChangeTrigger = pallet_babe::ExternalTrigger;
    type DisabledValidators = (); //pallet_session::Pallet<Runtime>;
    type WeightInfo = ();
    type MaxAuthorities = frame_support::traits::ConstU32<32>;
    type MaxNominators = frame_support::traits::ConstU32<0>; // 暂时不使用 nominator
    type KeyOwnerProof = sp_core::Void;
    type EquivocationReportSystem = ();
}

pub struct ValidatorIdOf;
impl sp_runtime::traits::Convert<AccountId, Option<AccountId>> for ValidatorIdOf {
    fn convert(account: AccountId) -> Option<AccountId> {
        Some(account)
    }
}

impl pallet_session::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = AccountId;
    type ValidatorIdOf = ValidatorIdOf;
    type ShouldEndSession = pallet_session::PeriodicSessions<Period, Offset>;
    type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
    // type ShouldEndSession = Babe;
    // type NextSessionRotation = Babe;
    type SessionManager = Validator;
    type SessionHandler = <SessionKeys as OpaqueKeys>::KeyTypeIdProviders;
    type Keys = SessionKeys;
    type WeightInfo = ();
    type DisablingStrategy = ();
}

impl pallet_authorship::Config for Runtime {
    type FindAuthor = pallet_session::FindAccountFromAuthorIndex<Self, Babe>;
    type EventHandler = (); // 之后添加区块奖励处理
}

// impl pallet_aura::Config for Runtime {
// 	type AuthorityId = AuraId;
// 	type DisabledValidators = ();
// 	type MaxAuthorities = ConstU32<32>;
// 	type AllowMultipleBlocksPerSlot = ConstBool<false>;
// 	type SlotDuration = pallet_aura::MinimumPeriodTimesTwo<Runtime>;
// }

impl pallet_grandpa::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;

    type WeightInfo = ();
    type MaxAuthorities = ConstU32<32>;
    type MaxNominators = ConstU32<0>;
    type MaxSetIdSessionEntries = ConstU64<0>;

    type KeyOwnerProof = sp_core::Void;
    type EquivocationReportSystem = ();
}

impl pallet_timestamp::Config for Runtime {
    /// A timestamp: milliseconds since the unix epoch.
    type Moment = u64;
    type OnTimestampSet = Babe;
    type MinimumPeriod = ConstU64<{ SLOT_DURATION / 2 }>;
    type WeightInfo = ();
}

impl pallet_balances::Config for Runtime {
    type MaxLocks = ConstU32<50>; // 支持质押时的锁仓数量
    type MaxReserves = (); // 支持多池储备，目前未启用
    type ReserveIdentifier = [u8; 8]; // 区分不同的储备池的标识符（例如："DATA"）
    /// The type for recording an account's balance.
    type Balance = Balance;
    /// The ubiquitous event type.
    type RuntimeEvent = RuntimeEvent;
    type DustRemoval = (); // 粉尘清理，关闭
    type ExistentialDeposit = ConstU128<EXISTENTIAL_DEPOSIT>;
    type AccountStore = System;
    type WeightInfo = pallet_balances::weights::SubstrateWeight<Runtime>;
    type FreezeIdentifier = RuntimeFreezeReason;
    type MaxFreezes = VariantCountOf<RuntimeFreezeReason>;
    type RuntimeHoldReason = RuntimeHoldReason;
    type RuntimeFreezeReason = RuntimeFreezeReason;
    type DoneSlashHandler = ();
}

parameter_types! {
    pub FeeMultiplier: Multiplier = Multiplier::one();
}

impl pallet_transaction_payment::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type OnChargeTransaction = FungibleAdapter<Balances, ()>;
    type OperationalFeeMultiplier = ConstU8<5>;
    type WeightToFee = IdentityFee<Balance>;
    type LengthToFee = IdentityFee<Balance>;
    type FeeMultiplierUpdate = ConstFeeMultiplier<FeeMultiplier>;
    type WeightInfo = pallet_transaction_payment::weights::SubstrateWeight<Runtime>;
}

impl pallet_sudo::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = pallet_sudo::weights::SubstrateWeight<Runtime>;
}

/// Configure the pallet-template in pallets/template.
impl pallet_template::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = pallet_template::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
    /// Base collateral amount: 2000 DATA
    /// This is the minimum collateral required for any asset
    pub const BaseCollateral: Balance = 2_000 * UNIT;

    /// Collateral per MB: 100 DATA/MB
    /// Additional collateral based on data size
    pub const CollateralPerMB: Balance = 100 * UNIT;

    /// Maximum collateral cap: 75000 DATA
    /// Upper limit to prevent excessive collateral requirements
    pub const MaxCollateral: Balance = 75_000 * UNIT;

    /// Maximum number of release phases for collateral
    pub const MaxReleasePhases: u32 = 5;
}

impl pallet_dataassets::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;

    /// Use Balances pallet for collateral management
    type Currency = Balances;

    /// Collateral configuration
    type BaseCollateral = BaseCollateral;
    type CollateralPerMB = CollateralPerMB;
    type MaxCollateral = MaxCollateral;

    /// Asset metadata constraints
    type MaxNameLength = ConstU32<256>;
    type MaxDescriptionLength = ConstU32<1024>;

    type IncentiveHandler = Incentive;
    type WeightInfo = pallet_dataassets::weights::WeightInfo<Runtime>;
}

// 添加参数配置
parameter_types! {
    // 5年线性释放 (按区块计算: 5年 * 365天 * 24小时 * 60分钟 * 60秒 / 18秒每区块)
    // pub const FoundationVestingPeriod: BlockNumber = 5 * 365 * 24 * 60 * 60 / (MILLI_SECS_PER_BLOCK / 1000) as BlockNumber;
    pub const FoundationVestingPeriod: BlockNumber = (
        (5 * 365 * 24 * 60 * 60 * 1000u64) / (MILLI_SECS_PER_BLOCK as u64)
    ) as BlockNumber;
    pub const MinVestedTransfer: Balance = 1 * UNIT;
    pub const MaxVestingSchedules: u32 = 20;
    pub const AllowedWithdrawReasons: WithdrawReasons = WithdrawReasons::from_bits(
        WithdrawReasons::TRANSFER.bits() | WithdrawReasons::TRANSACTION_PAYMENT.bits()
    ).expect("Valid bits");
}

// 实现vesting配置
impl pallet_vesting::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type BlockNumberToBalance = sp_runtime::traits::ConvertInto;
    type MinVestedTransfer = MinVestedTransfer;
    type WeightInfo = pallet_vesting::weights::SubstrateWeight<Runtime>;
    // 使用预定义的常量代替直接在类型参数中计算
    type UnvestedFundsAllowedWithdrawReasons = AllowedWithdrawReasons;
    type BlockNumberProvider = SystemPallet<Runtime>;
    // 之前修复的 MAX_VESTING_SCHEDULES 常量
    const MAX_VESTING_SCHEDULES: u32 = MaxVestingSchedules::get();
}

parameter_types! {
    pub const InitialReward: Balance = 5 * UNIT;
    pub const RewardAdjustmentThreshold: Balance = 250_000_000 * UNIT;
    pub const AdjustedReward: Balance = 1 * UNIT;
    pub const MaxSupply: Balance = 500_000_000 * UNIT;
}

pub struct BlockAuthor;
impl Get<AccountId> for BlockAuthor {
    fn get() -> AccountId {
        pallet_authorship::Pallet::<Runtime>::author()
			.unwrap_or_else(|| AccountId::from([0u8; 32]))
    }
}

impl pallet_rewards::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type RewardReceiver = BlockAuthor;
    type InitialReward = InitialReward;
    type RewardAdjustmentThreshold = RewardAdjustmentThreshold;
    type AdjustedReward = AdjustedReward;
    type MaxSupply = MaxSupply;
    type WeightInfo = pallet_rewards::weights::WeightInfo<Runtime>;
}

parameter_types! {
    // 激励池初始余额：3亿 DAT (经济模型的30%)
    pub const InitialIncentivePool: Balance = 300_000_000 * UNIT;

    // 动态释放比例：1%/月
    pub const DynamicReleaseRatio: Perbill = Perbill::from_percent(1);

    // 数据创建者奖励参数
    pub const FirstCreateReward: Balance = 1_000 * UNIT; // 1000 DAT
    pub const QualityDataReward: Balance = 3_000 * UNIT; // 3000 DAT
    pub const LongTermShareRatio: Perbill = Perbill::from_perthousand(5); // 0.5%
    pub const QualityDataTradeThreshold: u32 = 10; // 10笔交易

    // 市场运营者奖励参数
    pub const TopMarketMonthlyReward: Balance = 50_000 * UNIT; // 5万 DAT

    // 交易者奖励参数
    pub const TraderRebateThreshold: Balance = 100_000 * UNIT; // 10万 DAT
    pub const TraderRebateRatio: Perbill = Perbill::from_percent(10); // 10%
    pub const LiquidityRewardRatio: Perbill = Perbill::from_perthousand(5); // 0.5‰

    // 治理参与者奖励参数
    pub const GovernanceVotingRewardTotal: Balance = 5_000 * UNIT; // 5000 DAT
    pub const GovernanceProposalReward: Balance = 2_000 * UNIT; // 2000 DAT

    // 验证节点奖励参数
    pub const ValidatorVerificationReward: Balance = 50 * UNIT; // 50 DAT
}

impl pallet_incentive::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type DataAssetProvider = DataAssets;
    // 激励池配置
    type InitialIncentivePool = InitialIncentivePool;
    type DynamicReleaseRatio = DynamicReleaseRatio;

    // 数据创建者奖励配置
    type FirstCreateReward = FirstCreateReward;
    type QualityDataReward = QualityDataReward;
    type LongTermShareRatio = LongTermShareRatio;
    type QualityDataTradeThreshold = QualityDataTradeThreshold;

    // 市场运营者奖励配置
    type TopMarketMonthlyReward = TopMarketMonthlyReward;

    // 交易者奖励配置
    type TraderRebateThreshold = TraderRebateThreshold;
    type TraderRebateRatio = TraderRebateRatio;
    type LiquidityRewardRatio = LiquidityRewardRatio;

    // 治理参与者奖励配置
    type GovernanceVotingRewardTotal = GovernanceVotingRewardTotal;
    type GovernanceProposalReward = GovernanceProposalReward;

    // 验证节点奖励配置
    type ValidatorVerificationReward = ValidatorVerificationReward;
    type WeightInfo = pallet_incentive::weights::WeightInfo<Runtime>;
}

// 合约模块的常量
pub const CENTS: Balance = UNIT / 100;
pub const MILLICENTS: Balance = CENTS / 1_000;

// 押金计算逻辑，这里的15和6使用的是硬编码，分别对应每个item和每个byte的押金
pub const fn deposit(items: u32, bytes: u32) -> Balance {
    items as Balance * 15 * CENTS + (bytes as Balance) * 6 * CENTS
}

parameter_types! {
    pub const DepositPerItem: Balance = deposit(1, 0);
    pub const DepositPerByte: Balance = deposit(0, 1);
    // 合约存储可以删除的队列深度
    pub const DeletionQueueDepth: u32 = 128;
    // 每次区块可以删除的最大存储项权重
    pub const DeletionWeightLimit: Weight = Weight::from_parts(500_000_000, 0);
    pub const DefaultDepositLimit: Balance = deposit(1024, 1024 * 1024);
    pub const MaxCodeLen: u32 = 120 * 1024;
    pub const MaxStorageKeyLen: u32 = 128;
    pub const MaxDebugBufferLen: u32 = 2 * 1024 * 1024;
    // UnsafeUnstableInterface用于开启实验性功能，生产环境y一般设为false
    pub const UnsafeUnstableInterface: bool = true;
    pub const CodeHashLockupDepositPercent: Perbill = Perbill::from_percent(30);
}

// 合约模块的调度表
pub struct ContractsSchedule;
impl frame_support::traits::Get<pallet_contracts::Schedule<Runtime>> for ContractsSchedule {
    fn get() -> pallet_contracts::Schedule<Runtime> {
        pallet_contracts::Schedule::<Runtime>::default()
    }
}

// pallet_babe::ParentBlockRandomness<Runtime>返回Randomness<Option<H256>>，
// 但是需要Randomness<H256>，因此需要一个适配器来处理为空的情况
pub struct BabeRandomnessAdapter;
impl frame_support::traits::Randomness<Hash, BlockNumber> for BabeRandomnessAdapter {
    fn random(subject: &[u8]) -> (Hash, BlockNumber) {
        // 调用 Babe 的随机数，如果为空则返回默认Hash(0x00...)
        let (hash, block) = pallet_babe::ParentBlockRandomness::<Runtime>::random(subject);
        (hash.unwrap_or_default(), block)
    }
}

impl pallet_contracts::Config for Runtime {
    type Time = pallet_timestamp::Pallet<Runtime>;
    // 使用 pallet_babe或pallet_rand作为随机数源
    type Randomness = BabeRandomnessAdapter;
    type Currency = Balances;
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;

    // 允许所有合约调用，不设置过滤器
    type CallFilter = frame_support::traits::Nothing;

    // 将权重转换为费用，使用 transaction-payment 模块
    type WeightPrice = pallet_transaction_payment::Pallet<Self>;
    type WeightInfo = pallet_contracts::weights::SubstrateWeight<Self>;

    // 链扩展 (Chain Extension)，如果不需要自定义与 Runtime 交互的功能，设为 ()
    // 现在需要智能合约可以影响到数据资产的状态，因此需要引入数据资产扩展模块
    type ChainExtension = asset_market_extension::DataAssetsExtension;

    type Schedule = ContractsSchedule;
    type CallStack = [pallet_contracts::Frame<Self>; 5]; // 调用栈深度
    type DepositPerItem = DepositPerItem;
    type DepositPerByte = DepositPerByte;
    type DefaultDepositLimit = DefaultDepositLimit;
    type AddressGenerator = pallet_contracts::DefaultAddressGenerator;

    // 代码与存储限制
    type MaxCodeLen = ConstU32<{ 120 * 1024 }>;
    type MaxStorageKeyLen = ConstU32<128>;

    // 开启不安全/不稳定接口 (仅限开发网或特定需求)
    type UnsafeUnstableInterface = ConstBool<true>;

    // 调试缓冲区大小
    type MaxDebugBufferLen = ConstU32<{ 2 * 1024 * 1024 }>;

    // 上传代码的权限：EnsureSigned表示允许任何签名账户上传
    type UploadOrigin = EnsureSigned<Self::AccountId>;

    // 实例化合约的权限：允许任何签名账户实例化
    type InstantiateOrigin = EnsureSigned<Self::AccountId>;

    // 代码哈希锁定存款比例：通常设置为30%或0%
    type CodeHashLockupDepositPercent = CodeHashLockupDepositPercent;

    // 最大代理依赖数：防止深层递归攻击
    type MaxDelegateDependencies = ConstU32<32>;

    // 最大瞬时存储大小
    type MaxTransientStorageSize = ConstU32<{ 2 * 1024 * 1024 }>; // 2MB
    type RuntimeHoldReason = RuntimeHoldReason;
    type ApiVersion = ();

    type Migrations = ();
    type Xcm = ();
    type Environment = ();

    type Debug = ();
}

parameter_types! {
    pub const MarketsPalletId: PalletId = PalletId(*b"da/mrket");
    pub const MaxMarketId: u32 = u32::MAX;
    pub const MaxListingId: u32 = u32::MAX;
}

impl pallet_markets::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type MarketWeightInfo = pallet_markets::weights::WeightInfo<Runtime>;
}

parameter_types! {
    pub const MinValidatorBond: Balance = 1_000 * UNIT; // 先质押1000DAT
    pub const MaxValidators: u32 = 100;
}

impl pallet_validator::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type AddRemoveOrigin = frame_system::EnsureRoot<AccountId>; // 设为Root权限，开发者-->超级管理
    type MinValidatorBond = MinValidatorBond;
    type MaxValidators = MaxValidators;
    type ValidatorIdOf = ValidatorIdOf;
    type IdentificationOf = ValidatorIdOf;
}

parameter_types! {
    pub const UnsignedPriority: u64 = 1 << 20;
}

impl pallet_im_online::Config for Runtime {
    type AuthorityId = sp_consensus_babe::AuthorityId;
    type RuntimeEvent = RuntimeEvent;
    type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
    // 将pallet_validator设为验证者集合提供者
    type ValidatorSet = Validator;
    // 将pallet_validator设为违规报告接收者
    type ReportUnresponsiveness = Validator;

    type UnsignedPriority = UnsignedPriority;
    type WeightInfo = pallet_im_online::weights::SubstrateWeight<Runtime>;
    type MaxKeys = MaxValidators; // 使用定义的上限
    type MaxPeerInHeartbeats = ConstU32<0>; // Solo Chain 模式下通常设为 0,测试网5-10
}

// 告诉系统如何为im-online的调用创建基本的交易结构
impl frame_system::offchain::CreateTransactionBase<pallet_im_online::Call<Runtime>> for Runtime {
    type RuntimeCall = RuntimeCall;
    // 使用Block中定义的Extrinsic类型
    type Extrinsic = UncheckedExtrinsic;
}

impl frame_system::offchain::CreateInherent<pallet_im_online::Call<Runtime>> for Runtime {
    
    fn create_bare(call: RuntimeCall) -> Self::Extrinsic {

        // new_bare 接收的是 RuntimeCall
        sp_runtime::generic::UncheckedExtrinsic::new_bare(
            call
        ).into()
    }

    fn create_inherent(call: RuntimeCall) -> Self::Extrinsic {
        Self::create_bare(call)
    }
}

parameter_types! {
    pub const MinMarketOperatorCollateral: Balance = 10_000 * UNIT;
    pub const MinIpfsProviderCollateral: Balance = 5_000 * UNIT;
    pub const MinGovernancePledge: Balance = 20_000 * UNIT;

    // 资金池账户
    pub const DestructionAccount: AccountId = AccountId::new([0u8; 32]);
    pub const IncentivePoolAccount: AccountId = AccountId::new(hex_literal::hex!(
        "1a9de66d5ca5a6a7bad9add630d85b972f351082b0422e5f64c78a4eecc4a427"
    ));
    pub const IpfsPoolAccount: AccountId = AccountId::new([2u8; 32]);
    pub const CompensationPoolAccount: AccountId = AccountId::new([3u8; 32]);
}

impl pallet_collaterals::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances; // 使用 balances 模块进行质押

    type MinMarketOperatorCollateral = MinMarketOperatorCollateral;
    type MinIpfsProviderCollateral = MinIpfsProviderCollateral;
    type MinGovernancePledge = MinGovernancePledge;

    type IncentivePoolAccount = IncentivePoolAccount;
    type DestructionAccount = DestructionAccount;
    type IpfsPoolAccount = IpfsPoolAccount;
    type CompensationPoolAccount = CompensationPoolAccount;

    type WeightInfo = pallet_collaterals::weights::WeightInfo<Runtime>;
}

parameter_types! {
    pub const MaxActiveOrdersPerAsset: u32 = 1;
    pub const MarketRuntimePalletId: frame_support::PalletId = frame_support::PalletId(*b"da/mkx02");
}

impl pallet_market_runtime::Config for Runtime {
    type PalletId = MarketRuntimePalletId;
    type DataAssets = MarketRuntimeAdapter;
    type MaxActiveOrdersPerAsset = MaxActiveOrdersPerAsset;
}

impl crate::custom_header::AssetsStateRootProvider<sp_runtime::traits::BlakeTwo256> for Runtime {
    fn compute_assets_state_root() -> sp_core::H256 {
        pallet_dataassets::Pallet::<Runtime>::compute_asset_root()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn collateral_incentive_pool_account_matches_genesis_funding_account() {
        assert_eq!(
            IncentivePoolAccount::get(),
            AccountId::new(hex!(
                "1a9de66d5ca5a6a7bad9add630d85b972f351082b0422e5f64c78a4eecc4a427"
            )),
        );
    }
}
