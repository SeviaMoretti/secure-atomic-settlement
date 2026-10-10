use crate::Runtime;
use pallet_market_runtime::{DataAssetsAccess, AssetInfo, CertificateInfo};
use sp_runtime::{traits::Zero, DispatchError};

type Account = <Runtime as frame_system::Config>::AccountId;
type BlockNum = frame_system::pallet_prelude::BlockNumberFor<Runtime>;
type Balance = pallet_market_runtime::BalanceOf<Runtime>;

pub struct MarketRuntimeAdapter;

impl DataAssetsAccess<Account, Balance, BlockNum> for MarketRuntimeAdapter
{
    fn get_asset(asset_id: &[u8; 32]) -> Option<AssetInfo<Account, BlockNum>> {
        pallet_dataassets::Pallet::<Runtime>::get_asset(asset_id).map(|a| {
            let locked = a.is_locked();
            let owner = a.core.owner;
            AssetInfo { owner, is_locked: locked, created_at: Zero::zero() }
        })
    }

    fn get_certificate(
        asset_id: &[u8; 32],
        certificate_id: &[u8; 32],
    ) -> Option<CertificateInfo<Account, BlockNum>> {
        pallet_dataassets::Pallet::<Runtime>::get_certificate(asset_id, certificate_id).map(|c| {
            let now = pallet_timestamp::Pallet::<Runtime>::get();
            let active = c.is_valid(now);
            let rt = c.right_type as u8;
            CertificateInfo {
                owner: c.owner,
                issuer: c.issuer,
                right_type: rt,
                issued_at: Zero::zero(),
                valid_until: c.valid_until,
                active,
            }
        })
    }

    fn get_approval(asset_id: &[u8; 32]) -> Option<Account> {
        pallet_dataassets::AssetApprovals::<Runtime>::get(asset_id)
    }

    fn do_transfer_asset(
        asset_id: &[u8; 32],
        market_account: &Account,
        new_owner: &Account,
    ) -> Result<Account, DispatchError> {
        pallet_dataassets::Pallet::<Runtime>::do_transfer_asset(asset_id, market_account, new_owner)
    }

    fn do_transfer_certificate(
        asset_id: &[u8; 32],
        certificate_id: &[u8; 32],
        current_owner: &Account,
        new_holder: &Account,
    ) -> Result<Account, DispatchError> {
        pallet_dataassets::Pallet::<Runtime>::do_transfer_certificate(
            asset_id, certificate_id, current_owner, new_holder,
        )
    }

    fn issue_certificate_internal(
        asset_id: &[u8; 32],
        _certificate_id: &[u8; 32],
        _asset_data: &AssetInfo<Account, BlockNum>,
        holder: &Account,
        right_type: u8,
        valid_until: Option<u64>,
    ) -> Result<(), DispatchError> {
        let asset = pallet_dataassets::Pallet::<Runtime>::get_asset(asset_id)
            .ok_or(pallet_dataassets::Error::<Runtime>::AssetNotFound)?;
        pallet_dataassets::Pallet::<Runtime>::issue_certificate_internal(
            asset_id, &asset, holder, right_type, valid_until,
        ).map(|_| ())
    }

    fn compute_state_root() -> sp_core::H256 {
        pallet_dataassets::Pallet::<Runtime>::compute_asset_root()
    }

    fn current_timestamp() -> u64 {
        pallet_timestamp::Pallet::<Runtime>::get()
    }
}