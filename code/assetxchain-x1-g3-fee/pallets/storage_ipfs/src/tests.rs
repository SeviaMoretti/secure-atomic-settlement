use crate::{
    mock::*, types::StorageStatus, AssetStorageBinds, Error, Event, Providers, StorageOrders,
    StorageProofs,
};
use frame_support::{assert_noop, assert_ok};
use pallet_collaterals::{CollateralData, CollateralRole};
use sp_core::H256;

const ASSET_ID: [u8; 32] = [7; 32];

fn register_provider() {
    assert_ok!(StorageIpfs::register_provider(
        RuntimeOrigin::signed(BOB),
        b"/ip4/127.0.0.1/tcp/4001".to_vec(),
        1024,
        MinIpfsProviderCollateral::get(),
    ));
}

fn create_order() {
    assert_ok!(StorageIpfs::create_storage_order(
        RuntimeOrigin::signed(ALICE),
        ASSET_ID,
        b"bafybeidata".to_vec(),
        2048,
        42,
        100,
    ));
}

fn bind_provider() {
    assert_ok!(StorageIpfs::bind_asset_storage(
        RuntimeOrigin::signed(ALICE),
        ASSET_ID,
        BOB,
    ));
}

#[test]
fn register_provider_reserves_ipfs_provider_pledge_and_stores_provider() {
    new_test_ext().execute_with(|| {
        register_provider();

        let provider = Providers::<Test>::get(BOB).expect("provider should be stored");
        assert_eq!(
            provider.endpoint.to_vec(),
            b"/ip4/127.0.0.1/tcp/4001".to_vec()
        );
        assert_eq!(provider.capacity, 1024);
        assert_eq!(provider.pledged_amount, MinIpfsProviderCollateral::get());
        assert_eq!(provider.registered_at, 1);
        assert!(provider.is_active);

        let collateral = CollateralData::<Test>::get(BOB, CollateralRole::IpfsProvider);
        assert_eq!(collateral.amount, MinIpfsProviderCollateral::get());
        assert_eq!(
            Balances::reserved_balance(BOB),
            MinIpfsProviderCollateral::get()
        );

        System::assert_has_event(RuntimeEvent::Collaterals(
            pallet_collaterals::Event::Pledged {
                who: BOB,
                role: CollateralRole::IpfsProvider,
                amount: MinIpfsProviderCollateral::get(),
            },
        ));
        System::assert_has_event(RuntimeEvent::StorageIpfs(Event::ProviderRegistered {
            who: BOB,
            endpoint: b"/ip4/127.0.0.1/tcp/4001".to_vec(),
        }));
    });
}

#[test]
fn register_provider_rejects_pledge_below_ipfs_provider_minimum() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            StorageIpfs::register_provider(
                RuntimeOrigin::signed(BOB),
                b"/ip4/127.0.0.1/tcp/4001".to_vec(),
                1024,
                MinIpfsProviderCollateral::get() - 1,
            ),
            pallet_collaterals::Error::<Test>::InsufficientCollateralAmount,
        );

        assert!(!Providers::<Test>::contains_key(BOB));
        assert_eq!(Balances::reserved_balance(BOB), 0);
    });
}

#[test]
fn asset_owner_can_create_order_and_bind_registered_provider() {
    new_test_ext().execute_with(|| {
        register_provider();
        create_order();
        bind_provider();

        let order = StorageOrders::<Test>::get(ASSET_ID).expect("order should be stored");
        assert_eq!(order.cid.to_vec(), b"bafybeidata".to_vec());
        assert_eq!(order.size, 2048);
        assert_eq!(order.status, StorageStatus::Active);
        assert_eq!(order.paid_fee, 42);
        assert_eq!(order.ordered_at, 1);
        assert_eq!(order.valid_until, 100);

        let binding = AssetStorageBinds::<Test>::get(ASSET_ID).expect("binding should be stored");
        assert_eq!(binding.provider_id, BOB);
        assert_eq!(binding.storage_fund, 42);
        assert_eq!(binding.storage_account, BOB);
        assert!(!binding.is_weak);
    });
}

#[test]
fn non_owner_cannot_create_order_or_bind_provider() {
    new_test_ext().execute_with(|| {
        register_provider();

        assert_noop!(
            StorageIpfs::create_storage_order(
                RuntimeOrigin::signed(BOB),
                ASSET_ID,
                b"bafybeidata".to_vec(),
                2048,
                42,
                100,
            ),
            Error::<Test>::NotAssetOwner,
        );

        create_order();
        assert_noop!(
            StorageIpfs::bind_asset_storage(RuntimeOrigin::signed(BOB), ASSET_ID, BOB),
            Error::<Test>::NotAssetOwner,
        );
    });
}

#[test]
fn binding_requires_registered_provider_and_existing_order() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            StorageIpfs::bind_asset_storage(RuntimeOrigin::signed(ALICE), ASSET_ID, BOB),
            Error::<Test>::NotAProvider,
        );

        register_provider();
        assert_noop!(
            StorageIpfs::bind_asset_storage(RuntimeOrigin::signed(ALICE), ASSET_ID, BOB),
            Error::<Test>::StorageOrderNotFound,
        );
    });
}

#[test]
fn bound_provider_can_submit_storage_proof() {
    new_test_ext().execute_with(|| {
        register_provider();
        create_order();
        bind_provider();
        System::set_block_number(5);

        let proof_hash = H256::repeat_byte(0x44);
        assert_ok!(StorageIpfs::submit_storage_proof(
            RuntimeOrigin::signed(BOB),
            ASSET_ID,
            proof_hash,
        ));

        let proof = StorageProofs::<Test>::get(ASSET_ID, BOB).expect("proof should be stored");
        assert_eq!(proof.last_proof_block, 5);
        assert_eq!(proof.proof_hash, proof_hash);

        System::assert_has_event(RuntimeEvent::StorageIpfs(Event::ProofSubmitted {
            asset_id: ASSET_ID,
            provider: BOB,
        }));
    });
}

#[test]
fn unbound_or_unregistered_provider_cannot_submit_storage_proof() {
    new_test_ext().execute_with(|| {
        assert_noop!(
            StorageIpfs::submit_storage_proof(
                RuntimeOrigin::signed(BOB),
                ASSET_ID,
                H256::repeat_byte(0x44),
            ),
            Error::<Test>::NotAProvider,
        );

        register_provider();
        assert_noop!(
            StorageIpfs::submit_storage_proof(
                RuntimeOrigin::signed(BOB),
                ASSET_ID,
                H256::repeat_byte(0x44),
            ),
            Error::<Test>::StorageBindingNotFound,
        );
    });
}

#[test]
fn asset_lookup_failure_blocks_order_creation() {
    new_test_ext().execute_with(|| {
        set_asset_owner(None);

        assert_noop!(
            StorageIpfs::create_storage_order(
                RuntimeOrigin::signed(ALICE),
                ASSET_ID,
                b"bafybeidata".to_vec(),
                2048,
                42,
                100,
            ),
            Error::<Test>::AssetNotRegistered,
        );
    });
}
