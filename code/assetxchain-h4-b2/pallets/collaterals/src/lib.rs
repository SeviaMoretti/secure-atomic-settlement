#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

// 权重定义
pub mod weights;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{
        pallet_prelude::*,
        traits::{Currency, Imbalance, ReservableCurrency, Get},
        transactional,
    };
    use frame_system::pallet_prelude::*;
    use sp_runtime::{
        traits::{Zero, CheckedAdd, SaturatedConversion, Bounded, Saturating},
        DispatchError, ArithmeticError,
    };
    use scale_info::TypeInfo;
    use codec::{Encode, Decode, MaxEncodedLen, DecodeWithMemTracking};

    pub trait WeightInfo {
        fn unbond() -> Weight;
        fn pledge() -> Weight;
    }

    /// 货币类型的别名
    type BalanceOf<T> =
        <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    /// 惩罚类型，用于决定资金分配比例
    #[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen, DecodeWithMemTracking)]
    pub enum SlashType {
        /// 50% 销毁, 50% 进激励池
        HeavyViolation,
        /// 30% 销毁, 20% 补偿用户, 50% 激励池
        LightViolation,
        /// 市场运营者重度违规：50% 销毁, 50% 补偿用户
        MarketOperatorHeavy,
        /// IPFS 服务提供者重度违规：50% 销毁, 50% 转入 IPFS 存储池
        IpfsProviderHeavy,
    }

    /// 质押角色枚举
    #[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen, DecodeWithMemTracking)]
    pub enum CollateralRole {
        DataCreator,        // 数据提供者，高频
        MarketOperator,     // 市场创建者，低频
        IpfsProvider,       // IPFS服务提供者-->存储链
        GovernancePledge,   // 验证节点，这里这个角色没用到，相关逻辑在validators_set-->pallet-staking
    }

    /// 质押详细信息结构体
    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen, Default, DecodeWithMemTracking)]
    pub struct CollateralInfo<BlockNumber, Balance> {
        pub amount: Balance,                        // 当前质押金额
        pub start_block: BlockNumber,               // 质押起始区块
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        
        /// 使用 ReservableCurrency trait 来管理质押的锁定
        type Currency: ReservableCurrency<Self::AccountId>; 

        /// 定义各种角色的最小质押金额
        #[pallet::constant]
        type MinMarketOperatorCollateral: Get<BalanceOf<Self>>;
        #[pallet::constant]
        type MinIpfsProviderCollateral: Get<BalanceOf<Self>>;
        #[pallet::constant]
        type MinGovernancePledge: Get<BalanceOf<Self>>;

        /// 专门用于处理惩罚（Slash）和奖励（Reward）的账户ID
        #[pallet::constant]
        type IncentivePoolAccount: Get<Self::AccountId>;
        /// 用于通缩机制中的销毁（黑洞账户）
        #[pallet::constant]
        type DestructionAccount: Get<Self::AccountId>; 
        /// IPFS 存储费用池（一个资产一个池子）
        #[pallet::constant]
        type IpfsPoolAccount: Get<Self::AccountId>;
        /// 用户补偿池 (用于 MarketOperator 违规)
        #[pallet::constant]
        type CompensationPoolAccount: Get<Self::AccountId>;

        /// Weight information
        type WeightInfo: WeightInfo;
    }

    // 存储所有定制化质押角色的质押信息
    #[pallet::storage]
    #[pallet::getter(fn collateral_data)]
    pub type CollateralData<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat, T::AccountId,
        Blake2_128Concat, CollateralRole,
        CollateralInfo<BlockNumberFor<T>, BalanceOf<T>>,
        ValueQuery,
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// 质押成功
        Pledged { who: T::AccountId, role: CollateralRole, amount: BalanceOf<T> },
        /// 解除质押成功
        Unbonded { who: T::AccountId, role: CollateralRole, amount: BalanceOf<T> },
        /// 质押被惩罚并分配
        SlashedAndDistributed { 
            who: T::AccountId, 
            role: CollateralRole, 
            slashed_amount: BalanceOf<T>, 
            burn_amount: BalanceOf<T>, 
            incentive_amount: BalanceOf<T> 
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// 质押金额不足最小要求
        InsufficientCollateralAmount,
        /// 质押信息未找到
        CollateralNotFound,
        /// 质押金额为零
        AmountIsZero,
        /// 质押条件尚未满足，无法释放
        CollateralNotReadyForRelease,
        /// 角色不支持此操作
        UnsupportedRole,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::pledge())]
        pub fn pledge(origin: OriginFor<T>, role: CollateralRole, amount: BalanceOf<T>) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::internal_pledge(&who, role, amount)
        }

        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::unbond())]
        pub fn unbond(origin: OriginFor<T>, role: CollateralRole) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::internal_unbond(&who, role)
        }
    }

    /// 辅助函数
    impl<T: Config> Pallet<T> {
        
        pub fn internal_pledge(who: &T::AccountId, role: CollateralRole, amount: BalanceOf<T>) -> DispatchResult {
            ensure!(!amount.is_zero(), Error::<T>::AmountIsZero);
            Self::ensure_min_collateral(&role, amount)?;
            
            T::Currency::reserve(who, amount)?;

            CollateralData::<T>::try_mutate(who, &role, |info| -> DispatchResult {
                info.amount = info.amount.checked_add(&amount).ok_or(ArithmeticError::Overflow)?;
                if info.start_block.is_zero() {
                    info.start_block = frame_system::Pallet::<T>::block_number();
                }
                Ok(())
            })?;

            Self::deposit_event(Event::Pledged { who: who.clone(), role, amount });
            Ok(())
        }

        // 核心解除质押逻辑
        pub fn internal_unbond(who: &T::AccountId, role: CollateralRole) -> DispatchResult {
            let collateral_info = CollateralData::<T>::get(who, &role);
            ensure!(!collateral_info.amount.is_zero(), Error::<T>::CollateralNotFound);

            let (releasable, remaining) = Self::get_releasable_amount(&role, &collateral_info)?;
            ensure!(!releasable.is_zero(), Error::<T>::CollateralNotReadyForRelease);

            T::Currency::unreserve(who, releasable);
            
            if remaining.is_zero() {
                CollateralData::<T>::remove(who, &role);// 全部释放，移除存储项
            } else {
                CollateralData::<T>::mutate(who, &role, |info| info.amount = remaining);
            }

            Self::deposit_event(Event::Unbonded { who: who.clone(), role, amount: releasable });
            Ok(())
        }

        /// 检查最小质押要求
        fn ensure_min_collateral(role: &CollateralRole, amount: BalanceOf<T>) -> DispatchResult {
            let min_amount = match role {
                CollateralRole::MarketOperator => T::MinMarketOperatorCollateral::get(),
                CollateralRole::IpfsProvider => T::MinIpfsProviderCollateral::get(),
                CollateralRole::GovernancePledge => T::MinGovernancePledge::get(),
                // 数据创建者的基础质押在业务 Pallet 中处理
                _ => BalanceOf::<T>::zero(), 
            };
            
            if !min_amount.is_zero() {
                ensure!(amount >= min_amount, Error::<T>::InsufficientCollateralAmount);
            }
            Ok(())
        }

        /// 计算可释放金额和剩余金额
        fn get_releasable_amount(
            role: &CollateralRole,
            info: &CollateralInfo<BlockNumberFor<T>, BalanceOf<T>>,
        ) -> Result<(BalanceOf<T>, BalanceOf<T>), DispatchError> {
            
            match role {
                CollateralRole::DataCreator => {
                    // 检查是否通过了 90 天长期可用验证
                    let ninety_days_u32 = 90u32 * 24 * 60;
                    let ninety_days = BlockNumberFor::<T>::from(ninety_days_u32); // 假设计算单位

                    if frame_system::Pallet::<T>::block_number() > info.start_block.checked_add(&ninety_days).unwrap_or(Bounded::max_value()) {
                        Ok((info.amount, BalanceOf::<T>::zero()))
                    } else {
                        Err(Error::<T>::CollateralNotReadyForRelease.into())
                    }
                },
                CollateralRole::MarketOperator => {
                    // 运营满 2 年后释放
                    let two_years_u32 = 365 * 2 * 24 * 60u32;
                    let two_years = BlockNumberFor::<T>::from(two_years_u32);
                    if frame_system::Pallet::<T>::block_number() > info.start_block.checked_add(&two_years).unwrap_or(Bounded::max_value()) {
                        Ok((info.amount, BalanceOf::<T>::zero()))
                    } else {
                        Err(Error::<T>::CollateralNotReadyForRelease.into())
                    }
                },
                _ => {
                    // 对于其他角色，简单锁定 7 天后可释放
                    let lock_period_u32 = 7u32 * 24 * 60;
                    let lock_period = BlockNumberFor::<T>::from(lock_period_u32);
                    if frame_system::Pallet::<T>::block_number() > info.start_block.checked_add(&lock_period).unwrap_or(Bounded::max_value()) {
                        Ok((info.amount, BalanceOf::<T>::zero()))
                    } else {
                        Err(Error::<T>::CollateralNotReadyForRelease.into())
                    }
                }
            }
        }
        
        /// 执行惩罚和资金分配
        #[transactional]
        pub fn slash_and_distribute(
            who: &T::AccountId,
            role: CollateralRole,
            slash_amount: BalanceOf<T>,
            slash_type: SlashType,
        ) -> Result<BalanceOf<T>, DispatchError> {
            // 1. 基础检查
            ensure!(!slash_amount.is_zero(), Error::<T>::AmountIsZero);
            
            let collateral_info = CollateralData::<T>::get(who, &role);
            let available_amount = collateral_info.amount;
            
            // 确保惩罚金额不超过用户实际持有的质押金
            let actual_slash = if slash_amount > available_amount {
                available_amount
            } else {
                slash_amount
            };

            if actual_slash.is_zero() {
                return Ok(BalanceOf::<T>::zero());
            }

            // 2. 根据惩罚类型确定分配比例
            let (burn_ratio, compensation_ratio, ipfs_ratio) = match slash_type {
                SlashType::HeavyViolation => (50, 0, 0),
                SlashType::LightViolation => (30, 0, 0),
                SlashType::MarketOperatorHeavy => (50, 50, 0),
                SlashType::IpfsProviderHeavy => (50, 0, 50),
            };

            // 3. 计算各部分金额
            let total_u128: u128 = actual_slash.saturated_into();
            let burn_amount: BalanceOf<T> = (total_u128 * burn_ratio as u128 / 100).saturated_into();
            let compensation_amount: BalanceOf<T> = (total_u128 * compensation_ratio as u128 / 100).saturated_into();
            let ipfs_amount: BalanceOf<T> = (total_u128 * ipfs_ratio as u128 / 100).saturated_into();
            
            // 剩下的全部归入激励池，防止浮点数精度丢失导致的资金残留
            let final_incentive_amount = actual_slash
                .saturating_sub(burn_amount)
                .saturating_sub(compensation_amount)
                .saturating_sub(ipfs_amount);

            // 4. 从 reserved 罚没后再分配，允许池账户尚未存在。
            let (slashed_imbalance, unslashed) = T::Currency::slash_reserved(who, actual_slash);
            ensure!(unslashed.is_zero(), ArithmeticError::Underflow);
            let (burn_imbalance, remaining_imbalance) = slashed_imbalance.split(burn_amount);
            let (compensation_imbalance, remaining_imbalance) =
                remaining_imbalance.split(compensation_amount);
            let (ipfs_imbalance, incentive_imbalance) =
                remaining_imbalance.split(ipfs_amount);
            
            if !burn_amount.is_zero() {
                T::Currency::resolve_creating(&T::DestructionAccount::get(), burn_imbalance);
            }
            
            if !compensation_amount.is_zero() {
                T::Currency::resolve_creating(
                    &T::CompensationPoolAccount::get(),
                    compensation_imbalance,
                );
            }
            
            if !ipfs_amount.is_zero() {
                T::Currency::resolve_creating(&T::IpfsPoolAccount::get(), ipfs_imbalance);
            }
            
            if !final_incentive_amount.is_zero() {
                T::Currency::resolve_creating(&T::IncentivePoolAccount::get(), incentive_imbalance);
            }

            // 5. 更新存储
            CollateralData::<T>::mutate(who, &role, |info| {
                info.amount = info.amount.saturating_sub(actual_slash);
                if info.amount.is_zero() {
                    CollateralData::<T>::remove(who, &role);
                }
            });

            // 6. 触发事件
            Self::deposit_event(Event::SlashedAndDistributed { 
                who: who.clone(), 
                role,
                slashed_amount: actual_slash, 
                burn_amount, 
                incentive_amount: final_incentive_amount,
            });

            Ok(actual_slash)
        }
    }
}
