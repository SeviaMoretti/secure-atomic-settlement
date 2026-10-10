#![cfg(feature = "runtime-benchmarks")]

use super::*;
use crate::Pallet as Rewards;
use frame_benchmarking::{benchmarks, whitelisted_caller, account};
use frame_system::RawOrigin;

use frame_support::{
	traits::{Currency, Get, Hooks},
	sp_runtime::traits::{Saturating, Zero},
};
use frame_system::pallet_prelude::BlockNumberFor;

benchmarks! {
	get_current_reward {
		let caller: T::AccountId = whitelisted_caller();
	}: _(RawOrigin::Signed(caller))
	verify {
		assert!(!frame_system::Pallet::<T>::events().is_empty());
	}

	on_finalize_initial {
		let block_number: BlockNumberFor<T> = 1u32.into();
		TotalTokensMined::<T>::put(BalanceOf::<T>::zero());
	}: {
		<Rewards<T> as Hooks<BlockNumberFor<T>>>::on_finalize(block_number);
	}
	verify {
		assert_eq!(TotalTokensMined::<T>::get(), T::InitialReward::get());
	}

	on_finalize_adjustment {
		let block_number: BlockNumberFor<T> = 100u32.into();
		let threshold = T::RewardAdjustmentThreshold::get();
		let initial_reward = T::InitialReward::get();
		
		let near_threshold = threshold.saturating_sub(initial_reward);
		TotalTokensMined::<T>::put(near_threshold);
	}: {
		<Rewards<T> as Hooks<BlockNumberFor<T>>>::on_finalize(block_number);
	}
	verify {
		assert!(TotalTokensMined::<T>::get() >= threshold);
	}

	on_finalize_max_supply {
		let block_number: BlockNumberFor<T> = 999u32.into();
		let max_supply = T::MaxSupply::get();
		TotalTokensMined::<T>::put(max_supply);
	}: {
		<Rewards<T> as Hooks<BlockNumberFor<T>>>::on_finalize(block_number);
	}
	verify {
		assert_eq!(TotalTokensMined::<T>::get(), max_supply);
	}

	impl_benchmark_test_suite!(Rewards, crate::mock::new_test_ext(), crate::mock::Test);
}