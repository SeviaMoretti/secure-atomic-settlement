use crate::{mock::*, Error, Event, HasFirstCreateReward, IncentivePoolReleased, IncentivePoolReserved, IncentivePoolUsed};
use frame_support::{assert_noop, assert_ok};

#[test]
fn genesis_accounting_matches_pool_balance_and_initial_release() {
    new_test_ext(InitialIncentivePool::get()).execute_with(|| {
        let expected_released = DynamicReleaseRatio::get() * InitialIncentivePool::get();
        let expected_reserved = InitialIncentivePool::get() - expected_released;

        assert_eq!(Balances::free_balance(pool_account()), expected_released);
        assert_eq!(Balances::reserved_balance(pool_account()), expected_reserved);
        assert_eq!(IncentivePoolReleased::<Test>::get(), expected_released);
        assert_eq!(IncentivePoolUsed::<Test>::get(), 0);
        assert_eq!(IncentivePoolReserved::<Test>::get(), expected_reserved);
    });
}

#[test]
fn first_create_reward_transfers_from_released_pool_once() {
    new_test_ext(InitialIncentivePool::get()).execute_with(|| {
        let id = asset_id(0x11);

        assert_ok!(Incentive::distribute_first_create_reward(&ALICE, &id));

        assert_eq!(Balances::free_balance(ALICE), 110);
        assert_eq!(
            Balances::free_balance(pool_account()),
            (DynamicReleaseRatio::get() * InitialIncentivePool::get()) - FirstCreateReward::get(),
        );
        assert_eq!(IncentivePoolUsed::<Test>::get(), FirstCreateReward::get());
        assert!(HasFirstCreateReward::<Test>::get(ALICE));
        System::assert_has_event(RuntimeEvent::Incentive(Event::FirstCreateRewardDistributed {
            recipient: ALICE,
            amount: FirstCreateReward::get(),
            asset_id: id,
            pool_account: pool_account(),
        }));
    });
}

#[test]
fn duplicate_first_create_reward_is_rejected_without_accounting_changes() {
    new_test_ext(InitialIncentivePool::get()).execute_with(|| {
        let first = asset_id(0x22);
        let duplicate = asset_id(0x33);
        assert_ok!(Incentive::distribute_first_create_reward(&ALICE, &first));

        assert_noop!(
            Incentive::distribute_first_create_reward(&ALICE, &duplicate),
            Error::<Test>::FirstCreateRewardAlreadyClaimed,
        );

        assert_eq!(Balances::free_balance(ALICE), 110);
        assert_eq!(IncentivePoolUsed::<Test>::get(), FirstCreateReward::get());
    });
}

#[test]
fn first_create_reward_fails_when_released_pool_is_insufficient() {
    new_test_ext(500).execute_with(|| {
        let id = asset_id(0x44);

        assert_noop!(
            Incentive::distribute_first_create_reward(&ALICE, &id),
            Error::<Test>::InsufficientIncentivePoolBalance,
        );

        assert_eq!(Balances::free_balance(ALICE), 10);
        assert_eq!(IncentivePoolUsed::<Test>::get(), 0);
        assert!(!HasFirstCreateReward::<Test>::get(ALICE));
    });
}

#[test]
fn first_create_reward_keeps_pool_account_alive() {
    new_test_ext(1_000).execute_with(|| {
        let id = asset_id(0x55);

        assert_noop!(
            Incentive::distribute_first_create_reward(&ALICE, &id),
            Error::<Test>::InsufficientIncentivePoolBalance,
        );

        assert_eq!(Balances::free_balance(ALICE), 10);
        assert_eq!(Balances::free_balance(pool_account()), FirstCreateReward::get());
        assert_eq!(IncentivePoolUsed::<Test>::get(), 0);
        assert!(!HasFirstCreateReward::<Test>::get(ALICE));
    });
}
