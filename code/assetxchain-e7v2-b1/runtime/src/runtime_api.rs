use codec::Codec;
use sp_api::decl_runtime_apis;
use sp_core::H256;

decl_runtime_apis! {
    pub trait DataAssetsApi<AccountId> where
        AccountId: Codec,
    {
        fn get_asset(asset_id: [u8; 32]) -> Option<pallet_dataassets::types::DataAsset<AccountId>>;
        fn get_asset_by_token_id(token_id: u32) -> Option<pallet_dataassets::types::DataAsset<AccountId>>;
        fn get_certificate(asset_id: [u8; 32], cert_id: [u8; 32]) -> Option<pallet_dataassets::types::RightToken<AccountId>>;
        // fn get_asset_certificates(asset_id: [u8; 32]) -> Vec<pallet_dataassets::types::RightToken<AccountId>>;
        fn get_asset_root() -> H256;
    }
}