#![cfg(feature = "runtime-benchmarks")]

use super::*;

use frame_benchmarking::{benchmarks, account, whitelisted_caller};
use frame_system::RawOrigin;
use frame_support::traits::{Currency, Get, ReservableCurrency};
use sp_runtime::traits::{Saturating, SaturatedConversion};
use frame_support::storage::child;
use sp_core::{storage::ChildInfo, H256};
// 确保能够引用到 DataAsset 结构体
use pallet_dataassets::types::DataAsset;

// 与 pallet-dataassets 保持一致的子树 ID
const ASSET_TRIE_ID: &[u8] = b":asset_trie:";

/// 辅助函数：初始化激励池
fn setup_pool_v1<T: Config>() {
    let pool_account = incentive_pool_account::<T>();
    // 注入充足资金 (10倍初始值)
    let funded_amount = T::InitialIncentivePool::get().saturating_mul(10u32.into());
    T::Currency::make_free_balance_be(&pool_account, funded_amount);
    
    let initial_balance = T::InitialIncentivePool::get();
    let release_ratio = T::DynamicReleaseRatio::get();
    let initial_release = release_ratio * initial_balance;
    let locked_amount = initial_balance.saturating_sub(initial_release);
    
    // 必须锁定资金
    T::Currency::reserve(&pool_account, locked_amount).unwrap();
    
    IncentivePoolReleased::<T>::put(initial_release);
    IncentivePoolReserved::<T>::put(locked_amount);
}

benchmarks! {
    // 1. 手动释放逻辑测试
    trigger_dynamic_release {
        setup_pool_v1::<T>();
    }: _(RawOrigin::Root)
    verify {
        assert!(IncentivePoolReleased::<T>::get() > BalanceOf::<T>::zero());
    }

    // 2. 优质数据奖励分发测试
    distribute_quality_data_reward {
        setup_pool_v1::<T>();
        let asset_id: [u8; 32] = [1u8; 32];
        let owner: T::AccountId = account("owner", 0, 0);
        let timestamp = 1642220000u64;
        
        // 账户准备
        T::Currency::make_free_balance_be(&owner, T::Currency::minimum_balance() * 100u32.into());

        // 构造DataAsset并注入子树
        let mut asset = DataAsset::<T::AccountId>::minimal(
            owner.clone(),
            b"Benchmark Asset".to_vec(),
            b"Description".to_vec(),
            H256::repeat_byte(0x01),
            timestamp,
        );
        asset.core.asset_id = asset_id;

        let child_info = ChildInfo::new_default(ASSET_TRIE_ID);
        let mut key = b"assets/".to_vec();
        key.extend_from_slice(&asset_id);
        
        // 注入完整的 DataAsset 字节流
        child::put(&child_info, &key, &asset);

        // 交易量注入
        Asset30dTradeCount::<T>::insert(&asset_id, T::QualityDataTradeThreshold::get());
    }: _(RawOrigin::Root, asset_id)
    verify {
        assert!(IncentivePoolUsed::<T>::get() > BalanceOf::<T>::zero());
    }

    // 3. 市场交易额登记测试
    register_market_monthly_volume {
        let caller: T::AccountId = whitelisted_caller();
        let market_id: [u8; 32] = [2u8; 32];
        let volume: BalanceOf<T> = 10_000u32.into();
    }: _(RawOrigin::Signed(caller), market_id, volume)
    verify {
        assert_eq!(MarketMonthlyVolume::<T>::get(&market_id), volume);
    }

    // 4. 投票权重登记测试
    register_voting_weight {
        let voter: T::AccountId = account("voter", 0, 0);
        let weight: BalanceOf<T> = 5_000u32.into();
    }: _(RawOrigin::Root, voter.clone(), weight)
    verify {
        assert_eq!(GovernanceVotingWeight::<T>::get(&voter), weight);
    }

    impl_benchmark_test_suite!(Incentive, crate::mock::new_test_ext(), crate::mock::Test);
}