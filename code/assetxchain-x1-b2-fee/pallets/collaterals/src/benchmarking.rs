#![cfg(feature = "runtime-benchmarks")]

use super::*;

#[allow(unused)]
use crate::Pallet as Collaterals;
use frame_benchmarking::{benchmarks, account, whitelisted_caller};
use frame_system::RawOrigin;
use frame_support::traits::{Currency, Get, ReservableCurrency};
use sp_runtime::traits::{Bounded, One};
use sp_runtime::traits::{ Saturating, SaturatedConversion };
type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

// 辅助函数：生成一个有足够资金的账户
fn setup_funded_account<T: Config>(name: &'static str, index: u32, role: CollateralRole) -> T::AccountId {
    let caller: T::AccountId = account(name, index, 0);
    
    // 获取对应角色的最小质押要求
    let min_collateral = match role {
        CollateralRole::MarketOperator => T::MinMarketOperatorCollateral::get(),
        CollateralRole::IpfsProvider => T::MinIpfsProviderCollateral::get(),
        CollateralRole::GovernancePledge => T::MinGovernancePledge::get(),
        _ => 10_000u32.into(), // 数据创建者等默认给个基础值
    };

    // 注入资金 = 最小要求 * 10 + 系统的存在性存款(ED)
    // 确保绝对够用且不会因为支付手续费导致账户死亡
    let ed = T::Currency::minimum_balance();
    let funded_amount = min_collateral.saturating_mul(10u32.into()).saturating_add(ed);
    
    T::Currency::make_free_balance_be(&caller, funded_amount);
    caller
}

benchmarks! {
    // 1. 测试 pledge
    pledge {
        let role = CollateralRole::IpfsProvider;
        // 动态准备充足资金的账户
        let caller = setup_funded_account::<T>("caller", 0, role);
        let pledge_amount = T::MinIpfsProviderCollateral::get() + 100u32.into();

    }: _(RawOrigin::Signed(caller.clone()), role, pledge_amount)
    verify {
        assert!(CollateralData::<T>::contains_key(&caller, role));
        assert_eq!(T::Currency::reserved_balance(&caller), pledge_amount);
    }

    // 2. 测试 unbond
    unbond {
        let role = CollateralRole::IpfsProvider;
        let caller = setup_funded_account::<T>("caller", 0, role);
        let pledge_amount = T::MinIpfsProviderCollateral::get() + 100u32.into();

        // 前置状态：先执行质押
        Pallet::<T>::internal_pledge(&caller, role, pledge_amount)?;
        
        // 模拟时间流逝（7天 + 缓冲）
        let lock_period = 7u32 * 24 * 60 + 100; 
        let future_block = frame_system::Pallet::<T>::block_number() + lock_period.into();
        frame_system::Pallet::<T>::set_block_number(future_block);

    }: _(RawOrigin::Signed(caller.clone()), role)
    verify {
        assert!(!CollateralData::<T>::contains_key(&caller, role));
        assert_eq!(T::Currency::reserved_balance(&caller), 0u32.into());
    }

    impl_benchmark_test_suite!(Collaterals, crate::mock::new_test_ext(), crate::mock::Test);
}