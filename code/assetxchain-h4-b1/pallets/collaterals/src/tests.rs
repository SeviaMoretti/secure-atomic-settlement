use crate::{
    mock::*,
    CollateralData, CollateralRole, Error, Event, SlashType,
};
use frame_support::{assert_noop, assert_ok};

#[test]
fn pledge_reserves_balance_and_records_role_collateral() {
    new_test_ext().execute_with(|| {
        assert_ok!(Collaterals::pledge(
            RuntimeOrigin::signed(ALICE),
            CollateralRole::MarketOperator,
            MinMarketOperatorCollateral::get(),
        ));

        let info = CollateralData::<Test>::get(ALICE, CollateralRole::MarketOperator);
        assert_eq!(info.amount, MinMarketOperatorCollateral::get());
        assert_eq!(info.start_block, 1);
        assert_eq!(Balances::reserved_balance(ALICE), MinMarketOperatorCollateral::get());
        System::assert_has_event(RuntimeEvent::Collaterals(Event::Pledged {
            who: ALICE,
            role: CollateralRole::MarketOperator,
            amount: MinMarketOperatorCollateral::get(),
        }));
    });
}

#[test]
fn pledge_rejects_role_amount_below_minimum() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            Collaterals::pledge(
                RuntimeOrigin::signed(ALICE),
                CollateralRole::MarketOperator,
                MinMarketOperatorCollateral::get() - 1,
            ),
            Error::<Test>::InsufficientCollateralAmount,
        );

        assert_eq!(Balances::reserved_balance(ALICE), 0);
    });
}

#[test]
fn unbond_releases_collateral_after_role_lock_period() {
    new_test_ext().execute_with(|| {
        assert_ok!(Collaterals::pledge(
            RuntimeOrigin::signed(ALICE),
            CollateralRole::IpfsProvider,
            MinIpfsProviderCollateral::get(),
        ));

        assert_noop!(
            Collaterals::unbond(RuntimeOrigin::signed(ALICE), CollateralRole::IpfsProvider),
            Error::<Test>::CollateralNotReadyForRelease,
        );

        System::set_block_number(7 * 24 * 60 + 2);
        assert_ok!(Collaterals::unbond(
            RuntimeOrigin::signed(ALICE),
            CollateralRole::IpfsProvider,
        ));

        assert_eq!(Balances::reserved_balance(ALICE), 0);
        assert!(!CollateralData::<Test>::contains_key(ALICE, CollateralRole::IpfsProvider));
        System::assert_has_event(RuntimeEvent::Collaterals(Event::Unbonded {
            who: ALICE,
            role: CollateralRole::IpfsProvider,
            amount: MinIpfsProviderCollateral::get(),
        }));
    });
}

#[test]
fn slash_distribution_is_deterministic_for_light_violation() {
    new_test_ext().execute_with(|| {
        assert_ok!(Collaterals::pledge(
            RuntimeOrigin::signed(ALICE),
            CollateralRole::GovernancePledge,
            MinGovernancePledge::get(),
        ));

        assert_ok!(Collaterals::slash_and_distribute(
            &ALICE,
            CollateralRole::GovernancePledge,
            1_000,
            SlashType::LightViolation,
        ));

        let info = CollateralData::<Test>::get(ALICE, CollateralRole::GovernancePledge);
        assert_eq!(info.amount, MinGovernancePledge::get() - 1_000);
        assert_eq!(Balances::reserved_balance(ALICE), MinGovernancePledge::get() - 1_000);
        assert_eq!(Balances::free_balance(DESTRUCTION_POOL), 300);
        assert_eq!(Balances::free_balance(INCENTIVE_POOL), 700);
        assert_eq!(Balances::free_balance(COMPENSATION_POOL), 0);
        assert_eq!(Balances::free_balance(IPFS_POOL), 0);
        System::assert_has_event(RuntimeEvent::Collaterals(Event::SlashedAndDistributed {
            who: ALICE,
            role: CollateralRole::GovernancePledge,
            slashed_amount: 1_000,
            burn_amount: 300,
            incentive_amount: 700,
        }));
    });
}
