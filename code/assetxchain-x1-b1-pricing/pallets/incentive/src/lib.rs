//! # Incentive Pallet
//!
//! 激励模块，负责经济模型中所有角色的奖励发放、激励池管理、周期性奖励统计，
//! 与 dataassets 模块对接，支持治理参数动态调整。
//!
//! ## 功能
//! - 激励池余额管理（初始化、动态释放、余额校验）
//! - 数据创建者奖励{首次创建、优质数据、长期分成（分帐！！！在dataassets中业务逻辑中实现）}
//! - 市场运营者奖励（优质市场月度奖励，手续费（在业务逻辑中实现，分帐））
//! - 交易者奖励（手续费返还、流动性奖励）
//! - 治理参与者奖励（投票奖励、提案通过奖励）
//! - 验证节点奖励（元证验证奖励，这个从asset的验证激励池中分发）
//! - 治理参数更新（支持修改奖励金额、比例、阈值）

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use alloc::vec::Vec;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

pub mod weights;

pub use pallet::*;
use frame_support::{
    pallet_prelude::*,
    traits::{Currency, ReservableCurrency, Get, StorageVersion, ExistenceRequirement},
};
use frame_system::pallet_prelude::*;
use sp_runtime::{
    traits::{Saturating, CheckedDiv},
    Perbill,
};
use hex_literal::hex;

type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

// 激励池账户（固定地址）- 使用更通用的方式
fn incentive_pool_account<T: Config>() -> T::AccountId {
    let raw_account: [u8; 32] = hex!("1a9de66d5ca5a6a7bad9add630d85b972f351082b0422e5f64c78a4eecc4a427");
    T::AccountId::decode(&mut &raw_account[..])
        .unwrap_or_else(|_| panic!("Failed to decode incentive pool account"))
}

// 存储版本（用于后续升级）
const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);
// 月度奖励触发间隔（按区块计算：18秒/块 / 24×3600秒/天 ×30天 ≈ 144000块）
const MONTH_BLOCKS: u32 = 144000;
type AssetId = [u8; 32];

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use pallet_shared_traits::DataAssetProvider;

    pub trait WeightInfo {
        fn trigger_dynamic_release() -> Weight;
        fn distribute_quality_data_reward() -> Weight;
        fn register_market_monthly_volume() -> Weight;
        fn register_voting_weight() -> Weight;
    }

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    /// 配置Trait
    #[pallet::config]
    pub trait Config: frame_system::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        
        /// 货币类型
        type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;
        
        type DataAssetProvider: pallet_shared_traits::DataAssetProvider<Self::AccountId, AssetId>;

        /// 激励池初始余额（3亿DAT，对应经济模型30%总量）
        #[pallet::constant]
        type InitialIncentivePool: Get<BalanceOf<Self>>;
        
        /// 动态释放比例（按生态活跃度，默认1%/月）
        #[pallet::constant]
        type DynamicReleaseRatio: Get<Perbill>;
        
        // -------------------------- 奖励参数配置 --------------------------
        /// 数据创建者：首次创建元证奖励（默认1000DAT）
        #[pallet::constant]
        type FirstCreateReward: Get<BalanceOf<Self>>;
        
        /// 数据创建者：优质数据奖励（默认3000DAT）
        #[pallet::constant]
        type QualityDataReward: Get<BalanceOf<Self>>;
        
        /// 数据创建者：长期分成比例（默认0.5%）
        #[pallet::constant]
        type LongTermShareRatio: Get<Perbill>;
        
        /// 优质数据阈值：30天内权证交易≥N笔（默认10笔）
        #[pallet::constant]
        type QualityDataTradeThreshold: Get<u32>;
        
        /// 市场运营者：优质市场月度奖励（默认50000DAT）
        #[pallet::constant]
        type TopMarketMonthlyReward: Get<BalanceOf<Self>>;
        
        /// 交易者：手续费返还阈值（月交易额≥N DAT，默认10万）
        #[pallet::constant]
        type TraderRebateThreshold: Get<BalanceOf<Self>>;
        
        /// 交易者：手续费返还比例（默认10%）
        #[pallet::constant]
        type TraderRebateRatio: Get<Perbill>;
        
        /// 交易者：流动性奖励比例（默认0.05%）
        #[pallet::constant]
        type LiquidityRewardRatio: Get<Perbill>;
        
        /// 治理参与者：月度投票奖励总额（默认5000DAT）
        #[pallet::constant]
        type GovernanceVotingRewardTotal: Get<BalanceOf<Self>>;
        
        /// 治理参与者：提案通过奖励（默认2000DAT）
        #[pallet::constant]
        type GovernanceProposalReward: Get<BalanceOf<Self>>;
        
        /// 验证节点：元证验证奖励（默认50DAT/次）
        #[pallet::constant]
        type ValidatorVerificationReward: Get<BalanceOf<Self>>;

        type WeightInfo: WeightInfo;
    }

    // -------------------------- 存储 --------------------------
    // !!!!!怎么判断市场流动性（活力）不足
    /// 激励池已释放总额（用于动态释放计算）
    #[pallet::storage]
    #[pallet::getter(fn incentive_pool_released)]
    pub type IncentivePoolReleased<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    /// 激励池已使用总额（已发放的奖励总额）
    #[pallet::storage]
    #[pallet::getter(fn incentive_pool_used)]
    pub type IncentivePoolUsed<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    /// 激励池已锁定总额（未释放部分）
    #[pallet::storage]
    #[pallet::getter(fn incentive_pool_reserved)]
    pub type IncentivePoolReserved<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

    /// 记录账户是否首次创建元证（防止重复发放奖励）
    #[pallet::storage]
    #[pallet::getter(fn has_first_create_reward)]
    pub type HasFirstCreateReward<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        bool,
        ValueQuery,
    >;

    /// 元证交易统计（用于优质数据判定）：(asset_id, 30天内交易笔数)
    #[pallet::storage]
    #[pallet::getter(fn asset_30d_trade_count)]
    pub type Asset30dTradeCount<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        AssetId,
        u32,
        ValueQuery,
    >;

    /// 市场月交易额统计（用于优质市场判定）：(market_id, 月交易额)
    #[pallet::storage]
    #[pallet::getter(fn market_monthly_volume)]
    pub type MarketMonthlyVolume<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32],
        BalanceOf<T>,
        ValueQuery,
    >;

    /// 交易者月交易额统计（用于手续费返还）：(trader_account, 月交易额)
    #[pallet::storage]
    #[pallet::getter(fn trader_monthly_volume)]
    pub type TraderMonthlyVolume<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        BalanceOf<T>,
        ValueQuery,
    >;

    /// 治理投票权重统计（用于月度投票奖励分配）：(voter_account, 投票权重)
    #[pallet::storage]
    #[pallet::getter(fn governance_voting_weight)]
    pub type GovernanceVotingWeight<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        BalanceOf<T>,
        ValueQuery,
    >;

    /// 最后一次月度奖励发放的区块号
    #[pallet::storage]
    #[pallet::getter(fn last_monthly_reward_block)]
    pub type LastMonthlyRewardBlock<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

    // -------------------------- 事件 --------------------------
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// 激励池初始化成功
        IncentivePoolInitialized { balance: BalanceOf<T>, pool_account: T::AccountId },
        
        /// 激励池动态释放成功
        IncentivePoolReleased { amount: BalanceOf<T>, new_balance: BalanceOf<T>, pool_account: T::AccountId },
        
        /// 数据创建者：首次创建元证奖励发放
        FirstCreateRewardDistributed { recipient: T::AccountId, amount: BalanceOf<T>, asset_id: AssetId, pool_account: T::AccountId },
        
        /// 数据创建者：优质数据奖励发放
        QualityDataRewardDistributed { recipient: T::AccountId, amount: BalanceOf<T>, asset_id: AssetId, pool_account: T::AccountId },
                
        /// 市场运营者：优质市场月度奖励发放
        TopMarketRewardDistributed { recipient: T::AccountId, amount: BalanceOf<T>, market_id: [u8; 32], pool_account: T::AccountId },
        
        /// 交易者：手续费返还发放
        TraderRebateDistributed { recipient: T::AccountId, amount: BalanceOf<T>, monthly_volume: BalanceOf<T>, pool_account: T::AccountId },
        
        /// 交易者：流动性奖励发放
        LiquidityRewardDistributed { recipient: T::AccountId, amount: BalanceOf<T>, order_amount: BalanceOf<T>, pool_account: T::AccountId },
        
        /// 治理参与者：投票奖励发放
        GovernanceVotingRewardDistributed { recipient: T::AccountId, amount: BalanceOf<T>, weight: BalanceOf<T>, pool_account: T::AccountId },
        
        /// 治理参与者：提案通过奖励发放
        GovernanceProposalRewardDistributed { recipient: T::AccountId, amount: BalanceOf<T>, pool_account: T::AccountId },
                
        /// 激励池余额不足，奖励发放失败
        IncentivePoolInsufficientBalance { required: BalanceOf<T>, available: BalanceOf<T>, pool_account: T::AccountId },
        
        /// 奖励参数更新（治理操作）
        RewardParameterUpdated { parameter_name: Vec<u8>, old_value: Vec<u8>, new_value: Vec<u8>, pool_account: T::AccountId },
    }

    // -------------------------- 错误定义 --------------------------
    #[pallet::error]
    pub enum Error<T> {
        /// 激励池余额不足
        InsufficientIncentivePoolBalance,
        
        /// 已领取过首次创建奖励
        FirstCreateRewardAlreadyClaimed,
        
        /// 未满足优质数据奖励条件（交易笔数不足）
        QualityDataConditionNotMet,
        
        /// 未满足交易者手续费返还条件（交易额不足）
        TraderRebateConditionNotMet,
        
        /// 元证不存在
        AssetNotFound,

        /// 资产所有者账户不为空但不存在
        OwnerAccountDoesNotExist,

        /// 资产所有者账户为空
        OwnerAccountIsEmpty,
        
        /// 市场不存在
        MarketNotFound,
        
        /// 参数更新权限不足（需治理权限）
        UnauthorizedToUpdateParameter,
        
        /// 参数值无效（如比例超过100%）
        InvalidParameterValue,
    }

    // 定义创世配置结构体（留空即可，因为参数通过 config trait 获取）
    #[pallet::genesis_config]
    #[derive(frame_support::DefaultNoBound)]
    pub struct GenesisConfig<T: Config> {
        #[serde(skip)]
        pub _marker: PhantomData<T>,
    }

    // 实现创世构建逻辑
    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            let pool_account = incentive_pool_account::<T>();
            // 获取在 genesis_config_presets 中通过 balances 打入的真实余额
            let expected_balance = <T as Config>::Currency::free_balance(&pool_account);
            
            if expected_balance.is_zero() {
                log::error!("激励池初始余额为0，请检查 genesis balances 配置");
                return;
            }

            // 执行首次释放（释放 1%）
            let initial_release = T::DynamicReleaseRatio::get() * expected_balance;
            let locked_amount = expected_balance.saturating_sub(initial_release);
            
            // 使用 reserve 机制锁定未释放部分
            if let Err(e) = T::Currency::reserve(&pool_account, locked_amount) {
                log::error!("创世阶段激励池资金锁定失败: {:?}", e);
            }
            
            // 初始化内部逻辑变量
            IncentivePoolReleased::<T>::put(initial_release);
            IncentivePoolUsed::<T>::put(BalanceOf::<T>::zero());
            IncentivePoolReserved::<T>::put(locked_amount);
            LastMonthlyRewardBlock::<T>::put(BlockNumberFor::<T>::zero());
        }
    }

    // -------------------------- Hooks（周期性任务） --------------------------
    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// 区块初始化时执行：1. 激励池动态释放；2. 月度奖励发放
        fn on_initialize(current_block: BlockNumberFor<T>) -> Weight {
            let mut weight = Weight::zero();
            
            // 月度奖励发放
            let last_block = Self::last_monthly_reward_block();
            if current_block.saturating_sub(last_block) >= MONTH_BLOCKS.into() {
                weight = weight.saturating_add(Self::dynamic_release_incentive_pool());
                weight = weight.saturating_add(Self::distribute_monthly_rewards());
                LastMonthlyRewardBlock::<T>::put(current_block);
            }
            
            weight
        }

        // /// 链启动时初始化辅助存储 、激励池初始化！！！！！！！！！！！！！
        // /// 创世块（区块0）的构建过程中，on_runtime_upgrade钩子是不会被调用的。on_runtime_upgrade只会在链上升级时调用
        // fn on_runtime_upgrade() -> Weight {
        //     if StorageVersion::get::<Self>() < STORAGE_VERSION {
        //         let pool_account = incentive_pool_account::<T>();
        //         let actual_balance = <T as Config>::Currency::free_balance(&pool_account);
        //         let expected_balance = T::InitialIncentivePool::get();
                
        //         if actual_balance != expected_balance {
        //             log::warn!("创世配置激励池余额与经济模型不一致");
        //         }
                
        //         // 执行首次释放（链启动时立即释放1%）
        //         let initial_release = T::DynamicReleaseRatio::get() * expected_balance;
        //         let locked_amount = expected_balance.saturating_sub(initial_release);
                
        //         // 使用 reserve 机制锁定未释放部分（与质押模块相同）
        //         if let Err(e) = T::Currency::reserve(&pool_account, locked_amount) {
        //             log::error!("激励池资金锁定失败: {:?}", e);
        //             // 继续执行，但记录错误
        //         }
                
        //         IncentivePoolReleased::<T>::put(initial_release);
        //         IncentivePoolUsed::<T>::put(BalanceOf::<T>::zero());
        //         IncentivePoolReserved::<T>::put(locked_amount);
        //         LastMonthlyRewardBlock::<T>::put(BlockNumberFor::<T>::zero());
        //         StorageVersion::new(1).put::<Self>();
                
        //         Self::deposit_event(Event::IncentivePoolInitialized { 
        //             balance: actual_balance,
        //             pool_account: pool_account.clone()
        //         });
        //         Self::deposit_event(Event::IncentivePoolReleased {
        //             amount: initial_release,
        //             new_balance: initial_release,
        //             pool_account: pool_account.clone(),
        //         });
                
        //         T::DbWeight::get().writes(4)
        //     } else {
        //         Weight::zero()
        //     }
        // }
    }

    // -------------------------- Call（外部调用接口） --------------------------
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// 1. 手动触发激励池动态释放（仅治理权限）
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::trigger_dynamic_release())]
        pub fn trigger_dynamic_release(origin: OriginFor<T>) -> DispatchResult {
            ensure_root(origin)?;
            Self::dynamic_release_incentive_pool();
            Ok(())
        }

        /// 2. 手动发放优质数据奖励（支持治理或自动触发）
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::distribute_quality_data_reward())]
        pub fn distribute_quality_data_reward(origin: OriginFor<T>, asset_id: AssetId) -> DispatchResult {
            ensure_root(origin)?;      
            match T::DataAssetProvider::get_asset_owner(&asset_id) {
                Ok(owner) => {
                    // 正常处理奖励分发
                    Self::do_distribute_quality_data_reward(&owner, &asset_id)?;
                }
                Err(pallet_shared_traits::AssetQueryError::AssetNotFound) => {
                    log::error!("无法分发优质数据奖励：资产不存在 {:?}", asset_id);
                    return Err(Error::<T>::AssetNotFound.into());
                }
                Err(pallet_shared_traits::AssetQueryError::InvalidOwner) => {
                    log::error!("无法分发优质数据奖励：资产所有者无效 {:?}", asset_id);
                    return Err(Error::<T>::OwnerAccountIsEmpty.into());
                }
                Err(pallet_shared_traits::AssetQueryError::OwnerAccountDoesNotExist) => {
                    log::warn!("资产所有者账户不存在，但仍尝试分发奖励 {:?}", asset_id);
                    return Err(Error::<T>::OwnerAccountDoesNotExist.into());
                }
            }
            Ok(())
        }

        /// 4. 登记市场月交易额（市场运营者调用，用于优质市场判定）
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::register_market_monthly_volume())]
        pub fn register_market_monthly_volume(
            origin: OriginFor<T>,
            market_id: [u8; 32],
            volume: BalanceOf<T>,
        ) -> DispatchResult {
            let _who = ensure_signed(origin)?;
            MarketMonthlyVolume::<T>::insert(&market_id, volume);
            Ok(())
        }

        /// 5. 登记治理投票权重（治理模块调用）
        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::register_voting_weight())]
        pub fn register_voting_weight(
            origin: OriginFor<T>,
            voter: T::AccountId,
            weight: BalanceOf<T>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            GovernanceVotingWeight::<T>::insert(&voter, weight);
            Ok(())
        }
    }
}

// -------------------------- 核心逻辑实现 --------------------------
impl<T: Config> Pallet<T> {
    /// 获取激励池可用余额（已释放 - 已使用）
    fn get_available_balance() -> BalanceOf<T> {
        let released = Self::incentive_pool_released();
        let used = Self::incentive_pool_used();
        released.saturating_sub(used)
    }

    /// 内部转账函数，处理从激励池转账并更新已使用金额
    fn transfer_from_incentive_pool(
        recipient: &T::AccountId, 
        amount: BalanceOf<T>
    ) -> DispatchResult {
        let pool_account = incentive_pool_account::<T>();
        
        // 检查可用余额
        let available = Self::get_available_balance();
        ensure!(available >= amount, Error::<T>::InsufficientIncentivePoolBalance);
        
        // 检查实际可转余额，保留 existential deposit 维持池账户存活。
        let actual_balance = <T as Config>::Currency::free_balance(&pool_account);
        let transferable_balance =
            actual_balance.saturating_sub(<T as Config>::Currency::minimum_balance());
        ensure!(transferable_balance >= amount, Error::<T>::InsufficientIncentivePoolBalance);

        // 执行转账
        <T as Config>::Currency::transfer(
            &pool_account,
            recipient,
            amount,
            ExistenceRequirement::AllowDeath,
        )?;

        // 更新已使用金额
        IncentivePoolUsed::<T>::mutate(|used| *used = (*used).saturating_add(amount));

        Ok(())
    }

    /// 1. 激励池动态释放（从创世配置的账户余额中释放）
    fn dynamic_release_incentive_pool() -> Weight {
        let pool_account = incentive_pool_account::<T>();
        let total_initial = T::InitialIncentivePool::get();
        let released = Self::incentive_pool_released();
        let reserved = Self::incentive_pool_reserved();
        let remaining = total_initial.saturating_sub(released);
        
        if remaining.is_zero() {
            return Weight::zero();
        }

        let release_ratio = T::DynamicReleaseRatio::get();
        let release_amount = release_ratio * remaining;
        if release_amount.is_zero() {
            return Weight::zero();
        }

        // 未解除锁定的金额
        let remaining_reserved = T::Currency::unreserve(&pool_account, release_amount);

        // 计算真正解除锁定的金额
        let actual_unreserved = release_amount.saturating_sub(remaining_reserved);

        if actual_unreserved.is_zero() {
            log::error!("激励池资金释放失败: 实际释放为 0");
            return Weight::zero();
        }

        let new_released = released.saturating_add(actual_unreserved);
        let new_reserved = reserved.saturating_sub(actual_unreserved);
        
        IncentivePoolReleased::<T>::put(new_released);
        IncentivePoolReserved::<T>::put(new_reserved);

        Self::deposit_event(Event::IncentivePoolReleased {
            amount: actual_unreserved,
            new_balance: new_released,
            pool_account: pool_account.clone(),
        });

        T::DbWeight::get().writes(2)
    }

    /// 2. 月度奖励统一发放（优质市场、交易者返还、治理投票奖励）
    fn distribute_monthly_rewards() -> Weight {
        let mut weight = Weight::zero();

        weight = weight.saturating_add(Self::distribute_top_market_rewards());
        weight = weight.saturating_add(Self::distribute_trader_rebates());
        weight = weight.saturating_add(Self::distribute_governance_voting_rewards());
        Self::reset_monthly_statistics();

        weight
    }

    /// 2.1 优质市场月度奖励发放
    fn distribute_top_market_rewards() -> Weight {
        let mut weight = Weight::zero();
        let reward_per_market = T::TopMarketMonthlyReward::get();
        let pool_account = incentive_pool_account::<T>();

        // 收集所有市场
        let mut markets: Vec<([u8; 32], BalanceOf<T>)> = MarketMonthlyVolume::<T>::iter().collect();
        if markets.is_empty() {
            return Weight::zero();
        }

        // 按交易额降序排序
        markets.sort_by(|a, b| b.1.cmp(&a.1));

        // 计算优质市场数量（前10%，最少1个）- 使用整数运算
        let top_count = if markets.len() < 10 {
            1 // 少于10个市场时，只选第1名
        } else {
            // 计算10%（向上取整）
            (markets.len() + 9) / 10
        };

        let top_markets = &markets[0..top_count.min(markets.len())];

        // 计算总需求金额
        let total_required = if let Some(total) = reward_per_market.checked_mul(&(top_count as u32).into()) {
            total
        } else {
            return Weight::zero();
        };

        // 检查可用余额
        let available = Self::get_available_balance();
        if available < total_required {
            Self::deposit_event(Event::IncentivePoolInsufficientBalance {
                required: total_required,
                available,
                pool_account: pool_account.clone(),
            });
            return Weight::zero();
        }

        // 给每个优质市场发放奖励
        for (market_id, _volume) in top_markets {
            // ################: 需要从市场模块获取真实的运营者账户
            // 这里################3使用市场ID作为账户（实际项目中需要修改）
            let operator = T::AccountId::decode(&mut &market_id[..])
                .unwrap_or_else(|_| incentive_pool_account::<T>());

            if let Err(e) = Self::transfer_from_incentive_pool(&operator, reward_per_market) {
                log::error!("优质市场奖励转账失败：market_id={:?}, error={:?}", market_id, e);
                continue;
            }

            Self::deposit_event(Event::TopMarketRewardDistributed {
                recipient: operator,
                amount: reward_per_market,
                market_id: *market_id,
                pool_account: pool_account.clone(),
            });

            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }

        weight
    }

    /// 2.2 交易者手续费返还发放
    fn distribute_trader_rebates() -> Weight {
        let mut weight = Weight::zero();
        let threshold = T::TraderRebateThreshold::get();
        let rebate_ratio = T::TraderRebateRatio::get();
        let pool_account = incentive_pool_account::<T>();

        for (trader, monthly_volume) in TraderMonthlyVolume::<T>::iter() {
            if monthly_volume < threshold {
                continue;
            }

            let rebate_amount = rebate_ratio * monthly_volume;
            if rebate_amount.is_zero() {
                continue;
            }

            // 检查可用余额
            let available = Self::get_available_balance();
            if available < rebate_amount {
                Self::deposit_event(Event::IncentivePoolInsufficientBalance {
                    required: rebate_amount,
                    available,
                    pool_account: pool_account.clone(),
                });
                break;
            }

            if let Err(e) = Self::transfer_from_incentive_pool(&trader, rebate_amount) {
                log::error!("交易者手续费返还转账失败：trader={:?}, error={:?}", trader, e);
                continue;
            }

            Self::deposit_event(Event::TraderRebateDistributed {
                recipient: trader.clone(),
                amount: rebate_amount,
                monthly_volume,
                pool_account: pool_account.clone(),
            });

            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }

        weight
    }

    /// 2.3 治理参与者投票奖励发放
    fn distribute_governance_voting_rewards() -> Weight {
        let mut weight = Weight::zero();
        let total_reward = T::GovernanceVotingRewardTotal::get();
        let pool_account = incentive_pool_account::<T>();

        // 检查可用余额
        let available = Self::get_available_balance();
        if available < total_reward {
            Self::deposit_event(Event::IncentivePoolInsufficientBalance {
                required: total_reward,
                available,
                pool_account: pool_account.clone(),
            });
            return Weight::zero();
        }

        // 计算总投票权重
        let mut total_weight = BalanceOf::<T>::zero();
        for (_, weight_val) in GovernanceVotingWeight::<T>::iter() {
            total_weight = total_weight.saturating_add(weight_val);
        }

        if total_weight.is_zero() {
            return Weight::zero();
        }

        for (voter, weight_val) in GovernanceVotingWeight::<T>::iter() {
            let reward_amount = if let Some(amount) = total_reward.checked_div(&total_weight) {
                amount.saturating_mul(weight_val)
            } else {
                continue;
            };

            if reward_amount.is_zero() {
                continue;
            }

            if let Err(e) = Self::transfer_from_incentive_pool(&voter, reward_amount) {
                log::error!("治理投票奖励转账失败：voter={:?}, error={:?}", voter, e);
                continue;
            }

            Self::deposit_event(Event::GovernanceVotingRewardDistributed {
                recipient: voter.clone(),
                amount: reward_amount,
                weight: weight_val,
                pool_account: pool_account.clone(),
            });

            weight = weight.saturating_add(T::DbWeight::get().writes(1));
        }

        weight
    }

    /// 2.4 重置月度统计数据
    fn reset_monthly_statistics() {
        // 使用clear替代remove_all
        let _ = MarketMonthlyVolume::<T>::clear(u32::MAX, None);
        let _ = TraderMonthlyVolume::<T>::clear(u32::MAX, None);
        let _ = GovernanceVotingWeight::<T>::clear(u32::MAX, None);
        let _ = Asset30dTradeCount::<T>::clear(u32::MAX, None);
    }

    /// 3. 数据创建者：首次创建元证奖励（供dataassets模块调用）
    pub fn distribute_first_create_reward(recipient: &T::AccountId, asset_id: &AssetId) -> DispatchResult {
        ensure!(!Self::has_first_create_reward(recipient), Error::<T>::FirstCreateRewardAlreadyClaimed);
        
        let reward_amount = T::FirstCreateReward::get();
        
        // 使用内部转账函数，会自动检查可用余额并更新已使用金额
        Self::transfer_from_incentive_pool(recipient, reward_amount)?;

        HasFirstCreateReward::<T>::insert(recipient, true);

        let pool_account = incentive_pool_account::<T>();
        Self::deposit_event(Event::FirstCreateRewardDistributed {
            recipient: recipient.clone(),
            amount: reward_amount,
            asset_id: *asset_id,
            pool_account: pool_account.clone(),
        });

        Ok(())
    }

    /// 4. 数据创建者：优质数据奖励（供自动触发或手动调用）
    fn do_distribute_quality_data_reward(recipient: &T::AccountId, asset_id: &AssetId) -> DispatchResult {
        let trade_count = Self::asset_30d_trade_count(asset_id);
        let threshold = T::QualityDataTradeThreshold::get();
        
        ensure!(trade_count >= threshold, Error::<T>::QualityDataConditionNotMet);
        
        let reward_amount = T::QualityDataReward::get();
        
        // 使用内部转账函数
        Self::transfer_from_incentive_pool(recipient, reward_amount)?;

        let pool_account = incentive_pool_account::<T>();
        Self::deposit_event(Event::QualityDataRewardDistributed {
            recipient: recipient.clone(),
            amount: reward_amount,
            asset_id: *asset_id,
            pool_account: pool_account.clone(),
        });

        Ok(())
    }

    /// 5. 交易者：流动性奖励（供交易模块调用）
    pub fn distribute_liquidity_reward(recipient: &T::AccountId, order_amount: BalanceOf<T>) -> DispatchResult {
        let reward_ratio = T::LiquidityRewardRatio::get();
        let reward_amount = reward_ratio * order_amount;
        if reward_amount.is_zero() {
            return Ok(());
        }

        // 使用内部转账函数
        Self::transfer_from_incentive_pool(recipient, reward_amount)?;

        let pool_account = incentive_pool_account::<T>();
        Self::deposit_event(Event::LiquidityRewardDistributed {
            recipient: recipient.clone(),
            amount: reward_amount,
            order_amount,
            pool_account: pool_account.clone(),
        });

        Ok(())
    }

    /// 6. 治理参与者：提案通过奖励（供治理模块调用）
    pub fn distribute_proposal_reward(recipient: &T::AccountId) -> DispatchResult {
        let reward_amount = T::GovernanceProposalReward::get();
        
        // 使用内部转账函数
        Self::transfer_from_incentive_pool(recipient, reward_amount)?;

        let pool_account = incentive_pool_account::<T>();
        Self::deposit_event(Event::GovernanceProposalRewardDistributed {
            recipient: recipient.clone(),
            amount: reward_amount,
            pool_account: pool_account.clone(),
        });

        Ok(())
    }

    /// 登记元证交易笔数（供dataassets模块调用，用于优质数据判定）
    pub fn register_asset_trade(asset_id: &AssetId) {
        Asset30dTradeCount::<T>::mutate(asset_id, |count| *count = count.saturating_add(1));
    }

    /// 登记交易者月交易额（供交易模块调用）
    pub fn register_trader_monthly_volume(trader: &T::AccountId, volume: BalanceOf<T>) {
        TraderMonthlyVolume::<T>::mutate(trader, |v| *v = (*v).saturating_add(volume));
    }

    pub fn register_market_volume_internal(market_id: &[u8; 32], volume: BalanceOf<T>) {
         MarketMonthlyVolume::<T>::mutate(market_id, |v| *v = (*v).saturating_add(volume));
    }
}

impl<T: Config> pallet_shared_traits::IncentiveHandler<T::AccountId, [u8; 32], BalanceOf<T>> for Pallet<T> {
    fn distribute_first_create_reward(recipient: &T::AccountId, asset_id: &[u8; 32]) -> Result<(), &'static str> {
        Self::distribute_first_create_reward(recipient, asset_id)
            .map_err(|_| "Distribution failed")
    }
    
    fn register_asset_trade(asset_id: &[u8; 32]) {
        Self::register_asset_trade(asset_id)
    }
    
    fn distribute_liquidity_reward(recipient: &T::AccountId, order_amount: BalanceOf<T>) -> Result<(), &'static str> {
        Self::distribute_liquidity_reward(recipient, order_amount)
            .map_err(|_| "Liquidity reward failed")
    }
    
    fn distribute_proposal_reward(recipient: &T::AccountId) -> Result<(), &'static str> {
        Self::distribute_proposal_reward(recipient)
            .map_err(|_| "Proposal reward failed")
    }
}
