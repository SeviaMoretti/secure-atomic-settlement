//! # Block Rewards Pallet
//!
//! Initially, there are 5 DAT per block, 
//! and after mining 250 million, t
//! here will be 1 DAT per block

#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

pub mod weights;

#[frame_support::pallet]
pub mod pallet {
	// Import various useful types required by all FRAME pallets.
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	use frame_support::traits::Currency;
	use frame_support::sp_runtime::Saturating;

	pub type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	pub trait WeightInfo {
		fn get_current_reward() -> Weight;
		fn on_finalize_initial() -> Weight;
		fn on_finalize_adjustment() -> Weight;
		fn on_finalize_max_supply() -> Weight;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching runtime event type.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		/// A type representing the weights required by the dispatchables of this pallet.
		// type WeightInfo: WeightInfo;
		/// 用于奖励的货币类型
		type Currency: Currency<Self::AccountId>;
		/// 区块奖励接收者（区块生产者）
		type RewardReceiver: Get<Self::AccountId>;
		// 常量定义
        #[pallet::constant]
        type InitialReward: Get<BalanceOf<Self>>;
        #[pallet::constant]
        type RewardAdjustmentThreshold: Get<BalanceOf<Self>>;
        #[pallet::constant]
        type AdjustedReward: Get<BalanceOf<Self>>;
		#[pallet::constant]
    	type MaxSupply: Get<BalanceOf<Self>>;

		type WeightInfo: WeightInfo;
	}

	#[pallet::storage]
	pub type TotalTokensMined<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	// /// 初始区块奖励：5 DAT
	// #[pallet::type_value]
	// pub fn InitialReward<T: Config>() -> BalanceOf<T> {
	// 	5u32.into()
	// }

	// /// 奖励调整阈值：2.5亿个代币
	// #[pallet::type_value]
	// pub fn RewardAdjustmentThreshold<T: Config>() -> BalanceOf<T> {
	// 	250_000_000u32.into()
	// }

	// /// 调整后的区块奖励：1 DAT
	// #[pallet::type_value]
	// pub fn AdjustedReward<T: Config>() -> BalanceOf<T> {
	// 	1u32.into()
	// }

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		// 谁出的块，奖励金额，当前区块号
		RewardPaid{who: T::AccountId, amount: BalanceOf<T>, block_number: BlockNumberFor<T>},
		// 新奖励金额，调整发生的区块号
		RewardAdjusted{new_amount: BalanceOf<T>, block_number: BlockNumberFor<T>},
		// 当前奖励查询结果
        CurrentRewardQueried{who: T::AccountId, amount: BalanceOf<T>},
	}

	#[pallet::error]
	pub enum Error<T> {
		/// 奖励发放失败（例如余额不足，虽然使用deposit_creating，一般不会出现）
		RewardDistributionFailed,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		/// 在每个区块结束时发放奖励（每次都判断当前奖励金额）
		fn on_finalize(block_number: BlockNumberFor<T>) {
			// 获取当前已挖出的代币总量
			let current_total = TotalTokensMined::<T>::get();
			let max_supply = T::MaxSupply::get();
			
			// 检查是否已超最大供应量
			if current_total >= max_supply {
				return;
			}

			// 计算当前区块应发放的奖励（每次都判断：未达阈值发5，已达阈值发1）
			let mut reward_amount = Self::calculate_current_reward(current_total);

			// 防止最后一笔奖励超出5亿上限
			if current_total.saturating_add(reward_amount) > max_supply {
				reward_amount = max_supply.saturating_sub(current_total);
			}

			// 发放奖励给接收者
			let receiver = T::RewardReceiver::get();
			// 发放奖励给接收者，忽略返回的Imbalance
			let _ = T::Currency::deposit_creating(&receiver, reward_amount);

			// 更新总量
			let new_total = current_total.checked_add(&reward_amount)
				.expect("奖励金额不会导致溢出"); // 实际场景可根据需求处理溢出
			// 更新已挖出的代币总量
			TotalTokensMined::<T>::put(new_total);

			// 真给了区块奖励才发事件
			if reward_amount > Zero::zero() {
				Self::deposit_event(Event::RewardPaid {
					who: receiver.clone(),
					amount: reward_amount,
					block_number,
				});
			}

			// 若本次发放后首次达到阈值，触发奖励调整事件
			if current_total < T::RewardAdjustmentThreshold::get() 
				&& new_total >= T::RewardAdjustmentThreshold::get() 
			{
				Self::deposit_event(Event::RewardAdjusted {
					new_amount: T::AdjustedReward::get(),
					block_number,
				});
			}
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// 获取当前区块奖励的金额
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::get_current_reward())]
		// #[pallet::weight(T::WeightInfo::get_current_reward())]
		pub fn get_current_reward(origin: OriginFor<T>) -> DispatchResult {
			let who = ensure_signed(origin)?;
			
			let current_total = TotalTokensMined::<T>::get();
			let current_reward = Self::calculate_current_reward(current_total);
			
			// 返回当前区块的奖励
			Self::deposit_event(Event::CurrentRewardQueried {
                who,
                amount: current_reward,
            });
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		/// 每次发放奖励前计算当前应发金额
		/// 若累计已挖出的代币 < 2.5亿，发5个；否则发1个
		fn calculate_current_reward(current_total: BalanceOf<T>) -> BalanceOf<T> {
			if current_total < T::RewardAdjustmentThreshold::get() {
				T::InitialReward::get()
			} else {
				T::AdjustedReward::get()
			}
		}
	}
}
