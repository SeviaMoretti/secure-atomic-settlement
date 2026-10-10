use crate::{mock::*, Event, TotalTokensMined};
use frame_support::{
    assert_ok,
    traits::Hooks,
};

#[test]
fn finalize_pays_initial_reward_before_threshold() {
    new_test_ext().execute_with(|| {
        Rewards::on_finalize(1);

        assert_eq!(TotalTokensMined::<Test>::get(), InitialReward::get());
        assert_eq!(Balances::free_balance(RewardReceiverAccount::get()), InitialReward::get());
        System::assert_has_event(RuntimeEvent::Rewards(Event::RewardPaid {
            who: RewardReceiverAccount::get(),
            amount: InitialReward::get(),
            block_number: 1,
        }));
    });
}

#[test]
fn finalize_switches_to_adjusted_reward_at_threshold() {
    new_test_ext().execute_with(|| {
        TotalTokensMined::<Test>::put(RewardAdjustmentThreshold::get() - InitialReward::get());

        Rewards::on_finalize(7);
        Rewards::on_finalize(8);

        assert_eq!(
            TotalTokensMined::<Test>::get(),
            RewardAdjustmentThreshold::get() + AdjustedReward::get(),
        );
        assert_eq!(
            Balances::free_balance(RewardReceiverAccount::get()),
            InitialReward::get() + AdjustedReward::get(),
        );
        System::assert_has_event(RuntimeEvent::Rewards(Event::RewardAdjusted {
            new_amount: AdjustedReward::get(),
            block_number: 7,
        }));
        System::assert_has_event(RuntimeEvent::Rewards(Event::RewardPaid {
            who: RewardReceiverAccount::get(),
            amount: AdjustedReward::get(),
            block_number: 8,
        }));
    });
}

#[test]
fn finalize_caps_last_reward_and_stops_at_max_supply() {
    new_test_ext().execute_with(|| {
        TotalTokensMined::<Test>::put(MaxSupply::get() - 1);

        Rewards::on_finalize(10);
        Rewards::on_finalize(11);

        assert_eq!(TotalTokensMined::<Test>::get(), MaxSupply::get());
        assert_eq!(Balances::free_balance(RewardReceiverAccount::get()), 1);
        System::assert_has_event(RuntimeEvent::Rewards(Event::RewardPaid {
            who: RewardReceiverAccount::get(),
            amount: 1,
            block_number: 10,
        }));
        assert_eq!(
            System::events()
                .iter()
                .filter(|event| matches!(
                    event.event,
                    RuntimeEvent::Rewards(Event::RewardPaid { block_number: 11, .. })
                ))
                .count(),
            0,
        );
    });
}

#[test]
fn current_reward_query_reports_adjusted_reward_after_threshold() {
    new_test_ext().execute_with(|| {
        TotalTokensMined::<Test>::put(RewardAdjustmentThreshold::get());

        assert_ok!(Rewards::get_current_reward(RuntimeOrigin::signed(77)));

        System::assert_has_event(RuntimeEvent::Rewards(Event::CurrentRewardQueried {
            who: 77,
            amount: AdjustedReward::get(),
        }));
    });
}
